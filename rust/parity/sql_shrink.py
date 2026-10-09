#!/usr/bin/env python3
"""Shrink an unsafe SQL fuzz case (Rust verified, Python not) to a minimum.

Usage: sql_shrink.py FN 'OPTS_JSON' 'SQL'
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from sql_compare import EXAMPLE, build, classify, python_result  # noqa: E402


def unsafe(fn: str, opts: dict, sql: str) -> bool:
    record = {"fn": fn, "sql": sql, "opts": opts}
    proc = subprocess.run(
        [str(EXAMPLE)],
        input=json.dumps(record).encode(),
        capture_output=True,
        check=True,
    )
    rs = json.loads(proc.stdout)["rs"]
    record["result"] = python_result(record)
    return classify(record, rs) == "unsafe"


def main() -> int:
    fn, opts, sql = sys.argv[1], json.loads(sys.argv[2]), sys.argv[3]
    build()
    if not unsafe(fn, opts, sql):
        print("not unsafe")
        return 1
    changed = True
    while changed:
        changed = False
        for size in (24, 12, 6, 3, 2, 1):
            i = 0
            while i < len(sql):
                candidate = sql[:i] + sql[i + size :]
                if candidate.strip() and unsafe(fn, opts, candidate):
                    sql = candidate
                    changed = True
                else:
                    i += 1
    print(repr(sql))
    return 0


if __name__ == "__main__":
    sys.exit(main())
