#!/usr/bin/env python3
"""Build rust/parity/corpus.jsonl with Python Dippy as the oracle.

Steps:
1. ``--harvest``: run the Python suite with ``collect_plugin`` to harvest
   command strings and inline configs (written to a scratch directory).
2. Merge harvested cases with ``handwritten.jsonl``, deduplicate and drop
   configs that would execute programs or write files, and cases naming
   pytest ``tmp_path`` files.
3. Run ``dippy --cmd CMD --json --cwd CWD [--config FILE]`` semantics for every
   case in a child process with an empty temporary ``HOME`` and record the
   expected decision.

The child process calls ``dippy.dippy.parse_cli_args`` and ``cli_mode`` in
process (one interpreter for all cases) so the oracle runs the same code path
as the ``dippy`` executable without paying start-up cost per case.

Usage: rust/parity/build_corpus.py [--harvest] [--harvest-dir DIR]
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
CORPUS = HERE / "corpus.jsonl"
HANDWRITTEN = HERE / "handwritten.jsonl"
PARSER_CASES = HERE / "parser_cases.jsonl"
FUZZ_CASES = HERE / "fuzz_cases.jsonl"
sys.path.insert(0, str(HERE))

from parity_env import (  # noqa: E402
    CWD_PLACEHOLDER,
    HOME,
    ROOT,
    child_env,
    prepare_root,
)

# Settings that run programs, write files or depend on the live machine.
UNSAFE_CONFIG_WORDS = (
    "notifier",
    "askpass",
    "set log",
    "log-full",
    "log-rotate",
    "audit",
)

# Pytest tmp_path files exist only while the harvest runs, so decisions on
# commands or configs naming them are not reproducible.
TRANSIENT_PATH = "/pytest-of-"


def harvest(out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    for old in out_dir.glob("harvest-*.jsonl"):
        old.unlink()
    env = dict(os.environ)
    env["DIPPY_PARITY_OUT"] = str(out_dir)
    # Tests that embed Path.home() then produce the same text on every machine.
    HOME.mkdir(parents=True, exist_ok=True)
    env["HOME"] = str(HOME)
    env["PYTHONPATH"] = str(HERE)
    subprocess.run(
        [
            str(REPO / ".venv-3.12" / "bin" / "python"),
            "-m",
            "pytest",
            "-q",
            "-n",
            "auto",
            "-p",
            "collect_plugin",
            "-p",
            "no:cacheprovider",
        ],
        cwd=REPO,
        env=env,
        check=False,
    )


def load_cases(harvest_dir: Path) -> list[dict]:
    cases: dict[tuple[str, str], dict] = {}

    def add(cmd: str, config: str, src: str) -> None:
        if not isinstance(cmd, str) or not cmd.strip() or "\x00" in cmd:
            return
        if any(word in config for word in UNSAFE_CONFIG_WORDS):
            return
        if TRANSIENT_PATH in cmd or TRANSIENT_PATH in config:
            return
        key = (cmd, config)
        if key not in cases:
            cases[key] = {"cmd": cmd, "config": config, "src": src}

    for path in sorted(glob.glob(str(harvest_dir / "harvest-*.jsonl"))):
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                record = json.loads(line)
                add(record["cmd"], record.get("config", ""), record["src"])
    for path, src in (
        (HANDWRITTEN, "handwritten"),
        (PARSER_CASES, "parser"),
        (FUZZ_CASES, "fuzz"),
    ):
        if not path.exists():
            continue
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                if line.strip():
                    record = json.loads(line)
                    add(record["cmd"], record.get("config", ""), src)
    return sorted(cases.values(), key=lambda c: (c["config"], c["cmd"]))


def run_oracle(cases: list[dict]) -> list[dict]:
    prepare_root(cases)
    with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as fh:
        for case in cases:
            fh.write(json.dumps(case) + "\n")
        input_path = fh.name
    try:
        proc = subprocess.run(
            [
                str(REPO / ".venv-3.12" / "bin" / "python"),
                str(HERE / "oracle.py"),
                input_path,
            ],
            cwd=ROOT,
            env=child_env(),
            capture_output=True,
            text=True,
            check=True,
        )
    finally:
        os.unlink(input_path)
    results = [json.loads(line) for line in proc.stdout.splitlines() if line]
    if len(results) != len(cases):
        raise SystemExit(f"oracle returned {len(results)} of {len(cases)} results")
    return results


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--harvest", action="store_true")
    parser.add_argument(
        "--harvest-dir",
        default=os.environ.get("DIPPY_PARITY_HARVEST", str(ROOT / "harvest")),
    )
    args = parser.parse_args()
    harvest_dir = Path(args.harvest_dir)
    if args.harvest:
        harvest(harvest_dir)
    cases = load_cases(harvest_dir)
    results = run_oracle(cases)
    with open(CORPUS, "w", encoding="utf-8") as fh:
        for case, result in zip(cases, results):
            record = {
                "cmd": case["cmd"],
                "cwd": CWD_PLACEHOLDER,
                "config": case["config"],
                "src": case["src"],
                "expected": result["decision"],
                "reason": result["reason"],
            }
            fh.write(json.dumps(record, ensure_ascii=False) + "\n")
    counts: dict[str, int] = {}
    for result in results:
        counts[result["decision"]] = counts.get(result["decision"], 0) + 1
    print(f"wrote {len(results)} cases to {CORPUS}: {counts}")


if __name__ == "__main__":
    main()
