//! Port of `src/dippy/cli/sqlite3.py`.

use super::{Classification, Describe, HandlerContext};
use crate::sql::{ReadonlyOptions, is_readonly_sql, literal_sql_arg, py_lstrip};

pub const COMMANDS: &[&str] = &["sqlite3"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// SQLite-specific keywords that perform writes or modifications.
const SQLITE_WRITE: [&str; 6] = ["PRAGMA", "ATTACH", "DETACH", "VACUUM", "REINDEX", "ANALYZE"];

/// Options that take no argument.
const FLAGS: &[&str] = &[
    "-append",
    "-ascii",
    "-bail",
    "-batch",
    "-box",
    "-column",
    "-csv",
    "-deserialize",
    "-echo",
    "-header",
    "-noheader",
    "-help",
    "-html",
    "-interactive",
    "-json",
    "-line",
    "-list",
    "-markdown",
    "-memtrace",
    "-nofollow",
    "-quote",
    "-readonly",
    "-safe",
    "-stats",
    "-table",
    "-tabs",
    "-version",
    "-vfstrace",
];

/// Options that take one argument.
const ARG_OPTIONS: &[&str] = &[
    "-cmd",
    "-init",
    "-key",
    "-hexkey",
    "-textkey",
    "-maxsize",
    "-newline",
    "-nonce",
    "-nullvalue",
    "-pagecache",
    "-separator",
    "-vfs",
    "-escape",
    "-A",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens
        .iter()
        .any(|t| matches!(t.as_str(), "-help" | "--help" | "-version"))
    {
        return Classification::allow_desc("sqlite3 help/version");
    }
    if tokens.iter().any(|t| t == "-init") {
        return Classification::ask_desc("sqlite3 (init script)");
    }

    // sqlite3 [OPTIONS] [FILENAME [SQL...]], plus -cmd COMMAND
    let mut sql_indices = Vec::new();
    let mut i = 1;
    let mut filename_seen = false;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if FLAGS.contains(&token) {
            i += 1;
            continue;
        }
        if ARG_OPTIONS.contains(&token) {
            if token == "-cmd" && i + 1 < tokens.len() {
                sql_indices.push(i + 1);
            }
            i += 2;
            continue;
        }
        if token == "-lookaside" {
            i += 3;
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        if !filename_seen {
            filename_seen = true;
            i += 1;
            continue;
        }
        sql_indices.push(i);
        i += 1;
    }

    if sql_indices.is_empty() {
        return Classification::ask_desc("sqlite3 (interactive)");
    }
    let parts: Option<Vec<String>> = sql_indices
        .iter()
        .map(|&i| literal_sql_arg(ctx, i, ""))
        .collect();
    let Some(parts) = parts else {
        return Classification::ask_desc("sqlite3 (ambiguous SQL argument)");
    };
    if parts.iter().any(|p| py_lstrip(p).starts_with('.')) {
        return Classification::ask_desc("sqlite3 (dot-command)");
    }
    let sql = parts.join(";\n");
    let opts = ReadonlyOptions {
        extra_write: SQLITE_WRITE.iter().map(|s| s.to_string()).collect(),
        dialect: Some("sqlite".into()),
        ..ReadonlyOptions::default()
    };
    match is_readonly_sql(&sql, &opts) {
        Some(true) => Classification::allow_desc("sqlite3 (read-only query)"),
        Some(false) => Classification::ask_desc("sqlite3 (write query)"),
        None => Classification::ask_desc("sqlite3 (unknown query)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;
    use crate::sql::test_ctx;

    const CASES: &[(&str, bool)] = &[
        ("sqlite3 --help", true),
        ("sqlite3 -help", true),
        ("sqlite3 -version", true),
        ("sqlite3 -readonly mydb.db", false),
        ("sqlite3 -readonly mydb.db 'DROP TABLE users'", false),
        ("sqlite3 -safe mydb.db", false),
        ("sqlite3 -safe mydb.db 'DROP TABLE users'", false),
        ("sqlite3 mydb.db 'SELECT * FROM users'", true),
        ("sqlite3 mydb.db 'SELECT * FROM users;'", true),
        ("sqlite3 mydb.db 'EXPLAIN SELECT * FROM users'", true),
        ("sqlite3 :memory: 'SELECT 1'", true),
        ("sqlite3 -json mydb.db 'SELECT * FROM users'", true),
        ("sqlite3 -csv mydb.db 'SELECT * FROM users'", true),
        ("sqlite3 -table mydb.db 'SELECT * FROM users'", true),
        ("sqlite3 -line mydb.db 'SELECT * FROM users'", true),
        ("sqlite3 -column mydb.db 'SELECT * FROM users'", true),
        ("sqlite3 -header mydb.db 'SELECT * FROM users'", true),
        ("sqlite3 -cmd 'SELECT 1' mydb.db", true),
        ("sqlite3 mydb.db 'INSERT INTO users VALUES (1)'", false),
        ("sqlite3 mydb.db 'UPDATE users SET name = 1'", false),
        ("sqlite3 mydb.db 'DELETE FROM users'", false),
        ("sqlite3 mydb.db 'DROP TABLE users'", false),
        ("sqlite3 mydb.db 'CREATE TABLE users (id INT)'", false),
        (
            "sqlite3 mydb.db 'ALTER TABLE users ADD COLUMN x INT'",
            false,
        ),
        ("sqlite3 mydb.db 'PRAGMA foreign_keys = ON'", false),
        ("sqlite3 mydb.db 'ATTACH DATABASE other.db AS other'", false),
        ("sqlite3 mydb.db 'DETACH DATABASE other'", false),
        ("sqlite3 mydb.db 'VACUUM'", false),
        ("sqlite3 mydb.db 'REINDEX'", false),
        ("sqlite3 mydb.db 'ANALYZE'", false),
        ("sqlite3 -cmd 'DROP TABLE users' mydb.db", false),
        ("sqlite3", false),
        ("sqlite3 mydb.db", false),
        ("sqlite3 -json mydb.db", false),
        ("sqlite3 mydb.db 'SELECT 1; SELECT 2'", false),
        ("sqlite3 -init script.sql mydb.db", false),
        ("sqlite3 -separator '|' mydb.db 'SELECT 1'", true),
        ("sqlite3 -nullvalue NULL mydb.db 'SELECT 1'", true),
        ("sqlite3 -newline '\\n' mydb.db 'SELECT 1'", true),
        ("sqlite3 -lookaside 100 50 mydb.db 'SELECT 1'", true),
        ("sqlite3 mydb.db '.shell touch x'", false),
        ("sqlite3 mydb.db 'SELECT 1' 'SELECT 2'", false),
        ("sqlite3 mydb.db \"SELECT writefile('x', 'data')\"", false),
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
        assert_eq!(desc("sqlite3 db 'SELECT 1'"), "sqlite3 (read-only query)");
        assert_eq!(desc("sqlite3 db 'DROP TABLE t'"), "sqlite3 (write query)");
        assert_eq!(desc("sqlite3 db '.tables'"), "sqlite3 (dot-command)");
        assert_eq!(desc("sqlite3 db"), "sqlite3 (interactive)");
        assert_eq!(desc("sqlite3 -init x db"), "sqlite3 (init script)");
    }
}
