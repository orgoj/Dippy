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

# MCP, web and file-tool payloads, run with TOOL_CONFIG.
TOOL_CONFIG = f"""\
allow-mcp mcp__fake__*
deny-mcp mcp__fake__drop "no drop"
ask-mcp mcp__fake__write*
after-mcp mcp__fake__get "got it"
allow-web *rust*
deny-web *secret* "no secrets"
ask-web [ci] *build*
after-web *rust* "read docs"
allow-edit {WORK}/out/**
deny-edit {WORK}/out/keep "keep it"
ask-edit [!ci] {WORK}/out/ci/*
allow-read {WORK}/**
ask-read {WORK}/private/*
deny-read ~/.ssh/**
"""

TOOL_PAYLOADS = [
    {"tool_name": "mcp__fake__get"},
    {"tool_name": "mcp__fake__drop"},
    {"tool_name": "mcp__fake__write_file"},
    {"tool_name": "mcp__other__x"},
    {"tool_name": "mcp__other__x", "permission_mode": "bypassPermissions"},
    {"tool_name": "mcp__other__x", "permission_mode": "acceptEdits"},
    {"hook_event_name": "PostToolUse", "tool_name": "mcp__fake__get"},
    {"hook_event_name": "PostToolUse", "tool_name": "mcp__fake__drop"},
    {"tool_name": "WebSearch", "tool_input": {"query": "rust book"}},
    {"tool_name": "WebSearch", "tool_input": {"query": "a secret"}},
    {"tool_name": "WebSearch", "tool_input": {"query": "build farm"}},
    {"tool_name": "WebSearch", "tool_input": {"query": "cats"}},
    {"tool_name": "WebFetch", "tool_input": {"url": "https://x/rust"}},
    {"tool_name": "google_web_search", "tool_input": {"q": "secret"}},
    {"tool_name": "web_fetch", "tool_input": {"url": "https://x/y"}},
    {"tool_name": "WebSearch", "tool_input": {}},
    {
        "tool_name": "WebSearch",
        "tool_input": {"query": "x"},
        "permission_mode": "dontAsk",
    },
    {
        "hook_event_name": "PostToolUse",
        "tool_name": "WebSearch",
        "tool_input": {"query": "rust"},
    },
    {
        "hook_event_name": "PostToolUse",
        "tool_name": "WebSearch",
        "tool_input": {"query": "cats"},
    },
    {"tool_name": "Write", "tool_input": {"file_path": f"{WORK}/out/a.txt"}},
    {"tool_name": "Edit", "tool_input": {"file_path": f"{WORK}/out/keep"}},
    {"tool_name": "MultiEdit", "tool_input": {"file_path": f"{WORK}/out/ci/x"}},
    {"tool_name": "Write", "tool_input": {"file_path": "out/rel.txt"}},
    {"tool_name": "Write", "tool_input": {"file_path": f"{WORK}/src/a.py"}},
    {"tool_name": "Write", "tool_input": {"file_path": f"{WORK}/outx/a"}},
    {"tool_name": "Write", "tool_input": {"file_path": f"{WORK}/out/../../etc/passwd"}},
    {"tool_name": "Read", "tool_input": {"file_path": f"{WORK}/src/a.py"}},
    {"tool_name": "Read", "tool_input": {"file_path": f"{WORK}/private/k"}},
    {"tool_name": "Read", "tool_input": {"file_path": "~/.ssh/id_rsa"}},
    {"tool_name": "Read", "tool_input": {"file_path": "/etc/hosts"}},
    {"tool_name": "Grep", "tool_input": {"path": f"{WORK}/src"}},
    {"tool_name": "Glob", "tool_input": {"path": "/elsewhere"}},
    {"tool_name": "LS", "tool_input": {"path": f"{WORK}"}},
    {"tool_name": "write_file", "tool_input": {"filepath": f"{WORK}/out/k"}},
    {"tool_name": "read", "tool_input": {"path": f"{WORK}/private/k"}},
    {
        "tool_name": "Write",
        "tool_input": {"file_path": f"{WORK}/out/keep"},
        "permission_mode": "acceptEdits",
    },
    {
        "tool_name": "read_many_files",
        "tool_input": {"paths": [f"{WORK}/a", f"{WORK}/b"]},
    },
    {
        "tool_name": "read_many_files",
        "tool_input": {"paths": [f"{WORK}/a", f"{WORK}/private/k"]},
    },
    {"tool_name": "read_many_files", "tool_input": {"paths": ["/x/a"]}},
    {
        "tool_name": "Edit",
        "tool_input": {"paths": [f"{WORK}/out/a", f"{WORK}/out/keep"]},
    },
    {"tool_name": "Read", "tool_input": {"paths": []}},
    {
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": f"{WORK}/out/a"},
    },
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
    config_path_for(TOOL_CONFIG).write_text(TOOL_CONFIG, encoding="utf-8")
    for payload in TOOL_PAYLOADS:
        payload = {"hook_event_name": "PreToolUse", "tool_input": {}, **payload}
        jobs.append((dict(payload, cwd=str(WORK)), TOOL_CONFIG))

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
    tool_results = results[len(jobs) - len(TOOL_PAYLOADS) :]
    tool_diff = [
        (payload, py, rs)
        for payload, (py, rs) in zip(TOOL_PAYLOADS, tool_results)
        if py != rs
    ]
    print(f"tool payloads: {len(tool_diff)} of {len(TOOL_PAYLOADS)} not identical")
    for payload, py, rs in tool_diff:
        print("  ", json.dumps(payload)[:120])
        print("     py:", py[:160])
        print("     rs:", rs[:160])
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
