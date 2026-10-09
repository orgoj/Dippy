//! Port of `src/dippy/cli/psql.py`.

use super::{Classification, Describe, HandlerContext};
use crate::sql::{ReadonlyOptions, is_readonly_sql, literal_sql_arg};

pub const COMMANDS: &[&str] = &["psql"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// PostgreSQL-specific keywords that perform writes or modifications.
const POSTGRES_WRITE: [&str; 5] = ["COPY", "VACUUM", "CLUSTER", "REINDEX", "ANALYZE"];

/// SQL from `-c`/`--command` options.
fn extract_command_sql(ctx: &HandlerContext) -> Vec<Option<String>> {
    let tokens = &ctx.tokens;
    let mut sql_list = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if (token == "-c" || token == "--command") && i + 1 < tokens.len() {
            sql_list.push(literal_sql_arg(ctx, i + 1, ""));
            i += 2;
            continue;
        }
        if token.starts_with("--command=") {
            sql_list.push(literal_sql_arg(ctx, i, "--command="));
        }
        i += 1;
    }
    sql_list
}

fn has_file_option(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|t| t == "-f" || t == "--file" || t.starts_with("--file=") || t.starts_with("-f"))
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let has = |t: &str| tokens.iter().any(|x| x == t);

    if has("--help") || has("--version") || has("-V") {
        return Classification::allow_desc("psql help/version");
    }
    if has("-l") || has("--list") {
        return Classification::allow_desc("psql --list");
    }
    if has_file_option(tokens) {
        return Classification::ask_desc("psql (file input)");
    }
    let sql_list = extract_command_sql(ctx);
    if sql_list.is_empty() {
        return Classification::ask_desc("psql (interactive)");
    }
    let opts = ReadonlyOptions {
        extra_write: POSTGRES_WRITE.iter().map(|s| s.to_string()).collect(),
        dialect: Some("postgres".into()),
        ..ReadonlyOptions::default()
    };
    for sql in &sql_list {
        let Some(sql) = sql else {
            return Classification::ask_desc("psql (ambiguous SQL argument)");
        };
        match is_readonly_sql(sql, &opts) {
            Some(false) => return Classification::ask_desc("psql (write query)"),
            None => return Classification::ask_desc("psql (unknown query)"),
            Some(true) => {}
        }
    }
    Classification::allow_desc("psql (read-only query)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;
    use crate::sql::test_ctx;

    const CASES: &[(&str, bool)] = &[
        ("psql --help", true),
        ("psql --version", true),
        ("psql -V", true),
        ("psql -l", true),
        ("psql --list", true),
        ("psql -l -h localhost", true),
        ("psql -c 'SELECT * FROM users'", true),
        ("psql --command='SELECT * FROM users'", true),
        ("psql -c 'SELECT * FROM users' mydb", true),
        ("psql -d mydb -c 'SELECT * FROM users'", true),
        ("psql --dbname=mydb -c 'SELECT 1'", true),
        ("psql -h localhost -c 'SELECT 1'", true),
        ("psql -U postgres -c 'SELECT 1'", true),
        ("psql -c 'SHOW search_path'", true),
        ("psql -c 'EXPLAIN SELECT * FROM users'", true),
        (
            "psql -c \"SELECT '192.168.1.5'::INET <<= '192.168.1.0/24'::INET\"",
            true,
        ),
        ("psql -A -c 'SELECT 1'", true),
        ("psql -H -c 'SELECT 1'", true),
        ("psql -t -c 'SELECT 1'", true),
        ("psql -x -c 'SELECT 1'", true),
        ("psql -c 'SELECT 1' -c 'SELECT 2'", true),
        ("psql -c 'INSERT INTO users VALUES (1)'", false),
        ("psql -c 'UPDATE users SET name = 1'", false),
        ("psql -c 'DELETE FROM users'", false),
        ("psql -c 'DROP TABLE users'", false),
        ("psql -c 'CREATE TABLE users (id INT)'", false),
        ("psql -c 'ALTER TABLE users ADD COLUMN x INT'", false),
        ("psql -c 'TRUNCATE TABLE users'", false),
        ("psql -c 'GRANT SELECT ON users TO alice'", false),
        ("psql -c 'COPY users FROM /tmp/data.csv'", false),
        ("psql -c 'VACUUM'", false),
        ("psql -c 'CLUSTER users'", false),
        ("psql -c 'REINDEX TABLE users'", false),
        ("psql -c 'ANALYZE users'", false),
        ("psql -c 'SELECT 1' -c 'DROP TABLE users'", false),
        ("psql", false),
        ("psql mydb", false),
        ("psql -U postgres mydb", false),
        ("psql -h localhost -U postgres -d mydb", false),
        ("psql -f script.sql", false),
        ("psql --file=script.sql", false),
        ("psql -f script.sql mydb", false),
        ("psql -c 'SELECT 1; SELECT 2'", false),
        ("psql -c \"SELECT '$HOME'\"", false),
    ];

    #[test]
    fn classifies_commands() {
        for (command, allowed) in CASES {
            let result = classify(&test_ctx(command));
            assert_eq!(result.action == Action::Allow, *allowed, "{command}");
        }
    }

    #[test]
    fn descriptions() {
        let desc = |c: &str| classify(&test_ctx(c)).description.unwrap();
        assert_eq!(desc("psql -c 'SELECT 1'"), "psql (read-only query)");
        assert_eq!(desc("psql -c 'DROP TABLE t'"), "psql (write query)");
        assert_eq!(desc("psql -c 'FOO'"), "psql (unknown query)");
        assert_eq!(desc("psql mydb"), "psql (interactive)");
        assert_eq!(desc("psql -c \"$SQL\""), "psql (ambiguous SQL argument)");
    }
}
