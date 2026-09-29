"""mysql handler for Dippy."""

from __future__ import annotations


from dippy.cli import Classification, HandlerContext
from dippy.cli.sql_args import literal_sql_arg
from dippy.core.sql import is_readonly_sql

COMMANDS = ["mysql"]

# MySQL-specific keywords that perform writes
_MYSQL_WRITE = frozenset({"LOAD"})


def _extract_execute_sql(ctx: HandlerContext) -> str | None:
    """Extract SQL from -e or --execute option."""
    tokens = ctx.tokens
    i = 0
    while i < len(tokens):
        token = tokens[i]
        if token in ("-e", "--execute") and i + 1 < len(tokens):
            return literal_sql_arg(ctx, i + 1)
        if token.startswith("--execute="):
            return literal_sql_arg(ctx, i, prefix="--execute=")
        if token.startswith("-e") and len(token) > 2:
            return literal_sql_arg(ctx, i, prefix="-e")
        i += 1
    return None


def classify(ctx: HandlerContext) -> Classification:
    tokens = ctx.tokens

    # Help/version
    if any(t in ("--help", "-?", "--version", "-V") for t in tokens):
        return Classification("allow", description="mysql help/version")

    # Extract SQL from -e/--execute
    sql = _extract_execute_sql(ctx)

    # No SQL found - interactive mode
    if sql is None:
        return Classification("ask", description="mysql (interactive)")

    # Analyze SQL
    readonly = is_readonly_sql(sql, extra_write=_MYSQL_WRITE, dialect="mysql")
    if readonly is True:
        return Classification("allow", description="mysql (read-only query)")
    if readonly is False:
        return Classification("ask", description="mysql (write query)")
    # Unknown - ask
    return Classification("ask", description="mysql (unknown query)")
