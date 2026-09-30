"""Data type normalization must not be confused with discarded SQL syntax."""

from pathlib import Path

import pytest

from dippy.core import sql as sql_module
from dippy.core.sql import (
    duckdb_copy_export_target,
    duckdb_writes_only_main,
    is_readonly_sql,
)


def test_reported_duckdb_traffic_query():
    sql = (Path(__file__).parents[1] / "fixtures/sql/duckdb_traffic.sql").read_text()
    assert is_readonly_sql(sql, dialect="duckdb", bracket_identifiers=False) is True


@pytest.mark.parametrize(
    "dialect", ["duckdb", "postgres", "mysql", "sqlite", "tsql", "athena"]
)
@pytest.mark.parametrize(
    "type_name", ["VARCHAR", "INTEGER", "BOOL", "NUMERIC", "DOUBLE PRECISION"]
)
def test_cast_type_normalization(dialect, type_name):
    assert (
        is_readonly_sql(f"SELECT CAST(x AS {type_name}) FROM t", dialect=dialect)
        is True
    )


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT CAST(a AS VARCHAR), CAST(b AS INTEGER), CAST(c AS BOOL) FROM t",
        "SELECT TRY_CAST(a AS VARCHAR), TRY_CAST(b AS NUMERIC) FROM t",
        "SELECT a::VARCHAR, b::INTEGER, c::DOUBLE PRECISION FROM t",
        "SELECT CAST(a AS CHARACTER VARYING), CAST(b AS TIMESTAMP WITH TIME ZONE) FROM t",
        "SELECT CAST(a AS STRUCT(x VARCHAR, y INTEGER)), CAST(b AS VARCHAR[]) FROM t",
        "SELECT CAST(CAST(a AS INTEGER) AS VARCHAR) FROM t",
        "WITH x AS (SELECT CAST(a AS VARCHAR) v FROM t) SELECT v FROM x UNION ALL SELECT CAST(b AS VARCHAR) FROM t",
        "SELECT text, varchar, integer, CAST(x AS VARCHAR) AS varchar FROM t AS text",
        "SELECT 1 AS int, 2 AS varchar, 3 AS boolean",
        "SELECT CAST(a AS /* type */ VARCHAR), CAST(b AS DECIMAL(10,2)) FROM t",
    ],
)
def test_duckdb_cast_contexts_and_type_named_identifiers(sql):
    assert is_readonly_sql(sql, dialect="duckdb", bracket_identifiers=False) is True


@pytest.mark.parametrize(
    "dialect,sql",
    [
        ("tsql", "SELECT CONVERT(VARCHAR(50), x), CONVERT(INT, y) FROM t"),
        ("mysql", "SELECT CONVERT(x, CHAR), CONVERT(y, SIGNED INTEGER) FROM t"),
        ("postgres", "SELECT x::INTEGER, y::DOUBLE PRECISION FROM t"),
    ],
)
def test_dialect_specific_cast_syntax(dialect, sql):
    assert is_readonly_sql(sql, dialect=dialect) is True


def test_duckdb_cast_normalization_in_output_contexts():
    sql = "SELECT CAST(x AS VARCHAR) FROM t"
    assert duckdb_writes_only_main(f"CREATE TABLE main.result AS {sql}") is True
    assert duckdb_copy_export_target(f"COPY ({sql}) TO 'tmp/out.csv'") == "tmp/out.csv"
    assert (
        is_readonly_sql(
            f"CREATE TEMP TABLE result AS {sql}",
            dialect="duckdb",
            allow_temp_tables=True,
        )
        is True
    )


@pytest.mark.parametrize(
    "dialect", ["duckdb", "postgres", "mysql", "sqlite", "tsql", "athena"]
)
@pytest.mark.parametrize(
    "sql",
    [
        "SELECT CAST(fictional_effect() AS VARCHAR)",
        "WITH x AS (DELETE FROM t RETURNING *) SELECT CAST(a AS VARCHAR) FROM x",
        "SELECT CAST(a AS VARCHAR) FROM t; DROP TABLE t",
        "SELECT CAST(a AS VARCHAR) INTO outfile FROM t",
        "SELECT CAST(a AS VARCHAR) FROM t UNRECOGNIZED discarded",
        "SELECT CAST(a AS VARCHAR) FROM t WHERE",
    ],
)
def test_casts_do_not_hide_effects_or_unparsed_syntax(dialect, sql):
    assert is_readonly_sql(sql, dialect=dialect, allow_multiple=True) is not True


@pytest.mark.parametrize(
    "sql",
    [
        "SELECT CAST(query('DROP TABLE t') AS VARCHAR)",
        "SELECT CAST(a AS fictional_type) FROM t",
    ],
)
def test_unverified_casts_in_duckdb_output_contexts(sql):
    assert is_readonly_sql(sql, dialect="duckdb") is not True
    assert duckdb_writes_only_main(f"CREATE TABLE main.result AS {sql}") is False
    assert duckdb_copy_export_target(f"COPY ({sql}) TO 'tmp/out.csv'") is None


def test_type_normalization_does_not_forgive_discarded_identifiers(monkeypatch):
    original_parse = sql_module._parse_sql
    query = "SELECT text, CAST(x AS VARCHAR) FROM t"

    def discard_column(sql, dialect):
        trees = original_parse(sql, dialect)
        if sql == query:
            trees[0].set("expressions", trees[0].expressions[1:])
        return trees

    monkeypatch.setattr(sql_module, "_parse_sql", discard_column)
    assert is_readonly_sql(query, dialect="duckdb") is None
