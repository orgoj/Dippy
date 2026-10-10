#!/usr/bin/env python3
"""Compare `dippy config` of Python Dippy and dippy-rs.

Each scenario runs once per implementation in the same fresh sandbox (empty
HOME with `~/.dippy/config`, a project directory as cwd for `--project`),
optionally seeded with config files. Every step compares exit code and
stdout, and stderr unless the step is a usage error (exit 2: argparse and
clap word their usage differently). After the scenario the config files and
their modes must be identical.

Usage: config_compare.py
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from hook_compare import PY, RS  # noqa: E402
from parity_env import ROOT  # noqa: E402

BOX = ROOT / "config"
HOME = BOX / "home"
PROJECT = BOX / "project"
USER_FILE = HOME / ".dippy" / "config"
PROJECT_FILE = PROJECT / ".dippy"

KEYS = [
    "askpass",
    "askpass-timeout",
    "approval-wait-message",
    "run-on-server-backend",
    "run-on-server-session",
    "run-on-server-timeout",
    "run-on-server-poll-interval",
    "run-on-server-ssh-config",
    "run-on-server-ssh-auth-sock",
]

SEEDED = """\
# user config
set askpass-timeout 1
allow ls

# comment between
set ASKPASS_TIMEOUT 2
set askpass_timeout 3
server a
server  b
"""

SCENARIOS: list[tuple[str, dict[Path, tuple[str, int]], list[list[str]]]] = [
    (
        "defaults",
        {},
        [["get", k] for k in KEYS] + [["get"], ["server", "list"], ["get", "nope"]],
    ),
    (
        "set and get",
        {},
        [
            ["set", "askpass", "~/bin/ask"],
            ["set", "askpass-timeout", "30"],
            ["set", "Askpass_Timeout", "31"],
            ["set", "askpass-timeout", "abc"],
            ["set", "askpass-timeout", " 4_0 "],
            ["set", "run-on-server-timeout", "1e20"],
            ["set", "run-on-server-poll-interval", "0.25"],
            ["set", "run-on-server-backend", "herdr"],
            ["set", "run-on-server-backend", "bogus"],
            ["set", "run-on-server-session", "'my sess'"],
            ["set", "approval-wait-message", "  Hold on.  "],
            ["set", "approval-wait-message", "''"],
            ["set", "run-on-server-ssh-config", "cfg/ssh"],
            ["set", "run-on-server-ssh-auth-sock", "none"],
            ["set", "nope", "1"],
            ["set", "run-on-server-timeout", "-1"],
            ["get"],
        ]
        + [["get", k] for k in KEYS]
        + [["unset", "askpass-timeout"], ["unset", "nope"], ["get"]],
    ),
    (
        "ssh profile none",
        {},
        [
            ["set", "run-on-server-ssh-config", "none"],
            ["get", "run-on-server-ssh-config"],
            ["set", "run-on-server-ssh-auth-sock", "/run/a.sock"],
            ["get", "run-on-server-ssh-auth-sock"],
        ],
    ),
    (
        "floats",
        {},
        [
            step
            for value in ["5", "1e16", "1e-5", "0.0001", "123456789.125", "1_000.5"]
            for step in (
                ["set", "run-on-server-timeout", value],
                ["get", "run-on-server-timeout"],
            )
        ],
    ),
    (
        "project scope",
        {},
        [
            ["set", "--project", "run-on-server-session", "proj"],
            ["get", "--project", "run-on-server-session"],
            ["get", "--user", "run-on-server-session"],
            ["server", "add", "--project", "web1"],
            ["server", "list", "--project"],
            ["server", "list"],
            ["get", "--project"],
            ["get", "--user", "--project"],
        ],
    ),
    (
        "servers",
        {},
        [
            ["server", "add", "web1"],
            ["server", "add", "web1"],
            ["server", "add", "db.2_x-y"],
            ["server", "add", "bad@x"],
            ["server", "add", "_x"],
            ["server", "add", "a b"],
            ["server", "list"],
            ["server", "remove", "web1"],
            ["server", "remove", "web1"],
            ["server", "list"],
            ["get"],
        ],
    ),
    (
        "comments and duplicates kept",
        {USER_FILE: (SEEDED, 0o644)},
        [
            ["get", "askpass-timeout"],
            ["server", "list"],
            ["set", "askpass-timeout", "7"],
            ["server", "add", "b"],
            ["server", "remove", "a"],
            ["get"],
            ["unset", "askpass-timeout"],
            ["get"],
        ],
    ),
    (
        "mode kept",
        {PROJECT_FILE: ("allow x\n", 0o640)},
        [["set", "--project", "askpass-timeout", "9"], ["get", "--project"]],
    ),
    (
        "usage errors",
        {},
        [["config"], ["server"], ["set", "askpass"], ["get", "--bogus"]],
    ),
]


def run(binary: str, args: list[str]) -> tuple[int, str, str]:
    env = {
        "HOME": str(HOME),
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": "C.UTF-8",
        "DIPPY_TEST_NO_LOG": "1",
    }
    argv = [binary, *args] if args == ["config"] else [binary, "config", *args]
    proc = subprocess.run(
        argv, cwd=PROJECT, env=env, capture_output=True, text=True, timeout=30
    )
    return proc.returncode, proc.stdout, proc.stderr


def files() -> list[tuple[str, str, str]]:
    return [
        (str(p), p.read_text(), oct(p.stat().st_mode & 0o777))
        for p in (USER_FILE, PROJECT_FILE)
        if p.exists()
    ]


def play(binary: str, seed: dict[Path, tuple[str, int]], steps: list[list[str]]):
    if BOX.exists():
        shutil.rmtree(BOX)
    PROJECT.mkdir(parents=True)
    HOME.mkdir()
    for path, (text, mode) in seed.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(mode)
    outputs = [run(binary, step) for step in steps]
    return outputs, files()


def main() -> int:
    differ = total = 0
    for name, seed, steps in SCENARIOS:
        py_out, py_files = play(PY, seed, steps)
        rs_out, rs_files = play(RS, seed, steps)
        for step, py, rs in zip(steps, py_out, rs_out):
            total += 1
            usage = py[0] == 2 and rs[0] == 2
            if py[:2] != rs[:2] or (not usage and py[2] != rs[2]):
                differ += 1
                print(
                    f"  [{name}] config {' '.join(step)}\n     py: {py}\n     rs: {rs}"
                )
        if py_files != rs_files:
            differ += 1
            print(f"  [{name}] files\n     py: {py_files}\n     rs: {rs_files}")
    print(f"config: {len(SCENARIOS)} scenarios, {total} steps, {differ} differ")
    return 1 if differ else 0


if __name__ == "__main__":
    sys.exit(main())
