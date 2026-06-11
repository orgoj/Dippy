"""isort handler for Dippy.

isort is a Python import sorter. It modifies files in place by default,
but --check-only, --check, -c, --diff, -d are read-only modes.
"""

from dippy.cli import Classification, HandlerContext

COMMANDS = ["isort"]

SAFE_FLAGS = frozenset({"--check-only", "--check", "-c", "--diff", "-d"})


def classify(ctx: HandlerContext) -> Classification:
    """Classify isort command."""
    tokens = ctx.tokens
    if not tokens:
        return Classification("ask", description="isort")

    for token in tokens[1:]:
        if token in SAFE_FLAGS:
            return Classification("allow", description=f"isort {token}")

    return Classification("ask", description="isort")
