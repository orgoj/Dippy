#!/usr/bin/env python3
"""Generate the SQLGlot-derived tables embedded in ``dippy-rs/src/sql.rs``.

The Rust SQL checker cannot run SQLGlot. It accepts a conservative query
grammar and needs two facts that only SQLGlot knows:

* RESERVED: words that SQLGlot may treat as keywords. The Rust grammar never
  accepts them as identifiers. Candidates are every word in the tokenizer
  keyword tables of the supported dialects plus words that SQLGlot parsers
  match by text; a candidate is dropped only when it works as an identifier
  in every probe template and dialect.
* FUNCTIONS: function names with, per parser dialect, a bit mask of argument
  counts (0-4) for which ``is_readonly_sql`` returns True.

Usage: sql_tables.py  (prints Rust source to paste into sql.rs)
"""

from __future__ import annotations

import os
import re
import sys
import tempfile
from pathlib import Path

os.environ["HOME"] = tempfile.mkdtemp(prefix="dippy-sql-home-")
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "src"))

import sqlglot  # noqa: E402

from dippy.core.sql import _DUCKDB_READONLY_FUNCTIONS, is_readonly_sql  # noqa: E402

# Parser dialects in the order of the Rust `Dialect` enum.
DIALECTS = [None, "postgres", "mysql", "sqlite", "tsql", "duckdb", "athena"]

# Words SQLGlot parsers recognise by text (VAR tokens) in some position.
TEXT_KEYWORDS = """
AT ZONE NULLS LAST FIRST RESPECT IGNORE WITHIN FINAL SETTINGS PREWHERE
SAMPLE TABLESAMPLE LATERAL VIEW PIVOT UNPIVOT CONNECT PRIOR START NOCYCLE
MATCH_RECOGNIZE SYSTEM_TIME WINDOW QUALIFY OPTION XML FORMAT TIES ONLY
PERCENT FOLLOWING PRECEDING UNBOUNDED CURRENT GROUPS EXCLUDE INCLUDE
RENAME COLUMNS STAR ORDINALITY OFFSET FETCH LIMIT TOP USING ON
ASOF POSITIONAL ANTI SEMI NATURAL APPLY OUTER CROSS BERNOULLI REPEATABLE
SYSTEM BLOCK SEED METHOD PERCENTILE_CONT PERCENTILE_DISC MATCHED
COLLATE BINARY CHARACTER VARYING WITHOUT LOCAL TIME TIMESTAMP INTERVAL
ARRAY MAP STRUCT LIST UNION EXCEPT INTERSECT MINUS VALUES VALUE
DEFAULT DISTINCT ALL ANY SOME EXISTS CASE WHEN THEN ELSE END CAST
TRY_CAST CONVERT EXTRACT POSITION SUBSTRING TRIM OVERLAY LEADING
TRAILING BOTH FOR FROM WHERE GROUP HAVING ORDER BY ASC DESC JOIN INNER
LEFT RIGHT FULL ON AS WITH RECURSIVE SELECT NOT AND OR IS NULL TRUE
FALSE LIKE ILIKE IN BETWEEN ESCAPE OVER PARTITION ROWS RANGE FILTER
INTO OUTFILE DUMPFILE LOCK SHARE NOWAIT SKIP LOCKED MODE UPDATE OF
DIV MOD REGEXP RLIKE GLOB MATCH AGAINST SOUNDS MEMBER OVERLAPS SIMILAR
UNKNOWN ISNULL NOTNULL SEPARATOR WITHIN KEEP DENSE_RANK RANK ROW
CURRENT_DATE CURRENT_TIME CURRENT_TIMESTAMP CURRENT_USER LOCALTIME
LOCALTIMESTAMP SESSION_USER USER SYSDATE SYSTIMESTAMP ROWNUM LEVEL
""".split()

FUNCTION_CANDIDATES = """
abs avg ceil ceiling coalesce concat count floor greatest least length
lower ltrim max min nullif round rtrim sign sqrt substr substring sum trim
upper replace split_part string_agg array_agg group_concat row_number lag
lead first_value last_value median stddev stddev_pop stddev_samp variance
var_pop var_samp mod power pow exp ln log log10 left right lpad rpad
strpos instr md5 ifnull nvl isnull iif unnest list_value array_length
contains starts_with ends_with any_value arg_max arg_min bool_and bool_or
approx_count_distinct initcap char_length character_length
corr covar_pop covar_samp sha1 concat_ws repeat len regexp_extract
regexp_replace
""".split() + sorted(_DUCKDB_READONLY_FUNCTIONS)

IDENT_TEMPLATES = [
    "SELECT {w} FROM t",
    "SELECT {w}, b FROM t",
    "SELECT a {w} FROM t",
    "SELECT a {w}, b FROM t",
    "SELECT a AS {w} FROM t",
    "SELECT a FROM {w}",
    "SELECT a FROM {w} x",
    "SELECT a FROM t {w}",
    "SELECT a FROM t {w} WHERE b = 1",
    "SELECT a FROM t {w} JOIN u ON a = b",
    "SELECT a FROM t {w}, u",
    "SELECT a FROM t AS {w}",
    "SELECT {w}.a FROM t",
    "SELECT a FROM s.{w}",
    "SELECT a FROM {w}.t",
    "SELECT a FROM t WHERE {w} = 1",
    "SELECT a FROM t WHERE {w} > 1 AND b < 2",
    "SELECT a FROM t WHERE {w} IS NULL",
    "SELECT a FROM t WHERE {w} LIKE 'x'",
    "SELECT a FROM t WHERE {w} IN (1)",
    "SELECT a FROM t WHERE {w} BETWEEN 1 AND 2",
    "SELECT a FROM t WHERE NOT {w}",
    "SELECT a FROM t ORDER BY {w}",
    "SELECT a FROM t ORDER BY {w} DESC",
    "SELECT a FROM t GROUP BY {w}",
    "SELECT a FROM t GROUP BY {w} HAVING count(*) > 1",
    "SELECT count({w}) FROM t",
    "SELECT -{w} FROM t",
    "SELECT {w} + 1 FROM t",
    "SELECT {w} || 'x' FROM t",
    "SELECT CAST({w} AS INT) FROM t",
    "SELECT CASE WHEN {w} THEN 1 END FROM t",
    "SELECT CASE {w} WHEN 1 THEN 2 END FROM t",
    "WITH {w} AS (SELECT 1) SELECT * FROM {w}",
    "SELECT a FROM t JOIN u {w} ON {w}.a = t.a",
    "SELECT a FROM t JOIN u USING ({w})",
    "SELECT * FROM (SELECT 1) {w}",
    "SELECT sum(a) OVER (PARTITION BY {w}) FROM t",
    "SELECT sum(a) OVER (ORDER BY {w}) FROM t",
    "SELECT a FROM t UNION SELECT {w} FROM u",
    "SELECT DISTINCT {w} FROM t",
]


def ok(sql: str, dialect) -> bool:
    try:
        return is_readonly_sql(sql, dialect=dialect) is True
    except Exception:
        return False


def reserved_words() -> list[str]:
    words = set(TEXT_KEYWORDS)
    for name in DIALECTS + ["presto", "trino", "hive", "bigquery", "snowflake"]:
        dialect = sqlglot.Dialect.get_or_raise(name)
        for key in dialect.tokenizer_class.KEYWORDS:
            words.update(w.upper() for w in re.findall(r"[A-Za-z_]\w*", key))
    reserved = []
    for word in sorted(words):
        if not all(
            ok(template.format(w=spelling), dialect)
            for spelling in (word.lower(), word)
            for template in IDENT_TEMPLATES
            for dialect in DIALECTS
        ):
            reserved.append(word)
        elif word in TEXT_KEYWORDS:
            reserved.append(word)
    return reserved


def function_masks() -> list[tuple[str, list[int]]]:
    rows = []
    args = ["a", "b", "c", "d"]
    for name in dict.fromkeys(FUNCTION_CANDIDATES):
        masks = []
        for dialect in DIALECTS:
            mask = 0
            for n in range(5):
                arg_list = ", ".join(args[:n])
                if ok(f"SELECT {name}({arg_list}) FROM t", dialect) and ok(
                    f"SELECT {name.upper()}({arg_list}) FROM t", dialect
                ):
                    mask |= 1 << n
            masks.append(mask)
        if any(masks):
            rows.append((name, masks))
    return rows


def main() -> None:
    reserved = reserved_words()
    print("/// Generated by `rust/parity/sql_tables.py`.")
    print(f"const RESERVED: [&str; {len(reserved)}] = [")
    line = "   "
    for word in reserved:
        item = f' "{word}",'
        if len(line) + len(item) > 96:
            print(line)
            line = "   "
        line += item
    print(line)
    print("];")
    rows = function_masks()
    print()
    print("/// Generated by `rust/parity/sql_tables.py`: argument-count bit masks")
    print("/// per parser dialect (base, postgres, mysql, sqlite, tsql, duckdb, athena).")
    print(f"const FUNCTIONS: [(&str, [u8; 7]); {len(rows)}] = [")
    for name, masks in sorted(rows):
        print(f'    ("{name}", {masks}),')
    print("];")


if __name__ == "__main__":
    main()
