//! `set approval dashboard` end to end: the binary's hooks and `run` against
//! a dashboard node in this process. Every sandbox has an empty HOME and a
//! fake askpass that records its input and denies; no GUI, no real agent.

use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::Duration;

use dippy_rs::dashboard::{Server, serve_on};
use serde_json::{Value, json};

const TOKEN: &str = "test-token";

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let dir =
            std::env::temp_dir().join(format!("dippy-rs-approval-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["home/.dippy", "proj", "bin"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("token"), format!("{TOKEN}\n")).unwrap();
        // Records stdin and DIPPY_* variables, then denies.
        script(
            &dir.join("bin/askpass"),
            &format!(
                "#!/bin/bash\ncat > {0}/askpass.stdin\nenv | grep ^DIPPY_ | sort > {0}/askpass.env\nexit 1\n",
                dir.display()
            ),
        );
        // A fictional command that `run` executes when approved.
        script(
            &dir.join("bin/zorp"),
            &format!("#!/bin/bash\ntouch {}/zorp.ran\n", dir.display()),
        );
        Sandbox { dir }
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    fn proj(&self) -> PathBuf {
        self.dir.join("proj")
    }

    /// The user config: logging plus `extra`.
    fn config(&self, extra: &str) {
        let text = format!(
            "set log {}\nset log-full\nset askpass {}\nset askpass-timeout 30\n\
             set dashboard-token-file {}\n{extra}",
            self.dir.join("audit.log").display(),
            self.dir.join("bin/askpass").display(),
            self.dir.join("token").display(),
        );
        std::fs::write(self.home().join(".dippy/config"), text).unwrap();
    }

    fn run(&self, args: &[&str], stdin: &str) -> Output {
        let path = format!(
            "{}:{}",
            self.dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_dippy-rs"))
            .args(args)
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", path)
            .env("LANG", "C.UTF-8")
            .current_dir(self.proj())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn audit(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("audit.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| {
                let mut v: Value = serde_json::from_str(l).unwrap();
                v["ts"] = json!("TS");
                v
            })
            .collect()
    }

    fn askpass_ran(&self) -> bool {
        self.dir.join("askpass.stdin").exists()
    }

    fn reset(&self) {
        for f in ["audit.log", "askpass.stdin", "askpass.env", "zorp.ran"] {
            let _ = std::fs::remove_file(self.dir.join(f));
        }
    }
}

fn script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A node with its user config in the sandbox.
fn node(sb: &Sandbox) -> (Arc<Server>, SocketAddr) {
    let mut server = Server::new(TOKEN, None, None);
    server.user_config = sb.home().join(".dippy/config");
    let server = Arc::new(server);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let s = Arc::clone(&server);
    std::thread::spawn(move || serve_on(listener, s));
    (server, addr)
}

/// Decide the next pending item as soon as it appears.
fn decide_soon(server: &Arc<Server>, allow: bool, note: Option<&str>) {
    let server = Arc::clone(server);
    let note = note.map(str::to_string);
    std::thread::spawn(move || {
        for _ in 0..500 {
            if let Some(item) = server.approvals.list(Some(true)).first() {
                server.approvals.decide(&item.id, allow, note.clone());
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
}

fn bash(command: &str) -> String {
    json!({"tool_name": "Bash", "tool_input": {"command": command}}).to_string()
}

#[test]
fn claude_ask_waits_for_the_dashboard() {
    let sb = Sandbox::new("claude");
    let (server, addr) = node(&sb);
    sb.config(&format!(
        "set approval dashboard\nset dashboard-url http://{addr}\n"
    ));
    decide_soon(&server, true, Some("fine"));
    let o = sb.run(&["--claude"], &bash("zorp --fictional"));
    assert_eq!(o.status.code(), Some(0));
    let out: Value = serde_json::from_str(&stdout(&o)).unwrap();
    let hso = &out["hookSpecificOutput"];
    assert_eq!(hso["permissionDecision"], "allow");
    let reason = hso["permissionDecisionReason"].as_str().unwrap();
    assert!(
        reason.starts_with("🐤 approved by user: ") && reason.ends_with("(user note: fine)"),
        "{reason}"
    );
    assert!(!sb.askpass_ran());
    let entries = sb.audit();
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0]["decision"], "ask");
    assert_eq!(entries[1]["decision"], "allow");
    assert_eq!(entries[1]["message"], "fine");
    assert_eq!(entries[1]["agent"], "claude");
    let id = entries[1]["approval"].as_str().unwrap();
    let item = server.approvals.get(id).unwrap();
    assert_eq!(item.request["agent"], "claude");
    assert_eq!(item.request["command"], "zorp --fictional");
    assert_eq!(
        item.request["cwd"],
        json!(sb.proj().canonicalize().unwrap().to_string_lossy())
    );
    assert_eq!(item.request["causes"][0]["target"], "zorp --fictional");
    // A user deny carries the note to the agent.
    sb.reset();
    decide_soon(&server, false, Some("use the staging config"));
    let o = sb.run(&["--claude"], &bash("zorp --fictional"));
    let out: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(
        out["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("(user note: use the staging config)")
    );
    assert_eq!(sb.audit()[1]["decision"], "deny");
}

#[test]
fn claude_falls_back_to_its_own_prompt_when_no_node_listens() {
    let sb = Sandbox::new("claude-fallback");
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    sb.config("set approval ask\n");
    let native = sb.run(&["--claude"], &bash("zorp --fictional"));
    let native_audit = sb.audit();
    sb.reset();
    sb.config(&format!(
        "set approval dashboard\nset dashboard-url http://{closed}\n"
    ));
    let o = sb.run(&["--claude"], &bash("zorp --fictional"));
    assert_eq!(stdout(&o), stdout(&native));
    assert_eq!(o.status.code(), native.status.code());
    assert_eq!(sb.audit(), native_audit);
    let log = std::fs::read_to_string(sb.home().join(".claude/hook-approvals.log")).unwrap();
    assert!(log.contains("Dashboard approval unavailable, using the native flow"));
}

#[test]
fn claude_timeout_denies_with_the_configured_instruction() {
    let sb = Sandbox::new("claude-timeout");
    let (server, addr) = node(&sb);
    sb.config(&format!(
        "set approval dashboard\nset dashboard-url http://{addr}\nset askpass-timeout 4\n\
         set approval-timeout-message 'Ask the lead.'\n"
    ));
    let o = sb.run(&["--claude"], &bash("zorp --fictional"));
    let out: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        out["hookSpecificOutput"]["permissionDecisionReason"],
        "🐤 Approval timed out after 1 seconds. The requested command was not executed. Ask the lead."
    );
    assert_eq!(sb.audit()[1]["message"], "approval timed out");
    assert_eq!(server.approvals.list(Some(true)).len(), 0);
}

#[test]
fn agy_queue_replaces_askpass() {
    let sb = Sandbox::new("agy");
    let (server, addr) = node(&sb);
    sb.config(&format!(
        "set approval dashboard\nset dashboard-url http://{addr}\n"
    ));
    let payload = json!({"toolCall": {"name": "run_command", "args": {"CommandLine": "zorp x"}},
                         "workspacePaths": [sb.proj().canonicalize().unwrap()]})
    .to_string();
    decide_soon(&server, true, None);
    let o = sb.run(&["--agy"], &payload);
    let out: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(out["decision"], "allow", "{out}");
    assert!(
        out["reason"]
            .as_str()
            .unwrap()
            .starts_with("🐤 approved by user: ")
    );
    assert!(!sb.askpass_ran());
    decide_soon(&server, false, None);
    let o = sb.run(&["--agy"], &payload);
    let out: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(out["decision"], "deny");
    assert!(!sb.askpass_ran());
    // An unmatched tool, which AGY sends to askpass today, is queued too.
    let web = json!({"toolCall": {"name": "search_web", "args": {"query": "cats"}},
                     "workspacePaths": [sb.proj().canonicalize().unwrap()]})
    .to_string();
    decide_soon(&server, true, None);
    let out: Value = serde_json::from_str(&stdout(&sb.run(&["--agy"], &web))).unwrap();
    assert_eq!(out["decision"], "allow");
    let items = server.approvals.list(None);
    assert_eq!(items[0].request["causes"][0]["kind"], "web");
    assert!(!sb.askpass_ran());
}

#[test]
fn run_executes_only_after_dashboard_approval() {
    let sb = Sandbox::new("run");
    let (server, addr) = node(&sb);
    sb.config(&format!(
        "set approval dashboard\nset dashboard-url http://{addr}\n"
    ));
    decide_soon(&server, false, Some("not today"));
    let o = sb.run(&["run", "zorp --fictional"], "");
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(
        err.starts_with(
            "STATUS: Approval required; waiting up to 27 seconds. The requested command has not started."
        ),
        "{err}"
    );
    assert!(err.contains("ERROR: Approval denied.\nThe requested command was not executed:\nzorp --fictional\n\nReason: not today"));
    assert!(!sb.dir.join("zorp.ran").exists() && !sb.askpass_ran());
    let entries = sb.audit();
    assert_eq!(
        (
            entries[1]["decision"].as_str(),
            entries[1]["agent"].as_str()
        ),
        (Some("deny"), Some("run"))
    );
    assert!(entries[1]["approval"].is_string());
    let item = &server.approvals.list(None)[0];
    assert_eq!(
        (
            item.request["tool"].as_str(),
            item.request["remote"].as_bool()
        ),
        (Some("run"), Some(false))
    );
    sb.reset();
    decide_soon(&server, true, None);
    let o = sb.run(&["run", "zorp --fictional"], "");
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(sb.dir.join("zorp.ran").exists() && !sb.askpass_ran());
    // Timeout: neutral text with the configured instruction.
    sb.reset();
    sb.config(&format!(
        "set approval dashboard\nset dashboard-url http://{addr}\nset askpass-timeout 2\n"
    ));
    let o = sb.run(&["run", "zorp --fictional"], "");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        stderr(&o),
        "ERROR: Approval timed out after 0 seconds.\nThe requested command was not executed:\n\
         zorp --fictional\n\nWait for the user and retry the same request, or continue with \
         another safe solution.\n"
    );
}

/// One hook payload per mode, each of which asks.
fn mode_cases(sb: &Sandbox) -> Vec<(Vec<&'static str>, String)> {
    let proj = sb.proj().canonicalize().unwrap();
    vec![
        (vec!["--claude"], bash("zorp --fictional")),
        (vec!["--gemini"], json!({"hook_event_name": "BeforeTool", "tool_name": "run_shell_command", "tool_input": {"command": "zorp x"}}).to_string()),
        (vec!["--cursor"], json!({"hook_event_name": "beforeShellExecution", "command": "zorp x", "cwd": proj}).to_string()),
        (vec!["--codex"], json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "zorp x"}}).to_string()),
        (vec!["--codex"], json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "zorp x"}}).to_string()),
        (vec!["--agy"], json!({"toolCall": {"name": "run_command", "args": {"CommandLine": "zorp x"}}, "workspacePaths": [proj]}).to_string()),
        (vec!["--agy"], json!({"toolCall": {"name": "view_file", "args": {"AbsolutePath": "/etc/x"}}, "workspacePaths": [proj]}).to_string()),
        (vec!["--pi"], json!({"type": "bash", "command": "zorp x", "cwd": proj}).to_string()),
        (vec!["run"], String::new()),
        (vec!["run-on-server", "srv"], String::new()),
    ]
}

/// Everything observable of one run: output, audit, hook log, askpass input.
fn observe(sb: &Sandbox, args: &[&str], stdin: &str) -> Vec<String> {
    sb.reset();
    let _ = std::fs::remove_dir_all(sb.home().join(".claude"));
    let _ = std::fs::remove_dir_all(sb.home().join(".gemini"));
    let mut argv = args.to_vec();
    if args[0].starts_with("run") {
        argv.push("zorp --fictional");
    }
    let o = sb.run(&argv, stdin);
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
    let mask = |s: String| {
        s.lines()
            .map(|l| l.get(20..).unwrap_or(l).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };
    vec![
        format!("{:?}", o.status.code()),
        stdout(&o),
        stderr(&o),
        format!("{:?}", sb.audit()),
        mask(read(sb.home().join(".claude/hook-approvals.log"))),
        mask(read(sb.home().join(".gemini/hook-approvals.log"))),
        read(sb.dir.join("askpass.stdin")),
        read(sb.dir.join("askpass.env")),
    ]
}

#[test]
fn approval_ask_is_identical_to_no_setting_in_every_mode() {
    let sb = Sandbox::new("identity");
    std::fs::write(sb.proj().join(".dippy"), "server srv\n").unwrap();
    for (args, stdin) in mode_cases(&sb) {
        sb.config("");
        let without = observe(&sb, &args, &stdin);
        sb.config("set approval ask\n");
        let with = observe(&sb, &args, &stdin);
        assert_eq!(without, with, "{args:?} {stdin}");
        // The fake askpass denies; every mode produced something to compare.
        assert!(!without[1].is_empty() || !without[2].is_empty(), "{args:?}");
        if matches!(args[0], "--agy" | "run" | "run-on-server") {
            assert!(
                !without[6].is_empty(),
                "askpass did not run: {args:?} {without:?}"
            );
        }
    }
}
