"""yq handler for Dippy.

yq is a YAML/JSON/XML processor. It outputs to stdout by default,
but -i/--inplace modifies files in place.
"""

from dippy.cli import Classification, HandlerContext

COMMANDS = ["yq"]


def classify(ctx: HandlerContext) -> Classification:
    """Classify yq command."""
    tokens = ctx.tokens
    if not tokens:
        return Classification("ask", description="yq")

    for token in tokens[1:]:
        if token in ("-i", "--inplace"):
            return Classification("ask", description="yq -i")
        if token.startswith("-i=") or token.startswith("--inplace="):
            return Classification("ask", description="yq -i")

    return Classification("allow", description="yq")
