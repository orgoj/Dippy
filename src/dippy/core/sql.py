"""SQL statement classification for Dippy.

Provides dialect-agnostic detection of read-only vs write SQL statements.
"""

from __future__ import annotations

import re

_WHITESPACE_PATTERN = re.compile(r"\s+")
_KEYWORD_PATTERN = re.compile(r"[A-Za-z_]\w*")

_READONLY_KEYWORDS = frozenset({"SELECT", "SHOW", "DESCRIBE", "EXPLAIN"})
_WRITE_KEYWORDS = frozenset(
    {
        "INSERT",
        "CREATE",
        "ALTER",
        "DROP",
        "TRUNCATE",
        "DELETE",
        "UPDATE",
        "MERGE",
        "GRANT",
        "REVOKE",
        "REPLACE",
    }
)
_TEMP_TABLE_AS_SELECT = re.compile(
    r"CREATE\s+(?:OR\s+REPLACE\s+)?TEMP(?:ORARY)?\s+TABLE\s+"
    r"(?:IF\s+NOT\s+EXISTS\s+)?(?:temp\.)?[A-Za-z_]\w*\s+AS\s+",
    re.IGNORECASE,
)


def _mask_sql(sql: str, *, bracket_identifiers: bool) -> str | None:
    """Blank quoted data and comments, preserving separator positions."""
    if "\r" in sql:
        return None
    masked = list(sql)
    i = 0
    while i < len(sql):
        start = i
        if sql.startswith("--", i):
            i = sql.find("\n", i)
            if i < 0:
                i = len(sql)
        elif sql.startswith("/*", i):
            i = sql.find("*/", i + 2)
            if i < 0 or "/*" in sql[start + 2 : i]:
                return None
            i += 2
        elif sql[i] in "'\"`" or (bracket_identifiers and sql[i] == "["):
            quote = "]" if sql[i] == "[" else sql[i]
            if (
                quote == "'"
                and i > 0
                and sql[i - 1] in "Ee"
                and (i == 1 or not (sql[i - 2].isalnum() or sql[i - 2] == "_"))
            ):
                return None
            i += 1
            while i < len(sql):
                if sql[i] == quote:
                    if i + 1 < len(sql) and sql[i + 1] == quote:
                        i += 2
                        continue
                    i += 1
                    break
                i += 1
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
            i = end + delimiter.end()
        else:
            i += 1
            continue
        masked[start:i] = " " * (i - start)
    return "".join(masked)


def split_sql_statements(
    sql: str, *, bracket_identifiers: bool = True
) -> list[str] | None:
    """Split SQL on executable semicolons; return None for ambiguous syntax."""
    masked = _mask_sql(sql, bracket_identifiers=bracket_identifiers)
    if masked is None:
        return None
    ends = [i for i, char in enumerate(masked) if char == ";"]
    parts = []
    start = 0
    for end in ends:
        parts.append(sql[start:end])
        start = end + 1
    parts.append(sql[start:])
    masked_parts = []
    start = 0
    for end in ends:
        masked_parts.append(masked[start:end])
        start = end + 1
    masked_parts.append(masked[start:])
    nonempty = [i for i, part in enumerate(masked_parts) if part.strip()]
    if not nonempty or any(not masked_parts[i].strip() for i in range(nonempty[-1])):
        return None
    if any(part and not part.strip() for part in masked_parts[nonempty[-1] + 1 : -1]):
        return None
    return [parts[i] for i in nonempty]


def _skip_whitespace(sql: str, pos: int) -> int:
    """Skip whitespace at position."""
    m = _WHITESPACE_PATTERN.match(sql, pos)
    return m.end() if m else pos


def _skip_cte(sql: str, pos: int) -> int:
    """Skip over CTE definitions (name AS (...), ...) to find main statement."""
    length = len(sql)
    expect_as = True  # After WITH/comma, expect: name AS (...)
    while pos < length:
        pos = _skip_whitespace(sql, pos)
        if pos >= length:
            break
        # Check for opening paren - skip balanced parens
        if sql[pos] == "(":
            depth = 1
            pos += 1
            while pos < length and depth > 0:
                if sql[pos] == "(":
                    depth += 1
                elif sql[pos] == ")":
                    depth -= 1
                pos += 1
            expect_as = False
            continue
        # Check for comma (another CTE follows)
        if sql[pos] == ",":
            pos += 1
            expect_as = True
            continue
        # Check for identifier/keyword
        m = _KEYWORD_PATTERN.match(sql, pos)
        if m:
            kw = m.group().upper()
            if expect_as:
                pos = m.end()
                if kw == "AS":
                    expect_as = False
                elif kw == "RECURSIVE":
                    pass  # WITH RECURSIVE - still expect CTE name
                continue
            # Not expecting AS - this should be the main statement keyword
            return pos
        pos += 1
    return pos


def _check_select_into(sql: str, pos: int) -> bool:
    """Check if SELECT statement contains INTO (making it a write operation)."""
    # Scan forward looking for INTO before FROM
    length = len(sql)
    while pos < length:
        pos = _skip_whitespace(sql, pos)
        if pos >= length:
            break
        m = _KEYWORD_PATTERN.match(sql, pos)
        if m:
            kw = m.group().upper()
            if kw == "INTO":
                return True
            if kw == "FROM":
                return False
            pos = m.end()
            continue
        # Skip other characters (*, columns, etc.)
        pos += 1
    return False


def is_readonly_sql(
    sql: str,
    *,
    extra_readonly: frozenset[str] = frozenset(),
    extra_write: frozenset[str] = frozenset(),
    allow_multiple: bool = False,
    allow_temp_tables: bool = False,
    bracket_identifiers: bool = True,
) -> bool | None:
    """
    Determine if a SQL statement is read-only.

    Args:
        sql: The SQL statement to analyze.
        extra_readonly: Additional keywords to treat as read-only (dialect-specific).
        extra_write: Additional keywords to treat as write operations (dialect-specific).
        allow_multiple: Verify every statement in a batch independently.
        allow_temp_tables: Verify session-local CREATE TEMP TABLE AS SELECT.
        bracket_identifiers: Interpret brackets as SQL Server quoted identifiers.

    Returns:
        True: Statement is definitely read-only (safe to auto-approve).
        False: Statement is definitely a write operation.
        None: Unknown or ambiguous (caller should prompt user).

    Notes:
        - Multiple statements return None unless allow_multiple is enabled.
        - CTEs (WITH ... AS) are handled by analyzing the main statement.
        - Side-effect functions (e.g., SQLite's writefile) are NOT detected.
    """
    statements = split_sql_statements(sql, bracket_identifiers=bracket_identifiers)
    if statements is None or (len(statements) > 1 and not allow_multiple):
        return None
    if len(statements) > 1:
        results = [
            is_readonly_sql(
                statement,
                extra_readonly=extra_readonly,
                extra_write=extra_write,
                allow_temp_tables=allow_temp_tables,
                bracket_identifiers=bracket_identifiers,
            )
            for statement in statements
        ]
        if any(result is None for result in results):
            return None
        return all(results)

    # Strip quoted content for keyword detection
    stripped = _mask_sql(statements[0], bracket_identifiers=bracket_identifiers)
    if stripped is None:
        return None

    if allow_temp_tables:
        match = _TEMP_TABLE_AS_SELECT.match(stripped.lstrip())
        if match:
            offset = len(stripped) - len(stripped.lstrip())
            query = statements[0][offset + match.end() :]
            query_start = stripped[offset + match.end() :].lstrip()
            if not re.match(r"(?:SELECT|WITH)\b", query_start, re.IGNORECASE):
                return False
            return is_readonly_sql(
                query,
                extra_readonly=extra_readonly,
                extra_write=extra_write,
                bracket_identifiers=bracket_identifiers,
            )

    readonly_keywords = _READONLY_KEYWORDS | extra_readonly
    write_keywords = _WRITE_KEYWORDS | extra_write

    pos = 0
    while pos < len(stripped):
        pos = _skip_whitespace(stripped, pos)
        if pos >= len(stripped):
            break
        m = _KEYWORD_PATTERN.match(stripped, pos)
        if not m:
            return None
        kw = m.group().upper()
        if kw == "WITH":
            pos = _skip_cte(stripped, m.end())
            continue
        if kw == "SELECT":
            # Check for SELECT INTO (write operation)
            if _check_select_into(stripped, m.end()):
                return False
            return True
        if kw == "EXPLAIN":
            rest = stripped[m.end() :].lstrip()
            analyze = re.match(r"ANALYZE\b", rest, re.IGNORECASE)
            if analyze:
                return is_readonly_sql(
                    rest[analyze.end() :],
                    extra_readonly=extra_readonly,
                    extra_write=extra_write,
                    bracket_identifiers=bracket_identifiers,
                )
            if any(
                re.search(rf"\b{re.escape(word)}\b", rest, re.IGNORECASE)
                for word in extra_write
            ):
                return None
            return True
        if kw in readonly_keywords:
            return True
        if kw in write_keywords:
            return False
        return None
    return None
