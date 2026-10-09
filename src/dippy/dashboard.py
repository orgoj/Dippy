"""Live web view of the Dippy audit log, as a node or as a hub over nodes.

A node only reads the log Dippy already writes. A hub has no log: it keeps a
registry of node URLs and tokens and proxies the allowlisted node API, so the
same page shows one machine or all of them. Every `/api/*` and `/n/*` request
requires the server's token, so other local users and DNS-rebinding pages
cannot read logged commands. The REST contract is `docs/dashboard-api.json`.
"""

from __future__ import annotations

import hmac
import json
import os
import re
import secrets
import stat
import sys
import threading
import urllib.error
import urllib.request
from datetime import date, datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from dippy.audit import query_audit_log
from dippy.config_admin import _write_atomic

DECISIONS = ("allow", "ask", "deny", "pass")
DEFAULT_LIMIT = 200
MAX_LIMIT = 1000
MAX_NODE_RESPONSE = 8 * 1024 * 1024
MAX_REQUEST_BODY = 64 * 1024
NODE_TIMEOUT = 5
DEFAULT_TOKEN_FILE = "~/.dippy/dashboard-token"
_PARAMETERS = frozenset({"limit", "decision", "not_allow", "agent", "cwd", "grep"})
_NODE_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}")

_SECURITY_HEADERS = {
    "X-Content-Type-Options": "nosniff",
    "Cache-Control": "no-store",
    "Referrer-Policy": "no-referrer",
    "Content-Security-Policy": "default-src 'self'; frame-ancestors 'none'",
}

_STATIC_DIR = Path(__file__).parent / "dashboard_static"
_STATIC = {
    "/": ("index.html", "text/html; charset=utf-8"),
    "/app.js": ("app.js", "text/javascript; charset=utf-8"),
    "/app.css": ("app.css", "text/css; charset=utf-8"),
}

# (method, path template, server mode, handler method). `{name}` is one path
# segment. The contract test compares this table with docs/dashboard-api.json.
ROUTES = (
    ("GET", "/api/entries", "node", "_get_entries"),
    ("GET", "/api/nodes", "hub", "_get_nodes"),
    ("PUT", "/api/nodes/{name}", "hub", "_put_node"),
    ("DELETE", "/api/nodes/{name}", "hub", "_delete_node"),
    ("GET", "/n/{name}/api/entries", "hub", "_proxy_entries"),
)
_ROUTE_PATTERNS = [
    (method, re.compile(re.escape(path).replace(r"\{name\}", "([^/]+)")), mode, name)
    for method, path, mode, name in ROUTES
]


class RegistryError(Exception):
    """The nodes file cannot be read or holds an invalid entry."""


# HTTP(S) only: no environment proxy (it would receive node tokens), no
# redirects (a 3xx becomes HTTPError), no file:, ftp: or data: URLs.
_NODE_OPENER = urllib.request.OpenerDirector()
for _handler in (
    urllib.request.HTTPHandler,
    urllib.request.HTTPSHandler,
    urllib.request.HTTPDefaultErrorHandler,
    urllib.request.HTTPErrorProcessor,
    urllib.request.UnknownHandler,
):
    _NODE_OPENER.add_handler(_handler())


def _today() -> date:
    return datetime.now(timezone.utc).date()


def _warn_if_shared(path: Path) -> None:
    if path.stat().st_mode & (stat.S_IRWXG | stat.S_IRWXO):
        print(
            f"dashboard: warning: {path} is readable by group or others",
            file=sys.stderr,
        )


def load_token(path: Path) -> str:
    """Return the token stored in path, creating a private one when missing."""
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        token = path.read_text().strip()
        if not token:
            raise ValueError(f"token file is empty: {path}") from None
        _warn_if_shared(path)
        return token
    token = secrets.token_urlsafe(32)
    with os.fdopen(fd, "w") as stream:
        stream.write(token + "\n")
    return token


def _single(params: dict[str, list[str]], name: str) -> str | None:
    values = params.get(name)
    return values[-1] if values else None


def entries_query(log: Path, params: dict[str, list[str]]) -> list[dict]:
    """Return audit entries since yesterday (UTC) matching the URL parameters.

    Raises ValueError for invalid parameters.
    """
    unknown = sorted(set(params) - _PARAMETERS)
    if unknown:
        raise ValueError(f"unknown parameter: {', '.join(unknown)}")
    raw_limit = _single(params, "limit")
    try:
        limit = int(raw_limit) if raw_limit is not None else DEFAULT_LIMIT
    except ValueError:
        raise ValueError(f"limit must be an integer: {raw_limit}") from None
    if not 1 <= limit <= MAX_LIMIT:
        raise ValueError(f"limit must be between 1 and {MAX_LIMIT}: {limit}")
    decisions = params.get("decision")
    for decision in decisions or []:
        if decision not in DECISIONS:
            raise ValueError(f"decision must be one of {', '.join(DECISIONS)}")
    not_allow = _single(params, "not_allow")
    if not_allow not in (None, "1"):
        raise ValueError("not_allow must be 1")
    cwd = _single(params, "cwd")
    if cwd is not None and not os.path.isabs(cwd):
        raise ValueError(f"cwd must be an absolute path: {cwd}")
    lines = query_audit_log(
        log,
        since=(_today() - timedelta(days=1)).isoformat(),
        decisions=decisions,
        not_allow=not_allow == "1",
        agent=_single(params, "agent"),
        cwd=cwd,
        grep=_single(params, "grep"),
        limit=limit,
    )
    return [json.loads(line) for line in lines]


def _read_nodes(path: Path) -> dict[str, dict[str, str]]:
    """Return validated registry entries; RegistryError on any invalid content."""
    try:
        if not path.exists():
            return {}
        data = json.loads(path.read_text())
        if not isinstance(data, dict) or not isinstance(data.get("nodes", {}), dict):
            raise ValueError('expected {"nodes": {NAME: {"url", "token"}}}')
        return {
            name: _validate_node(name, node)
            for name, node in data.get("nodes", {}).items()
        }
    except (OSError, ValueError) as error:
        raise RegistryError(f"{path}: {error}") from None


def _write_nodes(path: Path, nodes: dict[str, dict[str, str]]) -> None:
    _write_atomic(path, json.dumps({"nodes": nodes}, indent=2), mode=0o600)


def _validate_node(name: str, body: object) -> dict[str, str]:
    if not _NODE_NAME.fullmatch(name):
        raise ValueError(
            "name must be 1-64 letters, digits, '_', '.' or '-', "
            "starting with a letter or digit"
        )
    if not isinstance(body, dict):
        raise ValueError("body must be a JSON object with url and token")
    url, token = body.get("url"), body.get("token")
    if not isinstance(url, str) or not isinstance(token, str) or not token:
        raise ValueError("url and token must be non-empty strings")
    parts = urlsplit(url)
    if parts.scheme not in ("http", "https") or not parts.hostname:
        raise ValueError("url must be http:// or https:// with a host")
    if parts.username or parts.password or parts.query or parts.fragment:
        raise ValueError("url must not contain credentials, query or fragment")
    return {"url": url, "token": token}


def _fetch_node(node: dict[str, str], path: str, query: str) -> tuple[int, bytes]:
    """Return status and JSON body of a node API call; ValueError on failure."""
    url = node["url"].rstrip("/") + path + ("?" + query if query else "")
    request = urllib.request.Request(url, headers={"X-Dippy-Token": node["token"]})
    try:
        response = _NODE_OPENER.open(request, timeout=NODE_TIMEOUT)
    except urllib.error.HTTPError as error:
        if 300 <= error.code < 400:
            error.close()
            raise ValueError(f"node redirected (HTTP {error.code})") from None
        response = error
    except (OSError, ValueError) as error:
        raise ValueError(f"node unreachable: {error}") from None
    with response:
        try:
            body = response.read(MAX_NODE_RESPONSE + 1)
        except OSError as error:
            raise ValueError(f"node read failed: {error}") from None
        code = response.status if hasattr(response, "status") else response.code
    if len(body) > MAX_NODE_RESPONSE:
        raise ValueError("node response too large")
    try:
        json.loads(body)
    except ValueError:
        raise ValueError("node response is not JSON") from None
    return code, body


class DashboardServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(
        self,
        address: tuple[str, int],
        token: str,
        *,
        log: Path | None = None,
        nodes: Path | None = None,
    ) -> None:
        self.token = token
        self.log = log
        self.nodes = nodes
        self.mode = "hub" if nodes is not None else "node"
        self.nodes_lock = threading.Lock()
        super().__init__(address, _Handler)


class _Handler(BaseHTTPRequestHandler):
    server: DashboardServer

    def log_message(self, format: str, *args: object) -> None:
        pass

    def _send(self, status: int, content_type: str, data: bytes) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        for name, value in _SECURITY_HEADERS.items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(data)

    def _json(self, status: int, payload: object) -> None:
        self._send(status, "application/json", json.dumps(payload).encode())

    def _authorized(self) -> bool:
        token = self.headers.get("X-Dippy-Token", "")
        return hmac.compare_digest(token.encode(), self.server.token.encode())

    def _dispatch(self, method: str) -> None:
        url = urlsplit(self.path)
        if method == "GET" and url.path in _STATIC:
            filename, content_type = _STATIC[url.path]
            self._send(200, content_type, (_STATIC_DIR / filename).read_bytes())
            return
        for route_method, pattern, mode, handler in _ROUTE_PATTERNS:
            match = pattern.fullmatch(url.path)
            if match and route_method == method and mode == self.server.mode:
                break
        else:
            self._json(404, {"error": "not found"})
            return
        if not self._authorized():
            self._json(401, {"error": "missing or invalid token"})
            return
        try:
            getattr(self, handler)(url.query, *match.groups())
        except ValueError as error:
            self._json(400, {"error": str(error)})
        except (OSError, RegistryError) as error:
            self._json(500, {"error": str(error)})

    def do_GET(self) -> None:
        self._dispatch("GET")

    def do_PUT(self) -> None:
        self._dispatch("PUT")

    def do_DELETE(self) -> None:
        self._dispatch("DELETE")

    def _get_entries(self, query: str) -> None:
        try:
            entries = entries_query(self.server.log, parse_qs(query))
        except OSError as error:
            self._json(500, {"error": f"cannot read audit log: {error}"})
            return
        self._json(200, entries)

    def _get_nodes(self, query: str) -> None:
        nodes = _read_nodes(self.server.nodes)
        self._json(200, [{"name": n, "url": v["url"]} for n, v in nodes.items()])

    def _put_node(self, query: str, name: str) -> None:
        length = int(self.headers.get("Content-Length") or 0)
        if not 0 <= length <= MAX_REQUEST_BODY:
            raise ValueError(f"Content-Length must be 0 to {MAX_REQUEST_BODY}")
        try:
            body = json.loads(self.rfile.read(length) or b"null")
        except ValueError:
            raise ValueError("body must be JSON") from None
        node = _validate_node(name, body)
        with self.server.nodes_lock:
            nodes = _read_nodes(self.server.nodes)
            nodes[name] = node
            _write_nodes(self.server.nodes, nodes)
        self._json(200, {"name": name, "url": node["url"]})

    def _delete_node(self, query: str, name: str) -> None:
        with self.server.nodes_lock:
            nodes = _read_nodes(self.server.nodes)
            if nodes.pop(name, None) is None:
                self._json(404, {"error": f"unknown node: {name}"})
                return
            _write_nodes(self.server.nodes, nodes)
        self._json(200, {"name": name})

    def _proxy_entries(self, query: str, name: str) -> None:
        node = _read_nodes(self.server.nodes).get(name)
        if node is None:
            self._json(404, {"error": f"unknown node: {name}"})
            return
        try:
            code, body = _fetch_node(node, "/api/entries", query)
        except ValueError as error:
            self._json(502, {"error": str(error)})
            return
        self._send(code, "application/json", body)


def serve(
    host: str,
    port: int,
    token_file: Path,
    *,
    log: Path | None = None,
    nodes: Path | None = None,
) -> int:
    """Serve the dashboard until interrupted. Returns the process exit code."""
    try:
        token = load_token(token_file)
        if nodes is not None and nodes.exists():
            _read_nodes(nodes)
            _warn_if_shared(nodes)
    except (OSError, ValueError, RegistryError) as error:
        print(f"dashboard: {error}", file=sys.stderr)
        return 1
    try:
        server = DashboardServer((host, port), token, log=log, nodes=nodes)
    except (OSError, OverflowError) as error:
        print(f"dashboard: cannot listen on {host}:{port}: {error}", file=sys.stderr)
        return 1
    with server:
        kind = f"hub ({nodes})" if nodes is not None else "node"
        print(
            f"Dippy dashboard {kind}: "
            f"http://{host}:{server.server_address[1]}/#token={token}"
        )
        print("Press Ctrl+C to stop.", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
    return 0
