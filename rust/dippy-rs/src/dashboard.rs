//! Port of `dippy.dashboard`: a live web view of the audit log, as a node or
//! as a hub over nodes.
//!
//! A node reads the log Dippy already writes. A hub has no log: it keeps a
//! registry of node URLs and tokens and proxies the allowlisted node API, so
//! the same page shows one machine or all of them. Every `/api/*` and `/n/*`
//! request requires the server's token. The REST contract is
//! `docs/dashboard-api.json`; a test compares it with [`ROUTES`].

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::NaiveDate;
use serde_json::{Map, Value, json};

use crate::hook::py_dumps;
use crate::http::{self, Request};
use crate::{audit, config, paths, queue};

/// Audit log decisions; `error` is a hook panic.
pub const DECISIONS: [&str; 5] = ["allow", "ask", "deny", "pass", "error"];
pub const DEFAULT_LIMIT: i64 = 200;
pub const MAX_LIMIT: i64 = 1000;
pub const MAX_NODE_RESPONSE: usize = 8 * 1024 * 1024;
pub const MAX_REQUEST_BODY: usize = 64 * 1024;
pub const NODE_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_TOKEN_FILE: &str = "~/.dippy/dashboard-token";
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 8765;
const PARAMETERS: [&str; 6] = ["limit", "decision", "not_allow", "agent", "cwd", "grep"];

const SECURITY_HEADERS: [(&str, &str); 4] = [
    ("X-Content-Type-Options", "nosniff"),
    ("Cache-Control", "no-store"),
    ("Referrer-Policy", "no-referrer"),
    (
        "Content-Security-Policy",
        "default-src 'self'; frame-ancestors 'none'",
    ),
];

const STATIC: [(&str, &str, &str); 3] = [
    (
        "/",
        include_str!("dashboard_static/index.html"),
        "text/html; charset=utf-8",
    ),
    (
        "/app.js",
        include_str!("dashboard_static/app.js"),
        "text/javascript; charset=utf-8",
    ),
    (
        "/app.css",
        include_str!("dashboard_static/app.css"),
        "text/css; charset=utf-8",
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Node,
    Hub,
}

/// What a handler answers: status and JSON body bytes.
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, v: &Value) -> Reply {
        Reply {
            status,
            body: py_dumps(v).into_bytes(),
        }
    }
    pub fn error(status: u16, message: &str) -> Reply {
        Reply::json(status, &json!({"error": message}))
    }
}

type Handler = fn(&Server, &mut Request, &[String]) -> Reply;

/// (method, path template, server mode, handler). `{name}`/`{id}` is one
/// path segment. The contract test compares this table with
/// `docs/dashboard-api.json`.
pub const ROUTES: &[(&str, &str, Mode, Handler)] = &[
    ("GET", "/api/entries", Mode::Node, get_entries),
    ("GET", "/api/nodes", Mode::Hub, get_nodes),
    ("PUT", "/api/nodes/{name}", Mode::Hub, put_node),
    ("DELETE", "/api/nodes/{name}", Mode::Hub, delete_node),
    ("GET", "/n/{name}/api/entries", Mode::Hub, proxy_entries),
    ("POST", "/api/approvals", Mode::Node, queue::post_approval),
    ("GET", "/api/approvals", Mode::Node, queue::list_approvals),
    (
        "GET",
        "/api/approvals/{id}",
        Mode::Node,
        queue::get_approval,
    ),
    (
        "POST",
        "/api/approvals/{id}/decision",
        Mode::Node,
        queue::decide_approval,
    ),
    (
        "POST",
        "/api/approvals/{id}/check",
        Mode::Node,
        queue::check_rule,
    ),
    (
        "POST",
        "/api/approvals/{id}/rule",
        Mode::Node,
        queue::save_rule,
    ),
    ("GET", "/n/{name}/api/approvals", Mode::Hub, proxy_approvals),
    (
        "POST",
        "/n/{name}/api/approvals/{id}/decision",
        Mode::Hub,
        proxy_decision,
    ),
    (
        "POST",
        "/n/{name}/api/approvals/{id}/check",
        Mode::Hub,
        proxy_check,
    ),
    (
        "POST",
        "/n/{name}/api/approvals/{id}/rule",
        Mode::Hub,
        proxy_rule,
    ),
];

/// Path segments of `path` captured by `template`, if it matches.
fn match_route(template: &str, path: &str) -> Option<Vec<String>> {
    let (t, p): (Vec<&str>, Vec<&str>) = (template.split('/').collect(), path.split('/').collect());
    if t.len() != p.len() {
        return None;
    }
    let mut captured = Vec::new();
    for (t, p) in t.iter().zip(&p) {
        if t.starts_with('{') {
            if p.is_empty() {
                return None;
            }
            captured.push(p.to_string());
        } else if t != p {
            return None;
        }
    }
    Some(captured)
}

pub struct Server {
    pub token: String,
    pub mode: Mode,
    pub log: Option<PathBuf>,
    pub nodes: Option<PathBuf>,
    nodes_lock: Mutex<()>,
    /// Fixed "today" (UTC) for tests.
    pub today: Option<NaiveDate>,
    /// Node: the approval queue (in memory).
    pub approvals: queue::Queue,
    /// The user config rules are checked with and saved to.
    pub user_config: PathBuf,
}

impl Server {
    pub fn new(token: &str, log: Option<PathBuf>, nodes: Option<PathBuf>) -> Server {
        Server {
            token: token.to_string(),
            mode: if nodes.is_some() {
                Mode::Hub
            } else {
                Mode::Node
            },
            log,
            nodes,
            nodes_lock: Mutex::new(()),
            today: None,
            approvals: queue::Queue::default(),
            user_config: config::user_config_path(),
        }
    }

    fn today(&self) -> NaiveDate {
        self.today
            .unwrap_or_else(|| chrono::Utc::now().date_naive())
    }

    fn authorized(&self, req: &Request) -> bool {
        let given = req.header("X-Dippy-Token").unwrap_or("").as_bytes();
        let want = self.token.as_bytes();
        // Constant time over the expected token (hmac.compare_digest).
        let mut diff = given.len() ^ want.len();
        for (i, b) in want.iter().enumerate() {
            diff |= (*b ^ given.get(i).copied().unwrap_or(0)) as usize;
        }
        diff == 0
    }

    /// Answer one request on `stream`.
    pub fn handle(&self, stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let Some(mut req) = http::read_request(stream) else {
            return;
        };
        let (status, content_type, body) = self.dispatch(&mut req);
        let _ = http::respond(req.stream(), status, content_type, &SECURITY_HEADERS, &body);
        req.finish();
    }

    fn dispatch(&self, req: &mut Request) -> (u16, &'static str, Vec<u8>) {
        const JSON: &str = "application/json";
        if !matches!(req.method.as_str(), "GET" | "PUT" | "DELETE" | "POST") {
            let r = Reply::error(501, &format!("Unsupported method ('{}')", req.method));
            return (501, JSON, r.body);
        }
        if req.method == "GET"
            && let Some((_, text, content_type)) = STATIC.iter().find(|(p, _, _)| *p == req.path)
        {
            return (200, content_type, text.as_bytes().to_vec());
        }
        let found = ROUTES.iter().find_map(|(method, template, mode, handler)| {
            let captured = match_route(template, &req.path)?;
            (*method == req.method && *mode == self.mode).then_some((handler, captured))
        });
        let Some((handler, captured)) = found else {
            return (404, JSON, Reply::error(404, "not found").body);
        };
        if !self.authorized(req) {
            return (
                401,
                JSON,
                Reply::error(401, "missing or invalid token").body,
            );
        }
        let reply = handler(self, req, &captured);
        (reply.status, JSON, reply.body)
    }
}

// --------------------------------------------------------------- entries

fn single<'a>(params: &'a [(String, Vec<String>)], name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(n, _)| n == name)
        .and_then(|(_, v)| v.last())
        .map(String::as_str)
}

/// Python `int(str)` for a query value.
fn py_int(value: &str) -> Option<i64> {
    let v = value.trim();
    let digits = v.strip_prefix(['+', '-']).unwrap_or(v);
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.chars().all(|c| c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    v.replace('_', "").parse().ok()
}

/// `entries_query`: audit entries since yesterday (UTC) matching the URL
/// parameters, oldest first. `Err((400|500, message))`.
pub fn entries_query(
    log: &Path,
    query: &str,
    today: NaiveDate,
) -> Result<Vec<Value>, (u16, String)> {
    let params = http::parse_qs(query);
    let bad = |m: String| Err((400, m));
    let mut unknown: Vec<&str> = params
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| !PARAMETERS.contains(n))
        .collect();
    unknown.sort();
    if !unknown.is_empty() {
        return bad(format!("unknown parameter: {}", unknown.join(", ")));
    }
    let limit = match single(&params, "limit") {
        None => DEFAULT_LIMIT,
        Some(raw) => match py_int(raw) {
            Some(n) => n,
            None => return bad(format!("limit must be an integer: {raw}")),
        },
    };
    if !(1..=MAX_LIMIT).contains(&limit) {
        return bad(format!("limit must be between 1 and {MAX_LIMIT}: {limit}"));
    }
    let decisions: Vec<String> = params
        .iter()
        .find(|(n, _)| n == "decision")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    if decisions.iter().any(|d| !DECISIONS.contains(&d.as_str())) {
        return bad(format!("decision must be one of {}", DECISIONS.join(", ")));
    }
    let not_allow = single(&params, "not_allow");
    if not_allow.is_some_and(|v| v != "1") {
        return bad("not_allow must be 1".into());
    }
    let cwd = single(&params, "cwd");
    if let Some(cwd) = cwd.filter(|c| !c.starts_with('/')) {
        return bad(format!("cwd must be an absolute path: {cwd}"));
    }
    let q = audit::Query {
        since: Some(
            (today - chrono::Days::new(1))
                .format("%Y-%m-%d")
                .to_string(),
        ),
        decisions,
        not_allow: not_allow == Some("1"),
        agent: single(&params, "agent").map(str::to_string),
        cwd: cwd.map(str::to_string),
        grep: single(&params, "grep").map(str::to_string),
        limit: Some(limit),
        ..audit::Query::default()
    };
    let lines = audit::query(log, &q, Path::new("/"))
        .map_err(|e| (500, format!("cannot read audit log: {e}")))?;
    Ok(lines
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

fn get_entries(server: &Server, req: &mut Request, _: &[String]) -> Reply {
    let log = server.log.as_deref().unwrap_or(Path::new(""));
    match entries_query(log, &req.query, server.today()) {
        Ok(entries) => Reply::json(200, &Value::Array(entries)),
        Err((status, message)) => Reply::error(status, &message),
    }
}

// --------------------------------------------------------------- registry

/// One registered node.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub url: String,
    pub token: String,
}

/// `_NODE_NAME`.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.chars().count() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// `_validate_node`.
pub fn validate_node(name: &str, body: &Value) -> Result<Node, String> {
    if !valid_name(name) {
        return Err("name must be 1-64 letters, digits, '_', '.' or '-', \
                    starting with a letter or digit"
            .into());
    }
    let Some(body) = body.as_object() else {
        return Err("body must be a JSON object with url and token".into());
    };
    let (url, token) = match (body.get("url"), body.get("token")) {
        (Some(Value::String(u)), Some(Value::String(t))) if !t.is_empty() => (u, t),
        _ => return Err("url and token must be non-empty strings".into()),
    };
    http::validate_url(url)?;
    Ok(Node {
        url: url.clone(),
        token: token.clone(),
    })
}

/// `_read_nodes`: validated entries in file order; any invalid content is
/// an error naming the file.
pub fn read_nodes(path: &Path) -> Result<Vec<(String, Node)>, String> {
    let fail = |e: String| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(fail(e.to_string())),
    };
    let data: Value = serde_json::from_str(&text).map_err(|e| fail(e.to_string()))?;
    let shape = || fail(r#"expected {"nodes": {NAME: {"url", "token"}}}"#.into());
    let Some(data) = data.as_object() else {
        return Err(shape());
    };
    let empty = Map::new();
    let nodes = match data.get("nodes") {
        None => &empty,
        Some(Value::Object(nodes)) => nodes,
        Some(_) => return Err(shape()),
    };
    nodes
        .iter()
        .map(|(name, node)| Ok((name.clone(), validate_node(name, node).map_err(fail)?)))
        .collect()
}

/// Python `json.dumps(value, indent=2)`.
fn py_dumps_indent(v: &Value, level: usize) -> String {
    let pad = |n: usize| " ".repeat(n * 2);
    match v {
        Value::Object(o) if !o.is_empty() => {
            let items: Vec<String> = o
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}{}: {}",
                        pad(level + 1),
                        crate::hook::py_json_str(k),
                        py_dumps_indent(v, level + 1)
                    )
                })
                .collect();
            format!("{{\n{}\n{}}}", items.join(",\n"), pad(level))
        }
        Value::Array(a) if !a.is_empty() => {
            let items: Vec<String> = a
                .iter()
                .map(|v| format!("{}{}", pad(level + 1), py_dumps_indent(v, level + 1)))
                .collect();
            format!("[\n{}\n{}]", items.join(",\n"), pad(level))
        }
        other => py_dumps(other),
    }
}

fn write_nodes(path: &Path, nodes: &[(String, Node)]) -> std::io::Result<()> {
    let map: Map<String, Value> = nodes
        .iter()
        .map(|(name, n)| (name.clone(), json!({"url": n.url, "token": n.token})))
        .collect();
    let text = py_dumps_indent(&json!({"nodes": map}), 0);
    crate::admin::write_atomic_mode(path, &text, Some(0o600))
}

fn nodes_path(server: &Server) -> &Path {
    server.nodes.as_deref().unwrap_or(Path::new(""))
}

fn get_nodes(server: &Server, _: &mut Request, _: &[String]) -> Reply {
    match read_nodes(nodes_path(server)) {
        Ok(nodes) => Reply::json(
            200,
            &Value::Array(
                nodes
                    .iter()
                    .map(|(name, n)| json!({"name": name, "url": n.url}))
                    .collect(),
            ),
        ),
        Err(e) => Reply::error(500, &e),
    }
}

fn put_node(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    let name = &captured[0];
    let body = match req.body(MAX_REQUEST_BODY) {
        Ok(body) => body,
        Err(e) => return Reply::error(400, &e),
    };
    let body: Value = if body.is_empty() {
        Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => return Reply::error(400, "body must be JSON"),
        }
    };
    let node = match validate_node(name, &body) {
        Ok(node) => node,
        Err(e) => return Reply::error(400, &e),
    };
    let _guard = server.nodes_lock.lock().unwrap_or_else(|e| e.into_inner());
    let path = nodes_path(server);
    let mut nodes = match read_nodes(path) {
        Ok(nodes) => nodes,
        Err(e) => return Reply::error(500, &e),
    };
    match nodes.iter_mut().find(|(n, _)| n == name) {
        Some(entry) => entry.1 = node.clone(),
        None => nodes.push((name.clone(), node.clone())),
    }
    if let Err(e) = write_nodes(path, &nodes) {
        return Reply::error(500, &e.to_string());
    }
    Reply::json(200, &json!({"name": name, "url": node.url}))
}

fn delete_node(server: &Server, _: &mut Request, captured: &[String]) -> Reply {
    let name = &captured[0];
    let _guard = server.nodes_lock.lock().unwrap_or_else(|e| e.into_inner());
    let path = nodes_path(server);
    let mut nodes = match read_nodes(path) {
        Ok(nodes) => nodes,
        Err(e) => return Reply::error(500, &e),
    };
    let Some(i) = nodes.iter().position(|(n, _)| n == name) else {
        return Reply::error(404, &format!("unknown node: {name}"));
    };
    nodes.remove(i);
    if let Err(e) = write_nodes(path, &nodes) {
        return Reply::error(500, &e.to_string());
    }
    Reply::json(200, &json!({"name": name}))
}

// ------------------------------------------------------------------ proxy

/// `_fetch_node`: status and JSON body of a node API call. No environment
/// proxy, no redirects, 5 s timeouts, response capped.
pub fn fetch_node(
    node: &Node,
    method: &str,
    path: &str,
    query: &str,
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>), String> {
    let target = http::Target::parse(&node.url).map_err(|e| format!("node unreachable: {e}"))?;
    let path_and_query = if query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query}")
    };
    let mut headers = vec![("X-Dippy-Token", node.token.as_str())];
    if body.is_some() {
        headers.push(("Content-Type", "application/json"));
    }
    let timeouts = http::Timeouts::within(NODE_TIMEOUT, NODE_TIMEOUT);
    let response = http::request(
        &target,
        method,
        &path_and_query,
        &headers,
        body,
        timeouts,
        MAX_NODE_RESPONSE,
    )
    .map_err(|e| match e {
        http::ClientError::TooLarge => "node response too large".to_string(),
        http::ClientError::Indeterminate(m) => format!("node read failed: {m}"),
        other => format!("node unreachable: {other}"),
    })?;
    if (300..400).contains(&response.status) {
        return Err(format!("node redirected (HTTP {})", response.status));
    }
    if serde_json::from_slice::<Value>(&response.body).is_err() {
        return Err("node response is not JSON".into());
    }
    Ok((response.status, response.body))
}

fn registered(server: &Server, name: &str) -> Result<Node, Reply> {
    match read_nodes(nodes_path(server)) {
        Err(e) => Err(Reply::error(500, &e)),
        Ok(nodes) => nodes
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, node)| node)
            .ok_or_else(|| Reply::error(404, &format!("unknown node: {name}"))),
    }
}

fn proxy_entries(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    let node = match registered(server, &captured[0]) {
        Ok(node) => node,
        Err(reply) => return reply,
    };
    match fetch_node(&node, "GET", "/api/entries", &req.query, None) {
        Ok((status, body)) => Reply { status, body },
        Err(e) => Reply::error(502, &e),
    }
}

/// Hub: the node's approval list. The UI polls it; nothing long-polls
/// through the hub (the proxy timeout is 5 s).
fn proxy_approvals(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    let node = match registered(server, &captured[0]) {
        Ok(node) => node,
        Err(reply) => return reply,
    };
    match fetch_node(&node, "GET", "/api/approvals", &req.query, None) {
        Ok((status, body)) => Reply { status, body },
        Err(e) => Reply::error(502, &e),
    }
}

/// Hub: forward a decision, check or rule body to the owning node.
fn proxy_approval_post(
    server: &Server,
    req: &mut Request,
    captured: &[String],
    action: &str,
) -> Reply {
    let (name, id) = (&captured[0], &captured[1]);
    if !crate::approval::valid_id(id) {
        return Reply::error(404, &format!("unknown approval: {id}"));
    }
    let node = match registered(server, name) {
        Ok(node) => node,
        Err(reply) => return reply,
    };
    let body = match req.body(MAX_REQUEST_BODY) {
        Ok(body) => body,
        Err(e) => return Reply::error(400, &e),
    };
    let path = format!("/api/approvals/{id}/{action}");
    match fetch_node(&node, "POST", &path, "", Some(&body)) {
        Ok((status, body)) => Reply { status, body },
        Err(e) => Reply::error(502, &e),
    }
}

fn proxy_decision(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    proxy_approval_post(server, req, captured, "decision")
}

fn proxy_check(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    proxy_approval_post(server, req, captured, "check")
}

fn proxy_rule(server: &Server, req: &mut Request, captured: &[String]) -> Reply {
    proxy_approval_post(server, req, captured, "rule")
}

// ------------------------------------------------------------------ serve

fn warn_if_shared(path: &Path) -> std::io::Result<()> {
    if std::fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
        eprintln!(
            "dashboard: warning: {} is readable by group or others",
            path.display()
        );
    }
    Ok(())
}

fn random_bytes(n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0; n];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf)
}

/// `secrets.token_urlsafe(32)`: 43 URL-safe base64 characters.
pub fn new_token() -> std::io::Result<String> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = random_bytes(32)?;
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    Ok(out)
}

/// `load_token`: the token stored in `path`, creating a private one when
/// missing.
pub fn load_token(path: &Path) -> Result<String, String> {
    let err = |e: std::io::Error| e.to_string();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(err)?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(mut file) => {
            let token = new_token().map_err(err)?;
            file.write_all(format!("{token}\n").as_bytes())
                .map_err(err)?;
            Ok(token)
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            let token = std::fs::read_to_string(path).map_err(err)?;
            let token = token.trim();
            if token.is_empty() {
                return Err(format!("token file is empty: {}", path.display()));
            }
            warn_if_shared(path).map_err(err)?;
            Ok(token.to_string())
        }
        Err(e) => Err(err(e)),
    }
}

/// Accept connections until the listener fails; one thread per request.
pub fn serve_on(listener: TcpListener, server: Arc<Server>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = Arc::clone(&server);
        std::thread::spawn(move || server.handle(stream));
    }
}

#[derive(clap::Args)]
pub struct DashboardArgs {
    /// Run as a hub proxying the nodes registered in this JSON file
    #[arg(long, value_name = "NODES_FILE")]
    hub: Option<String>,
    /// API token, created when missing (default: ~/.dippy/dashboard-token)
    #[arg(long, value_name = "PATH")]
    token_file: Option<String>,
    /// Listen address (default: 127.0.0.1)
    #[arg(long, default_value = DEFAULT_HOST)]
    host: String,
    /// Listen port (default: 8765)
    #[arg(long, default_value_t = DEFAULT_PORT as i64, allow_negative_numbers = true)]
    port: i64,
}

/// `handle_dashboard_subcommand` and `serve`.
pub fn run(args: &DashboardArgs, global: &crate::run::Global) -> i32 {
    let token_file = PathBuf::from(paths::expanduser(
        args.token_file.as_deref().unwrap_or(DEFAULT_TOKEN_FILE),
    ));
    let (log, nodes) = match &args.hub {
        Some(hub) => (None, Some(PathBuf::from(paths::expanduser(hub)))),
        None => {
            let cwd = match global.cwd {
                Some(c) => paths::resolve(Path::new(c)),
                None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            };
            let loaded = config::load_config(&cwd, global.config, global.config_only);
            config::print_warnings();
            let config = match loaded {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("config error: {e}");
                    return 1;
                }
            };
            let log = config.log.unwrap_or_else(config::default_log_path);
            (Some(log), None)
        }
    };
    let token = match load_token(&token_file).and_then(|token| {
        if let Some(nodes) = nodes.as_deref().filter(|n| n.exists()) {
            read_nodes(nodes)?;
            warn_if_shared(nodes).map_err(|e| e.to_string())?;
        }
        Ok(token)
    }) {
        Ok(token) => token,
        Err(e) => {
            eprintln!("dashboard: {e}");
            return 1;
        }
    };
    let host = &args.host;
    let listener = u16::try_from(args.port)
        .map_err(|_| "port must be 0-65535".to_string())
        .and_then(|port| TcpListener::bind((host.as_str(), port)).map_err(|e| e.to_string()));
    let listener = match listener {
        Ok(l) => l,
        Err(e) => {
            eprintln!("dashboard: cannot listen on {host}:{}: {e}", args.port);
            return 1;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let kind = match &nodes {
        Some(n) => format!("hub ({})", n.display()),
        None => "node".to_string(),
    };
    println!("Dippy dashboard {kind}: http://{host}:{port}/#token={token}");
    println!("Press Ctrl+C to stop.");
    let _ = std::io::stdout().flush();
    serve_on(listener, Arc::new(Server::new(&token, log, nodes)));
    0
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::net::SocketAddr;

    pub const TOKEN: &str = "secret-token";

    #[test]
    fn page_shows_all_decisions_by_default() {
        let page = STATIC[0].1;
        assert!(page.contains(r#"<option value="" selected>all decisions</option>"#));
        assert_eq!(page.matches(" selected>").count(), 1);
    }

    pub fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dippy-rs-dashboard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn start(server: Server) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = Arc::new(server);
        std::thread::spawn(move || serve_on(listener, server));
        addr
    }

    /// Raw request; returns status, headers and body.
    pub fn call(
        addr: SocketAddr,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
    ) -> (u16, String, Value) {
        let mut stream = TcpStream::connect(addr).unwrap();
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n");
        if let Some(t) = token {
            head.push_str(&format!("X-Dippy-Token: {t}\r\n"));
        }
        if let Some(b) = body {
            head.push_str(&format!("Content-Length: {}\r\n", b.len()));
        }
        head.push_str("\r\n");
        head.push_str(body.unwrap_or(""));
        stream.write_all(head.as_bytes()).unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        let status = headers[9..12].parse().unwrap();
        let body = serde_json::from_str(body).unwrap_or(Value::String(body.to_string()));
        (status, headers.to_string(), body)
    }

    fn log_dir(name: &str) -> PathBuf {
        let dir = temp(name);
        let entries = [
            (
                "audit-2026-10-01.log",
                vec![
                    json!({"decision": "ask", "cmd": "old", "cwd": "/w", "ts": "2026-10-01T08:00:00+00:00"}),
                ],
            ),
            (
                "audit-2026-10-02.log",
                vec![
                    json!({"decision": "allow", "cmd": "ls", "cwd": "/w", "agent": "agy", "ts": "2026-10-02T08:00:00+00:00"}),
                ],
            ),
            (
                "audit.log",
                vec![
                    json!({"decision": "ask", "cmd": "curl", "cwd": "/w/proj", "agent": "claude", "ts": "2026-10-03T09:00:00+00:00"}),
                    json!({"decision": "deny", "cmd": "rm", "cwd": "/other", "agent": "codex", "ts": "2026-10-03T10:00:00+00:00"}),
                ],
            ),
        ];
        for (name, list) in entries {
            let text: String = list.iter().map(|e| format!("{e}\n")).collect();
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    fn cmds(v: &Value) -> Vec<&str> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|e| e["cmd"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn node_serves_filtered_entries_with_token() {
        let dir = log_dir("log-node");
        let mut server = Server::new(TOKEN, Some(dir.join("audit.log")), None);
        server.today = NaiveDate::from_ymd_opt(2026, 10, 3);
        let addr = start(server);
        for token in [None, Some(""), Some("wrong")] {
            let (status, headers, body) = call(addr, "GET", "/api/entries", token, None);
            assert_eq!(status, 401);
            assert!(headers.contains("Content-Type: application/json"));
            assert!(body["error"].as_str().unwrap().contains("token"));
        }
        let get = |q: &str| call(addr, "GET", &format!("/api/entries{q}"), Some(TOKEN), None);
        assert_eq!(cmds(&get("").2), ["ls", "curl", "rm"]);
        assert_eq!(cmds(&get("?decision=ask&decision=deny").2), ["curl", "rm"]);
        assert_eq!(cmds(&get("?not_allow=1").2), ["curl", "rm"]);
        assert_eq!(cmds(&get("?agent=codex").2), ["rm"]);
        assert!(cmds(&get("?decision=error").2).is_empty());
        assert_eq!(cmds(&get("?cwd=/w").2), ["ls", "curl"]);
        assert_eq!(cmds(&get("?grep=cur").2), ["curl"]);
        assert_eq!(cmds(&get("?limit=1").2), ["rm"]);
        assert_eq!(cmds(&get("?agent=").2), ["ls", "curl", "rm"]);
        for q in [
            "?limit=x",
            "?limit=0",
            "?limit=1001",
            "?decision=maybe",
            "?cwd=rel/p",
            "?not_allow=yes",
            "?since=2026-10-01",
        ] {
            let (status, _, body) = get(q);
            assert_eq!(status, 400, "{q}");
            assert!(body["error"].is_string());
        }
        assert_eq!(
            get("?since=x&limit=y").2["error"],
            "unknown parameter: since"
        );
        assert_eq!(call(addr, "GET", "/nope", Some(TOKEN), None).0, 404);
        assert_eq!(call(addr, "GET", "/api/nodes", Some(TOKEN), None).0, 404);
        assert_eq!(call(addr, "HEAD", "/api/entries", Some(TOKEN), None).0, 501);
        for (path, ty) in [
            ("/", "text/html"),
            ("/app.js", "text/javascript"),
            ("/app.css", "text/css"),
        ] {
            let (status, headers, _) = call(addr, "GET", path, None, None);
            assert_eq!(status, 200);
            assert!(headers.contains(&format!("Content-Type: {ty}; charset=utf-8")));
            assert!(headers.contains("X-Content-Type-Options: nosniff"));
            assert!(headers.contains("Cache-Control: no-store"));
            assert!(headers.contains("default-src 'self'"));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn token_file_is_private_and_reused() {
        let dir = temp("token");
        let path = dir.join("sub/token");
        let token = load_token(&path).unwrap();
        assert_eq!(token.len(), 43);
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(load_token(&path).unwrap(), token);
        std::fs::write(&path, " \n").unwrap();
        assert!(
            load_token(&path)
                .unwrap_err()
                .contains("token file is empty")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn hub(dir: &Path) -> SocketAddr {
        start(Server::new(TOKEN, None, Some(dir.join("nodes.json"))))
    }

    #[test]
    fn registry_add_list_delete_and_hand_edits() {
        let dir = temp("registry");
        let addr = hub(&dir);
        let nodes = dir.join("nodes.json");
        assert_eq!(
            call(addr, "GET", "/api/nodes", Some(TOKEN), None).2,
            json!([])
        );
        assert_eq!(call(addr, "GET", "/api/entries", Some(TOKEN), None).0, 404);
        let put = |name: &str, body: &str| {
            call(
                addr,
                "PUT",
                &format!("/api/nodes/{name}"),
                Some(TOKEN),
                Some(body),
            )
        };
        let (status, _, body) = put("vm1", r#"{"url": "http://h:1", "token": "t1"}"#);
        assert_eq!(
            (status, body),
            (200, json!({"name": "vm1", "url": "http://h:1"}))
        );
        put("vm2", r#"{"url": "https://h2/base", "token": "t2"}"#);
        put("vm1", r#"{"url": "http://h:9", "token": "t3"}"#);
        assert_eq!(
            call(addr, "GET", "/api/nodes", Some(TOKEN), None).2,
            json!([{"name": "vm1", "url": "http://h:9"}, {"name": "vm2", "url": "https://h2/base"}])
        );
        assert_eq!(
            std::fs::metadata(&nodes).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::read_to_string(&nodes).unwrap(),
            "{\n  \"nodes\": {\n    \"vm1\": {\n      \"url\": \"http://h:9\",\n      \"token\": \"t3\"\n    },\n    \"vm2\": {\n      \"url\": \"https://h2/base\",\n      \"token\": \"t2\"\n    }\n  }\n}"
        );
        assert_eq!(
            call(addr, "DELETE", "/api/nodes/vm1", Some(TOKEN), None).2,
            json!({"name": "vm1"})
        );
        assert_eq!(
            call(addr, "DELETE", "/api/nodes/vm1", Some(TOKEN), None).0,
            404
        );
        assert_eq!(
            call(addr, "DELETE", "/api/nodes/vm2", Some("x"), None).0,
            401
        );
        std::fs::write(
            &nodes,
            r#"{"nodes": {"hand": {"url": "http://e:2", "token": "x"}}}"#,
        )
        .unwrap();
        std::fs::set_permissions(&nodes, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            call(addr, "GET", "/api/nodes", Some(TOKEN), None).2,
            json!([{"name": "hand", "url": "http://e:2"}])
        );
        put("vm3", r#"{"url": "http://h:3", "token": "t"}"#);
        assert_eq!(
            std::fs::metadata(&nodes).unwrap().permissions().mode() & 0o777,
            0o600
        );
        for bad in [
            "not json",
            "[]",
            r#"{"nodes": []}"#,
            r#"{"nodes": {"x": "http://h:1"}}"#,
            r#"{"nodes": {"x": {"url": "file:///etc/passwd", "token": "t"}}}"#,
            r#"{"nodes": {"..": {"url": "http://h:1", "token": "t"}}}"#,
        ] {
            std::fs::write(&nodes, bad).unwrap();
            for path in ["/api/nodes", "/n/x/api/entries"] {
                let (status, _, body) = call(addr, "GET", path, Some(TOKEN), None);
                assert_eq!(status, 500, "{bad}");
                assert!(
                    body["error"]
                        .as_str()
                        .unwrap()
                        .contains(&nodes.display().to_string())
                );
            }
        }
        std::fs::remove_file(&nodes).unwrap();
        for name in ["..", ".hidden", "-x", "a%2Fb", &"x".repeat(65), "a%20b"] {
            assert_eq!(
                put(name, r#"{"url": "http://h:1", "token": "t"}"#).0,
                400,
                "{name}"
            );
        }
        for body in [
            r#"{"url": "ftp://h/", "token": "t"}"#,
            r#"{"url": "http:///nohost", "token": "t"}"#,
            r#"{"url": "http://user:pw@h/", "token": "t"}"#,
            r#"{"url": "http://h/?q=1", "token": "t"}"#,
            r#"{"url": "http://h/#f", "token": "t"}"#,
            r#"{"url": "http://h/", "token": ""}"#,
            r#"{"url": "http://h/"}"#,
            r#"{"url": 5, "token": "t"}"#,
            "nope",
            "",
        ] {
            assert_eq!(put("vm1", body).0, 400, "{body}");
        }
        let (status, _, body) = call(addr, "PUT", "/api/nodes/vm1", Some(TOKEN), None);
        assert_eq!(
            (status, body["error"].as_str()),
            (400, Some("body must be a JSON object with url and token"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn negative_content_length_is_rejected() {
        let dir = temp("neglen");
        let addr = hub(&dir);
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("PUT /api/nodes/vm1 HTTP/1.1\r\nX-Dippy-Token: {TOKEN}\r\nContent-Length: -1\r\n\r\n").as_bytes()).unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.1 400"), "{raw}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concurrent_registry_writes_keep_every_node() {
        let dir = temp("concurrent");
        let addr = hub(&dir);
        let threads: Vec<_> = (0..20)
            .map(|i| {
                std::thread::spawn(move || {
                    call(
                        addr,
                        "PUT",
                        &format!("/api/nodes/n{i}"),
                        Some(TOKEN),
                        Some(&format!(r#"{{"url": "http://h:{}", "token": "t"}}"#, i + 1)),
                    )
                })
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap().0, 200);
        }
        assert_eq!(
            call(addr, "GET", "/api/nodes", Some(TOKEN), None)
                .2
                .as_array()
                .unwrap()
                .len(),
            20
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A one-shot fake node answering with a raw response.
    fn fake_node(response: &'static str) -> (SocketAddr, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            s.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (addr, handle)
    }

    #[test]
    fn proxy_forwards_query_and_token_and_relays() {
        let dir = log_dir("log-proxy");
        let mut node = Server::new("node-token", Some(dir.join("audit.log")), None);
        node.today = NaiveDate::from_ymd_opt(2026, 10, 3);
        let node = start(node);
        let hubdir = temp("proxy");
        let addr = hub(&hubdir);
        let put = |name: &str, url: String, token: &str| {
            call(
                addr,
                "PUT",
                &format!("/api/nodes/{name}"),
                Some(TOKEN),
                Some(&json!({"url": url, "token": token}).to_string()),
            )
        };
        put("vm1", format!("http://{node}/"), "node-token");
        let (status, _, body) = call(
            addr,
            "GET",
            "/n/vm1/api/entries?agent=codex",
            Some(TOKEN),
            None,
        );
        assert_eq!((status, cmds(&body)), (200, vec!["rm"]));
        assert_eq!(
            call(addr, "GET", "/n/vm1/api/entries?limit=0", Some(TOKEN), None).0,
            400
        );
        assert_eq!(
            call(addr, "GET", "/n/vm1/api/entries", Some("bad"), None).0,
            401
        );
        put("bad", format!("http://{node}/"), "wrong");
        assert_eq!(
            call(addr, "GET", "/n/bad/api/entries", Some(TOKEN), None).0,
            401
        );
        assert_eq!(
            call(addr, "GET", "/n/nope/api/entries", Some(TOKEN), None).0,
            404
        );
        assert_eq!(
            call(addr, "GET", "/n/vm1/api/nodes", Some(TOKEN), None).0,
            404
        );
        // Unreachable: a closed port.
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        put("dead", format!("http://{closed}"), "t");
        let (status, _, body) = call(addr, "GET", "/n/dead/api/entries", Some(TOKEN), None);
        assert_eq!(status, 502);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .starts_with("node unreachable")
        );
        // Redirect, non-JSON, prefix.
        let (redirect, _) =
            fake_node("HTTP/1.1 302 Found\r\nLocation: http://evil/\r\nContent-Length: 0\r\n\r\n");
        put("r", format!("http://{redirect}"), "t");
        assert_eq!(
            call(addr, "GET", "/n/r/api/entries", Some(TOKEN), None).2["error"],
            "node redirected (HTTP 302)"
        );
        let (html, _) = fake_node("HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n<hi>");
        put("h", format!("http://{html}"), "t");
        assert_eq!(
            call(addr, "GET", "/n/h/api/entries", Some(TOKEN), None).2["error"],
            "node response is not JSON"
        );
        let (prefixed, seen) =
            fake_node("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n[]\r\n0\r\n\r\n");
        put("p", format!("http://{prefixed}/base/"), "tok");
        let (status, _, body) = call(addr, "GET", "/n/p/api/entries?limit=3", Some(TOKEN), None);
        assert_eq!((status, body), (200, json!([])));
        let seen = seen.join().unwrap();
        assert!(
            seen.starts_with("GET /base/api/entries?limit=3 HTTP/1.1\r\n"),
            "{seen}"
        );
        assert!(seen.contains("X-Dippy-Token: tok\r\n"));
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&hubdir).unwrap();
    }

    #[test]
    fn ui_writes_text_only_and_guards_decisions() {
        let js = STATIC[1].1;
        assert!(!js.contains("innerHTML") && !js.contains("outerHTML"));
        assert!(js.contains("X-Dippy-Token") && js.contains("if (busy"));
        // Ctrl+Enter approves, Esc denies, both only when armed (1 s).
        assert!(js.contains("event.ctrlKey") && js.contains("\"cancel\""));
        assert!(js.contains("const ARMING_MS = 1000;") && js.contains("!armed()"));
        let html = STATIC[0].1;
        for section in ["REQUEST", "WHY IT ASKED", "ADD A RULE"] {
            assert!(html.contains(section), "{section}");
        }
        assert!(html.find("view-approvals") < html.find("view-log"));
    }

    #[test]
    fn hub_forwards_approval_lists_and_answers() {
        let dir = temp("hub-approvals");
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        let mut node = Server::new("node-token", None, None);
        node.user_config = dir.join("user-config");
        let node = Arc::new(node);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let node_addr = listener.local_addr().unwrap();
        let n = Arc::clone(&node);
        std::thread::spawn(move || serve_on(listener, n));
        let id = "0123456789abcdef0123456789abcdef";
        let request = json!({
            "version": 2, "id": id, "command": "zorp x", "cwd": dir.join("proj").to_string_lossy(),
            "tool": null, "file_path": null, "message": "zorp x", "agent": "claude",
            "server": null, "decision": "ask", "reason": "zorp x", "context_flags": [],
            "flags": [], "remote": false,
            "causes": [{"kind": "command", "target": "zorp x", "rule": null, "flags": []}],
            "deadline": crate::queue::now_ms() + 60_000,
        });
        node.approvals.add(request).unwrap();
        let addr = hub(&dir);
        let body = json!({"url": format!("http://{node_addr}"), "token": "node-token"}).to_string();
        call(addr, "PUT", "/api/nodes/vm1", Some(TOKEN), Some(&body));
        let (status, _, list) = call(
            addr,
            "GET",
            "/n/vm1/api/approvals?state=pending",
            Some(TOKEN),
            None,
        );
        assert_eq!((status, list[0]["id"].as_str()), (200, Some(id)));
        let rule = json!({"cause": 0, "action": "allow", "kind": "command", "tags": "",
                          "pattern": "zorp x", "target": "project"})
        .to_string();
        let (status, _, check) = call(
            addr,
            "POST",
            &format!("/n/vm1/api/approvals/{id}/check"),
            Some(TOKEN),
            Some(&rule),
        );
        assert_eq!((status, check["can_save"].as_bool()), (200, Some(true)));
        let (status, _, saved) = call(
            addr,
            "POST",
            &format!("/n/vm1/api/approvals/{id}/rule"),
            Some(TOKEN),
            Some(&rule),
        );
        assert_eq!((status, saved["statuses"].clone()), (200, json!(["allow"])));
        let decision = r#"{"decision": "allow", "note": "via hub"}"#;
        let path = format!("/n/vm1/api/approvals/{id}/decision");
        let (status, _, item) = call(addr, "POST", &path, Some(TOKEN), Some(decision));
        assert_eq!(
            (status, item["state"].as_str(), item["note"].as_str()),
            (200, Some("allowed"), Some("via hub"))
        );
        assert_eq!(
            call(addr, "POST", &path, Some(TOKEN), Some(decision)).0,
            409
        );
        // Allowlist: no long-poll or POST of requests through the hub.
        assert_eq!(
            call(
                addr,
                "GET",
                &format!("/n/vm1/api/approvals/{id}?wait=5"),
                Some(TOKEN),
                None
            )
            .0,
            404
        );
        assert_eq!(
            call(
                addr,
                "POST",
                "/n/vm1/api/approvals",
                Some(TOKEN),
                Some("{}")
            )
            .0,
            404
        );
        assert_eq!(
            call(
                addr,
                "POST",
                "/n/vm1/api/approvals/..%2F/decision",
                Some(TOKEN),
                Some(decision)
            )
            .0,
            404
        );
        assert_eq!(
            call(
                addr,
                "POST",
                &format!("/n/nope/api/approvals/{id}/decision"),
                Some(TOKEN),
                Some(decision)
            )
            .0,
            404
        );
        assert_eq!(
            call(addr, "POST", &path, Some("bad"), Some(decision)).0,
            401
        );
        let big = "x".repeat(MAX_REQUEST_BODY + 1);
        assert_eq!(call(addr, "POST", &path, Some(TOKEN), Some(&big)).0, 400);
        // The hub has no queue of its own.
        assert_eq!(
            call(addr, "GET", "/api/approvals", Some(TOKEN), None).0,
            404
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn contract_matches_routes() {
        let text = include_str!("../../../docs/dashboard-api.json");
        let contract: Value = serde_json::from_str(text).unwrap();
        let mut documented: Vec<(String, String)> = contract["paths"]
            .as_object()
            .unwrap()
            .iter()
            .flat_map(|(path, item)| {
                item.as_object()
                    .unwrap()
                    .keys()
                    .filter(|m| matches!(m.as_str(), "get" | "put" | "post" | "delete"))
                    .map(|m| (m.to_uppercase(), path.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut routes: Vec<(String, String)> = ROUTES
            .iter()
            .map(|(m, p, _, _)| (m.to_string(), p.to_string()))
            .collect();
        documented.sort();
        routes.sort();
        assert_eq!(documented, routes);
    }
}
