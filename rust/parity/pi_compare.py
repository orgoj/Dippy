#!/usr/bin/env python3
"""Compare the pi-mono extension protocol of Python Dippy and dippy-rs.

Python: `src/dippy/pi_wrapper.py`; dippy-rs: `dippy-rs --pi`. Every payload
runs once per implementation with an empty HOME, the config via DIPPY_CONFIG
and the project directory as process cwd. Exit code and stdout must match
exactly (invalid JSON: serde vs json wording of the reason is normalized), as
must the audit log entries without `ts`.

A non-object payload is not compared: Python prints its exception text, the
Rust unit tests cover the fixed message.

Intended divergence: dippy-rs logs every call. A last entry Python lacks
(`idle`, unknown types) is dropped and counted, and every dippy-rs run must
log at least one entry, in the `set log` file or, when the config is not
loaded (invalid JSON, config error), in the fallback `~/.dippy/audit.log`.

Usage: pi_compare.py
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from hook_compare import REPO, RS  # noqa: E402
from parity_env import ROOT, child_env  # noqa: E402

PY = [
    str(REPO / ".venv-3.12" / "bin" / "python"),
    str(REPO / "src/dippy/pi_wrapper.py"),
]
BOX = ROOT / "pi"
HOME = BOX / "home"
PROJECT = BOX / "project"
CONFIG = BOX / "config"
AUDIT = BOX / "audit.log"
FALLBACK = HOME / ".dippy" / "audit.log"

INVALID_JSON = re.compile(r'("reason": "Invalid JSON input: )[^"]*(")')

RULES = """\
set context-env PIFLAG
allow frob *
allow [$PIFLAG=on] flagged *
deny zap * "use frob instead"
deny boom
allow-edit src/**
deny-edit .env "no secrets"
ask-read secret/* "secret file"
allow-read notes/*
allow-read ~/shared/*
"""

FORMATS = {
    "builtin": "",
    "general": 'set deny-format "G {pattern}|{command}|{reason}"\n',
    "agent": "set deny-format G\nset deny-format-codex 'C {reason} {pattern}'\n",
    "pass": "set default pass\n",
    "allow": "set default allow\nset log-full\n",
}

P = str(PROJECT)

PAYLOADS: list[tuple[dict | str, dict[str, str]]] = [
    ({"type": "bash", "command": "frob x", "cwd": P}, {}),
    ({"command": "frob x"}, {}),
    ({"type": "bash", "command": "zap it", "cwd": P}, {}),
    ({"type": "bash", "command": "boom", "cwd": P}, {}),
    ({"type": "bash", "command": "unknowncmd --x", "cwd": P}, {}),
    ({"type": "bash", "command": "frob a | frob b && frob c", "cwd": P}, {}),
    ({"type": "bash", "command": "flagged run", "cwd": P}, {"PIFLAG": "on"}),
    ({"type": "bash", "command": "flagged run", "cwd": P}, {}),
    ({"type": "bash", "command": "FOO=1 rm -rf /tmp/x", "cwd": P}, {}),
    ({"type": "bash", "command": "frob x > out.txt", "cwd": P}, {}),
    ({"type": "bash", "command": "", "cwd": P}, {}),
    ({"type": "bash", "command": "   ", "cwd": P}, {}),
    ({"type": "bash", "cwd": P}, {}),
    ({"type": "bash", "command": "zap é⚠", "cwd": P, "agent": "codex"}, {}),
    ({"type": "bash", "command": "zap", "cwd": P, "agent": "other"}, {}),
    ({"type": "bash", "command": "zap", "cwd": P, "agent": "claude"}, {}),
    ({"type": "bash", "command": "frob", "cwd": "sub"}, {}),
    ({"type": "bash", "command": "frob", "cwd": ""}, {}),
    ({"type": "edit", "path": "src/a.rs", "cwd": P}, {}),
    ({"type": "edit", "path": f"{P}/src/deep/b.rs", "cwd": P}, {}),
    ({"type": "edit", "path": "src/../.env", "cwd": P}, {}),
    ({"type": "edit", "path": ".env", "cwd": P, "agent": "codex"}, {}),
    ({"type": "edit", "path": "other.txt", "cwd": P}, {}),
    ({"type": "edit", "path": "", "cwd": P}, {}),
    ({"type": "read", "path": "secret/k", "cwd": P}, {}),
    ({"type": "read", "path": "notes/a", "cwd": P}, {}),
    ({"type": "read", "path": "~/shared/x", "cwd": P}, {}),
    ({"type": "read", "path": "notes/sub/a", "cwd": P}, {}),
    ({"type": "read", "cwd": P}, {}),
    ({"type": "idle", "cwd": P}, {}),
    ({"type": "write", "path": "src/a", "cwd": P}, {}),
    ("{not json", {}),
    ("", {}),
]


def entries_of(path: Path) -> list[dict]:
    entries = []
    if path.exists():
        for line in path.read_text().splitlines():
            entry = json.loads(line)
            entry.pop("ts", None)
            entries.append(entry)
        path.unlink()
    return entries


def run(argv: list[str], payload: dict | str, env_extra: dict[str, str]):
    """Exit code, stdout, `set log` entries and fallback log entries."""
    for path in (AUDIT, FALLBACK):
        if path.exists():
            path.unlink()
    env = child_env()
    del env["DIPPY_TEST_NO_LOG"]
    env["HOME"] = str(HOME)
    env["DIPPY_CONFIG"] = str(CONFIG)
    env.update(env_extra)
    text = payload if isinstance(payload, str) else json.dumps(payload)
    proc = subprocess.run(
        argv,
        input=text,
        capture_output=True,
        text=True,
        env=env,
        cwd=PROJECT,
        timeout=60,
    )
    stdout = INVALID_JSON.sub(r"\1...\2", proc.stdout.strip())
    return proc.returncode, stdout, entries_of(AUDIT), entries_of(FALLBACK)


def closing(py, rs) -> tuple[tuple, bool]:
    """`rs` without its closing entry when Python lacks it (see module doc)."""
    code, stdout, entries, _ = rs
    if entries and entries[:-1] == py[2] and entries != py[2]:
        return (code, stdout, entries[:-1]), True
    return rs[:3], False


def main() -> int:
    if BOX.exists():
        shutil.rmtree(BOX)
    for d in (HOME, PROJECT / "sub", PROJECT / "src"):
        d.mkdir(parents=True)
    total = differ = audited = closed = unlogged = 0
    for name, extra in FORMATS.items():
        CONFIG.write_text(f"{RULES}{extra}set log {AUDIT}\n")
        for payload, env_extra in PAYLOADS:
            total += 1
            py = run(PY, payload, env_extra)
            audited += len(py[2])
            rs = run([RS, "--pi"], payload, env_extra)
            if not rs[2] and not rs[3]:
                unlogged += 1
                print(f"NO ENTRY [{name}] {payload!r}")
            rs, dropped = closing(py, rs)
            closed += dropped
            if py[:3] != rs:
                differ += 1
                print(f"DIFF [{name}] {payload!r} {env_extra}")
                print(f"  py: {py}")
                print(f"  rs: {rs}")
    # A config error is reported, not raised.
    CONFIG.write_text("set run-on-server-ssh-config\n")
    for payload, env_extra in PAYLOADS[:1]:
        total += 1
        py = run(PY, payload, env_extra)
        rs = run([RS, "--pi"], payload, env_extra)
        if not rs[3]:
            unlogged += 1
            print(f"NO ENTRY [config error] {rs}")
        # Intended: dippy-rs denies with a message telling the agent to stop.
        stop = "Dippy blocks every tool call until the configuration is fixed"
        if py[:3] != rs[:3] and not ('"action": "deny"' in rs[1] and stop in rs[1]):
            differ += 1
            print(f"DIFF [config error]\n  py: {py}\n  rs: {rs}")
    print(
        f"{total} payloads ({audited} audit entries), "
        f"{total - differ} same, {differ} differ; "
        f"{closed} dippy-rs closing entries dropped, "
        f"{unlogged} dippy-rs runs without an entry"
    )
    return 1 if differ or unlogged else 0


if __name__ == "__main__":
    sys.exit(main())
