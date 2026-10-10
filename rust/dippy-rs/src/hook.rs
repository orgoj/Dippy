//! Port of the hook path of `dippy.dippy.main` for every agent mode.
//!
//! Output formats: Claude Code (also pi, moltbot, Windsurf and PearAI, which
//! share it), Gemini CLI, Codex, Cursor and Antigravity CLI (AGY `toolCall`
//! payloads, `ask` resolved through the askpass program). Bash
//! classification, MCP, web and file-tool rules, `after` rules, permission
//! bypass modes, `Stop`/`Notification` events and invalid JSON are handled.
//! Not ported: logging and notifier programs (never run). Divergences from
//! Python: a config error fails closed in Gemini and AGY modes (Python
//! allows), and Codex output Python prints as `null` is left empty.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

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

/// Web tools recognised in `toolCall` payloads.
const TOOL_CALL_WEB_NAMES: [&str; 6] = [
    "search_web",
    "read_url_content",
    "google_web_search",
    "web_fetch",
    "WebSearch",
    "WebFetch",
];

/// Agent whose hook protocol the output follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Claude,
    Gemini,
    Agy,
    Cursor,
    Codex,
}

/// `_detect_mode_from_flags` order; pi, moltbot, Windsurf and PearAI use the
/// Claude output format.
const MODE_FLAGS: [(&str, &str, Mode); 10] = [
    ("--claude", "DIPPY_CLAUDE", Mode::Claude),
    ("--gemini", "DIPPY_GEMINI", Mode::Gemini),
    ("--agy", "DIPPY_AGY", Mode::Agy),
    ("--antigravity", "DIPPY_ANTIGRAVITY", Mode::Agy),
    ("--cursor", "DIPPY_CURSOR", Mode::Cursor),
    ("--pi", "DIPPY_PI", Mode::Claude),
    ("--moltbot", "DIPPY_MOLTBOT", Mode::Claude),
    ("--codex", "DIPPY_CODEX", Mode::Codex),
    ("--windsurf", "DIPPY_WINDSURF", Mode::Claude),
    ("--pearai", "DIPPY_PEARAI", Mode::Claude),
];

pub fn is_mode_flag(arg: &str) -> bool {
    MODE_FLAGS.iter().any(|(flag, _, _)| *flag == arg)
}

/// Mode from a command-line flag or a truthy `DIPPY_<AGENT>` variable.
pub fn mode_from_flags(args: &[String], env: impl Fn(&str) -> Option<String>) -> Option<Mode> {
    let truthy = |name: &str| {
        env(name).is_some_and(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
    };
    MODE_FLAGS
        .iter()
        .find(|(flag, var, _)| args.iter().any(|a| a == flag) || truthy(var))
        .map(|(_, _, mode)| *mode)
}

/// `_detect_mode_from_input`.
fn mode_from_input(input: &Value) -> Mode {
    let has = |k: &str| input.get(k).is_some();
    if has("toolCall") {
        return Mode::Agy;
    }
    if (has("command") && !has("tool_name")) || has("cursor_version") {
        return Mode::Cursor;
    }
    let tool_name = input.get("tool_name").and_then(Value::as_str);
    if matches!(
        tool_name,
        Some("shell" | "run_shell" | "run_shell_command" | "execute_shell")
    ) {
        return Mode::Gemini;
    }
    Mode::Claude
}

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

/// What the hook process emits.
#[derive(Debug, PartialEq)]
pub struct HookOutput {
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub exit_code: i32,
}

#[derive(Debug, PartialEq)]
enum Reply {
    Json(Value),
    /// Exit 0 with no output (Codex "no opinion").
    Silent,
    /// Codex block: message on stderr, exit 2.
    Block(String),
}

/// What a decision is about; mirrors the keyword arguments of Python's
/// `approve`/`ask`/`deny`/`pass_`.
#[derive(Clone, Copy, Default)]
struct Subject<'a> {
    tool: Option<&'a str>,
    command: Option<&'a str>,
    file_path: Option<&'a str>,
    match_value: Option<&'a str>,
    cwd: Option<&'a Path>,
    /// Passed only where Python passes `hook_event` (Codex `PermissionRequest`).
    event: Option<&'a str>,
}

#[derive(Clone, Copy)]
struct Hook<'a> {
    mode: Mode,
    config: Option<&'a Config>,
    /// Askpass program: `DIPPY_ASKPASS`, else the config's `askpass`.
    askpass: Option<&'a Path>,
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

fn cursor_reply(permission: &str, msg: &str) -> Value {
    // Both snake_case (v2.0+) and camelCase (v1.7.x) keys.
    json!({
        "permission": permission,
        "user_message": msg,
        "agent_message": msg,
        "userMessage": msg,
        "agentMessage": msg,
    })
}

fn pre_tool(decision: &str, msg: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": msg,
        }
    })
}

impl<'a> Hook<'a> {
    /// Python calls without `config` (no askpass, no `default`).
    fn without_config(self) -> Self {
        Hook {
            config: None,
            askpass: None,
            ..self
        }
    }

    fn approve(self, reason: &str, s: Subject) -> Reply {
        let msg = format!("🐤 {reason}");
        Reply::Json(match self.mode {
            Mode::Agy => {
                let mut res = json!({"decision": "allow", "reason": msg});
                let mut overrides = Vec::new();
                if let Some(c) = non_empty(s.command) {
                    overrides.push(format!("command({c})"));
                }
                if let Some(t) = non_empty(s.tool) {
                    overrides.push(t.to_string());
                }
                if let Some(f) = non_empty(s.file_path) {
                    overrides.push(format!("file({f})"));
                    overrides.push(f.to_string());
                }
                if let Some(m) = non_empty(s.match_value) {
                    overrides.push(m.to_string());
                }
                if !overrides.is_empty() {
                    res["permissionOverrides"] = json!(overrides);
                }
                res
            }
            Mode::Gemini => {
                json!({"decision": "allow", "reason": msg, "systemMessage": msg, "continue": true})
            }
            Mode::Codex if s.event == Some("PermissionRequest") => json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {"behavior": "allow"},
                }
            }),
            // A Codex PreToolUse allow does not approve execution; the later
            // PermissionRequest hook decides.
            Mode::Codex => return Reply::Silent,
            Mode::Cursor => cursor_reply("allow", &msg),
            Mode::Claude => pre_tool("allow", &msg),
        })
    }

    fn ask(self, reason: &str, s: Subject) -> Reply {
        let msg = format!("🐤 {reason}");
        Reply::Json(match self.mode {
            // AGY ignores `ask` in bypass mode: resolve it through askpass.
            Mode::Agy => {
                return if self.askpass_allows(reason, &s) {
                    self.approve(&format!("approved by user: {reason}"), s)
                } else {
                    self.deny(&format!("approval denied or unavailable: {reason}"), s)
                };
            }
            Mode::Gemini => {
                json!({"decision": "ask", "reason": msg, "systemMessage": msg, "continue": true})
            }
            // A Codex `ask` fails open; only show the message.
            Mode::Codex => json!({"systemMessage": msg}),
            Mode::Cursor => cursor_reply("ask", &msg),
            Mode::Claude => pre_tool("ask", &msg),
        })
    }

    fn deny(self, reason: &str, _s: Subject) -> Reply {
        let msg = format!("🐤 {reason}");
        Reply::Json(match self.mode {
            Mode::Agy => json!({"decision": "deny", "reason": msg}),
            Mode::Gemini => json!({"decision": "deny", "reason": msg, "systemMessage": msg}),
            Mode::Codex => return Reply::Block(msg),
            Mode::Cursor => cursor_reply("deny", &msg),
            Mode::Claude => pre_tool("deny", &msg),
        })
    }

    /// No rule matched: defer to the agent (AGY: apply `set default`).
    fn pass(self, reason: &str, s: Subject) -> Reply {
        match self.mode {
            Mode::Agy => match self.config.map(|c| c.default.as_str()) {
                Some("allow") => self.approve(reason, s),
                Some("deny") => self.deny(reason, s),
                _ => self.ask(reason, s),
            },
            Mode::Gemini => Reply::Json(
                json!({"decision": "allow", "reason": format!("🐤 {reason}"), "continue": true}),
            ),
            Mode::Codex => Reply::Silent,
            Mode::Cursor | Mode::Claude => Reply::Json(json!({})),
        }
    }

    fn rule(self, decision: &str, reason: &str, s: Subject) -> Reply {
        match decision {
            "allow" => self.approve(reason, s),
            "deny" => self.deny(reason, s),
            _ => self.ask(reason, s),
        }
    }

    /// `post_tool_response` without a notifier note.
    fn post(self, message: &str) -> Reply {
        let msg = format!("🐤 {message}");
        Reply::Json(match self.mode {
            Mode::Agy => json!({}),
            Mode::Gemini => json!({
                "decision": "allow",
                "reason": msg,
                "additionalContext": msg,
                "continue": true,
            }),
            _ => json!({
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": msg,
                }
            }),
        })
    }

    fn stop(self) -> Value {
        match self.mode {
            Mode::Agy => json!({}),
            Mode::Gemini => json!({"decision": "allow", "continue": false}),
            Mode::Codex => json!({"continue": false}),
            Mode::Cursor | Mode::Claude => json!({"decision": "approve", "continue": true}),
        }
    }

    /// `_run_askpass`: exit 0 allows; anything else (exit 1, other codes,
    /// timeout, missing program, no config) does not.
    fn askpass_allows(self, message: &str, s: &Subject) -> bool {
        let (Some(config), Some(program)) = (self.config, self.askpass) else {
            return false;
        };
        let command = s.command.unwrap_or("");
        let cwd = s
            .cwd
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        let cwd = cwd.to_string_lossy();
        let tool = non_empty(s.tool).or((!command.is_empty()).then_some("run_command"));
        let payload = py_dumps(&json!({
            "command": command,
            "cwd": cwd,
            "rule": null,
            "message": message,
            "tool": tool,
            "source": null,
            "file_path": s.file_path,
        }));
        let mut cmd = Command::new(program);
        cmd.env("DIPPY_COMMAND", command)
            .env("DIPPY_CWD", cwd.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if !message.is_empty() {
            cmd.env("DIPPY_MESSAGE", message);
        }
        if let Some(t) = tool {
            cmd.env("DIPPY_TOOL", t);
        }
        if let Some(f) = non_empty(s.file_path) {
            cmd.env("DIPPY_FILE_PATH", f);
        }
        if config.askpass_timeout != 0 {
            cmd.env("DIPPY_ASKPASS_TIMEOUT", config.askpass_timeout.to_string());
        }
        let Ok(mut child) = cmd.spawn() else {
            return false;
        };
        if let Some(mut stdin) = child.stdin.take() {
            // A separate writer cannot block the timeout loop on a full pipe.
            std::thread::spawn(move || stdin.write_all(payload.as_bytes()));
        }
        let timeout = Duration::from_secs(config.askpass_timeout.max(0) as u64);
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.code() == Some(0),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }
    }
}

/// Fallback for a missing object: `Value::get` on null finds nothing.
const NULL: &Value = &Value::Null;

fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn first_str<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| get_str(v, k))
}

fn process_cwd() -> PathBuf {
    paths::resolve(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
}

/// Run the hook for one stdin payload. `explicit` is the mode from flags or
/// environment; without it the mode is detected from the payload.
pub fn run_hook(explicit: Option<Mode>, stdin: &str) -> HookOutput {
    let reply = hook_reply(explicit, stdin);
    let (stdout, stderr, exit_code) = match reply {
        Reply::Json(v) => (Some(py_dumps(&v)), None, 0),
        Reply::Silent => (None, None, 0),
        Reply::Block(msg) => (None, Some(msg), 2),
    };
    HookOutput {
        stdout,
        stderr,
        exit_code,
    }
}

fn hook_reply(explicit: Option<Mode>, stdin: &str) -> Reply {
    let fallback = Hook {
        mode: explicit.unwrap_or(Mode::Claude),
        config: None,
        askpass: None,
    };
    let error = |reason: &str| match fallback.mode {
        Mode::Gemini => fallback.ask(reason, Subject::default()),
        _ => Reply::Json(json!({})),
    };
    let Ok(input) = serde_json::from_str::<Value>(stdin) else {
        return error("invalid json input");
    };
    if !input.is_object() {
        return error("error: hook input is not a JSON object");
    }
    handle(explicit.unwrap_or_else(|| mode_from_input(&input)), &input)
}

fn handle(mode: Mode, input: &Value) -> Reply {
    let base = Hook {
        mode,
        config: None,
        askpass: None,
    };
    let tool_input = input.get("tool_input").unwrap_or(NULL);
    let tool_args = input
        .get("toolCall")
        .and_then(|c| c.get("args"))
        .unwrap_or(NULL);
    let (cwd, policy_cwd) = if mode == Mode::Agy {
        match agy_cwd(input, tool_args) {
            Ok(dirs) => dirs,
            Err(reason) => return base.deny(reason, Subject::default()),
        }
    } else {
        let cwd = hook_cwd(input, tool_input, tool_args);
        (cwd.clone(), cwd)
    };
    let mut config = match config::load_config(&policy_cwd, None, None) {
        Ok(c) => c,
        Err(e) => return base.ask(&format!("config error: {e}"), Subject::default()),
    };
    if mode == Mode::Agy {
        config.path_rule_cwd = Some(policy_cwd);
    }
    let askpass = std::env::var("DIPPY_ASKPASS")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| config.askpass.clone());
    let hook = Hook {
        mode,
        config: Some(&config),
        askpass: askpass.as_deref(),
    };
    dispatch(hook, &config, input, &cwd)
}

/// AGY: rules resolve against the policy workspace (`DIPPY_POLICY_CWD`, the
/// process cwd when it is a workspace, else the first workspace); commands
/// run in the tool's `Cwd` relative to it.
fn agy_cwd(input: &Value, tool_args: &Value) -> Result<(PathBuf, PathBuf), &'static str> {
    let policy = match std::env::var("DIPPY_POLICY_CWD") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let workspaces: Vec<PathBuf> = input
                .get("workspacePaths")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .collect()
                })
                .unwrap_or_default();
            let process = process_cwd();
            if workspaces.is_empty() || workspaces.contains(&process) {
                process
            } else {
                workspaces[0].clone()
            }
        }
    };
    if !policy.is_absolute() || !policy.is_dir() {
        return Err("AGY policy workspace must be an absolute, existing directory");
    }
    let policy = paths::resolve(&policy);
    let cwd = match first_str(tool_args, &["Cwd", "cwd"]) {
        Some(op) => paths::resolve(&policy.join(op)),
        None => policy.clone(),
    };
    Ok((cwd, policy))
}

/// Non-AGY cwd: explicit fields, else the workspace containing the target
/// file, else the process cwd when it is a workspace, else the first one.
fn hook_cwd(input: &Value, tool_input: &Value, tool_args: &Value) -> PathBuf {
    let explicit = get_str(input, "cwd")
        .or_else(|| get_str(tool_input, "cwd"))
        .or_else(|| {
            first_str(
                tool_args,
                &["Cwd", "cwd", "SearchDirectory", "DirectoryPath"],
            )
        });
    if let Some(c) = explicit {
        return paths::resolve(Path::new(c));
    }
    let workspaces: Vec<&str> = input
        .get("workspacePaths")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let Some(first) = workspaces.first() {
        let resolved: Vec<PathBuf> = workspaces
            .iter()
            .filter(|w| !w.is_empty())
            .map(|w| paths::resolve(Path::new(w)))
            .collect();
        let target = first_str(
            tool_args,
            &[
                "AbsolutePath",
                "TargetFile",
                "SearchPath",
                "file_path",
                "path",
                "filepath",
            ],
        );
        if let Some(target) = target {
            let target = paths::resolve(Path::new(target));
            if let Some(ws) = resolved.iter().find(|ws| target.starts_with(ws)) {
                return ws.clone();
            }
        }
        let process = process_cwd();
        if resolved.contains(&process) {
            return process;
        }
        if !first.is_empty() {
            return paths::resolve(Path::new(first));
        }
    }
    process_cwd()
}

fn dispatch(hook: Hook, config: &Config, input: &Value, cwd: &Path) -> Reply {
    let has = |k: &str| input.get(k).is_some();
    let hook_event = get_str(input, "hook_event_name")
        .or_else(|| get_str(input, "hookEventName"))
        .map(str::to_string)
        .unwrap_or_else(|| {
            if has("terminationReason") || has("fullyIdle") {
                "Stop".into()
            } else if has("toolResponse") || (has("toolCall") && has("error")) {
                "PostToolUse".into()
            } else {
                "PreToolUse".into()
            }
        });
    if hook_event == "Notification" {
        return Reply::Json(json!({}));
    }
    if matches!(hook_event.as_str(), "Stop" | "SubagentStop" | "AfterAgent") {
        return Reply::Json(hook.stop());
    }
    let hook_event = match hook_event.as_str() {
        "BeforeTool" => "PreToolUse".to_string(),
        "AfterTool" => "PostToolUse".to_string(),
        _ => hook_event,
    };
    let post = hook_event == "PostToolUse";
    let permission_mode = input
        .get("permission_mode")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let routed = if hook.mode == Mode::Cursor {
        cursor_command(input)
    } else if hook.mode == Mode::Agy || has("toolCall") {
        tool_call(hook, config, input, post, cwd)
    } else {
        tool_input_payload(hook, config, input, &hook_event, permission_mode, post, cwd)
    };
    let command = match routed {
        Ok(c) => c,
        Err(reply) => return reply,
    };
    if !post && matches!(permission_mode, "bypassPermissions" | "dontAsk") {
        let s = Subject {
            event: Some(&hook_event),
            ..Subject::default()
        };
        return hook.approve(permission_mode, s);
    }
    if post {
        let words = tokenize(command, false);
        return match config::match_after(&words, config, cwd).filter(|m| !m.is_empty()) {
            Some(m) => hook.post(&m),
            None => Reply::Silent,
        };
    }
    let result = analyzer::analyze(command, config, cwd, None, false);
    let s = Subject {
        command: Some(command),
        cwd: Some(cwd),
        event: Some(&hook_event),
        ..Subject::default()
    };
    match result.action {
        Action::Allow => hook.approve(&result.reason, s),
        Action::Deny => hook.deny(&result.reason, s),
        Action::Ask => hook.ask(&result.reason, s),
    }
}

/// Cursor: `preToolUse` (Claude-shaped, shell tool `Shell`) or
/// `beforeShellExecution` (top-level `command`). `beforeMCPExecution` has a
/// JSON-string `tool_input` and falls through to the top-level command.
fn cursor_command(input: &Value) -> Result<&str, Reply> {
    let tool_input = input.get("tool_input").filter(|v| v.is_object());
    if let (Some(tool), Some(tool_input)) = (input.get("tool_name"), tool_input) {
        if !SHELL_TOOL_NAMES.contains(&tool.as_str().unwrap_or("")) {
            return Err(Reply::Json(json!({})));
        }
        return Ok(tool_input
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or(""));
    }
    Ok(input.get("command").and_then(Value::as_str).unwrap_or(""))
}

/// `toolCall` payloads (AGY): MCP, web, file and shell tools.
fn tool_call<'v>(
    hook: Hook,
    config: &Config,
    input: &'v Value,
    post: bool,
    cwd: &Path,
) -> Result<&'v str, Reply> {
    let call = input.get("toolCall");
    let name = call
        .and_then(|c| c.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let args = call.and_then(|c| c.get("args")).unwrap_or(NULL);
    if name == "call_mcp_tool" || name.starts_with("mcp__") {
        let mcp_name = if name == "call_mcp_tool" {
            let inner = args.get("ToolName").and_then(Value::as_str).unwrap_or("");
            match get_str(args, "ServerName") {
                Some(server) => format!("mcp__{server}__{inner}"),
                None => format!("mcp__{inner}"),
            }
        } else {
            name.to_string()
        };
        if post {
            return Err(after_reply(
                hook,
                config::match_after_mcp(&mcp_name, config),
            ));
        }
        let s = Subject {
            tool: Some(&mcp_name),
            ..Subject::default()
        };
        return Err(match config::match_mcp(&mcp_name, config) {
            Some(m) => hook.rule(&m.decision, &rule_reason(&m), s),
            None => hook.pass(
                "no matching rule",
                Subject {
                    cwd: Some(cwd),
                    ..s
                },
            ),
        });
    }
    if TOOL_CALL_WEB_NAMES.contains(&name) {
        let value = first_str(args, &["query", "Url", "url", "q"]).unwrap_or("");
        if post {
            return Err(after_reply(hook, config::match_after_web(value, config)));
        }
        let found = config::match_web(value, config, &config::env_context_flags(config));
        return Err(match found {
            Some(m) => {
                let s = Subject {
                    tool: Some("WebSearch"),
                    ..Subject::default()
                };
                hook.rule(&m.decision, &rule_reason(&m), s)
            }
            None => {
                let s = Subject {
                    tool: Some(name),
                    match_value: Some(value),
                    cwd: Some(cwd),
                    ..Subject::default()
                };
                hook.pass("no matching rule", s)
            }
        });
    }
    if FILE_TOOL_NAMES.contains(&name) {
        let file_path = first_str(
            args,
            &[
                "AbsolutePath",
                "TargetFile",
                "SearchPath",
                "SearchDirectory",
                "DirectoryPath",
                "file_path",
                "path",
                "filepath",
            ],
        );
        return Err(match (file_path, post) {
            (_, true) => Reply::Json(json!({})),
            (Some(path), false) => {
                check_file_tool(hook, config, name, path, cwd).unwrap_or_else(|| {
                    let s = Subject {
                        tool: Some(name),
                        file_path: Some(path),
                        cwd: Some(cwd),
                        ..Subject::default()
                    };
                    hook.pass("no matching rule", s)
                })
            }
            (None, false) => hook
                .without_config()
                .ask("no file path provided", Subject::default()),
        });
    }
    if !SHELL_TOOL_NAMES.contains(&name) {
        let reason = format!("unsupported tool: {name}");
        return Err(hook.without_config().pass(&reason, Subject::default()));
    }
    Ok(first_str(args, &["CommandLine", "command", "cmd"]).unwrap_or(""))
}

/// Claude, Gemini and Codex payloads (`tool_name`/`tool_input`). Gemini asks
/// where the others defer to the agent.
fn tool_input_payload<'v>(
    hook: Hook,
    config: &Config,
    input: &'v Value,
    hook_event: &str,
    permission_mode: &str,
    post: bool,
    cwd: &Path,
) -> Result<&'v str, Reply> {
    let gemini = hook.mode == Mode::Gemini;
    let defer = |reason: &str| {
        if gemini {
            hook.ask(reason, Subject::default())
        } else {
            Reply::Json(json!({}))
        }
    };
    let tool_input = input.get("tool_input").unwrap_or(NULL);
    let tool_name = input.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let bypass = matches!(permission_mode, "bypassPermissions" | "dontAsk");
    let is_mcp = tool_name.starts_with("mcp__");
    if is_mcp || WEB_TOOL_NAMES.contains(&tool_name) {
        let value = if is_mcp {
            tool_name
        } else {
            first_str(tool_input, &["query", "url", "q"]).unwrap_or("")
        };
        if post {
            let message = if is_mcp {
                config::match_after_mcp(value, config)
            } else {
                config::match_after_web(value, config)
            };
            return Err(after_reply(hook, message));
        }
        if bypass {
            let s = Subject {
                event: Some(hook_event),
                ..Subject::default()
            };
            return Err(hook.approve(permission_mode, s));
        }
        let found = if is_mcp {
            config::match_mcp(value, config)
        } else {
            config::match_web(value, config, &config::env_context_flags(config))
        };
        let s = Subject {
            tool: Some(if is_mcp { tool_name } else { "WebSearch" }),
            ..Subject::default()
        };
        return Err(match found {
            Some(m) => hook.rule(&m.decision, &rule_reason(&m), s),
            None => Reply::Json(json!({})),
        });
    }
    if FILE_TOOL_NAMES.contains(&tool_name) {
        if post {
            return Err(Reply::Json(json!({})));
        }
        let Some(file_path) = first_str(tool_input, &["file_path", "path", "filepath"]) else {
            let paths = tool_input.get("paths").and_then(Value::as_array);
            return Err(match paths.filter(|p| !p.is_empty()) {
                Some(paths) => multi_file(hook, config, tool_name, paths, cwd)
                    .unwrap_or_else(|| defer("no matching rule")),
                None => defer("no file path provided"),
            });
        };
        if matches!(
            permission_mode,
            "bypassPermissions" | "dontAsk" | "acceptEdits"
        ) {
            return Err(hook.approve(permission_mode, Subject::default()));
        }
        return Err(check_file_tool(hook, config, tool_name, file_path, cwd)
            .unwrap_or_else(|| defer("no matching rule")));
    }
    if !SHELL_TOOL_NAMES.contains(&tool_name) {
        return Err(defer(&format!("unsupported tool: {tool_name}")));
    }
    Ok(first_str(tool_input, &["command", "cmd"]).unwrap_or(""))
}

fn after_reply(hook: Hook, message: Option<String>) -> Reply {
    match message.filter(|m| !m.is_empty()) {
        Some(m) => hook.post(&m),
        None => Reply::Silent,
    }
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

/// `check_file_tool`: `None` when no rule matches.
fn check_file_tool(
    hook: Hook,
    config: &Config,
    tool_name: &str,
    file_path: &str,
    cwd: &Path,
) -> Option<Reply> {
    let active = config::env_context_flags(config);
    let found = if READ_TOOL_NAMES.contains(&tool_name) {
        config::match_read(file_path, config, cwd, &active)
    } else {
        config::match_edit(file_path, config, cwd, &active)
    }?;
    let s = Subject {
        tool: Some(tool_name),
        file_path: Some(file_path),
        cwd: Some(cwd),
        ..Subject::default()
    };
    Some(hook.rule(&found.decision, &rule_reason(&found), s))
}

/// Multi-file branch: the strictest match over `paths`, `None` when no rule
/// matches. Python matches without context flags and reports the pattern
/// without its source.
fn multi_file(
    hook: Hook,
    config: &Config,
    tool_name: &str,
    paths: &[Value],
    cwd: &Path,
) -> Option<Reply> {
    const ORDER: [&str; 4] = ["deny", "ask", "allow", "pass"];
    let rank = |d: &str| ORDER.iter().position(|o| *o == d).unwrap_or(0);
    let read = MULTI_READ_TOOL_NAMES.contains(&tool_name);
    let s = Subject {
        tool: Some(tool_name),
        ..Subject::default()
    };
    let mut strictest: Option<config::Match> = None;
    for path in paths {
        let Some(path) = path.as_str() else {
            return Some(hook.ask("dippy-rs: non-string entry in paths", s));
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
    let m = strictest?;
    let reason = match &m.message {
        Some(msg) if !msg.is_empty() => msg.clone(),
        _ => format!("[{}]", m.pattern),
    };
    Some(hook.rule(&m.decision, &reason, s))
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
        let out = run_hook(Some(Mode::Claude), "not json");
        assert_eq!(out.stdout.as_deref(), Some("{}"));
        let out = run_hook(Some(Mode::Gemini), "not json");
        assert!(out.stdout.unwrap().contains("\"decision\": \"ask\""));
    }

    #[test]
    fn modes_from_flags_env_and_input() {
        let none = |_: &str| None;
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            mode_from_flags(&args(&["--gemini"]), none),
            Some(Mode::Gemini)
        );
        assert_eq!(
            mode_from_flags(&args(&["--antigravity"]), none),
            Some(Mode::Agy)
        );
        assert_eq!(mode_from_flags(&args(&["--pi"]), none), Some(Mode::Claude));
        assert_eq!(mode_from_flags(&args(&[]), none), None);
        let codex_env = |k: &str| (k == "DIPPY_CODEX").then(|| "Yes".to_string());
        assert_eq!(mode_from_flags(&args(&[]), codex_env), Some(Mode::Codex));
        let off = |_: &str| Some("0".to_string());
        assert_eq!(mode_from_flags(&args(&[]), off), None);
        assert_eq!(mode_from_input(&json!({"toolCall": {}})), Mode::Agy);
        assert_eq!(mode_from_input(&json!({"command": "ls"})), Mode::Cursor);
        assert_eq!(
            mode_from_input(&json!({"tool_name": "Shell", "cursor_version": "2"})),
            Mode::Cursor
        );
        assert_eq!(
            mode_from_input(&json!({"tool_name": "run_shell_command"})),
            Mode::Gemini
        );
        assert_eq!(mode_from_input(&json!({"tool_name": "Bash"})), Mode::Claude);
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
        ask-read /w/private/*\n\
        allow frob\n\
        deny frob delete \"no frob\"\n";

    fn config() -> Config {
        let config = config::parse_config(RULES, None).unwrap();
        config::tag_rules(config, "/w/.dippy", "project")
    }

    fn reply_with(mode: Mode, askpass: Option<&Path>, config: &Config, payload: Value) -> Reply {
        let hook = Hook {
            mode,
            config: Some(config),
            askpass,
        };
        dispatch(hook, config, &payload, Path::new("/w"))
    }

    fn render(reply: Reply) -> Option<String> {
        match reply {
            Reply::Json(v) => Some(py_dumps(&v)),
            Reply::Silent => None,
            Reply::Block(msg) => Some(format!("exit 2: {msg}")),
        }
    }

    fn run(payload: Value) -> Option<String> {
        render(reply_with(Mode::Claude, None, &config(), payload))
    }

    fn decision(payload: Value) -> String {
        let out = run(payload).unwrap_or_default();
        let v: Value = serde_json::from_str(&out).unwrap_or(json!({}));
        v.pointer("/hookSpecificOutput/permissionDecision")
            .and_then(Value::as_str)
            .unwrap_or("none")
            .to_string()
    }

    fn bash(cmd: &str) -> Value {
        json!({"tool_name": "Bash", "tool_input": {"command": cmd}})
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

    #[test]
    fn gemini_format_and_asks_instead_of_deferring() {
        let c = config();
        let gemini = |payload: Value| render(reply_with(Mode::Gemini, None, &c, payload)).unwrap();
        let shell =
            |cmd: &str| json!({"tool_name": "run_shell_command", "tool_input": {"command": cmd}});
        assert_eq!(
            gemini(shell("frob x")),
            "{\"decision\": \"allow\", \"reason\": \"\\ud83d\\udc24 frob (frob)\", \
             \"systemMessage\": \"\\ud83d\\udc24 frob (frob)\", \"continue\": true}"
        );
        assert!(gemini(shell("frob delete")).starts_with("{\"decision\": \"deny\""));
        let unknown = gemini(json!({"tool_name": "glob_files", "tool_input": {}}));
        assert!(unknown.contains("\"ask\"") && unknown.contains("unsupported tool"));
        let nomatch = gemini(json!({"tool_name": "write_file", "tool_input": {"file_path": "/x"}}));
        assert!(nomatch.contains("\"ask\"") && nomatch.contains("no matching rule"));
        let stop = gemini(json!({"hook_event_name": "AfterAgent"}));
        assert_eq!(stop, "{\"decision\": \"allow\", \"continue\": false}");
    }

    #[test]
    fn codex_allow_is_silent_until_permission_request() {
        let c = config();
        let codex = |payload: Value| render(reply_with(Mode::Codex, None, &c, payload));
        assert_eq!(codex(bash("frob x")), None);
        let mut request = bash("frob x");
        request["hook_event_name"] = json!("PermissionRequest");
        assert_eq!(
            codex(request).as_deref(),
            Some(
                "{\"hookSpecificOutput\": {\"hookEventName\": \"PermissionRequest\", \
                 \"decision\": {\"behavior\": \"allow\"}}}"
            )
        );
        assert_eq!(
            codex(bash("frob delete")).as_deref(),
            Some("exit 2: 🐤 frob: no frob")
        );
        let ask = codex(bash("rm -rf x")).unwrap();
        assert!(ask.starts_with("{\"systemMessage\":"), "{ask}");
        // Python passes no hook_event for rule-matched tools.
        let mut mcp = json!({"tool_name": "mcp__fake__get"});
        mcp["hook_event_name"] = json!("PermissionRequest");
        assert_eq!(codex(mcp), None);
    }

    #[test]
    fn cursor_shell_payloads() {
        let c = config();
        let cursor = |payload: Value| render(reply_with(Mode::Cursor, None, &c, payload)).unwrap();
        let out = cursor(json!({"command": "frob x", "cwd": "/w"}));
        assert!(
            out.starts_with("{\"permission\": \"allow\", \"user_message\":"),
            "{out}"
        );
        assert!(out.contains("\"agentMessage\""));
        let out = cursor(json!({"tool_name": "Shell", "tool_input": {"command": "frob delete"}}));
        assert!(out.starts_with("{\"permission\": \"deny\""), "{out}");
        assert_eq!(
            cursor(json!({"tool_name": "Write", "tool_input": {"file_path": "/w/out/a"}})),
            "{}"
        );
        // beforeMCPExecution: tool_input is a JSON string, the command is top level.
        let out = cursor(json!({"tool_name": "x", "tool_input": "{}", "command": "rm -rf /"}));
        assert!(out.starts_with("{\"permission\": \"ask\""), "{out}");
    }

    fn agy(askpass: Option<&Path>, call: Value) -> Value {
        let c = config();
        let out = render(reply_with(
            Mode::Agy,
            askpass,
            &c,
            json!({"toolCall": call}),
        ))
        .unwrap();
        serde_json::from_str(&out).unwrap()
    }

    #[test]
    fn agy_tool_calls_and_overrides() {
        let shell = agy(
            None,
            json!({"name": "run_command", "args": {"CommandLine": "frob x"}}),
        );
        assert_eq!(shell["decision"], "allow");
        assert_eq!(shell["permissionOverrides"], json!(["command(frob x)"]));
        let mcp = agy(
            None,
            json!({"name": "call_mcp_tool", "args": {"ServerName": "fake", "ToolName": "get"}}),
        );
        assert_eq!(mcp["permissionOverrides"], json!(["mcp__fake__get"]));
        let file = agy(
            None,
            json!({"name": "view_file", "args": {"AbsolutePath": "/w/a"}}),
        );
        assert_eq!(
            file["permissionOverrides"],
            json!(["view_file", "file(/w/a)", "/w/a"])
        );
        let web = agy(
            None,
            json!({"name": "search_web", "args": {"query": "rust"}}),
        );
        assert_eq!(web["permissionOverrides"], json!(["WebSearch"]));
        let deny = agy(
            None,
            json!({"name": "run_command", "args": {"CommandLine": "frob delete"}}),
        );
        assert_eq!(
            deny,
            json!({"decision": "deny", "reason": "🐤 frob: no frob"})
        );
    }

    #[test]
    fn agy_ask_goes_through_askpass() {
        let ask = json!({"name": "run_command", "args": {"CommandLine": "rm -rf x"}});
        let none = agy(None, ask.clone());
        assert_eq!(none["decision"], "deny");
        assert!(
            none["reason"]
                .as_str()
                .unwrap()
                .contains("approval denied or unavailable")
        );
        let no = agy(Some(Path::new("/bin/false")), ask.clone());
        assert_eq!(no["decision"], "deny");
        let yes = agy(Some(Path::new("/bin/true")), ask);
        assert_eq!(yes["decision"], "allow");
        assert!(yes["reason"].as_str().unwrap().contains("approved by user"));
        assert_eq!(yes["permissionOverrides"], json!(["command(rm -rf x)"]));
        // Unsupported tools and missing file paths ask without a config: deny.
        let unknown = agy(
            Some(Path::new("/bin/true")),
            json!({"name": "browse", "args": {}}),
        );
        assert_eq!(unknown["decision"], "deny");
        let nopath = agy(
            Some(Path::new("/bin/true")),
            json!({"name": "view_file", "args": {}}),
        );
        assert_eq!(nopath["decision"], "deny");
    }

    #[test]
    fn agy_default_applies_to_unmatched_tools() {
        let mut c = config();
        c.default = "allow".into();
        let call = json!({"toolCall": {"name": "call_mcp_tool", "args": {"ToolName": "x"}}});
        let out = render(reply_with(Mode::Agy, None, &c, call)).unwrap();
        assert!(out.starts_with("{\"decision\": \"allow\""), "{out}");
        assert!(out.contains("[\"mcp__x\"]"), "{out}");
    }

    #[test]
    fn askpass_times_out_to_deny() {
        let mut c = config();
        c.askpass_timeout = 1;
        let hook = Hook {
            mode: Mode::Agy,
            config: Some(&c),
            // Never exits on its own.
            askpass: Some(Path::new("/usr/bin/yes")),
        };
        let started = Instant::now();
        assert!(!hook.askpass_allows("m", &Subject::default()));
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_secs(1) && elapsed < Duration::from_secs(5));
    }
}
