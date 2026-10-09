#!/usr/bin/env python3
"""Compare the Python and Rust SQL checkers on recorded calls.

Input: JSON lines from ``sql_collect.py`` (``fn``, ``sql``, ``opts``); the
Python verdict is recomputed here. The Rust side is the ``sql_compare`` example of
``dippy-rs``. Results are compared per function:

* is_readonly_sql: Rust ``true`` where Python is not ``True`` is UNSAFE;
  other differences are accepted (Rust asks).
* duckdb_writes_only_main: Rust ``true`` where Python is not ``True`` is UNSAFE.
* duckdb_copy_export_target: a Rust target that differs from Python's is UNSAFE.
* split_sql_statements: must match exactly.

Usage: sql_compare.py FILE.jsonl [--show N]
Exit status 1 when anything is unsafe or a split differs.
"""

from __future__ import annotations

import argparse
import collections
import json
import logging
import multiprocessing
import subprocess
import sys
import os
import signal
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
os.environ["HOME"] = tempfile.mkdtemp(prefix="dippy-sql-home-")
sys.path.insert(0, str(HERE.parent.parent / "src"))

import dippy.core.sql as sql_mod  # noqa: E402

logging.disable(logging.CRITICAL)

RUST = HERE.parent
EXAMPLE = RUST / "target" / "release" / "examples" / "sql_compare"


def build() -> None:
    subprocess.run(
        ["cargo", "build", "--release", "--quiet", "--example", "sql_compare"],
        cwd=RUST,
        check=True,
    )


def rust_results(records: list[dict]) -> list:
    payload = "\n".join(
        json.dumps({"fn": r["fn"], "sql": r["sql"], "opts": r["opts"]}) for r in records
    )
    proc = subprocess.run(
        [str(EXAMPLE)],
        input=payload.encode(),
        capture_output=True,
        check=True,
    )
    lines = proc.stdout.decode().splitlines()
    assert len(lines) == len(records), (len(lines), len(records))
    return [json.loads(line)["rs"] for line in lines]


def python_result(record: dict):
    """Recompute the Python verdict (recorded ones may come from patched tests)."""
    opts = dict(record["opts"])
    for key in ("extra_readonly", "extra_write"):
        if key in opts:
            opts[key] = frozenset(opts[key])
    signal.setitimer(signal.ITIMER_REAL, 1.0)
    try:
        return getattr(sql_mod, record["fn"])(record["sql"], **opts)
    except Exception as exc:  # noqa: BLE001 - a crash or hang is "not verified"
        return f"error: {type(exc).__name__}"
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)


class Timeout(Exception):
    pass


def _on_alarm(signum, frame):
    raise Timeout


signal.signal(signal.SIGALRM, _on_alarm)


def classify(record: dict, rs) -> str:
    """Return 'same', 'unsafe', 'split' or 'safe'."""
    py = record["result"]
    fn = record["fn"]
    if py == rs:
        return "same"
    if fn == "split_sql_statements":
        return "split"
    if fn in ("is_readonly_sql", "duckdb_writes_only_main"):
        return "unsafe" if rs is True else "safe"
    if fn == "duckdb_copy_export_target":
        return "unsafe" if rs is not None else "safe"
    return "unsafe"


def compare(records: list[dict], show: int = 10, label: str = "") -> bool:
    results = rust_results(records)
    with multiprocessing.Pool() as pool:
        verdicts = pool.map(python_result, records, chunksize=16)
    for record, verdict in zip(records, verdicts):
        record["result"] = verdict
    stats: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    examples: dict[tuple[str, str], list] = collections.defaultdict(list)
    for record, rs in zip(records, results):
        kind = classify(record, rs)
        stats[record["fn"]][kind] += 1
        if kind != "same":
            examples[(record["fn"], kind)].append((record, rs))
    ok = True
    for fn in sorted(stats):
        c = stats[fn]
        total = sum(c.values())
        print(
            f"{label}{fn:26} total={total:6} same={c['same']:6} "
            f"safe-diff={c['safe']:5} unsafe={c['unsafe']:3} split-diff={c['split']:3}"
        )
        ok &= not c["unsafe"] and not c["split"]
    for (fn, kind), items in sorted(
        examples.items(), key=lambda kv: kv[0][1] != "unsafe"
    ):
        limit = show
        for record, rs in items[:limit]:
            opts = {
                k: v for k, v in record["opts"].items() if v not in (None, False, [])
            }
            print(
                f"  {kind.upper():6} {fn} py={record['result']!r} rs={rs!r} "
                f"opts={opts} sql={record['sql']!r}"[:400]
            )
    return ok


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+")
    ap.add_argument("--show", type=int, default=10)
    args = ap.parse_args()
    build()
    records = []
    for path in args.files:
        for line in Path(path).read_text().splitlines():
            if line.strip():
                records.append(json.loads(line))
    return 0 if compare(records, args.show) else 1


if __name__ == "__main__":
    sys.exit(main())
