//! Command-line surface of the `dippy-rs` binary.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// Empty HOME and an empty `--config-only` file, so no live config is read.
fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dippy-rs-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    std::fs::write(dir.join("config"), "").unwrap();
    dir
}

fn run(dir: &PathBuf, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dippy-rs"))
        .args(args)
        .env("HOME", dir.join("home"))
        .current_dir(dir)
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

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn crate_version_matches_python_package() {
    let pyproject =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../pyproject.toml"))
            .unwrap();
    let line = pyproject
        .lines()
        .find(|l| l.starts_with("version = "))
        .unwrap();
    assert_eq!(line, format!("version = \"{}\"", env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_prints_dippy_and_version() {
    let dir = sandbox("version");
    let o = run(&dir, &["--version"], "");
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), format!("dippy {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_options_subcommands_and_exit_codes() {
    let dir = sandbox("help");
    let o = run(&dir, &["--help"], "");
    assert_eq!(o.status.code(), Some(0));
    let out = stdout(&o);
    for text in [
        "--cmd",
        "--stdin",
        "--config-only",
        "--remote",
        "audit",
        "2 = ask",
    ] {
        assert!(out.contains(text), "{text} missing in:\n{out}");
    }
    assert!(!out.contains("--claude"), "hook flags stay hidden:\n{out}");
    for text in ["run-on-server", "recover"] {
        assert!(!out.contains(text), "{text} is removed:\n{out}");
    }
}

#[test]
fn removed_run_on_server_subcommands_are_rejected() {
    let dir = sandbox("removed-run-on-server");
    for args in [
        &["run-on-server", "srv", "echo hi"][..],
        &["recover", "srv"],
        &["config", "server", "list"],
    ] {
        let o = run(&dir, args, "");
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(stdout(&o).is_empty(), "{args:?}");
    }
}

#[test]
fn audit_help_lists_filters() {
    let dir = sandbox("audit-help");
    let o = run(&dir, &["audit", "--help"], "");
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).contains("--group-by"));
}

#[test]
fn usage_errors_exit_two() {
    let dir = sandbox("usage");
    for args in [
        &["--cmd", "ls", "--stdin"][..],
        &["--config", "a", "--config-only", "b", "--cmd", "ls"],
        &["--bogus"],
        &["audit", "--decision", "maybe"],
    ] {
        let o = run(&dir, args, "");
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn cmd_and_stdin_classify() {
    let dir = sandbox("cmd");
    let o = run(
        &dir,
        &["--config-only", "config", "--cmd", "ls", "--json"],
        "",
    );
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).starts_with("{\"decision\": \"allow\""));
    let o = run(
        &dir,
        &["--agent", "x", "--config-only", "config", "--stdin"],
        "rm -rf x\n",
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(stdout(&o).starts_with("ask: "));
    let o = run(&dir, &["--config-only", "config", "--cmd", "-s x"], "");
    assert_eq!(o.status.code(), Some(2));
    assert!(stdout(&o).starts_with("ask: "), "{o:?}");
}

#[test]
fn cmd_and_stdin_write_the_audit_log() {
    let dir = sandbox("cmd-audit");
    let log = dir.join("audit.log");
    std::fs::create_dir_all(dir.join("home/.dippy")).unwrap();
    std::fs::write(
        dir.join("home/.dippy/config"),
        format!("set log {}\n", log.display()),
    )
    .unwrap();
    run(&dir, &["--cmd", "frobnicate x"], "");
    run(&dir, &["--stdin", "--agent", "wrapper"], "frobnicate --y");
    let entries: Vec<serde_json::Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let summary: Vec<_> = entries
        .iter()
        .map(|e| {
            (
                e["decision"].as_str(),
                e["agent"].as_str(),
                e["cmd"].is_string(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (Some("ask"), Some("cli"), true),
            (Some("ask"), Some("wrapper"), true)
        ]
    );
}

#[test]
fn every_call_logs_to_the_default_audit_log_without_set_log() {
    let dir = sandbox("default-log");
    let bash = r#"{"tool_name": "Bash", "tool_input": {"command": "ls"}}"#;
    run(&dir, &["--claude"], bash);
    run(&dir, &["--codex"], r#"{"hook_event_name": "Stop"}"#);
    run(&dir, &["--claude"], "not json");
    let idle = format!(r#"{{"type": "idle", "cwd": "{}"}}"#, dir.display());
    run(&dir, &["--pi"], &idle);
    run(&dir, &["--pi"], "not json");
    run(&dir, &["--cmd", "frobnicate x"], "");
    let entries: Vec<serde_json::Value> =
        std::fs::read_to_string(dir.join("home/.dippy/audit.log"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
    let summary: Vec<_> = entries
        .iter()
        .map(|e| {
            (
                e["decision"].as_str().unwrap(),
                e["agent"].as_str().unwrap(),
                e["event"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("allow", "claude", None),
            ("pass", "codex", Some("Stop")),
            ("pass", "claude", None),
            ("allow", "pi", Some("idle")),
            ("ask", "pi", None),
            ("ask", "cli", None),
        ],
        "{entries:?}"
    );
    assert_eq!(entries[2]["message"], "invalid json input");
    assert!(
        entries[4]["message"]
            .as_str()
            .unwrap()
            .starts_with("Invalid JSON input: "),
        "{entries:?}"
    );
    // `audit` reads the same default log.
    let o = run(&dir, &["audit", "--group-by", "agent"], "");
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    assert_eq!(stdout(&o), "2\tclaude\n2\tpi\n1\tcli\n1\tcodex\n");
}

#[test]
fn hook_mode_without_cmd_reads_payload() {
    let dir = sandbox("hook");
    let payload = r#"{"tool_name": "Bash", "tool_input": {"command": "ls"}}"#;
    for args in [&["--claude"][..], &[], &["--claude", "--json"]] {
        let o = run(&dir, args, payload);
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        assert!(stdout(&o).contains("\"allow\""), "{args:?}: {}", stdout(&o));
    }
}

#[test]
fn pi_mode_speaks_the_pi_extension_protocol() {
    let dir = sandbox("pi");
    std::fs::create_dir_all(dir.join("proj")).unwrap();
    std::fs::write(dir.join("proj/.dippy"), "allow-edit src/**\n").unwrap();
    let payload = format!(
        r#"{{"type": "edit", "path": "src/a.rs", "cwd": "{}"}}"#,
        dir.join("proj").display()
    );
    let o = run(&dir, &["--pi"], &payload);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        "{\"action\": \"allow\", \"reason\": \"edit src/a.rs: src/**\", \
         \"context_flags\": [], \"note\": null, \"error\": false}\n"
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_dippy-rs"))
        .env("HOME", dir.join("home"))
        .env("DIPPY_PI", "1")
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("\"error\": true"), "{}", stdout(&o));
}

fn config(dir: &PathBuf, args: &[&str]) -> (i32, String, String) {
    let argv: Vec<&str> = std::iter::once("config")
        .chain(args.iter().copied())
        .collect();
    let o = run(dir, &argv, "");
    (
        o.status.code().unwrap(),
        stdout(&o),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn config_set_get_unset_keeps_other_lines() {
    use std::os::unix::fs::PermissionsExt;
    let dir = sandbox("config");
    let user = dir.join("home/.dippy/config");
    assert_eq!(config(&dir, &["get", "askpass-timeout"]).1, "59\n");
    assert_eq!(config(&dir, &["get", "askpass"]).1, "None\n");
    assert_eq!(config(&dir, &["set", "askpass-timeout", "30"]).0, 0);
    let mode = std::fs::metadata(&user).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    std::fs::write(
        &user,
        "# keep\nset ASKPASS_TIMEOUT 1\nallow ls\nset askpass-timeout 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(config(&dir, &["set", "Askpass_Timeout", "7"]).0, 0);
    assert_eq!(
        std::fs::read_to_string(&user).unwrap(),
        "# keep\nset askpass-timeout 7\nallow ls\n"
    );
    let mode = std::fs::metadata(&user).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644);
    assert_eq!(config(&dir, &["get", "askpass-timeout"]).1, "7\n");
    assert_eq!(config(&dir, &["unset", "askpass-timeout"]).0, 0);
    assert_eq!(config(&dir, &["get"]).1, "# keep\nallow ls\n");
    let (code, _, err) = config(&dir, &["set", "askpass-timeout", "abc"]);
    assert_eq!(code, 1);
    assert!(
        err.ends_with("invalid value for askpass-timeout: abc\n"),
        "{err}"
    );
    let (code, _, err) = config(&dir, &["get", "nope"]);
    assert_eq!((code, err.as_str()), (1, "unsupported config key: nope\n"));
}

#[test]
fn config_approval_settings_and_user_only_scope() {
    let dir = sandbox("config-approval");
    let home = dir.join("home");
    assert_eq!(config(&dir, &["get", "approval"]).1, "ask\n");
    assert_eq!(
        config(&dir, &["get", "dashboard-url"]).1,
        "http://127.0.0.1:8765\n"
    );
    assert_eq!(
        config(&dir, &["get", "dashboard-token-file"]).1,
        format!("{}/.dippy/dashboard-token\n", home.display())
    );
    assert!(
        config(&dir, &["get", "approval-timeout-message"])
            .1
            .starts_with("Wait for the user")
    );
    assert_eq!(config(&dir, &["set", "approval", "dashboard"]).0, 0);
    assert_eq!(config(&dir, &["get", "approval"]).1, "dashboard\n");
    let (code, _, err) = config(&dir, &["set", "approval", "maybe"]);
    assert_eq!(
        (code, err.as_str()),
        (1, "invalid value for approval: maybe\n")
    );
    assert_eq!(
        config(&dir, &["set", "--project", "approval", "dashboard"]).0,
        0
    );
    let (code, _, err) = config(&dir, &["set", "--project", "dashboard-url", "http://h:1"]);
    assert_eq!(
        (code, err.as_str()),
        (1, "dashboard-url is allowed only in the user config\n")
    );
    assert_eq!(
        config(&dir, &["set", "dashboard-url", "http://127.0.0.1:9"]).0,
        0
    );
    assert_eq!(
        config(&dir, &["get", "dashboard-url"]).1,
        "http://127.0.0.1:9\n"
    );
}

#[test]
fn config_rejects_values_that_add_lines() {
    let dir = sandbox("config-newline");
    for value in ["/bin/ask\nallow *", "x\rallow *"] {
        let (code, _, err) = config(&dir, &["set", "askpass", value]);
        assert_eq!(code, 1, "{value:?}");
        assert!(err.starts_with("invalid value for askpass"), "{err}");
    }
    assert!(!dir.join("home/.dippy/config").exists());
}

#[test]
fn config_requires_an_action() {
    let dir = sandbox("config-action");
    let (code, _, err) = config(&dir, &[]);
    assert_eq!((code, err.as_str()), (1, "config action required\n"));
    assert_eq!(config(&dir, &["get", "--user", "--project"]).0, 2);
}

#[test]
fn config_add_rule_validates_and_appends() {
    let dir = sandbox("config-add-rule");
    let (code, out, _) = config(&dir, &["add-rule", "allow zorp *"]);
    let user = dir.join("home/.dippy/config");
    assert_eq!((code, out), (0, format!("{}\n", user.display())));
    let text = std::fs::read_to_string(&user).unwrap();
    assert!(text.starts_with("# added by dippy 20"), "{text}");
    assert!(text.ends_with("\nallow zorp *\n"), "{text}");
    let (code, _, err) = config(&dir, &["add-rule", "set askpass /x"]);
    assert_eq!((code, err.as_str()), (1, "not a rule: set askpass /x\n"));
    let (code, _, err) = config(&dir, &["add-rule", "allow a\nallow *"]);
    assert_eq!((code, err.as_str()), (1, "a rule must be one line\n"));
    std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
    std::fs::create_dir_all(dir.join("repo/x")).unwrap();
    let o = run(
        &dir,
        &[
            "--cwd",
            "repo/x",
            "config",
            "add-rule",
            "--project",
            "deny-redirect /etc/* \"no\"",
        ],
        "",
    );
    assert_eq!(o.status.code(), Some(0));
    let project = std::fs::read_to_string(dir.join("repo/.dippy")).unwrap();
    assert!(
        project.ends_with("\ndeny-redirect /etc/* \"no\"\n"),
        "{project}"
    );
}

#[test]
fn askpass_name_runs_the_dialog_and_denies_without_a_display() {
    let dir = sandbox("askpass");
    let link = dir.join("dippy-askpass");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_dippy-rs"), &link).unwrap();
    let mut child = Command::new(&link)
        .env("HOME", dir.join("home"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"command\": \"rm -rf x\"}")
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    let out = stdout(&o);
    assert!(
        out.starts_with("{\"decision\":\"deny\",\"note\":\"GUI error: "),
        "{out}"
    );
}

#[test]
fn audit_takes_global_config_options() {
    let dir = sandbox("audit");
    std::fs::write(dir.join("config"), "set log audit.log\n").unwrap();
    std::fs::write(
        dir.join("audit.log"),
        "{\"decision\": \"ask\", \"cmd\": \"rm\", \"ts\": \"2026-10-09T08:00:00+00:00\"}\n",
    )
    .unwrap();
    let o = run(
        &dir,
        &["--config-only", "config", "audit", "--group-by", "cmd"],
        "",
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(stdout(&o), "1\trm\n");
}

#[test]
fn hooks_install_and_uninstall_stay_in_the_sandbox() {
    let dir = sandbox("hooks");
    let settings = dir.join(".claude/settings.json");
    let o = run(&dir, &["hooks", "install", "claude"], "");
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(text.contains("\"command\": \"dippy --claude\""), "{text}");
    assert_eq!(
        run(&dir, &["hooks", "uninstall", "claude"], "")
            .status
            .code(),
        Some(0)
    );
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{}");
    // No ~/.claude in the sandbox HOME: a global install refuses.
    let o = run(&dir, &["hooks", "install", "claude", "--global"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(!dir.join("home/.claude").exists());
    let o = run(&dir, &["hooks"], "");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "Error: Please specify an action (list, install, uninstall)\n"
    );
}

fn doctor(dir: &PathBuf, args: &[&str]) -> Output {
    // An empty PATH directory: no `dippy` is found.
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    Command::new(env!("CARGO_BIN_EXE_dippy-rs"))
        .arg("doctor")
        .args(args)
        .env("HOME", dir.join("home"))
        .env("PATH", dir.join("bin"))
        .current_dir(dir)
        .output()
        .unwrap()
}

#[test]
fn doctor_reports_checks_as_json() {
    let dir = sandbox("doctor-json");
    let o = doctor(&dir, &["--json"]);
    assert_eq!(o.status.code(), Some(2), "dippy is not on PATH");
    let report: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(report["overall_status"], "critical");
    assert_eq!(report["checks"][0]["name"], "Installation");
    assert_eq!(report["checks"][0]["message"], "dippy not found on PATH");
    assert_eq!(report["summary"]["critical"], 1);
    assert_eq!(doctor(&dir, &["--quiet"]).stdout.len(), 0);
}

/// The `Logs` check of `dippy doctor --json --verbose`.
fn logs_check(dir: &PathBuf) -> serde_json::Value {
    let o = doctor(dir, &["--json", "--verbose"]);
    let report: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Logs")
        .unwrap()
        .clone()
}

#[test]
fn doctor_checks_only_the_audit_log() {
    let dir = sandbox("doctor-logs");
    let home = dir.join("home");
    // Agent directories without a Dippy log are not checked.
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".gemini"), "a file").unwrap();
    let check = logs_check(&dir);
    assert_eq!(check["status"], "ok", "{check}");
    assert_eq!(check["message"], "Audit log not created yet");
    let default = home.join(".dippy/audit.log");
    assert_eq!(
        check["details"],
        format!("Audit log: {}", default.display())
    );

    std::fs::create_dir_all(home.join(".dippy")).unwrap();
    std::fs::write(&default, vec![b'x'; 11 * 1024 * 1024]).unwrap();
    let check = logs_check(&dir);
    assert_eq!(check["status"], "warning", "{check}");
    assert_eq!(
        check["details"],
        format!(
            "Audit log: {}: log file is 11.0MB (consider rotation)",
            default.display()
        )
    );

    let custom = dir.join("logs/custom.log");
    std::fs::write(
        home.join(".dippy/config"),
        format!("set log {}\n", custom.display()),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("logs")).unwrap();
    std::fs::write(&custom, "{}\n").unwrap();
    let check = logs_check(&dir);
    assert_eq!(check["status"], "ok", "{check}");
    assert_eq!(check["message"], "Audit log is writable");
    assert_eq!(
        check["details"],
        format!("Audit log: {} (0.00MB)", custom.display())
    );
    let text = String::from_utf8(doctor(&dir, &["--verbose"]).stdout).unwrap();
    assert!(text.contains("Logs: Audit log is writable"), "{text}");
    assert!(!text.contains("Log directories"), "{text}");
    assert!(!text.contains("hook-approvals"), "{text}");
}

#[test]
fn doctor_fix_installs_missing_global_hook() {
    let dir = sandbox("doctor-fix");
    std::fs::create_dir_all(dir.join("home/.claude")).unwrap();
    let o = doctor(&dir, &["--fix"]);
    let out = stdout(&o);
    assert!(out.contains("Auto-fixing: Hook: Claude Code\n"), "{out}");
    assert!(
        out.contains("[+] Hook: Claude Code: Installed (global)\n"),
        "{out}"
    );
    let text = std::fs::read_to_string(dir.join("home/.claude/settings.json")).unwrap();
    assert!(text.contains("dippy --claude"), "{text}");
}

#[test]
fn doctor_agent_with_only_a_project_config() {
    let dir = sandbox("doctor-agent");
    std::fs::create_dir_all(dir.join(".claude")).unwrap();
    std::fs::write(dir.join(".claude/settings.json"), "{}").unwrap();
    let o = doctor(&dir, &["--agent", "claude", "--verbose"]);
    let out = stdout(&o);
    assert!(
        out.contains("[?] Claude Code: Issues: Claude Code not installed (no config found)\n"),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "    Project config: {}/.claude/settings.json\n",
            dir.display()
        )),
        "{out}"
    );
    assert_eq!(doctor(&dir, &["--agent", "moltbot"]).status.code(), Some(2));
}

/// `run` with extra environment variables (and nothing else set by Dippy).
fn run_env(dir: &PathBuf, args: &[&str], stdin: &str, env: &[(&str, String)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dippy-rs"));
    command
        .args(args)
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn env_rule_patterns_in_claude_hook_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = sandbox("env-rules");
    let log = dir.join("audit.log");
    let askpass_input = dir.join("askpass.json");
    let askpass = dir.join("fake-askpass");
    std::fs::write(
        &askpass,
        format!("#!/bin/sh\ncat > {}\nexit 2\n", askpass_input.display()),
    )
    .unwrap();
    std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        dir.join("config"),
        format!(
            "set log {}\nallow-edit ${{HCOM_DIR}}/agents/${{HCOM_INSTANCE_NAME}}/**\n\
             ask ${{HCOM_INSTANCE_NAME}}-tool *\n",
            log.display()
        ),
    )
    .unwrap();
    let base = [
        (
            "DIPPY_CONFIG_ONLY",
            dir.join("config").display().to_string(),
        ),
        ("DIPPY_ASKPASS", askpass.display().to_string()),
    ];
    let mut set = base.to_vec();
    set.push(("HCOM_DIR", dir.display().to_string()));
    set.push(("HCOM_INSTANCE_NAME", "bot1".to_string()));
    let write = |path: &str| {
        format!(
            r#"{{"hook_event_name": "PreToolUse", "tool_name": "Write", "cwd": "{}",
            "tool_input": {{"file_path": "{}/agents/{path}", "content": "x"}}}}"#,
            dir.display(),
            dir.display()
        )
    };
    let decision = |o: &Output| -> String {
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_default();
        v["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    let o = run_env(&dir, &["--claude"], &write("bot1/x"), &set);
    assert_eq!(decision(&o), "allow", "{o:?}");
    let o = run_env(&dir, &["--claude"], &write("bot10/x"), &set);
    assert_ne!(decision(&o), "allow", "{o:?}");
    let o = run_env(&dir, &["--claude"], &write("bot1/x"), &base);
    assert_ne!(decision(&o), "allow", "{o:?}");
    // Config warnings are reported by `dippy doctor` only.
    assert!(!dir.join("home/.claude/hook-approvals.log").exists());
    let bash = r#"{"hook_event_name": "PreToolUse", "tool_name": "Bash",
        "tool_input": {"command": "bot1-tool run"}}"#;
    let o = run_env(&dir, &["--claude"], bash, &set);
    assert_ne!(decision(&o), "allow", "{o:?}");
    let rules: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["rule"].as_str().map(str::to_string)
        })
        .collect();
    assert!(
        rules.contains(&"${HCOM_DIR}/agents/${HCOM_INSTANCE_NAME}/**".to_string()),
        "{rules:?}"
    );
    assert!(rules.iter().all(|r| !r.contains("bot1")), "{rules:?}");
    let agy = format!(
        r#"{{"toolCall": {{"name": "run_command", "args": {{"CommandLine": "bot1-tool run",
        "Cwd": "{}"}}}}, "workspacePaths": ["{}"]}}"#,
        dir.display(),
        dir.display()
    );
    let o = run_env(&dir, &["--agy"], &agy, &set);
    assert!(stdout(&o).contains("\"deny\""), "{o:?}");
    let sent = std::fs::read_to_string(&askpass_input).unwrap();
    assert!(sent.contains("${HCOM_INSTANCE_NAME}-tool *"), "{sent}");
}

#[test]
fn doctor_reports_every_faulty_config_line_once() {
    let dir = sandbox("doctor-faulty");
    std::fs::create_dir_all(dir.join("home/.dippy")).unwrap();
    std::fs::write(
        dir.join("home/.dippy/config"),
        "allow fict *\nallow ${DOCTOR_UNSET_A} *\ninclude part\nbogus directive\n",
    )
    .unwrap();
    std::fs::write(dir.join("home/.dippy/part"), "\nallow ${\n").unwrap();
    std::fs::write(dir.join(".dippy"), "ask fict ${DOCTOR_UNSET_B}\n").unwrap();
    let home = dir.join("home/.dippy").canonicalize().unwrap();
    let project = dir.canonicalize().unwrap().join(".dippy");
    let expected = [
        format!(
            "{}: line 2: ${{DOCTOR_UNSET_A}}: unset or empty (rule inactive)",
            home.join("config").display()
        ),
        format!(
            "{}: line 2: unterminated variable reference '${{' (skipped)",
            home.join("part").display()
        ),
        format!(
            "{}: line 4: unknown directive 'bogus' (skipped)",
            home.join("config").display()
        ),
        format!(
            "{}: line 1: ${{DOCTOR_UNSET_B}}: unset or empty (rule inactive)",
            project.display()
        ),
    ];

    let o = doctor(&dir, &["--json"]);
    let report: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let check = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Configuration")
        .unwrap()
        .clone();
    assert_eq!(check["status"], "warning", "{check}");
    assert_eq!(check["message"], "4 faulty config line(s)", "{check}");
    let details = check["details"].as_str().unwrap();
    for line in &expected {
        assert_eq!(
            details.matches(line.as_str()).count(),
            1,
            "{line}\n{details}"
        );
    }
    assert!(
        o.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );

    // Plain output lists the lines without --verbose.
    let o = doctor(&dir, &[]);
    let out = stdout(&o);
    for line in &expected {
        assert!(out.contains(line.as_str()), "{line}\n{out}");
    }
}

#[test]
fn config_add_rule_checks_env_syntax_not_values() {
    let dir = sandbox("add-rule-env");
    let rule = "allow-edit ${ADD_RULE_UNSET}/agents/**";
    let (code, _, err) = config(&dir, &["add-rule", rule]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("${ADD_RULE_UNSET}: unset or empty (rule inactive)"),
        "{err}"
    );
    let text = std::fs::read_to_string(dir.join("home/.dippy/config")).unwrap();
    assert!(text.ends_with(&format!("\n{rule}\n")), "{text}");
    let (code, _, err) = config(&dir, &["add-rule", "allow fict ${"]);
    assert_eq!(
        (code, err.as_str()),
        (
            1,
            "line 1: unterminated variable reference '${' (skipped)\n"
        )
    );
    let (code, _, _) = config(&dir, &["add-rule", "allow fict [${X}]"]);
    assert_eq!(code, 1);
}

#[test]
fn doctor_checks_the_project_config_found_above_cwd() {
    let dir = sandbox("doctor-walk-up");
    std::fs::write(dir.join(".dippy"), "allow ${DOCTOR_UNSET_C} *\n").unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_dippy-rs"))
        .args(["--cwd", "sub", "doctor", "--json"])
        .env("HOME", dir.join("home"))
        .env("PATH", dir.join("bin"))
        .current_dir(&dir)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let check = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Configuration")
        .unwrap()
        .clone();
    assert_eq!(check["status"], "warning", "{check}");
    assert!(
        check["details"]
            .as_str()
            .unwrap()
            .contains("${DOCTOR_UNSET_C}: unset or empty (rule inactive)"),
        "{check}"
    );
}

#[test]
fn invalid_setting_stops_the_agent_in_every_mode() {
    let dir = sandbox("bad-set");
    std::fs::write(dir.join("config"), "allow fict *\nset default bogus\n").unwrap();
    let config = dir.join("config").display().to_string();
    let error = format!(
        "Dippy config error: {config}: line 2: 'default' must be 'allow', 'ask' or 'pass', \
         got 'bogus'. Dippy blocks every tool call until the configuration is fixed: stop work \
         and ask the user to fix it."
    );
    let env = [
        ("DIPPY_CONFIG_ONLY", config.clone()),
        ("DIPPY_ASKPASS", "/bin/true".to_string()),
    ];
    let bash = r#"{"hook_event_name": "PreToolUse", "tool_name": "Bash",
        "tool_input": {"command": "fict x"}}"#;
    let gemini = r#"{"hook_event_name": "BeforeTool", "tool_name": "run_shell_command",
        "tool_input": {"command": "fict x"}}"#;
    let agy = r#"{"toolCall": {"name": "run_command", "args": {"CommandLine": "fict x"}}}"#;
    let pi = r#"{"type": "bash", "command": "fict x", "cwd": "/"}"#;
    for (flag, payload, decision) in [
        ("--claude", bash, "permissionDecision\": \"deny"),
        ("--gemini", gemini, "\"decision\": \"deny\""),
        ("--cursor", r#"{"command": "fict x"}"#, "deny"),
        ("--agy", agy, "\"decision\": \"deny\""),
        ("--pi", pi, "\"action\": \"deny\""),
    ] {
        let o = run_env(&dir, &[flag], payload, &env);
        let out = stdout(&o);
        assert!(out.contains(decision), "{flag}: {out}");
        assert!(out.contains(&error), "{flag}: {out}");
    }
    // Codex: exit 2 blocks the tool, the reason goes to stderr.
    let o = run_env(&dir, &["--codex"], bash, &env);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(String::from_utf8_lossy(&o.stderr).contains(&error), "{o:?}");
    // CLI mode names the same error.
    let o = run_env(&dir, &["--cmd", "fict x"], "", &env);
    assert!(stdout(&o).contains(&error), "{o:?}");
}
