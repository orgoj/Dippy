"""pre-commit handler for Dippy.

pre-commit manages git pre-commit hooks. Most commands modify files or hooks.
Only validation and help commands are safe.
"""

from dippy.cli import Classification, HandlerContext

COMMANDS = ["pre-commit"]

SAFE_ACTIONS = frozenset(
    {
        "validate-config",
        "validate-manifest",
        "help",
    }
)


def classify(ctx: HandlerContext) -> Classification:
    """Classify pre-commit command."""
    tokens = ctx.tokens
    if len(tokens) < 2:
        return Classification("allow", description="pre-commit")

    action = tokens[1]

    if action in SAFE_ACTIONS:
        return Classification("allow", description=f"pre-commit {action}")

    return Classification("ask", description=f"pre-commit {action}")
