//! Port of `src/dippy/cli/sqlcmd.py` (go-sqlcmd).

use super::{Classification, Describe, HandlerContext};
use crate::sql::{ReadonlyOptions, is_readonly_sql, literal_sql_arg};

pub const COMMANDS: &[&str] = &["sqlcmd"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Subcommands that don't modify anything.
const SAFE_SUBCOMMANDS: &[&str] = &["config", "open", "help", "completion"];

/// Subcommands that modify state.
const UNSAFE_SUBCOMMANDS: &[&str] = &["create", "install", "delete", "start", "stop"];

/// SQL of the `query` subcommand; `None` when absent or uncertain.
fn extract_query_sql(ctx: &HandlerContext) -> Option<String> {
    let tokens = &ctx.tokens;
    let query_idx = tokens.iter().position(|t| t == "query")?;
    let mut i = query_idx + 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if matches!(token, "-q" | "--query" | "-t" | "--text") && i + 1 < tokens.len() {
            return literal_sql_arg(ctx, i + 1, "");
        }
        if matches!(token, "-d" | "--database" | "-h" | "--help") {
            i += if matches!(token, "-d" | "--database") {
                2
            } else {
                1
            };
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        return literal_sql_arg(ctx, i, "");
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens
        .iter()
        .any(|t| matches!(t.as_str(), "--help" | "-h" | "--version"))
    {
        return Classification::allow_desc("sqlcmd help/version");
    }
    let Some(subcommand) = tokens.iter().skip(1).find(|t| !t.starts_with('-')) else {
        return Classification::ask_desc("sqlcmd (no subcommand)");
    };
    let subcommand = subcommand.as_str();
    if SAFE_SUBCOMMANDS.contains(&subcommand) {
        return Classification::allow_desc(format!("sqlcmd {subcommand}"));
    }
    if UNSAFE_SUBCOMMANDS.contains(&subcommand) {
        return Classification::ask_desc(format!("sqlcmd {subcommand}"));
    }
    if subcommand == "query" {
        let Some(sql) = extract_query_sql(ctx) else {
            return Classification::ask_desc("sqlcmd query (no SQL)");
        };
        let opts = ReadonlyOptions {
            dialect: Some("tsql".into()),
            ..ReadonlyOptions::default()
        };
        return match is_readonly_sql(&sql, &opts) {
            Some(true) => Classification::allow_desc("sqlcmd query (read-only)"),
            Some(false) => Classification::ask_desc("sqlcmd query (write)"),
            None => Classification::ask_desc("sqlcmd query (unknown)"),
        };
    }
    Classification::ask_desc(format!("sqlcmd {subcommand}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;
    use crate::sql::test_ctx;

    const CASES: &[(&str, bool)] = &[
        ("sqlcmd --help", true),
        ("sqlcmd -h", true),
        ("sqlcmd --version", true),
        ("sqlcmd query --help", true),
        ("sqlcmd config view", true),
        ("sqlcmd config cs", true),
        ("sqlcmd query 'SELECT @@VERSION'", true),
        ("sqlcmd query 'SELECT * FROM users'", true),
        ("sqlcmd query --query 'SELECT 1'", true),
        ("sqlcmd query -q 'SELECT 1'", true),
        ("sqlcmd query --text 'SELECT 1'", true),
        ("sqlcmd query -t 'SELECT 1'", true),
        ("sqlcmd query 'SELECT 1' --database master", true),
        ("sqlcmd query -d master 'SELECT 1'", true),
        ("sqlcmd query 'EXPLAIN SELECT * FROM users'", true),
        ("sqlcmd query 'INSERT INTO users VALUES (1)'", false),
        ("sqlcmd query 'UPDATE users SET name = 1'", false),
        ("sqlcmd query 'DELETE FROM users'", false),
        ("sqlcmd query 'DROP TABLE users'", false),
        ("sqlcmd query 'CREATE TABLE users (id INT)'", false),
        ("sqlcmd query 'ALTER TABLE users ADD x INT'", false),
        ("sqlcmd query 'TRUNCATE TABLE users'", false),
        ("sqlcmd query --query 'DROP TABLE users'", false),
        ("sqlcmd create mssql", false),
        ("sqlcmd create mssql --accept-eula", false),
        ("sqlcmd install mssql", false),
        ("sqlcmd delete", false),
        ("sqlcmd start", false),
        ("sqlcmd stop", false),
        ("sqlcmd open ads", true),
        ("sqlcmd", false),
        ("sqlcmd query 'SELECT 1; SELECT 2'", false),
        ("sqlcmd query \"SELECT '$HOME'\"", false),
        ("sqlcmd query", false),
        ("sqlcmd frobnicate", false),
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
        assert_eq!(desc("sqlcmd config view"), "sqlcmd config");
        assert_eq!(desc("sqlcmd start"), "sqlcmd start");
        assert_eq!(desc("sqlcmd query 'SELECT 1'"), "sqlcmd query (read-only)");
        assert_eq!(desc("sqlcmd query 'DROP TABLE t'"), "sqlcmd query (write)");
        assert_eq!(desc("sqlcmd query 'FOO'"), "sqlcmd query (unknown)");
        assert_eq!(desc("sqlcmd query"), "sqlcmd query (no SQL)");
        assert_eq!(desc("sqlcmd -x"), "sqlcmd (no subcommand)");
    }
}
