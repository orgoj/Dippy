//! Port of `dippy.config_admin` and `handle_config_subcommand`: read or
//! atomically edit one explicitly selected config file (`--user`, the
//! default, or `--project` = `./.dippy` in the process cwd).

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::config::{self, Config, PROJECT_CONFIG_NAME};
use crate::paths::py_path_str;

/// `_CONFIG_KEYS`: the settings `dippy config` reads and writes.
pub const KEYS: [&str; 9] = [
    "askpass",
    "askpass-timeout",
    "approval-wait-message",
    "run-on-server-backend",
    "run-on-server-session",
    "run-on-server-timeout",
    "run-on-server-poll-interval",
    "run-on-server-ssh-config",
    "run-on-server-ssh-auth-sock",
];

#[derive(clap::Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    action: Option<Action>,
}

#[derive(clap::Args)]
struct Scope {
    #[arg(long, conflicts_with = "project")]
    user: bool,
    #[arg(long)]
    project: bool,
}

#[derive(clap::Subcommand)]
enum Action {
    /// Print the config file, or one setting's effective value in it
    Get {
        key: Option<String>,
        #[command(flatten)]
        scope: Scope,
    },
    /// Remove a setting
    Unset {
        key: String,
        #[command(flatten)]
        scope: Scope,
    },
    /// Set a setting, replacing earlier lines for it
    Set {
        key: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[command(flatten)]
        scope: Scope,
    },
    /// Manage allowed `run-on-server` targets
    Server(ServerArgs),
}

#[derive(clap::Args)]
struct ServerArgs {
    #[command(subcommand)]
    action: Option<ServerAction>,
}

#[derive(clap::Subcommand)]
enum ServerAction {
    Add {
        server: String,
        #[command(flatten)]
        scope: Scope,
    },
    Remove {
        server: String,
        #[command(flatten)]
        scope: Scope,
    },
    List {
        #[command(flatten)]
        scope: Scope,
    },
}

/// One line edit of `edit_config`.
pub enum Edit<'a> {
    Set(&'a str, &'a str),
    Unset(&'a str),
    ServerAdd(&'a str),
    ServerRemove(&'a str),
}

fn normalize(key: &str) -> String {
    key.to_lowercase().replace('_', "-")
}

/// `_setting_key`: the normalized key of a `set` line.
fn setting_key(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    let directive = parts.next()?;
    let key = parts.next()?;
    (directive.to_lowercase() == "set").then(|| normalize(key))
}

/// `_write_atomic`: replace `path` through a temporary file in its
/// directory, keeping an existing file's mode (a new file gets 0600).
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mode = std::fs::metadata(path).ok().map(|m| m.permissions().mode());
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut n = 0;
    let (temporary, mut file) = loop {
        let candidate = dir.join(format!(".{name}.{}.{n}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        if let Some(mode) = mode {
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode))?;
        }
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// `edit_config`: change one setting or server line, keeping every other
/// line (comments, rules, blank lines) as it is.
pub fn edit_config(path: &Path, edit: Edit) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let lines = text.lines();
    let output: Vec<String> = match edit {
        Edit::Set(key, _) | Edit::Unset(key) => {
            let key = normalize(key);
            let mut replacement = match edit {
                Edit::Set(_, value) => Some(format!("set {key} {value}")),
                _ => None,
            };
            let mut output = Vec::new();
            for line in lines {
                if setting_key(line).as_deref() == Some(key.as_str()) {
                    output.extend(replacement.take());
                } else {
                    output.push(line.to_string());
                }
            }
            output.extend(replacement);
            output
        }
        Edit::ServerAdd(server) | Edit::ServerRemove(server) => {
            let directive = format!("server {server}");
            let mut output: Vec<String> = lines
                .filter(|line| line.trim() != directive)
                .map(str::to_string)
                .collect();
            if matches!(edit, Edit::ServerAdd(_)) {
                output.push(directive);
            }
            output
        }
    };
    let mut text = output.join("\n");
    if !output.is_empty() {
        text.push('\n');
    }
    write_atomic(path, &text)
}

/// Python `repr(float)`.
pub fn py_float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    let a = x.abs();
    if a != 0.0 && !(1e-4..1e16).contains(&a) {
        let s = format!("{x:e}");
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let exp: i32 = exp.parse().unwrap_or(0);
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exp.abs());
    }
    let s = x.to_string();
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// Python `str()` of the setting's value in `config`.
fn setting_value(config: &Config, key: &str) -> String {
    let path = |p: &Option<PathBuf>| p.as_deref().map_or("None".into(), py_path_str);
    match key {
        "askpass" => path(&config.askpass),
        "askpass-timeout" => config.askpass_timeout.to_string(),
        "approval-wait-message" => config.approval_wait_message.clone(),
        "run-on-server-backend" => config.run_on_server_backend.clone(),
        "run-on-server-session" => config.run_on_server_session.clone(),
        "run-on-server-timeout" => py_float_repr(config.run_on_server_timeout),
        "run-on-server-poll-interval" => py_float_repr(config.run_on_server_poll_interval),
        "run-on-server-ssh-config" => path(&config.run_on_server_ssh_config),
        "run-on-server-ssh-auth-sock" => config
            .run_on_server_ssh_auth_sock
            .clone()
            .unwrap_or("None".into()),
        _ => unreachable!("not in KEYS: {key}"),
    }
}

fn scope_path(scope: &Scope) -> PathBuf {
    if scope.project {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(PROJECT_CONFIG_NAME)
    } else {
        config::user_config_path()
    }
}

/// `_read_admin_config`: this file only, no merging.
fn read_config(path: &Path) -> Result<Config, String> {
    let parsed = match std::fs::read_to_string(path) {
        Ok(text) => config::parse_config(&text, Some(&path.to_string_lossy())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    config::print_warnings();
    parsed.map_err(|e| format!("config error: {e}"))
}

fn fail(message: String) -> i32 {
    eprintln!("{message}");
    1
}

fn edit(path: &Path, change: Edit) -> i32 {
    match edit_config(path, change) {
        Ok(()) => 0,
        Err(e) => fail(format!("{}: {e}", path.display())),
    }
}

/// Unknown keys fail before any file is read or written.
fn known_key(key: &str) -> Result<String, String> {
    let normalized = normalize(key);
    if KEYS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!("unsupported config key: {key}"))
    }
}

/// `handle_config_subcommand`.
pub fn run(args: &ConfigArgs) -> i32 {
    let Some(action) = &args.action else {
        return fail("config action required".into());
    };
    match action {
        Action::Server(server) => run_server(server),
        Action::Get { key: None, scope } => {
            if let Ok(text) = std::fs::read_to_string(scope_path(scope)) {
                print!("{text}");
            }
            0
        }
        Action::Get {
            key: Some(key),
            scope,
        } => {
            let result = known_key(key)
                .and_then(|k| read_config(&scope_path(scope)).map(|c| setting_value(&c, &k)));
            match result {
                Ok(value) => {
                    println!("{value}");
                    0
                }
                Err(e) => fail(e),
            }
        }
        Action::Set { key, value, scope } => {
            let normalized = match known_key(key) {
                Ok(k) => k,
                Err(e) => return fail(e),
            };
            // Python accepts these and writes the extra lines into the file.
            let one_line = !value.contains(['\n', '\r', '\0']);
            let candidate = config::parse_config(&format!("set {normalized} {value}"), None);
            config::print_warnings();
            let attribute = normalized.replace('-', "_");
            if !one_line || !candidate.is_ok_and(|c| c.configured_settings.contains(&attribute)) {
                return fail(format!("invalid value for {key}: {value}"));
            }
            edit(&scope_path(scope), Edit::Set(&normalized, value))
        }
        Action::Unset { key, scope } => match known_key(key) {
            Ok(k) => edit(&scope_path(scope), Edit::Unset(&k)),
            Err(e) => fail(e),
        },
    }
}

fn run_server(args: &ServerArgs) -> i32 {
    let Some(action) = &args.action else {
        return fail("server action required".into());
    };
    let (server, scope, add) = match action {
        ServerAction::List { scope } => {
            return match read_config(&scope_path(scope)) {
                Ok(config) => {
                    for server in config.servers {
                        println!("{server}");
                    }
                    0
                }
                Err(e) => fail(e),
            };
        }
        ServerAction::Add { server, scope } => (server, scope, true),
        ServerAction::Remove { server, scope } => (server, scope, false),
    };
    if let Err(e) = config::validate_server(server) {
        return fail(e);
    }
    let path = scope_path(scope);
    edit(
        &path,
        if add {
            Edit::ServerAdd(server)
        } else {
            Edit::ServerRemove(server)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        for (x, expected) in [
            (300.0, "300.0"),
            (0.1, "0.1"),
            (5.0, "5.0"),
            (1e16, "1e+16"),
            (1e20, "1e+20"),
            (1e-5, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (0.0001, "0.0001"),
            (123456789.125, "123456789.125"),
            (9999999999999998.0, "9999999999999998.0"),
            (f64::INFINITY, "inf"),
        ] {
            assert_eq!(py_float_repr(x), expected);
        }
    }

    #[test]
    fn setting_key_normalizes() {
        assert_eq!(
            setting_key("  SET Askpass_Timeout 3").as_deref(),
            Some("askpass-timeout")
        );
        assert_eq!(setting_key("set"), None);
        assert_eq!(setting_key("allow set x"), None);
    }
}
