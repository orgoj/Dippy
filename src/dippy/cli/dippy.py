"""Dippy's own CLI handler.

`dippy audit` only queries the audit log. Every other subcommand can execute
commands or change configuration and needs approval unless a rule allows it.
"""

from dippy.cli import Classification, HandlerContext

COMMANDS = ["dippy"]


def classify(ctx: HandlerContext) -> Classification:
    """Classify dippy command."""
    tokens = ctx.tokens
    if len(tokens) > 1 and tokens[1] == "audit":
        return Classification("allow", description="dippy audit")
    return Classification("ask")
