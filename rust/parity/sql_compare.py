#!/usr/bin/env python3
"""Compare the Python and Rust SQL checkers on recorded calls.

Input: JSON lines from ``sql_collect.py`` (``fn``, ``sql``, ``opts``); the
Python verdict is recomputed here. The Rust side is the ``sql_compare`` example of
``dippy-rs``. Results are compared per function:

* is_readonly_sql, duckdb_writes_only_main: Rust ``true`` where Python is not
  ``True`` is UNSAFE when Python saw a real rejection (``_unverified_operation``
  found a write, unknown function or user-defined type;
  ``_main_table_target`` found another database). Otherwise it is WIDER, an
  intended divergence: sqlparser verifies read-only syntax and main writes
  that SQLGlot cannot parse or regenerate faithfully (or Python crashed).
  WIDER examples are listed for review. Rust asking where Python allows is
  accepted.
* duckdb_copy_export_target: a Rust target that differs from Python's is
  UNSAFE; a Rust target where Python has none is WIDER unless Python saw a
  real rejection.
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
import re
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


# SQLGlot-based checks whose result is a real rejection (a write or unknown
# node, a target outside the main database). Every other Python rejection
# that Rust does not share is SQLGlot failing to parse or regenerate the SQL.
# An anonymous function whose name is not in the SQL comes from SQLGlot
# regenerating a known function (e.g. `len(x, '')` as `TYPEOF`), not a
# rejection.
_REJECTIONS: list[str] = []
_CURRENT_SQL = [""]


def _real_operation(result: bool, node, *_args) -> bool:
    if not result:
        return False
    if isinstance(node, sql_mod.exp.Anonymous):
        pattern = rf"(?<!\w){re.escape(node.name)}(?!\w)"
        return re.search(pattern, _CURRENT_SQL[0], re.IGNORECASE) is not None
    return True


def _record_rejection(name: str, is_rejection) -> None:
    original = getattr(sql_mod, name)

    def wrapper(*args, **kwargs):
        result = original(*args, **kwargs)
        if is_rejection(result, *args):
            _REJECTIONS.append(name)
        return result

    setattr(sql_mod, name, wrapper)


_record_rejection("_unverified_operation", _real_operation)
_record_rejection("_main_table_target", lambda result, *_args: result is False)


def python_result(record: dict):
    """Recompute the Python verdict (recorded ones may come from patched
    tests) and whether Python saw a real rejection."""
    opts = dict(record["opts"])
    for key in ("extra_readonly", "extra_write"):
        if key in opts:
            opts[key] = frozenset(opts[key])
    _REJECTIONS.clear()
    _CURRENT_SQL[0] = record["sql"]
    signal.setitimer(signal.ITIMER_REAL, 1.0)
    try:
        verdict = getattr(sql_mod, record["fn"])(record["sql"], **opts)
    except Exception as exc:  # noqa: BLE001 - a crash or hang is "not verified"
        verdict = f"error: {type(exc).__name__}"
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
    return verdict, bool(_REJECTIONS)


class Timeout(Exception):
    pass


def _on_alarm(signum, frame):
    raise Timeout


signal.signal(signal.SIGALRM, _on_alarm)


def classify(record: dict, rs) -> str:
    """Return 'same', 'unsafe', 'wider', 'split' or 'safe'."""
    py = record["result"]
    fn = record["fn"]
    if py == rs:
        return "same"
    if fn == "split_sql_statements":
        return "split"
    accepted = rs is True if fn != "duckdb_copy_export_target" else rs is not None
    if not accepted:
        return "safe"
    if record.get("rejected"):
        return "unsafe"
    if (
        fn == "duckdb_copy_export_target"
        and isinstance(py, str)
        and not py.startswith("error")
    ):
        return "unsafe"
    return "wider"


def compare(records: list[dict], show: int = 10, label: str = "") -> bool:
    results = rust_results(records)
    with multiprocessing.Pool() as pool:
        verdicts = pool.map(python_result, records, chunksize=16)
    for record, (verdict, rejected) in zip(records, verdicts):
        record["result"] = verdict
        record["rejected"] = rejected
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
            f"safe-diff={c['safe']:5} wider={c['wider']:5} "
            f"unsafe={c['unsafe']:3} split-diff={c['split']:3}"
        )
        ok &= not c["unsafe"] and not c["split"]
    for (fn, kind), items in sorted(
        examples.items(),
        key=lambda kv: ("unsafe", "wider", "split", "safe").index(kv[0][1]),
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
