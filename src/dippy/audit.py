"""Read-only queries over the Dippy audit log and its daily rotations."""

from __future__ import annotations

import json
import os
import re
from collections import Counter
from datetime import date, timedelta
from pathlib import Path

_ROTATED = re.compile(r"audit-(\d{4}-\d{2}-\d{2})\.log")
_GREP_FIELDS = ("command", "cmd", "message", "file_path", "tool", "suggestion")


def _parse_date(value: str) -> date:
    try:
        return date.fromisoformat(value)
    except ValueError:
        raise ValueError(f"invalid date (expected YYYY-MM-DD): {value}") from None


def _log_files(log: Path, since: date | None) -> list[Path]:
    """Return rotated logs (oldest first) followed by the current log."""
    rotated = []
    if log.parent.is_dir():
        for path in log.parent.iterdir():
            match = _ROTATED.fullmatch(path.name)
            if not match or path == log:
                continue
            # A rotation named D holds entries written up to the morning of D+1.
            if since and _parse_date(match.group(1)) < since - timedelta(days=1):
                continue
            rotated.append(path)
    rotated.sort()
    return rotated + ([log] if log.is_file() else [])


def _under(value: object, prefix: str) -> bool:
    if not isinstance(value, str):
        return False
    prefix = os.path.abspath(os.path.expanduser(prefix)).rstrip("/")
    return value == prefix or value.startswith(prefix + "/")


def _field(entry: dict, name: str) -> str:
    value = entry.get(name)
    if value is None:
        return "-"
    if isinstance(value, list):
        return ",".join(str(v) for v in value)
    return str(value)


def query_audit_log(
    log: Path,
    *,
    since: str | None = None,
    until: str | None = None,
    decisions: list[str] | None = None,
    not_allow: bool = False,
    agent: str | None = None,
    cwd: str | None = None,
    policy_cwd: str | None = None,
    tool: str | None = None,
    grep: str | None = None,
    group_by: list[str] | None = None,
    limit: int | None = None,
) -> list[str]:
    """Return matching raw JSON lines, or `count<TAB>values` lines when grouped."""
    if limit is not None and limit < 0:
        raise ValueError(f"limit must not be negative: {limit}")
    since_date = _parse_date(since) if since else None
    until_date = _parse_date(until) if until else None

    matches: list[tuple[str, str, dict]] = []
    for path in _log_files(log, since_date):
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.strip()
                try:
                    entry = json.loads(line)
                except ValueError:
                    continue
                if not isinstance(entry, dict):
                    continue
                ts = str(entry.get("ts", ""))
                day = ts[:10]
                if since_date and day < since_date.isoformat():
                    continue
                if until_date and day > until_date.isoformat():
                    continue
                decision = entry.get("decision")
                if decisions and decision not in decisions:
                    continue
                if not_allow and decision == "allow":
                    continue
                if agent and entry.get("agent") != agent:
                    continue
                if cwd and not _under(entry.get("cwd"), cwd):
                    continue
                if policy_cwd and not _under(entry.get("policy_cwd"), policy_cwd):
                    continue
                if tool and entry.get("tool") != tool:
                    continue
                if grep and not any(
                    grep in str(entry.get(name, "")) for name in _GREP_FIELDS
                ):
                    continue
                matches.append((ts, line, entry))

    matches.sort(key=lambda m: m[0])

    if group_by:
        counts = Counter(
            tuple(_field(entry, name) for name in group_by) for _, _, entry in matches
        )
        groups = sorted(counts.items(), key=lambda item: (-item[1], item[0]))
        if limit is not None:
            groups = groups[:limit]
        return [f"{count}\t" + "\t".join(key) for key, count in groups]

    lines = [line for _, line, _ in matches]
    if limit is not None:
        lines = lines[max(0, len(lines) - limit) :]
    return lines
