"""SQL statement classification for Dippy.

Provides dialect-agnostic detection of read-only vs write SQL statements.
"""

from __future__ import annotations

import re
from collections import Counter

import sqlglot
from sqlglot import exp
from sqlglot.errors import ErrorLevel, ParseError, TokenError, UnsupportedError
from sqlglot.tokens import TokenType
from sqlglot.trie import new_trie

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
_UNSAFE_TOKENS = frozenset(
    {
        "ALTER",
        "ATTACH",
        "CALL",
        "COPY",
        "CREATE",
        "DELETE",
        "DETACH",
        "DROP",
        "DUMPFILE",
        "EXECUTE",
        "EXPORT",
        "GRANT",
        "IMPORT",
        "INSERT",
        "INSTALL",
        "INTO",
        "KILL",
        "LOAD",
        "MERGE",
        "OUTFILE",
        "PRAGMA",
        "REINDEX",
        "REPLACE",
        "REVOKE",
        "TRUNCATE",
        "UNLOAD",
        "UPDATE",
        "VACUUM",
    }
)
_MAIN_WRITE_PREFIXES = {
    "CREATE": (exp.Create, frozenset({"CREATE", "REPLACE"})),
    "INSERT": (exp.Insert, frozenset({"INSERT", "INTO"})),
    "UPDATE": (exp.Update, frozenset({"UPDATE"})),
    "DELETE": (exp.Delete, frozenset({"DELETE"})),
    "DROP": (exp.Drop, frozenset({"DROP"})),
    "ALTER": (exp.Alter, frozenset({"ALTER"})),
    "TRUNCATE": (exp.TruncateTable, frozenset({"TRUNCATE"})),
}

# Pure DuckDB built-ins missing from SQLGlot's typed function expressions.
# https://duckdb.org/docs/stable/sql/functions/interval
# https://duckdb.org/docs/stable/sql/functions/utility
# https://duckdb.org/docs/stable/sql/functions/list
_DUCKDB_READONLY_FUNCTIONS = frozenset(
    {
        "stats",
        "list_sum",
        "to_centuries",
        "to_days",
        "to_decades",
        "to_hours",
        "to_microseconds",
        "to_milliseconds",
        "to_minutes",
        "to_months",
        "to_nanoseconds",
        "to_seconds",
        "to_weeks",
        "to_years",
    }
)


class _TypeSpanParserMixin:
    """Record actual type grammar, excluding speculative nodes discarded by SQLGlot."""

    def _parse_types(self, *args, **kwargs):
        start = self._index
        node = super()._parse_types(*args, **kwargs)
        if isinstance(node, exp.DataType) and self._index > start:
            node.meta["dippy_type_span"] = (
                self._tokens[start].start,
                self._tokens[self._index - 1].end + 1,
            )
        return node

    def _parse_comparison(self):
        this = self._parse_range()
        while True:
            if self._match_set(self.COMPARISON):
                this = self.expression(
                    self.COMPARISON[self._prev.token_type],
                    this=this,
                    comments=self._prev_comments,
                    expression=self._parse_range(),
                )
            elif self._match(TokenType.OPERATOR) and self._prev.text in {"<<=", ">>="}:
                op = self._prev.text
                this = exp.Operator(
                    this=this,
                    expression=self._parse_range(),
                    operator=op,
                )
            else:
                break
        return this


def _register_inet_operators(dialect_class: type[sqlglot.Dialect]) -> None:
    dialect_class.Tokenizer.KEYWORDS["<<="] = TokenType.OPERATOR
    dialect_class.Tokenizer.KEYWORDS[">>="] = TokenType.OPERATOR
    dialect_class.Tokenizer._KEYWORD_TRIE = new_trie(
        key.upper()
        for key in (
            *dialect_class.Tokenizer.KEYWORDS,
            *dialect_class.Tokenizer._COMMENTS,
            *dialect_class.Tokenizer._QUOTES,
            *dialect_class.Tokenizer._FORMAT_STRINGS,
        )
        if " " in key
        or any(single in key for single in dialect_class.Tokenizer.SINGLE_TOKENS)
    )
    dialect_class.Generator.TRANSFORMS[exp.Operator] = (
        lambda self,
        e: f"{self.sql(e, 'this')} {e.args['operator']} {self.sql(e, 'expression')}"
    )
    if (
        hasattr(dialect_class.Parser, "RANGE_PARSERS")
        and TokenType.OPERATOR in dialect_class.Parser.RANGE_PARSERS
    ):
        orig_op = dialect_class.Parser.RANGE_PARSERS[TokenType.OPERATOR]

        def _custom_range_operator(
            self, this: exp.Expression | None
        ) -> exp.Expression | None:
            if self._prev and self._prev.text in {"<<=", ">>="}:
                op_text = self._prev.text
                return exp.Operator(
                    this=this,
                    expression=self._parse_bitwise(),
                    operator=op_text,
                )
            return orig_op(self, this)

        dialect_class.Parser.RANGE_PARSERS[TokenType.OPERATOR] = _custom_range_operator


_register_inet_operators(sqlglot.dialects.DuckDB)
_register_inet_operators(sqlglot.dialects.Postgres)


def _parse_sql(sql: str, dialect: str | None) -> list[exp.Expression | None]:
    source = sqlglot.Dialect.get_or_raise(dialect)
    parser_class = type(
        "_TypeSpanParser", (_TypeSpanParserMixin, source.parser_class), {}
    )
    return parser_class(dialect=source, error_level=ErrorLevel.RAISE).parse(
        source.tokenize(sql), sql
    )


def _normalized_type_words(
    masked: str, tree: exp.Expression, dialect: str | None
) -> Counter[str] | None:
    """Canonicalize only final-AST type spans for the syntax-retention check."""
    spans = sorted(
        (start, -end, node)
        for node in tree.find_all(exp.DataType)
        if "dippy_type_span" in node.meta
        for start, end in [node.meta["dippy_type_span"]]
    )
    replacements = []
    previous_end = -1
    for start, negative_end, node in spans:
        end = -negative_end
        if start >= previous_end:
            replacements.append((start, end, " __DIPPY_DATA_TYPE__ "))
            previous_end = end
    for start, end, replacement in reversed(replacements):
        masked = masked[:start] + replacement + masked[end:]
    normalized = _mask_sql(masked, bracket_identifiers=dialect != "duckdb")
    return _semantic_words(normalized) if normalized is not None else None


def _retains_syntax(masked: str, tree: exp.Expression, dialect: str | None) -> bool:
    # Parse the generated SQL too: CAST generators may render a type differently
    # from the standalone type generator (e.g. MySQL INT becomes SIGNED).
    regenerated = tree.sql(dialect=dialect)
    parsed = _parse_sql(regenerated, dialect)
    if len(parsed) != 1 or parsed[0] is None:
        return False
    regenerated_masked = _mask_sql(regenerated, bracket_identifiers=dialect != "duckdb")
    if regenerated_masked is None:
        return False
    original_words = _normalized_type_words(masked, tree, dialect)
    regenerated_words = _normalized_type_words(regenerated_masked, parsed[0], dialect)
    return (
        original_words is not None
        and regenerated_words is not None
        and not original_words - regenerated_words
    )


def _unverified_operation(node: exp.Expression, dialect: str | None) -> bool:
    if isinstance(node, (exp.DML, exp.DDL, exp.Command, exp.Into, exp.NextValueFor)):
        return True
    if isinstance(node, exp.DataType) and node.this == exp.DataType.Type.USERDEFINED:
        return True
    if isinstance(node, exp.Anonymous):
        return not (
            dialect == "duckdb"
            and isinstance(node.this, (str, exp.Identifier, exp.Var))
            and not isinstance(node.parent, exp.Dot)
            and node.name.lower() in _DUCKDB_READONLY_FUNCTIONS
        )
    return False


def _semantic_words(masked: str) -> Counter[str]:
    """Count words except function names, which dialect generators may rename."""
    words: Counter[str] = Counter()
    for match in _KEYWORD_PATTERN.finditer(masked):
        if masked[match.end() :].lstrip().startswith("("):
            continue
        words[match.group().upper()] += 1
    return words


def _verify_query_ast(sql: str, dialect: str | None) -> bool | None:
    """Require a complete read-only query tree, not just a SELECT prefix."""
    parser_dialect = dialect or (
        "mysql" if "`" in sql else "tsql" if "[" in sql else None
    )
    masked = _mask_sql(
        sql,
        bracket_identifiers=dialect != "duckdb",
        reject_executable_comments=parser_dialect == "mysql",
    )
    if masked is None:
        return None
    words = _semantic_words(masked)
    if words.keys() & _UNSAFE_TOKENS:
        return False
    try:
        statements = _parse_sql(sql, parser_dialect)
        if len(statements) != 1 or statements[0] is None:
            return None
        tree = statements[0]
        if not isinstance(
            tree.unnest(),
            (exp.Select, exp.Union, exp.Intersect, exp.Except, exp.Values),
        ):
            return None
        if any(_unverified_operation(node, dialect) for node in tree.walk()):
            return False
        return True if _retains_syntax(masked, tree, parser_dialect) else None
    except (ParseError, TokenError, UnsupportedError, ValueError):
        return None


def _main_table_target(node: exp.Expression) -> bool:
    """Check that a mutation target belongs to the invoked database."""
    if isinstance(node, exp.Schema):
        node = node.this
    if isinstance(node, exp.Index):
        node = node.args.get("table")
    if not isinstance(node, exp.Table) or node.catalog:
        return False
    return not node.db or node.db.lower() == "main"


def _verified_main_write(statement: str) -> bool:
    masked = _mask_sql(statement, bracket_identifiers=False)
    if masked is None:
        return False
    words = _semantic_words(masked)
    first = _KEYWORD_PATTERN.match(masked.lstrip())
    if first is None:
        return False
    root_keyword = first.group().upper()
    root = _MAIN_WRITE_PREFIXES.get(root_keyword)
    if root is None:
        return False
    expected_type, allowed_tokens = root
    if words.keys() & (_UNSAFE_TOKENS - allowed_tokens):
        return False
    try:
        statements = _parse_sql(statement, "duckdb")
        if len(statements) != 1 or statements[0] is None:
            return False
        tree = statements[0]
        if type(tree) is not expected_type:
            return False
        if isinstance(tree, (exp.Create, exp.Drop, exp.Alter)) and tree.args.get(
            "kind"
        ) not in {"TABLE", "VIEW", "INDEX"}:
            return False
        if isinstance(tree, exp.TruncateTable):
            if not tree.expressions or not all(
                _main_table_target(table) for table in tree.expressions
            ):
                return False
        elif not _main_table_target(tree.this):
            return False
        if any(
            _unverified_operation(node, "duckdb")
            for node in tree.walk()
            if node is not tree
        ):
            return False
        source = tree.args.get("expression")
        if isinstance(source, (exp.Select, exp.Union, exp.Intersect, exp.Except)):
            if _verify_query_ast(source.sql(dialect="duckdb"), "duckdb") is not True:
                return False
        return _retains_syntax(masked, tree, "duckdb")
    except (ParseError, TokenError, UnsupportedError, ValueError):
        return False


def duckdb_writes_only_main(sql: str) -> bool:
    """Verify each DuckDB statement reads or mutates only the main database."""
    statements = split_sql_statements(sql, bracket_identifiers=False)
    if not statements:
        return False
    for statement in statements:
        if is_readonly_sql(statement, dialect="duckdb", bracket_identifiers=False):
            continue
        if re.match(r"^ATTACH(?:\s+DATABASE)?\b", statement, re.IGNORECASE):
            if re.search(
                r"\(\s*[^()]*\bREAD_ONLY\b[^()]*\)\s*$", statement, re.IGNORECASE
            ):
                continue
            return False
        if not _verified_main_write(statement):
            return False
    return True


def duckdb_copy_export_target(sql: str) -> str | None:
    """Verify a single-file CSV query export; its target still needs approval."""
    statements = split_sql_statements(sql, bracket_identifiers=False)
    if statements is None or len(statements) != 1:
        return None
    masked = _mask_sql(statements[0], bracket_identifiers=False)
    if (
        masked is None
        or not re.match(r"\s*COPY\s*\(", masked, re.IGNORECASE)
        or _semantic_words(masked).keys() & (_UNSAFE_TOKENS - {"COPY"})
    ):
        return None
    try:
        # Gate unsupported COPY targets before parsing: SQLGlot can loop on
        # unquoted paths. Tokenization also handles comments between TO/path.
        tokens = sqlglot.tokenize(statements[0], read="duckdb")
        depth = 0
        for i, token in enumerate(tokens[1:], start=1):
            if token.token_type == TokenType.L_PAREN:
                depth += 1
            elif token.token_type == TokenType.R_PAREN:
                depth -= 1
                if depth == 0:
                    if (
                        i + 2 >= len(tokens)
                        or tokens[i + 1].text.upper() != "TO"
                        or tokens[i + 2].token_type != TokenType.STRING
                    ):
                        return None
                    break
        else:
            return None
        parsed = _parse_sql(statements[0], "duckdb")
        if len(parsed) != 1 or parsed[0] is None:
            return None
        tree = parsed[0]
        if not isinstance(tree, exp.Copy) or tree.args.get("kind") is not False:
            return None
        if set(tree.args) - {"this", "kind", "credentials", "files", "params"}:
            return None
        credentials = tree.args.get("credentials")
        if credentials is not None and credentials.args:
            return None
        if not isinstance(tree.this, exp.Subquery):
            return None
        if (
            _verify_query_ast(tree.this.this.sql(dialect="duckdb"), "duckdb")
            is not True
        ):
            return None
        files = tree.args.get("files") or []
        if (
            len(files) != 1
            or not isinstance(files[0], exp.Literal)
            or not files[0].is_string
        ):
            return None
        target = files[0].this
        if (
            not target.strip()
            or target == "-"
            or any(char in target for char in ":*?[]")
            or any(ord(char) < 32 or ord(char) == 127 for char in target)
            or target.startswith(("/dev/", "/proc/"))
        ):
            return None
        seen = set()
        for param in tree.args.get("params") or []:
            if not isinstance(param, exp.CopyParameter) or not isinstance(
                param.this, exp.Var
            ):
                return None
            name = param.this.name.upper()
            value = param.args.get("expression")
            if name in seen:
                return None
            seen.add(name)
            if name == "HEADER":
                if not isinstance(value, exp.Boolean):
                    return None
            elif name == "FORMAT":
                if (
                    not isinstance(value, (exp.Var, exp.Literal))
                    or value.name.upper() != "CSV"
                ):
                    return None
            elif name in {"DELIMITER", "QUOTE", "ESCAPE", "NULL"}:
                if not isinstance(value, exp.Literal) or not value.is_string:
                    return None
            else:
                return None
        if not _retains_syntax(masked, tree, "duckdb"):
            return None
    except (ParseError, TokenError, UnsupportedError, ValueError):
        return None
    return target


def _mask_sql(
    sql: str, *, bracket_identifiers: bool, reject_executable_comments: bool = False
) -> str | None:
    """Blank quoted data and comments, preserving separator positions."""
    if "\r" in sql:
        return None
    masked = list(sql)
    i = 0
    while i < len(sql):
        start = i
        if sql.startswith("--", i) and (
            not reject_executable_comments or i + 2 == len(sql) or sql[i + 2] <= " "
        ):
            i = sql.find("\n", i)
            if i < 0:
                i = len(sql)
        elif sql.startswith("/*", i):
            if reject_executable_comments and (
                sql.startswith("/*!", i) or sql[i : i + 4].upper() == "/*M!"
            ):
                return None
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
    sql: str,
    *,
    bracket_identifiers: bool = True,
    reject_executable_comments: bool = False,
) -> list[str] | None:
    """Split SQL on executable semicolons; return None for ambiguous syntax."""
    masked = _mask_sql(
        sql,
        bracket_identifiers=bracket_identifiers,
        reject_executable_comments=reject_executable_comments,
    )
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
    """Skip over CTE definitions (name [(cols)] AS (...), ...) to find main statement."""
    length = len(sql)
    while pos < length:
        pos = _skip_whitespace(sql, pos)
        if pos >= length:
            break
        m = _KEYWORD_PATTERN.match(sql, pos)
        if not m:
            break
        kw = m.group().upper()
        if kw == "RECURSIVE":
            pos = _skip_whitespace(sql, m.end())
            continue
        # CTE name
        pos = _skip_whitespace(sql, m.end())
        # Optional column list: (col1, col2, ...)
        if pos < length and sql[pos] == "(":
            depth = 1
            pos += 1
            while pos < length and depth > 0:
                if sql[pos] == "(":
                    depth += 1
                elif sql[pos] == ")":
                    depth -= 1
                pos += 1
            pos = _skip_whitespace(sql, pos)
        # Expect AS
        m_as = _KEYWORD_PATTERN.match(sql, pos)
        if not m_as or m_as.group().upper() != "AS":
            break
        pos = _skip_whitespace(sql, m_as.end())
        # Expect CTE body in parens
        if pos < length and sql[pos] == "(":
            depth = 1
            pos += 1
            while pos < length and depth > 0:
                if sql[pos] == "(":
                    depth += 1
                elif sql[pos] == ")":
                    depth -= 1
                pos += 1
            pos = _skip_whitespace(sql, pos)
        else:
            break
        # If comma, another CTE follows
        if pos < length and sql[pos] == ",":
            pos += 1
            continue
        return pos
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
    dialect: str | None = None,
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
        dialect: SQLGlot source dialect for structural verification.

    Returns:
        True: Statement is definitely read-only (safe to auto-approve).
        False: Statement is definitely a write operation.
        None: Unknown or ambiguous (caller should prompt user).

    Notes:
        - Multiple statements return None unless allow_multiple is enabled.
        - CTEs (WITH ... AS) are handled by analyzing the main statement.
        - Unknown functions ask; named database functions may still have effects
          that cannot be inferred from SQL syntax alone.
    """
    reject_executable_comments = dialect == "mysql" or (dialect is None and "`" in sql)
    statements = split_sql_statements(
        sql,
        bracket_identifiers=bracket_identifiers,
        reject_executable_comments=reject_executable_comments,
    )
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
                dialect=dialect,
            )
            for statement in statements
        ]
        if any(result is None for result in results):
            return None
        return all(results)

    # Strip quoted content for keyword detection
    stripped = _mask_sql(
        statements[0],
        bracket_identifiers=bracket_identifiers,
        reject_executable_comments=reject_executable_comments,
    )
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
            try:
                created = sqlglot.parse_one(
                    statements[0], read=dialect, error_level=ErrorLevel.RAISE
                )
            except (ParseError, ValueError):
                return None
            if (
                not isinstance(created, exp.Create)
                or created.args.get("kind") != "TABLE"
            ):
                return None
            properties = created.args.get("properties")
            if not properties or not any(
                isinstance(prop, exp.TemporaryProperty)
                for prop in properties.expressions
            ):
                return None
            return is_readonly_sql(
                query,
                extra_readonly=extra_readonly,
                extra_write=extra_write,
                bracket_identifiers=bracket_identifiers,
                dialect=dialect,
            )

    readonly_keywords = _READONLY_KEYWORDS | extra_readonly
    write_keywords = _WRITE_KEYWORDS | extra_write

    if stripped.lstrip().startswith("("):
        return _verify_query_ast(statements[0], dialect)

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
        if kw in {"FROM", "VALUES"} and dialect == "duckdb":
            return _verify_query_ast(statements[0], dialect)
        if kw == "SELECT":
            # Check for SELECT INTO (write operation)
            if _check_select_into(stripped, m.end()):
                return False
            return _verify_query_ast(statements[0], dialect)
        if kw == "EXPLAIN":
            rest = statements[0][m.end() :].lstrip()
            analyze = re.match(r"ANALYZE\b", rest, re.IGNORECASE)
            if analyze:
                rest = rest[analyze.end() :].lstrip()
            plan = re.match(r"PLAN\s+FOR\b", rest, re.IGNORECASE)
            if plan:
                rest = rest[plan.end() :].lstrip()
            return is_readonly_sql(
                rest,
                extra_readonly=extra_readonly,
                extra_write=extra_write,
                bracket_identifiers=bracket_identifiers,
                dialect=dialect,
            )
        if kw in readonly_keywords:
            if kw in {"SHOW", "DESCRIBE"} and (
                _semantic_words(stripped).keys() & _UNSAFE_TOKENS
            ):
                return False
            return True
        if kw in write_keywords:
            return False
        return None
    return None
