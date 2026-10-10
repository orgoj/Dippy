//! Dashboard approvals (`set approval dashboard`): the version-2 request
//! payload shared by every agent hook and `dippy run`/`run-on-server`, and
//! its strict parser on the node.
//!
//! Native askpass payloads (the unversioned hook payload, version 1 of
//! `run`) are separate and unchanged; version 2 is only sent to the node.

use std::io::Read;

use serde_json::{Map, Value, json};

use crate::analyzer::Cause;
use crate::askpass;
use crate::hook::cause_json;

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

    #[test]
    fn ids_are_random_hex() {
        let (a, b) = (new_id().unwrap(), new_id().unwrap());
        assert!(valid_id(&a) && valid_id(&b) && a != b);
    }
}
