#!/usr/bin/env python3
"""Compare `dippy doctor` of Python Dippy and dippy-rs.

Each scenario runs once per implementation in the same fresh sandbox: an
empty HOME, a project directory as cwd and a `bin` directory as the whole
PATH besides /usr/bin:/bin, optionally holding a fake `dippy`. Every step
compares exit code, stdout and stderr (stderr not for usage errors). After
the scenario the whole sandbox tree (paths, contents, modes) must be
identical.

Intentional divergences are covered by Rust tests, not here: `--agent
moltbot`, legacy detection of an unrelated `/dippy` path, Codex matchers
and the pi-mono bridge line (both only with --verbose), a hook config that
is valid JSON but not an object, and `--agent` with only a project config
(both Python exceptions).

Usage: doctor_compare.py
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from hook_compare import PY, RS  # noqa: E402
from parity_env import ROOT  # noqa: E402

BOX = ROOT / "doctor"
HOME = BOX / "home"
PROJECT = BOX / "project"
BIN = BOX / "bin"

STAMP = re.compile(r"\.dippy-backup-\d{8}_\d{6}")

Seed = dict[str, tuple[str | None, int]]


def f(text: str | None, mode: int | None = None):
    """A file (or a directory, text None); `@target` is a symlink."""
    return (text, mode or (0o755 if text is None else 0o644))


def d(*args: str) -> list[str]:
    return ["doctor", *args]


FAKE_DIPPY = f("#!/bin/sh\necho 'dippy 9.9.9'\necho second\n", 0o755)
FAILING_DIPPY = f("#!/bin/sh\nexit 3\n", 0o755)

BASIC = [d(), d("--verbose"), d("--json"), d("--json", "--verbose"), d("--quiet")]
AGENT_IDS = ["claude", "gemini", "agy", "cursor", "windsurf", "pi", "codex", "pearai"]
AGENTS = [d("--agent", a, "--verbose") for a in AGENT_IDS]

CLAUDE_HOOK = """{"hooks": {"PreToolUse": [{"matcher": "Bash|Read", "hooks": [
  {"type": "command", "command": "dippy --claude"}]}],
  "PostToolUse": [{"matcher": 7, "hooks": [{"command": "other"}]}]}}"""
LEGACY_CLAUDE = """{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [
  {"type": "command", "command": "/usr/local/bin/dippy-hook --claude"}]}]}}"""
CODEX_HOOK = """{"hooks": {
  "PreToolUse": [{"matcher": "^Bash$", "hooks": [{"type": "command", "command": "dippy --codex"}]}],
  "PermissionRequest": [{"matcher": "^Bash$", "hooks": [{"type": "command", "command": "dippy --codex"}]}]
}}"""
GEMINI_HOOK = """{"hooks": {"BeforeTool": [{"matcher": "run_shell_command", "hooks": [
  {"type": "command", "command": "dippy --gemini"}]}]}, "approvalMode": "yolo"}"""

CODEX = [d(), d("--json"), d("--agent", "codex")]
NO_VERBOSE = [d(), d("--json"), d("--quiet")]

SCENARIOS: list[tuple[str, Seed, list[list[str]]]] = [
    ("nothing installed, no dippy", {}, [*BASIC, *AGENTS]),
    ("fake dippy on PATH", {"bin/dippy": FAKE_DIPPY}, BASIC),
    ("failing dippy on PATH", {"bin/dippy": FAILING_DIPPY}, BASIC),
    (
        "dippy not executable",
        {"bin/dippy": f("#!/bin/sh\n", 0o644)},
        [d("--json")],
    ),
    (
        "claude hooks global and project",
        {
            "bin/dippy": FAKE_DIPPY,
            "home/.claude/settings.json": f(CLAUDE_HOOK),
            "project/.claude/settings.json": f(CLAUDE_HOOK),
        },
        [*BASIC, *AGENTS],
    ),
    (
        "claude project only",
        {
            "home/.claude/settings.json": f('{"model": "x"}'),
            "project/.claude/settings.json": f(CLAUDE_HOOK),
        },
        [*BASIC, d("--agent", "claude", "--verbose")],
    ),
    (
        "claude empty and invalid configs",
        {
            "home/.claude/settings.json": f("{}"),
            "project/.claude/settings.json": f("{bad"),
            "home/.cursor/hooks.json": f(""),
        },
        [*BASIC, d("--agent", "claude"), d("--agent", "cursor", "--verbose")],
    ),
    (
        "legacy hook",
        {"home/.claude/settings.json": f(LEGACY_CLAUDE)},
        [*BASIC, d("--agent", "claude", "--verbose")],
    ),
    (
        "gemini modes",
        {
            "home/.gemini/settings.json": f(GEMINI_HOOK),
            "project/.gemini/settings.json": f(
                '{"policyEngineConfig": {"approvalMode": "default"}}'
            ),
        },
        [*BASIC, d("--agent", "gemini", "--verbose"), d("--agent", "gemini")],
    ),
    (
        "gemini project yolo",
        {
            "home/.gemini/settings.json": f('{"approvalMode": ""}'),
            "project/.gemini/settings.json": f('{"approvalMode": "yolo"}'),
        },
        [d("--agent", "gemini", "--verbose")],
    ),
    (
        "gemini without approval mode",
        {"home/.gemini/settings.json": f('{"policyEngineConfig": {"x": 1}}')},
        [d("--agent", "gemini", "--verbose")],
    ),
    (
        "agy, cursor and windsurf hooks",
        {
            "home/.gemini/config/hooks.json": f(
                '{"dippy": {"PreToolUse": [{"hooks": [{"command": "dippy --agy"}]}]}}'
            ),
            "project/.cursor/hooks.json": f(
                '{"hooks": {"beforeShellExecution": [{"command": "dippy --cursor"}]}}'
            ),
            "home/.windsurf/hooks.json": f('{"hooks": {}}'),
        },
        [*BASIC, d("--agent", "agy", "--verbose"), d("--agent", "windsurf")],
    ),
    (
        "codex without config",
        {"home/.codex/config.toml": f("[features]\nhooks = true\n")},
        CODEX,
    ),
    (
        "codex global compatible",
        {
            "home/.codex/hooks.json": f(CODEX_HOOK),
            "home/.codex/config.toml": f(
                'approval_policy = "on-request"\n[features]\nhooks = true\n'
            ),
        },
        CODEX,
    ),
    (
        "codex project policy and missing flags",
        {
            "home/.codex/hooks.json": f(CODEX_HOOK),
            "home/.codex/config.toml": f("approval_policy = 'never' # c\n"),
            "project/.codex/hooks.json": f(CODEX_HOOK),
            "project/.codex/config.toml": f(
                '[profiles.x]\napproval_policy = "on-request"\n'
            ),
        },
        CODEX,
    ),
    (
        "codex project overrides policy",
        {
            "project/.codex/hooks.json": f(CODEX_HOOK),
            "project/.codex/config.toml": f(
                'approval_policy = "on-request"\n[features]\ncodex_hooks = true\n'
            ),
        },
        CODEX[:2],
    ),
    (
        "pi extension file",
        {"home/.pi/agent/extensions/dippy-extension.ts": f("// x\n")},
        [*NO_VERBOSE, d("--agent", "pi")],
    ),
    (
        "pi extension symlink",
        {
            "home/real/ext.ts": f("// x\n"),
            "home/.pi/agent/extensions/dippy-extension.ts": f("@../../../real/ext.ts"),
            "home/.pi/config.json": f("{}"),
        },
        [*NO_VERBOSE, d("--agent", "pi")],
    ),
    (
        "configs valid",
        {
            "home/.dippy/config": f("allow foo *\n"),
            "project/.dippy": f("ask bar\n"),
        },
        BASIC,
    ),
    (
        "config warnings",
        {
            "home/.dippy/config": f("allow\ninclude nomatch-*\n"),
            "project/.dippy": f("bogus line\n"),
        },
        BASIC,
    ),
    (
        "project config only",
        {"project/.dippy": f("allow foo\n")},
        BASIC,
    ),
    (
        "config errors",
        {
            "home/.dippy/config": f("include ~/.dippy/config\n"),
            "project/.dippy": f("allow x\n"),
        },
        BASIC,
    ),
    (
        "project config error",
        {"home/.dippy/config": f(""), "project/.dippy": f("include .dippy\n")},
        BASIC,
    ),
    (
        "logs",
        {
            "home/.claude/hook-approvals.log": f("x\n"),
            "home/.dippy/audit.log": f("#11MB"),
            "home/.codex": f(None, 0o555),
            "home/.gemini": f("a file"),
        },
        [d(), d("--verbose"), d("--json")],
    ),
    (
        "small logs verbose",
        {"home/.dippy/audit.log": f("y" * 5000)},
        [d("--verbose")],
    ),
    (
        "fix",
        {
            "home/.claude": f(None),
            "home/.claude/settings.json": f(LEGACY_CLAUDE),
            "home/.cursor": f(None),
            "project/.codex/hooks.json": f(CODEX_HOOK),
        },
        [d("--fix"), d("--fix", "--json"), d("--fix", "--quiet")],
    ),
    (
        "--cwd",
        {"home/.claude/settings.json": f(CLAUDE_HOOK)},
        [
            ["--cwd", str(HOME), *d("--verbose")],
            ["--cwd", "sub", *d("--json")],
            ["--cwd", "sub/", *d("--agent", "claude", "--verbose")],
        ],
    ),
    (
        "usage errors",
        {},
        [d("--agent", "bogus"), d("--agent"), d("--nope")],
    ),
]


def env() -> dict[str, str]:
    home = str(HOME)
    assert home.startswith(str(BOX)), "sandbox HOME must stay inside the box"
    return {
        "HOME": home,
        "PATH": f"{BIN}:/usr/bin:/bin",
        "LANG": "C.UTF-8",
        "DIPPY_TEST_NO_LOG": "1",
    }


def normalize(text: str) -> str:
    return STAMP.sub(".dippy-backup-STAMP", text)


def run(binary: str, args: list[str]) -> tuple[int, str, str]:
    proc = subprocess.run(
        [binary, *args],
        cwd=PROJECT,
        env=env(),
        capture_output=True,
        text=True,
        timeout=30,
    )
    return proc.returncode, normalize(proc.stdout), normalize(proc.stderr)


def tree() -> list[tuple[str, str, str]]:
    result = []
    for path in sorted(BOX.rglob("*")):
        rel = normalize(str(path.relative_to(BOX)))
        if path.is_symlink():
            result.append((rel, f"-> {os.readlink(path)}", ""))
            continue
        mode = oct(path.stat().st_mode & 0o777)
        if path.is_dir():
            content = "<dir>"
        elif path.stat().st_size > 1_000_000:
            content = f"<{path.stat().st_size} bytes>"
        else:
            content = path.read_text(errors="replace")
        result.append((rel, content, mode))
    return result


def reset() -> None:
    if BOX.exists():
        for path in BOX.rglob("*"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(0o755)
        shutil.rmtree(BOX)


def play(binary: str, seed: Seed, steps: list[list[str]]):
    reset()
    for path in (PROJECT / "sub", HOME, BIN):
        path.mkdir(parents=True)
    modes = []
    for rel, (text, mode) in seed.items():
        path = BOX / rel
        if text is None:
            path.mkdir(parents=True, exist_ok=True)
            modes.append((path, mode))
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        if text.startswith("@"):
            path.symlink_to(text[1:])
        elif text.startswith("#") and text.endswith("MB"):
            with open(path, "wb") as out:
                out.truncate(int(text[1:-2]) * 1024 * 1024)
        else:
            path.write_text(text)
        path.chmod(mode)
    for path, mode in modes:
        path.chmod(mode)
    outputs = [run(binary, step) for step in steps]
    return outputs, tree()


def main() -> int:
    differ = total = 0
    for name, seed, steps in SCENARIOS:
        py_out, py_tree = play(PY, seed, steps)
        rs_out, rs_tree = play(RS, seed, steps)
        for step, py, rs in zip(steps, py_out, rs_out):
            total += 1
            if "Traceback" in py[2]:
                differ += 1
                print(f"  [{name}] {' '.join(step)}: Python crashed\n{py[2]}")
            usage = py[0] == 2 and rs[0] == 2 and "usage:" in py[2]
            if py[:2] != rs[:2] or (not usage and py[2] != rs[2]):
                differ += 1
                print(f"  [{name}] {' '.join(step)}\n     py: {py}\n     rs: {rs}")
        if py_tree != rs_tree:
            differ += 1
            py_only = [e for e in py_tree if e not in rs_tree]
            rs_only = [e for e in rs_tree if e not in py_tree]
            print(f"  [{name}] files\n     py: {py_only}\n     rs: {rs_only}")
    reset()
    print(f"doctor: {len(SCENARIOS)} scenarios, {total} steps, {differ} differ")
    return 1 if differ else 0


if __name__ == "__main__":
    sys.exit(main())
