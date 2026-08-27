"""duckdb handler for Dippy."""

from __future__ import annotations

import re

from dippy.cli import Classification, HandlerContext
from dippy.core.sql import is_readonly_sql

COMMANDS = ["duckdb"]

# DuckDB-specific keywords that perform writes or modifications
_DUCKDB_WRITE = frozenset(
    {"PRAGMA", "ATTACH", "DETACH", "VACUUM", "COPY", "EXPORT", "IMPORT"}
)

_LOCAL_WRITE = re.compile(
    r"^(?:"
    r"CREATE\s+(?:OR\s+REPLACE\s+)?(?:TEMP(?:ORARY)?\s+)?"
    r"(?:TABLE|VIEW|INDEX|SCHEMA|SEQUENCE|TYPE|MACRO)\b"
    r"|(?:ALTER|DROP)\s+(?:TABLE|VIEW|INDEX|SCHEMA|SEQUENCE|TYPE|MACRO)\b"
    r"|INSERT\b|UPDATE\b|DELETE\b|TRUNCATE\b|MERGE\b|REPLACE\b"
    r"|VACUUM\b|DETACH\b"
    r")",
    re.IGNORECASE,
)


def _writes_only_to_main_database(sql: str) -> bool:
    """Return whether every statement is read-only or writes only main DB state."""
    statements = [
        statement.strip() for statement in sql.split(";") if statement.strip()
    ]
    if not statements:
        return False

    for statement in statements:
        readonly = is_readonly_sql(statement, extra_write=_DUCKDB_WRITE)
        if readonly is True:
            continue
        if re.match(r"^ATTACH(?:\s+DATABASE)?\b", statement, re.IGNORECASE):
            if re.search(
                r"\(\s*[^()]*\bREAD_ONLY\b[^()]*\)\s*$", statement, re.IGNORECASE
            ):
                continue
            return False
        if readonly is False and _LOCAL_WRITE.match(statement):
            continue
        return False
    return True


def classify(ctx: HandlerContext) -> Classification:
    tokens = ctx.tokens

    # Help/version
    if any(t in ("-help", "--help", "-version") for t in tokens):
        return Classification("allow", description="duckdb help/version")

    # Check for -readonly or -safe flags - always safe
    if "-readonly" in tokens or "-safe" in tokens:
        return Classification("allow", description="duckdb (read-only mode)")

    # Check for -init (runs a script file - unknown content)
    if "-init" in tokens:
        return Classification("ask", description="duckdb (init script)")

    # Extract SQL from command line
    # duckdb [OPTIONS] [FILENAME [SQL...]]
    # Also check -c, -s, -cmd options
    sql_parts: list[str] = []
    i = 1
    filename_seen = False
    filename: str | None = None
    filename_has_expansions = False
    while i < len(tokens):
        token = tokens[i]
        # Skip option flags that take no argument
        if token in (
            "-ascii",
            "-bail",
            "-batch",
            "-box",
            "-column",
            "-csv",
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
            "-no-stdin",
            "-quote",
            "-readonly",
            "-safe",
            "-stats",
            "-table",
            "-tabs",
            "-unredacted",
            "-unsigned",
            "-version",
        ):
            i += 1
            continue
        # Skip option flags that take one argument
        if token in (
            "-cmd",
            "-init",
            "-separator",
            "-vfs",
            "-storage-version",
            "-newline",
            "-nullvalue",
        ):
            if token == "-cmd" and i + 1 < len(tokens):
                sql_parts.append(tokens[i + 1])
            i += 2
            continue
        # -c and -s run SQL and exit
        if token in ("-c", "-s") and i + 1 < len(tokens):
            sql_parts.append(tokens[i + 1])
            i += 2
            continue
        # This should be either filename or SQL
        if token.startswith("-"):
            # Unknown option
            i += 1
            continue
        if not filename_seen:
            # First non-option is filename (could be :memory: or a path)
            filename_seen = True
            filename = token
            filename_has_expansions = bool(
                ctx.word_has_expansions and ctx.word_has_expansions[i]
            )
            i += 1
            continue
        # Everything after filename is SQL
        sql_parts.append(token)
        i += 1

    # No SQL found - interactive mode
    if not sql_parts:
        return Classification("ask", description="duckdb (interactive)")

    # Combine SQL parts
    sql = " ".join(sql_parts)

    # Analyze SQL
    readonly = is_readonly_sql(sql, extra_write=_DUCKDB_WRITE)
    if readonly is True:
        return Classification("allow", description="duckdb (read-only query)")
    if filename and not filename_has_expansions and _writes_only_to_main_database(sql):
        return Classification(
            "allow",
            description="duckdb (database write)",
            redirect_targets=(filename,),
        )
    if readonly is False:
        return Classification("ask", description="duckdb (write query)")
    # Unknown - ask
    return Classification("ask", description="duckdb (unknown query)")
