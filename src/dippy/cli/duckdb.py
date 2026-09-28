"""duckdb handler for Dippy."""

from __future__ import annotations

import re

from dippy.cli import Classification, HandlerContext
from dippy.core.sql import _skip_cte, is_readonly_sql

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

_EXTERNAL_COMMANDS = frozenset({"COPY", "EXPORT", "IMPORT", "INSTALL", "LOAD"})


def _literal_shell_word(raw: str) -> bool:
    """Accept only one plain shell word with no concatenated quoting."""
    if len(raw) >= 2 and raw[0] in "'\"" and raw[-1] == raw[0]:
        quote = raw[0]
        inner = raw[1:-1]
        if quote == "'":
            return "'" not in inner
        return not any(char in inner for char in '"\\$`')
    return not any(char in raw for char in "'\"\\$`")


def _strip_duckdb_quoted(sql: str) -> str | None:
    """Blank DuckDB literals and comments; return None for ambiguous quoting."""
    if "\r" in sql:
        return None
    stripped = list(sql)
    i = 0
    while i < len(sql):
        if sql.startswith("--", i):
            end = i + 2
            while end < len(sql) and sql[end] not in "\r\n":
                end += 1
        elif sql.startswith("/*", i):
            end = sql.find("*/", i + 2)
            if end < 0 or "/*" in sql[i + 2 : end]:
                return None
            end += 2
        elif sql[i] in "'\"":
            quote = sql[i]
            if quote == "'" and i > 0 and sql[i - 1] in "Ee":
                return None
            end = i + 1
            while end < len(sql):
                if sql[end] == "\\":
                    return None
                if sql[end] == quote:
                    if end + 1 < len(sql) and sql[end + 1] == quote:
                        end += 2
                        continue
                    end += 1
                    break
                end += 1
            else:
                return None
        elif sql[i] == "$":
            if i > 0 and (sql[i - 1].isalnum() or sql[i - 1] in "_$"):
                return None
            delimiter = re.match(r"\$(?:[A-Za-z_]\w*)?\$", sql[i:])
            if not delimiter:
                return None
            end = sql.find(delimiter.group(), i + delimiter.end())
            if end < 0:
                return None
            end += delimiter.end()
        elif sql[i] in "[]":
            # DuckDB uses brackets for lists and indexing, not quoted names.
            stripped[i] = " "
            i += 1
            continue
        elif sql[i] == "`":
            return None
        else:
            i += 1
            continue
        stripped[i:end] = " " * (end - i)
        i = end
    return "".join(stripped)


def _needs_external_approval(sql: str) -> bool:
    """Find executable DuckDB commands without matching words in SQL data."""
    for statement in sql.split(";"):
        match = re.match(r"\s*([A-Za-z_]\w*)", statement)
        if not match:
            continue
        keyword = match.group(1).upper()
        if keyword == "WITH":
            match = re.match(
                r"([A-Za-z_]\w*)", statement[_skip_cte(statement, match.end()) :]
            )
            if not match:
                return True
            keyword = match.group(1).upper()
        if keyword == "EXPLAIN":
            # ANALYZE executes the explained statement; plain EXPLAIN stays on ask
            # when it names an external command, matching the old project guard.
            if re.search(
                r"\bANALYZE\b|\b(?:COPY|EXPORT|IMPORT|INSTALL|LOAD)\b", statement, re.I
            ):
                return True
        if keyword in _EXTERNAL_COMMANDS:
            return True
        if keyword == "ATTACH" and not re.search(r"\bREAD_ONLY\b", statement, re.I):
            return True
    return False


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


def _all_readonly_statements(sql: str) -> bool:
    """Require every nonempty SQL statement to be independently read-only."""
    statements = sql.split(";")
    if any(not statement.strip() for statement in statements[:-1]):
        return False
    return all(
        is_readonly_sql(statement, extra_write=_DUCKDB_WRITE) is True
        for statement in statements
        if statement.strip()
    ) and any(statement.strip() for statement in statements)


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
        not _literal_shell_word(ctx.raw_words[i])
        or (ctx.word_has_expansions and ctx.word_has_expansions[i])
        for i in sql_indices
    ):
        return Classification("ask", description="duckdb (ambiguous shell quoting)")

    # Quote state cannot safely span CLI arguments. Reject ambiguous fragments
    # before a generic SQL classifier could hide later statements.
    stripped_parts = [_strip_duckdb_quoted(part) for part in sql_parts]
    if any(part is None for part in stripped_parts):
        return Classification("ask", description="duckdb (ambiguous SQL quoting)")
    if any(part.lstrip().startswith(".") for part in sql_parts):
        return Classification("ask", description="duckdb (dot-command)")
    # Separate SQL arguments conservatively. A space can hide a later write
    # behind an initial SELECT during statement classification.
    sql = ";\n".join(part for part in stripped_parts if part is not None)

    if _needs_external_approval(sql):
        return Classification("ask", description="duckdb (external operation)")

    readonly = _all_readonly_statements(sql)
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
    if is_readonly_sql(sql, extra_write=_DUCKDB_WRITE) is False:
        return Classification("ask", description="duckdb (write query)")
    # Unknown - ask
    return Classification("ask", description="duckdb (unknown query)")
