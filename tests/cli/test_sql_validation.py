"""Cross-handler SQL classification boundaries."""

from pathlib import Path

import pytest
from conftest import is_approved, needs_confirmation


def test_reported_duckdb_traffic_command(check):
    sql = (Path(__file__).parents[1] / "fixtures/sql/duckdb_traffic.sql").read_text()
    assert is_approved(
        check(f'duckdb -readonly -csv server-logs/gate2/gate2.db "{sql}"')
    )


def test_reported_duckdb_sampled_union_command(check):
    sql = (
        "(SELECT 'serazene' typ, time_local, c_ip, sc_status, sc_bytes, "
        "http_referer, cs_uri FROM vvbox WHERE domain='d.vvbox.cz' "
        "AND cs_uri LIKE '/vv_show_url.php?idc=%' "
        "AND substr(time_local,1,11)='01/Oct/2026' USING SAMPLE 8 ROWS) UNION ALL "
        "(SELECT 'bezne', time_local, c_ip, sc_status, sc_bytes, http_referer, "
        "cs_uri FROM vvbox WHERE domain='d.vvbox.cz' "
        "AND cs_uri LIKE '/vv_show_url.php?idk=%' "
        "AND substr(time_local,1,11)='01/Oct/2026' USING SAMPLE 5 ROWS)"
    )
    assert is_approved(
        check(f'duckdb -readonly server-logs/gate2/gate2.db "{sql}" -line')
    )


@pytest.mark.parametrize(
    "prefix",
    [
        "duckdb -readonly data.db",
        "psql -c",
        "mysql -e",
        "sqlite3 data.db",
        "sqlcmd query -q",
        "aws athena start-query-execution --query-string",
    ],
)
def test_parenthesized_queries_preserve_handler_boundaries(check, prefix):
    assert is_approved(check(f'{prefix} "(SELECT 1) UNION ALL (SELECT 2)"'))
    for sql in [
        "(SELECT fictional_effect())",
        "(SELECT 1) UNION ALL (SELECT fictional_effect())",
        "(WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x)",
        "(SELECT 1 INTO outfile)",
        "(SELECT 1); DROP TABLE t",
        "(SELECT $SQL)",
        ".shell touch tmp/out",
        "(SELECT 1); .output tmp/out",
    ]:
        assert needs_confirmation(check(f'{prefix} "{sql}"')), sql


@pytest.mark.parametrize(
    "prefix",
    [
        "duckdb -readonly data.db",
        "psql -c",
        "mysql -e",
        "sqlite3 data.db",
        "sqlcmd query -q",
        "aws athena start-query-execution --query-string",
    ],
)
def test_casts_preserve_handler_boundaries(check, prefix):
    assert is_approved(check(f'{prefix} "SELECT CAST(x AS INTEGER) FROM t"'))
    for sql in [
        "SELECT CAST(fictional_effect() AS INTEGER)",
        "WITH x AS (DELETE FROM t RETURNING *) SELECT CAST(a AS INTEGER) FROM x",
        "SELECT CAST(a AS INTEGER) FROM t; DROP TABLE t",
        "SELECT CAST(a AS INTEGER) INTO outfile FROM t",
        "SELECT CAST($SQL AS INTEGER)",
        ".shell touch tmp/out",
        "SELECT CAST(a AS INTEGER) FROM t; .output tmp/out",
    ]:
        assert needs_confirmation(check(f'{prefix} "{sql}"')), sql


@pytest.mark.parametrize(
    "command",
    [
        "mysql -e \"SELECT * FROM t INTO OUTFILE '/tmp/output'\"",
        "psql -c 'WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x'",
        "sqlite3 data.db \"SELECT writefile('/tmp/output','data')\"",
        "sqlite3 -readonly data.db '.output /tmp/output'",
        "sqlite3 -safe data.db 'DROP TABLE t'",
        "sqlite3 -readonly data.db",
        'sqlcmd query -q "SELECT * FROM t INTO outfile"',
    ],
)
def test_writes_and_unverified_sql_ask(check, command):
    assert needs_confirmation(check(command))


@pytest.mark.parametrize(
    "command",
    [
        'mysql -e "$SQL"',
        'psql -c "$SQL"',
        'sqlite3 data.db "$SQL"',
        'sqlcmd query -q "$SQL"',
        "mysql --execute=\"SELECT '$HOME'\"",
        "psql --command=\"SELECT '$HOME'\"",
        "sqlite3 data.db \"SELECT '$HOME'\"",
        "sqlcmd query -q \"SELECT '$HOME'\"",
        "aws athena start-query-execution --query-string \"SELECT '$HOME'\"",
        "mysql -e \"SELECT '$(touch /tmp/dippy-bypass)'\"",
    ],
)
def test_shell_expansion_in_sql_asks(check, command):
    assert needs_confirmation(check(command))


@pytest.mark.parametrize(
    "command",
    [
        "mysql -e 'SELECT count(*) FROM t'",
        "psql -c 'WITH x AS (SELECT 1) SELECT * FROM x'",
        "sqlite3 data.db 'SELECT 1'",
        'sqlcmd query -q "SELECT 1"',
        "mysql -e \"SELECT 'a$'\"",
        'aws athena start-query-execution --query-string "SELECT 1"',
    ],
)
def test_plain_literal_reads_allow(check, command):
    assert is_approved(check(command))


@pytest.mark.parametrize(
    "command",
    [
        "mysql -e \"SELECT 1 /*!50000 INTO OUTFILE 'tmp/out' */\"",
        "mysql --execute=\"SELECT 1 /*M!50000 INTO OUTFILE 'tmp/out' */\"",
        'sqlcmd query -q "SELECT NEXT VALUE FOR dbo.seq"',
        'sqlcmd query -q "WITH x AS (SELECT NEXT VALUE FOR dbo.seq n) SELECT * FROM x"',
        "duckdb -readonly data.db \"SELECT stats(query('DROP TABLE t'))\"",
        "duckdb -readonly data.db 'SELECT list_sum([1]); DROP TABLE t'",
        "duckdb -readonly data.db \"SELECT list_sum([1]); COPY (SELECT 1) TO 'elsewhere/out'\"",
        'duckdb -readonly data.db "SELECT stats($SQL)"',
        "duckdb -readonly data.db '.shell touch tmp/out'",
    ],
)
def test_three_sql_fixes_preserve_confirmation_boundaries(check, command):
    assert needs_confirmation(check(command))


@pytest.mark.parametrize(
    "command",
    [
        'duckdb -readonly data.db "SELECT stats(x) FROM t"',
        "duckdb -readonly data.db 'SELECT list_sum([1, 2, 3])'",
        "duckdb -readonly data.db 'SELECT list_sum([1]); SELECT stats(x) FROM t'",
        "sqlcmd query -q \"SELECT 'NEXT VALUE FOR seq'\"",
        "mysql -e \"SELECT '/*! INTO OUTFILE */'\"",
    ],
)
def test_three_sql_fixes_allow_literal_reads(check, command):
    assert is_approved(check(command))
