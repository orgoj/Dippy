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
