#!/usr/bin/env python3
"""Replay audit log entries through Python Dippy and dippy-rs.

Each unique Bash command and Read/Edit/Write file path is sent to both
implementations as a Claude Code PreToolUse payload with the entry's `cwd`
and the `$NAME=value` context flags as environment, against the real user
and project configs (real HOME). Nothing reaches the live audit log:
DIPPY_TEST_NO_LOG disables the audit log, a DIPPY_CONFIG override points
`log` (and its rotation) into a scratch directory, and DIPPY_ASKPASS is a
fake script that denies without opening a dialog. Agents launched with
DIPPY_CONFIG_ONLY are replayed with the normal stack.

A Rust allow where Python asks is UNSAFE (exit status 1), except WIDER
cases: Python asked only because it could not verify SQL queries, which
dippy-rs verifies with sqlparser, for a sqlite3 PRAGMA or dot-command that
dippy-rs allows as read-only introspection, or for a DuckDB batch with
INSTALL/LOAD of a safe extension (intended divergences).

Usage: audit_replay.py --scratch DIR [--out FILE] AUDIT_LOG...
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from hook_compare import PY, RS, decision  # noqa: E402

FILE_TOOLS = ("Read", "Edit", "Write")


def payload_key(entry: dict) -> tuple | None:
    """(tool, value, cwd, env) of a replayable entry, else None."""
    cwd = entry.get("cwd")
    if not cwd:
        return None
    env = tuple(
        sorted(
            tuple(flag[1:].split("=", 1))
            for flag in entry.get("context_flags", [])
            if flag.startswith("$") and "=" in flag
        )
    )
    if "command" in entry:
        return ("Bash", entry["command"], cwd, env)
    if entry.get("tool") in FILE_TOOLS and entry.get("file_path"):
        return (entry["tool"], entry["file_path"], cwd, env)
    return None


def payload(tool: str, value: str, cwd: str) -> dict:
    field = "command" if tool == "Bash" else "file_path"
    return {
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": {field: value},
        "cwd": cwd,
    }


def run(binary: str, key: tuple, base_env: dict) -> tuple[str, str]:
    """Decision and reason of one hook call."""
    tool, value, cwd, env = key
    proc = subprocess.run(
        [binary, "--claude"],
        input=json.dumps(payload(tool, value, cwd)),
        capture_output=True,
        text=True,
        env={**base_env, **dict(env)},
        cwd=cwd if os.path.isdir(cwd) else "/",
        timeout=60,
    )
    result = decision("claude", (proc.returncode, proc.stdout.strip(), ""))
    try:
        hso = json.loads(proc.stdout).get("hookSpecificOutput", {})
    except (json.JSONDecodeError, AttributeError):
        hso = {}
    return result, hso.get("permissionDecisionReason", "")


SQL_REASON = re.compile(
    r"(duckdb|sqlite3|psql|mysql|mariadb|sqlcmd) "
    r"\((unverified read-only query|unknown query)\)"
)
# Python reasons that are wider only when dippy-rs names the Rust-only allow
# in its reason: read-only sqlite3 PRAGMA/dot-commands, and DuckDB batches
# with INSTALL/LOAD of a safe extension.
RUST_ONLY_ALLOWS = (
    (
        re.compile(r"sqlite3 \((write query|dot-command)\)"),
        re.compile(r"sqlite3 \(read-only introspection\)"),
    ),
    (
        re.compile(r"duckdb \((write query|unknown query)\)"),
        re.compile(r"duckdb \(.* with safe extensions\)"),
    ),
)


def reason_parts(reason: str) -> list[str]:
    reason = reason.removeprefix("🐤").strip()
    return [part.strip() for part in reason.split(",") if part.strip()]


def sql_only(py_reason: str, rs_reason: str) -> bool:
    """Python asked only because it could not verify SQL queries, or for a
    statement that dippy-rs allowed as a Rust-only allow named in its reason."""
    parts = reason_parts(py_reason)
    rs_parts = reason_parts(rs_reason)
    for part in parts:
        if SQL_REASON.fullmatch(part):
            continue
        if not any(
            py.fullmatch(part) and any(rs.fullmatch(p) for p in rs_parts)
            for py, rs in RUST_ONLY_ALLOWS
        ):
            return False
    return bool(parts)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", required=True, type=Path)
    ap.add_argument("--out", type=Path)
    ap.add_argument("logs", nargs="+", type=Path)
    args = ap.parse_args()

    keys = {}
    skipped = 0
    for log in args.logs:
        for line in log.read_text(encoding="utf-8").splitlines():
            key = payload_key(json.loads(line)) if line.strip() else None
            if key is None:
                skipped += 1
            else:
                keys.setdefault(key, None)

    scratch = args.scratch.resolve()
    scratch.mkdir(parents=True, exist_ok=True)
    askpass = scratch / "askpass-deny"
    askpass.write_text("#!/bin/sh\ncat >/dev/null\nexit 1\n", encoding="utf-8")
    askpass.chmod(0o755)
    override = scratch / "override.dippy"
    override.write_text(
        f"set log {scratch / 'audit.log'}\nset askpass {askpass}\n", encoding="utf-8"
    )
    base_env = {
        "HOME": str(Path.home()),
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": "C.UTF-8",
        "DIPPY_TEST_NO_LOG": "1",
        "DIPPY_CONFIG": str(override),
        "DIPPY_ASKPASS": str(askpass),
    }

    def both(key):
        return run(PY, key, base_env), run(RS, key, base_env)

    with ThreadPoolExecutor(os.cpu_count() or 4) as pool:
        results = list(pool.map(both, keys))

    # Rust allowing where Python asks only for unverifiable SQL is WIDER, an
    # intended divergence (sqlparser verifies the query; sql_compare.py and
    # sql_fuzz.py check each SQL verdict). Any other Rust allow is UNSAFE.
    diffs = []
    for k, ((py, py_reason), (rs, rs_reason)) in zip(keys, results):
        if py == rs:
            continue
        kind = "safe"
        if rs == "allow":
            wider = py == "ask" and sql_only(py_reason, rs_reason)
            kind = "wider" if wider else "unsafe"
        diffs.append((k, py, rs, kind))
    order = ("unsafe", "wider", "safe")
    diffs.sort(key=lambda d: (order.index(d[3]), d[1], d[2]))
    kinds = collections.Counter(kind for *_, kind in diffs)
    pairs = collections.Counter((py, rs) for _, py, rs, _ in diffs)
    lines = [
        f"{len(keys)} unique payloads ({skipped} entries skipped): "
        f"{len(diffs)} differ, {kinds['unsafe']} unsafe (rs allow), "
        f"{kinds['wider']} wider (SQL verified by Rust only)",
        "  rust decisions: "
        + ", ".join(
            f"{d} {n}"
            for d, n in collections.Counter(r for _, (r, _) in results).items()
        ),
        *(f"  python {py} -> rust {rs}: {n}" for (py, rs), n in pairs.most_common()),
    ]
    for (tool, value, cwd, env), py, rs, kind in diffs:
        flags = " ".join(f"{k}={v}" for k, v in env)
        lines.append(
            f"[{kind}: py {py} | rs {rs}] {tool} cwd={cwd} {flags}\n    {value}"
        )
    report = "\n".join(lines)
    if args.out:
        args.out.write_text(report + "\n", encoding="utf-8")
        print("\n".join(lines[: 2 + len(pairs)]))
    else:
        print(report)
    return 1 if kinds["unsafe"] else 0


if __name__ == "__main__":
    sys.exit(main())
