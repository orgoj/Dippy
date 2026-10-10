//! Port of `dippy.pi_wrapper`: the pi-mono extension protocol (`dippy --pi`).
//!
//! Input: `{"type": "bash"|"read"|"edit"|"idle", "command", "path", "cwd",
//! "agent"}`. Output: `{"action", "reason", "context_flags", "note",
//! "error"}`, or `{"action": "ask", "reason", "error": true}` on failure.
//! Not ported: the notifier, so `note` is always null. Divergences from
//! Python: invalid JSON reports serde's message, and a non-object payload
//! reports a fixed message instead of a Python exception text.

use std::path::Path;

use serde_json::{Value, json};

use crate::analyzer;
use crate::config::{self, Config, Flags};
use crate::hook::py_dumps;
use crate::logging::{Entry, Logs};
use crate::paths;

const DEFAULT_DENY_FORMAT: &str = "⚠️ DENIED by security policy.\n\nCommand: {command}\n\n{reason}";
/// Built-in template for `pi` and `claude`.
const AGENT_DENY_FORMAT: &str = "⚠️ COMMAND DENIED by security policy.\n\nOriginal: {command}\n\nINSTRUCTION: {reason}\n\nYou MUST follow the instruction above. Do NOT try alternative commands.";

/// Stdout line and exit code for one stdin payload.
pub fn run(stdin: &str) -> (String, i32) {
    let input: Value = match serde_json::from_str(stdin) {
        Ok(v) => v,
        Err(e) => return (failure(&format!("Invalid JSON input: {e}")), 1),
    };
    if !input.is_object() {
        return (failure("Dippy error: hook input is not a JSON object"), 1);
    }
    let cwd = paths::resolve(Path::new(
        input.get("cwd").and_then(Value::as_str).unwrap_or("."),
    ));
    let loaded = config::load_config(&cwd, None, None);
    for warning in config::take_warnings() {
        eprintln!("{warning}");
    }
    let config = match loaded {
        Ok(c) => c,
        Err(e) => return (failure(&format!("Config error: {e}")), 0),
    };
    let agent = input.get("agent").and_then(Value::as_str).unwrap_or("pi");
    let logs = Logs::none(agent);
    logs.configure(&config);
    (py_dumps(&respond(&config, &input, &cwd, &logs)), 0)
}

fn failure(reason: &str) -> String {
    py_dumps(&json!({"action": "ask", "reason": reason, "error": true}))
}

/// Decide, log and format the reply for a parsed payload.
fn respond(config: &Config, input: &Value, cwd: &Path, logs: &Logs) -> Value {
    let text = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or("");
    let req_type = match input.get("type") {
        None => "bash".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    let mut flags = Flags::new();
    let mut suggestion = None;
    let (action, reason) = match req_type.as_str() {
        "bash" if text("command").is_empty() => ("ask".to_string(), "Empty command".to_string()),
        "bash" => {
            let d = analyzer::analyze(text("command"), config, cwd, None, false);
            flags = d.context_flags.unwrap_or_default();
            suggestion = d.suggestion;
            (d.action.as_str().to_string(), d.reason)
        }
        "idle" => ("allow".to_string(), "idle".to_string()),
        kind @ ("edit" | "read") => file_decision(config, kind, text("path"), cwd),
        other => ("ask".to_string(), format!("Unknown request type: {other}")),
    };
    // Python logs an empty command and empty paths too.
    match req_type.as_str() {
        "bash" => logs.decision(Entry {
            decision: &action,
            message: Some(&reason),
            command: Some(text("command")),
            cwd: Some(cwd),
            context_flags: Some(&flags),
            suggestion: suggestion.as_deref(),
            ..Entry::default()
        }),
        kind @ ("edit" | "read") => logs.decision(Entry {
            decision: &action,
            message: Some(&reason),
            tool: Some(if kind == "edit" { "Edit" } else { "Read" }),
            file_path: Some(text("path")),
            cwd: Some(cwd),
            ..Entry::default()
        }),
        _ => {}
    }
    let reason = if action == "deny" {
        let command = (req_type == "bash").then(|| text("command"));
        let agent = input.get("agent").and_then(Value::as_str).unwrap_or("pi");
        format_deny_reason(&reason, command, config, agent)
    } else {
        reason
    };
    json!({
        "action": action,
        "reason": reason,
        "context_flags": flags,
        "note": null,
        "error": false,
    })
}

/// Edit and read rules without context flags, else the config default.
fn file_decision(config: &Config, kind: &str, path: &str, cwd: &Path) -> (String, String) {
    if path.is_empty() {
        return ("ask".into(), format!("Empty path for {kind}"));
    }
    let no_flags = Flags::new();
    let found = if kind == "edit" {
        config::match_edit(path, config, cwd, &no_flags)
    } else {
        config::match_read(path, config, cwd, &no_flags)
    };
    match found {
        Some(m) => {
            let detail = m.message.filter(|s| !s.is_empty()).unwrap_or(m.pattern);
            (m.decision, format!("{kind} {path}: {detail}"))
        }
        None => (config.default.clone(), format!("{kind} {path} (default)")),
    }
}

/// `format_deny_reason`: `deny-format-AGENT`, else `deny-format`, else the
/// built-in template; `{pattern}` is the reason's text before the first `:`.
fn format_deny_reason(reason: &str, command: Option<&str>, config: &Config, agent: &str) -> String {
    let template = match config.deny_format_agents.get(agent) {
        Some(t) => t.as_str(),
        None => match config.deny_format.as_deref() {
            Some(t) if !t.is_empty() => t,
            _ if matches!(agent, "pi" | "claude") => AGENT_DENY_FORMAT,
            _ => DEFAULT_DENY_FORMAT,
        },
    };
    let pattern = if reason.contains(": ") {
        reason.split(':').next().unwrap_or("").trim()
    } else {
        ""
    };
    template
        .replace("{command}", command.unwrap_or(""))
        .replace("{reason}", reason)
        .replace("{pattern}", pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "\
allow frob *
deny zap * \"use frob instead\"
allow-edit /w/src/**
deny-edit /w/.env
ask-read /w/secret/* \"secret file\"
allow-read /w/notes/*
";

    fn config(extra: &str) -> Config {
        config::parse_config(&format!("{RULES}{extra}"), None).unwrap()
    }

    fn reply(config: &Config, input: Value) -> Value {
        respond(config, &input, Path::new("/w"), &Logs::none("pi"))
    }

    fn action_reason(v: &Value) -> (String, String) {
        (
            v["action"].as_str().unwrap().to_string(),
            v["reason"].as_str().unwrap().to_string(),
        )
    }

    fn pair(action: &str, reason: &str) -> (String, String) {
        (action.to_string(), reason.to_string())
    }

    #[test]
    fn bash_uses_the_analyzer_and_defaults_to_bash() {
        let c = config("");
        let v = reply(&c, json!({"command": "frob x"}));
        assert_eq!(
            py_dumps(&v),
            r#"{"action": "allow", "reason": "frob *", "context_flags": [], "note": null, "error": false}"#
        );
        let v = reply(&c, json!({"type": "bash", "command": ""}));
        assert_eq!(action_reason(&v), pair("ask", "Empty command"));
    }

    #[test]
    fn bash_reports_sorted_context_flags() {
        let v = reply(
            &config(""),
            json!({"type": "bash", "command": "frob a | frob b"}),
        );
        assert_eq!(v["action"], "allow");
        assert_eq!(v["context_flags"], json!(["@compound", "@pipeline"]));
    }

    #[test]
    fn edit_and_read_use_path_rules_then_the_default() {
        let c = config("");
        let cases = [
            (
                json!({"type": "edit", "path": "/w/src/a.rs"}),
                pair("allow", "edit /w/src/a.rs: /w/src/**"),
            ),
            (
                json!({"type": "read", "path": "/w/secret/k"}),
                pair("ask", "read /w/secret/k: secret file"),
            ),
            (
                json!({"type": "read", "path": "/w/other"}),
                pair("ask", "read /w/other (default)"),
            ),
            (
                json!({"type": "edit", "path": ""}),
                pair("ask", "Empty path for edit"),
            ),
            (json!({"type": "read"}), pair("ask", "Empty path for read")),
            (json!({"type": "idle"}), pair("allow", "idle")),
            (
                json!({"type": "write"}),
                pair("ask", "Unknown request type: write"),
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                action_reason(&reply(&c, input.clone())),
                expected,
                "{input}"
            );
        }
        let pass = config("set default pass\n");
        let v = reply(&pass, json!({"type": "edit", "path": "/w/x"}));
        assert_eq!(action_reason(&v), pair("pass", "edit /w/x (default)"));
    }

    #[test]
    fn path_rules_with_context_flags_never_match() {
        let c = config("allow-edit [ci] /w/build/*\n");
        let v = reply(&c, json!({"type": "edit", "path": "/w/build/a"}));
        assert_eq!(action_reason(&v), pair("ask", "edit /w/build/a (default)"));
    }

    #[test]
    fn deny_reason_uses_the_agent_then_general_then_builtin_template() {
        let builtin = reply(&config(""), json!({"type": "bash", "command": "zap it"}));
        assert_eq!(
            builtin["reason"],
            "⚠️ COMMAND DENIED by security policy.\n\nOriginal: zap it\n\nINSTRUCTION: zap: use frob instead\n\n\
             You MUST follow the instruction above. Do NOT try alternative commands."
        );
        let general = config("set deny-format \"{pattern}|{command}|{reason}\"\n");
        let v = reply(&general, json!({"type": "edit", "path": "/w/.env"}));
        assert_eq!(v["action"], "deny");
        assert_eq!(v["reason"], "edit /w/.env||edit /w/.env: /w/.env");
        let agent = config("set deny-format G\nset deny-format-codex 'C {reason}'\n");
        let v = reply(
            &agent,
            json!({"type": "bash", "command": "zap", "agent": "codex"}),
        );
        assert_eq!(v["reason"], "C zap: use frob instead");
        let v = reply(&config(""), json!({"command": "zap", "agent": "other"}));
        assert_eq!(
            v["reason"],
            "⚠️ DENIED by security policy.\n\nCommand: zap\n\nzap: use frob instead"
        );
    }

    #[test]
    fn decisions_go_to_the_audit_log_with_the_agent() {
        let dir = std::env::temp_dir().join(format!("dippy-rs-pi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audit = dir.join("audit.log");
        let c = config(&format!("set log {}\nset log-full\n", audit.display()));
        let logs = Logs::none("hermes");
        logs.configure(&c);
        for input in [
            json!({"type": "bash", "command": "frob x"}),
            json!({"type": "bash", "command": ""}),
            json!({"type": "edit", "path": "/w/src/a"}),
            json!({"type": "read", "path": ""}),
            json!({"type": "idle"}),
            json!({"type": "nope"}),
        ] {
            respond(&c, &input, Path::new("/w"), &logs);
        }
        let entries: Vec<Value> = std::fs::read_to_string(&audit)
            .unwrap()
            .lines()
            .map(|l| {
                let mut v: Value = serde_json::from_str(l).unwrap();
                v.as_object_mut().unwrap().remove("ts");
                v
            })
            .collect();
        assert_eq!(
            entries,
            vec![
                json!({"decision": "allow", "message": "frob *", "command": "frob x", "cwd": "/w", "agent": "hermes"}),
                json!({"decision": "ask", "message": "Empty command", "command": "", "cwd": "/w", "agent": "hermes"}),
                json!({"decision": "allow", "message": "edit /w/src/a: /w/src/**", "cwd": "/w", "tool": "Edit", "file_path": "/w/src/a", "agent": "hermes"}),
                json!({"decision": "ask", "message": "Empty path for read", "cwd": "/w", "tool": "Read", "file_path": "", "agent": "hermes"}),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_input_fails_closed() {
        let (out, code) = run("{nope");
        assert_eq!(code, 1);
        assert!(
            out.starts_with(r#"{"action": "ask", "reason": "Invalid JSON input: "#),
            "{out}"
        );
        assert!(out.ends_with(r#", "error": true}"#), "{out}");
        assert_eq!(
            run("[1]"),
            (
                r#"{"action": "ask", "reason": "Dippy error: hook input is not a JSON object", "error": true}"#
                    .to_string(),
                1
            )
        );
    }
}
