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


def _start(**kwargs):
    srv = dashboard.DashboardServer(("127.0.0.1", 0), TOKEN, **kwargs)
    thread = threading.Thread(target=srv.serve_forever, daemon=True)
    thread.start()
    return srv


def _stop(srv):
    srv.shutdown()
    srv.server_close()


@pytest.fixture
def server(log_path):
    srv = _start(log=log_path)
    yield srv
    _stop(srv)


def _url(server):
    return f"http://127.0.0.1:{server.server_address[1]}"


def _get(server, path, token=TOKEN, method="GET", body=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(_url(server) + path, data=data, method=method)
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
        hub=None,
        token_file=None,
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
    assert needs_confirmation(check("dippy dashboard --hub nodes.json"))


# === token file ===


def test_token_file_is_created_private_and_reused(tmp_path):
    path = tmp_path / "sub" / "token"
    token = dashboard.load_token(path)
    assert len(token) >= 32
    assert path.stat().st_mode & 0o777 == 0o600
    assert dashboard.load_token(path) == token


def test_empty_token_file_is_rejected(tmp_path):
    path = tmp_path / "token"
    path.write_text("\n")
    with pytest.raises(ValueError, match="empty"):
        dashboard.load_token(path)


def test_readable_token_file_warns(tmp_path, capsys):
    path = tmp_path / "token"
    path.write_text("abc\n")
    path.chmod(0o644)
    assert dashboard.load_token(path) == "abc"
    assert "readable" in capsys.readouterr().err


def test_subcommand_uses_token_file(log_path, tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    config = tmp_path / "only.dippy"
    config.write_text(f"set log {log_path}\n")
    token_file = tmp_path / "token"
    token_file.write_text("fixed-token\n")
    token_file.chmod(0o600)
    monkeypatch.setattr(dashboard.DashboardServer, "serve_forever", lambda self: None)
    assert handle_subcommand(_args(config, token_file=str(token_file))) == 0
    assert "#token=fixed-token" in capsys.readouterr().out


# === hub ===


@pytest.fixture
def nodes_file(tmp_path):
    return tmp_path / "nodes.json"


@pytest.fixture
def hub(nodes_file):
    srv = _start(nodes=nodes_file)
    yield srv
    _stop(srv)


def _register(hub, name, url, token=TOKEN):
    return _get(
        hub, f"/api/nodes/{name}", method="PUT", body={"url": url, "token": token}
    )


def test_node_mode_has_no_registry_and_hub_has_no_log(server, hub):
    assert _get(server, "/api/nodes")[0] == 404
    assert _get(hub, "/api/entries")[0] == 404


def test_registry_add_list_delete_without_exposing_tokens(hub, nodes_file):
    status, _, body = _register(hub, "vm1", "http://10.0.0.5:8765")
    assert status == 200
    assert "token" not in json.loads(body)
    _register(hub, "vm2", "https://vm2.example:9000/dippy/")
    status, _, body = _get(hub, "/api/nodes")
    assert status == 200
    assert json.loads(body) == [
        {"name": "vm1", "url": "http://10.0.0.5:8765"},
        {"name": "vm2", "url": "https://vm2.example:9000/dippy/"},
    ]
    assert nodes_file.stat().st_mode & 0o777 == 0o600
    assert _get(hub, "/api/nodes/vm1", method="DELETE")[0] == 200
    assert [n["name"] for n in json.loads(_get(hub, "/api/nodes")[2])] == ["vm2"]
    assert _get(hub, "/api/nodes/vm1", method="DELETE")[0] == 404


def test_registry_picks_up_hand_edits(hub, nodes_file):
    nodes_file.write_text(
        json.dumps({"nodes": {"manual": {"url": "http://h:1", "token": "t"}}})
    )
    assert json.loads(_get(hub, "/api/nodes")[2]) == [
        {"name": "manual", "url": "http://h:1"}
    ]


def test_registry_write_is_private_even_after_hand_creation(hub, nodes_file):
    nodes_file.write_text(json.dumps({"nodes": {}}))
    nodes_file.chmod(0o644)
    _register(hub, "vm1", "http://h:1")
    assert nodes_file.stat().st_mode & 0o777 == 0o600
    _get(hub, "/api/nodes/vm1", method="DELETE")
    assert nodes_file.stat().st_mode & 0o777 == 0o600


@pytest.mark.parametrize(
    "content",
    [
        "not json",
        "[]",
        json.dumps({"nodes": []}),
        json.dumps({"nodes": {"x": "http://h:1"}}),
        json.dumps({"nodes": {"x": {"token": "t"}}}),
        json.dumps({"nodes": {"x": {"url": "file:///etc/passwd", "token": "t"}}}),
        json.dumps({"nodes": {"..": {"url": "http://h:1", "token": "t"}}}),
    ],
)
def test_invalid_hand_edited_registry_is_json_500(hub, nodes_file, content):
    nodes_file.write_text(content)
    for path in ("/api/nodes", "/n/x/api/entries"):
        status, headers, body = _get(hub, path)
        assert status == 500
        assert headers["Content-Type"] == "application/json"
        assert str(nodes_file) in json.loads(body)["error"]


def test_node_opener_handles_only_http():
    with pytest.raises(ValueError, match="unreachable"):
        dashboard._fetch_node({"url": "file:///etc/passwd", "token": "t"}, "", "")


def test_negative_content_length_is_rejected(hub):
    with socket.create_connection(hub.server_address, timeout=5) as conn:
        conn.sendall(
            b"PUT /api/nodes/vm1 HTTP/1.1\r\nHost: x\r\n"
            b"X-Dippy-Token: " + TOKEN.encode() + b"\r\n"
            b"Content-Length: -1\r\nConnection: close\r\n\r\n"
        )
        assert conn.recv(100).startswith(b"HTTP/1.0 400")


def test_ui_skips_poll_while_previous_runs(server):
    script = _get(server, "/app.js", token=None)[2].decode()
    assert "if (busy" in script


def test_registry_requires_hub_token(hub):
    assert _get(hub, "/api/nodes", token="wrong")[0] == 401
    assert _register(hub, "vm1", "http://h:1", token=TOKEN)[0] == 200
    request_status = _get(hub, "/api/nodes/vm1", token="wrong", method="DELETE")[0]
    assert request_status == 401


@pytest.mark.parametrize("name", ["..", ".hidden", "-x", "a/b", "x" * 65, "a b"])
def test_registry_rejects_bad_names(hub, name):
    status = _register(hub, urllib.request.quote(name, safe=""), "http://h:1")[0]
    assert status in (400, 404)


@pytest.mark.parametrize(
    "body",
    [
        {"url": "ftp://h/", "token": "t"},
        {"url": "http:///nohost", "token": "t"},
        {"url": "http://user:pw@h/", "token": "t"},
        {"url": "http://h/?q=1", "token": "t"},
        {"url": "http://h/#f", "token": "t"},
        {"url": "http://h/", "token": ""},
        {"url": "http://h/"},
        {"url": 5, "token": "t"},
    ],
)
def test_registry_rejects_bad_bodies(hub, body):
    assert _get(hub, "/api/nodes/vm1", method="PUT", body=body)[0] == 400


def test_registry_concurrent_writes_keep_every_node(hub):
    threads = [
        threading.Thread(target=_register, args=(hub, f"n{i}", f"http://h:{i + 1}"))
        for i in range(20)
    ]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert len(json.loads(_get(hub, "/api/nodes")[2])) == 20


def test_proxy_forwards_query_and_node_token(server, hub):
    _register(hub, "local", _url(server) + "/")
    status, _, body = _get(hub, "/n/local/api/entries?agent=codex")
    assert status == 200
    assert [e["cmd"] for e in json.loads(body)] == ["rm"]


def test_proxy_relays_node_errors(server, hub):
    _register(hub, "local", _url(server), token="wrong")
    assert _get(hub, "/n/local/api/entries")[0] == 401
    _register(hub, "local", _url(server))
    assert _get(hub, "/n/local/api/entries?limit=x")[0] == 400


def test_proxy_requires_hub_token(server, hub):
    _register(hub, "local", _url(server))
    assert _get(hub, "/n/local/api/entries", token=None)[0] == 401


def test_proxy_ignores_environment_proxy(server, hub, monkeypatch):
    with socket.socket() as dead:
        dead.bind(("127.0.0.1", 0))
        port = dead.getsockname()[1]
    for name in ("http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY"):
        monkeypatch.setenv(name, f"http://127.0.0.1:{port}")
    monkeypatch.delenv("no_proxy", raising=False)
    monkeypatch.delenv("NO_PROXY", raising=False)
    _register(hub, "local", _url(server))
    assert _get(hub, "/n/local/api/entries")[0] == 200


def test_proxy_unknown_node_and_unlisted_path(hub):
    assert _get(hub, "/n/missing/api/entries")[0] == 404
    _register(hub, "vm1", "http://127.0.0.1:1")
    assert _get(hub, "/n/vm1/api/nodes")[0] == 404
    assert _get(hub, "/n/vm1/")[0] == 404


def test_proxy_unreachable_node_is_502(hub):
    with socket.socket() as dead:
        dead.bind(("127.0.0.1", 0))
        port = dead.getsockname()[1]
    _register(hub, "down", f"http://127.0.0.1:{port}")
    status, _, body = _get(hub, "/n/down/api/entries")
    assert status == 502
    assert json.loads(body)["error"]


class _FakeNode(threading.Thread):
    """Answers every request with a fixed status, headers and body."""

    def __init__(self, status, body, headers=()):
        super().__init__(daemon=True)
        handler_status, handler_body, handler_headers = status, body, headers

        class Handler(dashboard.BaseHTTPRequestHandler):
            hits = []

            def log_message(self, *args):
                pass

            def do_GET(self):
                Handler.hits.append(self.path)
                self.send_response(handler_status)
                for name, value in handler_headers:
                    self.send_header(name, value)
                self.send_header("Content-Length", str(len(handler_body)))
                self.end_headers()
                self.wfile.write(handler_body)

        self.handler = Handler
        self.server = dashboard.ThreadingHTTPServer(("127.0.0.1", 0), Handler)

    def run(self):
        self.server.serve_forever()

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    def stop(self):
        self.server.shutdown()
        self.server.server_close()


@pytest.fixture
def fake_node():
    nodes = []

    def make(status, body, headers=()):
        node = _FakeNode(status, body, headers)
        node.start()
        nodes.append(node)
        return node

    yield make
    for node in nodes:
        node.stop()


def test_proxy_does_not_follow_redirects(hub, fake_node):
    target = fake_node(200, b"[]")
    redirect = fake_node(302, b"", headers=[("Location", target.url + "/x")])
    _register(hub, "r", redirect.url)
    assert _get(hub, "/n/r/api/entries")[0] == 502
    assert target.handler.hits == []


def test_proxy_requires_json_body(hub, fake_node):
    node = fake_node(200, b"<html>internal page</html>")
    _register(hub, "html", node.url)
    status, _, body = _get(hub, "/n/html/api/entries")
    assert status == 502
    assert b"internal page" not in body


def test_proxy_caps_response_size(hub, fake_node, monkeypatch):
    monkeypatch.setattr(dashboard, "MAX_NODE_RESPONSE", 10)
    node = fake_node(200, json.dumps(["x" * 20]).encode())
    _register(hub, "big", node.url)
    assert _get(hub, "/n/big/api/entries")[0] == 502


def test_proxy_joins_url_path_prefix(hub, fake_node):
    node = fake_node(200, b"[]")
    _register(hub, "p", node.url + "/prefix/")
    assert _get(hub, "/n/p/api/entries?limit=3")[0] == 200
    assert node.handler.hits == ["/prefix/api/entries?limit=3"]


def test_hub_subcommand_needs_no_log(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    config = tmp_path / "only.dippy"
    config.write_text("")
    served = []
    monkeypatch.setattr(
        dashboard.DashboardServer, "serve_forever", lambda self: served.append(self)
    )
    nodes = tmp_path / "nodes.json"
    assert handle_subcommand(_args(config, hub=str(nodes))) == 0
    assert served[0].nodes == nodes
    assert "hub" in capsys.readouterr().out


# === UI and contract ===


def test_ui_supports_hub_mode(server):
    script = _get(server, "/app.js", token=None)[2].decode()
    assert "/api/nodes" in script
    assert "/n/" in script


def test_api_contract_matches_routes():
    contract = json.loads(
        (
            dashboard.Path(__file__).parents[1] / "docs" / "dashboard-api.json"
        ).read_text()
    )
    documented = {
        (method.upper(), path)
        for path, item in contract["paths"].items()
        for method in item
        if method in ("get", "put", "post", "delete")
    }
    assert documented == {(method, path) for method, path, _, _ in dashboard.ROUTES}
