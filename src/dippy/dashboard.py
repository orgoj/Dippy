"""Read-only live web view of the Dippy audit log.

The server only reads the log Dippy already writes. `/api/*` requires the
random token printed at startup, so other local users and DNS-rebinding pages
cannot read logged commands.
"""

from __future__ import annotations

import hmac
import json
import os
import secrets
import sys
from datetime import date, datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from dippy.audit import query_audit_log

DECISIONS = ("allow", "ask", "deny", "pass")
DEFAULT_LIMIT = 200
MAX_LIMIT = 1000
_PARAMETERS = frozenset({"limit", "decision", "not_allow", "agent", "cwd", "grep"})

_SECURITY_HEADERS = {
    "X-Content-Type-Options": "nosniff",
    "Cache-Control": "no-store",
    "Referrer-Policy": "no-referrer",
    "Content-Security-Policy": "default-src 'self'; frame-ancestors 'none'",
}

_PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Dippy Dashboard</title>
<link rel="stylesheet" href="/app.css">
</head>
<body>
<header>
  <h1>Dippy</h1>
  <form id="filters">
    <select name="decision">
      <option value="">all decisions</option>
      <option value="not_allow" selected>not allow</option>
      <option value="allow">allow</option>
      <option value="ask">ask</option>
      <option value="deny">deny</option>
      <option value="pass">pass</option>
    </select>
    <input name="agent" placeholder="agent (claude, agy...)">
    <input name="cwd" placeholder="cwd /abs/path">
    <input name="grep" placeholder="text">
    <input name="limit" type="number" min="1" max="1000" value="200">
    <label><input name="pause" type="checkbox"> pause</label>
  </form>
  <p id="status">loading...</p>
</header>
<main>
<table>
  <thead><tr><th>time</th><th>decision</th><th>agent</th><th>cwd</th>
  <th>what</th><th>why</th></tr></thead>
  <tbody id="rows"></tbody>
</table>
</main>
<script src="/app.js"></script>
</body>
</html>
"""

_CSS = """:root { --bg: #fff; --fg: #1d1d1f; --muted: #6e6e73; --line: #e5e5ea;
  --allow: #1a7f37; --ask: #b35900; --deny: #c62828; --pass: #6e6e73; }
@media (prefers-color-scheme: dark) {
  :root { --bg: #161618; --fg: #ececf0; --muted: #9a9aa2; --line: #2c2c31;
    --allow: #4cc26a; --ask: #f0a040; --deny: #ff6b6b; --pass: #9a9aa2; } }
body { margin: 0; background: var(--bg); color: var(--fg);
  font: 14px/1.4 system-ui, sans-serif; }
header { position: sticky; top: 0; background: var(--bg); padding: 8px 16px;
  border-bottom: 1px solid var(--line); }
h1 { font-size: 18px; margin: 0 0 6px; }
form { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
input, select { font: inherit; color: inherit; background: transparent;
  border: 1px solid var(--line); border-radius: 4px; padding: 3px 6px; }
input[name=limit] { width: 5em; }
#status { margin: 6px 0 0; color: var(--muted); }
#status.error { color: var(--deny); }
main { padding: 0 16px 16px; overflow-x: auto; }
table { border-collapse: collapse; width: 100%; }
th, td { text-align: left; vertical-align: top; padding: 4px 6px;
  border-bottom: 1px solid var(--line); }
th { color: var(--muted); font-weight: 500; }
td.what { font-family: ui-monospace, monospace; white-space: pre-wrap;
  word-break: break-all; min-width: 20em; }
td.cwd { max-width: 16em; overflow: hidden; text-overflow: ellipsis;
  white-space: nowrap; }
td.time { white-space: nowrap; color: var(--muted); }
.d-allow { color: var(--allow); } .d-ask { color: var(--ask); font-weight: 600; }
.d-deny { color: var(--deny); font-weight: 600; } .d-pass { color: var(--pass); }
"""

_JS = """"use strict";
const token = new URLSearchParams(location.hash.slice(1)).get("token") || "";
const form = document.getElementById("filters");
const rows = document.getElementById("rows");
const status = document.getElementById("status");

function query() {
  const params = new URLSearchParams();
  const data = new FormData(form);
  const decision = data.get("decision");
  if (decision === "not_allow") params.set("not_allow", "1");
  else if (decision) params.set("decision", decision);
  for (const name of ["agent", "cwd", "grep", "limit"]) {
    const value = String(data.get(name) || "").trim();
    if (value) params.set(name, value);
  }
  return params.toString();
}

function cell(row, text, className, title) {
  const td = document.createElement("td");
  td.textContent = text || "";
  if (className) td.className = className;
  if (title) td.title = title;
  row.appendChild(td);
}

function what(entry) {
  if (entry.command) return entry.command;
  if (entry.tool) return [entry.tool, entry.file_path].filter(Boolean).join(" ");
  return entry.cmd || "";
}

function render(entries) {
  const fragment = document.createDocumentFragment();
  for (const entry of entries.slice().reverse()) {
    const row = document.createElement("tr");
    const time = entry.ts ? new Date(entry.ts) : null;
    cell(row, time ? time.toLocaleString() : "", "time");
    cell(row, entry.decision, "d-" + entry.decision);
    cell(row, entry.agent);
    cell(row, entry.cwd, "cwd", entry.cwd);
    cell(row, what(entry), "what");
    cell(row, [entry.rule, entry.message].filter(Boolean).join(" | "));
    fragment.appendChild(row);
  }
  rows.replaceChildren(fragment);
}

async function refresh() {
  if (form.elements.pause.checked) return;
  try {
    const response = await fetch("/api/entries?" + query(),
      {headers: {"X-Dippy-Token": token}, cache: "no-store"});
    const body = await response.json();
    if (!response.ok) throw new Error(body.error || response.statusText);
    render(body);
    status.className = "";
    status.textContent = body.length + " entries, updated "
      + new Date().toLocaleTimeString();
  } catch (error) {
    status.className = "error";
    status.textContent = String(error.message || error);
  }
}

if (!token) {
  status.className = "error";
  status.textContent = "Missing token: open the URL printed by dippy dashboard.";
} else {
  form.addEventListener("input", refresh);
  form.addEventListener("submit", (event) => { event.preventDefault(); refresh(); });
  refresh();
  setInterval(refresh, 2000);
}
"""

_STATIC = {
    "/": ("text/html; charset=utf-8", _PAGE),
    "/app.js": ("text/javascript; charset=utf-8", _JS),
    "/app.css": ("text/css; charset=utf-8", _CSS),
}


def _today() -> date:
    return datetime.now(timezone.utc).date()


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


class DashboardServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address: tuple[str, int], log: Path, token: str) -> None:
        self.log = log
        self.token = token
        super().__init__(address, _Handler)


class _Handler(BaseHTTPRequestHandler):
    server: DashboardServer

    def log_message(self, format: str, *args: object) -> None:
        pass

    def _send(self, status: int, content_type: str, body: str) -> None:
        data = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        for name, value in _SECURITY_HEADERS.items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(data)

    def _json(self, status: int, payload: object) -> None:
        self._send(status, "application/json", json.dumps(payload))

    def do_GET(self) -> None:
        url = urlsplit(self.path)
        if url.path in _STATIC:
            self._send(200, *_STATIC[url.path])
            return
        if url.path != "/api/entries":
            self._json(404, {"error": "not found"})
            return
        token = self.headers.get("X-Dippy-Token", "")
        if not hmac.compare_digest(token.encode(), self.server.token.encode()):
            self._json(401, {"error": "missing or invalid token"})
            return
        try:
            entries = entries_query(self.server.log, parse_qs(url.query))
        except ValueError as error:
            self._json(400, {"error": str(error)})
            return
        except OSError as error:
            self._json(500, {"error": f"cannot read audit log: {error}"})
            return
        self._json(200, entries)


def serve(log: Path, host: str, port: int) -> int:
    """Serve the dashboard until interrupted. Returns the process exit code."""
    token = secrets.token_urlsafe(32)
    try:
        server = DashboardServer((host, port), log, token)
    except (OSError, OverflowError) as error:
        print(f"dashboard: cannot listen on {host}:{port}: {error}", file=sys.stderr)
        return 1
    with server:
        print(
            f"Dippy dashboard: http://{host}:{server.server_address[1]}/#token={token}"
        )
        print("Press Ctrl+C to stop.", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
    return 0
