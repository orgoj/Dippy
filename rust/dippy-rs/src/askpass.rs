//! `dippy-askpass`: the approval dialog started by the hook's askpass call.
//! This module holds the logic; [`crate::askpass_gui`] draws it.
//!
//! Protocol (as the Python dialog): the request arrives as JSON on stdin,
//! with `DIPPY_*` variables as fallback; stdout gets `{"decision", "note"}`
//! and the exit code is 0 for allow, 1 otherwise.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::analyzer;
use crate::config::{self, Config};

/// Keys and clicks are ignored this long after the window gains focus.
pub const ARMING: Duration = Duration::from_secs(1);

/// Rule kinds the dialog offers, in display order.
pub const KINDS: [&str; 6] = ["command", "redirect", "read", "edit", "web", "mcp"];
pub const ACTIONS: [&str; 3] = ["allow", "ask", "deny"];

/// One entry of the hook's `causes`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cause {
    pub kind: String,
    pub target: String,
    /// The deciding rule as `directive pattern (file:line)`.
    pub rule: Option<String>,
    /// The context flags active where it asked.
    pub flags: Vec<String>,
}

/// Rule tags from context flags: all but the structural `@` ones,
/// comma-separated.
fn tags_of(flags: &[String]) -> String {
    let tags: Vec<&str> = flags
        .iter()
        .map(String::as_str)
        .filter(|f| !f.starts_with('@'))
        .collect();
    tags.join(",")
}

impl Cause {
    /// The tags a rule for this cause starts with.
    pub fn tags(&self) -> String {
        tags_of(&self.flags)
    }
}

/// What the agent asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub command: String,
    pub cwd: PathBuf,
    pub tool: Option<String>,
    pub message: String,
    pub file_path: Option<String>,
    pub causes: Vec<Cause>,
    /// Context flags and remoteness the command was analysed with
    /// (`dippy run-on-server`); empty and false for hooks.
    pub flags: Vec<String>,
    pub remote: bool,
    pub timeout: Option<Duration>,
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn cause(v: &Value) -> Option<Cause> {
    let rule = v.get("rule").filter(|r| r.is_object()).map(|r| {
        let mut s = format!(
            "{} {}",
            r["directive"].as_str().unwrap_or(""),
            r["pattern"].as_str().unwrap_or("")
        );
        if let (Some(file), Some(line)) = (r["file"].as_str(), r["line"].as_u64()) {
            s.push_str(&format!(" ({file}:{line})"));
        }
        s
    });
    Some(Cause {
        kind: text(v, "kind")?,
        target: text(v, "target")?,
        rule,
        flags: strings(v, "flags"),
    })
}

impl Request {
    /// The stdin JSON, with `env` (a `DIPPY_*` lookup) filling missing fields.
    pub fn parse(stdin: &str, env: impl Fn(&str) -> Option<String>) -> Self {
        let v: Value = serde_json::from_str(stdin).unwrap_or(Value::Null);
        let env = |k: &str| env(k).filter(|s| !s.is_empty());
        let cwd = text(&v, "cwd")
            .or_else(|| env("DIPPY_CWD"))
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            command: text(&v, "command")
                .or_else(|| env("DIPPY_COMMAND"))
                .unwrap_or_default(),
            cwd,
            tool: text(&v, "tool").or_else(|| env("DIPPY_TOOL")),
            message: text(&v, "message")
                .or_else(|| env("DIPPY_MESSAGE"))
                .unwrap_or_default(),
            file_path: text(&v, "file_path").or_else(|| env("DIPPY_FILE_PATH")),
            causes: v
                .get("causes")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(cause).collect())
                .unwrap_or_default(),
            flags: strings(&v, "flags"),
            remote: v.get("remote").and_then(Value::as_bool).unwrap_or(false),
            timeout: env("DIPPY_ASKPASS_TIMEOUT")
                .and_then(|t| t.parse::<u64>().ok())
                .filter(|t| *t > 0)
                .map(Duration::from_secs),
        }
    }

    /// Initial rule kind and pattern: the first cause, else the request.
    /// `${` is written as `$${`, so the rule matches the text literally.
    pub fn prefill(&self) -> (&str, String) {
        let (kind, pattern) = match self.causes.first() {
            Some(c) if KINDS.contains(&c.kind.as_str()) => (c.kind.as_str(), c.target.as_str()),
            _ if !self.command.is_empty() => ("command", self.command.as_str()),
            _ => ("command", self.file_path.as_deref().unwrap_or("")),
        };
        (kind, pattern.replace("${", "$${"))
    }

    /// Initial rule tags: the first cause's, else the request's.
    pub fn prefill_tags(&self) -> String {
        self.causes
            .first()
            .map_or_else(|| tags_of(&self.flags), Cause::tags)
    }

    /// The whole request as shown (command, else file path or tool).
    pub fn subject(&self) -> &str {
        if !self.command.is_empty() {
            &self.command
        } else if let Some(f) = &self.file_path {
            f
        } else {
            self.tool.as_deref().unwrap_or("")
        }
    }
}

/// `text` with paths under `home` shown as `~/...`, for display only.
pub fn tilde(text: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    if text == home {
        return "~".to_string();
    }
    text.replace(&format!("{home}/"), "~/")
}

/// `allow git *`, `deny-redirect /etc/*`, `allow [ssh,host] ls`, ...;
/// `tags` is a comma-separated flag list.
pub fn rule_line(action: &str, kind: &str, tags: &str, pattern: &str) -> String {
    let directive = if kind == "command" {
        action.to_string()
    } else {
        format!("{action}-{kind}")
    };
    let tags: Vec<&str> = tags
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    if tags.is_empty() {
        format!("{directive} {}", pattern.trim())
    } else {
        format!("{directive} [{}] {}", tags.join(","), pattern.trim())
    }
}

fn analyze(
    command: &str,
    flags: &[String],
    remote: bool,
    cwd: &Path,
    config: &Config,
) -> &'static str {
    let flags: config::Flags = flags.iter().cloned().collect();
    analyzer::analyze(command, config, cwd, Some(&flags), remote)
        .action
        .as_str()
}

/// The decision the request gets under `config`, as the AGY hook applies it
/// (an unmatched tool gets `set default`).
pub fn decide(req: &Request, config: &Config) -> &'static str {
    match req.causes.first() {
        Some(c) if !matches!(c.kind.as_str(), "command" | "redirect") => {
            decide_cause(req, c, config)
        }
        _ => analyze(&req.command, &req.flags, req.remote, &req.cwd, config),
    }
}

/// The decision one cause gets on its own under `config`. An unmatched
/// redirect keeps asking (it asked before); other unmatched tools get
/// `set default`.
pub fn decide_cause(req: &Request, cause: &Cause, config: &Config) -> &'static str {
    let verdict = |m: Option<config::Match>, unmatched: &str| -> &'static str {
        match m.as_ref().map_or(unmatched, |m| m.decision.as_str()) {
            "allow" => "allow",
            "deny" => "deny",
            _ => "ask",
        }
    };
    let mut flags = config::env_context_flags(config);
    flags.extend(cause.flags.iter().cloned());
    let (t, cwd, default) = (&cause.target, &req.cwd, config.default.as_str());
    match cause.kind.as_str() {
        "redirect" => verdict(config::match_redirect(t, config, cwd, &flags, false), "ask"),
        "read" => verdict(config::match_read(t, config, cwd, &flags), default),
        "edit" => verdict(config::match_edit(t, config, cwd, &flags), default),
        "web" => verdict(config::match_web(t, config, &flags), default),
        "mcp" => verdict(config::match_mcp(t, config), default),
        _ => analyze(t, &cause.flags, req.remote, cwd, config),
    }
}

/// Decisions with a candidate rule: the selected cause and the whole request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcome {
    pub cause: &'static str,
    pub request: &'static str,
}

/// Live check of a candidate rule.
#[derive(Debug, PartialEq)]
pub struct Check {
    pub line: String,
    /// Parse error, or the decisions with the rule.
    pub outcome: Result<Outcome, String>,
    /// A bare `*` or a command pattern without a literal command word.
    pub broad: bool,
}

impl Check {
    /// Saving needs a valid rule that gives the selected cause the chosen
    /// action; other causes may still ask.
    pub fn can_save(&self, action: &str) -> bool {
        self.outcome.as_ref().is_ok_and(|o| o.cause == action)
    }
}

/// Where rules go: the user config and the chosen target file.
pub struct Files<'a> {
    pub user: &'a Path,
    pub target: &'a Path,
}

/// Check a candidate rule against the selected cause (the whole request
/// when there is none).
pub fn check(
    req: &Request,
    cause: Option<&Cause>,
    action: &str,
    kind: &str,
    tags: &str,
    pattern: &str,
    files: &Files,
) -> Check {
    let line = rule_line(action, kind, tags, pattern);
    let first = pattern.split_whitespace().next().unwrap_or("");
    let broad = first == "*" || (kind == "command" && first.contains(['*', '?', '[']));
    let outcome = crate::admin::validate_rule(&line).and_then(|_| {
        let config = config::load_config_with_rule(&req.cwd, files.user, files.target, &line);
        config::take_warnings();
        config
            .map(|c| {
                let request = decide(req, &c);
                Outcome {
                    cause: cause.map_or(request, |k| decide_cause(req, k, &c)),
                    request,
                }
            })
            .map_err(|e| e.to_string())
    });
    Check {
        line,
        outcome,
        broad,
    }
}

/// The current decision of each cause, with user config `user`.
pub fn statuses(req: &Request, user: &Path) -> Result<Vec<&'static str>, String> {
    let config = config::load_config_from(&req.cwd, user);
    config::take_warnings();
    let config = config.map_err(|e| e.to_string())?;
    Ok(req
        .causes
        .iter()
        .map(|c| decide_cause(req, c, &config))
        .collect())
}

/// Approval is armed `ARMING` after the window gains focus and disarmed
/// whenever it loses focus.
#[derive(Debug, Default)]
pub struct Arming {
    armed_at: Option<Instant>,
}

impl Arming {
    pub fn focus(&mut self, focused: bool, now: Instant) {
        match (focused, self.armed_at) {
            (true, None) => self.armed_at = Some(now + ARMING),
            (false, _) => self.armed_at = None,
            (true, Some(_)) => {}
        }
    }
    pub fn armed(&self, now: Instant) -> bool {
        self.armed_at.is_some_and(|t| now >= t)
    }
}

/// Keyboard decision: Ctrl+Enter approves, Escape denies, nothing else
/// (no single-letter shortcuts, no plain Enter); only when armed.
pub fn key_decision(armed: bool, ctrl_enter: bool, escape: bool) -> Option<&'static str> {
    match (armed, ctrl_enter, escape) {
        (false, _, _) => None,
        (true, true, _) => Some("allow"),
        (true, false, true) => Some("deny"),
        _ => None,
    }
}

/// Seconds left before the dialog denies on its own; it ends one second
/// before the hook's own timeout so the reply is clean.
pub fn remaining(timeout: Option<Duration>, elapsed: Duration) -> Option<u64> {
    timeout.map(|t| t.saturating_sub(elapsed).as_secs().saturating_sub(1))
}

/// Stdout and exit code for a decision.
pub fn response(decision: &str, note: &str) -> (String, i32) {
    let mut v = json!({"decision": decision});
    if !note.trim().is_empty() {
        v["note"] = json!(note.trim());
    }
    (v.to_string(), if decision == "allow" { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(json: &str) -> Request {
        Request::parse(json, |_| None)
    }

    #[test]
    fn parses_stdin_and_env_fallback() {
        let r = req(
            r#"{"command": "ls && zorp x", "cwd": "/w", "tool": "run_command",
            "message": "zorp", "causes": [{"kind": "command", "target": "zorp x", "rule": null},
            {"kind": "redirect", "target": "/etc/x", "rule": {"directive": "ask-redirect",
            "pattern": "/etc/*", "file": "/u/.dippy/config", "line": 7}}]}"#,
        );
        assert_eq!(r.command, "ls && zorp x");
        assert_eq!(r.cwd, PathBuf::from("/w"));
        assert_eq!(
            r.causes[1].rule.as_deref(),
            Some("ask-redirect /etc/* (/u/.dippy/config:7)")
        );
        assert_eq!(r.prefill(), ("command", "zorp x".to_string()));
        // A prefilled rule never introduces an environment reference.
        let literal = req(r#"{"command": "zorp ${HOME} $${X}", "cwd": "/w"}"#);
        assert_eq!(
            literal.prefill(),
            ("command", "zorp $${HOME} $$${X}".to_string())
        );
        let env = |k: &str| match k {
            "DIPPY_COMMAND" => Some("rm x".to_string()),
            "DIPPY_CWD" => Some("/e".to_string()),
            "DIPPY_ASKPASS_TIMEOUT" => Some("30".to_string()),
            _ => None,
        };
        let r = Request::parse("not json", env);
        assert_eq!(
            (r.command.as_str(), r.cwd.as_path()),
            ("rm x", Path::new("/e"))
        );
        assert_eq!(r.timeout, Some(Duration::from_secs(30)));
        assert_eq!(r.prefill(), ("command", "rm x".to_string()));
        let file = req(r#"{"file_path": "/w/a", "tool": "view_file"}"#);
        assert_eq!(
            (file.subject(), file.prefill()),
            ("/w/a", ("command", "/w/a".to_string()))
        );
    }

    #[test]
    fn tilde_shortens_home_paths_only() {
        let home = Path::new("/home/u");
        assert_eq!(tilde("/home/u", home), "~");
        assert_eq!(tilde("/home/u/w/.dippy", home), "~/w/.dippy");
        assert_eq!(
            tilde("ask x (/home/u/.dippy/config:4)", home),
            "ask x (~/.dippy/config:4)"
        );
        assert_eq!(tilde("/home/user2/x", home), "/home/user2/x");
    }

    #[test]
    fn rule_lines_follow_action_and_kind() {
        assert_eq!(
            rule_line("allow", "command", "", " git push * "),
            "allow git push *"
        );
        assert_eq!(
            rule_line("deny", "redirect", "", "/etc/*"),
            "deny-redirect /etc/*"
        );
        assert_eq!(rule_line("ask", "read", "", ".env"), "ask-read .env");
        assert_eq!(
            rule_line("allow", "command", " ssh , host ", "ls"),
            "allow [ssh,host] ls"
        );
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dippy-rs-askpass-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn check_reclassifies_with_the_candidate() {
        let dir = temp("check");
        let target = dir.join(".dippy");
        let user = dir.join("no-user-config");
        let files = Files {
            user: &user,
            target: &target,
        };
        let r = req(&format!(
            r#"{{"command": "ls && zorp x", "cwd": "{}",
            "causes": [{{"kind": "command", "target": "zorp x", "rule": null}}]}}"#,
            dir.display()
        ));
        let cause = r.causes.first();
        let both = |cause, request| Ok(Outcome { cause, request });
        let c = check(&r, cause, "allow", "command", "", "zorp x", &files);
        assert_eq!(
            (c.line.as_str(), c.outcome.clone()),
            ("allow zorp x", both("allow", "allow"))
        );
        assert!(c.can_save("allow") && !c.broad);
        // A rule for another command leaves the request asking.
        let other = check(&r, cause, "allow", "command", "", "frob", &files);
        assert_eq!(other.outcome, both("ask", "ask"));
        assert!(!other.can_save("allow"));
        assert!(check(&r, cause, "allow", "command", "", "*", &files).broad);
        assert!(check(&r, cause, "allow", "command", "", "zo* x", &files).broad);
        let bad = check(&r, cause, "allow", "command", "", "", &files);
        assert!(bad.outcome.is_err() && !bad.can_save("allow"));
        assert!(!target.exists());
        // File tools are decided by their path rules.
        let read = req(&format!(
            r#"{{"file_path": "/w/k", "tool": "view_file", "cwd": "{}",
            "causes": [{{"kind": "read", "target": "/w/k", "rule": null}}]}}"#,
            dir.display()
        ));
        assert_eq!(
            check(
                &read,
                read.causes.first(),
                "deny",
                "read",
                "",
                "/w/*",
                &files
            )
            .outcome,
            both("deny", "deny")
        );
        // Without causes the whole request is the subject.
        let bare = req(&format!(
            r#"{{"command": "zorp x", "cwd": "{}"}}"#,
            dir.display()
        ));
        assert!(check(&bare, None, "allow", "command", "", "zorp x", &files).can_save("allow"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn several_causes_are_resolved_one_at_a_time() {
        let dir = temp("causes");
        let target = dir.join(".dippy");
        let user = dir.join("no-user-config");
        let files = Files {
            user: &user,
            target: &target,
        };
        let r = req(&format!(
            r#"{{"command": "zorp x; echo a > /etc/zorp.conf", "cwd": "{}",
            "causes": [{{"kind": "command", "target": "zorp x", "rule": null}},
            {{"kind": "redirect", "target": "/etc/zorp.conf", "rule": null}}]}}"#,
            dir.display()
        ));
        assert_eq!(statuses(&r, &user), Ok(vec!["ask", "ask"]));
        // The command rule resolves its cause; the redirect still asks.
        let c = check(
            &r,
            r.causes.first(),
            "allow",
            "command",
            "",
            "zorp x",
            &files,
        );
        assert_eq!(
            c.outcome,
            Ok(Outcome {
                cause: "allow",
                request: "ask"
            })
        );
        assert!(c.can_save("allow"));
        crate::admin::add_rule(&target, &c.line, "2026-10-10").unwrap();
        assert_eq!(statuses(&r, &user), Ok(vec!["allow", "ask"]));
        let redirect = r.causes.get(1);
        let c = check(
            &r,
            redirect,
            "allow",
            "redirect",
            "",
            "/etc/zorp.conf",
            &files,
        );
        assert_eq!(
            c.outcome,
            Ok(Outcome {
                cause: "allow",
                request: "allow"
            })
        );
        // An `ask` rule confirms the cause keeps asking and stays savable.
        assert!(check(&r, redirect, "ask", "redirect", "", "/etc/*", &files).can_save("ask"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tags_scope_rules_like_the_request() {
        let dir = temp("tags");
        let target = dir.join(".dippy");
        let user = dir.join("no-user-config");
        let files = Files {
            user: &user,
            target: &target,
        };
        let r = req(&format!(
            r#"{{"command": "zorp x | cat", "cwd": "{}", "server": "srv",
            "flags": ["run-on-server", "srv"], "remote": true,
            "causes": [{{"kind": "command", "target": "zorp x", "rule": null,
            "flags": ["@compound", "@pipeline", "run-on-server", "srv"]}}]}}"#,
            dir.display()
        ));
        assert_eq!(r.flags, ["run-on-server", "srv"]);
        assert!(r.remote);
        // Structural `@` flags are not prefilled.
        assert_eq!(r.causes[0].tags(), "run-on-server,srv");
        assert_eq!(r.prefill_tags(), "run-on-server,srv");
        let bare = Request {
            causes: Vec::new(),
            ..r.clone()
        };
        assert_eq!(bare.prefill_tags(), "run-on-server,srv");
        assert_eq!(statuses(&r, &user), Ok(vec!["ask"]));
        let cause = r.causes.first();
        let both = |cause, request| Ok(Outcome { cause, request });
        let tagged = check(
            &r,
            cause,
            "allow",
            "command",
            "run-on-server,srv",
            "zorp x",
            &files,
        );
        assert_eq!(tagged.line, "allow [run-on-server,srv] zorp x");
        assert_eq!(tagged.outcome, both("allow", "allow"));
        let other = check(&r, cause, "allow", "command", "other", "zorp x", &files);
        assert_eq!(other.line, "allow [other] zorp x");
        assert_eq!(other.outcome, both("ask", "ask"));
        assert_eq!(
            check(&r, cause, "allow", "command", " ", "zorp x", &files).line,
            "allow zorp x"
        );
        // The request is analysed as remote: a remote redirect target is
        // not a local file.
        let remote = req(&format!(
            r#"{{"command": "ls > /etc/zorp", "cwd": "{}", "remote": true}}"#,
            dir.display()
        ));
        let local = Request {
            remote: false,
            ..remote.clone()
        };
        let config = config::load_config_from(&dir, &user).unwrap();
        assert_eq!(
            (decide(&remote, &config), decide(&local, &config)),
            ("allow", "ask")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn arming_waits_after_each_focus_gain() {
        let t = Instant::now();
        let mut a = Arming::default();
        assert!(!a.armed(t + Duration::from_secs(5)));
        a.focus(true, t);
        assert!(!a.armed(t + Duration::from_millis(999)));
        assert!(a.armed(t + ARMING));
        a.focus(true, t + Duration::from_secs(3));
        assert!(a.armed(t + Duration::from_secs(3)));
        a.focus(false, t + Duration::from_secs(4));
        a.focus(true, t + Duration::from_secs(5));
        assert!(!a.armed(t + Duration::from_millis(5500)));
        assert!(a.armed(t + Duration::from_secs(6)));
    }

    #[test]
    fn only_ctrl_enter_and_escape_decide_when_armed() {
        assert_eq!(key_decision(false, true, false), None);
        assert_eq!(key_decision(false, false, true), None);
        assert_eq!(key_decision(true, true, false), Some("allow"));
        assert_eq!(key_decision(true, false, true), Some("deny"));
        assert_eq!(key_decision(true, false, false), None);
    }

    #[test]
    fn countdown_and_response() {
        let s = Duration::from_secs;
        assert_eq!(remaining(None, s(100)), None);
        assert_eq!(remaining(Some(s(59)), s(0)), Some(58));
        assert_eq!(remaining(Some(s(59)), s(70)), Some(0));
        assert_eq!(
            response("allow", " "),
            ("{\"decision\":\"allow\"}".to_string(), 0)
        );
        assert_eq!(
            response("deny", "later"),
            ("{\"decision\":\"deny\",\"note\":\"later\"}".to_string(), 1)
        );
    }
}
