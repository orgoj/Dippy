"""duckdb handler for Dippy."""

from __future__ import annotations

import re

from dippy.cli import Classification, HandlerContext
from dippy.core.sql import is_readonly_sql, split_sql_statements

COMMANDS = ["duckdb"]

# DuckDB-specific keywords that perform writes or modifications
_DUCKDB_WRITE = frozenset(
    {
        "PRAGMA",
        "ATTACH",
        "DETACH",
        "VACUUM",
        "COPY",
        "EXPORT",
        "IMPORT",
        "INSTALL",
        "LOAD",
    }
)


def _decode_shell_word(raw: str) -> str | None:
    """Apply Bash quote removal to a literal word; reject shell expansion."""
    output: list[str] = []
    quote = ""
    i = 0
    while i < len(raw):
        char = raw[i]
        if quote == "'":
            if char == "'":
                quote = ""
            else:
                output.append(char)
        elif char == quote and quote:
            quote = ""
        elif char in "'\"" and not quote:
            quote = char
        elif char == "$":
            if quote != '"' or (
                i + 1 < len(raw)
                and (raw[i + 1].isalnum() or raw[i + 1] in "_({[*@#?-$!")
            ):
                return None
            output.append(char)
        elif char == "`":
            return None
        elif char == "\\":
            if i + 1 >= len(raw):
                return None
            following = raw[i + 1]
            if quote == '"' and following not in '"\\$`\n':
                output.append(char)
            else:
                if following != "\n":
                    output.append(following)
                i += 1
        else:
            output.append(char)
        i += 1
    return "".join(output) if not quote else None


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
    statements = split_sql_statements(sql, bracket_identifiers=False)
    if not statements:
        return False

    for statement in statements:
        readonly = is_readonly_sql(
            statement, extra_write=_DUCKDB_WRITE, bracket_identifiers=False
        )
        if readonly is True:
            continue
        if re.match(r"^ATTACH(?:\s+DATABASE)?\b", statement, re.IGNORECASE):
            if re.search(
                r"\(\s*[^()]*\bREAD_ONLY\b[^()]*\)\s*$", statement, re.IGNORECASE
            ):
                continue
            return False
        if readonly is False and _LOCAL_WRITE.match(statement.strip()):
            continue
        return False
    return True


def classify(ctx: HandlerContext) -> Classification:
    tokens = ctx.tokens

    # Help/version
    if any(t in ("-help", "--help", "-version") for t in tokens):
        return Classification("allow", description="duckdb help/version")

    # Check for -init (runs a script file - unknown content)
    if "-init" in tokens:
        return Classification("ask", description="duckdb (init script)")

    # Extract SQL from command line
    # duckdb [OPTIONS] [FILENAME [SQL...]]
    # Also check -c, -s, -cmd options
    sql_parts: list[str] = []
    sql_indices: list[int] = []
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
                sql_indices.append(i + 1)
            i += 2
            continue
        # -c and -s run SQL and exit
        if token in ("-c", "-s") and i + 1 < len(tokens):
            sql_parts.append(tokens[i + 1])
            sql_indices.append(i + 1)
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
        sql_indices.append(i)
        i += 1

    # No SQL found - interactive mode
    if not sql_parts:
        return Classification("ask", description="duckdb (interactive)")

    if len(ctx.raw_words) != len(tokens) or any(
        ctx.word_has_expansions and ctx.word_has_expansions[i] for i in sql_indices
    ):
        return Classification("ask", description="duckdb (ambiguous shell quoting)")
    sql_parts = [_decode_shell_word(ctx.raw_words[i]) for i in sql_indices]
    if any(part is None for part in sql_parts):
        return Classification("ask", description="duckdb (ambiguous shell quoting)")
    if any(part.lstrip().startswith(".") for part in sql_parts):
        return Classification("ask", description="duckdb (dot-command)")
    # Separate SQL arguments conservatively. A space can hide a later write
    # behind an initial SELECT during statement classification.
    sql = ";\n".join(part for part in sql_parts if part is not None)
    readonly = is_readonly_sql(
        sql,
        extra_write=_DUCKDB_WRITE,
        allow_multiple=True,
        allow_temp_tables="-readonly" in tokens or "-safe" in tokens,
        bracket_identifiers=False,
    )
    if "-readonly" in tokens or "-safe" in tokens:
        if readonly:
            return Classification("allow", description="duckdb (read-only query)")
        return Classification("ask", description="duckdb (unverified read-only query)")

    # Analyze SQL
    if readonly:
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
