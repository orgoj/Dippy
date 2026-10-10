//! Port of hook-side logging: `setup_logging` (the per-agent
//! `hook-approvals.log`) and `configure_logging`/`log_decision` in
//! `dippy.core.config` (the JSON-lines audit log from `set log`).

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::config::{Config, Flags};
use crate::hook::py_dumps;

/// `_get_log_file`: `hook-approvals.log` in the agent's home directory.
pub fn approvals_path(home: &Path, agent: &str) -> PathBuf {
    let dir = match agent {
        "gemini" | "agy" => ".gemini",
        "cursor" => ".cursor",
        "pi" => ".pi",
        "moltbot" => ".moltbot",
        "codex" => ".codex",
        "windsurf" => ".windsurf",
        "pearai" => ".pearai",
        _ => ".claude",
    };
    home.join(dir).join("hook-approvals.log")
}

/// One audit log entry; the arguments of Python's `log_decision`.
#[derive(Clone, Copy, Default)]
pub struct Entry<'a> {
    pub decision: &'a str,
    pub cmd: Option<&'a str>,
    pub rule: Option<&'a str>,
    pub message: Option<&'a str>,
    /// Logged only with `set log-full`.
    pub command: Option<&'a str>,
    pub cwd: Option<&'a Path>,
    pub tool: Option<&'a str>,
    pub file_path: Option<&'a str>,
    pub context_flags: Option<&'a Flags>,
    /// Logged only with `set log-full`.
    pub suggestion: Option<&'a str>,
}

struct Audit {
    path: PathBuf,
    full: bool,
    policy_cwd: Option<PathBuf>,
}

/// Both logs of one hook run. Every write failure is silent; a failed audit
/// write disables the audit log for the rest of the run.
pub struct Logs {
    agent: String,
    approvals: RefCell<Option<File>>,
    audit: RefCell<Option<Audit>>,
}

impl Logs {
    /// No log files (before the agent is known, and in tests).
    pub fn none(agent: &str) -> Self {
        Logs {
            agent: agent.to_string(),
            approvals: RefCell::new(None),
            audit: RefCell::new(None),
        }
    }

    /// `setup_logging`: open (and create) the agent's `hook-approvals.log`.
    pub fn setup(agent: &str) -> Self {
        let file = std::env::var_os("HOME").and_then(|home| {
            let path = approvals_path(Path::new(&home), agent);
            std::fs::create_dir_all(path.parent()?).ok()?;
            OpenOptions::new().create(true).append(true).open(path).ok()
        });
        Logs {
            approvals: RefCell::new(file),
            ..Logs::none(agent)
        }
    }

    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// Tests: approvals log at an explicit path.
    #[cfg(test)]
    pub(crate) fn with_approvals(agent: &str, path: &Path) -> Self {
        let file = OpenOptions::new().create(true).append(true).open(path).ok();
        Logs {
            approvals: RefCell::new(file),
            ..Logs::none(agent)
        }
    }

    /// `configure_logging`: `DIPPY_TEST_NO_LOG` disables only the audit log
    /// and is checked first, as in Python.
    pub fn configure(&self, config: &Config) {
        if std::env::var_os("DIPPY_TEST_NO_LOG").is_some_and(|v| !v.is_empty()) {
            return;
        }
        if !config.log_hook_approvals {
            *self.approvals.borrow_mut() = None;
        }
        let Some(path) = &config.log else { return };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        *self.audit.borrow_mut() = Some(Audit {
            path: path.clone(),
            full: config.log_full,
            policy_cwd: config.path_rule_cwd.clone(),
        });
    }

    fn line(&self, level: &str, message: &str) {
        if let Some(file) = self.approvals.borrow_mut().as_mut() {
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(file, "{ts} [{level}] {message}");
        }
    }

    pub fn info(&self, message: &str) {
        self.line("INFO", message);
    }

    pub fn warning(&self, message: &str) {
        self.line("WARNING", message);
    }

    pub fn error(&self, message: &str) {
        self.line("ERROR", message);
    }

    /// `log_decision`: append one JSON line in Python's key order.
    pub fn decision(&self, e: Entry) {
        let mut audit = self.audit.borrow_mut();
        let Some(a) = audit.as_ref() else { return };
        let mut entry = Map::new();
        entry.insert("decision".into(), json!(e.decision));
        let mut put = |key: &str, value: Option<&str>| {
            if let Some(v) = value {
                entry.insert(key.into(), json!(v));
            }
        };
        put("cmd", e.cmd);
        put("rule", e.rule);
        put("message", e.message);
        put("command", e.command.filter(|_| a.full));
        let cwd = e.cwd.map(|p| p.to_string_lossy());
        put("cwd", cwd.as_deref());
        let policy_cwd = a.policy_cwd.as_ref().map(|p| p.to_string_lossy());
        put("policy_cwd", policy_cwd.as_deref());
        put("tool", e.tool);
        put("file_path", e.file_path);
        if let Some(flags) = e.context_flags.filter(|f| !f.is_empty()) {
            entry.insert("context_flags".into(), json!(flags));
        }
        entry.insert("agent".into(), json!(self.agent));
        if a.full
            && let Some(s) = e.suggestion
        {
            entry.insert("suggestion".into(), json!(s));
        }
        entry.insert("ts".into(), json!(utc_isoformat(chrono::Utc::now())));
        let line = py_dumps(&Value::Object(entry)) + "\n";
        let written = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&a.path)
            .and_then(|mut f| f.write_all(line.as_bytes()));
        if written.is_err() {
            *audit = None;
        }
    }
}

/// `datetime.now(timezone.utc).isoformat()`: microseconds only when non-zero.
fn utc_isoformat(t: chrono::DateTime<chrono::Utc>) -> String {
    let micros = t.timestamp_subsec_micros();
    let fraction = if micros == 0 {
        String::new()
    } else {
        format!(".{micros:06}")
    };
    format!("{}{fraction}+00:00", t.format("%Y-%m-%dT%H:%M:%S"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dippy-rs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn isoformat_like_python() {
        let t = chrono::Utc.with_ymd_and_hms(2026, 10, 10, 8, 5, 3).unwrap();
        assert_eq!(utc_isoformat(t), "2026-10-10T08:05:03+00:00");
        let t = t + chrono::Duration::microseconds(42);
        assert_eq!(utc_isoformat(t), "2026-10-10T08:05:03.000042+00:00");
    }

    #[test]
    fn approvals_path_per_agent() {
        let home = Path::new("/h");
        assert_eq!(
            approvals_path(home, "agy"),
            Path::new("/h/.gemini/hook-approvals.log")
        );
        assert_eq!(
            approvals_path(home, "pi"),
            Path::new("/h/.pi/hook-approvals.log")
        );
        assert_eq!(
            approvals_path(home, "claude"),
            Path::new("/h/.claude/hook-approvals.log")
        );
    }

    #[test]
    fn decision_entry_fields_and_order() {
        let dir = temp_dir("audit");
        let log = dir.join("sub").join("audit.log");
        let text = format!("set log {}", log.display());
        let mut config = crate::config::parse_config(&text, None).unwrap();
        config.path_rule_cwd = Some(PathBuf::from("/p"));
        let logs = Logs::none("pi");
        logs.configure(&config);
        let flags: Flags = ["b".to_string(), "a".to_string()].into();
        logs.decision(Entry {
            decision: "ask",
            cmd: Some("rm ✓"),
            command: Some("rm -rf x"),
            cwd: Some(Path::new("/w")),
            context_flags: Some(&flags),
            suggestion: Some("rm *"),
            ..Entry::default()
        });
        let line = std::fs::read_to_string(&log).unwrap();
        let (head, ts) = line.split_once(", \"ts\": \"").unwrap();
        assert_eq!(
            head,
            "{\"decision\": \"ask\", \"cmd\": \"rm \\u2713\", \"cwd\": \"/w\", \
             \"policy_cwd\": \"/p\", \"context_flags\": [\"a\", \"b\"], \"agent\": \"pi\""
        );
        assert!(ts.ends_with("+00:00\"}\n"), "{ts}");

        config.log_full = true;
        logs.configure(&config);
        logs.decision(Entry {
            decision: "allow",
            command: Some("ls"),
            suggestion: Some("ls *"),
            ..Entry::default()
        });
        let last = std::fs::read_to_string(&log).unwrap();
        let last = last.lines().last().unwrap();
        assert!(
            last.starts_with(
                "{\"decision\": \"allow\", \"command\": \"ls\", \"policy_cwd\": \"/p\", \
                 \"agent\": \"pi\", \"suggestion\": \"ls *\", \"ts\": "
            ),
            "{last}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hook_approvals_off_stops_lines() {
        let dir = temp_dir("approvals");
        let path = dir.join("h.log");
        let logs = Logs::with_approvals("claude", &path);
        logs.info("Auto-detected mode: claude");
        let config = crate::config::parse_config("set log-hook-approvals off", None).unwrap();
        logs.configure(&config);
        logs.info("APPROVED: x");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(
            text.ends_with(" [INFO] Auto-detected mode: claude\n"),
            "{text}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
