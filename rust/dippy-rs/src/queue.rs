//! The node's approval queue (`/api/approvals`): requests from agent hooks
//! and `dippy run` with `set approval dashboard`, decided in the web UI.
//!
//! The queue lives only in the node process, in memory: pending requests
//! are lost on restart. One mutex guards every item, so the first decision
//! wins and an item expires atomically at its deadline. Rule checks and
//! saves reuse `askpass::check`, `askpass::statuses` and `admin::add_rule`;
//! the config is loaded per request, never cached.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::askpass::{self, Files};
use crate::dashboard::{MAX_REQUEST_BODY, Reply, Server};
use crate::http::{self, Request};
use crate::{admin, approval, config};

/// Done items (allowed, denied, expired) kept for the list.
pub const KEEP_DONE: usize = 200;
/// Server cap of `?wait=`.
pub const MAX_WAIT: Duration = Duration::from_secs(10);
/// A deadline may be at most this far ahead at POST time.
pub const MAX_AHEAD_MS: i64 = 3_600_000;
pub const MAX_NOTE: usize = 1000;
/// Keys of an item, in the order [`item_json`] writes them.
pub const ITEM_KEYS: [&str; 8] = [
    "id", "state", "request", "statuses", "created", "deadline", "decided", "note",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Pending,
    Allowed,
    Denied,
    Expired,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Allowed => "allowed",
            State::Denied => "denied",
            State::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub state: State,
    /// The validated version-2 payload.
    pub request: Value,
    pub created: i64,
    pub deadline: i64,
    pub decided: Option<i64>,
    pub note: Option<String>,
}

#[derive(Default)]
pub struct Queue {
    /// Insertion order: newest last.
    items: Mutex<Vec<Item>>,
    changed: Condvar,
    /// `admin::add_rule` is read-modify-write.
    save: Mutex<()>,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl Queue {
    /// Lock and expire every pending item whose deadline has passed.
    fn lock(&self) -> MutexGuard<'_, Vec<Item>> {
        let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        let now = now_ms();
        let mut expired = false;
        for item in items.iter_mut() {
            if item.state == State::Pending && item.deadline <= now {
                item.state = State::Expired;
                item.decided = Some(item.deadline);
                expired = true;
            }
        }
        if expired {
            prune(&mut items);
            self.changed.notify_all();
        }
        items
    }

    /// Store a new request, or return the existing item for the same id.
    /// `Err(409)` for a different body under an existing id.
    pub fn add(&self, request: Value) -> Result<(u16, Item), (u16, String)> {
        let id = request["id"].as_str().unwrap_or("").to_string();
        let deadline = request["deadline"].as_i64().unwrap_or(0);
        let mut items = self.lock();
        if let Some(existing) = items.iter().find(|i| i.id == id) {
            if existing.request == request {
                return Ok((200, existing.clone()));
            }
            return Err((
                409,
                format!("approval {id} exists with a different request"),
            ));
        }
        let now = now_ms();
        if deadline <= now {
            return Err((400, "deadline is in the past".into()));
        }
        if deadline > now + MAX_AHEAD_MS {
            return Err((400, "deadline is more than one hour ahead".into()));
        }
        let item = Item {
            id,
            state: State::Pending,
            request,
            created: now,
            deadline,
            decided: None,
            note: None,
        };
        items.push(item.clone());
        Ok((201, item))
    }

    pub fn get(&self, id: &str) -> Option<Item> {
        self.lock().iter().find(|i| i.id == id).cloned()
    }

    /// The item after waiting up to `wait` while it is pending.
    pub fn wait(&self, id: &str, wait: Duration) -> Option<Item> {
        let end = now_ms() + wait.as_millis() as i64;
        let mut items = self.lock();
        loop {
            let item = items.iter().find(|i| i.id == id)?.clone();
            let now = now_ms();
            if item.state != State::Pending || now >= end {
                return Some(item);
            }
            let until = end.min(item.deadline);
            let timeout = Duration::from_millis((until - now).max(1) as u64);
            items = self
                .changed
                .wait_timeout(items, timeout)
                .unwrap_or_else(|e| e.into_inner())
                .0;
            drop(items);
            items = self.lock();
        }
    }

    /// Items newest first, optionally only pending or only done.
    pub fn list(&self, pending: Option<bool>) -> Vec<Item> {
        self.lock()
            .iter()
            .rev()
            .filter(|i| pending.is_none_or(|p| (i.state == State::Pending) == p))
            .cloned()
            .collect()
    }

    /// First decision wins: `Err(item)` when it is already decided or expired.
    pub fn decide(
        &self,
        id: &str,
        allow: bool,
        note: Option<String>,
    ) -> Option<Result<Item, Item>> {
        let mut items = self.lock();
        let item = items.iter_mut().find(|i| i.id == id)?;
        if item.state != State::Pending {
            return Some(Err(item.clone()));
        }
        item.state = if allow { State::Allowed } else { State::Denied };
        item.decided = Some(now_ms());
        item.note = note;
        let decided = item.clone();
        prune(&mut items);
        self.changed.notify_all();
        Some(Ok(decided))
    }
}

/// Keep every pending item and the newest `KEEP_DONE` done ones.
fn prune(items: &mut Vec<Item>) {
    let mut done: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.state != State::Pending)
        .map(|(n, i)| (i.decided.unwrap_or(0), n))
        .collect();
    if done.len() <= KEEP_DONE {
        return;
    }
    done.sort_unstable();
    let drop: HashSet<usize> = done[..done.len() - KEEP_DONE]
        .iter()
        .map(|(_, n)| *n)
        .collect();
    let mut n = 0;
    items.retain(|_| {
        n += 1;
        !drop.contains(&(n - 1))
    });
}

// ----------------------------------------------------------------- render

/// Current decision per cause under the config on disk (`ask` for every
/// cause when the config has an error).
fn statuses(request: &Value, user: &Path) -> Vec<&'static str> {
    let req = approval::askpass_request(request);
    askpass::statuses(&req, user).unwrap_or_else(|_| vec!["ask"; req.causes.len()])
}

pub fn item_json(item: &Item, user: &Path) -> Value {
    json!({
        "id": item.id,
        "state": item.state.as_str(),
        "request": item.request,
        "statuses": statuses(&item.request, user),
        "created": item.created,
        "deadline": item.deadline,
        "decided": item.decided,
        "note": item.note,
    })
}

// ----------------------------------------------------------------- routes

fn json_body(req: &mut Request) -> Result<Map<String, Value>, Reply> {
    let body = req
        .body(MAX_REQUEST_BODY)
        .map_err(|e| Reply::error(400, &e))?;
    match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(o)) => Ok(o),
        Ok(_) => Err(Reply::error(400, "body must be a JSON object")),
        Err(_) => Err(Reply::error(400, "body must be JSON")),
    }
}

/// Query parameters limited to `allowed`.
fn query(req: &Request, allowed: &[&str]) -> Result<Vec<(String, Vec<String>)>, Reply> {
    let params = http::parse_qs(&req.query);
    if let Some((name, _)) = params.iter().find(|(n, _)| !allowed.contains(&n.as_str())) {
        return Err(Reply::error(400, &format!("unknown parameter: {name}")));
    }
    Ok(params)
}

fn last<'a>(params: &'a [(String, Vec<String>)], name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(n, _)| n == name)
        .and_then(|(_, v)| v.last())
        .map(String::as_str)
}

fn not_found(id: &str) -> Reply {
    Reply::error(404, &format!("unknown approval: {id}"))
}

pub fn post_approval(server: &Server, req: &mut Request, _: &[String]) -> Reply {
    let body = match json_body(req) {
        Ok(body) => Value::Object(body),
        Err(reply) => return reply,
    };
    if let Err(e) = approval::validate(&body) {
        return Reply::error(400, &e);
    }
    match server.approvals.add(body) {
        Ok((status, item)) => Reply::json(status, &item_json(&item, &server.user_config)),
        Err((status, message)) => Reply::error(status, &message),
    }
}

pub fn list_approvals(server: &Server, req: &mut Request, _: &[String]) -> Reply {
    let params = match query(req, &["state"]) {
        Ok(p) => p,
        Err(reply) => return reply,
    };
    let pending = match last(&params, "state") {
        None => None,
        Some("pending") => Some(true),
        Some("done") => Some(false),
        Some(other) => {
            return Reply::error(400, &format!("state must be pending or done: {other}"));
        }
    };
    let items: Vec<Value> = server
        .approvals
        .list(pending)
        .iter()
        .map(|i| item_json(i, &server.user_config))
        .collect();
    Reply::json(200, &Value::Array(items))
}

pub fn get_approval(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    let id = &captured[0];
    let params = match query(req, &["wait"]) {
        Ok(p) => p,
        Err(reply) => return reply,
    };
    let wait = match last(&params, "wait") {
        None => Duration::ZERO,
        Some(raw) => match raw.trim().parse::<f64>() {
            Ok(s) if s.is_finite() && s >= 0.0 => Duration::from_secs_f64(s).min(MAX_WAIT),
            _ => return Reply::error(400, &format!("wait must be a number of seconds: {raw}")),
        },
    };
    match server.approvals.wait(id, wait) {
        Some(item) => Reply::json(200, &item_json(&item, &server.user_config)),
        None => not_found(id),
    }
}

pub fn decide_approval(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    let id = &captured[0];
    let body = match json_body(req) {
        Ok(body) => body,
        Err(reply) => return reply,
    };
    if let Some(k) = body
        .keys()
        .find(|k| !matches!(k.as_str(), "decision" | "note"))
    {
        return Reply::error(400, &format!("unknown field: {k}"));
    }
    let allow = match body.get("decision").and_then(Value::as_str) {
        Some("allow") => true,
        Some("deny") => false,
        _ => return Reply::error(400, "decision must be allow or deny"),
    };
    let note = match body.get("note") {
        None | Some(Value::Null) => None,
        Some(Value::String(n)) if n.chars().count() <= MAX_NOTE => {
            Some(n.trim().to_string()).filter(|n| !n.is_empty())
        }
        Some(Value::String(_)) => {
            return Reply::error(400, &format!("note must be at most {MAX_NOTE} characters"));
        }
        Some(_) => return Reply::error(400, "note must be a string"),
    };
    match server.approvals.decide(id, allow, note) {
        None => not_found(id),
        Some(Ok(item)) => Reply::json(200, &item_json(&item, &server.user_config)),
        Some(Err(item)) => Reply::json(409, &item_json(&item, &server.user_config)),
    }
}

/// A validated rule candidate.
struct Candidate {
    cause: Option<usize>,
    action: String,
    kind: String,
    tags: String,
    pattern: String,
    project: bool,
}

fn candidate(body: &Map<String, Value>, causes: usize) -> Result<Candidate, String> {
    const KEYS: [&str; 6] = ["cause", "action", "kind", "tags", "pattern", "target"];
    if let Some(k) = body.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!("unknown field: {k}"));
    }
    if let Some(k) = KEYS.iter().find(|k| !body.contains_key(**k)) {
        return Err(format!("missing field: {k}"));
    }
    let text = |key: &str| -> Result<String, String> {
        body[key]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{key} must be a string"))
    };
    let cause = match &body["cause"] {
        Value::Null if causes == 0 => None,
        Value::Null => return Err("cause must be an index into the request's causes".into()),
        Value::Number(n) => match n.as_u64() {
            Some(i) if (i as usize) < causes => Some(i as usize),
            _ => return Err(format!("cause must be 0 to {}", causes.saturating_sub(1))),
        },
        _ => return Err("cause must be an integer or null".into()),
    };
    let action = text("action")?;
    if !askpass::ACTIONS.contains(&action.as_str()) {
        return Err("action must be allow, ask or deny".into());
    }
    let kind = text("kind")?;
    if !askpass::KINDS.contains(&kind.as_str()) {
        return Err(format!("kind must be one of {}", askpass::KINDS.join(", ")));
    }
    let project = match text("target")?.as_str() {
        "project" => true,
        "user" => false,
        _ => return Err("target must be project or user".into()),
    };
    Ok(Candidate {
        cause,
        action,
        kind,
        tags: text("tags")?,
        pattern: text("pattern")?,
        project,
    })
}

/// The stored request, the candidate and its check (recomputed from the
/// stored request, never from the client).
fn checked(
    server: &Server,
    req: &mut Request,
    id: &str,
) -> Result<(Candidate, PathBuf, askpass::Check, Value), Reply> {
    let item = server.approvals.get(id).ok_or_else(|| not_found(id))?;
    let body = json_body(req)?;
    let request = approval::askpass_request(&item.request);
    let c = candidate(&body, request.causes.len()).map_err(|e| Reply::error(400, &e))?;
    let target = if c.project {
        admin::project_rule_path(&request.cwd)
    } else {
        server.user_config.clone()
    };
    let files = Files {
        user: &server.user_config,
        target: &target,
    };
    let cause = c.cause.and_then(|i| request.causes.get(i));
    let check = askpass::check(
        &request, cause, &c.action, &c.kind, &c.tags, &c.pattern, &files,
    );
    Ok((c, target, check, item.request))
}

fn check_json(check: &askpass::Check, action: &str, file: &Path) -> Value {
    let (outcome, error) = match &check.outcome {
        Ok(o) => (json!({"cause": o.cause, "request": o.request}), Value::Null),
        Err(e) => (Value::Null, json!(e)),
    };
    json!({
        "line": check.line,
        "outcome": outcome,
        "error": error,
        "broad": check.broad,
        "can_save": check.can_save(action),
        "file": file.to_string_lossy(),
    })
}

pub fn check_rule(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    match checked(server, req, &captured[0]) {
        Ok((c, target, check, _)) => Reply::json(200, &check_json(&check, &c.action, &target)),
        Err(reply) => reply,
    }
}

pub fn save_rule(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    let _guard = server
        .approvals
        .save
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (c, target, check, request) = match checked(server, req, &captured[0]) {
        Ok(found) => found,
        Err(reply) => return reply,
    };
    if !check.can_save(&c.action) {
        return Reply::json(409, &check_json(&check, &c.action, &target));
    }
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    if let Err(e) = admin::add_rule(&target, &check.line, &date) {
        return Reply::error(500, &format!("{}: {e}", target.display()));
    }
    config::take_warnings();
    Reply::json(
        200,
        &json!({
            "file": target.to_string_lossy(),
            "line": check.line,
            "statuses": statuses(&request, &server.user_config),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::tests::{TOKEN, call, start, temp};
    use std::net::SocketAddr;

    fn node(dir: &Path) -> SocketAddr {
        let mut server = Server::new(TOKEN, Some(dir.join("audit.log")), None);
        server.user_config = dir.join("home/.dippy/config");
        start(server)
    }

    fn payload(dir: &Path, id: &str, deadline: i64) -> Value {
        let cause = |kind: &str, target: &str| json!({"kind": kind, "target": target, "rule": null, "flags": []});
        json!({
            "version": 2, "id": id, "command": "zorp x; echo a > /etc/zorp.conf",
            "cwd": dir.join("proj").to_string_lossy(), "tool": "Bash", "file_path": null,
            "message": "zorp x", "agent": "claude", "server": null, "decision": "ask",
            "reason": "zorp x", "context_flags": [], "flags": [], "remote": false,
            "causes": [cause("command", "zorp x"), cause("redirect", "/etc/zorp.conf")],
            "deadline": deadline,
        })
    }

    const ID: &str = "0123456789abcdef0123456789abcdef";
    const ID2: &str = "fedcba9876543210fedcba9876543210";

    fn post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
        let (status, _, v) = call(addr, "POST", path, Some(TOKEN), Some(&body.to_string()));
        (status, v)
    }

    fn get(addr: SocketAddr, path: &str) -> (u16, Value) {
        let (status, _, v) = call(addr, "GET", path, Some(TOKEN), None);
        (status, v)
    }

    fn setup(name: &str) -> (PathBuf, SocketAddr) {
        let dir = temp(name);
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        let addr = node(&dir);
        (dir, addr)
    }

    #[test]
    fn post_is_idempotent_and_validated() {
        let (dir, addr) = setup("q-post");
        let deadline = now_ms() + 60_000;
        let p = payload(&dir, ID, deadline);
        let (status, item) = post(addr, "/api/approvals", &p);
        assert_eq!(status, 201);
        assert_eq!(item["state"], "pending");
        assert_eq!(item["statuses"], json!(["ask", "ask"]));
        assert_eq!(item["request"], p);
        assert_eq!(
            (item["decided"].clone(), item["note"].clone()),
            (Value::Null, Value::Null)
        );
        let keys: Vec<&str> = item
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ITEM_KEYS);
        assert_eq!(post(addr, "/api/approvals", &p), (200, item.clone()));
        let mut other = p.clone();
        other["command"] = json!("other");
        assert_eq!(post(addr, "/api/approvals", &other).0, 409);
        // Malformed and unknown fields, expired and far deadlines.
        let mut bad = payload(&dir, ID2, deadline);
        bad["cwd"] = json!("rel");
        assert_eq!(post(addr, "/api/approvals", &bad).0, 400);
        let mut bad = payload(&dir, ID2, deadline);
        bad["x"] = json!(1);
        assert_eq!(post(addr, "/api/approvals", &bad).0, 400);
        assert_eq!(
            post(addr, "/api/approvals", &payload(&dir, ID2, now_ms() - 1)).1["error"],
            "deadline is in the past"
        );
        assert_eq!(
            post(
                addr,
                "/api/approvals",
                &payload(&dir, ID2, now_ms() + MAX_AHEAD_MS + 60_000)
            )
            .0,
            400
        );
        assert_eq!(
            call(addr, "POST", "/api/approvals", Some(TOKEN), Some("nope")).0,
            400
        );
        assert_eq!(
            call(addr, "POST", "/api/approvals", Some(TOKEN), Some("[]")).0,
            400
        );
        let big = format!("{{\"x\": \"{}\"}}", "a".repeat(MAX_REQUEST_BODY));
        assert_eq!(
            call(addr, "POST", "/api/approvals", Some(TOKEN), Some(&big)).0,
            400
        );
        assert_eq!(
            call(
                addr,
                "POST",
                "/api/approvals",
                Some("bad"),
                Some(&p.to_string())
            )
            .0,
            401
        );
        assert_eq!(call(addr, "GET", "/api/approvals", None, None).0, 401);
        // Lists.
        assert_eq!(
            get(addr, "/api/approvals?state=pending")
                .1
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(get(addr, "/api/approvals?state=done").1, json!([]));
        assert_eq!(get(addr, "/api/approvals?state=x").0, 400);
        assert_eq!(get(addr, "/api/approvals?foo=1").0, 400);
        assert_eq!(get(addr, &format!("/api/approvals/{ID2}")).0, 404);
        assert_eq!(get(addr, &format!("/api/approvals/{ID}?wait=x")).0, 400);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn decisions_first_wins_and_long_poll_returns() {
        let (dir, addr) = setup("q-decide");
        post(
            addr,
            "/api/approvals",
            &payload(&dir, ID, now_ms() + 60_000),
        );
        let waiter = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let r = get(addr, &format!("/api/approvals/{ID}?wait=8"));
            (r, started.elapsed())
        });
        std::thread::sleep(Duration::from_millis(300));
        let path = format!("/api/approvals/{ID}/decision");
        assert_eq!(post(addr, &path, &json!({"decision": "maybe"})).0, 400);
        assert_eq!(
            post(addr, &path, &json!({"decision": "deny", "x": 1})).0,
            400
        );
        assert_eq!(
            post(addr, &path, &json!({"decision": "deny", "note": 5})).0,
            400
        );
        assert_eq!(
            post(
                addr,
                &path,
                &json!({"decision": "deny", "note": "n".repeat(1001)})
            )
            .0,
            400
        );
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let d = if i % 2 == 0 { "allow" } else { "deny" };
                    post(
                        addr,
                        &path,
                        &json!({"decision": d, "note": format!(" n{i} ")}),
                    )
                })
            })
            .collect();
        let results: Vec<(u16, Value)> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        let winners: Vec<&Value> = results
            .iter()
            .filter(|r| r.0 == 200)
            .map(|r| &r.1)
            .collect();
        assert_eq!(winners.len(), 1);
        assert!(results.iter().all(|r| r.0 == 200 || r.0 == 409));
        // Every 409 shows the final state.
        for (_, v) in &results {
            assert_eq!(v["state"], winners[0]["state"]);
            assert_eq!(v["note"], winners[0]["note"]);
        }
        assert!(winners[0]["note"].as_str().unwrap().starts_with('n'));
        let ((status, item), elapsed) = waiter.join().unwrap();
        assert_eq!(status, 200);
        assert_ne!(item["state"], "pending");
        assert!(elapsed < Duration::from_secs(5));
        assert_eq!(
            get(addr, "/api/approvals?state=done")
                .1
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            post(
                addr,
                &format!("/api/approvals/{ID2}/decision"),
                &json!({"decision": "allow"})
            )
            .0,
            404
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn items_expire_at_the_deadline_and_wait_is_capped() {
        let (dir, addr) = setup("q-expire");
        post(addr, "/api/approvals", &payload(&dir, ID, now_ms() + 700));
        let started = std::time::Instant::now();
        let (status, item) = get(addr, &format!("/api/approvals/{ID}?wait=5"));
        assert_eq!((status, item["state"].as_str()), (200, Some("expired")));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(item["decided"], item["deadline"]);
        let path = format!("/api/approvals/{ID}/decision");
        let (status, late) = post(addr, &path, &json!({"decision": "allow"}));
        assert_eq!((status, late["state"].as_str()), (409, Some("expired")));
        // A short wait on a pending item returns it pending.
        post(
            addr,
            "/api/approvals",
            &payload(&dir, ID2, now_ms() + 60_000),
        );
        let (status, item) = get(addr, &format!("/api/approvals/{ID2}?wait=0.2"));
        assert_eq!((status, item["state"].as_str()), (200, Some("pending")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn check_and_save_use_the_stored_request() {
        let (dir, addr) = setup("q-rule");
        post(
            addr,
            "/api/approvals",
            &payload(&dir, ID, now_ms() + 60_000),
        );
        let check = format!("/api/approvals/{ID}/check");
        let rule = format!("/api/approvals/{ID}/rule");
        let body = |cause: Value, pattern: &str, target: &str| {
            json!({"cause": cause, "action": "allow", "kind": "command", "tags": "",
                   "pattern": pattern, "target": target})
        };
        let (status, c) = post(addr, &check, &body(json!(0), "zorp x", "project"));
        assert_eq!(status, 200);
        let project = dir.join("proj/.dippy");
        assert_eq!(
            c,
            json!({"line": "allow zorp x", "outcome": {"cause": "allow", "request": "ask"},
                   "error": null, "broad": false, "can_save": true,
                   "file": project.to_string_lossy()})
        );
        let (status, c) = post(addr, &check, &body(json!(0), "", "project"));
        assert_eq!(
            (status, c["can_save"].clone(), c["outcome"].clone()),
            (200, json!(false), Value::Null)
        );
        assert!(c["error"].is_string());
        assert_eq!(post(addr, &check, &body(json!(2), "x", "user")).0, 400);
        assert_eq!(post(addr, &check, &body(Value::Null, "x", "user")).0, 400);
        assert_eq!(post(addr, &check, &body(json!(0), "x", "elsewhere")).0, 400);
        let mut extra = body(json!(0), "x", "user");
        extra["x"] = json!(1);
        assert_eq!(post(addr, &check, &extra).0, 400);
        // Saving a rule that does not resolve the cause is refused.
        let (status, refused) = post(addr, &rule, &body(json!(0), "frob", "project"));
        assert_eq!((status, refused["can_save"].clone()), (409, json!(false)));
        assert!(!project.exists());
        // Saved line == checked line; statuses refresh.
        let (status, saved) = post(addr, &rule, &body(json!(0), "zorp x", "project"));
        assert_eq!(status, 200);
        assert_eq!(saved["line"], "allow zorp x");
        assert_eq!(saved["statuses"], json!(["allow", "ask"]));
        let text = std::fs::read_to_string(&project).unwrap();
        assert!(text.ends_with("\nallow zorp x\n") && text.starts_with("# added by dippy "));
        assert_eq!(
            get(addr, &format!("/api/approvals/{ID}")).1["statuses"],
            json!(["allow", "ask"])
        );
        // The user target writes the node's user config.
        let redirect = json!({"cause": 1, "action": "allow", "kind": "redirect", "tags": "",
                              "pattern": "/etc/zorp.conf", "target": "user"});
        let (status, saved) = post(addr, &rule, &redirect);
        assert_eq!(
            (status, saved["statuses"].clone()),
            (200, json!(["allow", "allow"]))
        );
        assert!(dir.join("home/.dippy/config").exists());
        // Unknown id.
        assert_eq!(
            post(addr, &format!("/api/approvals/{ID2}/check"), &redirect).0,
            404
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn done_items_are_capped() {
        let q = Queue::default();
        for i in 0..(KEEP_DONE + 5) {
            let id = format!("{i:032x}");
            q.add(json!({"id": id, "deadline": now_ms() + 60_000}))
                .unwrap();
            q.decide(&id, true, None).unwrap().unwrap();
        }
        q.add(json!({"id": "f".repeat(32), "deadline": now_ms() + 60_000}))
            .unwrap();
        assert_eq!(q.list(Some(false)).len(), KEEP_DONE);
        assert_eq!(q.list(Some(true)).len(), 1);
        assert_eq!(q.list(None)[0].id, "f".repeat(32));
        assert!(q.get(&format!("{:032x}", 0)).is_none());
        assert!(q.get(&format!("{:032x}", KEEP_DONE + 4)).is_some());
    }
}
