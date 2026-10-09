//! Port of `src/dippy/cli/mysql.py`.

use super::{Classification, Describe, HandlerContext};
use crate::sql::{ReadonlyOptions, is_readonly_sql, literal_sql_arg};

pub const COMMANDS: &[&str] = &["mysql"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// SQL from `-e`/`--execute`; `None` when absent or uncertain.
fn extract_execute_sql(ctx: &HandlerContext) -> Option<String> {
    let tokens = &ctx.tokens;
    for (i, token) in tokens.iter().enumerate() {
        if (token == "-e" || token == "--execute") && i + 1 < tokens.len() {
            return literal_sql_arg(ctx, i + 1, "");
        }
        if token.starts_with("--execute=") {
            return literal_sql_arg(ctx, i, "--execute=");
        }
        if token.starts_with("-e") && token.chars().count() > 2 {
            return literal_sql_arg(ctx, i, "-e");
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens
        .iter()
        .any(|t| matches!(t.as_str(), "--help" | "-?" | "--version" | "-V"))
    {
        return Classification::allow_desc("mysql help/version");
    }
    let Some(sql) = extract_execute_sql(ctx) else {
        return Classification::ask_desc("mysql (interactive)");
    };
    let opts = ReadonlyOptions {
        extra_write: vec!["LOAD".into()],
        dialect: Some("mysql".into()),
        ..ReadonlyOptions::default()
    };
    match is_readonly_sql(&sql, &opts) {
        Some(true) => Classification::allow_desc("mysql (read-only query)"),
        Some(false) => Classification::ask_desc("mysql (write query)"),
        None => Classification::ask_desc("mysql (unknown query)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;
    use crate::sql::test_ctx;

    const CASES: &[(&str, bool)] = &[
        ("mysql --help", true),
        ("mysql -?", true),
        ("mysql --version", true),
        ("mysql -V", true),
        ("mysql -e 'SELECT * FROM users'", true),
        ("mysql --execute='SELECT * FROM users'", true),
        ("mysql -e 'SELECT * FROM users' mydb", true),
        ("mysql -D mydb -e 'SELECT * FROM users'", true),
        ("mysql --database=mydb -e 'SELECT 1'", true),
        ("mysql -h localhost -e 'SELECT 1'", true),
        ("mysql -u root -e 'SELECT 1'", true),
        ("mysql -e 'SHOW DATABASES'", true),
        ("mysql -e 'SHOW TABLES'", true),
        ("mysql -e 'DESCRIBE users'", true),
        ("mysql -e 'EXPLAIN SELECT * FROM users'", true),
        ("mysql -B -e 'SELECT 1'", true),
        ("mysql --batch -e 'SELECT 1'", true),
        ("mysql -H -e 'SELECT 1'", true),
        ("mysql --html -e 'SELECT 1'", true),
        ("mysql -X -e 'SELECT 1'", true),
        ("mysql --xml -e 'SELECT 1'", true),
        ("mysql -N -e 'SELECT 1'", true),
        ("mysql -e'SELECT 1'", true),
        ("mysql -e 'INSERT INTO users VALUES (1)'", false),
        ("mysql -e 'UPDATE users SET name = 1'", false),
        ("mysql -e 'DELETE FROM users'", false),
        ("mysql -e 'DROP TABLE users'", false),
        ("mysql -e 'CREATE TABLE users (id INT)'", false),
        ("mysql -e 'ALTER TABLE users ADD COLUMN x INT'", false),
        ("mysql -e 'TRUNCATE TABLE users'", false),
        ("mysql -e 'GRANT SELECT ON db.* TO user'", false),
        ("mysql", false),
        ("mysql mydb", false),
        ("mysql -u root mydb", false),
        ("mysql -h localhost -u root -p mydb", false),
        ("mysql -e 'SELECT 1; SELECT 2'", false),
        (
            "mysql -e 'LOAD DATA INFILE \"/tmp/data.csv\" INTO TABLE users'",
            false,
        ),
        ("mysql -e \"SELECT 1 /*!50000 INTO OUTFILE 'x' */\"", false),
        (
            "mysql -e 'SELECT * FROM t INTO OUTFILE \"/tmp/out\"'",
            false,
        ),
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
        assert_eq!(desc("mysql -e 'SELECT 1'"), "mysql (read-only query)");
        assert_eq!(desc("mysql -e 'DROP TABLE t'"), "mysql (write query)");
        assert_eq!(desc("mysql -e 'FOO'"), "mysql (unknown query)");
        assert_eq!(desc("mysql mydb"), "mysql (interactive)");
    }
}
