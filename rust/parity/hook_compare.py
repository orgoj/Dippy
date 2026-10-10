#!/usr/bin/env python3
"""Compare hook output of Python Dippy and dippy-rs in every agent mode.

Feeds identical payloads for a deterministic sample of corpus commands, plus
hand-written payloads for tools and events, to both implementations with an
empty HOME (configs via DIPPY_CONFIG) and compares exit code, stdout and
stderr exactly and the permission decision separately. Modes: Claude Code,
Gemini CLI, Codex, Cursor and AGY (askpass is a fake script that exits 0 or
1; the real dialog never opens), plus payloads without a mode flag.

Usage: hook_compare.py [--sample N]
"""

from __future__ import annotations

import argparse
import collections
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

from parity_env import (  # noqa: E402
    CONFIGS,
    ROOT,
    WORK,
    child_env,
    config_path_for,
    prepare_root,
)

PY = str(REPO / ".venv-3.12" / "bin" / "dippy")
RS = str(HERE.parent / "target" / "release" / "dippy-rs")

ASKPASS_ALLOW = CONFIGS / "askpass-allow"
ASKPASS_DENY = CONFIGS / "askpass-deny"

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
allow frob
deny frob delete "no frob"
after frob "frobbed"
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

# Gemini and Codex use Claude-shaped payloads with their own tool and event names.
GEMINI_EXTRA = [
    {"hook_event_name": "AfterAgent"},
    {"hook_event_name": "BeforeTool", "tool_name": "glob_files", "tool_input": {}},
    {"hook_event_name": "BeforeTool", "tool_name": "write_file", "tool_input": {}},
    {
        "hook_event_name": "BeforeTool",
        "tool_name": "write_file",
        "tool_input": {"file_path": "/nowhere/x"},
    },
    {
        "hook_event_name": "BeforeTool",
        "tool_name": "read_many_files",
        "tool_input": {"paths": ["/x/a"]},
    },
    {
        "hook_event_name": "AfterTool",
        "tool_name": "run_shell_command",
        "tool_input": {"command": "frob x"},
    },
]

CODEX_EXTRA = [
    {"hook_event_name": "Stop"},
    {
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": {"command": "frob x"},
    },
    {
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": {"command": "frob x"},
        "permission_mode": "bypassPermissions",
    },
    {
        "hook_event_name": "PermissionRequest",
        "tool_name": "mcp__fake__get",
    },
]

CURSOR_PAYLOADS = [
    {"command": "frob x"},
    {"command": "frob delete"},
    {"command": "rm -rf x"},
    {
        "hook_event_name": "preToolUse",
        "tool_name": "Shell",
        "tool_input": {"command": "frob x"},
    },
    {
        "hook_event_name": "preToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": "x"},
    },
    {"tool_name": "fake", "tool_input": '{"a": 1}', "command": "frob delete"},
    {"hook_event_name": "Stop"},
    {"hook_event_name": "PostToolUse", "command": "frob x"},
]


def tool_call(name: str, args: dict, **extra) -> dict:
    return {
        "toolCall": {"name": name, "args": args},
        "workspacePaths": [str(WORK)],
        **extra,
    }


AGY_PAYLOADS = [
    tool_call("run_command", {"CommandLine": "frob x", "Cwd": str(WORK)}),
    tool_call("run_command", {"CommandLine": "frob delete"}),
    tool_call("run_command", {"CommandLine": "rm -rf x", "Cwd": "sub"}),
    tool_call(
        "run_command", {"command": "frob x"}, permission_mode="bypassPermissions"
    ),
    tool_call("call_mcp_tool", {"ServerName": "fake", "ToolName": "get"}),
    tool_call("call_mcp_tool", {"ServerName": "fake", "ToolName": "drop"}),
    tool_call("call_mcp_tool", {"ToolName": "solo"}),
    tool_call("mcp__fake__write_file", {}),
    tool_call(
        "call_mcp_tool", {"ServerName": "fake", "ToolName": "get"}, toolResponse={}
    ),
    tool_call("search_web", {"query": "rust book"}),
    tool_call("search_web", {"query": "a secret"}),
    tool_call("read_url_content", {"Url": "https://x/cats"}),
    tool_call("read_url_content", {"Url": "https://x/rust"}, toolResponse={}),
    tool_call("view_file", {"AbsolutePath": f"{WORK}/src/a.py"}),
    tool_call("view_file", {"AbsolutePath": f"{WORK}/private/k"}),
    tool_call("view_file", {"AbsolutePath": "/etc/hosts"}),
    tool_call("write_to_file", {"TargetFile": f"{WORK}/out/a"}),
    tool_call("write_to_file", {"TargetFile": f"{WORK}/out/keep"}),
    tool_call("replace_file_content", {"TargetFile": "/nowhere/x"}),
    tool_call("list_dir", {"DirectoryPath": str(WORK)}),
    tool_call("grep_search", {"SearchPath": "/elsewhere"}),
    tool_call("view_file", {}),
    tool_call("view_file", {"AbsolutePath": f"{WORK}/a"}, toolResponse={}),
    tool_call("browser_open", {}),
    tool_call("run_command", {"CommandLine": "frob x"}, toolResponse={}),
    {"terminationReason": "done"},
    {"fullyIdle": True},
]


def decision(fmt: str, result: tuple[int, str, str]) -> str:
    code, stdout, _ = result
    if code == 2:
        return "deny"
    try:
        data = json.loads(stdout) if stdout.strip() else {}
    except json.JSONDecodeError:
        return "invalid"
    if not isinstance(data, dict):
        return "none"
    if fmt == "cursor":
        return data.get("permission", "none")
    if fmt in ("gemini", "agy"):
        return data.get("decision", "none")
    hso = data.get("hookSpecificOutput", {})
    if fmt == "codex":
        if hso.get("decision", {}).get("behavior") == "allow":
            return "allow"
        return "ask" if "systemMessage" in data else "none"
    return hso.get("permissionDecision", "none")


def normalize(fmt: str, result: tuple[int, str, str]) -> tuple[int, str, str]:
    """Python prints `null` where Codex output is empty; dippy-rs prints nothing.
    stderr is hook output only for a Codex block (exit 2); otherwise it holds
    Python's log warnings."""
    code, stdout, stderr = result
    if fmt == "codex" and stdout == "null":
        stdout = ""
    return code, stdout, stderr if code == 2 else ""


def run(binary: str, job: dict) -> tuple[int, str, str]:
    env = child_env()
    if job["config"]:
        env["DIPPY_CONFIG"] = str(config_path_for(job["config"]))
    if job.get("askpass"):
        env["DIPPY_ASKPASS"] = str(job["askpass"])
    env.update(job["env"])
    proc = subprocess.run(
        [binary, *job["flags"]],
        input=json.dumps(job["payload"]),
        capture_output=True,
        text=True,
        env=env,
        cwd=ROOT,
        timeout=60,
    )
    return proc.returncode, proc.stdout.strip(), proc.stderr.strip()


def job(
    fmt: str,
    payload: dict,
    config: str = TOOL_CONFIG,
    flags=None,
    askpass=None,
    env=None,
) -> dict:
    """A hand-written payload (TOOL_CONFIG) must give byte-identical output;
    corpus commands may differ in reason text."""
    return {
        "fmt": fmt,
        "flags": [f"--{fmt}"] if flags is None else flags,
        "payload": payload,
        "config": config,
        "askpass": askpass,
        "env": env or {},
        "hand": config == TOOL_CONFIG,
    }


def build_jobs(cases: list[dict], sample_size: int) -> list[dict]:
    random.seed(20261009)
    sample = random.sample(cases, min(sample_size, len(cases)))
    small = sample[: max(sample_size // 4, 1)]
    after_cases = [c for c in cases if c["config"] and "after " in c["config"]][:40]
    work = str(WORK)
    jobs = []

    def bash(event: str, cmd: str, tool: str = "Bash") -> dict:
        return {
            "hook_event_name": event,
            "tool_name": tool,
            "tool_input": {"command": cmd},
            "cwd": work,
        }

    # Claude Code
    jobs += [job("claude", bash("PreToolUse", c["cmd"]), c["config"]) for c in sample]
    jobs += [
        job("claude", bash("PostToolUse", c["cmd"]), c["config"]) for c in after_cases
    ]
    jobs += [job("claude", dict(p, cwd=work)) for p in EXTRA]
    for p in TOOL_PAYLOADS:
        payload = {"hook_event_name": "PreToolUse", "tool_input": {}, **p, "cwd": work}
        jobs.append(job("claude", payload, TOOL_CONFIG))
        jobs.append(job("gemini", payload, TOOL_CONFIG))
        jobs.append(job("codex", payload, TOOL_CONFIG))
    # Gemini CLI
    shell = "run_shell_command"
    jobs += [
        job("gemini", bash("BeforeTool", c["cmd"], shell), c["config"]) for c in small
    ]
    jobs += [job("gemini", dict(p, cwd=work), TOOL_CONFIG) for p in GEMINI_EXTRA]
    # Codex
    jobs += [job("codex", bash("PreToolUse", c["cmd"]), c["config"]) for c in small]
    jobs += [
        job("codex", bash("PermissionRequest", c["cmd"]), c["config"]) for c in small
    ]
    jobs += [job("codex", dict(p, cwd=work), TOOL_CONFIG) for p in CODEX_EXTRA]
    # Cursor
    jobs += [
        job("cursor", {"command": c["cmd"], "cwd": work}, c["config"]) for c in small
    ]
    jobs += [job("cursor", dict(p, cwd=work), TOOL_CONFIG) for p in CURSOR_PAYLOADS]
    # AGY: corpus commands with a refusing askpass, tools with each askpass.
    for c in small:
        payload = tool_call("run_command", {"CommandLine": c["cmd"], "Cwd": work})
        jobs.append(job("agy", payload, c["config"], askpass=ASKPASS_DENY))
    for p in AGY_PAYLOADS:
        for askpass in (None, ASKPASS_DENY, ASKPASS_ALLOW):
            jobs.append(job("agy", p, TOOL_CONFIG, askpass=askpass))
    # Bare AGY (no workspaces: the process cwd is the policy workspace) and the
    # hcom integration (DIPPY_POLICY_CWD), valid and invalid.
    bare = {"toolCall": {"name": "run_command", "args": {"CommandLine": "frob x"}}}
    jobs.append(job("agy", bare))
    for policy in (work, "relative", f"{work}/missing"):
        jobs.append(job("agy", AGY_PAYLOADS[2], env={"DIPPY_POLICY_CWD": policy}))
    # No mode flag: detected from the payload.
    auto = [
        ("agy", AGY_PAYLOADS[0]),
        ("agy", AGY_PAYLOADS[2]),
        ("cursor", dict(CURSOR_PAYLOADS[1], cwd=work)),
        (
            "cursor",
            {
                "tool_name": "Shell",
                "tool_input": {"command": "frob x"},
                "cursor_version": "2",
            },
        ),
        ("gemini", bash("BeforeTool", "frob delete", shell)),
        ("claude", bash("PreToolUse", "frob x")),
    ]
    jobs += [
        job(fmt, p, TOOL_CONFIG, flags=[], askpass=ASKPASS_DENY) for fmt, p in auto
    ]
    return jobs


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--sample", type=int, default=600)
    args = ap.parse_args()
    cases = [
        json.loads(line) for line in (HERE / "corpus.jsonl").read_text().splitlines()
    ]
    prepare_root(cases)
    config_path_for(TOOL_CONFIG).write_text(TOOL_CONFIG, encoding="utf-8")
    for path, code in ((ASKPASS_ALLOW, 0), (ASKPASS_DENY, 1)):
        path.write_text(f"#!/bin/sh\ncat >/dev/null\nexit {code}\n", encoding="utf-8")
        path.chmod(0o755)
    jobs = build_jobs(cases, args.sample)

    def both(j):
        return normalize(j["fmt"], run(PY, j)), normalize(j["fmt"], run(RS, j))

    with ThreadPoolExecutor(os.cpu_count() or 4) as pool:
        results = list(pool.map(both, jobs))

    per_mode = collections.defaultdict(lambda: [0, 0, 0])
    unsafe = []
    diffs = []
    for j, (py, rs) in zip(jobs, results):
        label = j["fmt"] if j["flags"] else f"{j['fmt']} (auto)"
        stats = per_mode[label]
        stats[0] += 1
        stats[1] += py == rs
        same = decision(j["fmt"], py) == decision(j["fmt"], rs)
        stats[2] += same
        if decision(j["fmt"], rs) == "allow" and decision(j["fmt"], py) != "allow":
            unsafe.append(j)
        if py != rs:
            diffs.append((j, py, rs, same))
    total = len(jobs)
    exact = sum(s[1] for s in per_mode.values())
    same_decision = sum(s[2] for s in per_mode.values())
    print(
        f"{total} payloads: {exact} identical output, {same_decision} same decision, {len(unsafe)} unsafe"
    )
    for label, (n, ident, same) in sorted(per_mode.items()):
        print(f"  {label}: {n} payloads, {ident} identical, {same} same decision")
    hand = sum(1 for j in jobs if j["hand"])
    hand_diffs = sum(1 for j, *_ in diffs if j["hand"])
    print(f"hand-written payloads: {hand_diffs} of {hand} not identical")
    # Decision differences first, then hand-written, then corpus output.
    diffs.sort(key=lambda d: (d[3], not d[0]["hand"]))
    for j, py, rs, same in diffs[:25]:
        tag = "output" if same else "DECISION"
        print(
            f"  [{tag}] {j['fmt']} {j['flags']} askpass={j['askpass'] and j['askpass'].name}"
        )
        print("     ", json.dumps(j["payload"])[:140])
        print("     py:", py[0], py[1][:160], py[2][:80])
        print("     rs:", rs[0], rs[1][:160], rs[2][:80])
    return 1 if unsafe else 0


if __name__ == "__main__":
    sys.exit(main())
