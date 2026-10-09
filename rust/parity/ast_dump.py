"""Dump Parable ASTs as canonical JSON for comparison with dippy-rs.

Only the node attributes Dippy's analyzer reads are included. dippy-rs builds
the same Parable-shaped tree from Rable (``dippy-rs --dump-ast CMD``); any
difference is a parser divergence the Rust port must account for.

Usage: python ast_dump.py < cases.jsonl  (prints one JSON line per case)
"""

from __future__ import annotations

import json
import sys

from dippy.vendor.parable import ParseError, parse

FIELDS = {
    "word": ("value", "parts"),
    "command": ("words", "redirects"),
    "pipeline": ("commands",),
    "list": ("parts",),
    "operator": (),
    "redirect": ("op", "target"),
    "heredoc": ("content", "quoted", "fd"),
    "subshell": ("body", "redirects"),
    "brace-group": ("body", "redirects"),
    "if": ("condition", "then_body", "else_body", "redirects"),
    "while": ("condition", "body", "redirects"),
    "until": ("condition", "body", "redirects"),
    "for": ("words", "body", "redirects"),
    "for-arith": ("init", "cond", "incr", "body", "redirects"),
    "select": ("words", "body", "redirects"),
    "case": ("word", "patterns", "redirects"),
    "pattern": ("body",),
    "function": ("body",),
    "param": ("arg",),
    "param-len": (),
    "param-indirect": ("arg",),
    "cmdsub": ("command",),
    "procsub": ("direction", "command"),
    "arith": ("expression",),
    "arith-cmd": ("expression", "redirects"),
    "ansi-c": (),
    "locale": (),
    "negation": ("pipeline",),
    "time": ("pipeline",),
    "cond-expr": ("body", "redirects"),
    "unary-test": ("operand",),
    "binary-test": ("left", "right"),
    "cond-and": ("left", "right"),
    "cond-or": ("left", "right"),
    "cond-not": ("operand",),
    "cond-paren": ("inner",),
    "coproc": ("command",),
    "comment": (),
    "empty": (),
    "array": (),
    "pipe-both": (),
    "arith-deprecated": (),
}

ARITH_CHILDREN = ("value", "target", "left", "right", "operand", "index", "expression")


def arith_cmdsubs(node, out: list) -> None:
    """Mirror analyzer._find_cmdsubs_in_arith: collect cmdsub nodes."""
    if node is None:
        return
    if getattr(node, "kind", None) == "cmdsub":
        out.append(node)
        return
    for attr in ARITH_CHILDREN:
        child = getattr(node, attr, None)
        if child is not None:
            arith_cmdsubs(child, out)


def dump(node):
    if node is None or isinstance(node, (str, int, bool)):
        return node
    if isinstance(node, list):
        return [dump(n) for n in node]
    kind = getattr(node, "kind", None)
    if kind in ("arith", "arith-cmd"):
        found: list = []
        arith_cmdsubs(node.expression, found)
        result = {"kind": kind, "cmdsubs": [dump(c) for c in found]}
        if kind == "arith-cmd":
            result["redirects"] = dump(getattr(node, "redirects", None) or [])
        return result
    result = {"kind": kind}
    for name in FIELDS.get(kind, ()):
        value = getattr(node, name, None)
        if name == "redirects":
            value = value or []
        if name == "fd" and value == 0:
            value = 0
        result[name] = dump(value)
    if kind not in FIELDS:
        result["unknown"] = True
    return result


def dump_command(cmd: str) -> dict:
    try:
        return {"nodes": dump(parse(cmd))}
    except ParseError:
        return {"error": "parse"}
    except Exception as exc:  # parser crash
        return {"error": type(exc).__name__}


def main() -> None:
    for line in sys.stdin:
        if line.strip():
            case = json.loads(line)
            print(json.dumps(dump_command(case["cmd"].strip()), sort_keys=True))


if __name__ == "__main__":
    main()
