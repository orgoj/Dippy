#!/usr/bin/env python3
"""Compare the logs Python Dippy and dippy-rs write in hook mode, and `audit`.

1. Every hook_compare.py job runs in its own empty HOME whose
   ``~/.dippy/config`` turns logging on (``DIPPY_TEST_NO_LOG`` unset). The
   audit log lines (``ts`` masked, its format checked), the
   ``hook-approvals.log`` lines (time masked) and the set of files written
   must be identical. Hand-written payloads also run with
   ``log-hook-approvals off`` and without ``log-full``.
2. ``audit`` queries over a fixture log with rotations: exit code and stdout
   identical (stderr too for Dippy's own messages; argparse usage text is
   not compared).
3. Daily rotation: the files left after one config load.

Usage: log_compare.py [--sample N]
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import date, timedelta
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import hook_compare as hc  # noqa: E402
from parity_env import ROOT, WORK, child_env, config_path_for, prepare_root  # noqa: E402

LOGS = ROOT / "logs"
TS = re.compile(r'"ts": "(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{6})?\+00:00)"')
LINE_TIME = re.compile(r"^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d (?=\[)")

FULL = "set log ~/logs/audit.log\nset log-full\n"
# Also exercises every config warning that reaches hook-approvals.log.
QUIET = """\
set log ~/audit.log
set log-hook-approvals off
alias a b
alias a c
wrapper w
wrapper w
bogus directive
include
include /nonexistent/*.dippy
set final /nonexistent/final
"""


def drop_codex_phantom_pass(lines: list[str]) -> tuple[list[str], int]:
    """Python bug: Codex `approve()` returns None, so an allowed MCP, web or
    file tool also logs `pass`/`no matching rule`. dippy-rs logs the allow
    only."""
    out, dropped = [], 0
    for line in lines:
        if (
            out
            and '"decision": "allow"' in out[-1]
            and line.startswith('{"decision": "pass", "message": "no matching rule"')
        ):
            dropped += 1
            continue
        out.append(line)
    return out, dropped


def run_logged(binary: str, job: dict, home: Path, settings: str) -> dict:
    if home.exists():
        shutil.rmtree(home)
    (home / ".dippy").mkdir(parents=True)
    (home / ".dippy" / "config").write_text(settings, encoding="utf-8")
    env = child_env()
    del env["DIPPY_TEST_NO_LOG"]
    env["HOME"] = str(home)
    if job["config"]:
        env["DIPPY_CONFIG"] = str(config_path_for(job["config"]))
    if job.get("askpass"):
        env["DIPPY_ASKPASS"] = str(job["askpass"])
    env.update(job["env"])
    subprocess.run(
        [binary, *job["flags"]],
        input=json.dumps(job["payload"]),
        capture_output=True,
        text=True,
        env=env,
        cwd=ROOT,
        timeout=60,
    )
    files, bad = {}, []
    for path in sorted(p for p in home.rglob("*") if p.is_file()):
        rel = str(path.relative_to(home))
        if rel == ".dippy/config":
            continue
        text = path.read_text(encoding="utf-8", errors="replace").replace(
            str(home), "$HOME"
        )
        lines = []
        for line in text.splitlines():
            if rel.endswith("hook-approvals.log"):
                # A multi-line message continues on lines without a time.
                masked = LINE_TIME.sub("", line)
                if masked == line and not lines:
                    bad.append(line)
            else:
                masked = TS.sub('"ts": "TS"', line)
                if masked == line:
                    bad.append(line)
            lines.append(masked)
        files[rel] = lines
    return {"files": files, "bad": bad}


def compare_hooks(jobs: list[dict]) -> int:
    runs = []
    for i, j in enumerate(jobs):
        runs.append((j, FULL, i))
        if j["hand"]:
            runs.append((j, QUIET, i))

    def both(item):
        j, settings, i = item
        tag = f"{i}-{'full' if settings is FULL else 'quiet'}"
        return (
            run_logged(hc.PY, j, LOGS / f"{tag}-py", settings),
            run_logged(hc.RS, j, LOGS / f"{tag}-rs", settings),
        )

    with ThreadPoolExecutor(os.cpu_count() or 4) as pool:
        results = list(pool.map(both, runs))
    phantom = 0
    for (j, _, _), (py, _) in zip(runs, results):
        if j["fmt"] != "codex":
            continue
        for name, lines in py["files"].items():
            if name.endswith("audit.log"):
                py["files"][name], dropped = drop_codex_phantom_pass(lines)
                phantom += dropped
    diffs = [(r, py, rs) for r, (py, rs) in zip(runs, results) if py != rs]
    bad = sum(len(py["bad"]) + len(rs["bad"]) for py, rs in results)
    lines = sum(len(v) for py, _ in results for v in py["files"].values())
    print(
        f"hook logs: {len(runs)} runs, {lines} Python log lines, "
        f"{len(diffs)} differ, {bad} malformed timestamps; "
        f"{phantom} Codex phantom `pass` entries dropped (Python bug)"
    )
    for (j, settings, _), py, rs in diffs[:15]:
        print(f"  {j['fmt']} {j['flags']} {'full' if settings is FULL else 'quiet'}")
        print("     ", json.dumps(j["payload"])[:140])
        for name in sorted(set(py["files"]) | set(rs["files"])):
            a, b = py["files"].get(name), rs["files"].get(name)
            if a != b:
                print(f"     {name}\n       py: {a}\n       rs: {b}")
    return len(diffs) + bad


def entry(day: str, **fields) -> str:
    return json.dumps({**fields, "ts": f"{day}T10:00:00.000001+00:00"})


def audit_fixture(base: Path, today: date) -> Path:
    if base.exists():
        shutil.rmtree(base)
    base.mkdir(parents=True)
    work = str(WORK)
    d = [(today - timedelta(days=n)).isoformat() for n in range(6)]
    (base / f"audit-{d[5]}.log").write_text(
        entry(d[5], decision="ask", cmd="old", agent="pi") + "\n"
    )
    (base / f"audit-{d[2]}.log").write_text(
        "\n".join(
            [
                entry(d[2], decision="allow", cmd="ls", cwd=work, agent="claude"),
                "not json",
                "[1, 2]",
                entry(
                    d[1],
                    decision="deny",
                    cmd="frob delete",
                    cwd=f"{work}/sub",
                    agent="agy",
                    policy_cwd=work,
                ),
            ]
        )
        + "\n"
    )
    (base / "audit-junk.log").write_text(entry(d[3], decision="ask", cmd="junk") + "\n")
    (base / "audit.log").write_text(
        "\n".join(
            [
                entry(
                    d[0],
                    decision="ask",
                    cmd="rm x",
                    command="rm -rf x",
                    cwd=work,
                    agent="claude",
                    context_flags=["$A=1", "$B=2"],
                    suggestion="rm *",
                ),
                entry(
                    d[0],
                    decision="pass",
                    message="no matching rule",
                    tool="Write",
                    file_path=f"{work}/a",
                    agent="pi",
                ),
                entry(
                    d[0], decision="allow", cmd="frob x", cwd=f"{work}x", agent="claude"
                ),
                json.dumps({"decision": "ask", "cmd": "no ts"}),
                entry(d[1], decision="ask", cmd="✓ unicode", agent=None),
            ]
        )
        + "\n"
    )
    config = base / "config"
    config.write_text(f"set log {base}/audit.log\nset log-rotate-max-days 0\n")
    return config


def run_audit(binary: str, config: Path, args: list[str]) -> tuple[int, str, str]:
    env = child_env()
    env["HOME"] = str(ROOT / "home")
    proc = subprocess.run(
        [binary, "--config-only", str(config), "audit", *args],
        capture_output=True,
        text=True,
        env=env,
        cwd=ROOT,
        timeout=60,
    )
    stderr = proc.stderr.strip() if proc.returncode == 1 else ""
    return proc.returncode, proc.stdout, stderr


def compare_audit() -> int:
    today = date.today()
    config = audit_fixture(LOGS / "audit", today)
    work = str(WORK)
    d1 = (today - timedelta(days=1)).isoformat()
    queries = [
        [],
        ["--since", d1],
        ["--until", d1],
        ["--since", d1, "--until", d1],
        ["--decision", "ask", "--decision", "deny"],
        ["--not-allow"],
        ["--agent", "claude"],
        ["--cwd", work],
        ["--cwd", f"{work}/"],
        ["--cwd", "work"],
        ["--policy-cwd", work],
        ["--tool", "Write"],
        ["--grep", "frob"],
        ["--grep", "✓"],
        ["--group-by", "cmd"],
        ["--group-by", "decision", "--group-by", "agent", "--limit", "2"],
        ["--group-by", "context_flags"],
        ["--limit", "2"],
        ["--limit", "0"],
        ["--limit", "-1"],
        ["--since", "yesterday"],
        ["--decision", "maybe"],
        ["--bogus"],
    ]
    failures = 0
    for args in queries:
        py, rs = run_audit(hc.PY, config, args), run_audit(hc.RS, config, args)
        if py != rs:
            failures += 1
            print(f"  audit {args}\n     py: {py}\n     rs: {rs}")
    missing = LOGS / "audit" / "nolog"
    missing.write_text("set log-full\n")
    for args in ([],):
        py, rs = run_audit(hc.PY, missing, args), run_audit(hc.RS, missing, args)
        if py != rs:
            failures += 1
            print(f"  audit without log\n     py: {py}\n     rs: {rs}")
    print(f"audit: {len(queries) + 1} queries, {failures} differ")
    return failures


def compare_rotation() -> int:
    today = date.today()
    results = []
    for binary in (hc.PY, hc.RS):
        base = LOGS / "rotate"
        if base.exists():
            shutil.rmtree(base)
        base.mkdir(parents=True)
        (base / "audit.log").write_text("current\n")
        for n in (2, 3, 4, 10):
            day = (today - timedelta(days=n)).isoformat()
            (base / f"audit-{day}.log").write_text(f"{day}\n")
        (base / "audit-x-y-z.log").write_text("junk\n")
        config = base / "config"
        config.write_text(f"set log {base}/audit.log\nset log-rotate-max-days 3\n")
        run_audit(binary, config, ["--limit", "0"])
        listing = sorted(
            (p.name, p.read_text()) for p in base.iterdir() if p.name != "config"
        )
        results.append(listing)
    same = results[0] == results[1]
    print(f"rotation: {'identical' if same else 'DIFFERENT'} ({len(results[0])} files)")
    if not same:
        print(f"     py: {results[0]}\n     rs: {results[1]}")
    return 0 if same else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--sample", type=int, default=200)
    args = ap.parse_args()
    cases = [
        json.loads(line) for line in (HERE / "corpus.jsonl").read_text().splitlines()
    ]
    prepare_root(cases)
    config_path_for(hc.TOOL_CONFIG).write_text(hc.TOOL_CONFIG, encoding="utf-8")
    for path, code in ((hc.ASKPASS_ALLOW, 0), (hc.ASKPASS_DENY, 1)):
        path.write_text(f"#!/bin/sh\ncat >/dev/null\nexit {code}\n", encoding="utf-8")
        path.chmod(0o755)
    if LOGS.exists():
        shutil.rmtree(LOGS)
    failures = compare_hooks(hc.build_jobs(cases, args.sample))
    failures += compare_audit()
    failures += compare_rotation()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
