"""Tests for scripts/debug/check-rule.py."""

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "debug" / "check-rule.py"


def _run(tmp_path, *args):
    env = {
        **os.environ,
        "HOME": str(tmp_path / "home"),
        "PYTHONPATH": str(ROOT / "src"),
    }
    result = subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        capture_output=True,
        text=True,
        env=env,
        check=True,
    )
    return result.stdout.splitlines()


def test_path_kinds_with_candidate_file(tmp_path):
    rules = tmp_path / "cand.dippy"
    rules.write_text(
        "allow-read /srv/box/**\n"
        "allow-edit /srv/box/notes/**\n"
        "allow-redirect /srv/box/out/**\n"
        "allow-edit [$AGENT=k] /srv/box/k/**\n"
        "allow-redirect [$AGENT=k] /srv/box/kout/**\n"
    )
    common = ("--config-only", str(rules), "--cwd", str(tmp_path))
    assert _run(tmp_path, "read", *common, "/srv/box/a", "/srv/other") == [
        "read /srv/box/a -> allow (/srv/box/** @ " + str(rules) + ")",
        "read /srv/other -> no match (default: ask)",
    ]
    lines = _run(tmp_path, "edit", *common, "/srv/box/notes/x", "/srv/box/notes/../y")
    assert lines[0].startswith("edit /srv/box/notes/x -> allow")
    assert lines[1].endswith("no match (default: ask)")
    assert _run(tmp_path, "redirect", *common, "/srv/box/out/f")[0].startswith(
        "redirect /srv/box/out/f -> allow"
    )
    assert "no match" in _run(tmp_path, "edit", *common, "/srv/box/k/f")[0]
    flagged = _run(tmp_path, "edit", *common, "--flag", "$AGENT=k", "/srv/box/k/f")
    assert "-> allow" in flagged[0]
    assert "no match" in _run(tmp_path, "redirect", *common, "/srv/box/kout/f")[0]
    flagged = _run(
        tmp_path, "redirect", *common, "--flag", "$AGENT=k", "/srv/box/kout/f"
    )
    assert "-> allow" in flagged[0]


def test_web_and_mcp_kinds(tmp_path):
    rules = tmp_path / "cand.dippy"
    rules.write_text("allow-web docs *\nallow-mcp mcp__zorp__get_*\n")
    common = ("--config-only", str(rules), "--cwd", str(tmp_path))
    assert "-> allow" in _run(tmp_path, "web", *common, "docs python")[0]
    assert "no match" in _run(tmp_path, "web", *common, "other")[0]
    assert "-> allow" in _run(tmp_path, "mcp", *common, "mcp__zorp__get_x")[0]
    assert "no match" in _run(tmp_path, "mcp", *common, "mcp__zorp__put_x")[0]
