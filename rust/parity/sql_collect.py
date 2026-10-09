#!/usr/bin/env python3
"""Collect SQL checker calls and their Python verdicts for the Rust port.

Wraps the public functions of ``dippy.core.sql``, then runs the SQL-related
test modules and every corpus/case command through the SQL CLI handlers.
Each distinct call is written as one JSON line:

  {"fn": "is_readonly_sql", "sql": ..., "opts": {...}, "result": ...}

``sql_compare`` (a Rust example) replays the file and reports agreement.
Nothing is executed against a database.

Usage:
  sql_collect.py OUT.jsonl [--cases FILE ...]
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent

os.environ["HOME"] = tempfile.mkdtemp(prefix="dippy-sql-home-")
sys.path.insert(0, str(REPO / "src"))

import dippy.core.sql as sql_mod  # noqa: E402

RECORDS: dict[str, dict] = {}


def _record(fn: str, sql: str, opts: dict, result) -> None:
    item = {"fn": fn, "sql": sql, "opts": opts, "result": result}
    RECORDS.setdefault(json.dumps(item, sort_keys=True), item)


def _wrap_readonly(orig):
    def wrapper(sql, **kwargs):
        result = orig(sql, **kwargs)
        opts = {
            key: sorted(value) if isinstance(value, frozenset) else value
            for key, value in kwargs.items()
        }
        _record("is_readonly_sql", sql, opts, result)
        return result

    return wrapper


def _wrap_simple(name, orig):
    def wrapper(sql, **kwargs):
        result = orig(sql, **kwargs)
        _record(name, sql, kwargs, result)
        return result

    return wrapper


sql_mod.is_readonly_sql = _wrap_readonly(sql_mod.is_readonly_sql)
for _name in (
    "split_sql_statements",
    "duckdb_writes_only_main",
    "duckdb_copy_export_target",
):
    setattr(sql_mod, _name, _wrap_simple(_name, getattr(sql_mod, _name)))


def run_tests() -> None:
    import pytest

    tests = [
        "tests/core/test_sql.py",
        "tests/core/test_sql_types.py",
        "tests/cli/test_psql.py",
        "tests/cli/test_mysql.py",
        "tests/cli/test_sqlite3.py",
        "tests/cli/test_duckdb.py",
        "tests/cli/test_sqlcmd.py",
        "tests/cli/test_aws.py",
        "tests/test_sql_validation.py",
    ]
    paths = [str(REPO / t) for t in tests if (REPO / t).exists()]
    pytest.main(["-q", "-p", "no:cacheprovider", "--no-header", *paths])


def run_corpus(case_files: list[str]) -> None:
    sys.path.insert(0, str(HERE))
    from handler_compare import contexts, py_result  # noqa: PLC0415
    from dippy.cli import KNOWN_HANDLERS  # noqa: PLC0415

    commands = [
        json.loads(line)["cmd"]
        for line in (HERE / "corpus.jsonl").read_text().splitlines()
    ]
    for path in case_files:
        for line in Path(path).read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                commands.append(line)
    for command in dict.fromkeys(commands):
        for tokens, raw, exp in contexts(command):
            if KNOWN_HANDLERS.get(tokens[0]) in {
                "psql",
                "mysql",
                "sqlite3",
                "duckdb",
                "sqlcmd",
                "aws",
            }:
                py_result(tokens, raw, exp)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--cases", action="append", default=[])
    ap.add_argument("--no-tests", action="store_true")
    args = ap.parse_args()
    if not args.no_tests:
        run_tests()
    run_corpus(args.cases)
    with open(args.out, "w", encoding="utf-8") as fh:
        for item in RECORDS.values():
            fh.write(json.dumps(item) + "\n")
    counts: dict[str, int] = {}
    for item in RECORDS.values():
        counts[item["fn"]] = counts.get(item["fn"], 0) + 1
    print(json.dumps(counts), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
