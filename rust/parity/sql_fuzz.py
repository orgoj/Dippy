#!/usr/bin/env python3
"""Differential fuzzing of the Rust SQL checker against Python/SQLGlot.

Generates random SQL from a grammar that covers (and slightly exceeds) the
conservative Rust grammar, plus token-level mutations of generated and
collected SQL, and compares both implementations with ``sql_compare``.
Every Rust "verified" answer must match Python (unsafe count 0).

Usage: sql_fuzz.py [--count N] [--seed S] [--seeds FILE.jsonl] [--show N]
"""

from __future__ import annotations

import argparse
import json
import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from sql_compare import build, compare  # noqa: E402

DIALECTS = [None, "postgres", "mysql", "sqlite", "tsql", "duckdb", "athena"]

FUNCS = """abs any_value approx_count_distinct arg_max avg bit_xor bool_and ceil
ceiling char_length coalesce concat concat_ws contains corr count covar_pop
ends_with entropy exp first_value floor fsum greatest group_concat histogram
ifnull iif initcap instr isnull kurtosis lag last_value lead least left len
length list_sum list_value ln log log10 lower lpad ltrim mad max md5 median
min mod mode nullif nvl pow power product regexp_extract regexp_replace repeat
replace right round row_number rpad rtrim sha1 sign skewness split_part sqrt
starts_with stats stddev string_agg strpos substr substring sum to_days
to_milliseconds to_years trim unnest upper var_pop variance
query writefile read_csv fictional_effect rank now date_trunc nextval""".split()

TYPES = """INT INTEGER BIGINT SMALLINT TINYINT BOOL BOOLEAN TEXT FLOAT REAL BLOB
JSON UUID TIME INET VARCHAR CHAR DECIMAL NUMERIC DATE TIMESTAMP TIMESTAMPTZ
HUGEINT STRING DATETIME INTERVAL MONEY SIGNED fictional_type""".split() + [
    "DOUBLE",
    "DOUBLE PRECISION",
    "CHARACTER VARYING",
    "TIMESTAMP WITH TIME ZONE",
    "VARCHAR(10)",
    "DECIMAL(10,2)",
    "NUMERIC(5)",
    "CHARACTER VARYING(3)",
    "VARCHAR[]",
    "INTEGER[][]",
    "STRUCT(x INT)",
]

IDENTS = ["a", "b", "c", "t", "u", "x", "id", "name", "date", "main", "temp", "n"]
ODD_IDENTS = """key value user year first last format filter sample using text
varchar int left right natural window qualify over end offset limit top
at zone nulls only sets cube rollup glob regexp div into copy drop install
select from where group order""".split()

STRINGS = [
    "'x'",
    "''",
    "'it''s'",
    "'a\\b'",
    "'\\'",
    "'$['",
    "'%Y-%m'",
    "'day'",
    "'--'",
    "'/*'",
    "';'",
    "'DROP TABLE t'",
    "'a\\'b'",
    "'\\\\'",
    "'%\\%27%'",
    "'^(\\d+)'",
]

SPACES = [" ", " ", " ", "  ", "\n", "\t", " /* c */ ", " -- c\n", "/**/"]


class Gen:
    def __init__(self, rng: random.Random):
        self.r = rng

    def ws(self) -> str:
        return self.r.choice(SPACES) if self.r.random() < 0.15 else " "

    def kw(self, word: str) -> str:
        roll = self.r.random()
        if roll < 0.15:
            return word.lower()
        if roll < 0.2:
            return word.capitalize()
        return word

    def join(self, *parts: str) -> str:
        out = ""
        for part in parts:
            if not part:
                continue
            out = part if not out else out + self.ws() + part
        return out

    def ident(self) -> str:
        roll = self.r.random()
        if roll < 0.06:
            return self.r.choice(ODD_IDENTS)
        if roll < 0.1:
            return self.r.choice(['"a"', '"q x"', "`a`", "[a]", '"main"', '"x""y"'])
        return self.r.choice(IDENTS)

    def column(self) -> str:
        roll = self.r.random()
        if roll < 0.2:
            return f"{self.ident()}.{self.ident()}"
        if roll < 0.23:
            return f"{self.ident()}.{self.ident()}.{self.ident()}"
        return self.ident()

    def type_name(self) -> str:
        return self.kw(self.r.choice(TYPES))

    def literal(self) -> str:
        roll = self.r.random()
        if roll < 0.35:
            return self.r.choice(["1", "0", "42", "1.5", "1e6", "2.5E-3", "007", ".5", "1."])
        if roll < 0.75:
            return self.r.choice(STRINGS)
        return self.kw(self.r.choice(["NULL", "TRUE", "FALSE"]))

    def func(self, depth: int) -> str:
        name = self.r.choice(FUNCS)
        if self.r.random() < 0.3:
            name = name.upper()
        roll = self.r.random()
        if name.lower() == "count" and roll < 0.3:
            args = "*"
        else:
            n = self.r.choice([0, 1, 1, 1, 2, 2, 3, 4, 5])
            args = ", ".join(self.expr(depth + 1) for _ in range(n))
            if n and self.r.random() < 0.15:
                args = self.kw("DISTINCT") + " " + args
        out = f"{name}({args})"
        if self.r.random() < 0.08:
            out = self.join(out, self.kw("FILTER"), f"({self.kw('WHERE')} {self.expr(depth + 1)})")
        if self.r.random() < 0.15:
            inner = []
            if self.r.random() < 0.5:
                inner.append(self.join(self.kw("PARTITION BY"), self.expr_list(depth + 1)))
            if self.r.random() < 0.6:
                inner.append(self.join(self.kw("ORDER BY"), self.order_list(depth + 1)))
            if self.r.random() < 0.05:
                inner.append(self.kw("ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW"))
            out = self.join(out, self.kw("OVER"), "(" + " ".join(inner) + ")")
        return out

    def expr_list(self, depth: int) -> str:
        return ", ".join(self.expr(depth) for _ in range(self.r.choice([1, 1, 2, 3])))

    def order_list(self, depth: int) -> str:
        items = []
        for _ in range(self.r.choice([1, 1, 2])):
            item = self.expr(depth)
            roll = self.r.random()
            if roll < 0.25:
                item = self.join(item, self.kw("DESC"))
            elif roll < 0.4:
                item = self.join(item, self.kw("ASC"))
            elif roll < 0.43:
                item = self.join(item, self.kw("NULLS FIRST"))
            items.append(item)
        return ", ".join(items)

    def primary(self, depth: int) -> str:
        if depth > 4:
            return self.r.choice([self.literal(), self.column()])
        roll = self.r.random()
        if roll < 0.25:
            return self.literal()
        if roll < 0.5:
            return self.column()
        if roll < 0.68:
            return self.func(depth)
        if roll < 0.75:
            fn = self.kw(self.r.choice(["CAST", "CAST", "TRY_CAST"]))
            return f"{fn}({self.expr(depth + 1)} {self.kw('AS')} {self.type_name()})"
        if roll < 0.8:
            return f"{self.primary(depth + 1)}::{self.type_name()}"
        if roll < 0.85:
            whens = " ".join(
                self.join(self.kw("WHEN"), self.expr(depth + 1), self.kw("THEN"), self.expr(depth + 1))
                for _ in range(self.r.choice([1, 2]))
            )
            operand = self.expr(depth + 1) if self.r.random() < 0.3 else ""
            other = self.join(self.kw("ELSE"), self.expr(depth + 1)) if self.r.random() < 0.5 else ""
            return self.join(self.kw("CASE"), operand, whens, other, self.kw("END"))
        if roll < 0.9:
            return f"({self.expr(depth + 1)})"
        if roll < 0.94:
            return f"({self.query(depth + 1)})"
        if roll < 0.96:
            return self.join(self.kw("EXISTS"), f"({self.query(depth + 1)})")
        return "[" + ", ".join(self.expr(depth + 1) for _ in range(self.r.choice([0, 1, 2]))) + "]"

    def expr(self, depth: int = 0) -> str:
        left = self.primary(depth)
        if depth > 4:
            return left
        roll = self.r.random()
        if roll < 0.45:
            return left
        if roll < 0.6:
            op = self.r.choice(["=", "<>", "!=", "<", ">", "<=", ">=", "<<=", ">>=", "==", "<=>"])
            return self.join(left, op, self.primary(depth + 1))
        if roll < 0.7:
            op = self.r.choice(["+", "-", "*", "/", "%", "||", "&", "^"])
            return self.join(left, op, self.primary(depth + 1))
        if roll < 0.76:
            word = self.kw(self.r.choice(["AND", "OR"]))
            return self.join(left, word, self.expr(depth + 1))
        if roll < 0.8:
            neg = self.kw("NOT") if self.r.random() < 0.3 else ""
            like = self.kw(self.r.choice(["LIKE", "ILIKE"]))
            esc = self.join(self.kw("ESCAPE"), self.r.choice(STRINGS)) if self.r.random() < 0.2 else ""
            return self.join(left, neg, like, self.primary(depth + 1), esc)
        if roll < 0.85:
            neg = self.kw("NOT") if self.r.random() < 0.3 else ""
            inner = self.query(depth + 1) if self.r.random() < 0.3 else self.expr_list(depth + 1)
            return self.join(left, neg, self.kw("IN"), f"({inner})")
        if roll < 0.88:
            neg = self.kw("NOT") if self.r.random() < 0.3 else ""
            return self.join(left, neg, self.kw("BETWEEN"), self.primary(depth + 1), self.kw("AND"), self.primary(depth + 1))
        if roll < 0.93:
            neg = self.kw("NOT") if self.r.random() < 0.3 else ""
            what = self.kw(self.r.choice(["NULL", "NULL", "TRUE", "FALSE", "UNKNOWN"]))
            return self.join(left, self.kw("IS"), neg, what)
        if roll < 0.97:
            return self.join(self.kw("NOT"), left)
        return self.join(self.r.choice(["-", "+", "- -"]), left)

    def table_factor(self, depth: int) -> str:
        if depth < 4 and self.r.random() < 0.15:
            base = f"({self.query(depth + 1)})"
        elif self.r.random() < 0.03:
            base = f"read_csv({self.r.choice(STRINGS)})"
        else:
            base = self.column()
        roll = self.r.random()
        if roll < 0.2:
            base = self.join(base, self.ident())
        elif roll < 0.35:
            base = self.join(base, self.kw("AS"), self.ident())
        elif roll < 0.37:
            base = self.join(base, self.kw("AS"), f"{self.ident()}({self.ident()})")
        return base

    def from_item(self, depth: int) -> str:
        out = self.table_factor(depth)
        while self.r.random() < 0.25:
            kind = self.kw(
                self.r.choice(
                    ["JOIN", "INNER JOIN", "LEFT JOIN", "LEFT OUTER JOIN", "RIGHT JOIN",
                     "FULL JOIN", "FULL OUTER JOIN", "CROSS JOIN", "NATURAL JOIN",
                     "ASOF JOIN", "SEMI JOIN"]
                )
            )
            out = self.join(out, kind, self.table_factor(depth))
            if "CROSS" not in kind.upper() and "NATURAL" not in kind.upper():
                if self.r.random() < 0.8:
                    out = self.join(out, self.kw("ON"), self.expr(depth + 1))
                else:
                    out = self.join(out, self.kw("USING"), f"({self.ident()})")
        return out

    def select(self, depth: int) -> str:
        parts = [self.kw("SELECT")]
        roll = self.r.random()
        if roll < 0.1:
            parts.append(self.kw("DISTINCT"))
        elif roll < 0.14:
            parts.append(self.join(self.kw("DISTINCT ON"), f"({self.expr_list(depth + 1)})"))
        elif roll < 0.15:
            parts.append(self.kw("ALL"))
        items = []
        for _ in range(self.r.choice([1, 1, 2, 3])):
            roll = self.r.random()
            if roll < 0.1:
                items.append("*")
            elif roll < 0.13:
                items.append(f"{self.ident()}.*")
            else:
                item = self.expr(depth + 1)
                roll = self.r.random()
                if roll < 0.2:
                    item = self.join(item, self.ident())
                elif roll < 0.35:
                    item = self.join(item, self.kw("AS"), self.ident())
                items.append(item)
        parts.append(", ".join(items))
        if self.r.random() < 0.75:
            parts.append(self.kw("FROM"))
            parts.append(", ".join(self.from_item(depth) for _ in range(self.r.choice([1, 1, 2]))))
        if self.r.random() < 0.35:
            parts.append(self.join(self.kw("WHERE"), self.expr(depth + 1)))
        if self.r.random() < 0.15:
            parts.append(self.join(self.kw("GROUP BY"), self.expr_list(depth + 1)))
            if self.r.random() < 0.4:
                parts.append(self.join(self.kw("HAVING"), self.expr(depth + 1)))
        if self.r.random() < 0.05:
            parts.append(self.join(self.kw("QUALIFY"), self.expr(depth + 1)))
        if self.r.random() < 0.04:
            parts.append(self.join(self.kw("USING SAMPLE"), self.r.choice(["5", "10%", "1.5"]), self.kw(self.r.choice(["ROWS", "PERCENT", ""]))))
        return self.join(*parts)

    def term(self, depth: int) -> str:
        roll = self.r.random()
        if depth < 4 and roll < 0.1:
            return f"({self.query(depth + 1)})"
        if roll < 0.14:
            rows = ", ".join(f"({self.expr_list(depth + 1)})" for _ in range(self.r.choice([1, 2])))
            return self.join(self.kw("VALUES"), rows)
        if roll < 0.17:
            tail = self.join(self.kw("SELECT"), self.expr_list(depth + 1)) if self.r.random() < 0.5 else ""
            where = self.join(self.kw("WHERE"), self.expr(depth + 1)) if self.r.random() < 0.3 else ""
            return self.join(self.kw("FROM"), self.from_item(depth), tail, where)
        return self.select(depth)

    def query(self, depth: int = 0) -> str:
        parts = []
        if depth < 3 and self.r.random() < 0.15:
            ctes = []
            for _ in range(self.r.choice([1, 1, 2])):
                name = self.ident()
                if self.r.random() < 0.2:
                    name += f"({self.ident()})"
                body = self.query(depth + 1)
                ctes.append(self.join(name, self.kw("AS"), f"({body})"))
            head = self.kw("WITH")
            if self.r.random() < 0.2:
                head = self.join(head, self.kw("RECURSIVE"))
            parts.append(self.join(head, ", ".join(ctes)))
        body = self.term(depth)
        while depth < 4 and self.r.random() < 0.15:
            op = self.kw(self.r.choice(["UNION", "UNION ALL", "INTERSECT", "EXCEPT", "UNION DISTINCT", "EXCEPT ALL"]))
            body = self.join(body, op, self.term(depth + 1))
        parts.append(body)
        if self.r.random() < 0.15:
            parts.append(self.join(self.kw("ORDER BY"), self.order_list(depth + 1)))
        roll = self.r.random()
        if roll < 0.1:
            parts.append(self.join(self.kw("LIMIT"), self.r.choice(["5", "1", "ALL", "1.5"])))
            if self.r.random() < 0.4:
                parts.append(self.join(self.kw("OFFSET"), "2"))
        elif roll < 0.12:
            parts.append(self.join(self.kw("OFFSET"), "2"))
        return self.join(*parts)

    def write(self) -> str:
        target = self.r.choice(["t", "main.t", "other.t", "temp.t", '"t"', "main.s.t", "MAIN.t"])
        roll = self.r.random()
        if roll < 0.25:
            head = self.kw(self.r.choice(["CREATE TABLE", "CREATE OR REPLACE TABLE", "CREATE TABLE IF NOT EXISTS", "CREATE TEMP TABLE", "CREATE VIEW"]))
            if self.r.random() < 0.6:
                return self.join(head, target, self.kw("AS"), self.query(1))
            cols = ", ".join(self.join(self.ident(), self.type_name()) for _ in range(self.r.choice([1, 2])))
            if self.r.random() < 0.1:
                cols += " PRIMARY KEY"
            return self.join(head, target, f"({cols})")
        if roll < 0.45:
            cols = f"({self.ident()})" if self.r.random() < 0.3 else ""
            if self.r.random() < 0.5:
                src = self.join(self.kw("VALUES"), f"({self.expr_list(1)})")
            else:
                src = self.query(1)
            tail = self.kw(" RETURNING *") if self.r.random() < 0.05 else ""
            return self.join(self.kw("INSERT INTO"), target, cols, src, tail)
        if roll < 0.6:
            sets = ", ".join(f"{self.ident()} = {self.expr(1)}" for _ in range(self.r.choice([1, 2])))
            where = self.join(self.kw("WHERE"), self.expr(1)) if self.r.random() < 0.5 else ""
            return self.join(self.kw("UPDATE"), target, self.kw("SET"), sets, where)
        if roll < 0.75:
            where = self.join(self.kw("WHERE"), self.expr(1)) if self.r.random() < 0.5 else ""
            return self.join(self.kw("DELETE FROM"), target, where)
        if roll < 0.85:
            head = self.kw(self.r.choice(["DROP TABLE", "DROP TABLE IF EXISTS", "DROP VIEW", "TRUNCATE"]))
            return self.join(head, target)
        if roll < 0.95:
            col = self.kw(self.r.choice(["ADD COLUMN", "ADD", "DROP COLUMN"]))
            return self.join(self.kw("ALTER TABLE"), target, col, self.ident(), self.type_name())
        return self.query()

    def copy(self) -> str:
        target = self.r.choice(["'tmp/out.csv'", "'out'", "'/dev/null'", "''", "'a''b'", "'x:y'", "'-'", "'a\\b'", "tmp/out", "' '"])
        opts = []
        for _ in range(self.r.choice([0, 0, 1, 2, 3])):
            opts.append(
                self.r.choice(
                    ["HEADER true", "HEADER false", "HEADER", "HEADER 1", "FORMAT CSV", "FORMAT 'csv'",
                     "FORMAT parquet", "DELIMITER ','", "QUOTE '\"'", "ESCAPE '\\'", "NULL 'none'",
                     "NULL NULL", "PARTITION_BY (a)", "DELIMITER x"]
                )
            )
        tail = f"({', '.join(opts)})" if opts else ""
        if tail and self.r.random() < 0.1:
            tail = "WITH " + tail
        return self.join(self.kw("COPY"), f"({self.query(1)})", self.kw("TO"), target, tail)

    def temp(self) -> str:
        head = self.kw(
            self.r.choice(
                ["CREATE TEMP TABLE", "CREATE TEMPORARY TABLE", "CREATE OR REPLACE TEMP TABLE",
                 "CREATE TEMP TABLE IF NOT EXISTS"]
            )
        )
        name = self.r.choice(["n", "temp.n", "main.n", "x"])
        return self.join(head, name, self.kw("AS"), self.query(1))


TOKENS = (
    ["(", ")", ",", ".", ";", "*", "=", "-", "--", "/*", "*/", "'", '"', "`", "[", "]", "$$", "@", "#", "?", ":", "::", "||", "{", "}", "\\"]
    + ["SELECT", "FROM", "WHERE", "INTO", "AS", "ON", "JOIN", "UNION", "AND", "NOT", "NULL", "CAST", "OVER", "WITH", "VALUES", "TABLE", "USING", "SAMPLE", "LIMIT", "OFFSET", "FETCH", "TOP", "FOR", "UPDATE", "LOCK", "SHARE", "INTERVAL", "COLLATE", "ESCAPE", "EXCEPT", "QUALIFY", "WINDOW", "LATERAL", "ORDER", "BY", "ALL", "DISTINCT"]
    + ["a", "t", "1", "'x'", "count(*)", "stats(x)", "foo", "bar", "query('x')", "N'x'", "E'x'", "x'00'", "1e", "0x1"]
)


def mutate(rng: random.Random, sql: str) -> str:
    parts = sql.split(" ")
    for _ in range(rng.choice([1, 1, 2, 3])):
        roll = rng.random()
        k = rng.randrange(len(parts) + 1)
        if roll < 0.4:
            parts.insert(k, rng.choice(TOKENS))
        elif roll < 0.6 and len(parts) > 1:
            del parts[min(k, len(parts) - 1)]
        elif roll < 0.7 and len(parts) > 1:
            i = min(k, len(parts) - 1)
            parts.insert(i, parts[i])
        elif roll < 0.85:
            text = " ".join(parts)
            if text:
                i = rng.randrange(len(text))
                text = text[:i] + rng.choice(["", " ", "(", ")", "'", "-", ",", "\n", "/", "*"]) + text[i + 1 :]
            parts = text.split(" ")
        else:
            i = min(k, len(parts) - 1)
            parts[i] = parts[i] + rng.choice(TOKENS)
    return " ".join(parts)


def records_for(rng: random.Random, gen: Gen, n: int, seeds: list[dict]) -> list[dict]:
    records = []
    seed_sql = [s["sql"] for s in seeds]
    for _ in range(n):
        roll = rng.random()
        dialect = rng.choice(DIALECTS)
        if roll < 0.45:
            sql = gen.query()
        elif roll < 0.55:
            sql = mutate(rng, gen.query())
        elif roll < 0.62 and seed_sql:
            sql = mutate(rng, rng.choice(seed_sql))
        elif roll < 0.72:
            sql = gen.temp()
            if rng.random() < 0.3:
                sql = mutate(rng, sql)
            records.append(
                {"fn": "is_readonly_sql", "sql": sql, "opts": {"dialect": dialect, "allow_temp_tables": True}}
            )
            continue
        elif roll < 0.86:
            sql = gen.write()
            if rng.random() < 0.2:
                sql = mutate(rng, sql)
            if rng.random() < 0.2:
                sql = sql + ";\n" + gen.write()
            records.append({"fn": "duckdb_writes_only_main", "sql": sql, "opts": {}})
            continue
        else:
            sql = gen.copy()
            if rng.random() < 0.25:
                sql = mutate(rng, sql)
            records.append({"fn": "duckdb_copy_export_target", "sql": sql, "opts": {}})
            continue
        opts = {"dialect": dialect}
        if dialect == "duckdb" and rng.random() < 0.5:
            opts["bracket_identifiers"] = False
        if rng.random() < 0.1:
            opts["allow_multiple"] = True
        records.append({"fn": "is_readonly_sql", "sql": sql, "opts": opts})
        if dialect == "duckdb" and rng.random() < 0.3:
            records.append({"fn": "duckdb_copy_export_target", "sql": f"COPY ({sql}) TO 'tmp/o.csv'", "opts": {}})
            records.append({"fn": "duckdb_writes_only_main", "sql": f"CREATE TABLE main.r AS {sql}", "opts": {}})
    return records


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", type=int, default=20000)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--seeds", action="append", default=[])
    ap.add_argument("--show", type=int, default=10)
    ap.add_argument("--save", help="write generated records (JSON lines) here")
    args = ap.parse_args()
    rng = random.Random(args.seed)
    seeds = []
    for path in args.seeds:
        seeds += [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]
    records = records_for(rng, Gen(rng), args.count, seeds)
    if args.save:
        Path(args.save).write_text("".join(json.dumps(r) + "\n" for r in records))
    build()
    ok = compare(records, args.show)
    true_count = sum(1 for r in records if r.get("result") is True or (r["fn"] == "duckdb_copy_export_target" and r.get("result")))
    print(f"python-verified records: {true_count} of {len(records)}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
