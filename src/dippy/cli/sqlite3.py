"""sqlite3 handler for Dippy."""

from __future__ import annotations


from dippy.cli import Classification, HandlerContext
from dippy.cli.sql_args import literal_sql_arg
from dippy.core.sql import is_readonly_sql

COMMANDS = ["sqlite3"]

# SQLite-specific keywords that perform writes or modifications
_SQLITE_WRITE = frozenset(
    {"PRAGMA", "ATTACH", "DETACH", "VACUUM", "REINDEX", "ANALYZE"}
)


def classify(ctx: HandlerContext) -> Classification:
    tokens = ctx.tokens
    # Help/version
    if any(t in ("-help", "--help", "-version") for t in tokens):
        return Classification("allow", description="sqlite3 help/version")

    # Check for -init (runs a script file - unknown content)
    if "-init" in tokens:
        return Classification("ask", description="sqlite3 (init script)")

    # Extract SQL from command line
    # sqlite3 [OPTIONS] [FILENAME [SQL...]]
    # Also check -cmd COMMAND
    sql_indices: list[int] = []
    i = 1
    filename_seen = False
    while i < len(tokens):
        token = tokens[i]
        # Skip option flags that take no argument
        if token in (
            "-append",
            "-ascii",
            "-bail",
            "-batch",
            "-box",
            "-column",
            "-csv",
            "-deserialize",
            "-echo",
            "-header",
            "-noheader",
            "-help",
            "-html",
            "-interactive",
            "-json",
            "-line",
            "-list",
            "-markdown",
            "-memtrace",
            "-nofollow",
            "-quote",
            "-readonly",
            "-safe",
            "-stats",
            "-table",
            "-tabs",
            "-version",
            "-vfstrace",
        ):
            i += 1
            continue
        # Skip option flags that take one argument
        if token in (
            "-cmd",
            "-init",
            "-key",
            "-hexkey",
            "-textkey",
            "-maxsize",
            "-newline",
            "-nonce",
            "-nullvalue",
            "-pagecache",
            "-separator",
            "-vfs",
            "-escape",
            "-A",
        ):
            if token == "-cmd" and i + 1 < len(tokens):
                sql_indices.append(i + 1)
            i += 2
            continue
        # -lookaside takes TWO arguments: SIZE N
        if token == "-lookaside":
            i += 3
            continue
        # This should be either filename or SQL
        if token.startswith("-"):
            # Unknown option
            i += 1
            continue
        if not filename_seen:
            # First non-option is filename (could be :memory: or a path)
            filename_seen = True
            i += 1
            continue
        # Everything after filename is SQL
        sql_indices.append(i)
        i += 1

    # No SQL found - interactive mode
    if not sql_indices:
        return Classification("ask", description="sqlite3 (interactive)")

    sql_parts = [literal_sql_arg(ctx, i) for i in sql_indices]
    if any(part is None for part in sql_parts):
        return Classification("ask", description="sqlite3 (ambiguous SQL argument)")
    if any(part.lstrip().startswith(".") for part in sql_parts if part is not None):
        return Classification("ask", description="sqlite3 (dot-command)")
    sql = ";\n".join(part for part in sql_parts if part is not None)

    # Analyze SQL
    readonly = is_readonly_sql(sql, extra_write=_SQLITE_WRITE, dialect="sqlite")
    if readonly is True:
        return Classification("allow", description="sqlite3 (read-only query)")
    if readonly is False:
        return Classification("ask", description="sqlite3 (write query)")
    # Unknown - ask
    return Classification("ask", description="sqlite3 (unknown query)")
