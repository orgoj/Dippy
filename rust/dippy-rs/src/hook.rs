//! Port of the Claude Code hook path of `dippy.dippy.main` (`--claude`).
//!
//! Supported: Bash `PreToolUse` (classification), `PostToolUse` `after`
//! rules, permission bypass modes, `Stop`/`Notification` events, invalid
//! JSON. Not ported, and therefore failing closed with `ask`: MCP, web and
//! file-tool rule matching, other agents' payloads (`toolCall`, Cursor).
//! Notifier programs are never run.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::analyzer::{self, Action};
use crate::config::{self, Config};
use crate::parser::tokenize;
use crate::paths;

const SHELL_TOOL_NAMES: [&str; 9] = [
    "Bash",
    "Shell",
    "bash",
    "exec",
    "shell",
    "run_shell",
    "run_shell_command",
    "execute_shell",
    "run_command",
];

const FILE_TOOL_NAMES: [&str; 21] = [
    "Write",
    "Edit",
    "MultiEdit",
    "Read",
    "LS",
    "Glob",
    "Grep",
    "Search",
    "write",
    "edit",
    "read",
    "write_file",
    "replace",
    "read_file",
    "read_many_files",
    "view_file",
    "write_to_file",
    "replace_file_content",
    "grep_search",
    "find_by_name",
    "list_dir",
];

const WEB_TOOL_NAMES: [&str; 4] = ["WebSearch", "WebFetch", "google_web_search", "web_fetch"];

/// Python `json.dumps` with default separators and ASCII escaping.
pub fn py_dumps(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => py_json_str(s),
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(py_dumps).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", py_json_str(k), py_dumps(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python `json.dumps` string encoding (ASCII only, `\uXXXX` escapes).
pub fn py_json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn pre_tool(decision: &str, reason: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": format!("🐤 {reason}"),
        }
    })
}

fn approve(reason: &str) -> Value {
    pre_tool("allow", reason)
}

fn ask(reason: &str) -> Value {
    pre_tool("ask", reason)
}

fn deny(reason: &str) -> Value {
    pre_tool("deny", reason)
}

fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Hook output for one stdin payload, or `None` when Python prints nothing.
pub fn claude_hook(stdin: &str) -> Option<String> {
    let Ok(input) = serde_json::from_str::<Value>(stdin) else {
        return Some("{}".into());
    };
    let Some(obj) = input.as_object() else {
        return Some("{}".into());
    };
    Some(py_dumps(&match handle(obj) {
        Some(v) => v,
        None => return None,
    }))
}

fn handle(input: &Map<String, Value>) -> Option<Value> {
    let input_v = Value::Object(input.clone());
    let empty = Value::Object(Map::new());
    let tool_input = input
        .get("tool_input")
        .filter(|v| v.is_object())
        .unwrap_or(&empty);
    let cwd_str = get_str(&input_v, "cwd").or_else(|| get_str(tool_input, "cwd"));
    let cwd = match cwd_str {
        Some(c) => paths::resolve(&PathBuf::from(c)),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
    };
    let config = match config::load_config(&cwd, None, None) {
        Ok(c) => c,
        Err(e) => return Some(ask(&format!("config error: {e}"))),
    };
    let hook_event = get_str(&input_v, "hook_event_name")
        .or_else(|| get_str(&input_v, "hookEventName"))
        .map(str::to_string)
        .unwrap_or_else(|| {
            if input.contains_key("terminationReason") || input.contains_key("fullyIdle") {
                "Stop".into()
            } else if input.contains_key("toolResponse")
                || (input.contains_key("toolCall") && input.contains_key("error"))
            {
                "PostToolUse".into()
            } else {
                "PreToolUse".into()
            }
        });
    if hook_event == "Notification" {
        return Some(json!({}));
    }
    if matches!(hook_event.as_str(), "Stop" | "SubagentStop" | "AfterAgent") {
        return Some(json!({"decision": "approve", "continue": true}));
    }
    let hook_event = match hook_event.as_str() {
        "BeforeTool" => "PreToolUse".to_string(),
        "AfterTool" => "PostToolUse".to_string(),
        _ => hook_event,
    };
    let post = hook_event == "PostToolUse";
    if input.contains_key("toolCall") {
        return (!post).then(|| ask("dippy-rs: toolCall payloads are not supported"));
    }
    let tool_name = input.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let permission_mode = input
        .get("permission_mode")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let bypass = matches!(permission_mode, "bypassPermissions" | "dontAsk");

    if tool_name.starts_with("mcp__") || WEB_TOOL_NAMES.contains(&tool_name) {
        if post {
            return None;
        }
        if bypass {
            return Some(approve(permission_mode));
        }
        return Some(ask(&format!(
            "dippy-rs: rules for {tool_name} are not ported"
        )));
    }
    if FILE_TOOL_NAMES.contains(&tool_name) {
        if post {
            return Some(json!({}));
        }
        let has_path = ["file_path", "path", "filepath"]
            .iter()
            .any(|k| get_str(tool_input, k).is_some());
        if has_path && (bypass || permission_mode == "acceptEdits") {
            return Some(approve(permission_mode));
        }
        if !has_path
            && tool_input
                .get("paths")
                .and_then(Value::as_array)
                .is_none_or(|a| a.is_empty())
        {
            return Some(json!({}));
        }
        return Some(ask(&format!(
            "dippy-rs: rules for {tool_name} are not ported"
        )));
    }
    if !SHELL_TOOL_NAMES.contains(&tool_name) {
        return Some(json!({}));
    }
    let command = get_str(tool_input, "command")
        .or_else(|| get_str(tool_input, "cmd"))
        .unwrap_or("");
    if !post && bypass {
        return Some(approve(permission_mode));
    }
    if post {
        return post_tool_use(command, &config, &cwd);
    }
    let result = analyzer::analyze(command, &config, &cwd, None, false);
    Some(match result.action {
        Action::Allow => approve(&result.reason),
        Action::Deny => deny(&result.reason),
        Action::Ask => ask(&result.reason),
    })
}

fn post_tool_use(command: &str, config: &Config, cwd: &Path) -> Option<Value> {
    let words = tokenize(command, false);
    let message = config::match_after(&words, config, cwd).filter(|m| !m.is_empty())?;
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": format!("🐤 {message}"),
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumps_like_python() {
        let v = json!({"a": "🐤 x", "b": [1, true], "c": {}});
        assert_eq!(py_dumps(&v), "{\"a\": \"\\ud83d\\udc24 x\", \"b\": [1, true], \"c\": {}}");
    }

    #[test]
    fn invalid_json_passes() {
        assert_eq!(claude_hook("not json").as_deref(), Some("{}"));
    }
}
