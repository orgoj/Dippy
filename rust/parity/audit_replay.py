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

Usage: audit_replay.py --scratch DIR [--out FILE] AUDIT_LOG...
"""

from __future__ import annotations

import argparse
import collections
import json
import os
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


def run(binary: str, key: tuple, base_env: dict) -> str:
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
    return decision("claude", (proc.returncode, proc.stdout.strip(), ""))


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

    diffs = [(k, py, rs) for k, (py, rs) in zip(keys, results) if py != rs]
    # Unsafe first: dippy-rs allows what Python does not.
    diffs.sort(key=lambda d: (d[2] != "allow", d[1], d[2]))
    unsafe = sum(1 for _, _, rs in diffs if rs == "allow")
    pairs = collections.Counter((py, rs) for _, py, rs in diffs)
    lines = [
        f"{len(keys)} unique payloads ({skipped} entries skipped): "
        f"{len(diffs)} differ, {unsafe} unsafe (rs allow)",
        "  rust decisions: "
        + ", ".join(
            f"{d} {n}" for d, n in collections.Counter(r for _, r in results).items()
        ),
        *(f"  python {py} -> rust {rs}: {n}" for (py, rs), n in pairs.most_common()),
    ]
    for (tool, value, cwd, env), py, rs in diffs:
        flags = " ".join(f"{k}={v}" for k, v in env)
        lines.append(f"[py {py} | rs {rs}] {tool} cwd={cwd} {flags}\n    {value}")
    report = "\n".join(lines)
    if args.out:
        args.out.write_text(report + "\n", encoding="utf-8")
        print("\n".join(lines[: 2 + len(pairs)]))
    else:
        print(report)
    return 1 if unsafe else 0


if __name__ == "__main__":
    sys.exit(main())
