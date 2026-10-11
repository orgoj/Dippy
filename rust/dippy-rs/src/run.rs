//! Port of `dippy.execution` and `dippy.ssh_transport`: `dippy run`,
//! `run-on-server` and `recover` classify a command string once, ask the
//! askpass program when needed and execute the original string locally or on
//! an allowed server. Run state files stay compatible with Python Dippy.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::analyzer::{self, Action, Decision};
use crate::config::{self, Config, Flags, SERVER_ALIAS_RE, py_repr};
use crate::hook::{cause_json, py_dumps};
use crate::logging::{Entry, Logs};
use crate::paths;

#[derive(clap::Args)]
pub struct RunArgs {
    /// Quoted Bash command string (default: stdin)
    command: Option<String>,
}

#[derive(clap::Args)]
pub struct RunOnServerArgs {
    /// Allowed SSH-config alias
    server: String,
    /// Quoted Bash command string (default: stdin)
    command: Option<String>,
}

#[derive(clap::Args)]
pub struct RecoverArgs {
    server: String,
    /// Explicitly release the target
    #[arg(long)]
    clear: bool,
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
    execute(&paths::home(), &command, &config, &cwd, None)
}

pub fn run_on_server(args: &RunOnServerArgs, global: &Global) -> i32 {
    if let Err(e) = config::validate_server(&args.server) {
        eprintln!("{e}");
        return 1;
    }
    let Some(command) = command_arg(&args.command) else {
        return 1;
    };
    let Some((cwd, config)) = load(global) else {
        return 1;
    };
    execute(&paths::home(), &command, &config, &cwd, Some(&args.server))
}

pub fn recover(args: &RecoverArgs, global: &Global) -> i32 {
    let Some((cwd, config)) = load(global) else {
        return 1;
    };
    recover_target(&paths::home(), &args.server, &config, args.clear, &cwd)
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

/// Python's `str(OSError)`: `[Errno 2] No such file or directory: 'ssh'`.
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

fn os_path(path: &Path) -> impl Fn(io::Error) -> String + '_ {
    move |e| os_error(&e, &path.to_string_lossy())
}

// --------------------------------------------------------------- processes

/// Set by Ctrl-C while a remote backend runs (Python's `KeyboardInterrupt`).
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

enum Ran {
    Exited(ExitStatus, Vec<u8>),
    TimedOut,
    Interrupted,
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

/// `subprocess.run(..., input=, timeout=)`: the child is killed on timeout
/// or Ctrl-C. With `capture`, stdout is returned and stderr discarded.
fn run_timed(
    cmd: &mut Command,
    input: Option<Vec<u8>>,
    seconds: f64,
    capture: bool,
) -> io::Result<Ran> {
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let mut child = cmd.spawn()?;
    if let (Some(mut stdin), Some(input)) = (child.stdin.take(), input) {
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
        let ran = if INTERRUPTED.load(Ordering::SeqCst) {
            Ran::Interrupted
        } else if deadline.is_some_and(|d| Instant::now() >= d) {
            Ran::TimedOut
        } else {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        let _ = child.kill();
        let _ = child.wait();
        return Ok(ran);
    };
    let out = stdout
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    Ok(Ran::Exited(status, out))
}

/// Why a remote backend has no reliable result.
#[derive(Debug, PartialEq)]
enum Fail {
    /// Python's `TimeoutError`/`KeyboardInterrupt`: no message is shown.
    Uncertain,
    /// Python's `OSError`/`RuntimeError`/`ValueError`.
    Error(String),
}

/// `_run_checked` with captured output: stdout of a successful command.
fn run_checked(argv: &[&str]) -> Result<String, Fail> {
    let out = Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .map_err(|e| Fail::Error(os_error(&e, argv[0])))?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr);
        let detail = if detail.is_empty() {
            "command failed"
        } else {
            &detail
        };
        return Err(Fail::Error(format!(
            "{} failed: {}",
            argv[0],
            detail.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
    server: Option<&str>,
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
    // The dialog re-analyses the command as here: same flags and remoteness.
    let payload = py_dumps(&json!({
        "version": 1,
        "command": command,
        "cwd": cwd,
        "server": server,
        "decision": decision.action.as_str(),
        "reason": decision.reason,
        "context_flags": context_flags,
        "flags": server.map(server_flags).unwrap_or_default(),
        "remote": server.is_some(),
        "causes": decision.causes.iter().map(cause_json).collect::<Vec<_>>(),
    }));
    let prompt = match server {
        Some(s) => format!("Approve command on {s}?"),
        None => "Approve command?".to_string(),
    };
    let mut cmd = Command::new(program);
    cmd.arg(prompt)
        .env("DIPPY_COMMAND", command)
        .env("DIPPY_CWD", cwd.as_ref())
        .env("DIPPY_RULE", &decision.reason)
        .env("DIPPY_MESSAGE", &decision.reason)
        .env(
            "DIPPY_TOOL",
            if server.is_some() {
                "run-on-server"
            } else {
                "run"
            },
        )
        .env("DIPPY_SERVER", server.unwrap_or(""))
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
        Some(payload.into_bytes()),
        config.askpass_timeout as f64,
        true,
    );
    if tty {
        eprint!("\r\x1b[2K");
    }
    let (status, stdout) = match ran {
        Ok(Ran::Exited(status, stdout)) => (status, stdout),
        Ok(_) => {
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
    server: Option<&str>,
    cwd: &Path,
) -> Option<(Approval, Option<String>)> {
    let refuse = |note: String| Approval {
        allowed: false,
        note: Some(note),
    };
    let cwd_text = cwd.to_string_lossy();
    let agent = if server.is_some() {
        "run-on-server"
    } else {
        "run"
    };
    let ask = crate::approval::Ask {
        command,
        cwd: &cwd_text,
        tool: Some(agent),
        file_path: None,
        message: &decision.reason,
        agent,
        server,
        decision: decision.action.as_str(),
        reason: &decision.reason,
        context_flags: decision.context_flags.iter().flatten().cloned().collect(),
        flags: server
            .map(server_flags)
            .unwrap_or_default()
            .into_iter()
            .collect(),
        remote: server.is_some(),
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

// ----------------------------------------------------------------- markers

fn marker_re(pattern: &str, marker: &str) -> Regex {
    Regex::new(&pattern.replace("{m}", &regex::escape(marker))).expect("marker pattern")
}

pub fn parse_marker(output: &str, marker: &str) -> Result<i32, String> {
    marker_re(r"(?m)(?:^|\n)DIPPY_END_{m}:(\d+)\r?$", marker)
        .captures(output)
        .and_then(|c| c[1].parse::<i64>().ok())
        .map(|code| code as i32)
        .ok_or_else(|| format!("missing completion marker DIPPY_END_{marker}"))
}

fn new_marker() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ------------------------------------------------------------------- state

type State = Map<String, Value>;

const STATUSES: [&str; 5] = ["running", "indeterminate", "complete", "cleared", "ready"];

fn state_dir(home: &Path) -> PathBuf {
    home.join(".dippy").join("run-state")
}

/// `mkdir(parents=True, exist_ok=True, mode=0o700)`: only the last
/// directory gets the mode.
fn make_state_dir(home: &Path) -> Result<PathBuf, String> {
    let dir = state_dir(home);
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent).map_err(os_path(parent))?;
    }
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists || !dir.is_dir() => {
            Err(os_error(&e, &dir.to_string_lossy()))
        }
        _ => Ok(dir),
    }
}

/// `_write_state`: sorted keys, written through a temporary file.
fn write_state(home: &Path, name: &str, state: &State) -> Result<(), String> {
    let dir = make_state_dir(home)?;
    let sorted: BTreeMap<&String, &Value> = state.iter().collect();
    let sorted: State = sorted
        .into_iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let path = dir.join(format!("{name}.json"));
    let temporary = dir.join(format!("{name}.tmp"));
    fs::write(&temporary, py_dumps(&Value::Object(sorted))).map_err(os_path(&temporary))?;
    fs::rename(&temporary, &path).map_err(os_path(&temporary))
}

fn read_state(home: &Path, name: &str) -> Result<State, String> {
    let path = state_dir(home).join(format!("{name}.json"));
    let cannot = |detail: String| format!("cannot read execution state {name}: {detail}");
    let text = match fs::read(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(State::new()),
        Err(e) => return Err(cannot(os_error(&e, &path.to_string_lossy()))),
        Ok(text) => text,
    };
    match serde_json::from_slice(&text).map_err(|e| cannot(e.to_string()))? {
        Value::Object(state)
            if state
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|s| STATUSES.contains(&s)) =>
        {
            Ok(state)
        }
        _ => Err(format!("invalid execution state: {name}")),
    }
}

fn pending(state: &State) -> bool {
    matches!(
        state.get("status").and_then(Value::as_str),
        Some("running" | "indeterminate")
    )
}

/// `{**state, key: value, ...}`.
fn with(mut state: State, entries: &[(&str, Value)]) -> State {
    for (key, value) in entries {
        state.insert(key.to_string(), value.clone());
    }
    state
}

/// `_server_lock`: held until the returned file is dropped.
fn server_lock(home: &Path, name: &str) -> Result<File, String> {
    let path = make_state_dir(home)?.join(format!("{name}.lock"));
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(os_path(&path))?;
    file.lock().map_err(os_path(&path))?;
    Ok(file)
}

/// `_operation_key`: nested working directories share their nearest
/// project configuration.
fn operation_key(cwd: &Path, server: &str) -> String {
    let resolved = paths::resolve(cwd);
    let project = resolved
        .ancestors()
        .find(|d| d.join(".dippy").is_file() || d.join(".git").exists())
        .unwrap_or(&resolved);
    let digest = Sha256::digest(paths::py_path_str(project).as_bytes());
    format!("{server}-{}", &hex(&digest)[..24])
}

// --------------------------------------------------------------- transport

/// `SSHTransport`.
struct Transport {
    argv: Vec<String>,
    env: BTreeMap<OsString, OsString>,
    cwd: PathBuf,
    identity: String,
}

/// Quote one literal POSIX shell argument without interpreting its contents.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Python's `repr(list_of_str)`.
fn py_list_repr(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|s| py_repr(s)).collect();
    format!("[{}]", items.join(", "))
}

/// OpenSSH expands these even in quoted option arguments. Reject rather than
/// accidentally selecting another socket or environment-derived location.
fn option_path(value: &str) -> Result<String, String> {
    if value.contains(['%', '$', '"', '\\', '\n', '\r', '\0']) {
        return Err("SSH socket path contains unsupported OpenSSH expansion characters".into());
    }
    Ok(format!("\"{value}\""))
}

fn private_control_path() -> Result<String, String> {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let directory = std::env::temp_dir().join(format!("dippy-ssh-{uid}"));
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => {
            return Err(os_error(&e, &directory.to_string_lossy()));
        }
        _ => {}
    }
    let metadata = fs::symlink_metadata(&directory).map_err(os_path(&directory))?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
        return Err(format!(
            "unsafe SSH control directory: {}",
            directory.display()
        ));
    }
    let path = directory.join(new_marker().map_err(|e| e.to_string())?);
    if path.as_os_str().len() > 100 {
        return Err(
            "SSH control socket path is too long; use a shorter temporary directory".into(),
        );
    }
    option_path(&path.to_string_lossy())
}

const PROFILE_OPTIONS: [&str; 10] = [
    "IdentityFile=none",
    "IdentityAgent=",
    "IdentitiesOnly=yes",
    "BatchMode=yes",
    "ForwardAgent=no",
    "ControlPath=none",
    "ControlPersist=no",
    "PKCS11Provider=none",
    "PreferredAuthentications=publickey",
    "PermitLocalCommand=no",
];

/// `build_transport`: resolve an optional profile before any approved
/// command is launched.
fn build_transport(config: &Config, server: &str, cwd: &Path) -> Result<Transport, String> {
    let mut env: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    let profile = config.run_on_server_ssh_config.as_ref();
    let socket = config.run_on_server_ssh_auth_sock.as_deref();
    let mut argv = vec!["ssh".to_string()];
    let destination = ["--", server, "bash", "-s"].map(String::from);
    let configured = profile.is_some()
        || config
            .configured_settings
            .contains("run_on_server_ssh_config");
    if (!configured && socket.is_none())
        || (configured && profile.is_none() && socket == Some("none"))
    {
        argv.extend(destination);
        return Ok(Transport {
            argv,
            env,
            cwd: cwd.to_path_buf(),
            identity: "user".into(),
        });
    }
    let (Some(profile), Some(socket)) = (profile, socket) else {
        return Err("SSH profile requires both ssh-config and ssh-auth-sock (or none)".into());
    };
    let profile = std::path::absolute(profile).map_err(|e| e.to_string())?;
    if !profile.is_file() {
        return Err(format!("SSH config does not exist: {}", profile.display()));
    }
    // Fail before execution when the config cannot be read.
    fs::read(&profile).map_err(os_path(&profile))?;
    let agent = if socket == "none" {
        env.remove(OsStr::new("SSH_AUTH_SOCK"));
        "none".to_string()
    } else {
        let agent = option_path(socket)?;
        let metadata = fs::metadata(socket).map_err(|e| os_error(&e, socket))?;
        if !metadata.file_type().is_socket() {
            return Err(format!("SSH auth socket is not a Unix socket: {socket}"));
        }
        env.insert("SSH_AUTH_SOCK".into(), socket.into());
        agent
    };
    env.remove(OsStr::new("SSH_AGENT_PID"));
    let profile = profile.to_string_lossy().into_owned();
    argv.extend(["-F".to_string(), profile.clone()]);
    for option in PROFILE_OPTIONS {
        let option = if option == "IdentityAgent=" {
            format!("{option}{agent}")
        } else {
            option.to_string()
        };
        argv.extend(["-o".to_string(), option]);
    }
    let probe_argv: Vec<String> = argv
        .iter()
        .cloned()
        .chain(["-G".to_string()])
        .chain(destination.iter().cloned())
        .collect();
    let mut probe = Command::new(&probe_argv[0]);
    probe
        .args(&probe_argv[1..])
        .current_dir(cwd)
        .env_clear()
        .envs(&env);
    let (status, stdout, stderr) = match spawn_probe(&mut probe) {
        Err(e) => return Err(os_error(&e, "ssh")),
        Ok(None) => {
            return Err(format!(
                "Command '{}' timed out after 10 seconds",
                py_list_repr(&probe_argv)
            ));
        }
        Ok(Some(result)) => result,
    };
    if !status.success() {
        return Err(format!("SSH profile validation failed: {}", stderr.trim()));
    }
    let mut options: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for line in stdout.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        options
            .entry(key.to_lowercase())
            .or_default()
            .push(value.trim());
    }
    let any_set = |key: &str| {
        options
            .get(key)
            .is_some_and(|v| v.iter().any(|v| *v != "none"))
    };
    if !any_set("identityfile") {
        return Err(
            "SSH profile must explicitly select an IdentityFile (a public key is sufficient)"
                .into(),
        );
    }
    if any_set("proxyjump") {
        return Err(
            "Isolated SSH profiles do not support ProxyJump; use an explicitly isolated ProxyCommand"
                .into(),
        );
    }
    let identity = hex(&Sha256::digest(
        format!("{profile}\0{socket}\0{stdout}").as_bytes(),
    ));
    // Honor the profile's ControlMaster mode, but never attach to
    // authentication cached by another operation (including an earlier
    // version of this profile). No persistent master is left afterwards.
    let control = argv
        .iter()
        .position(|a| a == "ControlPath=none")
        .expect("option");
    argv[control] = format!("ControlPath={}", private_control_path()?);
    argv.extend(destination);
    Ok(Transport {
        argv,
        env,
        cwd: cwd.to_path_buf(),
        identity,
    })
}

/// The `ssh -G` probe (10 s timeout): status, stdout, stderr; `None` on timeout.
fn spawn_probe(cmd: &mut Command) -> io::Result<Option<(ExitStatus, String, String)>> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = deadline(10.0);
    loop {
        if let Some(status) = child.try_wait()? {
            let text = |h: Option<JoinHandle<Vec<u8>>>| {
                String::from_utf8_lossy(
                    &h.map(|h| h.join().unwrap_or_default()).unwrap_or_default(),
                )
                .into_owned()
            };
            return Ok(Some((status, text(stdout), text(stderr))));
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn shell_payload(command: &str, marker: &str) -> String {
    format!(
        "printf 'DIPPY_START_{marker}\\n'; printf %s {} | base64 -d | bash; \
         dippy_status=$?; printf '\\nDIPPY_END_{marker}:%s\\n' \"$dippy_status\"",
        base64(command.as_bytes())
    )
}

/// The user command is data on SSH stdin, never code in the local pane.
fn transport_payload(transport: &Transport, command: &str, marker: &str) -> String {
    let encoded = base64(shell_payload(command, marker).as_bytes());
    let mut environment: Vec<String> = ["env", "-u", "SSH_AUTH_SOCK", "-u", "SSH_AGENT_PID"]
        .map(String::from)
        .into();
    for name in ["SSH_AUTH_SOCK", "SSH_AGENT_PID"] {
        if let Some(value) = transport.env.get(OsStr::new(name)) {
            environment.push(format!("{name}={}", value.to_string_lossy()));
        }
    }
    let argv: Vec<String> = environment
        .iter()
        .chain(&transport.argv)
        .map(|a| shell_quote(a))
        .collect();
    let script = format!(
        "cd -- {} && printf %s {encoded} | base64 -d | {}; \
         printf '\\nDIPPY_TRANSPORT_{marker}:%s\\n' \"$?\"",
        shell_quote(&transport.cwd.to_string_lossy()),
        argv.join(" ")
    );
    format!("/bin/bash --noprofile --norc -c {}", shell_quote(&script))
}

// ---------------------------------------------------------------- backends

fn run_ssh(transport: &Transport, command: &str, timeout: f64) -> Result<i32, Fail> {
    let mut cmd = Command::new(&transport.argv[0]);
    cmd.args(&transport.argv[1..])
        .env_clear()
        .envs(&transport.env)
        .current_dir(&transport.cwd);
    match run_timed(&mut cmd, Some(command.as_bytes().to_vec()), timeout, false) {
        Err(e) => Err(Fail::Error(os_error(&e, &transport.argv[0]))),
        Ok(Ran::TimedOut | Ran::Interrupted) => Err(Fail::Uncertain),
        // Exit 255 or a signal: the connection ended without a reliable result.
        Ok(Ran::Exited(status, _)) => match status.code() {
            Some(255) | None => Err(Fail::Uncertain),
            Some(code) => Ok(code),
        },
    }
}

/// `_poll_capture`: wait for the marker and print the captured output.
fn poll_capture(
    mut capture: impl FnMut() -> Result<String, Fail>,
    marker: &str,
    timeout: f64,
    interval: f64,
) -> Result<i32, Fail> {
    let transport = marker_re(r"(?m)(?:^|\n)DIPPY_TRANSPORT_{m}:\d+\r?$", marker);
    let start = marker_re(r"(?:^|\n)DIPPY_START_{m}\r?\n", marker);
    let end = marker_re(r"(?m)(?:^|\n)DIPPY_END_{m}:\d+\r?$", marker);
    let interval = Duration::try_from_secs_f64(interval.max(0.0)).unwrap_or(Duration::ZERO);
    let deadline = deadline(timeout);
    while deadline.is_none_or(|d| Instant::now() < d) {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return Err(Fail::Uncertain);
        }
        let last = capture()?;
        let Ok(code) = parse_marker(&last, marker) else {
            if transport.is_match(&last) {
                // SSH ended without a remote completion marker.
                return Err(Fail::Uncertain);
            }
            std::thread::sleep(interval);
            continue;
        };
        if let Some(end) = end.find(&last) {
            let from = start
                .find(&last)
                .filter(|s| end.start() >= s.end())
                .map_or(0, |s| s.end());
            let output = &last[from..end.start()];
            if !output.is_empty() {
                let newline = if output.ends_with('\n') { "" } else { "\n" };
                print!("{output}{newline}");
                let _ = io::stdout().flush();
            }
        }
        return Ok(code);
    }
    Err(Fail::Uncertain)
}

fn session(config: &Config) -> Result<&str, Fail> {
    let session = config.run_on_server_session.as_str();
    if SERVER_ALIAS_RE.is_match(session) {
        Ok(session)
    } else {
        Err(Fail::Error("invalid run-on-server session name".into()))
    }
}

static PANE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^%\d+$").unwrap());

/// `{"status": "ready", **state, pane_key: pane}`.
fn record_pane(home: &Path, name: &str, pane_key: &str, pane: &str) -> Result<(), Fail> {
    let mut state = State::new();
    state.insert("status".into(), json!("ready"));
    state.extend(read_state(home, name).map_err(Fail::Error)?);
    state.insert(pane_key.into(), json!(pane));
    write_state(home, name, &state).map_err(Fail::Error)
}

fn ensure_tmux(home: &Path, config: &Config, name: &str) -> Result<String, Fail> {
    let session = session(config)?;
    let target = format!("={session}");
    let exists = Command::new("tmux")
        .args(["has-session", "-t", &target])
        .output()
        .map_err(|e| Fail::Error(os_error(&e, "tmux")))?;
    let mut argv = if exists.status.success() {
        vec!["tmux", "new-window", "-d", "-t", &target]
    } else {
        vec!["tmux", "new-session", "-d", "-s", session]
    };
    argv.extend([
        "-n",
        name,
        "-P",
        "-F",
        "#{pane_id}",
        "/bin/bash --noprofile --norc",
    ]);
    let pane = run_checked(&argv)?.trim().to_string();
    if !PANE_RE.is_match(&pane) {
        return Err(Fail::Error("tmux did not return a concrete pane ID".into()));
    }
    record_pane(home, name, "tmux_pane", &pane)?;
    Ok(pane)
}

fn tmux_send(target: &str, text: &str) -> Result<(), Fail> {
    run_checked(&["tmux", "send-keys", "-t", target, "-l", text])?;
    run_checked(&["tmux", "send-keys", "-t", target, "Enter"])?;
    Ok(())
}

fn tmux_capture(target: &str) -> Result<String, Fail> {
    run_checked(&["tmux", "capture-pane", "-p", "-S", "-", "-t", target])
}

fn herdr(config: &Config, args: &[&str]) -> Result<String, Fail> {
    let session = session(config)?;
    let argv: Vec<&str> = ["herdr", "--session", session]
        .into_iter()
        .chain(args.iter().copied())
        .collect();
    run_checked(&argv)
}

fn ensure_herdr(home: &Path, config: &Config, name: &str) -> Result<String, Fail> {
    let label = format!("dippy-{name}");
    let created = herdr(
        config,
        &["workspace", "create", "--label", &label, "--no-focus"],
    )?;
    let invalid = || Fail::Error("Herdr returned invalid JSON".into());
    let data: Value = serde_json::from_str(&created).map_err(|_| invalid())?;
    if !data.is_object() {
        return Err(invalid());
    }
    let Some(pane) = data
        .get("result")
        .and_then(|r| r.get("root_pane"))
        .and_then(|r| r.get("pane_id"))
        .and_then(Value::as_str)
    else {
        return Err(Fail::Error(
            "Herdr create response omitted root pane".into(),
        ));
    };
    herdr(
        config,
        &["pane", "run", pane, "exec /bin/bash --noprofile --norc"],
    )?;
    record_pane(home, name, "herdr_pane", pane)?;
    Ok(pane.to_string())
}

fn herdr_capture(config: &Config, pane: &str) -> Result<String, Fail> {
    herdr(
        config,
        &[
            "pane",
            "read",
            pane,
            "--source",
            "recent-unwrapped",
            "--lines",
            "10000",
        ],
    )
}

/// The terminal backends: wait for a ready shell, then send the transport.
fn run_terminal(
    config: &Config,
    send: impl Fn(&str) -> Result<(), Fail>,
    capture: impl Fn() -> Result<String, Fail>,
    command: &str,
    marker: &str,
    transport: &Transport,
) -> Result<i32, Fail> {
    let timeout = config.run_on_server_timeout;
    let interval = config.run_on_server_poll_interval;
    let ready = new_marker().map_err(|e| Fail::Error(e.to_string()))?;
    send(&format!("printf 'DIPPY_END_{ready}:0\\n'"))?;
    poll_capture(&capture, &ready, timeout, interval)?;
    send(&transport_payload(transport, command, marker))?;
    poll_capture(&capture, marker, timeout, interval)
}

// --------------------------------------------------------------- execution

/// The context flags of a command run on `server`.
fn server_flags(server: &str) -> Flags {
    ["run-on-server", server].map(String::from).into()
}

/// `execute`: classify exactly once, approve if needed, and execute the
/// original string.
pub fn execute(
    home: &Path,
    command: &str,
    config: &Config,
    cwd: &Path,
    server: Option<&str>,
) -> i32 {
    let agent = if server.is_some() {
        "run-on-server"
    } else {
        "run"
    };
    let logs = Logs::none(agent);
    logs.configure(config);
    let decision = match server {
        Some(server) => {
            if let Err(e) = config::validate_server(server) {
                eprintln!("{e}");
                return 1;
            }
            if !config.servers.iter().any(|s| s == server) {
                eprintln!("server is not allowed: {server}");
                return 1;
            }
            analyzer::analyze(command, config, cwd, Some(&server_flags(server)), true)
        }
        None => analyzer::analyze(command, config, cwd, None, false),
    };
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
                .then(|| approve_with_dashboard(config, command, &decision, server, cwd))
                .flatten();
            let (approval, id) = dashboard.unwrap_or_else(|| {
                let program = askpass_program(config);
                let approval = approve_with_askpass(
                    program.as_deref(),
                    config,
                    command,
                    &decision,
                    server,
                    cwd,
                );
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
    let Some(server) = server else {
        return match Command::new("/bin/bash")
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
        };
    };
    execute_remote(home, command, config, cwd, server).unwrap_or_else(|e| {
        eprintln!("remote execution unavailable: {e}");
        1
    })
}

fn execute_remote(
    home: &Path,
    command: &str,
    config: &Config,
    cwd: &Path,
    server: &str,
) -> Result<i32, String> {
    let key = operation_key(cwd, server);
    // Honor old versions' alias-only records under their lock during migration.
    let _server_lock = server_lock(home, server)?;
    let _key_lock = server_lock(home, &key)?;
    let legacy = read_state(home, server)?;
    let state = read_state(home, &key)?;
    if pending(&legacy) || pending(&state) {
        eprintln!("previous command on {server} is indeterminate; recover it before retrying");
        return Ok(1);
    }
    let transport = build_transport(config, server, cwd)?;
    let marker = new_marker().map_err(|e| e.to_string())?;
    let backend = config.run_on_server_backend.as_str();
    let running = with(
        State::new(),
        &[
            ("status", json!("running")),
            ("marker", json!(marker)),
            ("backend", json!(backend)),
            ("session", json!(config.run_on_server_session)),
            ("profile", json!(transport.identity)),
        ],
    );
    write_state(home, &key, &running)?;
    INTERRUPTED.store(false, Ordering::SeqCst);
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGINT, on_sigint as *const () as libc::sighandler_t);
    }
    let result = match backend {
        "ssh" => run_ssh(&transport, command, config.run_on_server_timeout),
        "tmux" => ensure_tmux(home, config, &key).and_then(|pane| {
            run_terminal(
                config,
                |text| tmux_send(&pane, text),
                || tmux_capture(&pane),
                command,
                &marker,
                &transport,
            )
        }),
        "herdr" => ensure_herdr(home, config, &key).and_then(|pane| {
            run_terminal(
                config,
                |text| herdr(config, &["pane", "run", &pane, text]).map(drop),
                || herdr_capture(config, &pane),
                command,
                &marker,
                &transport,
            )
        }),
        other => Err(Fail::Error(format!("unsupported backend: {other}"))),
    };
    let state = read_state(home, &key)?;
    match result {
        Ok(code) => {
            let state = with(
                state,
                &[("status", json!("complete")), ("exit_code", json!(code))],
            );
            write_state(home, &key, &state)?;
            Ok(code)
        }
        Err(fail) => {
            let mut entries = vec![
                ("status", json!("indeterminate")),
                ("marker", json!(marker)),
            ];
            if let Fail::Error(e) = &fail {
                entries.push(("error", json!(e)));
            }
            write_state(home, &key, &with(state, &entries))?;
            match fail {
                Fail::Uncertain => {
                    eprintln!("command status on {server} is INDETERMINATE; it was not retried")
                }
                Fail::Error(e) => eprintln!("command status on {server} is INDETERMINATE: {e}"),
            }
            Ok(125)
        }
    }
}

/// `recover`: recover only the project and connection that started the
/// operation.
pub fn recover_target(home: &Path, server: &str, config: &Config, clear: bool, cwd: &Path) -> i32 {
    recover_inner(home, server, config, clear, cwd).unwrap_or_else(|e| {
        eprintln!("{e}");
        1
    })
}

/// `_recover`: resolve or explicitly clear a target's indeterminate state.
fn recover_inner(
    home: &Path,
    server: &str,
    config: &Config,
    clear: bool,
    cwd: &Path,
) -> Result<i32, String> {
    config::validate_server(server)?;
    if !config.servers.iter().any(|s| s == server) {
        eprintln!("server is not allowed: {server}");
        return Ok(1);
    }
    let key = operation_key(cwd, server);
    let _server_lock = server_lock(home, server)?;
    let _key_lock = server_lock(home, &key)?;
    let legacy = read_state(home, server)?;
    if pending(&legacy) {
        if !clear {
            eprintln!("legacy execution state requires manual inspection and --clear");
            return Ok(1);
        }
        write_state(home, server, &with(legacy, &[("status", json!("cleared"))]))?;
    }
    let state = read_state(home, &key)?;
    if !pending(&state) {
        return Ok(0);
    }
    if clear {
        write_state(home, &key, &with(state, &[("status", json!("cleared"))]))?;
        return Ok(0);
    }
    let Some(marker) = state.get("marker").and_then(Value::as_str) else {
        eprintln!("indeterminate state has no recovery marker");
        return Ok(1);
    };
    if state.get("backend") != Some(&json!(config.run_on_server_backend))
        || state.get("session") != Some(&json!(config.run_on_server_session))
    {
        eprintln!(
            "configured backend/session changed; restore it or inspect the server and use --clear"
        );
        return Ok(1);
    }
    if state.get("profile") != Some(&json!(build_transport(config, server, cwd)?.identity)) {
        eprintln!("SSH profile changed; restore it or inspect the server and use --clear");
        return Ok(1);
    }
    let captured = match config.run_on_server_backend.as_str() {
        "tmux" => match state.get("tmux_pane").and_then(Value::as_str) {
            Some(pane) if PANE_RE.is_match(pane) => tmux_capture(pane),
            _ => Err(Fail::Error(
                "indeterminate tmux state has no concrete pane".into(),
            )),
        },
        "herdr" => match state.get("herdr_pane").and_then(Value::as_str) {
            Some(pane) => herdr_capture(config, pane),
            None => {
                eprintln!("indeterminate Herdr state has no pane");
                return Ok(1);
            }
        },
        _ => {
            eprintln!(
                "SSH state cannot be recovered automatically; inspect the server and use --clear"
            );
            return Ok(1);
        }
    };
    let code = captured
        .map_err(|fail| match fail {
            Fail::Error(e) => e,
            Fail::Uncertain => "interrupted".to_string(),
        })
        .and_then(|output| parse_marker(&output, marker));
    let code = match code {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            return Ok(1);
        }
    };
    let state = with(
        state,
        &[("status", json!("complete")), ("exit_code", json!(code))],
    );
    write_state(home, &key, &state)?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn servers(names: &[&str]) -> Config {
        Config {
            servers: names.iter().map(|s| s.to_string()).collect(),
            ..Config::default()
        }
    }

    #[test]
    fn askpass_gets_versioned_json_and_environment() {
        let dir = temp("askpass-json");
        let seen = dir.join("seen");
        let program = script(
            &dir,
            "askpass",
            &format!(
                "{{ printf '%s\\n' \"$1\" \"$DIPPY_COMMAND\" \"$DIPPY_TOOL\" \"$DIPPY_SERVER\"; cat; }} > '{}'\n\
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
            Some("host-1"),
            Path::new("/w"),
        );
        assert!(approval.allowed);
        assert_eq!(approval.note.as_deref(), Some("ok"));
        let seen = fs::read_to_string(seen).unwrap();
        assert_eq!(
            seen,
            "Approve command on host-1?\nfrob --write\nnext\nrun-on-server\nhost-1\n\
             {\"version\": 1, \"command\": \"frob --write\\nnext\", \"cwd\": \"/w\", \
             \"server\": \"host-1\", \"decision\": \"ask\", \"reason\": \"unknown\", \
             \"context_flags\": [], \"flags\": [\"host-1\", \"run-on-server\"], \
             \"remote\": true, \"causes\": [{\"kind\": \"command\", \
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
                None,
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
        let config =
            config::parse_config("server srv\ndeny [run-on-server] frob --erase|", None).unwrap();
        let home = dir.join("home");
        assert_eq!(
            execute(&home, "frob --erase", &config, &dir, Some("srv")),
            1
        );
        assert!(!state_dir(&home).exists());
        let config = config::parse_config("deny frob-local", None).unwrap();
        let marker = dir.join("ran");
        let command = format!("frob-local; touch '{}'", marker.display());
        assert_eq!(execute(&home, &command, &config, &dir, None), 1);
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
        assert_eq!(execute(&dir, command, &config, &dir, None), 127);
        assert_eq!(
            fs::read_to_string(dir.join("out")).unwrap(),
            dir.to_string_lossy()
        );
    }

    #[test]
    fn server_must_be_valid_and_allowed() {
        for server in [
            "",
            "-oProxyCommand=x",
            "user@host",
            "host name",
            "host;touch-x",
            "a/b",
        ] {
            assert!(config::validate_server(server).is_err(), "{server:?}");
        }
        let dir = temp("servers");
        assert_eq!(
            execute(&dir, "ls", &servers(&["other"]), &dir, Some("srv")),
            1
        );
        assert_eq!(
            recover_target(&dir, "srv", &servers(&["other"]), true, &dir),
            1
        );
    }

    #[test]
    fn marker_parser_requires_matching_marker() {
        assert_eq!(parse_marker("noise\nDIPPY_END_abcd:7\n", "abcd"), Ok(7));
        assert_eq!(
            parse_marker("DIPPY_END_abc:255\r\nDIPPY_TRANSPORT_abc:255\n", "abc"),
            Ok(255)
        );
        assert!(parse_marker("DIPPY_END_other:0", "abcd").is_err());
    }

    #[test]
    fn poll_capture_handles_scrolled_start_and_missing_marker() {
        let output = "oldest available line\nnewest line\nDIPPY_END_abcdef:7\n";
        assert_eq!(
            poll_capture(|| Ok(output.into()), "abcdef", 0.1, 0.001),
            Ok(7)
        );
        for status in [0, 255, 143] {
            let last = format!("DIPPY_TRANSPORT_abc:{status}\n");
            assert_eq!(
                poll_capture(|| Ok(last.clone()), "abc", 1.0, 0.001),
                Err(Fail::Uncertain)
            );
        }
        assert_eq!(
            poll_capture(|| Ok(String::new()), "abc", 0.05, 0.001),
            Err(Fail::Uncertain)
        );
    }

    #[test]
    fn base64_matches_python() {
        for (data, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("frob | next > out", "ZnJvYiB8IG5leHQgPiBvdXQ="),
        ] {
            assert_eq!(base64(data.as_bytes()), encoded);
        }
    }

    fn local_transport(argv: &[&str], env: &[(&str, &str)], cwd: &Path) -> Transport {
        Transport {
            argv: argv.iter().map(|s| s.to_string()).collect(),
            env: env.iter().map(|(k, v)| (k.into(), v.into())).collect(),
            cwd: cwd.to_path_buf(),
            identity: "test".into(),
        }
    }

    #[test]
    fn terminal_transport_keeps_command_on_ssh_stdin() {
        let dir = temp("transport");
        let received = dir.join("received");
        let fake = script(
            &dir,
            "fake ssh '$%",
            &format!(
                "{{ printf '%s\\n' \"$@\" \"$SSH_AUTH_SOCK\"; cat; }} > '{}'",
                received.display()
            ),
        );
        let injected = dir.join("must-not-exist");
        let command = format!("frob\nprintf unsafe > '{}'\n", injected.display());
        let socket = "/project/agent ' $ socket";
        let transport = local_transport(
            &[&fake.to_string_lossy(), "-F", "config '$%\nspace"],
            &[("SSH_AUTH_SOCK", socket)],
            &dir,
        );
        let out = Command::new("/bin/bash")
            .arg("-c")
            .arg(transport_payload(&transport, &command, "abc"))
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let received = fs::read_to_string(received).unwrap();
        assert!(received.starts_with(&format!("-F\nconfig '$%\nspace\n{socket}\n")));
        assert!(received.contains("DIPPY_END_abc"));
        assert!(!injected.exists());
        assert!(!stdout.contains("DIPPY_END_abc"));
        assert!(stdout.contains("DIPPY_TRANSPORT_abc:0"));
    }

    #[test]
    fn remote_wrapper_handles_output_without_newline_and_exit_255() {
        let dir = temp("wrapper");
        // A local Bash stands in for SSH's remote Bash; no connection is made.
        let transport = local_transport(&["/bin/bash", "-s"], &[], &dir);
        let out = Command::new("/bin/bash")
            .arg("-c")
            .arg(transport_payload(
                &transport,
                "printf hello; exit 255",
                "abc",
            ))
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("hello\nDIPPY_END_abc:255\n"));
        assert_eq!(parse_marker(&stdout, "abc"), Ok(255));
    }

    #[test]
    fn state_is_python_json_and_validated() {
        let home = temp("state");
        let state = with(
            State::new(),
            &[
                ("status", json!("running")),
                ("marker", json!("m")),
                ("exit_code", json!(3)),
            ],
        );
        write_state(&home, "srv", &state).unwrap();
        let path = state_dir(&home).join("srv.json");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"exit_code\": 3, \"marker\": \"m\", \"status\": \"running\"}"
        );
        assert_eq!(
            fs::metadata(state_dir(&home)).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(read_state(&home, "srv").unwrap(), state);
        assert_eq!(read_state(&home, "missing").unwrap(), State::new());
        fs::write(&path, "{broken").unwrap();
        assert!(
            read_state(&home, "srv")
                .unwrap_err()
                .contains("cannot read execution state srv")
        );
        fs::write(&path, "{\"status\": \"odd\"}").unwrap();
        assert_eq!(
            read_state(&home, "srv").unwrap_err(),
            "invalid execution state: srv"
        );
    }

    #[test]
    fn operation_key_follows_project_root() {
        let dir = temp("key");
        fs::write(dir.join(".dippy"), "server srv\n").unwrap();
        let child = dir.join("child");
        fs::create_dir(&child).unwrap();
        assert_eq!(operation_key(&dir, "srv"), operation_key(&child, "srv"));
        assert_ne!(operation_key(&dir, "srv"), operation_key(&dir, "other"));
        let digest = hex(&Sha256::digest(
            paths::resolve(&dir).to_string_lossy().as_bytes(),
        ));
        assert_eq!(
            operation_key(&child, "srv"),
            format!("srv-{}", &digest[..24])
        );
    }

    fn allow_all(names: &[&str]) -> Config {
        Config {
            default: "allow".into(),
            ..servers(names)
        }
    }

    #[test]
    fn pending_state_blocks_until_cleared() {
        let dir = temp("pending");
        let home = dir.join("home");
        let config = allow_all(&["srv"]);
        let key = operation_key(&dir, "srv");
        for status in ["running", "indeterminate"] {
            write_state(
                &home,
                &key,
                &with(
                    State::new(),
                    &[("status", json!(status)), ("profile", json!("old"))],
                ),
            )
            .unwrap();
            // Checked before the transport: a broken profile is never reached.
            let broken = Config {
                run_on_server_ssh_auth_sock: Some("/different/socket".into()),
                ..allow_all(&["srv"])
            };
            assert_eq!(execute(&home, "frob", &broken, &dir, Some("srv")), 1);
        }
        assert_eq!(recover_target(&home, "srv", &config, false, &dir), 1);
        assert_eq!(recover_target(&home, "srv", &config, true, &dir), 0);
        assert_eq!(read_state(&home, &key).unwrap()["status"], json!("cleared"));
        write_state(
            &home,
            "srv",
            &with(State::new(), &[("status", json!("running"))]),
        )
        .unwrap();
        assert_eq!(execute(&home, "frob", &config, &dir, Some("srv")), 1);
        assert_eq!(recover_target(&home, "srv", &config, false, &dir), 1);
        assert_eq!(recover_target(&home, "srv", &config, true, &dir), 0);
        assert_eq!(
            read_state(&home, "srv").unwrap()["status"],
            json!("cleared")
        );
    }

    #[test]
    fn recovery_checks_backend_and_session_before_capture() {
        let dir = temp("recover");
        let home = dir.join("home");
        let key = operation_key(&dir, "srv");
        let state = with(
            State::new(),
            &[
                ("status", json!("running")),
                ("backend", json!("tmux")),
                ("session", json!("old")),
                ("marker", json!("abc")),
                ("profile", json!("user")),
            ],
        );
        write_state(&home, &key, &state).unwrap();
        let config = Config {
            run_on_server_backend: "tmux".into(),
            ..servers(&["srv"])
        };
        assert_eq!(recover_target(&home, "srv", &config, false, &dir), 1);
        let no_marker = with(state.clone(), &[("session", json!("dippy"))]);
        let mut no_marker = no_marker;
        no_marker.remove("marker");
        write_state(&home, &key, &no_marker).unwrap();
        assert_eq!(recover_target(&home, "srv", &config, false, &dir), 1);
        // No concrete pane: the capture is never attempted.
        write_state(&home, &key, &with(state, &[("session", json!("dippy"))])).unwrap();
        assert_eq!(recover_target(&home, "srv", &config, false, &dir), 1);
        assert_eq!(read_state(&home, &key).unwrap()["status"], json!("running"));
    }

    #[test]
    fn default_transport_preserves_user_settings() {
        let dir = temp("transport-user");
        let transport = build_transport(&Config::default(), "srv", &dir).unwrap();
        assert_eq!(transport.argv, ["ssh", "--", "srv", "bash", "-s"]);
        assert_eq!(transport.identity, "user");
        let reset = config::parse_config(
            "set run-on-server-ssh-config none\nset run-on-server-ssh-auth-sock none",
            None,
        )
        .unwrap();
        assert_eq!(
            build_transport(&reset, "srv", &dir).unwrap().identity,
            "user"
        );
    }

    #[test]
    fn incomplete_or_unsafe_profiles_fail_before_ssh() {
        let dir = temp("transport-bad");
        let profile = dir.join("config");
        fs::write(&profile, "Host *\n").unwrap();
        let both = "SSH profile requires both ssh-config and ssh-auth-sock (or none)";
        for config in [
            Config {
                run_on_server_ssh_config: Some("/missing".into()),
                ..Config::default()
            },
            Config {
                run_on_server_ssh_auth_sock: Some("/missing".into()),
                ..Config::default()
            },
            config::parse_config("set run-on-server-ssh-config none", None).unwrap(),
            config::parse_config("set run-on-server-ssh-auth-sock none", None).unwrap(),
        ] {
            assert_eq!(
                build_transport(&config, "srv", &dir).err().as_deref(),
                Some(both)
            );
        }
        let with_socket = |socket: &str| Config {
            run_on_server_ssh_config: Some(profile.clone()),
            run_on_server_ssh_auth_sock: Some(socket.into()),
            ..Config::default()
        };
        for name in ["agent%h", "agent${HOME}", "agent\\socket", "agent\"socket"] {
            let socket = dir.join(name).to_string_lossy().into_owned();
            let error = build_transport(&with_socket(&socket), "srv", &dir)
                .err()
                .unwrap();
            assert!(error.contains("expansion"), "{error}");
        }
        let missing = dir.join("missing").to_string_lossy().into_owned();
        let error = build_transport(&with_socket(&missing), "srv", &dir)
            .err()
            .unwrap();
        assert!(
            error.starts_with("[Errno 2] No such file or directory: '"),
            "{error}"
        );
        let plain = dir.join("plain").to_string_lossy().into_owned();
        fs::write(&plain, "").unwrap();
        let error = build_transport(&with_socket(&plain), "srv", &dir)
            .err()
            .unwrap();
        assert_eq!(
            error,
            format!("SSH auth socket is not a Unix socket: {plain}")
        );
    }
}
