#!/usr/bin/env python3
"""Compare Claude Code hook output (``--claude``) of Python Dippy and dippy-rs.

Feeds identical PreToolUse/PostToolUse payloads for a deterministic sample of
corpus commands, plus hand-written payloads for other events, to both
implementations with an empty HOME (configs via DIPPY_CONFIG) and compares
the emitted JSON exactly and the permission decision separately.

Usage: hook_compare.py [--sample N]
"""

from __future__ import annotations

import argparse
import json
import os
import random
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))

from parity_env import ROOT, WORK, child_env, config_path_for, prepare_root  # noqa: E402

PY = [str(REPO / ".venv-3.12" / "bin" / "dippy"), "--claude"]
RS = [str(HERE.parent / "target" / "release" / "dippy-rs"), "--claude"]

EXTRA = [
    {"hook_event_name": "Stop"},
    {"hook_event_name": "Notification", "notification_type": "idle_prompt"},
    {
        "tool_name": "Bash",
        "tool_input": {"command": "rm -rf x"},
        "permission_mode": "bypassPermissions",
    },
    {
        "tool_name": "Bash",
        "tool_input": {"command": "ls"},
        "permission_mode": "dontAsk",
    },
    {"tool_name": "UnknownTool", "tool_input": {}},
    {"tool_name": "Read", "tool_input": {}},
    {"tool_name": "Bash", "tool_input": {"cmd": "ls -la"}},
    {"tool_name": "Bash", "tool_input": {}},
]


def decision(output: str) -> str:
    try:
        data = json.loads(output) if output.strip() else {}
    except json.JSONDecodeError:
        return "invalid"
    return data.get("hookSpecificOutput", {}).get("permissionDecision", "none")


def run(cmd: list[str], payload: dict, config: str) -> str:
    env = child_env()
    if config:
        env["DIPPY_CONFIG"] = str(config_path_for(config))
    proc = subprocess.run(
        cmd,
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        env=env,
        cwd=ROOT,
        timeout=60,
    )
    return proc.stdout.strip()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--sample", type=int, default=600)
    args = ap.parse_args()
    cases = [
        json.loads(line) for line in (HERE / "corpus.jsonl").read_text().splitlines()
    ]
    prepare_root(cases)
    random.seed(20261009)
    sample = random.sample(cases, min(args.sample, len(cases)))
    jobs = []
    for case in sample:
        payload = {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": case["cmd"]},
            "cwd": str(WORK),
        }
        jobs.append((payload, case["config"]))
    for case in [c for c in cases if c["config"] and "after " in c["config"]][:40]:
        payload = {
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": case["cmd"]},
            "cwd": str(WORK),
        }
        jobs.append((payload, case["config"]))
    jobs += [(dict(p, cwd=str(WORK)), "") for p in EXTRA]

    def both(job):
        payload, config = job
        return run(PY, payload, config), run(RS, payload, config)

    with ThreadPoolExecutor(os.cpu_count() or 4) as pool:
        results = list(pool.map(both, jobs))

    exact = sum(1 for py, rs in results if py == rs)
    same_decision = sum(1 for py, rs in results if decision(py) == decision(rs))
    unsafe = [
        (job, py, rs)
        for job, (py, rs) in zip(jobs, results)
        if decision(rs) == "allow" and decision(py) != "allow"
    ]
    print(
        f"{len(jobs)} payloads: {exact} identical output, {same_decision} same decision, {len(unsafe)} unsafe"
    )
    shown = 0
    for job, (py, rs) in zip(jobs, results):
        if decision(py) != decision(rs) and shown < 15:
            shown += 1
            print("  ", json.dumps(job[0])[:120])
            print("     py:", py[:160])
            print("     rs:", rs[:160])
    return 1 if unsafe else 0


if __name__ == "__main__":
    sys.exit(main())
