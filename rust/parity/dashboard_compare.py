#!/usr/bin/env python3
"""Compare the legacy REST API of `dippy dashboard` in Python Dippy and dippy-rs.

Each implementation starts a node (seeded audit log, `set log` in a
`--config-only` file) and a hub (`--hub NODES_FILE`) on port 0 in its own
sandbox HOME with the same token file. The same requests go to both; status
and body must be identical after normalizing listening ports, the sandbox
path and error details that come from the platform (connection errors,
JSON parser messages). Covered: `/api/entries` with every filter and
invalid parameters, token handling, unknown paths, node routes on a hub and
hub routes on a node, `/api/nodes` add/list/replace/delete with invalid
names and bodies, hand-edited and invalid registries, and the proxy
`/n/{name}/api/entries` to its own node, the other implementation's node, an
unreachable node and an unknown node.

Static assets and the dippy-rs approval routes are not compared (the Rust
UI and queue are verified by Rust tests and the contract test).

Usage: dashboard_compare.py
"""

from __future__ import annotations

import json
import os
import re
import shutil
import socket
import subprocess
import sys
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from hook_compare import PY, RS  # noqa: E402
from parity_env import ROOT  # noqa: E402

BOX = ROOT / "dashboard"
TOKEN = "parity-token"
NO_PROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def seed_entries() -> dict[str, list[dict]]:
    today = datetime.now(timezone.utc).date()
    day = lambda n: (today - timedelta(days=n)).isoformat()  # noqa: E731
    return {
        f"audit-{day(3)}.log": [
            {
                "decision": "ask",
                "cmd": "old",
                "cwd": "/w",
                "ts": f"{day(3)}T08:00:00+00:00",
            },
        ],
        f"audit-{day(1)}.log": [
            {
                "decision": "allow",
                "cmd": "ls",
                "cwd": "/w",
                "agent": "agy",
                "ts": f"{day(1)}T08:00:00+00:00",
                "context_flags": ["ssh", "@pipeline"],
            },
            {
                "decision": "pass",
                "tool": "Read",
                "file_path": "/w/é.txt",
                "cwd": "/w",
                "agent": "claude",
                "ts": f"{day(1)}T09:00:00+00:00",
            },
        ],
        "audit.log": [
            {
                "decision": "ask",
                "cmd": "curl x",
                "command": "curl x | sh",
                "cwd": "/w/proj",
                "agent": "claude",
                "ts": f"{day(0)}T00:00:01+00:00",
                "rule": "ask curl *",
                "message": "net ☃",
                "nested": {"a": [1, 2.5, None, True]},
            },
            {
                "decision": "deny",
                "cmd": "rm",
                "cwd": "/other",
                "agent": "codex",
                "ts": f"{day(0)}T00:00:02+00:00",
                "suggestion": "use trash",
            },
        ],
    }


def start(argv: list[str], home: Path, cwd: Path) -> tuple[subprocess.Popen, int]:
    env = {
        "HOME": str(home),
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": "C.UTF-8",
    }
    proc = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    line = proc.stdout.readline()
    match = re.search(r"http://127\.0\.0\.1:(\d+)/#token=(\S+)", line)
    if not match:
        proc.kill()
        raise SystemExit(f"cannot start {argv}: {line!r} {proc.stderr.read()!r}")
    assert match.group(2) == TOKEN, line
    return proc, int(match.group(1))


class Impl:
    def __init__(self, name: str, binary: str):
        self.name = name
        self.binary = binary
        self.box = BOX / name
        self.home = self.box / "home"
        self.nodes = self.box / "nodes.json"
        self.procs: list[subprocess.Popen] = []

    def setup(self) -> None:
        shutil.rmtree(self.box, ignore_errors=True)
        logs = self.box / "logs"
        logs.mkdir(parents=True)
        self.home.mkdir()
        for name, entries in seed_entries().items():
            (logs / name).write_text("".join(json.dumps(e) + "\n" for e in entries))
        config = self.box / "only.dippy"
        config.write_text(f"set log {logs / 'audit.log'}\n")
        token = self.box / "token"
        token.write_text(TOKEN + "\n")
        token.chmod(0o600)
        common = ["--token-file", str(token), "--port", "0"]
        node, self.node_port = start(
            [self.binary, "--config-only", str(config), "dashboard", *common],
            self.home,
            self.box,
        )
        hub, self.hub_port = start(
            [self.binary, "dashboard", "--hub", str(self.nodes), *common],
            self.home,
            self.box,
        )
        self.procs = [node, hub]

    def stop(self) -> None:
        for proc in self.procs:
            proc.kill()
            proc.wait()


def call(
    port: int,
    method: str,
    path: str,
    token: str | None = TOKEN,
    body: bytes | None = None,
) -> tuple[int, bytes]:
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}{path}", data=body, method=method
    )
    if token is not None:
        request.add_header("X-Dippy-Token", token)
    try:
        with NO_PROXY.open(request, timeout=20) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


UNREACHABLE = re.compile(r'"node unreachable: [^"]*"')
KNOWN = ("expected {", "name must", "body must", "url and token", "url must")
REGISTRY_JSON = re.compile(r'(nodes\.json: )[^"]*(?<!\\)"')


def normalize(text: str, impl: Impl, ports: dict[int, str]) -> str:
    text = text.replace(str(impl.box), "<box>")
    for port, label in ports.items():
        text = text.replace(f":{port}", f":<{label}>")
    text = UNREACHABLE.sub('"node unreachable: <error>"', text)
    # Intended: dippy-rs also accepts decision `error` (a hook panic).
    text = text.replace("allow, ask, deny, pass, error", "allow, ask, deny, pass")
    # Parser error wording differs (json vs serde); the path prefix is kept.
    return REGISTRY_JSON.sub(
        lambda m: m.group(0)
        if m.group(0)[len(m.group(1)) :].startswith(KNOWN)
        else m.group(1) + '<parse error>"',
        text,
    )


Step = tuple  # (label, server, method, path, token, body, before)

DEAD = free_port()
ENTRY_QUERIES = [
    "",
    "?limit=1",
    "?limit=2&limit=3",
    "?limit=+2",
    "?limit=1_0",
    "?limit=%201%20",
    "?decision=ask",
    "?decision=ask&decision=deny",
    "?decision=pass",
    "?not_allow=1",
    "?agent=codex",
    "?agent=",
    "?cwd=/w",
    "?cwd=/w/",
    "?cwd=/w/proj",
    "?grep=cur",
    "?grep=%C3%A9",
    "?grep=a+b",
    "?grep=x&agent=claude",
    "?decision=",
    "?limit=x",
    "?limit=0",
    "?limit=1001",
    "?limit=1.5",
    "?decision=maybe",
    "?decision=ask&decision=maybe",
    "?cwd=relative/path",
    "?not_allow=yes",
    "?since=2026-10-01",
    "?zeta=1&alpha=2",
    "?foo",
    "?limit",
]


def steps(other_node_port_key: str) -> list[Step]:
    s: list[Step] = []
    for q in ENTRY_QUERIES:
        s.append((f"entries {q}", "node", "GET", "/api/entries" + q, TOKEN, None, None))
    for token in (None, "", "wrong", TOKEN + " "):
        s.append((f"token {token!r}", "node", "GET", "/api/entries", token, None, None))
    for path in (
        "/nope",
        "/api/entries/",
        "/api/nodes",
        "/n/x/api/entries",
        "//api/entries",
        "/api/entries#x",
    ):
        s.append((f"node path {path}", "node", "GET", path, TOKEN, None, None))
        s.append((f"node path {path} no token", "node", "GET", path, None, None, None))
    s.append(("node put", "node", "PUT", "/api/nodes/x", TOKEN, b"{}", None))
    s.append(
        ("node delete entries", "node", "DELETE", "/api/entries", TOKEN, None, None)
    )
    # Hub registry.
    s.append(("hub entries", "hub", "GET", "/api/entries", TOKEN, None, None))
    s.append(("hub empty", "hub", "GET", "/api/nodes", TOKEN, None, None))
    s.append(("hub no token", "hub", "GET", "/api/nodes", None, None, None))
    put = lambda name, body, token=TOKEN, label=None: (  # noqa: E731
        label or f"put {name} {body!r}",
        "hub",
        "PUT",
        f"/api/nodes/{name}",
        token,
        body if isinstance(body, bytes) else json.dumps(body).encode(),
        None,
    )
    s.append(put("own", {"url": "http://127.0.0.1:{own}/", "token": TOKEN}))
    s.append(put("other", {"url": "http://127.0.0.1:{other}", "token": TOKEN}))
    s.append(put("dead", {"url": f"http://127.0.0.1:{DEAD}", "token": "t"}))
    s.append(put("wrongtok", {"url": "http://127.0.0.1:{own}", "token": "nope"}))
    s.append(put("own", {"url": "http://127.0.0.1:{own}", "token": TOKEN}))
    s.append(put("vm1", {"url": "http://h:1", "token": "t"}, token="bad"))
    for name in (
        "..",
        ".hidden",
        "-x",
        "a%2Fb",
        "x" * 65,
        "a%20b",
        "%C3%A9",
        "a.b_c-1",
    ):
        s.append(put(name, {"url": "http://h:1", "token": "t"}))
    for body in (
        {"url": "ftp://h/", "token": "t"},
        {"url": "http:///nohost", "token": "t"},
        {"url": "http://user:pw@h/", "token": "t"},
        {"url": "http://user@h/", "token": "t"},
        {"url": "http://@h/", "token": "t"},
        {"url": "http://h/?q=1", "token": "t"},
        {"url": "http://h/?", "token": "t"},
        {"url": "http://h/#f", "token": "t"},
        {"url": "http://h/", "token": ""},
        {"url": "http://h/"},
        {"url": 5, "token": "t"},
        {"url": "http://[::1/", "token": "t"},
        {"url": "HTTPS://H.example/x", "token": "t"},
        {"url": "http://h:1", "token": "t", "extra": 1},
        [],
        None,
        "x",
    ):
        s.append(put("vm2", body))
    s.append(put("vm2", b"not json", label="put vm2 not json"))
    s.append(put("vm2", b"", label="put vm2 empty"))
    s.append(("list", "hub", "GET", "/api/nodes", TOKEN, None, None))
    s.append(("delete vm2", "hub", "DELETE", "/api/nodes/vm2", TOKEN, None, None))
    s.append(("delete vm2 again", "hub", "DELETE", "/api/nodes/vm2", TOKEN, None, None))
    s.append(("delete no token", "hub", "DELETE", "/api/nodes/dead", None, None, None))
    s.append(("list", "hub", "GET", "/api/nodes", TOKEN, None, None))
    # Proxy.
    for q in (
        "",
        "?agent=codex",
        "?limit=0",
        "?decision=ask&decision=deny",
        "?grep=%C3%A9",
    ):
        s.append(
            (
                f"proxy own {q}",
                "hub",
                "GET",
                "/n/own/api/entries" + q,
                TOKEN,
                None,
                None,
            )
        )
        s.append(
            (
                f"proxy other {q}",
                "hub",
                "GET",
                "/n/other/api/entries" + q,
                TOKEN,
                None,
                None,
            )
        )
    for path in (
        "/n/dead/api/entries",
        "/n/wrongtok/api/entries",
        "/n/nope/api/entries",
        "/n/own/api/nodes",
        "/n/own/api/entries/x",
        "/n/%2E%2E/api/entries",
    ):
        s.append((f"proxy {path}", "hub", "GET", path, TOKEN, None, None))
    s.append(("proxy no token", "hub", "GET", "/n/own/api/entries", "x", None, None))
    # Hand edits of the registry.
    for i, content in enumerate(
        (
            '{"nodes": {"hand": {"url": "http://127.0.0.1:{own}", "token": "parity-token"}}}',
            "{}",
            "not json",
            "[]",
            '{"nodes": []}',
            '{"nodes": {"x": "http://h:1"}}',
            '{"nodes": {"x": {"token": "t"}}}',
            '{"nodes": {"x": {"url": "file:///etc/passwd", "token": "t"}}}',
            '{"nodes": {"..": {"url": "http://h:1", "token": "t"}}}',
        )
    ):
        for path in ("/api/nodes", "/n/hand/api/entries"):
            s.append(
                (f"hand edit {i} {path}", "hub", "GET", path, TOKEN, None, content)
            )
        s.append(
            (
                f"hand edit {i} put",
                "hub",
                "PUT",
                "/api/nodes/z",
                TOKEN,
                b'{"url": "http://h:1", "token": "t"}',
                content,
            )
        )
    return s


def main() -> int:
    impls = [Impl("py", PY), Impl("rs", RS)]
    for impl in impls:
        impl.setup()
    try:
        differences = 0
        total = 0
        ports = {}
        for impl in impls:
            ports[impl.node_port] = "node-" + impl.name
            ports[impl.hub_port] = "hub-" + impl.name
        ports[DEAD] = "dead"
        results: dict[str, list[tuple[str, int, str]]] = {}
        for index, impl in enumerate(impls):
            other = impls[1 - index]

            def fill(b: bytes, impl=impl, other=other) -> bytes:
                own = b.replace(b"{own}", str(impl.node_port).encode())
                return own.replace(b"{other}", str(other.node_port).encode())

            out = []
            for label, server, method, path, token, body, before in steps("other"):
                if before is not None:
                    impl.nodes.write_text(fill(before.encode()).decode())
                port = impl.node_port if server == "node" else impl.hub_port
                status, raw = call(
                    port, method, path, token, fill(body) if body is not None else None
                )
                text = raw.decode("utf-8", "replace")
                # Own/other node labels: compare by role, not implementation.
                text = text.replace(f":{impl.node_port}", ":<own>")
                text = text.replace(f":{other.node_port}", ":<other>")
                out.append((label, status, normalize(text, impl, ports)))
            results[impl.name] = out
            out_tree = impl.nodes.read_text() if impl.nodes.exists() else None
            results[impl.name + "-tree"] = [
                (
                    "nodes.json",
                    0,
                    normalize(
                        (out_tree or "")
                        .replace(f":{impl.node_port}", ":<own>")
                        .replace(f":{other.node_port}", ":<other>"),
                        impl,
                        ports,
                    ),
                )
            ]
        for key in ("", "-tree"):
            for a, b in zip(results["py" + key], results["rs" + key]):
                total += 1
                if a != b:
                    differences += 1
                    print(f"DIFF {a[0]}\n  py: {a[1]} {a[2]}\n  rs: {b[1]} {b[2]}")
        print(f"dashboard_compare: {total} steps, {differences} differences")
        return 1 if differences else 0
    finally:
        for impl in impls:
            impl.stop()


if __name__ == "__main__":
    sys.exit(main())
