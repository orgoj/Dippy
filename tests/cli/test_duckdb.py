"""Test cases for duckdb."""

import pytest
from conftest import is_approved, needs_confirmation
from dippy.core.config import Config, Rule

CONCURRENCY_SQL = """WITH b AS (SELECT c_ip FROM access_mp2
WHERE substr(time_local,4,8)='Sep/2026' AND substr(time_local,1,2) BETWEEN '16' AND '30'
AND cs_uri LIKE '/_/v1/elasticsearch%' GROUP BY c_ip HAVING count(DISTINCT cs_user_agent) >= 10),
r AS (SELECT DISTINCT ON (rid) c_ip, regexp_extract(domain,'([^ ]+)$',1) host,
strptime(substr(time_local,1,20),'%d/%b/%Y:%H:%M:%S') t_end, TRY_CAST(request_time AS DOUBLE) rt
FROM access_mp2 WHERE substr(time_local,4,8)='Sep/2026'
AND substr(time_local,1,2) BETWEEN '16' AND '30' AND cs_uri LIKE '/_/v1/elasticsearch%'
AND c_ip NOT IN (SELECT c_ip FROM b) AND NOT (c_ip LIKE '158.173.3.%'
OR c_ip LIKE '158.173.20.%' OR c_ip LIKE '158.173.21.%' OR c_ip LIKE '158.173.75.%'
OR c_ip LIKE '158.173.79.%' OR c_ip LIKE '212.56.48.%')),
e AS (SELECT c_ip, t_end - to_milliseconds(CAST(coalesce(rt,0)*1000 AS BIGINT)) ts, 1 d
FROM r UNION ALL SELECT c_ip, t_end, -1 FROM r),
c AS (SELECT c_ip, ts, sum(d) OVER (PARTITION BY c_ip ORDER BY ts, d ROWS UNBOUNDED PRECEDING) conc FROM e),
s AS (SELECT c_ip, date_trunc('second', ts) sec, max(conc) mc FROM c GROUP BY c_ip, date_trunc('second', ts)),
rq AS (SELECT c_ip, count(*) reqs, arg_max(host,1) host FROM r GROUP BY c_ip)
SELECT s.c_ip, any_value(rq.host) host, any_value(rq.reqs) reqs, max(mc) max_conc,
quantile_disc(mc,0.99) p99_conc FROM s JOIN rq USING (c_ip) GROUP BY s.c_ip
ORDER BY max_conc DESC, reqs DESC LIMIT 15"""

POOL_EXPORT_SQL = """COPY (SELECT c_ip, count(DISTINCT rid) n,
count(DISTINCT cs_user_agent) uas FROM access_mp2
WHERE substr(time_local,4,8)='Sep/2026' AND substr(time_local,1,2) BETWEEN '16' AND '30'
AND cs_uri LIKE '/_/v1/elasticsearch%' GROUP BY c_ip
HAVING count(DISTINCT cs_user_agent) >= 10 ORDER BY n DESC)
TO 'tmp/wdt_traffic-tos-pool-ips.txt' (HEADER false, DELIMITER ' ')"""


@pytest.mark.parametrize("flags", ["", "-readonly", "-safe"])
def test_reported_pool_export_obeys_redirect_rules(check, tmp_path, flags):
    command = (
        f'duckdb {flags} -noheader -separator " " -list data.db "{POOL_EXPORT_SQL}"'
    )
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])
    assert is_approved(check(command, config, tmp_path))
    assert needs_confirmation(check(command, Config(), tmp_path))
    denied = Config(redirect_rules=[Rule("allow", "tmp/**"), Rule("deny", "tmp/**")])
    assert (
        check(command, denied, tmp_path)["hookSpecificOutput"]["permissionDecision"]
        == "deny"
    )


@pytest.mark.parametrize("flags", ["", "-readonly", "-safe"])
@pytest.mark.parametrize(
    "extra", ["DROP TABLE t", "INSTALL httpfs", "SELECT writefile('x','y')"]
)
@pytest.mark.parametrize("first", [True, False])
def test_export_cannot_hide_other_effects(check, tmp_path, flags, extra, first):
    sql = f"{extra}; {POOL_EXPORT_SQL}" if first else f"{POOL_EXPORT_SQL}; {extra}"
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])
    assert needs_confirmation(
        check(f'duckdb {flags} tmp/data.db "{sql}"', config, tmp_path)
    )


@pytest.mark.parametrize(
    "suffix,approved",
    [
        ("; SELECT 2", True),
        ("; COPY (SELECT 2) TO 'tmp/second.csv'", True),
        ("; COPY (SELECT 2) TO 'elsewhere/second.csv'", False),
    ],
)
def test_export_checks_all_output_targets(check, tmp_path, suffix, approved):
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])
    result = check(
        f'duckdb -readonly data.db "{POOL_EXPORT_SQL}{suffix}"', config, tmp_path
    )
    assert is_approved(result) if approved else needs_confirmation(result)


def test_exports_in_separate_sql_arguments(check, tmp_path):
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])
    command = "duckdb -readonly -cmd \"COPY (SELECT 1) TO 'tmp/one'\" data.db \"COPY (SELECT 2) TO 'elsewhere/two'\""
    assert needs_confirmation(check(command, config, tmp_path))


@pytest.mark.parametrize(
    "sql",
    [
        "COPY t TO 'tmp/out'",
        "COPY t FROM 'tmp/in'",
        "COPY (SELECT 1) TO /tmp/dippy-out",
        "COPY (SELECT 1) TO 'https://example.com/out'",
        "COPY (SELECT 1) TO 'tmp/out' (PARTITION_BY (x))",
        "COPY (SELECT 1) TO 'tmp/out' (PER_THREAD_OUTPUT true)",
        "COPY (SELECT query('DELETE FROM t')) TO 'tmp/out'",
        "COPY (SELECT 1) TO 'tmp/$FILE'",
        "COPY (SELECT 1) TO 'tmp/$(touch /tmp/dippy-bypass)'",
        ".output tmp/out",
        ".shell touch tmp/out",
    ],
)
def test_unsupported_exports_ask(check, tmp_path, sql):
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])
    assert needs_confirmation(
        check(f'duckdb -readonly data.db "{sql}"', config, tmp_path)
    )


@pytest.mark.parametrize(
    "suffix,approved",
    [
        ("", True),
        ("; COPY (SELECT 1) TO 'tmp/output'", False),
        ("; INSTALL httpfs", False),
        ("; DELETE FROM access_mp2", False),
    ],
)
def test_reported_concurrency_query(check, suffix, approved):
    sql = (CONCURRENCY_SQL + suffix).replace("$", r"\$")
    result = check(f'duckdb -readonly -csv server-logs/gate2/gate2.db "{sql}"')
    assert is_approved(result) if approved else needs_confirmation(result)


TESTS = [
    # Help/version - safe
    ("duckdb --help", True),
    ("duckdb -help", True),
    ("duckdb -version", True),
    # Read-only and safe flags do not prove an interactive or write query safe
    ("duckdb -readonly mydb.db", False),
    ("duckdb -readonly mydb.db 'DROP TABLE users'", False),
    ("duckdb -safe mydb.db", False),
    ("duckdb -safe mydb.db 'DROP TABLE users'", False),
    ("duckdb -readonly mydb.db 'SELECT 1'", True),
    ("duckdb -safe mydb.db 'SELECT 1'", True),
    # Read-only SQL (positional)
    ("duckdb mydb.db 'SELECT * FROM users'", True),
    ("duckdb mydb.db 'SELECT * FROM users;'", True),
    ("duckdb mydb.db 'EXPLAIN SELECT * FROM users'", True),
    ("duckdb :memory: 'SELECT 1'", True),
    # Read-only SQL with -c/-s
    ("duckdb -c 'SELECT 1'", True),
    ("duckdb -s 'SELECT 1'", True),
    ("duckdb mydb.db -c 'SELECT * FROM users'", True),
    # Read-only with output options
    ("duckdb -json mydb.db 'SELECT * FROM users'", True),
    ("duckdb -csv mydb.db 'SELECT * FROM users'", True),
    ("duckdb -table mydb.db 'SELECT * FROM users'", True),
    ("duckdb -line mydb.db 'SELECT * FROM users'", True),
    ("duckdb -column mydb.db 'SELECT * FROM users'", True),
    # Read-only with -cmd
    ("duckdb -cmd 'SELECT 1' mydb.db", True),
    # Write SQL - unsafe
    ("duckdb mydb.db 'INSERT INTO users VALUES (1)'", False),
    ("duckdb mydb.db 'UPDATE users SET name = 1'", False),
    ("duckdb mydb.db 'DELETE FROM users'", False),
    ("duckdb mydb.db 'DROP TABLE users'", False),
    ("duckdb mydb.db 'CREATE TABLE users (id INT)'", False),
    ("duckdb mydb.db 'ALTER TABLE users ADD COLUMN x INT'", False),
    ("duckdb -c 'DROP TABLE users'", False),
    ("duckdb -s 'INSERT INTO t VALUES (1)'", False),
    # DuckDB-specific write operations
    ("duckdb mydb.db 'PRAGMA enable_progress_bar'", False),
    ("duckdb mydb.db 'ATTACH DATABASE other.db'", False),
    ("duckdb mydb.db 'DETACH DATABASE other'", False),
    ("duckdb mydb.db 'VACUUM'", False),
    ("duckdb mydb.db 'COPY users TO /tmp/out.csv'", False),
    ("duckdb mydb.db 'EXPORT DATABASE /tmp/backup'", False),
    ("duckdb mydb.db 'IMPORT DATABASE /tmp/backup'", False),
    # Write with -cmd
    ("duckdb -cmd 'DROP TABLE users' mydb.db", False),
    # Interactive mode - unsafe
    ("duckdb", False),
    ("duckdb mydb.db", False),
    ("duckdb -json mydb.db", False),
    # Multiple verified read-only statements
    ("duckdb mydb.db 'SELECT 1; SELECT 2'", True),
    ("duckdb -readonly -csv mydb.db 'SELECT 1; SELECT 2;'", True),
    ("duckdb -readonly mydb.db 'SELECT 1; -- comment\nSELECT 2'", True),
    ("duckdb -readonly mydb.db \"SELECT ';' AS delimiter; SELECT 2\"", True),
    ("duckdb -readonly mydb.db 'SELECT 1; INSERT INTO items VALUES (2)'", False),
    ("duckdb mydb.db 'SELECT 1; ; SELECT 2'", False),
    # File input - unsafe
    ("duckdb -init script.sql mydb.db", False),
    ("duckdb -f script.sql", False),
    # Options with arguments - should parse correctly
    ("duckdb -separator '|' mydb.db 'SELECT 1'", True),
    # -newline takes ONE argument (SEP)
    ("duckdb -newline '\\n' mydb.db 'SELECT 1'", True),
    # -nullvalue takes ONE argument (TEXT)
    ("duckdb -nullvalue NULL mydb.db 'SELECT 1'", True),
]


@pytest.mark.parametrize("command,expected", TESTS)
def test_command(check, command: str, expected: bool):
    result = check(command)
    if expected:
        assert is_approved(result), f"Expected approve: {command}"
    else:
        assert needs_confirmation(result), f"Expected confirm: {command}"


def test_write_to_database_allowed_by_redirect_rule(check, tmp_path):
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])

    result = check(
        "duckdb tmp/work.db 'CREATE TABLE results AS SELECT 1 AS value'",
        config,
        tmp_path,
    )

    assert is_approved(result)


def test_readonly_attach_and_writes_to_allowed_database(check, tmp_path):
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])
    sql = (
        "ATTACH 'source.db' AS source (READ_ONLY); "
        "CREATE OR REPLACE TABLE results AS SELECT * FROM source.items; "
        "SELECT count(*) FROM results;"
    )

    result = check(f'duckdb tmp/work.db "{sql}"', config, tmp_path)

    assert is_approved(result)


@pytest.mark.parametrize(
    "command",
    [
        "duckdb data/work.db 'CREATE TABLE results AS SELECT 1'",
        "duckdb tmp/../../work.db 'CREATE TABLE results AS SELECT 1'",
        "duckdb tmp/$DATABASE 'CREATE TABLE results AS SELECT 1'",
        (
            "duckdb tmp/work.db \"ATTACH 'other.db' AS other; "
            'CREATE TABLE other.results AS SELECT 1;"'
        ),
        "duckdb tmp/work.db \"COPY (SELECT 1) TO '/etc/dippy-bypass'\"",
        "duckdb tmp/work.db 'DROP SECRET production_credentials'",
        "duckdb tmp/work.db 'CREATE TABLE other_db.t AS SELECT 1'",
        "duckdb tmp/work.db 'INSERT INTO other_db.t SELECT 1'",
        "duckdb tmp/work.db \"CREATE TABLE t AS SELECT query('COPY (SELECT 1) TO /tmp/out')\"",
        "duckdb tmp/work.db \"CREATE TABLE t AS SELECT writefile('/tmp/out','x')\"",
    ],
)
def test_database_redirect_rule_does_not_allow_external_writes(
    check, tmp_path, command
):
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])

    result = check(command, config, tmp_path)

    assert needs_confirmation(result)


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE TABLE main.t AS SELECT 1",
        "INSERT INTO t SELECT 1",
        "UPDATE t SET x = 1",
    ],
)
def test_database_redirect_rule_allows_verified_main_writes(check, tmp_path, sql):
    config = Config(redirect_rules=[Rule("allow", "tmp/**")])

    assert is_approved(check(f'duckdb tmp/work.db "{sql}"', config, tmp_path))


@pytest.mark.parametrize(
    "command",
    [
        "duckdb -readonly work.db \"COPY (SELECT 1) TO '/tmp/output.csv'\"",
        "duckdb -readonly work.db \"EXPORT DATABASE '/tmp/backup'\"",
        'duckdb -safe work.db "INSTALL httpfs"',
        'duckdb -readonly work.db "LOAD httpfs"',
        "duckdb -readonly work.db \"SELECT 1; EXPORT DATABASE '/tmp/backup'\"",
        "duckdb -readonly work.db \"EXPLAIN ANALYZE COPY (SELECT 1) TO '/tmp/output.csv'\"",
        'duckdb -readonly work.db "EXPLAIN ANALYZE INSERT INTO items VALUES (1)"',
        "duckdb -readonly work.db \"ATTACH 'other.db' AS other\"",
        "duckdb -cmd 'SELECT 1' work.db \"EXPORT DATABASE '/tmp/backup'\"",
        "duckdb -readonly -cmd 'SELECT 1' work.db 'DROP TABLE items'",
        "duckdb work.db 'SELECT 1' 'DROP TABLE items'",
        "duckdb -readonly work.db \"SELECT 1; -- a comment\nCOPY (SELECT 1) TO '/tmp/output.csv'\"",
        "duckdb -readonly work.db \"SELECT [']']; INSTALL httpfs; --'\"",
        "duckdb -readonly work.db \"SELECT [']']; COPY (SELECT 1) TO '/tmp/output.csv'; --'\"",
        "duckdb -readonly work.db \"SELECT [']']\" '.tables'",
        "duckdb -readonly work.db \"SELECT E'\\\\' x '; COPY (SELECT 1) TO '/tmp/output.csv' --'\"",
        "duckdb -readonly work.db \"SELECT $$'$$; COPY (SELECT 1) TO '/tmp/output.csv' --'\"",
        'duckdb -readonly work.db "SELECT 1 -- comment\rINSTALL httpfs"',
    ],
)
def test_readonly_flags_do_not_approve_external_side_effects(check, command):
    assert needs_confirmation(check(command))


@pytest.mark.parametrize(
    "command",
    [
        "duckdb -readonly work.db \"SELECT * FROM access WHERE uri LIKE '%export%'\"",
        "duckdb -readonly work.db \"SELECT 'COPY; EXPORT; INSTALL; LOAD'\"",
        "duckdb -readonly work.db \"/* EXPORT DATABASE '/tmp/no' */ SELECT 1\"",
        "duckdb -readonly work.db \"WITH x AS (SELECT 'INSTALL') SELECT * FROM x\"",
        "duckdb -readonly work.db 'SELECT [1, 2]'",
        "duckdb -readonly work.db 'SELECT [\"export\"]'",
        "duckdb -readonly work.db 'SELECT $$export$$'",
        'duckdb -readonly work.db "SELECT "\'1\'""',
    ],
)
def test_readonly_query_mentions_side_effect_words_as_data(check, command):
    assert is_approved(check(command))


def test_readonly_aggregate_queries_in_one_invocation(check):
    sql = (
        "SELECT 'alpha' t, substr(time_local,13,5) m, count(*) n FROM alpha "
        "WHERE time_local LIKE '28/Sep/2026:%' GROUP BY 1,2 ORDER BY 2 DESC LIMIT 2; "
        "SELECT 'beta' t, substr(time_local,13,5) m, count(*) n FROM beta "
        "WHERE time_local LIKE '28/Sep/2026:%' GROUP BY 1,2 ORDER BY 2 DESC LIMIT 2;"
    )

    assert is_approved(check(f'duckdb -readonly -csv data.db "{sql}"'))


def test_shell_escaped_sql_literals_reach_shared_checker(check):
    sql = (
        "SELECT domain, regexp_extract(cs_uri_query,'action=([A-Za-z_.]+)',1) akce, "
        "(cs_uri_query LIKE '%\\%27%' ESCAPE '\\' OR "
        "cs_uri_query LIKE '%\\%2F\\%2A%' ESCAPE '\\' OR "
        "cs_uri_query LIKE '%library_version=%.&%' ESCAPE '\\') payload, "
        "sc_status, sc_bytes, count(*) n FROM wdt "
        "WHERE time_local LIKE '28/Sep/2026:%' AND c_ip='89.163.154.141' "
        "GROUP BY ALL ORDER BY akce, payload, sc_status"
    )
    command = f'duckdb -readonly -csv server-logs/ferda7/ferda7.db "{sql}"'

    assert is_approved(check(command))


def test_shell_decoding_does_not_hide_sql_write(check):
    assert needs_confirmation(
        check('duckdb -readonly data.db "SELECT 1; \\INSTALL httpfs"')
    )


def test_readonly_batch_with_temp_table_and_select(check):
    sql = (
        "CREATE TEMP TABLE n AS SELECT unnest(['200.162.155','151.240.46']) net; "
        "WITH a AS (SELECT 'access' t, rid, time_local, c_ip FROM access "
        "WHERE time_local LIKE '29/Sep/2026:%') "
        "SELECT t, count(DISTINCT rid) FILTER (WHERE "
        "regexp_extract(c_ip,'^(\\d+\\.\\d+\\.\\d+)\\.',1) "
        "IN (SELECT net FROM n)) flood_req FROM a GROUP BY ALL"
    )

    assert is_approved(check(f'duckdb -readonly -csv data.db "{sql}"'))


def test_readonly_select_with_literal_regex_dollar(check):
    sql = (
        "SELECT DISTINCT ON (rid) regexp_extract(domain,'([^ ]+)$',1) dom "
        "FROM access WHERE time_local LIKE '29/Sep/2026:%'"
    )

    assert is_approved(check(f'duckdb -readonly -csv data.db "{sql}"'))


@pytest.mark.parametrize(
    "sql",
    [
        "CREATE TABLE n AS SELECT 1; SELECT * FROM n",
        "CREATE TEMP TABLE main.n AS SELECT 1; SELECT * FROM n",
        "CREATE TEMP TABLE n (id INT); SELECT * FROM n",
        "CREATE TEMP TABLE n AS DELETE FROM data; SELECT * FROM n",
        "CREATE TEMP TABLE n AS SELECT 1; COPY (SELECT 1) TO '/tmp/out.csv'",
        "CREATE TEMP TABLE n AS SELECT 1; INSTALL httpfs",
    ],
)
def test_readonly_temp_table_batch_rejects_writes(check, sql):
    assert needs_confirmation(check(f'duckdb -readonly -csv data.db "{sql}"'))


@pytest.mark.parametrize(
    "expansion",
    [
        "$(touch /tmp/dippy-bypass)",
        "$((1+2))",
        "${HOME}",
        "$[1+2]",
        "$HOME",
        "$0",
        "$_",
        "$$",
        "$?",
        "$!",
        "$@",
        "$*",
        "$#",
        "$-",
    ],
)
def test_readonly_sql_rejects_shell_expansion(check, expansion):
    assert needs_confirmation(
        check(f"duckdb -readonly data.db \"SELECT '{expansion}'\"")
    )


def test_duckdb_readonly_inet_subnet_containment_auto_approves(check):
    command = (
        'duckdb -readonly -csv server-logs/gate2/gate2.db "WITH ips AS ('
        "SELECT DISTINCT c_ip FROM access WHERE substr(time_local,1,11) IN "
        "('04/Oct/2026','05/Oct/2026','06/Oct/2026','07/Oct/2026','08/Oct/2026') "
        "AND (cs_user_agent LIKE '%WP-Safe-Scanner%' OR cs_user_agent LIKE 'Hello from Palo Alto Networks%')) "
        "SELECT count(*) ips_celkem, count(il.label) ips_v_ip_labels, string_agg(DISTINCT il.label, ',') labels "
        'FROM ips LEFT JOIN ip_labels il ON TRY_CAST(ips.c_ip AS INET) <<= il.cidr::INET"'
    )
    assert is_approved(check(command))


def test_duckdb_cte_with_column_aliases_auto_approves(check):
    command = (
        'duckdb -readonly -csv server-logs/gate2/gate2.db "WITH u AS ('
        "SELECT DISTINCT ON (rid) rid, c_ip FROM access), "
        "nase(ip, kdo) AS (VALUES ('145.239.12.84','gate2')) "
        'SELECT u.c_ip, n.kdo FROM u LEFT JOIN nase n ON u.c_ip=n.ip"'
    )
    assert is_approved(check(command))
