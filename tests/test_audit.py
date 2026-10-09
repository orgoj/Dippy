"""Tests for the read-only `dippy audit` log query subcommand."""

import json
from argparse import Namespace

import pytest

from conftest import is_approved, needs_confirmation
from dippy.audit import query_audit_log
from dippy.dippy import handle_subcommand

ENTRIES = {
    "audit-2026-10-01.log": [
        {
            "decision": "ask",
            "cmd": "mkdir",
            "command": "mkdir -p /abs/tmp",
            "cwd": "/home/u/wiki",
            "policy_cwd": "/home/u/wiki",
            "agent": "agy",
            "ts": "2026-10-01T08:00:00+00:00",
        },
    ],
    "audit-2026-10-02.log": [
        {
            "decision": "allow",
            "cmd": "ls",
            "command": "ls",
            "cwd": "/home/u/wiki",
            "agent": "agy",
            "ts": "2026-10-02T08:00:00+00:00",
        },
        {
            "decision": "pass",
            "message": "no matching rule",
            "tool": "view_file",
            "file_path": "/home/u/.hcom/agents/k/SOUL.md",
            "cwd": "/home/u/wiki",
            "policy_cwd": "/home/u/wiki",
            "agent": "agy",
            "ts": "2026-10-02T09:00:00+00:00",
        },
    ],
    "audit.log": [
        {
            "decision": "pass",
            "message": "no matching rule",
            "tool": "view_file",
            "file_path": "/home/u/.hcom/agents/k/SOUL.md",
            "cwd": "/home/u/wiki",
            "policy_cwd": "/home/u/wiki",
            "agent": "agy",
            "ts": "2026-10-03T09:00:00+00:00",
        },
        {
            "decision": "ask",
            "cmd": "curl",
            "command": "curl -X POST http://x",
            "cwd": "/home/u/proj",
            "agent": "claude",
            "ts": "2026-10-03T10:00:00+00:00",
        },
    ],
}


@pytest.fixture
def log_path(tmp_path):
    for name, entries in ENTRIES.items():
        lines = [json.dumps(e) for e in entries]
        if name == "audit.log":
            lines.append("not json")
        (tmp_path / name).write_text("\n".join(lines) + "\n")
    (tmp_path / "unrelated.log").write_text(json.dumps({"decision": "ask"}) + "\n")
    return tmp_path / "audit.log"


def _rows(lines):
    return [json.loads(line) for line in lines]


def test_reads_rotated_and_current_logs_in_time_order(log_path):
    rows = _rows(query_audit_log(log_path))
    assert [r["ts"][:10] for r in rows] == [
        "2026-10-01",
        "2026-10-02",
        "2026-10-02",
        "2026-10-03",
        "2026-10-03",
    ]


def test_filters_combine_with_and_and_decisions_with_or(log_path):
    rows = _rows(
        query_audit_log(
            log_path,
            decisions=["ask", "pass"],
            agent="agy",
            policy_cwd="/home/u/wiki",
        )
    )
    assert [r["decision"] for r in rows] == ["ask", "pass", "pass"]


def test_not_allow_shortcut_excludes_allow(log_path):
    rows = _rows(query_audit_log(log_path, not_allow=True))
    assert "allow" not in {r["decision"] for r in rows}
    assert len(rows) == 4


def test_cwd_matches_path_prefix_on_component_boundary(log_path):
    assert query_audit_log(log_path, cwd="/home/u/wik") == []
    assert len(query_audit_log(log_path, cwd="/home/u")) == 5


def test_since_until_are_inclusive_dates(log_path):
    rows = _rows(query_audit_log(log_path, since="2026-10-02", until="2026-10-02"))
    assert {r["ts"][:10] for r in rows} == {"2026-10-02"}


def test_tool_and_grep_filters(log_path):
    assert len(query_audit_log(log_path, tool="view_file")) == 2
    rows = _rows(query_audit_log(log_path, grep="POST"))
    assert [r["cmd"] for r in rows] == ["curl"]
    rows = _rows(query_audit_log(log_path, grep="SOUL.md"))
    assert len(rows) == 2


def test_group_by_counts_descending_with_stable_ties(log_path):
    lines = query_audit_log(log_path, group_by=["decision"])
    assert lines == ["2\task", "2\tpass", "1\tallow"]
    lines = query_audit_log(log_path, not_allow=True, group_by=["tool", "file_path"])
    assert lines == ["2\t-\t-", "2\tview_file\t/home/u/.hcom/agents/k/SOUL.md"]


def test_limit_keeps_last_entries_or_top_groups(log_path):
    rows = _rows(query_audit_log(log_path, limit=2))
    assert [r["ts"] for r in rows] == [
        "2026-10-03T09:00:00+00:00",
        "2026-10-03T10:00:00+00:00",
    ]
    lines = query_audit_log(log_path, group_by=["decision"], limit=1)
    assert lines == ["2\task"]


def test_subcommand_uses_configured_log(log_path, tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    config = tmp_path / "only.dippy"
    config.write_text(f"set log {log_path}\n")
    args = Namespace(
        subcommand="audit",
        cwd=str(tmp_path),
        config=None,
        config_only=str(config),
        since=None,
        until=None,
        decision=["ask"],
        not_allow=False,
        audit_agent=None,
        audit_cwd=None,
        policy_cwd=None,
        tool=None,
        grep=None,
        group_by=["cmd"],
        limit=None,
    )
    assert handle_subcommand(args) == 0
    assert capsys.readouterr().out.splitlines() == ["1\tcurl", "1\tmkdir"]


def test_subcommand_without_log_reports_error(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    config = tmp_path / "only.dippy"
    config.write_text("")
    args = Namespace(
        subcommand="audit",
        cwd=str(tmp_path),
        config=None,
        config_only=str(config),
        since=None,
        until=None,
        decision=None,
        not_allow=False,
        audit_agent=None,
        audit_cwd=None,
        policy_cwd=None,
        tool=None,
        grep=None,
        group_by=None,
        limit=None,
    )
    assert handle_subcommand(args) == 1
    assert "set log" in capsys.readouterr().err


CLASSIFY = [
    ("dippy audit", True),
    ("dippy audit --not-allow --group-by cmd", True),
    ("dippy audit --agent agy --policy-cwd ~/wiki --since 2026-10-01", True),
    ("dippy audit --grep 'rm -rf'", True),
    ("dippy audit --not-allow > /etc/passwd", False),
    ("dippy doctor --fix", False),
    ("dippy hooks install claude", False),
    ("dippy config set askpass x", False),
]


def test_relative_cwd_filter_resolves_against_process_cwd(log_path, monkeypatch):
    monkeypatch.chdir("/")
    assert len(query_audit_log(log_path, cwd="home/u/wiki")) == 4


def test_limit_larger_than_matches_keeps_all(log_path):
    assert len(query_audit_log(log_path, limit=7)) == 5
    assert query_audit_log(log_path, limit=0) == []


def test_negative_limit_is_rejected(log_path):
    with pytest.raises(ValueError, match="limit"):
        query_audit_log(log_path, limit=-1)


def test_other_dippy_subcommands_keep_subcommand_in_reason(check_single):
    _, reason = check_single("dippy config set askpass x")
    assert "dippy config" in reason


@pytest.mark.parametrize("command,expected", CLASSIFY)
def test_audit_is_read_only_builtin(check, command, expected):
    result = check(command)
    if expected:
        assert is_approved(result), command
    else:
        assert needs_confirmation(result), command
