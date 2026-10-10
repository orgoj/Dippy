#!/usr/bin/env python3
"""Compare `dippy hooks` of Python Dippy and dippy-rs.

Each scenario runs once per implementation in the same fresh sandbox: an
empty HOME and a project directory as cwd, optionally seeded with files (or
directories, content None) with a mode and mtime. Every step compares exit
code and stdout, and stderr unless the step is a usage error (exit 2). Backup
timestamps and parser error details (serde vs json/tomllib wording) are
normalized. After the scenario the whole sandbox tree (paths, contents,
modes) must be identical.

Python divergences fixed in dippy-rs (a command containing `/dippy` counted
as a Dippy hook, a double space in the upgrade hint without --global,
tracebacks on malformed hooks) are covered by Rust unit tests, not here.

Usage: hooks_compare.py
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

BOX = ROOT / "hooks"
HOME = BOX / "home"
PROJECT = BOX / "project"
OTHER = BOX / "other"

STAMP = re.compile(r"\.dippy-backup-\d{8}_\d{6}")
PARSE_ERROR = re.compile(
    r"^(Error: (?:Invalid JSON in|Invalid Codex TOML at|Could not read) \S+?): .*$",
    re.M,
)

AGENTS = ["claude", "gemini", "agy", "cursor", "windsurf", "codex"]

Seed = dict[str, tuple[str | None, int, int | None]]


def h(*args: str) -> list[str]:
    return ["hooks", *args]


def f(text: str | None, mode: int = 0o644, mtime: int | None = None):
    return (text, mode, mtime)


LIST = [h("list"), h("list", "--verbose"), h("list", "--json")]

FOREIGN_CLAUDE = """{
  "model": "opus \\u00e9",
  "ratio": 1.5,
  "big": 1e20,
  "hooks": {
    "PreToolUse": [
      {"matcher": "Bash", "hooks": [{"type": "command", "command": "memorix hook"}]},
      {"matcher": "Read", "hooks": [{"type": "command", "command": "dippy --claude"},
                                     {"type": "command", "command": "other"}]}
    ],
    "SessionStart": [{"hooks": [{"type": "command", "command": "memorix start"}]}],
    "Stop": [{"hooks": []}]
  },
  "permissions": {"allow": ["Bash(ls:*)"], "deny": []}
}
"""

LEGACY_CLAUDE = """{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [
  {"type": "command", "command": "/usr/local/bin/dippy-hook --claude"}]}]}}"""

CODEX_INSTALLED = """{"hooks": {
  "PreToolUse": [{"matcher": "^Bash$", "hooks": [{"type": "command", "command": "dippy --codex"}]}],
  "PermissionRequest": [{"matcher": "^Bash$", "hooks": [{"type": "command", "command": "dippy --codex"}]}],
  "PostToolUse": [{"matcher": "^Bash$", "hooks": [{"type": "command", "command": "dippy --codex"}]}]
}}"""

CODEX_STEPS = [
    h("list"),
    h("install", "codex", "--dry-run"),
    h("install", "codex"),
    h("list", "--verbose"),
]

T0 = 1_700_000_000

SCENARIOS: list[tuple[str, Seed, list[list[str]]]] = [
    (
        "empty project, claude lifecycle",
        {},
        [
            *LIST,
            h("list", "--quiet"),
            h(),
            h("install", "claude", "--dry-run"),
            h("install", "claude"),
            h("install", "claude"),
            h("install", "claude", "--all", "--dry-run"),
            h("install", "claude", "--all"),
            h("install", "claude", "--no-backup"),
            h("install", "claude", "--force", "--no-backup"),
            h("list", "--verbose"),
            h("uninstall", "claude", "--dry-run"),
            h("uninstall", "claude"),
            h("uninstall", "claude"),
            h("list", "--json"),
        ],
    ),
    *[
        (
            f"{agent} lifecycle",
            {},
            [
                h("install", agent, "--dry-run"),
                h("install", agent),
                h("install", agent),
                h("install", agent, "--all", "--dry-run"),
                h("install", agent, "--all"),
                *LIST,
                h("uninstall", agent, "--dry-run"),
                h("uninstall", agent),
                h("list", "--json"),
            ],
        )
        for agent in AGENTS[1:]
    ],
    (
        "global without agent directories",
        {},
        [
            h("install", "claude", "--global"),
            h("install", "--all", "--global", "--dry-run"),
            h("install", "--all", "--global"),
            h("uninstall", "gemini", "--global"),
            *LIST,
        ],
    ),
    (
        "global install",
        {"home/.claude": f(None), "home/.codex": f(None)},
        [
            h("install", "claude", "--global"),
            h("install", "codex", "--global", "--all"),
            *LIST,
            h("uninstall", "claude", "--global"),
            h("list"),
        ],
    ),
    (
        "install --all without agent",
        {},
        [
            h("install"),
            h("install", "--all", "--dry-run"),
            h("install", "--all"),
            *LIST,
        ],
    ),
    (
        "foreign hooks kept",
        {"project/.claude/settings.json": f(FOREIGN_CLAUDE)},
        [
            *LIST,
            h("install", "claude", "--dry-run"),
            h("install", "claude", "--all"),
            h("list", "--verbose"),
            h("uninstall", "claude"),
        ],
    ),
    (
        "legacy hook (global)",
        {"home/.claude/settings.json": f(LEGACY_CLAUDE)},
        [
            *LIST,
            h("install", "claude", "--global"),
            h("install", "claude", "--global", "--force", "--dry-run"),
            h("install", "claude", "--global", "--force"),
            h("list", "--json"),
        ],
    ),
    (
        "codex legacy flat run entry",
        {
            "project/.codex/hooks.json": f(
                '{"hooks": {"PreToolUse": [{"run": ["dippy", "--codex"]}],'
                ' "Stop": [{"hooks": [{"command": "notify"}]}]}}'
            )
        },
        [*LIST, h("install", "codex"), h("list", "--json")],
    ),
    (
        "codex toml with profile table",
        {
            "project/.codex/config.toml": f(
                'model = "o3"\n\n[profiles.x]\napproval_policy = "never"\n'
            )
        },
        CODEX_STEPS,
    ),
    (
        "codex toml crlf and comments",
        {
            "project/.codex/config.toml": f(
                "approval_policy = 'never' # c\r\n[features]\r\n"
                "codex_hooks = false # x\r\n[other]\r\nk = 1\r\n"
            )
        },
        CODEX_STEPS,
    ),
    (
        "codex toml without final newline",
        {"project/.codex/config.toml": f('model = "x"')},
        CODEX_STEPS,
    ),
    (
        "codex toml features header only",
        {"project/.codex/config.toml": f("[features]")},
        CODEX_STEPS,
    ),
    (
        "codex toml table first",
        {"project/.codex/config.toml": f("[tui]\nx = 1\n[features]\nother = true\n")},
        CODEX_STEPS,
    ),
    (
        "codex invalid toml",
        {"project/.codex/config.toml": f("model = \n")},
        CODEX_STEPS,
    ),
    (
        "codex already compatible",
        {
            "project/.codex/config.toml": f(
                'approval_policy = "on-request"\n[features]\nhooks = true\n'
            ),
            "project/.codex/hooks.json": f(CODEX_INSTALLED),
        },
        CODEX_STEPS,
    ),
    (
        "codex hooks without feature flag",
        {"project/.codex/hooks.json": f(CODEX_INSTALLED)},
        CODEX_STEPS,
    ),
    (
        "codex global flags in list",
        {
            "home/.codex/config.toml": f("[features]\nhooks = true # on\n"),
            "home/.codex/hooks.json": f(CODEX_INSTALLED),
        },
        LIST,
    ),
    (
        "codex path is a file",
        {"project/.codex": f("not a dir")},
        [h("install", "codex"), h("list")],
    ),
    (
        "invalid json",
        {
            "project/.claude/settings.json": f("{bad"),
            "project/.gemini/settings.json": f("[1, 2"),
            "home/.cursor/hooks.json": f(""),
        },
        [
            *LIST,
            h("install", "claude"),
            h("uninstall", "claude"),
            h("setup-gemini-yolo"),
            h("install", "cursor", "--global"),
        ],
    ),
    (
        "gemini yolo",
        {},
        [
            h("setup-gemini-yolo", "--disable"),
            h("setup-gemini-yolo", "--dry-run"),
            h("setup-gemini-yolo"),
            h("setup-gemini-yolo"),
            h("setup-gemini-yolo", "--disable", "--dry-run"),
            h("setup-gemini-yolo", "--disable"),
            h("setup-gemini-yolo", "--disable"),
            h("setup-gemini-yolo", "--global"),
        ],
    ),
    (
        "gemini yolo policy engine",
        {
            "project/.gemini/settings.json": f(
                '{"policyEngineConfig": {"approvalMode": "default", "x": 1}}'
            ),
            "home/.gemini/settings.json": f(
                '{"approvalMode": "", "policyEngineConfig": {"approvalMode": "yolo"}}'
            ),
        },
        [
            h("setup-gemini-yolo"),
            h("setup-gemini-yolo", "--global"),
            h("setup-gemini-yolo", "--global", "--disable"),
        ],
    ),
    (
        "backup rotation",
        {
            "project/.claude/settings.json": f("{}", 0o600, T0),
            **{
                f"project/.claude/settings.json.dippy-backup-2020010{i}_000000": f(
                    f'{{"n": {i}}}', 0o644, T0 - 1000 * (10 - i)
                )
                for i in range(1, 7)
            },
        },
        [h("install", "claude"), h("install", "gemini")],
    ),
    (
        "codex toml backup",
        {"project/.codex/config.toml": f('model = "x"\n', 0o600, T0)},
        [h("install", "codex", "--dry-run"), h("install", "codex")],
    ),
    (
        "no backup",
        {"project/.claude/settings.json": f('{"x": 1}')},
        [h("install", "claude", "--no-backup")],
    ),
    (
        "cursor upgrade keeps version and other hooks",
        {
            "project/.cursor/hooks.json": f(
                '{"version": 2, "hooks": {"beforeShellExecution": [{"command": "other"}],'
                ' "preToolUse": [{"command": "dippy --cursor"}]}}'
            )
        },
        [*LIST, h("install", "cursor", "--dry-run"), h("install", "cursor")],
    ),
    (
        "agy hooks outside the dippy block",
        {
            "project/.agents/hooks.json": f(
                '{"other": 1, "hooks": {"PreToolUse": [{"hooks": [{"command": "dippy --agy"}]},'
                ' {"hooks": [{"command": "x"}]}], "Stop": []}}'
            )
        },
        [
            *LIST,
            h("install", "agy", "--all"),
            h("list", "--verbose"),
            h("uninstall", "agy"),
        ],
    ),
    (
        "windsurf foreign hooks",
        {
            "project/.windsurf/hooks.json": f(
                '{"hooks": {"beforeShellExecution": [{"command": "dippy --windsurf"},'
                ' {"command": "audit"}]}}'
            )
        },
        [h("install", "windsurf"), h("uninstall", "windsurf", "--dry-run")],
    ),
    (
        "pi extension present",
        {"home/.pi/agent/extensions/dippy-extension.ts": f("// x\n")},
        LIST,
    ),
    (
        "--cwd",
        {"other": f(None)},
        [
            ["--cwd", str(OTHER), *h("install", "cursor")],
            ["--cwd", str(OTHER), *h("list", "--json")],
            ["--cwd", "sub/", *h("install", "gemini", "--dry-run")],
            ["--cwd", "sub", *h("install", "gemini")],
            ["--cwd", "sub", *h("setup-gemini-yolo")],
            ["--cwd", "sub", *h("list")],
        ],
    ),
    (
        "usage errors",
        {},
        [
            h("install", "bogus"),
            h("uninstall"),
            h("uninstall", "pi"),
            h("bogus"),
            h("list", "--nope"),
        ],
    ),
]


def env() -> dict[str, str]:
    home = str(HOME)
    assert home.startswith(str(BOX)), "sandbox HOME must stay inside the box"
    return {
        "HOME": home,
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": "C.UTF-8",
        "DIPPY_TEST_NO_LOG": "1",
    }


def normalize(text: str) -> str:
    return PARSE_ERROR.sub(r"\1: <detail>", STAMP.sub(".dippy-backup-STAMP", text))


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
        mode = oct(path.stat().st_mode & 0o777)
        content = "<dir>" if path.is_dir() else path.read_text(errors="replace")
        result.append((rel, content, mode))
    return result


def play(binary: str, seed: Seed, steps: list[list[str]]):
    if BOX.exists():
        shutil.rmtree(BOX)
    PROJECT.mkdir(parents=True)
    HOME.mkdir()
    for rel, (text, mode, mtime) in seed.items():
        path = BOX / rel
        if text is None:
            path.mkdir(parents=True, exist_ok=True)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(mode)
        if mtime is not None:
            os.utime(path, (mtime, mtime))
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
            usage = py[0] == 2 and rs[0] == 2
            if py[:2] != rs[:2] or (not usage and py[2] != rs[2]):
                differ += 1
                print(f"  [{name}] {' '.join(step)}\n     py: {py}\n     rs: {rs}")
        if py_tree != rs_tree:
            differ += 1
            py_only = [e for e in py_tree if e not in rs_tree]
            rs_only = [e for e in rs_tree if e not in py_tree]
            print(f"  [{name}] files\n     py: {py_only}\n     rs: {rs_only}")
    shutil.rmtree(BOX, ignore_errors=True)
    print(f"hooks: {len(SCENARIOS)} scenarios, {total} steps, {differ} differ")
    return 1 if differ else 0


if __name__ == "__main__":
    sys.exit(main())
