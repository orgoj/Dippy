//! Port of `dippy.execution`: `dippy run` classifies a command string once,
//! asks the askpass program when needed and executes the original string
//! locally.

use std::io::{self, IsTerminal, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::analyzer::{self, Action, Decision};
use crate::config::{self, Config, py_repr};
use crate::hook::{cause_json, py_dumps};
use crate::logging::{Entry, Logs};
use crate::paths;

#[derive(clap::Args)]
pub struct RunArgs {
    /// Quoted Bash command string (default: stdin)
    command: Option<String>,
}

/// The global options a subcommand loads its config with.
pub struct Global<'a> {
    pub cwd: Option<&'a str>,
    pub config: Option<&'a str>,
    pub config_only: Option<&'a str>,
}

/// `_subcommand_config`.
fn load(global: &Global) -> Option<(PathBuf, Config)> {
    let cwd = match global.cwd {
        Some(c) => paths::resolve(Path::new(c)),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
    };
    let loaded = config::load_config(&cwd, global.config, global.config_only);
    config::print_warnings();
    match loaded {
        Ok(config) => Some((cwd, config)),
        Err(e) => {
            eprintln!("{}", config::stop_message(&e));
            None
        }
    }
}

/// `_execution_command`: the argument, else a script read from stdin.
fn command_arg(command: &Option<String>) -> Option<String> {
    let command = command.clone().unwrap_or_else(|| {
        let mut s = String::new();
        let _ = io::stdin().read_to_string(&mut s);
        s
    });
    if command.trim().is_empty() {
        eprintln!("command is required as an argument or on stdin");
        return None;
    }
    Some(command)
}

pub fn run(args: &RunArgs, global: &Global) -> i32 {
    let Some(command) = command_arg(&args.command) else {
        return 1;
    };
    let Some((cwd, config)) = load(global) else {
        return 1;
    };
    execute(&command, &config, &cwd)
}

// ---------------------------------------------------------------- messages

const RETRY_LATER: &str = "Retry later or use another safe solution.";
const REVISE: &str = "Revise the request or use another safe solution.";

fn blocked(command: &str, heading: &str, reason: Option<&str>, guidance: &str) -> String {
    let mut message =
        format!("ERROR: {heading}.\nThe requested command was not executed:\n{command}");
    if let Some(reason) = reason.filter(|r| !r.is_empty()) {
        message += &format!("\n\nReason: {reason}");
    }
    format!("{message}\n\n{guidance}")
}

fn denied(command: &str, note: Option<&str>) -> String {
    blocked(
        command,
        "Approval denied",
        Some(note.unwrap_or("unknown")),
        REVISE,
    )
}

/// Python's `str(OSError)`: `[Errno 2] No such file or directory: 'bash'`.
fn os_error(e: &io::Error, filename: &str) -> String {
    match e.raw_os_error() {
        Some(code) => {
            let text = e.to_string();
            let text = text
                .strip_suffix(&format!(" (os error {code})"))
                .unwrap_or(&text);
            format!("[Errno {code}] {text}: {}", py_repr(filename))
        }
        None => e.to_string(),
    }
}

// --------------------------------------------------------------- processes

enum Ran {
    Exited(ExitStatus, Vec<u8>),
    TimedOut,
}

fn deadline(seconds: f64) -> Option<Instant> {
    Duration::try_from_secs_f64(seconds.max(0.0))
        .ok()
        .and_then(|d| Instant::now().checked_add(d))
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Option<JoinHandle<Vec<u8>>> {
    pipe.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    })
}

/// `subprocess.run(..., input=, timeout=, capture_output=True)`: the child is
/// killed on timeout; stdout is returned and stderr discarded.
fn run_timed(cmd: &mut Command, input: Vec<u8>, seconds: f64) -> io::Result<Ran> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // A separate writer cannot block the timeout loop on a full pipe.
        std::thread::spawn(move || stdin.write_all(&input));
    }
    let stdout = drain(child.stdout.take());
    let _stderr = drain(child.stderr.take());
    let deadline = deadline(seconds);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Ran::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let out = stdout
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    Ok(Ran::Exited(status, out))
}

// ---------------------------------------------------------------- approval

pub struct Approval {
    pub allowed: bool,
    pub note: Option<String>,
}

/// `DIPPY_ASKPASS` (expanded), else the config's `askpass`.
fn askpass_program(config: &Config) -> Option<PathBuf> {
    match std::env::var("DIPPY_ASKPASS")
        .ok()
        .filter(|s| !s.is_empty())
    {
        Some(p) => Some(PathBuf::from(paths::expanduser(&p))),
        None => config.askpass.clone(),
    }
}

/// `approve_with_askpass`: every provider failure denies execution.
fn approve_with_askpass(
    program: Option<&Path>,
    config: &Config,
    command: &str,
    decision: &Decision,
    cwd: &Path,
) -> Approval {
    let refuse = |note: String| Approval {
        allowed: false,
        note: Some(note),
    };
    let Some(program) = program else {
        return refuse(blocked(
            command,
            "Approval unavailable",
            Some("Approval is not configured."),
            RETRY_LATER,
        ));
    };
    let cwd = cwd.to_string_lossy();
    let context_flags: Vec<&String> = decision.context_flags.iter().flatten().collect();
    let payload = py_dumps(&json!({
        "version": 1,
        "command": command,
        "cwd": cwd,
        "decision": decision.action.as_str(),
        "reason": decision.reason,
        "context_flags": context_flags,
        "causes": decision.causes.iter().map(cause_json).collect::<Vec<_>>(),
    }));
    let mut cmd = Command::new(program);
    cmd.arg("Approve command?")
        .env("DIPPY_COMMAND", command)
        .env("DIPPY_CWD", cwd.as_ref())
        .env("DIPPY_RULE", &decision.reason)
        .env("DIPPY_MESSAGE", &decision.reason)
        .env("DIPPY_TOOL", "run")
        .env("DIPPY_ASKPASS_TIMEOUT", config.askpass_timeout.to_string());
    let tty = io::stderr().is_terminal();
    eprint!(
        "STATUS: Approval required; waiting up to {} seconds. \
         The requested command has not started. {}{}",
        config.askpass_timeout,
        config.approval_wait_message,
        if tty { "" } else { "\n" }
    );
    let ran = run_timed(
        &mut cmd,
        payload.into_bytes(),
        config.askpass_timeout as f64,
    );
    if tty {
        eprint!("\r\x1b[2K");
    }
    let (status, stdout) = match ran {
        Ok(Ran::Exited(status, stdout)) => (status, stdout),
        Ok(Ran::TimedOut) => {
            return refuse(blocked(
                command,
                &format!(
                    "Approval timed out after {} seconds",
                    config.askpass_timeout
                ),
                None,
                "Retry the same request when approval can be reviewed, or use \
                 another safe solution.",
            ));
        }
        Err(_) => {
            return refuse(blocked(
                command,
                "Approval failed",
                Some("The approval service is unavailable."),
                RETRY_LATER,
            ));
        }
    };
    let allowed = status.code() == Some(0);
    let stdout = String::from_utf8_lossy(&stdout);
    let mut note = None;
    if !stdout.trim().is_empty()
        && let Ok(Value::Object(response)) = serde_json::from_str::<Value>(&stdout)
    {
        note = response
            .get("note")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        match response.get("decision") {
            None | Some(Value::Null) => {}
            Some(Value::String(d)) if d == "deny" => {
                return refuse(denied(command, note.as_deref()));
            }
            Some(Value::String(d)) if d == "allow" => {}
            Some(_) => {
                let reason = note
                    .as_deref()
                    .unwrap_or("The approval service returned an invalid response.");
                return refuse(blocked(
                    command,
                    "Approval failed",
                    Some(reason),
                    RETRY_LATER,
                ));
            }
        }
    }
    if allowed {
        Approval {
            allowed: true,
            note,
        }
    } else {
        refuse(denied(command, note.as_deref()))
    }
}

/// `set approval dashboard`: the node's queue replaces the askpass call.
/// `None` when the node provably never received the request (use askpass).
fn approve_with_dashboard(
    config: &Config,
    command: &str,
    decision: &Decision,
    cwd: &Path,
) -> Option<(Approval, Option<String>)> {
    let refuse = |note: String| Approval {
        allowed: false,
        note: Some(note),
    };
    let cwd_text = cwd.to_string_lossy();
    let ask = crate::approval::Ask {
        command,
        cwd: &cwd_text,
        tool: Some("run"),
        file_path: None,
        message: &decision.reason,
        agent: "run",
        decision: decision.action.as_str(),
        reason: &decision.reason,
        context_flags: decision.context_flags.iter().flatten().cloned().collect(),
        causes: &decision.causes,
    };
    let seconds = crate::approval::budget_seconds(config);
    let tty = io::stderr().is_terminal();
    let printed = std::cell::Cell::new(false);
    let waiting = || {
        eprint!(
            "STATUS: Approval required; waiting up to {seconds} seconds. \
             The requested command has not started. {}{}",
            config.approval_wait_message,
            if tty { "" } else { "\n" }
        );
        printed.set(true);
    };
    let outcome = crate::approval::wait(config, &ask, &|_| {}, &waiting);
    if tty && printed.get() {
        eprint!("\r\x1b[2K");
    }
    use crate::approval::Outcome;
    let id = outcome.id().map(str::to_string);
    let approval = match outcome {
        Outcome::Fallback(_) => return None,
        Outcome::Allowed { note, .. } => Approval {
            allowed: true,
            note,
        },
        Outcome::Denied { note, .. } => refuse(denied(command, note.as_deref())),
        Outcome::TimedOut { .. } => refuse(blocked(
            command,
            &format!("Approval timed out after {seconds} seconds"),
            None,
            &config.approval_timeout_message,
        )),
        Outcome::Failed { .. } => refuse(blocked(
            command,
            "Approval failed",
            Some("The approval service rejected the request."),
            RETRY_LATER,
        )),
    };
    Some((approval, id))
}

// --------------------------------------------------------------- execution

pub fn execute(command: &str, config: &Config, cwd: &Path) -> i32 {
    let logs = Logs::none("run");
    logs.configure(config);
    let decision = analyzer::analyze(command, config, cwd, None, false);
    logs.decision(Entry {
        decision: decision.action.as_str(),
        cmd: Some(&decision.reason),
        message: Some(&decision.reason),
        command: Some(command),
        cwd: Some(cwd),
        context_flags: decision.context_flags.as_ref(),
        suggestion: decision.suggestion.as_deref(),
        ..Entry::default()
    });
    match decision.action {
        Action::Deny => {
            eprintln!(
                "{}",
                blocked(command, "Execution denied", Some(&decision.reason), REVISE)
            );
            return 1;
        }
        Action::Ask => {
            let dashboard = (config.approval == "dashboard")
                .then(|| approve_with_dashboard(config, command, &decision, cwd))
                .flatten();
            let (approval, id) = dashboard.unwrap_or_else(|| {
                let program = askpass_program(config);
                let approval =
                    approve_with_askpass(program.as_deref(), config, command, &decision, cwd);
                (approval, None)
            });
            logs.decision(Entry {
                decision: if approval.allowed { "allow" } else { "deny" },
                cmd: Some(&decision.reason),
                message: Some(approval.note.as_deref().unwrap_or(&decision.reason)),
                command: Some(command),
                cwd: Some(cwd),
                approval: id.as_deref(),
                ..Entry::default()
            });
            if !approval.allowed {
                eprintln!("{}", approval.note.as_deref().unwrap_or("approval denied"));
                return 1;
            }
        }
        Action::Allow => {}
    }
    match Command::new("/bin/bash")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .status()
    {
        Ok(status) => status
            .code()
            .unwrap_or_else(|| -status.signal().unwrap_or(1)),
        Err(e) => {
            eprintln!("{}", os_error(&e, "/bin/bash"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dippy-rs-run-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn ask(reason: &str) -> Decision {
        let mut d = analyzer::analyze(
            "fictional-tool",
            &Config::default(),
            Path::new("/"),
            None,
            false,
        );
        d.reason = reason.into();
        d
    }

    #[test]
    fn askpass_gets_versioned_json_and_environment() {
        let dir = temp("askpass-json");
        let seen = dir.join("seen");
        let program = script(
            &dir,
            "askpass",
            &format!(
                "{{ printf '%s\\n' \"$1\" \"$DIPPY_COMMAND\" \"$DIPPY_TOOL\"; cat; }} > '{}'\n\
                 echo '{{\"decision\":\"allow\",\"note\":\"ok\"}}'",
                seen.display()
            ),
        );
        let config = Config::default();
        let approval = approve_with_askpass(
            Some(&program),
            &config,
            "frob --write\nnext",
            &ask("unknown"),
            Path::new("/w"),
        );
        assert!(approval.allowed);
        assert_eq!(approval.note.as_deref(), Some("ok"));
        let seen = fs::read_to_string(seen).unwrap();
        assert_eq!(
            seen,
            "Approve command?\nfrob --write\nnext\nrun\n\
             {\"version\": 1, \"command\": \"frob --write\\nnext\", \"cwd\": \"/w\", \
             \"decision\": \"ask\", \"reason\": \"unknown\", \
             \"context_flags\": [], \"causes\": [{\"kind\": \"command\", \
             \"target\": \"fictional-tool\", \"rule\": null, \"flags\": []}]}"
        );
    }

    #[test]
    fn askpass_failures_deny_with_explanations() {
        let dir = temp("askpass-deny");
        let config = Config {
            askpass_timeout: 1,
            ..Config::default()
        };
        let decision = ask("unknown");
        let approve = |program: Option<&Path>| {
            approve_with_askpass(
                program,
                &config,
                "frob --change /x",
                &decision,
                Path::new("/w"),
            )
        };
        let note = |a: Approval| {
            assert!(!a.allowed);
            a.note.unwrap()
        };
        for code in [1, 2, 127] {
            let program = script(&dir, &format!("exit{code}"), &format!("exit {code}"));
            assert!(note(approve(Some(&program))).starts_with("ERROR: Approval denied"));
        }
        let missing = note(approve(None));
        assert!(missing.starts_with("ERROR: Approval unavailable"));
        assert!(missing.contains("The requested command was not executed"));
        let broken = note(approve(Some(&dir.join("missing"))));
        assert!(broken.starts_with("ERROR: Approval failed"));
        let slow = script(&dir, "slow", "sleep 5");
        let timed_out = note(approve(Some(&slow)));
        assert!(timed_out.contains("ERROR: Approval timed out after 1 seconds"));
        assert!(timed_out.contains("Retry the same request"));
        assert!(!timed_out.contains(&slow.to_string_lossy().into_owned()));
        let manual = script(
            &dir,
            "manual",
            "echo '{\"decision\":\"deny\",\"note\":\"Needs manual review\"}'; exit 1",
        );
        let manual = note(approve(Some(&manual)));
        assert!(manual.contains("frob --change /x"));
        assert!(manual.contains("Reason: Needs manual review"));
        let plain = script(&dir, "plain", "echo '{\"decision\":\"deny\"}'; exit 1");
        assert!(note(approve(Some(&plain))).contains("Reason: unknown"));
        let invalid = script(&dir, "invalid", "echo '{\"decision\":\"maybe\"}'");
        assert!(note(approve(Some(&invalid))).contains("returned an invalid response"));
        let refused = script(&dir, "refused", "echo '{\"decision\":\"allow\"}'; exit 1");
        assert!(!approve(Some(&refused)).allowed);
    }

    #[test]
    fn deny_rule_never_executes() {
        let dir = temp("deny");
        let config = config::parse_config("deny frob-local", None).unwrap();
        let marker = dir.join("ran");
        let command = format!("frob-local; touch '{}'", marker.display());
        assert_eq!(execute(&command, &config, &dir), 1);
        assert!(!marker.exists());
    }

    #[test]
    fn local_allow_runs_original_string_in_cwd() {
        let dir = temp("local");
        let config = config::parse_config(
            "allow printf *\nallow fictional-exit\nallow-redirect *",
            None,
        )
        .unwrap();
        let command = "printf '%s' \"$PWD\" > out; fictional-exit";
        // Bash's exit status for an unknown command.
        assert_eq!(execute(command, &config, &dir), 127);
        assert_eq!(
            fs::read_to_string(dir.join("out")).unwrap(),
            dir.to_string_lossy()
        );
    }
}
