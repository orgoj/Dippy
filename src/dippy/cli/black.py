"""black handler for Dippy.

black is a Python code formatter. It modifies files in place by default,
but --check and --diff are read-only modes.
"""

from dippy.cli import Classification, HandlerContext

COMMANDS = ["black"]

SAFE_FLAGS = frozenset({"--check", "--diff"})


def classify(ctx: HandlerContext) -> Classification:
    """Classify black command."""
    tokens = ctx.tokens
    if not tokens:
        return Classification("ask", description="black")

    for token in tokens[1:]:
        if token in SAFE_FLAGS:
            return Classification("allow", description=f"black {token}")

    return Classification("ask", description="black")
