#!/usr/bin/env python3
"""Compare Python and Rust CLI handlers on identical inputs.

For every simple command in the corpus (and in optional extra files) whose
base word has a Python handler, build the HandlerContext exactly as the
analyzer does (quote-stripped tokens, raw words, expansion flags; leading
assignments skipped), run the Python handler, then feed the same inputs to
``dippy-rs --handler-jsonl`` and compare the classifications.

Usage:
  handler_compare.py [--module NAME ...] [--cases FILE ...] [--show N]

Exit status is 1 when any handler marked PORTED differs (unsafe ones first).
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from parity_env import HOME, WORK, child_env  # noqa: E402

os.environ["HOME"] = str(HOME)
HOME.mkdir(parents=True, exist_ok=True)
WORK.mkdir(parents=True, exist_ok=True)

from dippy.cli import HandlerContext, KNOWN_HANDLERS, get_handler  # noqa: E402
from dippy.core.config import Config  # noqa: E402
from dippy.vendor.parable import parse  # noqa: E402

BIN = HERE.parent / "target" / "release" / "dippy-rs"
FIELDS = (
    "action",
    "inner_command",
    "redirect_targets",
    "wrapper_context",
    "remote",
    "replace_suggestion",
)


def strip_quotes(value: str) -> str:
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
        return value[1:-1]
    return value


def simple_commands(node, out: list) -> None:
    """Collect every command node, including those inside substitutions."""
    if node is None or isinstance(node, (str, int, bool)):
        return
    if isinstance(node, list):
        for item in node:
            simple_commands(item, out)
        return
    kind = getattr(node, "kind", None)
    if kind == "command":
        out.append(node)
    for name, value in vars(node).items():
        if name.startswith("_") or name == "kind":
            continue
        if isinstance(value, list) or hasattr(value, "kind"):
            simple_commands(value, out)


def contexts(command: str):
    try:
        nodes = parse(command)
    except Exception:
        return
    found: list = []
    simple_commands(nodes, found)
    for node in found:
        words = [strip_quotes(w.value) for w in node.words]
        raw = [w.value for w in node.words]
        exp = [bool(getattr(w, "parts", [])) for w in node.words]
        i = 0
        while i < len(words) and "=" in words[i] and not words[i].startswith("-"):
            i += 1
        if i < len(words):
            yield words[i:], raw[i:], exp[i:]


def py_result(tokens, raw, exp) -> dict:
    handler = get_handler(tokens[0])
    ctx = HandlerContext(
        tokens,
        remote=False,
        cwd=WORK,
        config=Config(),
        word_has_expansions=tuple(exp),
        raw_words=tuple(raw),
    )
    try:
        result = handler.classify(ctx)
    except Exception as exc:
        return {"exception": type(exc).__name__}
    out = {name: getattr(result, name) for name in FIELDS}
    out["description"] = result.description
    for key in ("redirect_targets", "wrapper_context"):
        if out[key] is not None:
            out[key] = list(out[key])
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--module", action="append", default=[])
    ap.add_argument("--cases", action="append", default=[])
    ap.add_argument("--show", type=int, default=10)
    ap.add_argument("--all", action="store_true", help="include unported modules")
    args = ap.parse_args()

    commands: list[str] = []
    for line in (HERE / "corpus.jsonl").read_text().splitlines():
        commands.append(json.loads(line)["cmd"])
    for path in args.cases:
        for line in Path(path).read_text().splitlines():
            line = line.strip()
            if line.startswith("{"):
                commands.append(json.loads(line)["cmd"])
            elif line and not line.startswith("#"):
                commands.append(line)

    inputs = []
    seen = set()
    for command in dict.fromkeys(commands):
        for tokens, raw, exp in contexts(command):
            module = KNOWN_HANDLERS.get(tokens[0])
            if not module or (args.module and module not in args.module):
                continue
            key = json.dumps([tokens, raw, exp])
            if key in seen:
                continue
            seen.add(key)
            inputs.append(
                {
                    "module": module,
                    "tokens": tokens,
                    "raw": raw,
                    "exp": exp,
                    "cwd": str(WORK),
                    "py": py_result(tokens, raw, exp),
                }
            )

    payload = "\n".join(json.dumps(i) for i in inputs).encode()
    proc = subprocess.run(
        [str(BIN), "--handler-jsonl"],
        input=payload,
        capture_output=True,
        env=child_env(),
        check=True,
    )
    rs_results = [json.loads(line) for line in proc.stdout.decode().splitlines()]
    assert len(rs_results) == len(inputs)

    stats = collections.defaultdict(lambda: collections.Counter())
    diffs = collections.defaultdict(list)
    for item, rs in zip(inputs, rs_results):
        module = item["module"]
        ported = rs.pop("ported", False)
        if not ported and not args.all:
            stats[module]["unported"] += 1
            continue
        py = item["py"]
        same = all(py.get(f) == rs.get(f) for f in FIELDS) and "exception" not in py
        stats[module]["same" if same else "diff"] += 1
        if py.get("description") != rs.get("description"):
            stats[module]["desc"] += 1
        if not same:
            unsafe = rs.get("action") == "allow" and py.get("action") != "allow"
            stats[module]["unsafe"] += unsafe
            diffs[module].append((unsafe, item["tokens"], py, rs))

    failed = False
    for module in sorted(stats):
        c = stats[module]
        if c["unported"] and not (c["same"] or c["diff"]):
            continue
        print(
            f"{module:16} same={c['same']:5} diff={c['diff']:4} "
            f"unsafe={c['unsafe']:3} desc-diff={c['desc']:4}"
        )
        failed |= bool(c["diff"])
        for unsafe, tokens, py, rs in sorted(diffs[module], key=lambda d: not d[0])[
            : args.show
        ]:
            tag = "UNSAFE " if unsafe else ""
            print(f"   {tag}{tokens!r}")
            print(f"      py={json.dumps({k: py.get(k) for k in FIELDS})}")
            print(f"      rs={json.dumps({k: rs.get(k) for k in FIELDS})}")
    unported = sorted(m for m, c in stats.items() if c["unported"])
    if unported:
        print(f"unported modules with cases: {len(unported)}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
