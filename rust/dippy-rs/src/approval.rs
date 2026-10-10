//! Dashboard approvals (`set approval dashboard`): the version-2 request
//! payload shared by every agent hook and `dippy run`/`run-on-server`, and
//! its strict parser on the node.
//!
//! Native askpass payloads (the unversioned hook payload, version 1 of
//! `run`) are separate and unchanged; version 2 is only sent to the node.

use std::io::Read;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::analyzer::Cause;
use crate::askpass;
use crate::config::Config;
use crate::hook::{cause_json, py_dumps};
use crate::http::{self, ClientError, Target, Timeouts};
use crate::queue::now_ms;

pub const VERSION: u64 = 2;

/// Payload keys, in the order [`Ask::payload`] writes them.
pub const KEYS: [&str; 16] = [
    "version",
    "id",
    "command",
    "cwd",
    "tool",
    "file_path",
    "message",
    "agent",
    "server",
    "decision",
    "reason",
    "context_flags",
    "flags",
    "remote",
    "causes",
    "deadline",
];

/// What one dashboard approval asks for; [`Ask::payload`] is the only
/// place the version-2 payload is built.
#[derive(Debug, Clone, Default)]
pub struct Ask<'a> {
    pub command: &'a str,
    /// Absolute.
    pub cwd: &'a str,
    pub tool: Option<&'a str>,
    pub file_path: Option<&'a str>,
    pub message: &'a str,
    /// `claude`, `agy`, `run`, `run-on-server`, ...
    pub agent: &'a str,
    pub server: Option<&'a str>,
    pub decision: &'a str,
    pub reason: &'a str,
    pub context_flags: Vec<String>,
    /// Flags and remoteness the command was analysed with.
    pub flags: Vec<String>,
    pub remote: bool,
    pub causes: &'a [Cause],
}

impl Ask<'_> {
    /// The version-2 payload; `deadline` is Unix time in milliseconds.
    pub fn payload(&self, id: &str, deadline: i64) -> Value {
        json!({
            "version": VERSION,
            "id": id,
            "command": self.command,
            "cwd": self.cwd,
            "tool": self.tool,
            "file_path": self.file_path,
            "message": self.message,
            "agent": self.agent,
            "server": self.server,
            "decision": self.decision,
            "reason": self.reason,
            "context_flags": self.context_flags,
            "flags": self.flags,
            "remote": self.remote,
            "causes": self.causes.iter().map(cause_json).collect::<Vec<_>>(),
            "deadline": deadline,
        })
    }
}

/// A random approval id: 32 lowercase hex digits.
pub fn new_id() -> std::io::Result<String> {
    let mut buf = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn field<'v>(o: &'v Map<String, Value>, key: &str) -> Result<&'v Value, String> {
    o.get(key).ok_or_else(|| format!("missing field: {key}"))
}

fn string<'v>(o: &'v Map<String, Value>, key: &str) -> Result<&'v str, String> {
    field(o, key)?
        .as_str()
        .ok_or_else(|| format!("{key} must be a string"))
}

fn optional_string(o: &Map<String, Value>, key: &str) -> Result<(), String> {
    match field(o, key)? {
        Value::String(_) | Value::Null => Ok(()),
        _ => Err(format!("{key} must be a string or null")),
    }
}

fn strings(v: &Value, key: &str) -> Result<(), String> {
    match v.as_array() {
        Some(a) if a.iter().all(Value::is_string) => Ok(()),
        _ => Err(format!("{key} must be a list of strings")),
    }
}

fn check_keys(o: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), String> {
    match o.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("unknown field in {what}: {k}")),
        None => Ok(()),
    }
}

fn check_cause(v: &Value) -> Result<(), String> {
    let o = v.as_object().ok_or("each cause must be an object")?;
    check_keys(o, &["kind", "target", "rule", "flags"], "cause")?;
    let kind = string(o, "kind").map_err(|e| format!("cause {e}"))?;
    if !askpass::KINDS.contains(&kind) {
        return Err(format!("unknown cause kind: {kind}"));
    }
    string(o, "target").map_err(|e| format!("cause {e}"))?;
    strings(field(o, "flags")?, "cause flags")?;
    match field(o, "rule")? {
        Value::Null => Ok(()),
        Value::Object(rule) => {
            check_keys(
                rule,
                &["directive", "pattern", "message", "scope", "file", "line"],
                "rule",
            )?;
            string(rule, "directive").map_err(|e| format!("rule {e}"))?;
            string(rule, "pattern").map_err(|e| format!("rule {e}"))?;
            for key in ["message", "scope", "file"] {
                optional_string(rule, key).map_err(|e| format!("rule {e}"))?;
            }
            match field(rule, "line")? {
                Value::Null => Ok(()),
                Value::Number(n) if n.is_u64() => Ok(()),
                _ => Err("rule line must be an integer or null".into()),
            }
        }
        _ => Err("cause rule must be an object or null".into()),
    }
}

/// Validate a version-2 payload from a client: every field present with its
/// type, no unknown fields, an absolute `cwd`, no environment fallback.
pub fn validate(v: &Value) -> Result<(), String> {
    let o = v.as_object().ok_or("body must be a JSON object")?;
    check_keys(o, &KEYS, "request")?;
    if field(o, "version")?.as_u64() != Some(VERSION) {
        return Err(format!("version must be {VERSION}"));
    }
    if !valid_id(string(o, "id")?) {
        return Err("id must be 32 lowercase hex digits".into());
    }
    let cwd = string(o, "cwd")?;
    if !cwd.starts_with('/') {
        return Err(format!("cwd must be an absolute path: {cwd}"));
    }
    for key in ["command", "message", "agent", "reason"] {
        string(o, key)?;
    }
    for key in ["tool", "file_path", "server"] {
        optional_string(o, key)?;
    }
    let decision = string(o, "decision")?;
    if !matches!(decision, "allow" | "ask" | "deny") {
        return Err("decision must be allow, ask or deny".into());
    }
    strings(field(o, "context_flags")?, "context_flags")?;
    strings(field(o, "flags")?, "flags")?;
    if !field(o, "remote")?.is_boolean() {
        return Err("remote must be a boolean".into());
    }
    let causes = field(o, "causes")?
        .as_array()
        .ok_or("causes must be a list")?;
    for cause in causes {
        check_cause(cause)?;
    }
    if !field(o, "deadline")?.is_i64() {
        return Err("deadline must be an integer (Unix milliseconds)".into());
    }
    Ok(())
}

/// The askpass request a validated payload describes (for the rule editor).
pub fn askpass_request(v: &Value) -> askpass::Request {
    // Validated: every field is present, `cwd` absolute; no fallback is used.
    askpass::Request::parse(&v.to_string(), |_| None)
}

// ------------------------------------------------------------------ client

/// When the process started: the wait budget counts from here.
static START: OnceLock<Instant> = OnceLock::new();

/// Record the process start; call first thing in `main`.
pub fn mark_start() {
    let _ = START.set(Instant::now());
}

/// Seconds the whole dashboard wait may take: `askpass-timeout - 3`,
/// saturating at 0 (no queue wait).
pub fn budget_seconds(config: &Config) -> u64 {
    config.askpass_timeout.saturating_sub(3).max(0) as u64
}

/// Seconds the connect may take at most.
const CONNECT: Duration = Duration::from_secs(1);
/// Largest node response the client reads.
const MAX_RESPONSE: usize = 1024 * 1024;

/// How a dashboard wait ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Allowed {
        id: String,
        note: Option<String>,
    },
    Denied {
        id: String,
        note: Option<String>,
    },
    /// The budget ended (or the node expired the item) without a decision.
    TimedOut {
        id: Option<String>,
    },
    /// The node answered but did not take the request (deny).
    Failed {
        id: Option<String>,
        message: String,
    },
    /// The node provably never received the request: use `set approval ask`.
    Fallback(String),
}

impl Outcome {
    pub fn id(&self) -> Option<&str> {
        match self {
            Outcome::Allowed { id, .. } | Outcome::Denied { id, .. } => Some(id),
            Outcome::TimedOut { id } | Outcome::Failed { id, .. } => id.as_deref(),
            Outcome::Fallback(_) => None,
        }
    }
}

/// The neutral timeout text of hooks: the fixed facts, then the configured
/// instruction.
pub fn timeout_message(config: &Config) -> String {
    format!(
        "Approval timed out after {} seconds. The requested command was not executed. {}",
        budget_seconds(config),
        config.approval_timeout_message
    )
}

/// Whether an outcome allows, and the agent-facing text for `reason`;
/// `None` for a fallback.
pub fn verdict(outcome: &Outcome, reason: &str, config: &Config) -> Option<(bool, String)> {
    let with_note = |text: String, note: &Option<String>| match note {
        Some(n) => format!("{text} (user note: {n})"),
        None => text,
    };
    Some(match outcome {
        Outcome::Allowed { note, .. } => {
            (true, with_note(format!("approved by user: {reason}"), note))
        }
        Outcome::Denied { note, .. } => {
            (false, with_note(format!("approval denied: {reason}"), note))
        }
        Outcome::TimedOut { .. } => (false, timeout_message(config)),
        Outcome::Failed { message, .. } => (false, format!("approval failed: {message}")),
        Outcome::Fallback(_) => return None,
    })
}

/// Audit message of an outcome entry.
pub fn outcome_message(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Allowed { note, .. } => note.clone().unwrap_or_else(|| "approved".into()),
        Outcome::Denied { note, .. } => note.clone().unwrap_or_else(|| "denied".into()),
        Outcome::TimedOut { .. } => "approval timed out".into(),
        Outcome::Failed { message, .. } => message.clone(),
        Outcome::Fallback(why) => why.clone(),
    }
}

/// The item state in a node response, if it is a well-formed item for `id`.
fn decided(body: &[u8], id: &str) -> Option<Option<Outcome>> {
    let v: Value = serde_json::from_slice(body).ok()?;
    if v.get("id").and_then(Value::as_str) != Some(id) {
        return None;
    }
    let note = v
        .get("note")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    let id = id.to_string();
    Some(match v.get("state").and_then(Value::as_str)? {
        "pending" => None,
        "allowed" => Some(Outcome::Allowed { id, note }),
        "denied" => Some(Outcome::Denied { id, note }),
        "expired" => Some(Outcome::TimedOut { id: Some(id) }),
        _ => return None,
    })
}

/// Queue `ask` on the dashboard node and wait for the user's decision.
/// Only `Allowed` allows. `info` receives progress lines for the hook log;
/// `waiting` runs once when the wait for the user starts.
pub fn wait(config: &Config, ask: &Ask, info: &dyn Fn(&str), waiting: &dyn Fn()) -> Outcome {
    wait_from(*START.get_or_init(Instant::now), config, ask, info, waiting)
}

/// [`wait`] with the budget counted from `start`.
pub fn wait_from(
    start: Instant,
    config: &Config,
    ask: &Ask,
    info: &dyn Fn(&str),
    waiting: &dyn Fn(),
) -> Outcome {
    let end = start + Duration::from_secs(budget_seconds(config));
    let remaining = || end.saturating_duration_since(Instant::now());
    if remaining().is_zero() {
        return Outcome::TimedOut { id: None };
    }
    let token_path = config.dashboard_token_path();
    let token = match std::fs::read_to_string(&token_path) {
        Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
        Ok(_) => {
            return Outcome::Fallback(format!(
                "dashboard token file is empty: {}",
                token_path.display()
            ));
        }
        Err(e) => {
            return Outcome::Fallback(format!(
                "dashboard token file unreadable: {}: {e}",
                token_path.display()
            ));
        }
    };
    let target = match Target::parse(&config.dashboard_url) {
        Ok(t) => t,
        Err(e) => {
            return Outcome::Failed {
                id: None,
                message: e,
            };
        }
    };
    let id = match new_id() {
        Ok(id) => id,
        Err(e) => {
            return Outcome::Failed {
                id: None,
                message: format!("cannot create an approval id: {e}"),
            };
        }
    };
    let deadline = now_ms() + remaining().as_millis() as i64;
    let body = py_dumps(&ask.payload(&id, deadline));
    let headers = [
        ("X-Dippy-Token", token.as_str()),
        ("Content-Type", "application/json"),
    ];
    let timeouts = |rest: Duration| Timeouts {
        connect: CONNECT.min(rest),
        io: rest,
    };
    info(&format!(
        "DASHBOARD: queueing approval {id} at {}",
        config.dashboard_url
    ));
    let posted = http::request(
        &target,
        "POST",
        "/api/approvals",
        &headers,
        Some(body.as_bytes()),
        timeouts(remaining()),
        MAX_RESPONSE,
    );
    let failed = |message: String| Outcome::Failed {
        id: Some(id.clone()),
        message,
    };
    match posted {
        Err(ClientError::Refused(e)) => {
            return Outcome::Fallback(format!("dashboard refused the connection: {e}"));
        }
        Err(ClientError::ConnectTimeout(e)) => {
            return Outcome::Fallback(format!("dashboard connect timeout: {e}"));
        }
        Err(ClientError::Connect(e)) => return failed(format!("dashboard unreachable: {e}")),
        Err(e) => info(&format!("DASHBOARD: request {id} indeterminate: {e}")),
        Ok(r) if matches!(r.status, 401 | 404) => {
            return Outcome::Fallback(format!(
                "dashboard answered HTTP {} to the request",
                r.status
            ));
        }
        Ok(r) if matches!(r.status, 200 | 201) => match decided(&r.body, &id) {
            Some(Some(outcome)) => return outcome,
            Some(None) => {}
            None => info(&format!("DASHBOARD: malformed response for {id}")),
        },
        Ok(r) if (400..500).contains(&r.status) => {
            let error = serde_json::from_slice::<Value>(&r.body)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default();
            return failed(format!(
                "dashboard rejected the request (HTTP {}): {error}",
                r.status
            ));
        }
        Ok(r) => info(&format!(
            "DASHBOARD: request {id} indeterminate: HTTP {}",
            r.status
        )),
    }
    // Indeterminate or pending: poll the same id until the budget ends. The
    // response of the last poll inside the budget decides; no second POST.
    waiting();
    loop {
        let rest = remaining();
        if rest < Duration::from_millis(100) {
            return Outcome::TimedOut { id: Some(id) };
        }
        let wait = rest
            .saturating_sub(Duration::from_millis(500))
            .min(crate::queue::MAX_WAIT);
        let path = format!("/api/approvals/{id}?wait={:.1}", wait.as_secs_f64());
        let sent = Instant::now();
        match http::request(
            &target,
            "GET",
            &path,
            &headers[..1],
            None,
            timeouts(rest),
            MAX_RESPONSE,
        ) {
            Ok(r) if r.status == 200 => match decided(&r.body, &id) {
                Some(Some(outcome)) => return outcome,
                // A node that answers pending at once must not spin the loop.
                Some(None) if sent.elapsed() >= Duration::from_millis(100) => continue,
                Some(None) => {}
                None => info(&format!("DASHBOARD: malformed response for {id}")),
            },
            Ok(r) => info(&format!("DASHBOARD: polling {id}: HTTP {}", r.status)),
            Err(e) => info(&format!("DASHBOARD: polling {id}: {e}")),
        }
        std::thread::sleep(Duration::from_millis(250).min(remaining()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    pub fn sample() -> Value {
        let rule = config::Match {
            decision: "ask".into(),
            pattern: "zorp *".into(),
            message: None,
            source: None,
            scope: Some("user".into()),
            file: Some("/u/.dippy/config".into()),
            line: Some(3),
        };
        let causes = [Cause {
            kind: "command",
            target: "zorp x".into(),
            rule: Some(rule),
            flags: ["@pipeline".to_string()].into(),
        }];
        Ask {
            command: "zorp x | cat",
            cwd: "/w",
            tool: Some("Bash"),
            message: "zorp x",
            agent: "claude",
            decision: "ask",
            reason: "zorp x",
            context_flags: vec!["@pipeline".into()],
            causes: &causes,
            ..Ask::default()
        }
        .payload("0123456789abcdef0123456789abcdef", 1_800_000_000_000)
    }

    #[test]
    fn payload_has_every_key_in_order_and_validates() {
        let v = sample();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, KEYS);
        assert_eq!(validate(&v), Ok(()));
        assert_eq!(
            v["causes"][0],
            json!({"kind": "command", "target": "zorp x", "rule": {"directive": "ask",
                "pattern": "zorp *", "message": null, "scope": "user",
                "file": "/u/.dippy/config", "line": 3}, "flags": ["@pipeline"]})
        );
        let r = askpass_request(&v);
        assert_eq!(
            (r.command.as_str(), r.cwd.to_str()),
            ("zorp x | cat", Some("/w"))
        );
        assert_eq!(r.causes[0].tags(), "");
    }

    #[test]
    fn invalid_payloads_are_rejected() {
        let cases: [(&str, Value); 14] = [
            ("cwd", json!("relative")),
            ("cwd", json!(null)),
            ("version", json!(1)),
            ("id", json!("XYZ")),
            ("id", json!("0123456789ABCDEF0123456789ABCDEF")),
            ("command", json!(5)),
            ("tool", json!(["Bash"])),
            ("remote", json!("false")),
            ("flags", json!(["a", 1])),
            ("causes", json!({})),
            (
                "causes",
                json!([{"kind": "shell", "target": "x", "rule": null, "flags": []}]),
            ),
            (
                "causes",
                json!([{"kind": "command", "target": "x", "rule": null, "flags": [], "x": 1}]),
            ),
            ("deadline", json!(1.5)),
            ("decision", json!("maybe")),
        ];
        for (key, value) in cases {
            let mut v = sample();
            v[key] = value.clone();
            assert!(validate(&v).is_err(), "{key}={value}");
        }
        let mut v = sample();
        v.as_object_mut().unwrap().remove("cwd");
        assert_eq!(validate(&v), Err("missing field: cwd".into()));
        let mut v = sample();
        v["extra"] = json!(1);
        assert_eq!(validate(&v), Err("unknown field in request: extra".into()));
        let mut v = sample();
        v["note"] = json!("x");
        assert!(validate(&v).is_err());
        assert!(validate(&json!([])).is_err());
    }

    use crate::dashboard::{Server, serve_on};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const TOKEN: &str = "client-token";

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dippy-rs-client-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A node on port 0 and a config pointing at it.
    fn node(dir: &Path) -> (Arc<Server>, SocketAddr, Config) {
        let mut server = Server::new(TOKEN, None, None);
        server.user_config = dir.join("no-user-config");
        let server = Arc::new(server);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let s = Arc::clone(&server);
        std::thread::spawn(move || serve_on(listener, s));
        std::fs::write(dir.join("token"), format!("{TOKEN}\n")).unwrap();
        let config = config_for(dir, &format!("http://{addr}"), 30);
        (server, addr, config)
    }

    fn config_for(dir: &Path, url: &str, timeout: i64) -> Config {
        Config {
            approval: "dashboard".into(),
            dashboard_url: url.into(),
            dashboard_token_file: Some(dir.join("token")),
            askpass_timeout: timeout,
            ..Config::default()
        }
    }

    fn ask(cwd: &str) -> Ask<'_> {
        Ask {
            command: "zorp x",
            cwd,
            message: "zorp x",
            agent: "claude",
            decision: "ask",
            reason: "zorp x",
            ..Ask::default()
        }
    }

    /// Decide the first pending item once it appears.
    fn decide_soon(server: &Arc<Server>, allow: bool, note: Option<&str>) {
        let server = Arc::clone(server);
        let note = note.map(str::to_string);
        std::thread::spawn(move || {
            for _ in 0..200 {
                if let Some(item) = server.approvals.list(Some(true)).first() {
                    server.approvals.decide(&item.id, allow, note.clone());
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
    }

    fn run(config: &Config, cwd: &str) -> Outcome {
        wait_from(Instant::now(), config, &ask(cwd), &|_| {}, &|| {})
    }

    #[test]
    fn user_decisions_reach_the_client() {
        let dir = temp("decide");
        let (server, _, config) = node(&dir);
        let cwd = dir.to_string_lossy().into_owned();
        decide_soon(&server, true, None);
        let outcome = run(&config, &cwd);
        assert!(
            matches!(&outcome, Outcome::Allowed { note: None, .. }),
            "{outcome:?}"
        );
        let item = server.approvals.get(outcome.id().unwrap()).unwrap();
        assert_eq!(item.request["cwd"], json!(cwd));
        assert_eq!(validate(&item.request), Ok(()));
        // The deadline is the end of the budget (askpass-timeout - 3).
        let ahead = item.deadline - item.created;
        assert!((26_000..=27_100).contains(&ahead), "{ahead}");
        decide_soon(&server, false, Some("not now"));
        assert!(
            matches!(run(&config, &cwd), Outcome::Denied { note: Some(n), .. } if n == "not now")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unaccepted_requests_fall_back() {
        let dir = temp("fallback");
        let (_server, addr, config) = node(&dir);
        let cwd = dir.to_string_lossy().into_owned();
        // Missing token file.
        let mut c = config.clone();
        c.dashboard_token_file = Some(dir.join("missing"));
        assert!(matches!(run(&c, &cwd), Outcome::Fallback(m) if m.contains("token file")));
        // Wrong token: 401.
        std::fs::write(dir.join("bad"), "wrong\n").unwrap();
        c.dashboard_token_file = Some(dir.join("bad"));
        assert!(matches!(run(&c, &cwd), Outcome::Fallback(m) if m.contains("HTTP 401")));
        // A hub or an old node: 404.
        let hub = Arc::new(Server::new(TOKEN, None, Some(dir.join("nodes.json"))));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let hub_addr = listener.local_addr().unwrap();
        std::thread::spawn(move || serve_on(listener, hub));
        let c = config_for(&dir, &format!("http://{hub_addr}"), 30);
        assert!(matches!(run(&c, &cwd), Outcome::Fallback(m) if m.contains("HTTP 404")));
        // Connection refused.
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let c = config_for(&dir, &format!("http://{closed}"), 30);
        assert!(matches!(run(&c, &cwd), Outcome::Fallback(m) if m.contains("refused")));
        // A rejected payload (relative cwd) is not a fallback: deny at once.
        let c = config_for(&dir, &format!("http://{addr}"), 30);
        assert!(
            matches!(run(&c, "relative"), Outcome::Failed { message, .. } if message.contains("HTTP 400"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn short_timeouts_deny_without_queueing() {
        let dir = temp("short");
        let (server, _, config) = node(&dir);
        let cwd = dir.to_string_lossy().into_owned();
        for timeout in [3, 1, 0, -5] {
            let c = Config {
                askpass_timeout: timeout,
                ..config.clone()
            };
            assert_eq!(run(&c, &cwd), Outcome::TimedOut { id: None });
        }
        assert!(server.approvals.list(None).is_empty());
        // A budget of one second ends in a timeout; the node expires it too.
        let c = Config {
            askpass_timeout: 4,
            ..config.clone()
        };
        let started = Instant::now();
        let outcome = run(&c, &cwd);
        assert!(started.elapsed() < Duration::from_millis(1500));
        let Outcome::TimedOut { id: Some(id) } = outcome else {
            panic!("{outcome:?}")
        };
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            server.approvals.get(&id).unwrap().state,
            crate::queue::State::Expired
        );
        assert!(timeout_message(&c).starts_with("Approval timed out after 1 seconds. The requested command was not executed. Wait for the user"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A proxy that sends POSTs to `node` and other requests to `polls`,
    /// drops the response to the first POST after the node stored it, and
    /// counts POSTs.
    fn lossy_proxy(node: SocketAddr, polls: SocketAddr, posts: Arc<AtomicUsize>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut client = stream.unwrap();
                let posts = Arc::clone(&posts);
                std::thread::spawn(move || {
                    let mut buf = vec![0u8; 65536];
                    let n = std::io::Read::read(&mut client, &mut buf).unwrap();
                    let is_post = buf.starts_with(b"POST");
                    let mut upstream =
                        TcpStream::connect(if is_post { node } else { polls }).unwrap();
                    let mut request = buf[..n].to_vec();
                    // Read the rest of a body the first read missed.
                    let text = String::from_utf8_lossy(&request).into_owned();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("Content-Length: "))
                            .map_or(0, |v| v.trim().parse::<usize>().unwrap());
                        let mut missing = length.saturating_sub(body.len());
                        while missing > 0 {
                            let n = std::io::Read::read(&mut client, &mut buf).unwrap();
                            request.extend_from_slice(&buf[..n]);
                            missing -= n.min(missing);
                        }
                    }
                    std::io::Write::write_all(&mut upstream, &request).unwrap();
                    let mut response = Vec::new();
                    std::io::Read::read_to_end(&mut upstream, &mut response).unwrap();
                    if is_post && posts.fetch_add(1, Ordering::SeqCst) == 0 {
                        return; // Stored by the node; the response is lost.
                    }
                    let _ = std::io::Write::write_all(&mut client, &response);
                });
            }
        });
        addr
    }

    #[test]
    fn lost_post_response_polls_the_same_id() {
        let dir = temp("lossy");
        let (server, addr, _) = node(&dir);
        let posts = Arc::new(AtomicUsize::new(0));
        let proxy = lossy_proxy(addr, addr, Arc::clone(&posts));
        let config = config_for(&dir, &format!("http://{proxy}"), 30);
        let cwd = dir.to_string_lossy().into_owned();
        decide_soon(&server, true, Some("ok"));
        let outcome = run(&config, &cwd);
        assert!(
            matches!(&outcome, Outcome::Allowed { note: Some(n), .. } if n == "ok"),
            "{outcome:?}"
        );
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        assert_eq!(server.approvals.list(None).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn restarted_node_ends_in_timeout() {
        let dir = temp("restart");
        let (_, addr, _) = node(&dir);
        // The proxy loses the POST response; a second node (the "restart")
        // answers every poll with 404.
        let posts = Arc::new(AtomicUsize::new(0));
        let mut fresh = Server::new(TOKEN, None, None);
        fresh.user_config = dir.join("none");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fresh_addr = listener.local_addr().unwrap();
        std::thread::spawn(move || serve_on(listener, Arc::new(fresh)));
        let proxy = lossy_proxy(addr, fresh_addr, Arc::clone(&posts));
        let config = config_for(&dir, &format!("http://{proxy}"), 5);
        let cwd = dir.to_string_lossy().into_owned();
        // The id is unknown to every poll: deny with the timeout, no retry.
        let started = Instant::now();
        let outcome = run(&config, &cwd);
        assert!(
            matches!(outcome, Outcome::TimedOut { id: Some(_) }),
            "{outcome:?}"
        );
        assert!(started.elapsed() < Duration::from_millis(2600));
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ids_are_random_hex() {
        let (a, b) = (new_id().unwrap(), new_id().unwrap());
        assert!(valid_id(&a) && valid_id(&b) && a != b);
    }
}
