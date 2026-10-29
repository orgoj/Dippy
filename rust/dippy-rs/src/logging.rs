//! Port of `configure_logging`/`log_decision` in `dippy.core.config` (the
//! JSON-lines audit log from `set log`). Divergence from Python: no
//! per-agent `hook-approvals.log`; every hook call writes at least one audit
//! entry instead (see [`Logs::close`]).

use std::cell::{Cell, RefCell};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::config::{Config, Flags};
use crate::hook::py_dumps;

/// One audit log entry; the arguments of Python's `log_decision`.
#[derive(Clone, Copy, Default)]
pub struct Entry<'a> {
    pub decision: &'a str,
    pub cmd: Option<&'a str>,
    pub rule: Option<&'a str>,
    pub message: Option<&'a str>,
    pub command: Option<&'a str>,
    pub cwd: Option<&'a Path>,
    pub tool: Option<&'a str>,
    pub file_path: Option<&'a str>,
    /// Hook event of a closing entry ([`Logs::close`]).
    pub event: Option<&'a str>,
    pub context_flags: Option<&'a Flags>,
    pub suggestion: Option<&'a str>,
    /// Dashboard approval id (outcome entries of `set approval dashboard`).
    pub approval: Option<&'a str>,
}

struct Audit {
    path: PathBuf,
    policy_cwd: Option<PathBuf>,
}

/// `DIPPY_TEST_NO_LOG` disables the audit log.
fn test_no_log() -> bool {
    std::env::var_os("DIPPY_TEST_NO_LOG").is_some_and(|v| !v.is_empty())
}

/// The audit log of one run. Every write failure is silent; a failed audit
/// write disables the audit log for the rest of the run.
pub struct Logs {
    agent: String,
    audit: RefCell<Option<Audit>>,
    /// Audit log of a hook call that never configured one (`close`).
    fallback: Option<PathBuf>,
    configured: Cell<bool>,
    /// An entry was logged since the last `close`.
    written: Cell<bool>,
    /// Failures for the closing entry's `message`.
    notes: RefCell<Vec<String>>,
}

impl Logs {
    /// No log until `configure`.
    pub fn none(agent: &str) -> Self {
        Logs {
            agent: agent.to_string(),
            audit: RefCell::new(None),
            fallback: None,
            configured: Cell::new(false),
            written: Cell::new(false),
            notes: RefCell::new(Vec::new()),
        }
    }

    /// A hook call: `close` logs to `fallback` when no config was applied.
    pub fn hook(agent: &str, fallback: Option<PathBuf>) -> Self {
        Logs {
            fallback,
            ..Logs::none(agent)
        }
    }

    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// `configure_logging`: `DIPPY_TEST_NO_LOG` disables the audit log and
    /// is checked first, as in Python.
    pub fn configure(&self, config: &Config) {
        self.configured.set(true);
        if test_no_log() {
            return;
        }
        let Some(path) = &config.log else { return };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        *self.audit.borrow_mut() = Some(Audit {
            path: path.clone(),
            policy_cwd: config.path_rule_cwd.clone(),
        });
    }

    /// Run one call; a panic is noted for the closing entry and returned as
    /// its text.
    pub fn catch<T>(&self, call: impl FnOnce() -> T) -> Result<T, String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)).map_err(|payload| {
            let text = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            self.note(&format!("panic: {text}"));
            text
        })
    }

    /// Record a failure for the closing entry.
    pub fn note(&self, message: &str) {
        self.notes.borrow_mut().push(message.to_string());
    }

    /// End of a hook call: log `e` (with the notes as `message`) unless an
    /// entry was logged and nothing failed since. Without a config the entry
    /// goes to the fallback log.
    pub fn close(&self, e: Entry) {
        let notes = std::mem::take(&mut *self.notes.borrow_mut());
        if self.written.replace(false) && notes.is_empty() {
            return;
        }
        if !self.configured.get()
            && !test_no_log()
            && let Some(path) = &self.fallback
            && path
                .parent()
                .is_none_or(|p| std::fs::create_dir_all(p).is_ok())
        {
            *self.audit.borrow_mut() = Some(Audit {
                path: path.clone(),
                policy_cwd: None,
            });
        }
        let message = notes.join("; ");
        self.decision(Entry {
            message: (!message.is_empty()).then_some(message.as_str()),
            ..e
        });
        self.written.set(false);
    }

    /// `log_decision`: append one JSON line in Python's key order.
    pub fn decision(&self, e: Entry) {
        self.written.set(true);
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
        put("command", e.command);
        let cwd = e.cwd.map(|p| p.to_string_lossy());
        put("cwd", cwd.as_deref());
        let policy_cwd = a.policy_cwd.as_ref().map(|p| p.to_string_lossy());
        put("policy_cwd", policy_cwd.as_deref());
        put("tool", e.tool);
        put("file_path", e.file_path);
        put("event", e.event);
        if let Some(flags) = e.context_flags.filter(|f| !f.is_empty()) {
            entry.insert("context_flags".into(), json!(flags));
        }
        entry.insert("agent".into(), json!(self.agent));
        if let Some(id) = e.approval {
            entry.insert("approval".into(), json!(id));
        }
        if let Some(s) = e.suggestion {
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
        // `command` and `suggestion` are logged without `set log-full`.
        assert_eq!(
            head,
            "{\"decision\": \"ask\", \"cmd\": \"rm \\u2713\", \"command\": \"rm -rf x\", \
             \"cwd\": \"/w\", \"policy_cwd\": \"/p\", \"context_flags\": [\"a\", \"b\"], \
             \"agent\": \"pi\", \"suggestion\": \"rm *\""
        );
        assert!(ts.ends_with("+00:00\"}\n"), "{ts}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
