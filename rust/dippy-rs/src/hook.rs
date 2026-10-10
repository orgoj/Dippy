//! Port of the Claude Code hook path of `dippy.dippy.main` (`--claude`).
//!
//! Supported: Bash `PreToolUse` (classification), MCP, web and file-tool
//! rules, `PostToolUse` `after`/`after-mcp`/`after-web` rules, permission
//! bypass modes, `Stop`/`Notification` events, invalid JSON. Not ported, and
//! therefore failing closed with `ask`: other agents' payloads (`toolCall`,
//! Cursor). Notifier programs are never run.

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
    Some(py_dumps(&handle(obj)?))
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
    dispatch(input, &config, &cwd)
}

fn dispatch(input: &Map<String, Value>, config: &Config, cwd: &Path) -> Option<Value> {
    let input_v = Value::Object(input.clone());
    let empty = Value::Object(Map::new());
    let tool_input = input
        .get("tool_input")
        .filter(|v| v.is_object())
        .unwrap_or(&empty);
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

    let is_mcp = tool_name.starts_with("mcp__");
    if is_mcp || WEB_TOOL_NAMES.contains(&tool_name) {
        let value = if is_mcp {
            tool_name
        } else {
            // WebSearch/google_web_search use query, WebFetch/web_fetch use url.
            ["query", "url", "q"]
                .iter()
                .find_map(|k| get_str(tool_input, k))
                .unwrap_or("")
        };
        if post {
            let message = if is_mcp {
                config::match_after_mcp(value, config)
            } else {
                config::match_after_web(value, config)
            };
            return message.filter(|m| !m.is_empty()).map(|m| post_response(&m));
        }
        if bypass {
            return Some(approve(permission_mode));
        }
        let found = if is_mcp {
            config::match_mcp(value, config)
        } else {
            config::match_web(value, config, &config::env_context_flags(config))
        };
        return Some(found.map_or(json!({}), |m| rule_response(&m.decision, &rule_reason(&m))));
    }
    if FILE_TOOL_NAMES.contains(&tool_name) {
        return file_tool(tool_name, tool_input, permission_mode, post, config, cwd);
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
        return post_tool_use(command, config, cwd);
    }
    let result = analyzer::analyze(command, config, cwd, None, false);
    Some(match result.action {
        Action::Allow => approve(&result.reason),
        Action::Deny => deny(&result.reason),
        Action::Ask => ask(&result.reason),
    })
}

/// Read-only tools in `check_file_tool`.
const READ_TOOL_NAMES: [&str; 10] = [
    "Read",
    "read_file",
    "view_file",
    "LS",
    "Glob",
    "Grep",
    "Search",
    "grep_search",
    "find_by_name",
    "list_dir",
];

/// Read-only tools in the multi-file `paths` branch (a different list in Python).
const MULTI_READ_TOOL_NAMES: [&str; 8] = [
    "Read",
    "read_file",
    "read",
    "read_many_files",
    "LS",
    "Glob",
    "Grep",
    "Search",
];

/// `_rule_reason`: the rule's own message, else the pattern and its source.
fn rule_reason(m: &config::Match) -> String {
    match (&m.message, &m.source) {
        (Some(msg), _) if !msg.is_empty() => msg.clone(),
        (_, Some(source)) => format!("[{} @ {source}]", m.pattern),
        _ => format!("[{}]", m.pattern),
    }
}

fn rule_response(decision: &str, reason: &str) -> Value {
    match decision {
        "allow" => approve(reason),
        "deny" => deny(reason),
        _ => ask(reason),
    }
}

/// File tools (Claude `tool_input`): one path, or the strictest of `paths`.
fn file_tool(
    tool_name: &str,
    tool_input: &Value,
    permission_mode: &str,
    post: bool,
    config: &Config,
    cwd: &Path,
) -> Option<Value> {
    let file_path = ["file_path", "path", "filepath"]
        .iter()
        .find_map(|k| get_str(tool_input, k));
    if post {
        return Some(json!({}));
    }
    let Some(file_path) = file_path else {
        let paths = tool_input.get("paths").and_then(Value::as_array);
        let Some(paths) = paths.filter(|p| !p.is_empty()) else {
            return Some(json!({}));
        };
        return Some(multi_file(tool_name, paths, config, cwd));
    };
    if matches!(
        permission_mode,
        "bypassPermissions" | "dontAsk" | "acceptEdits"
    ) {
        return Some(approve(permission_mode));
    }
    let active = config::env_context_flags(config);
    let found = if READ_TOOL_NAMES.contains(&tool_name) {
        config::match_read(file_path, config, cwd, &active)
    } else {
        config::match_edit(file_path, config, cwd, &active)
    };
    Some(found.map_or(json!({}), |m| rule_response(&m.decision, &rule_reason(&m))))
}

/// Multi-file branch: Python matches without context flags and reports the
/// pattern without its source.
fn multi_file(tool_name: &str, paths: &[Value], config: &Config, cwd: &Path) -> Value {
    const ORDER: [&str; 4] = ["deny", "ask", "allow", "pass"];
    let rank = |d: &str| ORDER.iter().position(|o| *o == d).unwrap_or(0);
    let read = MULTI_READ_TOOL_NAMES.contains(&tool_name);
    let mut strictest: Option<config::Match> = None;
    for path in paths {
        let Some(path) = path.as_str() else {
            return ask("dippy-rs: non-string entry in paths");
        };
        let found = if read {
            config::match_read(path, config, cwd, &config::Flags::new())
        } else {
            config::match_edit(path, config, cwd, &config::Flags::new())
        };
        if let Some(m) = found
            && strictest
                .as_ref()
                .is_none_or(|s| rank(&m.decision) < rank(&s.decision))
        {
            strictest = Some(m);
        }
    }
    let Some(m) = strictest else {
        return json!({});
    };
    let reason = match &m.message {
        Some(msg) if !msg.is_empty() => msg.clone(),
        _ => format!("[{}]", m.pattern),
    };
    rule_response(&m.decision, &reason)
}

fn post_response(message: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": format!("🐤 {message}"),
        }
    })
}

fn post_tool_use(command: &str, config: &Config, cwd: &Path) -> Option<Value> {
    let words = tokenize(command, false);
    let message = config::match_after(&words, config, cwd).filter(|m| !m.is_empty())?;
    Some(post_response(&message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumps_like_python() {
        let v = json!({"a": "🐤 x", "b": [1, true], "c": {}});
        assert_eq!(
            py_dumps(&v),
            "{\"a\": \"\\ud83d\\udc24 x\", \"b\": [1, true], \"c\": {}}"
        );
    }

    #[test]
    fn invalid_json_passes() {
        assert_eq!(claude_hook("not json").as_deref(), Some("{}"));
    }

    const RULES: &str = "allow-mcp mcp__fake__*\n\
        deny-mcp mcp__fake__drop \"no drop\"\n\
        after-mcp mcp__fake__get \"got it\"\n\
        allow-web *rust*\n\
        deny-web *secret*\n\
        after-web *rust* \"read docs\"\n\
        allow-edit /w/out/**\n\
        deny-edit /w/out/keep \"keep it\"\n\
        allow-read /w/**\n\
        ask-read /w/private/*\n";

    fn run(payload: Value) -> Option<String> {
        let mut config = config::parse_config(RULES, None).unwrap();
        config = config::tag_rules(config, "/w/.dippy", "project");
        let input = payload.as_object().unwrap().clone();
        dispatch(&input, &config, Path::new("/w")).map(|v| py_dumps(&v))
    }

    fn decision(payload: Value) -> String {
        let out = run(payload).unwrap_or_default();
        let v: Value = serde_json::from_str(&out).unwrap_or(json!({}));
        v.pointer("/hookSpecificOutput/permissionDecision")
            .and_then(Value::as_str)
            .unwrap_or("none")
            .to_string()
    }

    #[test]
    fn mcp_rules_decide() {
        assert_eq!(decision(json!({"tool_name": "mcp__fake__get"})), "allow");
        assert_eq!(decision(json!({"tool_name": "mcp__fake__drop"})), "deny");
        assert_eq!(
            run(json!({"tool_name": "mcp__other__x"})).as_deref(),
            Some("{}")
        );
        let out = run(json!({"tool_name": "mcp__fake__drop"})).unwrap();
        assert!(out.contains("no drop"), "{out}");
        let out = run(json!({"tool_name": "mcp__fake__get"})).unwrap();
        assert!(out.contains("[mcp__fake__* @ /w/.dippy]"), "{out}");
    }

    #[test]
    fn mcp_and_web_post_tool_use() {
        let post = |tool: &str, input: Value| {
            run(json!({"hook_event_name": "PostToolUse", "tool_name": tool, "tool_input": input}))
        };
        assert!(
            post("mcp__fake__get", json!({}))
                .unwrap()
                .contains("got it")
        );
        assert_eq!(post("mcp__fake__x", json!({})), None);
        let out = post("WebSearch", json!({"query": "rust book"})).unwrap();
        assert!(out.contains("read docs"), "{out}");
        assert_eq!(post("WebSearch", json!({"query": "cats"})), None);
    }

    #[test]
    fn web_rules_use_query_url_or_q() {
        let web =
            |tool: &str, input: Value| decision(json!({"tool_name": tool, "tool_input": input}));
        assert_eq!(web("WebSearch", json!({"query": "rust book"})), "allow");
        assert_eq!(web("WebFetch", json!({"url": "https://x/secret"})), "deny");
        assert_eq!(web("google_web_search", json!({"q": "rust"})), "allow");
        assert_eq!(web("WebSearch", json!({"query": "cats"})), "none");
    }

    #[test]
    fn file_rules_split_read_and_edit() {
        let file = |tool: &str, path: &str| {
            decision(json!({"tool_name": tool, "tool_input": {"file_path": path}}))
        };
        assert_eq!(file("Write", "/w/out/a.txt"), "allow");
        assert_eq!(file("Edit", "/w/out/keep"), "deny");
        assert_eq!(file("Write", "/w/src/a.rs"), "none");
        assert_eq!(file("Read", "/w/src/a.rs"), "allow");
        assert_eq!(file("Read", "/w/private/k"), "ask");
        assert_eq!(file("Grep", "/elsewhere/x"), "none");
    }

    #[test]
    fn multi_file_takes_strictest() {
        let paths = |tool: &str, paths: Value| {
            decision(json!({"tool_name": tool, "tool_input": {"paths": paths}}))
        };
        assert_eq!(paths("read_many_files", json!(["/w/a", "/w/b"])), "allow");
        assert_eq!(
            paths("read_many_files", json!(["/w/a", "/w/private/k"])),
            "ask"
        );
        assert_eq!(paths("Write", json!(["/w/out/a", "/w/out/keep"])), "deny");
        assert_eq!(paths("read_many_files", json!(["/x/a"])), "none");
        assert_eq!(paths("read_many_files", json!(["/w/a", 7])), "ask");
    }

    #[test]
    fn bypass_modes_for_tools() {
        let mode = |tool: &str, input: Value, mode: &str| {
            decision(json!({"tool_name": tool, "tool_input": input, "permission_mode": mode}))
        };
        assert_eq!(
            mode("mcp__other__x", json!({}), "bypassPermissions"),
            "allow"
        );
        assert_eq!(mode("mcp__other__x", json!({}), "acceptEdits"), "none");
        assert_eq!(
            mode("WebSearch", json!({"query": "cats"}), "dontAsk"),
            "allow"
        );
        assert_eq!(
            mode("Write", json!({"file_path": "/w/out/keep"}), "acceptEdits"),
            "allow"
        );
    }
}
