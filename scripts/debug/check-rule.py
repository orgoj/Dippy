"""Check paths, web queries or MCP tool names against Dippy rules.

Usage:
  PYTHONPATH=src python3 ./scripts/debug/check-rule.py KIND [options] VALUE [VALUE ...]

KIND is read, edit, redirect, web or mcp. Without --config/--config-only the
live configuration for --cwd is used, exactly as a hook would load it.
--config-only FILE evaluates a candidate rules file in isolation.
Command rules are checked with scripts/debug/try-rules.sh.
"""

import argparse
from pathlib import Path

from dippy.core.config import (
    load_config,
    match_edit,
    match_mcp,
    match_read,
    match_redirect,
    match_web,
)

parser = argparse.ArgumentParser(prog="check-rule.py")
parser.add_argument("kind", choices=["read", "edit", "redirect", "web", "mcp"])
parser.add_argument("values", nargs="+", metavar="VALUE")
scope = parser.add_mutually_exclusive_group()
scope.add_argument("--config", metavar="FILE", help="Add FILE on top of live config")
scope.add_argument("--config-only", metavar="FILE", help="Use only FILE")
parser.add_argument("--cwd", default=".", help="Working directory (default: .)")
parser.add_argument("--remote", action="store_true", help="Remote redirect target")
parser.add_argument(
    "--flag",
    action="append",
    default=[],
    help="Active context flag, e.g. '$HCOM_INSTANCE_NAME=knowledge' (repeatable)",
)
args = parser.parse_args()

cwd = Path(args.cwd).resolve()
config = load_config(cwd, config_path=args.config, config_only_path=args.config_only)
flags = frozenset(args.flag)

for value in args.values:
    if args.kind == "read":
        match = match_read(value, config, cwd, flags)
    elif args.kind == "edit":
        match = match_edit(value, config, cwd, flags)
    elif args.kind == "redirect":
        match = match_redirect(value, config, cwd, remote=args.remote)
    elif args.kind == "web":
        match = match_web(value, config, flags)
    else:
        match = match_mcp(value, config)
    if match:
        detail = f"{match.pattern} @ {match.source}" if match.source else match.pattern
        print(f"{args.kind} {value} -> {match.decision} ({detail})")
    else:
        print(f"{args.kind} {value} -> no match (default: {config.default})")
