//! Minimal HTTP/1.1 for the dashboard: request parsing and responses for the
//! std `TcpListener` server, a client for node proxying and the approval
//! queue (plain TCP or rustls), and Python's `urlsplit`/`parse_qs`.
//!
//! Every connection carries one request (`Connection: close`), as Python's
//! `BaseHTTPRequestHandler` does with HTTP/1.0.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Request line plus headers may not exceed this.
const MAX_HEAD: usize = 64 * 1024;

// ------------------------------------------------------------- URL parsing

/// Python `urllib.parse.urlsplit` (3.12), without its NFKC netloc check.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplitUrl {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

fn is_scheme(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

pub fn urlsplit(url: &str) -> Result<SplitUrl, String> {
    let url: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut out = SplitUrl::default();
    let mut rest = url.as_str();
    if let Some(i) = rest.find(':')
        && is_scheme(&rest[..i])
    {
        out.scheme = rest[..i].to_ascii_lowercase();
        rest = &rest[i + 1..];
    }
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        out.netloc = after[..end].to_string();
        rest = &after[end..];
        let (open, close) = (out.netloc.contains('['), out.netloc.contains(']'));
        if open != close {
            return Err("Invalid IPv6 URL".into());
        }
    }
    if let Some(i) = rest.find('#') {
        out.fragment = rest[i + 1..].to_string();
        rest = &rest[..i];
    }
    if let Some(i) = rest.find('?') {
        out.query = rest[i + 1..].to_string();
        rest = &rest[..i];
    }
    out.path = rest.to_string();
    Ok(out)
}

impl SplitUrl {
    fn userinfo(&self) -> Option<&str> {
        self.netloc.rsplit_once('@').map(|(u, _)| u)
    }
    fn hostport(&self) -> &str {
        self.netloc
            .rsplit_once('@')
            .map_or(&self.netloc, |(_, h)| h)
    }
    /// `username` (may be empty).
    pub fn username(&self) -> Option<&str> {
        self.userinfo()
            .map(|u| u.split_once(':').map_or(u, |(n, _)| n))
    }
    /// `password` (None without a `:`).
    pub fn password(&self) -> Option<&str> {
        self.userinfo()
            .and_then(|u| u.split_once(':').map(|(_, p)| p))
    }
    /// `hostname`: lowercased, brackets removed, None when empty.
    pub fn hostname(&self) -> Option<String> {
        let hp = self.hostport();
        let host = if let Some(inner) = hp.strip_prefix('[') {
            inner.split(']').next().unwrap_or("")
        } else {
            hp.split(':').next().unwrap_or("")
        };
        (!host.is_empty()).then(|| host.to_ascii_lowercase())
    }
    /// The port text after the host, if any.
    fn port_text(&self) -> Option<&str> {
        let hp = self.hostport();
        let after = if hp.starts_with('[') {
            hp.split_once(']').map_or("", |(_, a)| a)
        } else {
            hp.split_once(':').map_or("", |(_, p)| p)
        };
        let after = after.strip_prefix(':').unwrap_or(after);
        (!after.is_empty()).then_some(after)
    }
}

/// Percent-decoding with `+` as space and invalid UTF-8 replaced
/// (`unquote_plus` with `errors="replace"`).
fn unquote_plus(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(0));
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Python `parse_qs(query)`: `&`-separated, blank values and fields without
/// `=` dropped, values in order per name (names in first-seen order).
pub fn parse_qs(query: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for field in query.split('&') {
        let Some((name, value)) = field.split_once('=') else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let (name, value) = (unquote_plus(name), unquote_plus(value));
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, values)) => values.push(value),
            None => out.push((name, vec![value])),
        }
    }
    out
}

// ------------------------------------------------------------------ server

/// One request; the body is read on demand.
pub struct Request {
    pub method: String,
    /// The request target as sent (`/api/entries?limit=5`).
    pub target: String,
    pub path: String,
    pub query: String,
    headers: Vec<(String, String)>,
    reader: BufReader<TcpStream>,
}

impl Request {
    /// The first header named `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The `Content-Length` body, at most `max` bytes.
    pub fn body(&mut self, max: usize) -> Result<Vec<u8>, String> {
        let raw = self
            .header("Content-Length")
            .unwrap_or("")
            .trim()
            .to_string();
        let length: i64 = if raw.is_empty() {
            0
        } else {
            raw.parse()
                .map_err(|_| format!("invalid literal for int() with base 10: '{raw}'"))?
        };
        if length < 0 || length as u64 > max as u64 {
            return Err(format!("Content-Length must be 0 to {max}"));
        }
        let mut body = vec![0; length as usize];
        self.reader
            .read_exact(&mut body)
            .map_err(|e| format!("cannot read body: {e}"))?;
        Ok(body)
    }

    pub fn stream(&self) -> &TcpStream {
        self.reader.get_ref()
    }
}

fn read_line_limited(reader: &mut impl BufRead, budget: &mut usize) -> io::Result<String> {
    let mut line = Vec::new();
    let n = reader
        .by_ref()
        .take(*budget as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if n > *budget {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "header too long",
        ));
    }
    *budget -= n;
    while line.last().is_some_and(|b| matches!(b, b'\n' | b'\r')) {
        line.pop();
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

/// Read the request line and headers; `None` for a closed or malformed
/// connection.
pub fn read_request(stream: TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut budget = MAX_HEAD;
    let line = read_line_limited(&mut reader, &mut budget).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let line = read_line_limited(&mut reader, &mut budget).ok()?;
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        // Python keeps trailing whitespace of header values.
        headers.push((name.trim().to_string(), value.trim_start().to_string()));
    }
    // Python 3.12 collapses leading slashes (gh-87389).
    let target = if target.starts_with("//") {
        format!("/{}", target.trim_start_matches('/'))
    } else {
        target
    };
    let split = urlsplit(&target).unwrap_or_default();
    Some(Request {
        method,
        path: split.path,
        query: split.query,
        target,
        headers,
        reader,
    })
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        _ => "",
    }
}

/// Write one complete response and close the connection.
pub fn respond(
    mut stream: &TcpStream,
    status: u16,
    content_type: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n",
        reason(status),
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    let mut data = head.into_bytes();
    data.extend_from_slice(body);
    stream.write_all(&data)?;
    stream.flush()
}

// ------------------------------------------------------------------ client

/// Where a client request goes: `http(s)://host[:port][/prefix]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    /// Path prefix without a trailing slash.
    pub prefix: String,
}

impl Target {
    pub fn parse(url: &str) -> Result<Target, String> {
        let split = urlsplit(url)?;
        let tls = match split.scheme.as_str() {
            "http" => false,
            "https" => true,
            _ => return Err(format!("unsupported URL scheme: {url}")),
        };
        let host = split
            .hostname()
            .ok_or_else(|| format!("URL has no host: {url}"))?;
        let port = match split.port_text() {
            None => {
                if tls {
                    443
                } else {
                    80
                }
            }
            Some(p) => p
                .parse::<u16>()
                .map_err(|_| format!("invalid port in URL: {url}"))?,
        };
        Ok(Target {
            tls,
            host,
            port,
            prefix: split.path.trim_end_matches('/').to_string(),
        })
    }

    fn host_header(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let default = if self.tls { 443 } else { 80 };
        if self.port == default {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }
}

/// Client timeouts: connect, then every read and write.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub connect: Duration,
    pub io: Duration,
}

/// How a client request failed. Only `Refused` and `ConnectTimeout` prove
/// the server never received the request.
#[derive(Debug, PartialEq)]
pub enum ClientError {
    Refused(String),
    ConnectTimeout(String),
    /// Name resolution, TLS or other failures before the request was sent.
    Connect(String),
    /// The request may have reached the server (write, read or parse error).
    Indeterminate(String),
    TooLarge,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Refused(m)
            | ClientError::ConnectTimeout(m)
            | ClientError::Connect(m)
            | ClientError::Indeterminate(m) => f.write_str(m),
            ClientError::TooLarge => f.write_str("response too large"),
        }
    }
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            for cert in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(cert);
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map(|b| Arc::new(b.with_root_certificates(roots).with_no_client_auth()))
                .map_err(|e| e.to_string())
        })
        .clone()
}

fn connect(target: &Target, timeouts: Timeouts) -> Result<Box<dyn Stream>, ClientError> {
    let addrs: Vec<SocketAddr> = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|e| ClientError::Connect(format!("cannot resolve {}: {e}", target.host)))?
        .collect();
    let mut last = ClientError::Connect(format!("no address for {}", target.host));
    let mut tcp = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeouts.connect) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                last = ClientError::Refused(e.to_string());
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                last = ClientError::ConnectTimeout(e.to_string());
            }
            Err(e) => last = ClientError::Connect(e.to_string()),
        }
    }
    let tcp = tcp.ok_or(last)?;
    let set = |e: io::Error| ClientError::Connect(e.to_string());
    tcp.set_read_timeout(Some(timeouts.io)).map_err(set)?;
    tcp.set_write_timeout(Some(timeouts.io)).map_err(set)?;
    let _ = tcp.set_nodelay(true);
    if !target.tls {
        return Ok(Box::new(tcp));
    }
    let config = tls_config().map_err(ClientError::Connect)?;
    let name = rustls::pki_types::ServerName::try_from(target.host.clone())
        .map_err(|e| ClientError::Connect(format!("invalid TLS server name: {e}")))?;
    let mut conn = rustls::ClientConnection::new(config, name)
        .map_err(|e| ClientError::Connect(e.to_string()))?;
    let mut tcp = tcp;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp)
            .map_err(|e| ClientError::Connect(format!("TLS handshake failed: {e}")))?;
    }
    Ok(Box::new(rustls::StreamOwned::new(conn, tcp)))
}

fn read_chunked(reader: &mut impl BufRead, max: usize) -> Result<Vec<u8>, ClientError> {
    let bad = |m: &str| ClientError::Indeterminate(m.to_string());
    let mut body = Vec::new();
    loop {
        let mut budget = 1024;
        let line = read_line_limited(reader, &mut budget).map_err(|e| bad(&e.to_string()))?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| bad("malformed chunked response"))?;
        if size == 0 {
            return Ok(body);
        }
        if body.len() + size > max {
            return Err(ClientError::TooLarge);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader
            .read_exact(&mut body[start..])
            .map_err(|e| bad(&e.to_string()))?;
        let mut crlf = String::new();
        reader
            .read_line(&mut crlf)
            .map_err(|e| bad(&e.to_string()))?;
    }
}

/// Send one request and read the whole response (at most `max` body bytes).
/// Redirects are returned as they are, never followed; no proxy is used.
pub fn request(
    target: &Target,
    method: &str,
    path_and_query: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    timeouts: Timeouts,
    max: usize,
) -> Result<Response, ClientError> {
    let mut stream = connect(target, timeouts)?;
    let mut head = format!(
        "{method} {}{path_and_query} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\
         Accept-Encoding: identity\r\nUser-Agent: dippy/{}\r\n",
        target.prefix,
        target.host_header(),
        env!("CARGO_PKG_VERSION")
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    let mut data = head.into_bytes();
    data.extend_from_slice(body.unwrap_or_default());
    let lost = |e: io::Error| ClientError::Indeterminate(e.to_string());
    stream.write_all(&data).map_err(lost)?;
    stream.flush().map_err(lost)?;
    let mut reader = BufReader::new(stream);
    let mut budget = MAX_HEAD;
    let status_line = read_line_limited(&mut reader, &mut budget).map_err(lost)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .filter(|_| status_line.starts_with("HTTP/"))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| {
            ClientError::Indeterminate(format!("malformed status line: {status_line:?}"))
        })?;
    let mut length = None;
    let mut chunked = false;
    loop {
        let line = read_line_limited(&mut reader, &mut budget).map_err(lost)?;
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(ClientError::Indeterminate("malformed header".into()));
        };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| ClientError::Indeterminate("malformed Content-Length".into()))?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }
    let body = if chunked {
        read_chunked(&mut reader, max)?
    } else if let Some(length) = length {
        if length > max {
            return Err(ClientError::TooLarge);
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).map_err(lost)?;
        body
    } else {
        let mut body = Vec::new();
        reader
            .take(max as u64 + 1)
            .read_to_end(&mut body)
            .map_err(lost)?;
        if body.len() > max {
            return Err(ClientError::TooLarge);
        }
        body
    };
    Ok(Response { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlsplit_like_python() {
        let u = urlsplit("HTTP://user:pw@Host.example:8765/p/x?q=1#f").unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.netloc, "user:pw@Host.example:8765");
        assert_eq!(
            (u.path.as_str(), u.query.as_str(), u.fragment.as_str()),
            ("/p/x", "q=1", "f")
        );
        assert_eq!(u.username(), Some("user"));
        assert_eq!(u.password(), Some("pw"));
        assert_eq!(u.hostname().as_deref(), Some("host.example"));
        let v6 = urlsplit("http://[::1]:80/").unwrap();
        assert_eq!(v6.hostname().as_deref(), Some("::1"));
        assert_eq!(urlsplit("http://[::1/").unwrap_err(), "Invalid IPv6 URL");
        assert_eq!(urlsplit("//api/x").unwrap().path, "/x");
        assert_eq!(urlsplit("/api/x?a=1").unwrap().path, "/api/x");
        assert_eq!(urlsplit("ftp:x").unwrap().scheme, "ftp");
        assert_eq!(urlsplit("1http://x").unwrap().scheme, "");
        assert!(urlsplit("http://@h").unwrap().username() == Some(""));
    }

    #[test]
    fn parse_qs_drops_blank_values() {
        assert_eq!(
            parse_qs("decision=ask&decision=deny&agent=&grep=a+b%2Fc&x&limit=5"),
            vec![
                (
                    "decision".to_string(),
                    vec!["ask".to_string(), "deny".to_string()]
                ),
                ("grep".to_string(), vec!["a b/c".to_string()]),
                ("limit".to_string(), vec!["5".to_string()]),
            ]
        );
        assert_eq!(
            parse_qs("g=%zz%4"),
            vec![("g".to_string(), vec!["%zz%4".to_string()])]
        );
        assert_eq!(parse_qs("g=%C3%A9%FF")[0].1[0], "é\u{fffd}");
    }

    #[test]
    fn https_to_a_plain_server_fails_before_sending() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\n\r\n");
        });
        let target = Target::parse(&format!("https://127.0.0.1:{port}")).unwrap();
        let t = Timeouts {
            connect: Duration::from_secs(2),
            io: Duration::from_secs(2),
        };
        let err = request(&target, "GET", "/", &[], None, t, 100).unwrap_err();
        assert!(
            matches!(&err, ClientError::Connect(m) if m.contains("TLS")),
            "{err:?}"
        );
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let target = Target::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        let err = request(&target, "GET", "/", &[], None, t, 100).unwrap_err();
        assert!(matches!(err, ClientError::Refused(_)), "{err:?}");
    }

    #[test]
    fn targets() {
        let t = Target::parse("http://127.0.0.1:8765/").unwrap();
        assert_eq!(
            (t.tls, t.host.as_str(), t.port, t.prefix.as_str()),
            (false, "127.0.0.1", 8765, "")
        );
        let t = Target::parse("https://Node.example/base/").unwrap();
        assert_eq!((t.tls, t.port, t.prefix.as_str()), (true, 443, "/base"));
        assert_eq!(t.host_header(), "node.example");
        assert!(Target::parse("ftp://x").is_err());
        assert!(Target::parse("http://x:99999").is_err());
    }
}
