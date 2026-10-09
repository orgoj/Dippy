"""Tests for the read-only `dippy dashboard` live audit view."""

import json
import socket
import threading
import urllib.error
import urllib.request
from argparse import Namespace
from datetime import date

import pytest

from conftest import needs_confirmation
from dippy import dashboard
from dippy.dippy import handle_subcommand

TOKEN = "secret-token"

ENTRIES = {
    "audit-2026-10-01.log": [
        {
            "decision": "ask",
            "cmd": "old",
            "cwd": "/w",
            "ts": "2026-10-01T08:00:00+00:00",
        },
    ],
    "audit-2026-10-02.log": [
        {
            "decision": "allow",
            "cmd": "ls",
            "cwd": "/w",
            "agent": "agy",
            "ts": "2026-10-02T08:00:00+00:00",
        },
    ],
    "audit.log": [
        {
            "decision": "ask",
            "cmd": "curl",
            "cwd": "/w/proj",
            "agent": "claude",
            "ts": "2026-10-03T09:00:00+00:00",
        },
        {
            "decision": "deny",
            "cmd": "rm",
            "cwd": "/other",
            "agent": "codex",
            "ts": "2026-10-03T10:00:00+00:00",
        },
    ],
}


@pytest.fixture
def log_path(tmp_path, monkeypatch):
    monkeypatch.setattr(dashboard, "_today", lambda: date(2026, 10, 3))
    for name, entries in ENTRIES.items():
        (tmp_path / name).write_text("\n".join(json.dumps(e) for e in entries) + "\n")
    return tmp_path / "audit.log"


@pytest.fixture
def server(log_path):
    srv = dashboard.DashboardServer(("127.0.0.1", 0), log_path, TOKEN)
    thread = threading.Thread(target=srv.serve_forever, daemon=True)
    thread.start()
    yield srv
    srv.shutdown()
    srv.server_close()


def _get(server, path, token=TOKEN, method="GET"):
    request = urllib.request.Request(
        f"http://127.0.0.1:{server.server_address[1]}{path}", method=method
    )
    if token is not None:
        request.add_header("X-Dippy-Token", token)
    try:
        with urllib.request.urlopen(request) as response:
            return response.status, response.headers, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.headers, error.read()


def _entries(server, query=""):
    status, _, body = _get(server, "/api/entries" + query)
    assert status == 200, body
    return json.loads(body)


@pytest.mark.parametrize("token", [None, "", "wrong"])
def test_api_requires_token(server, token):
    status, headers, body = _get(server, "/api/entries", token=token)
    assert status == 401
    assert headers["Content-Type"] == "application/json"
    assert "token" in json.loads(body)["error"]


def test_entries_since_yesterday_in_time_order(server):
    assert [e["cmd"] for e in _entries(server)] == ["ls", "curl", "rm"]


def test_since_is_computed_per_request(server, monkeypatch):
    monkeypatch.setattr(dashboard, "_today", lambda: date(2026, 10, 4))
    assert [e["cmd"] for e in _entries(server)] == ["curl", "rm"]


def test_filters_map_onto_audit_query(server):
    assert [e["cmd"] for e in _entries(server, "?decision=ask&decision=deny")] == [
        "curl",
        "rm",
    ]
    assert [e["cmd"] for e in _entries(server, "?not_allow=1")] == ["curl", "rm"]
    assert [e["cmd"] for e in _entries(server, "?agent=codex")] == ["rm"]
    assert [e["cmd"] for e in _entries(server, "?cwd=/w")] == ["ls", "curl"]
    assert [e["cmd"] for e in _entries(server, "?grep=cur")] == ["curl"]
    assert [e["cmd"] for e in _entries(server, "?limit=1")] == ["rm"]


@pytest.mark.parametrize(
    "query",
    [
        "?limit=x",
        "?limit=0",
        "?limit=1001",
        "?decision=maybe",
        "?cwd=relative/path",
        "?not_allow=yes",
        "?since=2026-10-01",
    ],
)
def test_invalid_parameters_are_rejected(server, query):
    status, headers, body = _get(server, "/api/entries" + query)
    assert status == 400
    assert headers["Content-Type"] == "application/json"
    assert json.loads(body)["error"]


def test_read_error_returns_json_500(server, monkeypatch):
    def failing(*args, **kwargs):
        raise FileNotFoundError("audit-2026-10-02.log vanished")

    monkeypatch.setattr(dashboard, "query_audit_log", failing)
    status, _, body = _get(server, "/api/entries")
    assert status == 500
    assert "vanished" in json.loads(body)["error"]


@pytest.mark.parametrize(
    "path,content_type",
    [
        ("/", "text/html; charset=utf-8"),
        ("/app.js", "text/javascript; charset=utf-8"),
        ("/app.css", "text/css; charset=utf-8"),
    ],
)
def test_static_files_without_token_and_with_security_headers(
    server, path, content_type
):
    status, headers, body = _get(server, path, token=None)
    assert status == 200
    assert headers["Content-Type"] == content_type
    assert headers["X-Content-Type-Options"] == "nosniff"
    assert headers["Cache-Control"] == "no-store"
    assert "default-src 'self'" in headers["Content-Security-Policy"]
    assert body


def test_page_inserts_log_text_without_html_parsing(server):
    _, _, body = _get(server, "/app.js", token=None)
    script = body.decode()
    assert "innerHTML" not in script
    assert "X-Dippy-Token" in script


def test_unknown_path_and_write_methods(server):
    assert _get(server, "/nope")[0] == 404
    assert _get(server, "/api/entries", method="POST")[0] == 501


def _args(config, **overrides):
    values = dict(
        subcommand="dashboard",
        cwd=str(config.parent),
        config=None,
        config_only=str(config),
        host="127.0.0.1",
        port=0,
    )
    values.update(overrides)
    return Namespace(**values)


def test_subcommand_without_log_reports_error(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    config = tmp_path / "only.dippy"
    config.write_text("")
    assert handle_subcommand(_args(config)) == 1
    assert "set log" in capsys.readouterr().err


def test_subcommand_port_in_use_exits_cleanly(log_path, tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    config = tmp_path / "only.dippy"
    config.write_text(f"set log {log_path}\n")
    with socket.socket() as busy:
        busy.bind(("127.0.0.1", 0))
        busy.listen()
        assert handle_subcommand(_args(config, port=busy.getsockname()[1])) == 1
    assert "dashboard:" in capsys.readouterr().err


def test_subcommand_invalid_port_exits_cleanly(log_path, tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    config = tmp_path / "only.dippy"
    config.write_text(f"set log {log_path}\n")
    assert handle_subcommand(_args(config, port=70000)) == 1
    assert "dashboard:" in capsys.readouterr().err


def test_subcommand_prints_tokenized_url_and_serves(
    log_path, tmp_path, monkeypatch, capsys
):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    config = tmp_path / "only.dippy"
    config.write_text(f"set log {log_path}\n")
    monkeypatch.setattr(dashboard.secrets, "token_urlsafe", lambda n: TOKEN)
    served = []
    monkeypatch.setattr(
        dashboard.DashboardServer, "serve_forever", lambda self: served.append(self)
    )
    assert handle_subcommand(_args(config)) == 0
    port = served[0].server_address[1]
    assert f"http://127.0.0.1:{port}/#token={TOKEN}" in capsys.readouterr().out
    assert served[0].log == log_path


def test_dashboard_subcommand_needs_approval(check):
    assert needs_confirmation(check("dippy dashboard"))
    assert needs_confirmation(check("dippy dashboard --host 0.0.0.0"))
