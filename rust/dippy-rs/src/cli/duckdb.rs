//! Port of `src/dippy/cli/duckdb.py`.

use super::{Classification, Describe, HandlerContext};
use crate::bash::decode_literal_word;
use crate::sql::{
    ReadonlyOptions, duckdb_copy_export_target, duckdb_writes_only_main, is_readonly_sql,
    py_lstrip, split_sql_statements,
};

pub const COMMANDS: &[&str] = &["duckdb"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// DuckDB-specific keywords that perform writes or modifications.
const DUCKDB_WRITE: [&str; 9] = [
    "PRAGMA", "ATTACH", "DETACH", "VACUUM", "COPY", "EXPORT", "IMPORT", "INSTALL", "LOAD",
];

/// Options that take no argument.
const FLAGS: &[&str] = &[
    "-ascii",
    "-bail",
    "-batch",
    "-box",
    "-column",
    "-csv",
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
    "-no-stdin",
    "-quote",
    "-readonly",
    "-safe",
    "-stats",
    "-table",
    "-tabs",
    "-unredacted",
    "-unsigned",
    "-version",
];

/// Options that take one argument.
const ARG_OPTIONS: &[&str] = &[
    "-cmd",
    "-init",
    "-separator",
    "-vfs",
    "-storage-version",
    "-newline",
    "-nullvalue",
];

fn options(allow_multiple: bool, allow_temp_tables: bool) -> ReadonlyOptions {
    ReadonlyOptions {
        extra_write: DUCKDB_WRITE.iter().map(|s| s.to_string()).collect(),
        allow_multiple,
        allow_temp_tables,
        bracket_identifiers: false,
        dialect: Some("duckdb".into()),
        ..ReadonlyOptions::default()
    }
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let has = |t: &str| tokens.iter().any(|x| x == t);
    if tokens
        .iter()
        .any(|t| matches!(t.as_str(), "-help" | "--help" | "-version"))
    {
        return Classification::allow_desc("duckdb help/version");
    }
    if has("-init") {
        return Classification::ask_desc("duckdb (init script)");
    }

    // duckdb [OPTIONS] [FILENAME [SQL...]], plus -c, -s, -cmd options
    let mut sql_indices: Vec<usize> = Vec::new();
    let mut i = 1;
    let mut filename: Option<&str> = None;
    let mut filename_has_expansions = false;
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
        if (token == "-c" || token == "-s") && i + 1 < tokens.len() {
            sql_indices.push(i + 1);
            i += 2;
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        if filename.is_none() {
            filename = Some(token);
            filename_has_expansions = ctx.word_has_expansions.get(i) == Some(&true);
            i += 1;
            continue;
        }
        sql_indices.push(i);
        i += 1;
    }

    if sql_indices.is_empty() {
        return Classification::ask_desc("duckdb (interactive)");
    }
    if ctx.raw_words.len() != tokens.len()
        || sql_indices
            .iter()
            .any(|&i| ctx.word_has_expansions.get(i) == Some(&true))
    {
        return Classification::ask_desc("duckdb (ambiguous shell quoting)");
    }
    let parts: Option<Vec<String>> = sql_indices
        .iter()
        .map(|&i| decode_literal_word(&ctx.raw_words[i], false))
        .collect();
    let Some(parts) = parts else {
        return Classification::ask_desc("duckdb (ambiguous shell quoting)");
    };
    if parts.iter().any(|p| py_lstrip(p).starts_with('.')) {
        return Classification::ask_desc("duckdb (dot-command)");
    }
    // Separate SQL arguments conservatively. A space can hide a later write
    // behind an initial SELECT during statement classification.
    let sql = parts.join(";\n");
    let readonly_flag = has("-readonly") || has("-safe");
    let single = options(false, readonly_flag);
    let mut export_targets: Vec<String> = Vec::new();
    let mut unverified_statement = false;
    for statement in split_sql_statements(&sql, false, false).unwrap_or_default() {
        if is_readonly_sql(&statement, &single) == Some(true) {
            continue;
        }
        match duckdb_copy_export_target(&statement) {
            Some(target) => export_targets.push(target),
            None => unverified_statement = true,
        }
    }
    if !export_targets.is_empty() {
        if unverified_statement {
            return Classification::ask_desc("duckdb (unverified export batch)");
        }
        return Classification::allow_desc("duckdb (query export)").redirects(export_targets);
    }
    let readonly = is_readonly_sql(&sql, &options(true, readonly_flag));
    if readonly_flag {
        if readonly == Some(true) {
            return Classification::allow_desc("duckdb (read-only query)");
        }
        return Classification::ask_desc("duckdb (unverified read-only query)");
    }
    if readonly == Some(true) {
        return Classification::allow_desc("duckdb (read-only query)");
    }
    if let Some(filename) = filename
        && !filename.is_empty()
        && !filename_has_expansions
        && duckdb_writes_only_main(&sql)
    {
        return Classification::allow_desc("duckdb (database write)")
            .redirects(vec![filename.to_string()]);
    }
    if readonly == Some(false) {
        return Classification::ask_desc("duckdb (write query)");
    }
    Classification::ask_desc("duckdb (unknown query)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;
    use crate::sql::test_ctx;

    const CONCURRENCY_SQL: &str = "WITH b AS (SELECT c_ip FROM access_mp2
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
ORDER BY max_conc DESC, reqs DESC LIMIT 15";

    const POOL_EXPORT_SQL: &str = "COPY (SELECT c_ip, count(DISTINCT rid) n,
count(DISTINCT cs_user_agent) uas FROM access_mp2
WHERE substr(time_local,4,8)='Sep/2026' AND substr(time_local,1,2) BETWEEN '16' AND '30'
AND cs_uri LIKE '/_/v1/elasticsearch%' GROUP BY c_ip
HAVING count(DISTINCT cs_user_agent) >= 10 ORDER BY n DESC)
TO 'tmp/wdt_traffic-tos-pool-ips.txt' (HEADER false, DELIMITER ' ')";

    const CASES: &[(&str, bool)] = &[
        ("duckdb --help", true),
        ("duckdb -help", true),
        ("duckdb -version", true),
        ("duckdb -readonly mydb.db", false),
        ("duckdb -readonly mydb.db 'DROP TABLE users'", false),
        ("duckdb -safe mydb.db", false),
        ("duckdb -safe mydb.db 'DROP TABLE users'", false),
        ("duckdb -readonly mydb.db 'SELECT 1'", true),
        ("duckdb -safe mydb.db 'SELECT 1'", true),
        ("duckdb mydb.db 'SELECT * FROM users'", true),
        ("duckdb mydb.db 'SELECT * FROM users;'", true),
        ("duckdb mydb.db 'EXPLAIN SELECT * FROM users'", true),
        ("duckdb :memory: 'SELECT 1'", true),
        ("duckdb -c 'SELECT 1'", true),
        ("duckdb -s 'SELECT 1'", true),
        ("duckdb mydb.db -c 'SELECT * FROM users'", true),
        ("duckdb -json mydb.db 'SELECT * FROM users'", true),
        ("duckdb -csv mydb.db 'SELECT * FROM users'", true),
        ("duckdb -table mydb.db 'SELECT * FROM users'", true),
        ("duckdb -line mydb.db 'SELECT * FROM users'", true),
        ("duckdb -column mydb.db 'SELECT * FROM users'", true),
        ("duckdb -cmd 'SELECT 1' mydb.db", true),
        ("duckdb -c 'DROP TABLE users'", false),
        ("duckdb -s 'INSERT INTO t VALUES (1)'", false),
        ("duckdb mydb.db 'PRAGMA enable_progress_bar'", false),
        ("duckdb mydb.db 'ATTACH DATABASE other.db'", false),
        ("duckdb mydb.db 'DETACH DATABASE other'", false),
        ("duckdb mydb.db 'VACUUM'", false),
        ("duckdb mydb.db 'COPY users TO /tmp/out.csv'", false),
        ("duckdb mydb.db 'EXPORT DATABASE /tmp/backup'", false),
        ("duckdb mydb.db 'IMPORT DATABASE /tmp/backup'", false),
        ("duckdb", false),
        ("duckdb mydb.db", false),
        ("duckdb -json mydb.db", false),
        ("duckdb mydb.db 'SELECT 1; SELECT 2'", true),
        ("duckdb -readonly -csv mydb.db 'SELECT 1; SELECT 2;'", true),
        (
            "duckdb -readonly mydb.db 'SELECT 1; -- comment\nSELECT 2'",
            true,
        ),
        (
            "duckdb -readonly mydb.db \"SELECT ';' AS delimiter; SELECT 2\"",
            true,
        ),
        (
            "duckdb -readonly mydb.db 'SELECT 1; INSERT INTO items VALUES (2)'",
            false,
        ),
        ("duckdb mydb.db 'SELECT 1; ; SELECT 2'", false),
        ("duckdb -init script.sql mydb.db", false),
        ("duckdb -f script.sql", false),
        ("duckdb -separator '|' mydb.db 'SELECT 1'", true),
        ("duckdb -newline '\\n' mydb.db 'SELECT 1'", true),
        ("duckdb -nullvalue NULL mydb.db 'SELECT 1'", true),
        // External side effects are never approved by -readonly/-safe.
        (
            "duckdb -readonly work.db \"COPY (SELECT 1) TO '/tmp/output.csv'\"",
            false,
        ),
        (
            "duckdb -readonly work.db \"EXPORT DATABASE '/tmp/backup'\"",
            false,
        ),
        ("duckdb -safe work.db \"INSTALL httpfs\"", false),
        ("duckdb -readonly work.db \"LOAD httpfs\"", false),
        (
            "duckdb -readonly work.db \"SELECT 1; EXPORT DATABASE '/tmp/backup'\"",
            false,
        ),
        (
            "duckdb -readonly work.db \"EXPLAIN ANALYZE COPY (SELECT 1) TO '/tmp/output.csv'\"",
            false,
        ),
        (
            "duckdb -readonly work.db \"EXPLAIN ANALYZE INSERT INTO items VALUES (1)\"",
            false,
        ),
        (
            "duckdb -readonly work.db \"ATTACH 'other.db' AS other\"",
            false,
        ),
        (
            "duckdb -cmd 'SELECT 1' work.db \"EXPORT DATABASE '/tmp/backup'\"",
            false,
        ),
        (
            "duckdb -readonly -cmd 'SELECT 1' work.db 'DROP TABLE items'",
            false,
        ),
        ("duckdb work.db 'SELECT 1' 'DROP TABLE items'", false),
        (
            "duckdb -readonly work.db \"SELECT 1; -- a comment\nCOPY (SELECT 1) TO '/tmp/output.csv'\"",
            false,
        ),
        (
            "duckdb -readonly work.db \"SELECT [']']; INSTALL httpfs; --'\"",
            false,
        ),
        (
            "duckdb -readonly work.db \"SELECT [']']; COPY (SELECT 1) TO '/tmp/output.csv'; --'\"",
            false,
        ),
        ("duckdb -readonly work.db \"SELECT [']']\" '.tables'", false),
        (
            "duckdb -readonly work.db \"SELECT E'\\\\' x '; COPY (SELECT 1) TO '/tmp/output.csv' --'\"",
            false,
        ),
        (
            "duckdb -readonly work.db \"SELECT $$'$$; COPY (SELECT 1) TO '/tmp/output.csv' --'\"",
            false,
        ),
        (
            "duckdb -readonly work.db \"SELECT 1 -- comment\rINSTALL httpfs\"",
            false,
        ),
        // Side-effect words as data.
        (
            "duckdb -readonly work.db \"SELECT * FROM access WHERE uri LIKE '%export%'\"",
            true,
        ),
        (
            "duckdb -readonly work.db \"SELECT 'COPY; EXPORT; INSTALL; LOAD'\"",
            true,
        ),
        (
            "duckdb -readonly work.db \"/* EXPORT DATABASE '/tmp/no' */ SELECT 1\"",
            true,
        ),
        (
            "duckdb -readonly work.db \"WITH x AS (SELECT 'INSTALL') SELECT * FROM x\"",
            true,
        ),
        ("duckdb -readonly work.db 'SELECT [1, 2]'", true),
        ("duckdb -readonly work.db 'SELECT [\"export\"]'", true),
        ("duckdb -readonly work.db \"SELECT \"'1'\"\"", true),
        (
            "duckdb -readonly data.db \"SELECT 1; \\INSTALL httpfs\"",
            false,
        ),
        // Temp-table batches.
        (
            "duckdb -readonly -csv data.db \"CREATE TABLE n AS SELECT 1; SELECT * FROM n\"",
            false,
        ),
        (
            "duckdb -readonly -csv data.db \"CREATE TEMP TABLE main.n AS SELECT 1; SELECT * FROM n\"",
            false,
        ),
        (
            "duckdb -readonly -csv data.db \"CREATE TEMP TABLE n (id INT); SELECT * FROM n\"",
            false,
        ),
        (
            "duckdb -readonly -csv data.db \"CREATE TEMP TABLE n AS DELETE FROM data; SELECT * FROM n\"",
            false,
        ),
        (
            "duckdb -readonly -csv data.db \"CREATE TEMP TABLE n AS SELECT 1; COPY (SELECT 1) TO '/tmp/out.csv'\"",
            false,
        ),
        (
            "duckdb -readonly -csv data.db \"CREATE TEMP TABLE n AS SELECT 1; INSTALL httpfs\"",
            false,
        ),
        // Exports and database-file writes are approved only through
        // redirect rules: the handler reports them as redirect targets.
        (
            "duckdb data/work.db \"ATTACH 'other.db' AS other; CREATE TABLE other.results AS SELECT 1;\"",
            false,
        ),
        (
            "duckdb tmp/work.db \"COPY (SELECT 1) TO '/etc/dippy-bypass'\"",
            false,
        ),
        (
            "duckdb tmp/work.db 'DROP SECRET production_credentials'",
            false,
        ),
        (
            "duckdb tmp/work.db 'CREATE TABLE other_db.t AS SELECT 1'",
            false,
        ),
        (
            "duckdb tmp/work.db 'INSERT INTO other_db.t SELECT 1'",
            false,
        ),
        (
            "duckdb tmp/work.db \"CREATE TABLE t AS SELECT query('COPY (SELECT 1) TO /tmp/out')\"",
            false,
        ),
        (
            "duckdb tmp/work.db \"CREATE TABLE t AS SELECT writefile('/tmp/out','x')\"",
            false,
        ),
    ];

    #[test]
    fn classifies_commands() {
        for (command, allowed) in CASES {
            let result = classify(&test_ctx(command));
            let approved = result.action == Action::Allow && result.redirect_targets.is_none();
            assert_eq!(approved, *allowed, "{command}");
        }
    }

    fn redirects(command: &str) -> Option<Vec<String>> {
        let result = classify(&test_ctx(command));
        assert_eq!(result.action, Action::Allow, "{command}");
        result.redirect_targets
    }

    #[test]
    fn export_targets_need_redirect_approval() {
        for flags in ["", "-readonly", "-safe"] {
            let command = format!(
                "duckdb {flags} -noheader -separator \" \" -list data.db \"{POOL_EXPORT_SQL}\""
            );
            assert_eq!(
                redirects(&command),
                Some(vec!["tmp/wdt_traffic-tos-pool-ips.txt".to_string()])
            );
        }
        let command = format!(
            "duckdb -readonly data.db \"{POOL_EXPORT_SQL}; COPY (SELECT 2) TO 'elsewhere/second.csv'\""
        );
        assert_eq!(
            redirects(&command),
            Some(vec![
                "tmp/wdt_traffic-tos-pool-ips.txt".to_string(),
                "elsewhere/second.csv".to_string()
            ])
        );
        let command = format!("duckdb -readonly data.db \"{POOL_EXPORT_SQL}; SELECT 2\"");
        assert_eq!(
            redirects(&command),
            Some(vec!["tmp/wdt_traffic-tos-pool-ips.txt".to_string()])
        );
        let command = "duckdb -readonly -cmd \"COPY (SELECT 1) TO 'tmp/one'\" data.db \"COPY (SELECT 2) TO 'elsewhere/two'\"";
        assert_eq!(
            redirects(command),
            Some(vec!["tmp/one".to_string(), "elsewhere/two".to_string()])
        );
    }

    #[test]
    fn export_cannot_hide_other_effects() {
        for flags in ["", "-readonly", "-safe"] {
            for extra in [
                "DROP TABLE t",
                "INSTALL httpfs",
                "SELECT writefile('x','y')",
            ] {
                for sql in [
                    format!("{extra}; {POOL_EXPORT_SQL}"),
                    format!("{POOL_EXPORT_SQL}; {extra}"),
                ] {
                    let command = format!("duckdb {flags} tmp/data.db \"{sql}\"");
                    let result = classify(&test_ctx(&command));
                    assert_eq!(result.action, Action::Ask, "{command}");
                }
            }
        }
    }

    #[test]
    fn unsupported_exports_ask() {
        for sql in [
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
        ] {
            let command = format!("duckdb -readonly data.db \"{sql}\"");
            assert_eq!(classify(&test_ctx(&command)).action, Action::Ask, "{sql}");
        }
    }

    #[test]
    fn reported_concurrency_query() {
        for (suffix, approved) in [
            ("", true),
            ("; COPY (SELECT 1) TO 'tmp/output'", false),
            ("; INSTALL httpfs", false),
            ("; DELETE FROM access_mp2", false),
        ] {
            let sql = format!("{CONCURRENCY_SQL}{suffix}").replace('$', "\\$");
            let command = format!("duckdb -readonly -csv server-logs/gate2/gate2.db \"{sql}\"");
            let result = classify(&test_ctx(&command));
            if approved {
                assert_eq!(result.action, Action::Allow, "{suffix}");
                assert_eq!(result.redirect_targets, None);
            } else {
                assert!(
                    result.action == Action::Ask || result.redirect_targets.is_some(),
                    "{suffix}"
                );
            }
        }
    }

    #[test]
    fn verified_main_writes_target_the_database() {
        for sql in [
            "CREATE TABLE results AS SELECT 1 AS value",
            "CREATE TABLE main.t AS SELECT 1",
            "INSERT INTO t SELECT 1",
            "UPDATE t SET x = 1",
        ] {
            let command = format!("duckdb tmp/work.db \"{sql}\"");
            let result = classify(&test_ctx(&command));
            if result.action == Action::Allow {
                assert_eq!(result.redirect_targets, Some(vec!["tmp/work.db".into()]));
            }
        }
        let command = "duckdb tmp/work.db \"ATTACH 'source.db' AS source (READ_ONLY); CREATE OR REPLACE TABLE results AS SELECT * FROM source.items; SELECT count(*) FROM results;\"";
        let result = classify(&test_ctx(command));
        if result.action == Action::Allow {
            assert_eq!(result.redirect_targets, Some(vec!["tmp/work.db".into()]));
        }
        let result = classify(&test_ctx(
            "duckdb tmp/$DATABASE 'CREATE TABLE results AS SELECT 1'",
        ));
        assert_eq!(result.action, Action::Ask);
    }

    #[test]
    fn shell_expansion_asks() {
        for expansion in [
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
        ] {
            let command = format!("duckdb -readonly data.db \"SELECT '{expansion}'\"");
            assert_eq!(
                classify(&test_ctx(&command)).action,
                Action::Ask,
                "{expansion}"
            );
        }
    }

    #[test]
    fn descriptions() {
        let desc = |c: &str| classify(&test_ctx(c)).description.unwrap();
        assert_eq!(desc("duckdb db 'SELECT 1'"), "duckdb (read-only query)");
        assert_eq!(desc("duckdb db 'DROP SECRET x'"), "duckdb (write query)");
        assert_eq!(desc("duckdb db '.tables'"), "duckdb (dot-command)");
        assert_eq!(desc("duckdb db"), "duckdb (interactive)");
        assert_eq!(desc("duckdb -init x db"), "duckdb (init script)");
        assert_eq!(
            desc("duckdb -readonly db 'DROP TABLE t'"),
            "duckdb (unverified read-only query)"
        );
        assert_eq!(
            desc("duckdb db \"SELECT '$HOME'\""),
            "duckdb (ambiguous shell quoting)"
        );
    }
}
