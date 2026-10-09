"""Run Python Dippy's CLI mode in process for every case in a JSONL file.

Executed by build_corpus.py with an empty HOME. Prints one JSON object per
input line: {"decision": ..., "reason": ..., "exit": ...}.
"""

from __future__ import annotations

import io
import json
import sys
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from parity_env import cli_args  # noqa: E402

from dippy import dippy as dippy_main  # noqa: E402

EXIT_DECISION = {0: "allow", 1: "deny", 2: "ask"}


def run_case(case: dict) -> dict:
    sys.argv = ["dippy", *cli_args(case)]
    out = io.StringIO()
    err = io.StringIO()
    try:
        with redirect_stdout(out), redirect_stderr(err):
            args = dippy_main.parse_cli_args()
            code = dippy_main.cli_mode(args)
    except SystemExit as exc:
        code = exc.code if isinstance(exc.code, int) else 2
    except Exception as exc:  # oracle crash: Dippy itself would fail open? record
        return {"decision": "error", "reason": f"{type(exc).__name__}: {exc}", "exit": -1}
    text = out.getvalue().strip().splitlines()
    try:
        payload = json.loads(text[0]) if text else {}
    except json.JSONDecodeError:
        payload = {}
    decision = payload.get("decision") or EXIT_DECISION.get(code, "ask")
    if EXIT_DECISION.get(code) != decision:
        decision = f"mismatch:{decision}/{code}"
    return {"decision": decision, "reason": payload.get("reason", ""), "exit": code}


def main() -> None:
    with open(sys.argv[1], encoding="utf-8") as fh:
        cases = [json.loads(line) for line in fh if line.strip()]
    for case in cases:
        print(json.dumps(run_case(case)), flush=True)


if __name__ == "__main__":
    main()
