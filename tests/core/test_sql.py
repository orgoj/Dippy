"""Comprehensive tests for SQL statement classification."""

import pytest

from dippy.core.sql import (
    duckdb_copy_export_target,
    duckdb_writes_only_main,
    is_readonly_sql,
    split_sql_statements,
)


@pytest.mark.parametrize(
    "dialect", ["duckdb", "postgres", "mysql", "sqlite", "tsql", "athena"]
)
@pytest.mark.parametrize(
    "sql",
    [
        "(SELECT 1)",
        "((SELECT 1))",
        "(SELECT 1) UNION ALL (SELECT 2)",
        "((SELECT 1) UNION ALL (SELECT 2))",
        "(WITH x AS (SELECT 1 a) SELECT a FROM x)",
    ],
)
def test_parenthesized_reads(sql, dialect):
    assert is_readonly_sql(sql, dialect=dialect) is True


@pytest.mark.parametrize(
    "dialect", ["duckdb", "postgres", "mysql", "sqlite", "tsql", "athena"]
)
@pytest.mark.parametrize(
    "sql",
    [
        "(SELECT fictional_effect())",
        "(SELECT 1) UNION ALL (SELECT fictional_effect())",
        "(WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x)",
        "(SELECT 1 INTO outfile)",
        "(SELECT 1); DROP TABLE t",
        "(DELETE FROM t)",
        "(1 + 1)",
        "(SELECT 1) fictional_unknown_clause",
        "(SELECT 1",
    ],
)
def test_parenthesized_unverified_operations(sql, dialect):
    assert is_readonly_sql(sql, dialect=dialect, allow_multiple=True) is not True


def test_duckdb_sampled_union_is_readonly():
    assert (
        is_readonly_sql(
            "(SELECT * FROM t USING SAMPLE 8 ROWS) UNION ALL "
            "(SELECT * FROM t USING SAMPLE 5 ROWS)",
            dialect="duckdb",
        )
        is True
    )


def test_mysql_double_dash_without_whitespace_cannot_hide_executable_comment():
    sql = "SELECT 1--1 /*!50000 INTO OUTFILE 'tmp/out' */"
    assert split_sql_statements(sql, reject_executable_comments=True) is None


@pytest.mark.parametrize("marker", ["!", "M!", "m!"])
@pytest.mark.parametrize(
    "sql",
    [
        "SELECT 1 /*{marker}50000 INTO OUTFILE 'tmp/out' */",
        "SELECT 1 /*{marker}50000 , fictional_effect() */",
        "WITH x AS (SELECT 1 /*{marker}50000 , fictional_effect() */) SELECT * FROM x",
        "EXPLAIN SELECT 1 /*{marker}50000 INTO OUTFILE 'tmp/out' */",
        "SHOW TABLES /*{marker}50000 INTO OUTFILE 'tmp/out' */",
        "SELECT 1; SELECT 2 /*{marker}50000 INTO OUTFILE 'tmp/out' */",
    ],
)
def test_mysql_executable_comments_are_not_readonly(marker, sql):
    assert (
        is_readonly_sql(sql.format(marker=marker), dialect="mysql", allow_multiple=True)
        is not True
    )


def test_inferred_mysql_dialect_also_rejects_executable_comments():
    assert (
        is_readonly_sql("SELECT `x` FROM t /*!50000 INTO OUTFILE 'tmp/out' */")
        is not True
    )


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT '/*!50000 INTO OUTFILE */'",
        "SELECT '/*M!50000 INTO OUTFILE */'",
        "SELECT 1 /* ordinary INTO OUTFILE 'tmp/out' */",
        "SELECT 1 -- ordinary INTO OUTFILE 'tmp/out'",
    ],
)
def test_mysql_literal_comment_markers_and_ordinary_comments_allow(sql):
    assert is_readonly_sql(sql, dialect="mysql") is True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT NEXT VALUE FOR dbo.seq",
        "SELECT NEXT VALUE FOR dbo.seq OVER (ORDER BY id) FROM t",
        "SELECT (SELECT NEXT VALUE FOR dbo.seq)",
        "WITH x AS (SELECT NEXT VALUE FOR dbo.seq n) SELECT * FROM x",
        "EXPLAIN SELECT NEXT VALUE FOR dbo.seq",
    ],
)
def test_sequence_increment_is_not_readonly(sql):
    assert is_readonly_sql(sql, dialect="tsql") is False


def test_sequence_words_in_literal_allow():
    assert is_readonly_sql("SELECT 'NEXT VALUE FOR dbo.seq'", dialect="tsql") is True


@pytest.mark.parametrize("expression", ["stats(x)", "list_sum([1,2,3])"])
def test_duckdb_pure_builtins_in_all_query_contexts(expression):
    sql = f"SELECT {expression} FROM t"
    assert is_readonly_sql(sql, dialect="duckdb") is True
    assert duckdb_writes_only_main(f"CREATE TABLE main.result AS {sql}") is True
    assert (
        is_readonly_sql(
            f"CREATE TEMP TABLE result AS {sql}",
            dialect="duckdb",
            allow_temp_tables=True,
        )
        is True
    )
    assert duckdb_copy_export_target(f"COPY ({sql}) TO 'tmp/out.csv'") == "tmp/out.csv"


@pytest.mark.parametrize("name", ["stats", "list_sum"])
@pytest.mark.parametrize(
    "expression",
    [
        "evil.{name}(1)",
        '"evil"."{name}"(1)',
        "{name}(query('DROP TABLE t'))",
        "{name}(writefile('tmp/out','data'))",
    ],
)
def test_duckdb_pure_builtin_names_do_not_hide_effects(name, expression):
    sql = f"SELECT {expression.format(name=name)}"
    assert is_readonly_sql(sql, dialect="duckdb") is not True
    assert duckdb_writes_only_main(f"CREATE TABLE main.result AS {sql}") is False
    assert duckdb_copy_export_target(f"COPY ({sql}) TO 'tmp/out.csv'") is None


@pytest.mark.parametrize(
    "dialect", [None, "mysql", "postgres", "sqlite", "tsql", "athena"]
)
@pytest.mark.parametrize("name", ["stats", "list_sum"])
def test_pure_builtin_allow_is_duckdb_only(dialect, name):
    assert is_readonly_sql(f"SELECT {name}(x) FROM t", dialect=dialect) is not True


@pytest.mark.parametrize(
    "options",
    [
        "",
        "(HEADER false, DELIMITER ' ')",
        "(FORMAT CSV, HEADER true, NULL 'none', QUOTE '\"', ESCAPE '\"')",
        "(FORMAT 'csv')",
    ],
)
def test_copy_export_is_a_write_with_a_separate_verified_target(options):
    sql = f"COPY (SELECT to_milliseconds(1)) TO 'tmp/out.csv' {options}"
    assert duckdb_copy_export_target(sql) == "tmp/out.csv"
    assert is_readonly_sql(sql, dialect="duckdb") is not True


def test_copy_export_handles_comments_and_escaped_literal_path():
    sql = "/* COPY */ COPY (SELECT '; DROP TABLE t' v) TO /* path */ 'tmp/a''b.csv' (HEADER false)"
    assert duckdb_copy_export_target(sql) == "tmp/a'b.csv"


@pytest.mark.parametrize(
    "tail",
    [
        "TO ''",
        "TO '-'",
        "TO '/dev/stdout'",
        "TO 's3://bucket/out'",
        "TO 'tmp/*.csv'",
        "TO 'tmp/out\nfile'",
        "TO /tmp/out",
        "FROM 'tmp/in'",
        "TO 'tmp/out' (FORMAT PARQUET)",
        "TO 'tmp/out' (PARTITION_BY (x))",
        "TO 'tmp/out' (PER_THREAD_OUTPUT true)",
        "TO 'tmp/out' (HEADER true, HEADER false)",
        "TO 'tmp/out' (HEADER query('DELETE FROM t'))",
        "TO 'tmp/out'; INSTALL httpfs",
        "TO 'tmp/out' FOO BAR",
    ],
)
def test_copy_export_rejects_unsupported_effects_and_targets(tail):
    assert duckdb_copy_export_target(f"COPY (SELECT 1) {tail}") is None


@pytest.mark.parametrize(
    "inner",
    [
        "SELECT evil.to_milliseconds(1)",
        "SELECT query('DROP TABLE t')",
        "SELECT 1 FOO BAR",
        "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x",
    ],
)
def test_copy_export_validates_original_query(inner):
    assert duckdb_copy_export_target(f"COPY ({inner}) TO 'tmp/out'") is None


@pytest.mark.parametrize(
    "unit",
    [
        "centuries",
        "days",
        "decades",
        "hours",
        "microseconds",
        "milliseconds",
        "minutes",
        "months",
        "nanoseconds",
        "seconds",
        "weeks",
        "years",
    ],
)
def test_duckdb_interval_constructors(unit):
    sql = f"SELECT to_{unit}(CAST(coalesce(rt, 0)*1000 AS BIGINT)) FROM r"
    assert is_readonly_sql(sql, dialect="duckdb") is True
    assert duckdb_writes_only_main(f"CREATE TABLE main.t AS {sql}") is True
    assert (
        is_readonly_sql(
            f"CREATE TEMP TABLE t AS {sql}", dialect="duckdb", allow_temp_tables=True
        )
        is True
    )


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT evil.to_milliseconds(1)",
        'SELECT "evil"."to_milliseconds"(1)',
        "SELECT to_milliseconds(query('DELETE FROM t'))",
        "SELECT to_milliseconds(writefile('x','data'))",
        "SELECT to_milliseconds(1), fictional_effect()",
    ],
)
def test_interval_constructor_does_not_hide_unknown_effects(sql):
    assert is_readonly_sql(sql, dialect="duckdb") is not True
    assert duckdb_writes_only_main(f"CREATE TABLE main.t AS {sql}") is False


@pytest.mark.parametrize(
    "dialect", [None, "sqlite", "postgres", "mysql", "tsql", "athena"]
)
def test_interval_constructor_allow_is_duckdb_only(dialect):
    assert is_readonly_sql("SELECT to_milliseconds(1)", dialect=dialect) is not True


def test_mysql_select_into_outfile_is_not_readonly():
    assert (
        is_readonly_sql("SELECT * FROM t INTO OUTFILE '/tmp/output'", dialect="mysql")
        is False
    )


def test_postgres_modifying_cte_is_not_readonly():
    sql = "WITH changed AS (DELETE FROM t RETURNING *) SELECT * FROM changed"

    assert is_readonly_sql(sql, dialect="postgres") is False


def test_sqlite_side_effect_function_is_not_verified_readonly():
    assert is_readonly_sql("SELECT writefile('x', 'data')", dialect="sqlite") is False


def test_sql_keywords_in_literals_do_not_change_readonly_result():
    assert is_readonly_sql("SELECT 'DROP INTO OUTFILE'", dialect="mysql") is True


def test_unparsed_sql_fragment_is_not_verified_readonly():
    assert (
        is_readonly_sql("SELECT * FROM t INTO OUTFILE path", dialect="mysql") is False
    )


def test_silently_dropped_sql_tokens_are_not_verified_readonly():
    assert is_readonly_sql("SELECT 1 FOO BAR", dialect="mysql") is None


def test_dynamic_sql_function_is_not_verified_readonly():
    assert is_readonly_sql("SELECT query('DELETE FROM t')", dialect="duckdb") is False


def test_show_with_output_sink_is_not_readonly():
    assert is_readonly_sql("SHOW TABLES INTO OUTFILE x", dialect="mysql") is False


def test_duckdb_read_only_shorthand_forms():
    assert is_readonly_sql("FROM t", dialect="duckdb") is True
    assert is_readonly_sql("VALUES (1), (2)", dialect="duckdb") is True


def test_temp_table_as_select_is_readonly_only_when_enabled():
    sql = "CREATE TEMP TABLE n AS SELECT 1; SELECT * FROM n"

    assert is_readonly_sql(sql, allow_multiple=True) is False
    assert is_readonly_sql(sql, allow_multiple=True, allow_temp_tables=True) is True


def test_temp_table_embedded_write_is_rejected():
    sql = "CREATE TEMP TABLE n AS DELETE FROM data; SELECT * FROM n"

    assert is_readonly_sql(sql, allow_multiple=True, allow_temp_tables=True) is False


class TestBasicReadOnly:
    """Basic read-only statements."""

    def test_select(self):
        assert is_readonly_sql("SELECT * FROM users") is True

    def test_select_with_where(self):
        assert is_readonly_sql("SELECT id, name FROM users WHERE age > 30") is True

    def test_select_with_join(self):
        assert is_readonly_sql("SELECT * FROM a JOIN b ON a.id = b.a_id") is True

    def test_select_with_subquery(self):
        assert is_readonly_sql("SELECT * FROM (SELECT id FROM users) sub") is True

    def test_show(self):
        assert is_readonly_sql("SHOW TABLES") is True

    def test_show_columns(self):
        assert is_readonly_sql("SHOW COLUMNS FROM users") is True

    def test_describe(self):
        assert is_readonly_sql("DESCRIBE users") is True

    def test_explain(self):
        assert is_readonly_sql("EXPLAIN SELECT * FROM users") is True

    def test_explain_analyze_select(self):
        # EXPLAIN ANALYZE in some DBs executes, but we treat EXPLAIN as safe
        assert is_readonly_sql("EXPLAIN ANALYZE SELECT * FROM users") is True


class TestBasicWrite:
    """Basic write statements."""

    def test_insert(self):
        assert is_readonly_sql("INSERT INTO users (name) VALUES ('alice')") is False

    def test_insert_select(self):
        assert is_readonly_sql("INSERT INTO users SELECT * FROM other") is False

    def test_update(self):
        assert is_readonly_sql("UPDATE users SET name = 'bob' WHERE id = 1") is False

    def test_delete(self):
        assert is_readonly_sql("DELETE FROM users WHERE id = 1") is False

    def test_create_table(self):
        assert is_readonly_sql("CREATE TABLE users (id INT)") is False

    def test_create_index(self):
        assert is_readonly_sql("CREATE INDEX idx ON users(name)") is False

    def test_drop_table(self):
        assert is_readonly_sql("DROP TABLE users") is False

    def test_drop_index(self):
        assert is_readonly_sql("DROP INDEX idx") is False

    def test_alter_table(self):
        assert is_readonly_sql("ALTER TABLE users ADD COLUMN age INT") is False

    def test_truncate(self):
        assert is_readonly_sql("TRUNCATE TABLE users") is False

    def test_grant(self):
        assert is_readonly_sql("GRANT SELECT ON users TO alice") is False

    def test_revoke(self):
        assert is_readonly_sql("REVOKE SELECT ON users FROM alice") is False

    def test_merge(self):
        assert is_readonly_sql("MERGE INTO t USING s ON t.id = s.id") is False


class TestCTEs:
    """Common Table Expressions (WITH clauses)."""

    def test_cte_select(self):
        sql = "WITH cte AS (SELECT id FROM users) SELECT * FROM cte"
        assert is_readonly_sql(sql) is True

    def test_cte_insert(self):
        sql = "WITH cte AS (SELECT id FROM users) INSERT INTO other SELECT * FROM cte"
        assert is_readonly_sql(sql) is False

    def test_cte_delete(self):
        sql = "WITH cte AS (SELECT id FROM users) DELETE FROM users WHERE id IN (SELECT id FROM cte)"
        assert is_readonly_sql(sql) is False

    def test_multiple_ctes_select(self):
        sql = """
        WITH
            cte1 AS (SELECT id FROM users),
            cte2 AS (SELECT id FROM orders)
        SELECT * FROM cte1 JOIN cte2 ON cte1.id = cte2.id
        """
        assert is_readonly_sql(sql) is True

    def test_multiple_ctes_insert(self):
        sql = """
        WITH
            cte1 AS (SELECT id FROM users),
            cte2 AS (SELECT id FROM orders)
        INSERT INTO results SELECT * FROM cte1
        """
        assert is_readonly_sql(sql) is False

    def test_nested_cte_parens(self):
        sql = "WITH cte AS (SELECT (1 + 2) AS val) SELECT * FROM cte"
        assert is_readonly_sql(sql) is True

    def test_cte_with_recursive(self):
        sql = """
        WITH RECURSIVE cte AS (
            SELECT 1 AS n
            UNION ALL
            SELECT n + 1 FROM cte WHERE n < 10
        )
        SELECT * FROM cte
        """
        assert is_readonly_sql(sql) is True


class TestComments:
    """SQL comments handling."""

    def test_single_line_comment_before(self):
        sql = "-- this is a comment\nSELECT * FROM users"
        assert is_readonly_sql(sql) is True

    def test_single_line_comment_after(self):
        sql = "SELECT * FROM users -- get all users"
        assert is_readonly_sql(sql) is True

    def test_block_comment_before(self):
        sql = "/* comment */ SELECT * FROM users"
        assert is_readonly_sql(sql) is True

    def test_block_comment_inline(self):
        sql = "SELECT /* columns */ * FROM users"
        assert is_readonly_sql(sql) is True

    def test_block_comment_multiline(self):
        sql = """
        /*
         * Multi-line comment
         */
        SELECT * FROM users
        """
        assert is_readonly_sql(sql) is True

    def test_comment_containing_write_keyword(self):
        # DELETE in comment should be ignored
        sql = "-- DELETE everything\nSELECT * FROM users"
        assert is_readonly_sql(sql) is True

    def test_block_comment_containing_write_keyword(self):
        sql = "/* INSERT INTO users */ SELECT * FROM users"
        assert is_readonly_sql(sql) is True

    def test_multiple_comments(self):
        sql = "-- comment 1\n/* comment 2 */ SELECT * FROM users"
        assert is_readonly_sql(sql) is True


class TestWhitespace:
    """Whitespace handling."""

    def test_leading_whitespace(self):
        assert is_readonly_sql("   SELECT * FROM users") is True

    def test_leading_newlines(self):
        assert is_readonly_sql("\n\n\nSELECT * FROM users") is True

    def test_leading_tabs(self):
        assert is_readonly_sql("\t\tSELECT * FROM users") is True

    def test_mixed_whitespace(self):
        assert is_readonly_sql("  \n\t  SELECT * FROM users") is True

    def test_whitespace_and_comments(self):
        sql = "  \n-- comment\n  \nSELECT * FROM users"
        assert is_readonly_sql(sql) is True


class TestMultipleStatements:
    """Multiple statements (semicolon-separated)."""

    def test_two_selects(self):
        # Multiple statements should return None (unknown)
        sql = "SELECT 1; SELECT 2"
        assert is_readonly_sql(sql) is None

    def test_select_then_delete(self):
        sql = "SELECT * FROM users; DELETE FROM users"
        assert is_readonly_sql(sql) is None

    def test_verified_batch(self):
        assert is_readonly_sql("SELECT 1; SELECT 2", allow_multiple=True) is True

    def test_batch_with_write(self):
        assert (
            is_readonly_sql("SELECT 1; DELETE FROM users", allow_multiple=True) is False
        )

    def test_batch_with_semicolon_in_literal(self):
        assert is_readonly_sql("SELECT ';'; SELECT 2", allow_multiple=True) is True

    def test_trailing_comment_after_semicolon(self):
        assert is_readonly_sql("SELECT 1; -- done", allow_multiple=True) is True

    def test_bracket_list_cannot_hide_later_statement(self):
        sql = "SELECT [']']; INSTALL httpfs; --'"
        assert (
            is_readonly_sql(sql, allow_multiple=True, bracket_identifiers=False) is None
        )

    def test_trailing_semicolon_ok(self):
        # Single statement with trailing semicolon is fine
        sql = "SELECT * FROM users;"
        assert is_readonly_sql(sql) is True

    def test_trailing_semicolon_with_whitespace(self):
        sql = "SELECT * FROM users;  \n"
        assert is_readonly_sql(sql) is True

    def test_multiple_trailing_semicolons(self):
        # Multiple trailing semicolons - still single statement
        sql = "SELECT * FROM users;;;"
        assert is_readonly_sql(sql) is True

    def test_semicolon_in_middle(self):
        sql = "SELECT 1; "
        # Has content after semicolon (whitespace) - but trailing is OK
        assert is_readonly_sql(sql) is True

    def test_empty_statement_after_semicolon(self):
        sql = "SELECT 1;   ;  "
        # Multiple semicolons with whitespace between - ambiguous
        assert is_readonly_sql(sql) is None


class TestCaseInsensitivity:
    """Case insensitivity for keywords."""

    def test_lowercase_select(self):
        assert is_readonly_sql("select * from users") is True

    def test_uppercase_select(self):
        assert is_readonly_sql("SELECT * FROM USERS") is True

    def test_mixed_case_select(self):
        assert is_readonly_sql("SeLeCt * FrOm users") is True

    def test_lowercase_insert(self):
        assert is_readonly_sql("insert into users values (1)") is False

    def test_mixed_case_insert(self):
        assert is_readonly_sql("InSeRt INTO users VALUES (1)") is False


class TestExtraKeywords:
    """Dialect-specific extra keywords."""

    def test_extra_readonly_keyword(self):
        # Hypothetical dialect where FETCH is read-only
        sql = "FETCH NEXT FROM cursor"
        assert is_readonly_sql(sql) is None  # Unknown by default
        assert is_readonly_sql(sql, extra_readonly=frozenset({"FETCH"})) is True

    def test_extra_write_keyword_pragma(self):
        sql = "PRAGMA table_info(users)"
        assert is_readonly_sql(sql) is None  # Unknown by default
        assert is_readonly_sql(sql, extra_write=frozenset({"PRAGMA"})) is False

    def test_extra_write_keyword_vacuum(self):
        sql = "VACUUM"
        assert is_readonly_sql(sql) is None  # Unknown by default
        assert is_readonly_sql(sql, extra_write=frozenset({"VACUUM"})) is False

    def test_extra_write_keyword_attach(self):
        sql = "ATTACH DATABASE 'other.db' AS other"
        assert is_readonly_sql(sql) is None  # Unknown by default
        assert is_readonly_sql(sql, extra_write=frozenset({"ATTACH"})) is False

    def test_extra_write_keyword_detach(self):
        sql = "DETACH DATABASE other"
        assert is_readonly_sql(sql) is None  # Unknown by default
        assert is_readonly_sql(sql, extra_write=frozenset({"DETACH"})) is False

    def test_extra_write_msck(self):
        # Athena-specific
        sql = "MSCK REPAIR TABLE my_table"
        assert is_readonly_sql(sql) is None  # Unknown by default
        assert is_readonly_sql(sql, extra_write=frozenset({"MSCK"})) is False

    def test_extra_write_unload(self):
        # Athena-specific
        sql = "UNLOAD (SELECT * FROM t) TO 's3://bucket/'"
        assert is_readonly_sql(sql) is None  # Unknown by default
        assert is_readonly_sql(sql, extra_write=frozenset({"UNLOAD"})) is False

    def test_extra_keywords_combined(self):
        # Both extra_readonly and extra_write
        sql = "VACUUM"
        assert (
            is_readonly_sql(
                sql,
                extra_readonly=frozenset({"FETCH"}),
                extra_write=frozenset({"VACUUM"}),
            )
            is False
        )


class TestUnknownKeywords:
    """Unknown/unrecognized keywords."""

    def test_unknown_keyword(self):
        sql = "FOOBAR something"
        assert is_readonly_sql(sql) is None

    def test_unknown_dialect_specific(self):
        sql = "CALL stored_procedure()"
        assert is_readonly_sql(sql) is None

    def test_set_statement(self):
        # SET could be read or write depending on dialect
        sql = "SET search_path TO myschema"
        assert is_readonly_sql(sql) is None


class TestEdgeCases:
    """Edge cases and boundary conditions."""

    def test_empty_string(self):
        assert is_readonly_sql("") is None

    def test_only_whitespace(self):
        assert is_readonly_sql("   \n\t  ") is None

    def test_only_comment(self):
        assert is_readonly_sql("-- just a comment") is None

    def test_only_block_comment(self):
        assert is_readonly_sql("/* just a comment */") is None

    def test_incomplete_statement(self):
        # No actual statement, just keyword
        assert is_readonly_sql("SELECT") is True  # Still identified as SELECT

    def test_select_no_from(self):
        assert is_readonly_sql("SELECT 1") is True

    def test_select_expression(self):
        assert is_readonly_sql("SELECT 1 + 2 * 3") is True

    def test_very_long_sql(self):
        # Ensure no performance issues with long SQL
        sql = "SELECT " + ", ".join([f"col{i}" for i in range(1000)]) + " FROM t"
        assert is_readonly_sql(sql) is True


class TestExplainVariants:
    """EXPLAIN with various statements."""

    def test_explain_select(self):
        assert is_readonly_sql("EXPLAIN SELECT * FROM users") is True

    def test_explain_insert(self):
        # EXPLAIN doesn't execute, so this is still safe
        assert is_readonly_sql("EXPLAIN INSERT INTO t VALUES (1)") is False

    def test_explain_delete(self):
        assert is_readonly_sql("EXPLAIN DELETE FROM users") is False

    def test_explain_update(self):
        assert is_readonly_sql("EXPLAIN UPDATE users SET x = 1") is False

    def test_explain_plan(self):
        assert is_readonly_sql("EXPLAIN PLAN FOR SELECT * FROM users") is True

    def test_explain_analyze_write(self):
        assert is_readonly_sql("EXPLAIN ANALYZE INSERT INTO t VALUES (1)") is False

    def test_explain_analyze_write_after_newline(self):
        assert is_readonly_sql("EXPLAIN ANALYZE\nINSERT INTO t VALUES (1)") is False


class TestAthenaDialect:
    """Athena-specific SQL patterns."""

    ATHENA_WRITE = frozenset({"MSCK", "UNLOAD", "VACUUM"})

    def test_athena_msck_repair(self):
        sql = "MSCK REPAIR TABLE my_table"
        assert is_readonly_sql(sql, extra_write=self.ATHENA_WRITE) is False

    def test_athena_unload(self):
        sql = "UNLOAD (SELECT * FROM t) TO 's3://bucket/path'"
        assert is_readonly_sql(sql, extra_write=self.ATHENA_WRITE) is False

    def test_athena_vacuum(self):
        sql = "VACUUM my_iceberg_table"
        assert is_readonly_sql(sql, extra_write=self.ATHENA_WRITE) is False

    def test_athena_select(self):
        sql = "SELECT * FROM my_table"
        assert is_readonly_sql(sql, extra_write=self.ATHENA_WRITE) is True

    def test_athena_show_tables(self):
        sql = "SHOW TABLES IN my_database"
        assert is_readonly_sql(sql, extra_write=self.ATHENA_WRITE) is True

    def test_athena_describe(self):
        sql = "DESCRIBE my_table"
        assert is_readonly_sql(sql, extra_write=self.ATHENA_WRITE) is True


class TestSQLiteDialect:
    """SQLite-specific SQL patterns."""

    SQLITE_WRITE = frozenset(
        {"PRAGMA", "ATTACH", "DETACH", "VACUUM", "REINDEX", "ANALYZE"}
    )

    def test_sqlite_pragma(self):
        sql = "PRAGMA table_info(users)"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is False

    def test_sqlite_pragma_set(self):
        sql = "PRAGMA foreign_keys = ON"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is False

    def test_sqlite_attach(self):
        sql = "ATTACH DATABASE 'other.db' AS other"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is False

    def test_sqlite_detach(self):
        sql = "DETACH DATABASE other"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is False

    def test_sqlite_vacuum(self):
        sql = "VACUUM"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is False

    def test_sqlite_vacuum_into(self):
        sql = "VACUUM INTO 'backup.db'"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is False

    def test_sqlite_reindex(self):
        sql = "REINDEX"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is False

    def test_sqlite_analyze(self):
        sql = "ANALYZE"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is False

    def test_sqlite_select(self):
        sql = "SELECT * FROM users"
        assert is_readonly_sql(sql, extra_write=self.SQLITE_WRITE) is True


class TestSelectInto:
    """SELECT INTO creates a new table - this is a write operation."""

    def test_select_into_basic(self):
        # SELECT INTO creates a new table (SQL Server, PostgreSQL)
        sql = "SELECT * INTO new_table FROM users"
        assert is_readonly_sql(sql) is False

    def test_select_into_with_where(self):
        sql = "SELECT id, name INTO backup_users FROM users WHERE active = 1"
        assert is_readonly_sql(sql) is False

    def test_select_into_temp(self):
        sql = "SELECT * INTO #temp_table FROM users"
        assert is_readonly_sql(sql) is False

    def test_select_into_with_join(self):
        sql = "SELECT a.*, b.name INTO results FROM a JOIN b ON a.id = b.a_id"
        assert is_readonly_sql(sql) is False


class TestUpsertVariants:
    """UPSERT/REPLACE operations across different database dialects."""

    def test_insert_or_replace(self):
        # SQLite syntax
        sql = "INSERT OR REPLACE INTO users (id, name) VALUES (1, 'alice')"
        assert is_readonly_sql(sql) is False

    def test_insert_or_ignore(self):
        sql = "INSERT OR IGNORE INTO users (id, name) VALUES (1, 'alice')"
        assert is_readonly_sql(sql) is False

    def test_replace_into(self):
        # MySQL/SQLite syntax
        sql = "REPLACE INTO users (id, name) VALUES (1, 'alice')"
        assert is_readonly_sql(sql) is False

    def test_insert_on_conflict_do_nothing(self):
        # PostgreSQL/SQLite syntax
        sql = "INSERT INTO users (id) VALUES (1) ON CONFLICT DO NOTHING"
        assert is_readonly_sql(sql) is False

    def test_insert_on_conflict_do_update(self):
        sql = "INSERT INTO users (id, name) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET name = 'b'"
        assert is_readonly_sql(sql) is False

    def test_insert_on_duplicate_key(self):
        # MySQL syntax
        sql = "INSERT INTO users (id, name) VALUES (1, 'a') ON DUPLICATE KEY UPDATE name = 'b'"
        assert is_readonly_sql(sql) is False


class TestStringLiterals:
    """Keywords and special characters inside string literals."""

    def test_keyword_in_single_quoted_string(self):
        sql = "SELECT * FROM logs WHERE action = 'DELETE'"
        assert is_readonly_sql(sql) is True

    def test_keyword_in_double_quoted_string(self):
        sql = 'SELECT * FROM logs WHERE action = "DELETE"'
        assert is_readonly_sql(sql) is True

    def test_semicolon_in_string(self):
        sql = "SELECT 'foo;bar' AS val"
        assert is_readonly_sql(sql) is True

    def test_escaped_quote_in_string(self):
        sql = "SELECT 'it''s a test' AS val"
        assert is_readonly_sql(sql) is True

    def test_multiple_strings_with_keywords(self):
        sql = "SELECT 'INSERT', 'UPDATE', 'DELETE' AS keywords"
        assert is_readonly_sql(sql) is True


class TestIdentifierQuoting:
    """Quoted identifiers containing keywords."""

    def test_backtick_identifier_with_keyword(self):
        # MySQL style
        sql = "SELECT `DELETE` FROM users"
        assert is_readonly_sql(sql) is True

    def test_bracket_identifier_with_keyword(self):
        # SQL Server style
        sql = "SELECT [DELETE] FROM users"
        assert is_readonly_sql(sql) is True

    def test_double_quote_identifier_with_keyword(self):
        # PostgreSQL/standard style
        sql = 'SELECT "DELETE" FROM users'
        assert is_readonly_sql(sql) is True

    def test_backtick_table_name(self):
        sql = "SELECT * FROM `DROP`"
        assert is_readonly_sql(sql) is True

    def test_bracket_table_name(self):
        sql = "SELECT * FROM [INSERT]"
        assert is_readonly_sql(sql) is True


class TestInetOperators:
    """Test network / INET subnet containment operators (DuckDB and PostgreSQL)."""

    @pytest.mark.parametrize("dialect", ["duckdb", "postgres"])
    @pytest.mark.parametrize(
        "op",
        ["<<=", ">>="],
    )
    def test_inet_subnet_containment_operators(self, dialect, op):
        sql = f"SELECT '192.168.1.5'::INET {op} '192.168.1.0/24'::INET"
        assert is_readonly_sql(sql, dialect=dialect) is True

    def test_duckdb_inet_subnet_containment_in_cte_query(self):
        sql = (
            "WITH ips AS (SELECT DISTINCT c_ip FROM access WHERE substr(time_local,1,11) IN "
            "('04/Oct/2026','05/Oct/2026') AND (cs_user_agent LIKE '%WP-Safe-Scanner%')) "
            "SELECT count(*) ips_celkem, count(il.label) ips_v_ip_labels "
            "FROM ips LEFT JOIN ip_labels il ON TRY_CAST(ips.c_ip AS INET) <<= il.cidr::INET"
        )
        assert is_readonly_sql(sql, dialect="duckdb") is True


class TestCteColumnAliases:
    """Test CTE definitions with column aliases: WITH name(col1, col2) AS (...)."""

    def test_cte_with_column_alias_is_readonly(self):
        sql = "WITH n(net) AS (VALUES ('147.45.142')) SELECT * FROM n"
        assert is_readonly_sql(sql, dialect="duckdb") is True

    def test_multiple_ctes_with_column_aliases_is_readonly(self):
        sql = (
            "WITH u AS (SELECT 1 a), "
            "nase(ip, kdo) AS (VALUES ('145.239.12.84', 'gate2')) "
            "SELECT u.a, n.kdo FROM u LEFT JOIN nase n ON u.a = 1"
        )
        assert is_readonly_sql(sql, dialect="duckdb") is True

    def test_recursive_cte_with_column_aliases_is_readonly(self):
        sql = "WITH RECURSIVE t(n) AS (VALUES (1)) SELECT * FROM t"
        assert is_readonly_sql(sql, dialect="duckdb") is True

    def test_cte_with_column_alias_write_is_rejected(self):
        sql = "WITH n(net) AS (VALUES ('147.45.142')) DELETE FROM n"
        assert is_readonly_sql(sql, dialect="duckdb") is False
