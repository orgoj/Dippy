"""Cross-handler SQL classification boundaries."""

import pytest
from conftest import is_approved, needs_confirmation


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
