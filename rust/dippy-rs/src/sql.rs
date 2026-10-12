//! Port of `dippy.core.sql`: read-only vs write SQL classification.
//!
//! The lexical layer (masking of literals and comments, statement splitting,
//! keyword checks, CTE skipping, `SELECT ... INTO`, temp-table and EXPLAIN
//! prefixes) is an exact port of the Python code.
//!
//! Python verifies query structure with SQLGlot (`_verify_query_ast`,
//! `_verified_main_write`, the COPY export check and the temp-table parse).
//! Rust parses with the `sqlparser` crate and checks the AST against an
//! allowlist instead ([`checker`]). Intended divergences are documented
//! there.

mod checker;
mod functions;

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::bash::decode_literal_word;
use crate::cli::HandlerContext;

/// Keyword arguments of Python `is_readonly_sql`.
#[derive(Debug, Clone)]
pub struct ReadonlyOptions {
    pub extra_readonly: Vec<String>,
    pub extra_write: Vec<String>,
    pub allow_multiple: bool,
    pub allow_temp_tables: bool,
    /// Python default is `true` (also the `Default` here).
    pub bracket_identifiers: bool,
    pub dialect: Option<String>,
}

impl Default for ReadonlyOptions {
    fn default() -> Self {
        Self {
            extra_readonly: Vec::new(),
            extra_write: Vec::new(),
            allow_multiple: false,
            allow_temp_tables: false,
            bracket_identifiers: true,
            dialect: None,
        }
    }
}

const READONLY_KEYWORDS: [&str; 4] = ["SELECT", "SHOW", "DESCRIBE", "EXPLAIN"];
const WRITE_KEYWORDS: [&str; 11] = [
    "INSERT", "CREATE", "ALTER", "DROP", "TRUNCATE", "DELETE", "UPDATE", "MERGE", "GRANT",
    "REVOKE", "REPLACE",
];
const UNSAFE_TOKENS: [&str; 28] = [
    "ALTER", "ATTACH", "CALL", "COPY", "CREATE", "DELETE", "DETACH", "DROP", "DUMPFILE", "EXECUTE",
    "EXPORT", "GRANT", "IMPORT", "INSERT", "INSTALL", "INTO", "KILL", "LOAD", "MERGE", "OUTFILE",
    "PRAGMA", "REINDEX", "REPLACE", "REVOKE", "TRUNCATE", "UNLOAD", "UPDATE", "VACUUM",
];

/// `_MAIN_WRITE_PREFIXES`: root keyword -> tokens that root may contain.
fn main_write_allowed(root: &str) -> Option<&'static [&'static str]> {
    Some(match root {
        "CREATE" => &["CREATE", "REPLACE"],
        "INSERT" => &["INSERT", "INTO"],
        "UPDATE" => &["UPDATE"],
        "DELETE" => &["DELETE"],
        "DROP" => &["DROP"],
        "ALTER" => &["ALTER"],
        "TRUNCATE" => &["TRUNCATE"],
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Python string helpers (code-point based, like Python `str`).

/// `str.isspace()` / `re` `\s` for `str` patterns.
pub(crate) fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `re` `\w` for `str` patterns.
fn py_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn lstrip_len(s: &[char]) -> usize {
    s.iter().take_while(|c| py_isspace(**c)).count()
}

fn is_blank(s: &[char]) -> bool {
    s.iter().all(|c| py_isspace(*c))
}

fn starts_with(s: &[char], i: usize, pat: &str) -> bool {
    let pat: Vec<char> = pat.chars().collect();
    s.get(i..).is_some_and(|rest| rest.starts_with(&pat))
}

fn find_seq(s: &[char], pat: &[char], from: usize) -> Option<usize> {
    if pat.is_empty() {
        return Some(from.min(s.len()));
    }
    (from..s.len()).find(|&i| s[i..].starts_with(pat))
}

fn to_string(s: &[char]) -> String {
    s.iter().collect()
}

/// `_KEYWORD_PATTERN.match(s, pos)`: end of `[A-Za-z_]\w*` at `pos`.
fn keyword_at(s: &[char], pos: usize) -> Option<usize> {
    let first = *s.get(pos)?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut end = pos + 1;
    while end < s.len() && py_word(s[end]) {
        end += 1;
    }
    Some(end)
}

fn upper(s: &[char]) -> String {
    to_string(s).to_uppercase()
}

/// Case-insensitive ASCII keyword at the start of `s` followed by `\b`.
fn word_prefix(s: &[char], word: &str) -> Option<usize> {
    let n = word.chars().count();
    if s.len() < n
        || !s[..n]
            .iter()
            .zip(word.chars())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
    {
        return None;
    }
    if s.get(n).is_some_and(|c| py_word(*c)) {
        return None;
    }
    Some(n)
}

/// `_skip_whitespace`.
fn skip_whitespace(s: &[char], mut pos: usize) -> usize {
    while pos < s.len() && py_isspace(s[pos]) {
        pos += 1;
    }
    pos
}

// ---------------------------------------------------------------------------
// Masking and splitting.

/// `_mask_sql`: blank quoted data and comments, preserving positions.
fn mask_sql(sql: &[char], bracket_identifiers: bool, reject_exec: bool) -> Option<Vec<char>> {
    if sql.contains(&'\r') {
        return None;
    }
    let n = sql.len();
    let mut masked = sql.to_vec();
    let mut i = 0;
    while i < n {
        let start = i;
        if starts_with(sql, i, "--") && (!reject_exec || i + 2 == n || sql[i + 2] <= ' ') {
            i = (i..n).find(|&k| sql[k] == '\n').unwrap_or(n);
        } else if starts_with(sql, i, "/*") {
            if reject_exec
                && (starts_with(sql, i, "/*!") || (i + 4 <= n && upper(&sql[i..i + 4]) == "/*M!"))
            {
                return None;
            }
            let end = find_seq(sql, &['*', '/'], i + 2)?;
            if find_seq(&sql[start + 2..end], &['/', '*'], 0).is_some() {
                return None;
            }
            i = end + 2;
        } else if matches!(sql[i], '\'' | '"' | '`') || (bracket_identifiers && sql[i] == '[') {
            let quote = if sql[i] == '[' { ']' } else { sql[i] };
            if quote == '\''
                && i > 0
                && matches!(sql[i - 1], 'E' | 'e')
                && (i == 1 || !(sql[i - 2].is_alphanumeric() || sql[i - 2] == '_'))
            {
                return None;
            }
            i += 1;
            loop {
                if i >= n {
                    return None;
                }
                if sql[i] == quote {
                    if i + 1 < n && sql[i + 1] == quote {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else if sql[i] == '$' {
            if i > 0 && (sql[i - 1].is_alphanumeric() || matches!(sql[i - 1], '_' | '$')) {
                return None;
            }
            // \$(?:[A-Za-z_]\w*)?\$
            let mut j = i + 1;
            if j < n && (sql[j].is_ascii_alphabetic() || sql[j] == '_') {
                j += 1;
                while j < n && py_word(sql[j]) {
                    j += 1;
                }
            }
            if j >= n || sql[j] != '$' {
                return None;
            }
            let delimiter = &sql[i..=j];
            let end = find_seq(sql, delimiter, j + 1)?;
            i = end + delimiter.len();
        } else {
            i += 1;
            continue;
        }
        for c in &mut masked[start..i] {
            *c = ' ';
        }
    }
    Some(masked)
}

fn split_chars(
    sql: &[char],
    bracket_identifiers: bool,
    reject_exec: bool,
) -> Option<Vec<Vec<char>>> {
    let masked = mask_sql(sql, bracket_identifiers, reject_exec)?;
    let ends: Vec<usize> = (0..masked.len()).filter(|&i| masked[i] == ';').collect();
    let mut parts = Vec::new();
    let mut masked_parts = Vec::new();
    let mut start = 0;
    for &end in &ends {
        parts.push(&sql[start..end]);
        masked_parts.push(&masked[start..end]);
        start = end + 1;
    }
    parts.push(&sql[start..]);
    masked_parts.push(&masked[start..]);
    let nonempty: Vec<usize> = (0..masked_parts.len())
        .filter(|&i| !is_blank(masked_parts[i]))
        .collect();
    let last = *nonempty.last()?;
    if (0..last).any(|i| is_blank(masked_parts[i])) {
        return None;
    }
    let tail = masked_parts
        .get(last + 1..masked_parts.len() - 1)
        .unwrap_or_default();
    if tail.iter().any(|part| !part.is_empty() && is_blank(part)) {
        return None;
    }
    Some(nonempty.iter().map(|&i| parts[i].to_vec()).collect())
}

/// `split_sql_statements`: split on executable semicolons; `None` when
/// the SQL is ambiguous. Python defaults: `bracket_identifiers=True`,
/// `reject_executable_comments=False`.
pub fn split_sql_statements(
    sql: &str,
    bracket_identifiers: bool,
    reject_executable_comments: bool,
) -> Option<Vec<String>> {
    let chars: Vec<char> = sql.chars().collect();
    let parts = split_chars(&chars, bracket_identifiers, reject_executable_comments)?;
    Some(parts.iter().map(|p| to_string(p)).collect())
}

/// `_semantic_words`: word counts, excluding function names.
fn semantic_words(masked: &[char]) -> HashMap<String, usize> {
    let mut words = HashMap::new();
    let mut i = 0;
    while i < masked.len() {
        let Some(end) = keyword_at(masked, i) else {
            i += 1;
            continue;
        };
        let after = skip_whitespace(masked, end);
        if masked.get(after) != Some(&'(') {
            *words.entry(upper(&masked[i..end])).or_insert(0) += 1;
        }
        i = end;
    }
    words
}

fn has_unsafe(words: &HashMap<String, usize>, allowed: &[&str]) -> bool {
    UNSAFE_TOKENS
        .iter()
        .any(|t| !allowed.contains(t) && words.contains_key(*t))
}

// ---------------------------------------------------------------------------
// Lexical statement checks.

/// `_skip_cte`.
fn skip_cte(sql: &[char], mut pos: usize) -> usize {
    let length = sql.len();
    let skip_parens = |mut pos: usize| {
        let mut depth = 1;
        pos += 1;
        while pos < length && depth > 0 {
            if sql[pos] == '(' {
                depth += 1;
            } else if sql[pos] == ')' {
                depth -= 1;
            }
            pos += 1;
        }
        pos
    };
    while pos < length {
        pos = skip_whitespace(sql, pos);
        if pos >= length {
            break;
        }
        let Some(end) = keyword_at(sql, pos) else {
            break;
        };
        if upper(&sql[pos..end]) == "RECURSIVE" {
            pos = skip_whitespace(sql, end);
            continue;
        }
        pos = skip_whitespace(sql, end);
        if pos < length && sql[pos] == '(' {
            pos = skip_whitespace(sql, skip_parens(pos));
        }
        match keyword_at(sql, pos) {
            Some(as_end) if upper(&sql[pos..as_end]) == "AS" => {
                pos = skip_whitespace(sql, as_end);
            }
            _ => break,
        }
        if pos < length && sql[pos] == '(' {
            pos = skip_whitespace(sql, skip_parens(pos));
        } else {
            break;
        }
        if pos < length && sql[pos] == ',' {
            pos += 1;
            continue;
        }
        return pos;
    }
    pos
}

/// `_check_select_into`.
fn check_select_into(sql: &[char], mut pos: usize) -> bool {
    while pos < sql.len() {
        pos = skip_whitespace(sql, pos);
        if pos >= sql.len() {
            break;
        }
        if let Some(end) = keyword_at(sql, pos) {
            match upper(&sql[pos..end]).as_str() {
                "INTO" => return true,
                "FROM" => return false,
                _ => {}
            }
            pos = end;
            continue;
        }
        pos += 1;
    }
    false
}

/// `_TEMP_TABLE_AS_SELECT.match`: end of the `CREATE TEMP TABLE x AS ` prefix.
fn match_temp_table(s: &[char]) -> Option<usize> {
    fn ws1(s: &[char], pos: usize) -> Option<usize> {
        let end = skip_whitespace(s, pos);
        (end > pos).then_some(end)
    }
    fn lit(s: &[char], pos: usize, word: &str) -> Option<usize> {
        let n = word.len();
        let ok = s.len() >= pos + n
            && s[pos..pos + n]
                .iter()
                .zip(word.chars())
                .all(|(a, b)| a.eq_ignore_ascii_case(&b));
        ok.then_some(pos + n)
    }
    let mut pos = ws1(s, lit(s, 0, "CREATE")?)?;
    if let Some(p) = lit(s, pos, "OR")
        .and_then(|p| ws1(s, p))
        .and_then(|p| lit(s, p, "REPLACE"))
        .and_then(|p| ws1(s, p))
    {
        pos = p;
    }
    pos = lit(s, pos, "TEMP")?;
    if let Some(p) = lit(s, pos, "ORARY") {
        pos = p;
    }
    pos = ws1(s, pos)?;
    pos = ws1(s, lit(s, pos, "TABLE")?)?;
    if let Some(p) = lit(s, pos, "IF")
        .and_then(|p| ws1(s, p))
        .and_then(|p| lit(s, p, "NOT"))
        .and_then(|p| ws1(s, p))
        .and_then(|p| lit(s, p, "EXISTS"))
        .and_then(|p| ws1(s, p))
    {
        pos = p;
    }
    if let Some(p) = lit(s, pos, "temp.") {
        pos = p;
    }
    pos = keyword_at(s, pos)?;
    pos = ws1(s, pos)?;
    pos = lit(s, pos, "AS")?;
    ws1(s, pos)
}

/// `is_readonly_sql`: `Some(true)` read-only, `Some(false)` write, `None`
/// unknown (ask).
pub fn is_readonly_sql(sql: &str, opts: &ReadonlyOptions) -> Option<bool> {
    let dialect = opts.dialect.as_deref();
    let reject_exec = dialect == Some("mysql") || (dialect.is_none() && sql.contains('`'));
    let chars: Vec<char> = sql.chars().collect();
    let statements = split_chars(&chars, opts.bracket_identifiers, reject_exec)?;
    if statements.len() > 1 && !opts.allow_multiple {
        return None;
    }
    if statements.len() > 1 {
        let single = ReadonlyOptions {
            allow_multiple: false,
            ..opts.clone()
        };
        let mut all = true;
        for statement in &statements {
            all &= is_readonly_sql(&to_string(statement), &single)?;
        }
        return Some(all);
    }
    let statement = &statements[0];
    let stripped = mask_sql(statement, opts.bracket_identifiers, reject_exec)?;
    let nested = ReadonlyOptions {
        allow_multiple: false,
        allow_temp_tables: false,
        ..opts.clone()
    };

    if opts.allow_temp_tables {
        let offset = lstrip_len(&stripped);
        if let Some(end) = match_temp_table(&stripped[offset..]) {
            let query = to_string(&statement[offset + end..]);
            let rest = &stripped[offset + end..];
            let query_start = &rest[lstrip_len(rest)..];
            if word_prefix(query_start, "SELECT").is_none()
                && word_prefix(query_start, "WITH").is_none()
            {
                return Some(false);
            }
            // Python parses the whole statement with SQLGlot first.
            let parsed = checker::Dialect::parse(dialect)
                .is_some_and(|d| checker::verify_temp_create(&to_string(statement), d));
            if !parsed {
                return None;
            }
            return is_readonly_sql(&query, &nested);
        }
    }

    let is_readonly_kw =
        |kw: &str| READONLY_KEYWORDS.contains(&kw) || opts.extra_readonly.iter().any(|k| k == kw);
    let is_write_kw =
        |kw: &str| WRITE_KEYWORDS.contains(&kw) || opts.extra_write.iter().any(|k| k == kw);

    if stripped[lstrip_len(&stripped)..].first() == Some(&'(') {
        return verify_query_ast(&to_string(statement), dialect);
    }

    let mut pos = 0;
    while pos < stripped.len() {
        pos = skip_whitespace(&stripped, pos);
        if pos >= stripped.len() {
            break;
        }
        let end = keyword_at(&stripped, pos)?;
        let kw = upper(&stripped[pos..end]);
        if kw == "WITH" {
            pos = skip_cte(&stripped, end);
            continue;
        }
        if (kw == "FROM" || kw == "VALUES") && dialect == Some("duckdb") {
            return verify_query_ast(&to_string(statement), dialect);
        }
        if kw == "SELECT" {
            if check_select_into(&stripped, end) {
                return Some(false);
            }
            return verify_query_ast(&to_string(statement), dialect);
        }
        if kw == "EXPLAIN" {
            let mut rest = &statement[end..];
            rest = &rest[lstrip_len(rest)..];
            if let Some(n) = word_prefix(rest, "ANALYZE") {
                rest = &rest[n..];
                rest = &rest[lstrip_len(rest)..];
            }
            if let Some(n) = word_prefix(rest, "PLAN") {
                let after = skip_whitespace(rest, n);
                if after > n
                    && let Some(m) = word_prefix(&rest[after..], "FOR")
                {
                    rest = &rest[after + m..];
                    rest = &rest[lstrip_len(rest)..];
                }
            }
            return is_readonly_sql(&to_string(rest), &nested);
        }
        if is_readonly_kw(&kw) {
            if (kw == "SHOW" || kw == "DESCRIBE") && has_unsafe(&semantic_words(&stripped), &[]) {
                return Some(false);
            }
            return Some(true);
        }
        if is_write_kw(&kw) {
            return Some(false);
        }
        return None;
    }
    None
}

/// `_verify_query_ast`: require a complete read-only query, not a prefix.
fn verify_query_ast(sql: &str, dialect: Option<&str>) -> Option<bool> {
    let parser_dialect = dialect.or(if sql.contains('`') {
        Some("mysql")
    } else if sql.contains('[') {
        Some("tsql")
    } else {
        None
    });
    let chars: Vec<char> = sql.chars().collect();
    let masked = mask_sql(
        &chars,
        dialect != Some("duckdb"),
        parser_dialect == Some("mysql"),
    )?;
    if has_unsafe(&semantic_words(&masked), &[]) {
        return Some(false);
    }
    let d = checker::Dialect::parse(parser_dialect)?;
    checker::verify_query(sql, d)
}

// ---------------------------------------------------------------------------
// SQLite introspection pragmas (Rust only: Python asks for every PRAGMA).

/// Pragmas that only report, with or without a table/index name argument.
const SQLITE_NAME_PRAGMAS: &[&str] = &[
    "foreign_key_check",
    "foreign_key_list",
    "index_info",
    "index_list",
    "index_xinfo",
    "table_info",
    "table_list",
    "table_xinfo",
];

/// Pragmas that only report in the argument-less form; with an argument
/// some of them set a value.
const SQLITE_QUERY_PRAGMAS: &[&str] = &[
    "application_id",
    "collation_list",
    "compile_options",
    "data_version",
    "database_list",
    "encoding",
    "freelist_count",
    "function_list",
    "integrity_check",
    "module_list",
    "page_count",
    "page_size",
    "pragma_list",
    "quick_check",
    "schema_version",
    "user_version",
];

static SQLITE_PRAGMA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\A\s*PRAGMA\s+(?:[A-Za-z_][A-Za-z0-9_]*\s*\.\s*)?([A-Za-z_]+)\s*(\(\s*(?:[A-Za-z_][A-Za-z0-9_]*|'[A-Za-z0-9_]+'|"[A-Za-z0-9_]+")\s*\))?\s*;?\s*\z"#,
    )
    .unwrap()
});

/// A single read-only introspection `PRAGMA name` or `PRAGMA name(table)`;
/// the `=` form, comments and other statements are not matched.
pub fn sqlite_readonly_pragma(sql: &str) -> bool {
    let Some(caps) = SQLITE_PRAGMA.captures(sql) else {
        return false;
    };
    let name = caps[1].to_ascii_lowercase();
    SQLITE_NAME_PRAGMAS.contains(&name.as_str())
        || (caps.get(2).is_none() && SQLITE_QUERY_PRAGMAS.contains(&name.as_str()))
}

// ---------------------------------------------------------------------------
// DuckDB output checks.

static ATTACH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\AATTACH(?:[\s\x1c-\x1f]+DATABASE)?\b").unwrap());
static READ_ONLY_OPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\([\s\x1c-\x1f]*[^()]*\bREAD_ONLY\b[^()]*\)[\s\x1c-\x1f]*\z").unwrap()
});

/// `duckdb_writes_only_main`: every statement reads, attaches read-only, or
/// mutates only the main database.
pub fn duckdb_writes_only_main(sql: &str) -> bool {
    let Some(statements) = split_sql_statements(sql, false, false) else {
        return false;
    };
    if statements.is_empty() {
        return false;
    }
    let opts = ReadonlyOptions {
        bracket_identifiers: false,
        dialect: Some("duckdb".into()),
        ..ReadonlyOptions::default()
    };
    for statement in &statements {
        if is_readonly_sql(statement, &opts) == Some(true) {
            continue;
        }
        if ATTACH.is_match(statement) {
            if READ_ONLY_OPTION.is_match(statement) {
                continue;
            }
            return false;
        }
        if !verified_main_write(statement) {
            return false;
        }
    }
    true
}

/// A read-only ATTACH of a local database file (Rust only: Python asks).
pub fn duckdb_readonly_attach(statement: &str) -> bool {
    checker::verify_readonly_attach(statement)
}

/// `_verified_main_write`.
fn verified_main_write(statement: &str) -> bool {
    let chars: Vec<char> = statement.chars().collect();
    let Some(masked) = mask_sql(&chars, false, false) else {
        return false;
    };
    let words = semantic_words(&masked);
    let body = &masked[lstrip_len(&masked)..];
    let Some(end) = keyword_at(body, 0) else {
        return false;
    };
    let root = upper(&body[..end]);
    let Some(allowed) = main_write_allowed(&root) else {
        return false;
    };
    if has_unsafe(&words, allowed) {
        return false;
    }
    checker::verify_main_write(statement, &root)
}

/// `duckdb_copy_export_target`: the target of a verified single-file CSV
/// query export (the target still needs approval), else `None`.
pub fn duckdb_copy_export_target(sql: &str) -> Option<String> {
    let statements = split_sql_statements(sql, false, false)?;
    if statements.len() != 1 {
        return None;
    }
    let chars: Vec<char> = statements[0].chars().collect();
    let masked = mask_sql(&chars, false, false)?;
    let mut pos = skip_whitespace(&masked, 0);
    let copy = masked.get(pos..pos + 4).is_some_and(|w| {
        w.iter()
            .zip("COPY".chars())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
    });
    if !copy {
        return None;
    }
    pos = skip_whitespace(&masked, pos + 4);
    if masked.get(pos) != Some(&'(') || has_unsafe(&semantic_words(&masked), &["COPY"]) {
        return None;
    }
    checker::copy_export_target(&statements[0])
}

// ---------------------------------------------------------------------------
// CLI helpers.

/// Port of `dippy.cli.sql_args.literal_sql_arg`: the SQL Bash passes for a
/// literal word (after `prefix`), or `None` if uncertain.
pub fn literal_sql_arg(ctx: &HandlerContext, index: usize, prefix: &str) -> Option<String> {
    if index >= ctx.tokens.len() || ctx.raw_words.len() != ctx.tokens.len() {
        return None;
    }
    if ctx.word_has_expansions.get(index) == Some(&true) {
        return None;
    }
    let raw = ctx.raw_words[index].strip_prefix(prefix)?;
    decode_literal_word(raw, false)
}

/// Python `str.lstrip()`.
pub fn py_lstrip(s: &str) -> &str {
    s.trim_start_matches(py_isspace)
}

/// Handler test context built like the analyzer: raw words from the Bash
/// parser, quote-stripped tokens and a conservative expansion flag.
#[cfg(test)]
pub(crate) fn test_ctx(command: &str) -> HandlerContext<'static> {
    let raw = crate::parser::tokenize(command, true);
    assert!(!raw.is_empty(), "unparsable test command: {command}");
    let tokens: Vec<String> = raw
        .iter()
        .map(|w| crate::parser::strip_quotes(w).to_string())
        .collect();
    let mut ctx = HandlerContext::new(&tokens);
    // Words Bash would expand do not decode as literals.
    ctx.word_has_expansions = raw
        .iter()
        .map(|w| decode_literal_word(w, false).is_none())
        .collect();
    ctx.raw_words = raw;
    ctx
}

/// Port of `tests/core/test_sql.py` and `tests/core/test_sql_types.py`.
///
/// Python `is True` expectations that the `sqlparser` checker does not
/// reproduce are asserted as `!= Some(true)` and listed as divergences.
#[cfg(test)]
mod tests {
    use super::*;

    const DIALECTS: [&str; 6] = ["duckdb", "postgres", "mysql", "sqlite", "tsql", "athena"];

    fn opts(dialect: Option<&str>) -> ReadonlyOptions {
        ReadonlyOptions {
            dialect: dialect.map(String::from),
            ..ReadonlyOptions::default()
        }
    }

    fn ro(sql: &str) -> Option<bool> {
        is_readonly_sql(sql, &ReadonlyOptions::default())
    }

    fn ro_d(sql: &str, dialect: &str) -> Option<bool> {
        is_readonly_sql(sql, &opts(Some(dialect)))
    }

    fn ro_batch(sql: &str, dialect: Option<&str>) -> Option<bool> {
        is_readonly_sql(
            sql,
            &ReadonlyOptions {
                allow_multiple: true,
                ..opts(dialect)
            },
        )
    }

    fn ro_temp(sql: &str) -> Option<bool> {
        is_readonly_sql(
            sql,
            &ReadonlyOptions {
                allow_temp_tables: true,
                ..opts(Some("duckdb"))
            },
        )
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn ro_write(sql: &str, extra: &[&str]) -> Option<bool> {
        is_readonly_sql(
            sql,
            &ReadonlyOptions {
                extra_write: words(extra),
                ..ReadonlyOptions::default()
            },
        )
    }

    #[test]
    fn parenthesized_reads() {
        for dialect in DIALECTS {
            for sql in [
                "(SELECT 1)",
                "((SELECT 1))",
                "(SELECT 1) UNION ALL (SELECT 2)",
                "((SELECT 1) UNION ALL (SELECT 2))",
                "(WITH x AS (SELECT 1 a) SELECT a FROM x)",
            ] {
                assert_eq!(ro_d(sql, dialect), Some(true), "{dialect}: {sql}");
            }
        }
    }

    #[test]
    fn parenthesized_unverified_operations() {
        for dialect in DIALECTS {
            for sql in [
                "(SELECT fictional_effect())",
                "(SELECT 1) UNION ALL (SELECT fictional_effect())",
                "(WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x)",
                "(SELECT 1 INTO outfile)",
                "(SELECT 1); DROP TABLE t",
                "(DELETE FROM t)",
                "(1 + 1)",
                "(SELECT 1) fictional_unknown_clause",
                "(SELECT 1",
            ] {
                assert_ne!(ro_batch(sql, Some(dialect)), Some(true), "{dialect}: {sql}");
            }
        }
    }

    #[test]
    fn duckdb_sampled_union_is_readonly() {
        let sql =
            "(SELECT * FROM t USING SAMPLE 8 ROWS) UNION ALL (SELECT * FROM t USING SAMPLE 5 ROWS)";
        assert_eq!(ro_d(sql, "duckdb"), Some(true));
    }

    #[test]
    fn mysql_executable_comments() {
        let sql = "SELECT 1--1 /*!50000 INTO OUTFILE 'tmp/out' */";
        assert_eq!(split_sql_statements(sql, true, true), None);
        for marker in ["!", "M!", "m!"] {
            for template in [
                "SELECT 1 /*{m}50000 INTO OUTFILE 'tmp/out' */",
                "SELECT 1 /*{m}50000 , fictional_effect() */",
                "WITH x AS (SELECT 1 /*{m}50000 , fictional_effect() */) SELECT * FROM x",
                "EXPLAIN SELECT 1 /*{m}50000 INTO OUTFILE 'tmp/out' */",
                "SHOW TABLES /*{m}50000 INTO OUTFILE 'tmp/out' */",
                "SELECT 1; SELECT 2 /*{m}50000 INTO OUTFILE 'tmp/out' */",
            ] {
                let sql = template.replace("{m}", marker);
                assert_ne!(ro_batch(&sql, Some("mysql")), Some(true), "{sql}");
            }
        }
        assert_ne!(
            ro("SELECT `x` FROM t /*!50000 INTO OUTFILE 'tmp/out' */"),
            Some(true)
        );
        for sql in [
            "SELECT '/*!50000 INTO OUTFILE */'",
            "SELECT '/*M!50000 INTO OUTFILE */'",
            "SELECT 1 /* ordinary INTO OUTFILE 'tmp/out' */",
            "SELECT 1 -- ordinary INTO OUTFILE 'tmp/out'",
        ] {
            assert_eq!(ro_d(sql, "mysql"), Some(true), "{sql}");
        }
    }

    #[test]
    fn sequence_increment_is_not_readonly() {
        // Python returns False (a write); Rust asks without classifying.
        for sql in [
            "SELECT NEXT VALUE FOR dbo.seq",
            "SELECT NEXT VALUE FOR dbo.seq OVER (ORDER BY id) FROM t",
            "SELECT (SELECT NEXT VALUE FOR dbo.seq)",
            "WITH x AS (SELECT NEXT VALUE FOR dbo.seq n) SELECT * FROM x",
            "EXPLAIN SELECT NEXT VALUE FOR dbo.seq",
        ] {
            assert_ne!(ro_d(sql, "tsql"), Some(true), "{sql}");
        }
        assert_eq!(ro_d("SELECT 'NEXT VALUE FOR dbo.seq'", "tsql"), Some(true));
    }

    #[test]
    fn duckdb_pure_builtins() {
        for expression in ["stats(x)", "list_sum([1,2,3])"] {
            let sql = format!("SELECT {expression} FROM t");
            assert_eq!(ro_d(&sql, "duckdb"), Some(true), "{sql}");
            assert!(duckdb_writes_only_main(&format!(
                "CREATE TABLE main.result AS {sql}"
            )));
            assert_eq!(
                ro_temp(&format!("CREATE TEMP TABLE result AS {sql}")),
                Some(true)
            );
            assert_eq!(
                duckdb_copy_export_target(&format!("COPY ({sql}) TO 'tmp/out.csv'")).as_deref(),
                Some("tmp/out.csv")
            );
        }
        for name in ["stats", "list_sum"] {
            for template in [
                "evil.{n}(1)",
                "\"evil\".\"{n}\"(1)",
                "{n}(query('DROP TABLE t'))",
                "{n}(writefile('tmp/out','data'))",
            ] {
                let sql = format!("SELECT {}", template.replace("{n}", name));
                assert_ne!(ro_d(&sql, "duckdb"), Some(true), "{sql}");
                assert!(!duckdb_writes_only_main(&format!(
                    "CREATE TABLE main.result AS {sql}"
                )));
                assert_eq!(
                    duckdb_copy_export_target(&format!("COPY ({sql}) TO 'tmp/out.csv'")),
                    None
                );
            }
            for dialect in [
                None,
                Some("mysql"),
                Some("postgres"),
                Some("sqlite"),
                Some("tsql"),
                Some("athena"),
            ] {
                let sql = format!("SELECT {name}(x) FROM t");
                assert_ne!(is_readonly_sql(&sql, &opts(dialect)), Some(true), "{sql}");
            }
        }
    }

    #[test]
    fn copy_export_targets() {
        for options in [
            "",
            "(HEADER false, DELIMITER ' ')",
            "(FORMAT CSV, HEADER true, NULL 'none', QUOTE '\"', ESCAPE '\"')",
            "(FORMAT 'csv')",
        ] {
            let sql = format!("COPY (SELECT to_milliseconds(1)) TO 'tmp/out.csv' {options}");
            assert_eq!(
                duckdb_copy_export_target(&sql).as_deref(),
                Some("tmp/out.csv"),
                "{sql}"
            );
            assert_ne!(ro_d(&sql, "duckdb"), Some(true));
        }
        let sql = "/* COPY */ COPY (SELECT '; DROP TABLE t' v) TO /* path */ 'tmp/a''b.csv' (HEADER false)";
        assert_eq!(
            duckdb_copy_export_target(sql).as_deref(),
            Some("tmp/a'b.csv")
        );
        for tail in [
            "TO ''",
            "TO '-'",
            "TO '/dev/stdout'",
            "TO 's3://bucket/out'",
            "TO 'tmp/*.csv'",
            "TO 'tmp/out\nfile'",
            "TO /tmp/out",
            "FROM 'tmp/in'",
            "TO 'tmp/out' (FORMAT PARQUET)",
            "TO 'tmp/out' (PARTITION_BY (x))",
            "TO 'tmp/out' (PER_THREAD_OUTPUT true)",
            "TO 'tmp/out' (HEADER true, HEADER false)",
            "TO 'tmp/out' (HEADER query('DELETE FROM t'))",
            "TO 'tmp/out'; INSTALL httpfs",
            "TO 'tmp/out' FOO BAR",
        ] {
            let sql = format!("COPY (SELECT 1) {tail}");
            assert_eq!(duckdb_copy_export_target(&sql), None, "{sql}");
        }
        for inner in [
            "SELECT evil.to_milliseconds(1)",
            "SELECT query('DROP TABLE t')",
            "SELECT 1 FOO BAR",
            "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x",
        ] {
            let sql = format!("COPY ({inner}) TO 'tmp/out'");
            assert_eq!(duckdb_copy_export_target(&sql), None, "{sql}");
        }
    }

    #[test]
    fn duckdb_interval_constructors() {
        for unit in [
            "centuries",
            "days",
            "decades",
            "hours",
            "microseconds",
            "milliseconds",
            "minutes",
            "months",
            "nanoseconds",
            "seconds",
            "weeks",
            "years",
        ] {
            let sql = format!("SELECT to_{unit}(CAST(coalesce(rt, 0)*1000 AS BIGINT)) FROM r");
            assert_eq!(ro_d(&sql, "duckdb"), Some(true), "{sql}");
            assert!(duckdb_writes_only_main(&format!(
                "CREATE TABLE main.t AS {sql}"
            )));
            assert_eq!(
                ro_temp(&format!("CREATE TEMP TABLE t AS {sql}")),
                Some(true)
            );
        }
        for sql in [
            "SELECT evil.to_milliseconds(1)",
            "SELECT \"evil\".\"to_milliseconds\"(1)",
            "SELECT to_milliseconds(query('DELETE FROM t'))",
            "SELECT to_milliseconds(writefile('x','data'))",
            "SELECT to_milliseconds(1), fictional_effect()",
        ] {
            assert_ne!(ro_d(sql, "duckdb"), Some(true), "{sql}");
            assert!(!duckdb_writes_only_main(&format!(
                "CREATE TABLE main.t AS {sql}"
            )));
        }
        for dialect in [
            None,
            Some("sqlite"),
            Some("postgres"),
            Some("mysql"),
            Some("tsql"),
            Some("athena"),
        ] {
            assert_ne!(
                is_readonly_sql("SELECT to_milliseconds(1)", &opts(dialect)),
                Some(true)
            );
        }
    }

    #[test]
    fn dialect_writes_and_unverified_syntax() {
        assert_eq!(
            ro_d("SELECT * FROM t INTO OUTFILE '/tmp/output'", "mysql"),
            Some(false)
        );
        assert_eq!(
            ro_d(
                "WITH changed AS (DELETE FROM t RETURNING *) SELECT * FROM changed",
                "postgres"
            ),
            Some(false)
        );
        // Python: False (unknown function in a parsed query).
        assert_eq!(ro_d("SELECT writefile('x', 'data')", "sqlite"), Some(false));
        assert_eq!(
            ro_d("SELECT CAST(fictional_effect() AS INTEGER)", "athena"),
            Some(false)
        );
        assert_eq!(ro("SELECT fictional(a, b + 1) FROM t"), Some(false));
        assert_eq!(ro("SELECT fictional(a FROM t"), None);
        assert_eq!(ro_d("SELECT 'DROP INTO OUTFILE'", "mysql"), Some(true));
        assert_eq!(
            ro_d("SELECT * FROM t INTO OUTFILE path", "mysql"),
            Some(false)
        );
        assert_eq!(ro_d("SELECT 1 FOO BAR", "mysql"), None);
        assert_ne!(ro_d("SELECT query('DELETE FROM t')", "duckdb"), Some(true));
        assert_eq!(ro_d("SHOW TABLES INTO OUTFILE x", "mysql"), Some(false));
        assert_eq!(ro_d("FROM t", "duckdb"), Some(true));
        assert_eq!(ro_d("VALUES (1), (2)", "duckdb"), Some(true));
    }

    #[test]
    fn temp_tables() {
        let sql = "CREATE TEMP TABLE n AS SELECT 1; SELECT * FROM n";
        assert_eq!(ro_batch(sql, None), Some(false));
        let temp = ReadonlyOptions {
            allow_multiple: true,
            allow_temp_tables: true,
            ..ReadonlyOptions::default()
        };
        assert_eq!(is_readonly_sql(sql, &temp), Some(true));
        let sql = "CREATE TEMP TABLE n AS DELETE FROM data; SELECT * FROM n";
        assert_eq!(is_readonly_sql(sql, &temp), Some(false));
    }

    #[test]
    fn basic_reads_and_writes() {
        for sql in [
            "SELECT * FROM users",
            "SELECT id, name FROM users WHERE age > 30",
            "SELECT * FROM a JOIN b ON a.id = b.a_id",
            "SELECT * FROM (SELECT id FROM users) sub",
            "SHOW TABLES",
            "SHOW COLUMNS FROM users",
            "DESCRIBE users",
            "EXPLAIN SELECT * FROM users",
            "EXPLAIN ANALYZE SELECT * FROM users",
            "EXPLAIN PLAN FOR SELECT * FROM users",
            "WITH cte AS (SELECT id FROM users) SELECT * FROM cte",
            "\n        WITH\n            cte1 AS (SELECT id FROM users),\n            cte2 AS (SELECT id FROM orders)\n        SELECT * FROM cte1 JOIN cte2 ON cte1.id = cte2.id\n        ",
            "WITH cte AS (SELECT (1 + 2) AS val) SELECT * FROM cte",
            "\n        WITH RECURSIVE cte AS (\n            SELECT 1 AS n\n            UNION ALL\n            SELECT n + 1 FROM cte WHERE n < 10\n        )\n        SELECT * FROM cte\n        ",
            "-- this is a comment\nSELECT * FROM users",
            "SELECT * FROM users -- get all users",
            "/* comment */ SELECT * FROM users",
            "SELECT /* columns */ * FROM users",
            "\n        /*\n         * Multi-line comment\n         */\n        SELECT * FROM users\n        ",
            "-- DELETE everything\nSELECT * FROM users",
            "/* INSERT INTO users */ SELECT * FROM users",
            "-- comment 1\n/* comment 2 */ SELECT * FROM users",
            "   SELECT * FROM users",
            "\n\n\nSELECT * FROM users",
            "\t\tSELECT * FROM users",
            "  \n\t  SELECT * FROM users",
            "  \n-- comment\n  \nSELECT * FROM users",
            "SELECT * FROM users;",
            "SELECT * FROM users;  \n",
            "SELECT * FROM users;;;",
            "SELECT 1; ",
            "select * from users",
            "SELECT * FROM USERS",
            "SeLeCt * FrOm users",
            "SELECT",
            "SELECT 1",
            "SELECT 1 + 2 * 3",
            "SELECT * FROM logs WHERE action = 'DELETE'",
            "SELECT * FROM logs WHERE action = \"DELETE\"",
            "SELECT 'foo;bar' AS val",
            "SELECT 'it''s a test' AS val",
            "SELECT 'INSERT', 'UPDATE', 'DELETE' AS keywords",
            "SELECT `DELETE` FROM users",
            "SELECT [DELETE] FROM users",
            "SELECT \"DELETE\" FROM users",
            "SELECT * FROM `DROP`",
            "SELECT * FROM [INSERT]",
        ] {
            assert_eq!(ro(sql), Some(true), "{sql:?}");
        }
        let long = format!(
            "SELECT {} FROM t",
            (0..1000)
                .map(|i| format!("col{i}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        assert_eq!(ro(&long), Some(true));
        for sql in [
            "INSERT INTO users (name) VALUES ('alice')",
            "INSERT INTO users SELECT * FROM other",
            "UPDATE users SET name = 'bob' WHERE id = 1",
            "DELETE FROM users WHERE id = 1",
            "CREATE TABLE users (id INT)",
            "CREATE INDEX idx ON users(name)",
            "DROP TABLE users",
            "DROP INDEX idx",
            "ALTER TABLE users ADD COLUMN age INT",
            "TRUNCATE TABLE users",
            "GRANT SELECT ON users TO alice",
            "REVOKE SELECT ON users FROM alice",
            "MERGE INTO t USING s ON t.id = s.id",
            "WITH cte AS (SELECT id FROM users) INSERT INTO other SELECT * FROM cte",
            "WITH cte AS (SELECT id FROM users) DELETE FROM users WHERE id IN (SELECT id FROM cte)",
            "\n        WITH\n            cte1 AS (SELECT id FROM users),\n            cte2 AS (SELECT id FROM orders)\n        INSERT INTO results SELECT * FROM cte1\n        ",
            "insert into users values (1)",
            "InSeRt INTO users VALUES (1)",
            "EXPLAIN INSERT INTO t VALUES (1)",
            "EXPLAIN DELETE FROM users",
            "EXPLAIN UPDATE users SET x = 1",
            "EXPLAIN ANALYZE INSERT INTO t VALUES (1)",
            "EXPLAIN ANALYZE\nINSERT INTO t VALUES (1)",
            "SELECT * INTO new_table FROM users",
            "SELECT id, name INTO backup_users FROM users WHERE active = 1",
            "SELECT * INTO #temp_table FROM users",
            "SELECT a.*, b.name INTO results FROM a JOIN b ON a.id = b.a_id",
            "INSERT OR REPLACE INTO users (id, name) VALUES (1, 'alice')",
            "INSERT OR IGNORE INTO users (id, name) VALUES (1, 'alice')",
            "REPLACE INTO users (id, name) VALUES (1, 'alice')",
            "INSERT INTO users (id) VALUES (1) ON CONFLICT DO NOTHING",
            "INSERT INTO users (id, name) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET name = 'b'",
            "INSERT INTO users (id, name) VALUES (1, 'a') ON DUPLICATE KEY UPDATE name = 'b'",
        ] {
            assert_eq!(ro(sql), Some(false), "{sql:?}");
        }
        for sql in [
            "SELECT 1; SELECT 2",
            "SELECT * FROM users; DELETE FROM users",
            "SELECT 1;   ;  ",
            "FOOBAR something",
            "CALL stored_procedure()",
            "SET search_path TO myschema",
            "",
            "   \n\t  ",
            "-- just a comment",
            "/* just a comment */",
        ] {
            assert_eq!(ro(sql), None, "{sql:?}");
        }
    }

    #[test]
    fn batches() {
        assert_eq!(ro_batch("SELECT 1; SELECT 2", None), Some(true));
        assert_eq!(ro_batch("SELECT 1; DELETE FROM users", None), Some(false));
        assert_eq!(ro_batch("SELECT ';'; SELECT 2", None), Some(true));
        assert_eq!(ro_batch("SELECT 1; -- done", None), Some(true));
        let no_brackets = ReadonlyOptions {
            allow_multiple: true,
            bracket_identifiers: false,
            ..ReadonlyOptions::default()
        };
        assert_eq!(
            is_readonly_sql("SELECT [']']; INSTALL httpfs; --'", &no_brackets),
            None
        );
    }

    #[test]
    fn extra_keywords() {
        let sql = "FETCH NEXT FROM cursor";
        assert_eq!(ro(sql), None);
        let readonly = ReadonlyOptions {
            extra_readonly: words(&["FETCH"]),
            ..ReadonlyOptions::default()
        };
        assert_eq!(is_readonly_sql(sql, &readonly), Some(true));
        for (sql, keyword) in [
            ("PRAGMA table_info(users)", "PRAGMA"),
            ("VACUUM", "VACUUM"),
            ("ATTACH DATABASE 'other.db' AS other", "ATTACH"),
            ("DETACH DATABASE other", "DETACH"),
            ("MSCK REPAIR TABLE my_table", "MSCK"),
            ("UNLOAD (SELECT * FROM t) TO 's3://bucket/'", "UNLOAD"),
        ] {
            assert_eq!(ro(sql), None, "{sql}");
            assert_eq!(ro_write(sql, &[keyword]), Some(false), "{sql}");
        }
        let both = ReadonlyOptions {
            extra_readonly: words(&["FETCH"]),
            extra_write: words(&["VACUUM"]),
            ..ReadonlyOptions::default()
        };
        assert_eq!(is_readonly_sql("VACUUM", &both), Some(false));

        let athena = ["MSCK", "UNLOAD", "VACUUM"];
        for sql in [
            "MSCK REPAIR TABLE my_table",
            "UNLOAD (SELECT * FROM t) TO 's3://bucket/path'",
            "VACUUM my_iceberg_table",
        ] {
            assert_eq!(ro_write(sql, &athena), Some(false), "{sql}");
        }
        for sql in [
            "SELECT * FROM my_table",
            "SHOW TABLES IN my_database",
            "DESCRIBE my_table",
        ] {
            assert_eq!(ro_write(sql, &athena), Some(true), "{sql}");
        }
        let sqlite = ["PRAGMA", "ATTACH", "DETACH", "VACUUM", "REINDEX", "ANALYZE"];
        for sql in [
            "PRAGMA table_info(users)",
            "PRAGMA foreign_keys = ON",
            "ATTACH DATABASE 'other.db' AS other",
            "DETACH DATABASE other",
            "VACUUM",
            "VACUUM INTO 'backup.db'",
            "REINDEX",
            "ANALYZE",
        ] {
            assert_eq!(ro_write(sql, &sqlite), Some(false), "{sql}");
        }
        assert_eq!(ro_write("SELECT * FROM users", &sqlite), Some(true));
    }

    #[test]
    fn inet_operators_and_cte_aliases() {
        for dialect in ["duckdb", "postgres"] {
            for op in ["<<=", ">>="] {
                let sql = format!("SELECT '192.168.1.5'::INET {op} '192.168.1.0/24'::INET");
                assert_eq!(ro_d(&sql, dialect), Some(true), "{dialect}: {sql}");
            }
        }
        let sql = "WITH ips AS (SELECT DISTINCT c_ip FROM access WHERE substr(time_local,1,11) IN \
                   ('04/Oct/2026','05/Oct/2026') AND (cs_user_agent LIKE '%WP-Safe-Scanner%')) \
                   SELECT count(*) ips_celkem, count(il.label) ips_v_ip_labels \
                   FROM ips LEFT JOIN ip_labels il ON TRY_CAST(ips.c_ip AS INET) <<= il.cidr::INET";
        assert_eq!(ro_d(sql, "duckdb"), Some(true));
        for sql in [
            "WITH n(net) AS (VALUES ('147.45.142')) SELECT * FROM n",
            "WITH u AS (SELECT 1 a), nase(ip, kdo) AS (VALUES ('145.239.12.84', 'gate2')) SELECT u.a, n.kdo FROM u LEFT JOIN nase n ON u.a = 1",
            "WITH RECURSIVE t(n) AS (VALUES (1)) SELECT * FROM t",
        ] {
            assert_eq!(ro_d(sql, "duckdb"), Some(true), "{sql}");
        }
        assert_eq!(
            ro_d(
                "WITH n(net) AS (VALUES ('147.45.142')) DELETE FROM n",
                "duckdb"
            ),
            Some(false)
        );
        for func in [
            "mode", "entropy", "kurtosis", "skewness", "mad", "bit_xor", "product",
        ] {
            let sql = format!("SELECT {func}(host) FROM t GROUP BY ua");
            assert_eq!(ro_d(&sql, "duckdb"), Some(true), "{sql}");
        }
    }

    // -- tests/core/test_sql_types.py ----------------------------------------

    fn ro_duckdb(sql: &str) -> Option<bool> {
        is_readonly_sql(
            sql,
            &ReadonlyOptions {
                bracket_identifiers: false,
                ..opts(Some("duckdb"))
            },
        )
    }

    #[test]
    fn reported_duckdb_traffic_query() {
        let sql = include_str!("../../../tests/fixtures/sql/duckdb_traffic.sql");
        assert_eq!(ro_duckdb(sql), Some(true));
    }

    /// Queries from the `wdt_traffic` audit log that Python verifies:
    /// read-only statements and the main-database write batches.
    #[test]
    fn reported_duckdb_traffic_queries_2026_10() {
        let options = ReadonlyOptions {
            extra_write: words(&[
                "ATTACH", "COPY", "DETACH", "EXPORT", "IMPORT", "INSTALL", "LOAD", "PRAGMA",
                "VACUUM",
            ]),
            allow_temp_tables: true,
            bracket_identifiers: false,
            ..opts(Some("duckdb"))
        };
        let fixture = include_str!("../../../tests/fixtures/sql/duckdb_traffic_2026_10.jsonl");
        let mut failed = Vec::new();
        for line in fixture.lines() {
            let item: serde_json::Value = serde_json::from_str(line).unwrap();
            let sql = item["sql"].as_str().unwrap();
            let ok = match item["fn"].as_str().unwrap() {
                "is_readonly_sql" => is_readonly_sql(sql, &options) == Some(true),
                _ => duckdb_writes_only_main(sql),
            };
            if !ok {
                failed.push(sql.to_string());
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n\n"));
    }

    #[test]
    fn cast_type_normalization() {
        for dialect in DIALECTS {
            for type_name in ["VARCHAR", "INTEGER", "BOOL", "NUMERIC", "DOUBLE PRECISION"] {
                let sql = format!("SELECT CAST(x AS {type_name}) FROM t");
                assert_eq!(ro_d(&sql, dialect), Some(true), "{dialect}: {sql}");
            }
        }
        for sql in [
            "SELECT CAST(a AS VARCHAR), CAST(b AS INTEGER), CAST(c AS BOOL) FROM t",
            "SELECT TRY_CAST(a AS VARCHAR), TRY_CAST(b AS NUMERIC) FROM t",
            "SELECT a::VARCHAR, b::INTEGER, c::DOUBLE PRECISION FROM t",
            "SELECT CAST(a AS CHARACTER VARYING), CAST(b AS TIMESTAMP WITH TIME ZONE) FROM t",
            "SELECT CAST(CAST(a AS INTEGER) AS VARCHAR) FROM t",
            "WITH x AS (SELECT CAST(a AS VARCHAR) v FROM t) SELECT v FROM x UNION ALL SELECT CAST(b AS VARCHAR) FROM t",
            "SELECT CAST(a AS /* type */ VARCHAR), CAST(b AS DECIMAL(10,2)) FROM t",
        ] {
            assert_eq!(ro_duckdb(sql), Some(true), "{sql}");
        }
        assert_eq!(
            ro_d("SELECT x::INTEGER, y::DOUBLE PRECISION FROM t", "postgres"),
            Some(true)
        );
        for sql in [
            "SELECT CAST(a AS STRUCT(x VARCHAR, y INTEGER)), CAST(b AS VARCHAR[]) FROM t",
            "SELECT text, varchar, integer, CAST(x AS VARCHAR) AS varchar FROM t AS text",
            "SELECT 1 AS int, 2 AS varchar, 3 AS boolean",
        ] {
            assert_eq!(ro_duckdb(sql), Some(true), "{sql}");
        }
        // Accepted divergence: CONVERT is not on the allowlist (it asks).
        assert_eq!(
            ro_d(
                "SELECT CONVERT(VARCHAR(50), x), CONVERT(INT, y) FROM t",
                "tsql"
            ),
            None
        );
        assert_eq!(
            ro_d(
                "SELECT CONVERT(x, CHAR), CONVERT(y, SIGNED INTEGER) FROM t",
                "mysql"
            ),
            None
        );
    }

    #[test]
    fn casts_in_output_contexts() {
        let sql = "SELECT CAST(x AS VARCHAR) FROM t";
        assert!(duckdb_writes_only_main(&format!(
            "CREATE TABLE main.result AS {sql}"
        )));
        assert_eq!(
            duckdb_copy_export_target(&format!("COPY ({sql}) TO 'tmp/out.csv'")).as_deref(),
            Some("tmp/out.csv")
        );
        assert_eq!(
            ro_temp(&format!("CREATE TEMP TABLE result AS {sql}")),
            Some(true)
        );
        for dialect in DIALECTS {
            for sql in [
                "SELECT CAST(fictional_effect() AS VARCHAR)",
                "WITH x AS (DELETE FROM t RETURNING *) SELECT CAST(a AS VARCHAR) FROM x",
                "SELECT CAST(a AS VARCHAR) FROM t; DROP TABLE t",
                "SELECT CAST(a AS VARCHAR) INTO outfile FROM t",
                "SELECT CAST(a AS VARCHAR) FROM t UNRECOGNIZED discarded",
                "SELECT CAST(a AS VARCHAR) FROM t WHERE",
            ] {
                assert_ne!(ro_batch(sql, Some(dialect)), Some(true), "{dialect}: {sql}");
            }
        }
        for sql in [
            "SELECT CAST(query('DROP TABLE t') AS VARCHAR)",
            "SELECT CAST(a AS fictional_type) FROM t",
        ] {
            assert_ne!(ro_d(sql, "duckdb"), Some(true), "{sql}");
            assert!(!duckdb_writes_only_main(&format!(
                "CREATE TABLE main.result AS {sql}"
            )));
            assert_eq!(
                duckdb_copy_export_target(&format!("COPY ({sql}) TO 'tmp/out.csv'")),
                None
            );
        }
    }

    // -- Syntax beyond the ported Python tests (checked with sql_compare) -----

    #[test]
    fn grammar_extensions() {
        for (dialect, sql) in [
            ("duckdb", "SELECT a FROM t GROUP BY ALL"),
            ("duckdb", "SELECT a FROM t GROUP BY ROLLUP (a, t.b), c"),
            ("postgres", "SELECT a FROM t GROUP BY CUBE (a)"),
            ("duckdb", "SELECT string_agg(DISTINCT a, ',') FROM t"),
            ("duckdb", "SELECT $$export$$"),
            ("postgres", "SELECT $$it's$$ FROM t"),
            ("tsql", "SELECT @@VERSION"),
            ("tsql", "SELECT TOP 5 * FROM t"),
            ("tsql", "SELECT DISTINCT TOP (3) a FROM t"),
            ("duckdb", "SELECT 1 AS value, a AS mode FROM t"),
            ("duckdb", "SELECT date_trunc('second', ts) FROM t"),
            ("duckdb", "SELECT strptime(a, '%d') FROM t"),
            ("duckdb", "SELECT quantile_disc(a, 0.99) FROM t"),
            (
                "duckdb",
                "SELECT sum(d) OVER (PARTITION BY a ORDER BY ts, d ROWS UNBOUNDED PRECEDING) FROM e",
            ),
            (
                "postgres",
                "SELECT sum(d) OVER (ORDER BY ts RANGE BETWEEN 1 PRECEDING AND CURRENT ROW) FROM e",
            ),
            ("duckdb", "SELECT count(DISTINCT (a, b)) FROM t"),
            (
                "duckdb",
                "SELECT count(*) FILTER (a > 1), max(b) FILTER (c) FROM t",
            ),
            (
                "duckdb",
                "SELECT ts - INTERVAL (CAST(a AS BIGINT)) MILLISECOND FROM t",
            ),
            (
                "duckdb",
                "SELECT time_bucket(INTERVAL 30 SECOND, ts) FROM t",
            ),
            (
                "duckdb",
                "SELECT a FROM t WHERE ts < TIMESTAMP '2026-10-08 06:45:30'",
            ),
            (
                "duckdb",
                "SELECT string_agg(a, ', ' ORDER BY n DESC) FROM t",
            ),
            ("duckdb", "SELECT column_name FROM (DESCRIBE t)"),
            (
                "duckdb",
                "SELECT * FROM read_csv('x.csv', header=true, quote='')",
            ),
            (
                "duckdb",
                "WITH top AS (SELECT a FROM t) SELECT a sample FROM top",
            ),
            ("duckdb", "SELECT unnest(str_split(a, '&')) FROM t"),
            ("duckdb", "FROM t WHERE x"),
            ("duckdb", "FROM t ORDER BY a LIMIT 1"),
            ("duckdb", "FROM t SELECT a WHERE x UNION ALL SELECT 1"),
            ("duckdb", "SELECT * FROM (FROM t SELECT a LIMIT 1)"),
            ("duckdb", "SELECT * FROM (FROM read_csv('x.csv'))"),
        ] {
            assert_eq!(ro_d(sql, dialect), Some(true), "{dialect}: {sql}");
        }
        // Intended divergence: read-only syntax SQLGlot does not parse or
        // regenerate faithfully (Python asks), which sqlparser verifies.
        for (dialect, sql) in [
            ("duckdb", "SELECT a FROM t GROUP BY ROLLUP (a + 1)"),
            (
                "duckdb",
                "SELECT sum(a) OVER (ORDER BY ts GROUPS 1 PRECEDING) FROM t",
            ),
            ("duckdb", "SELECT list_value((SELECT 1), d)"),
            ("duckdb", "SELECT $$a$$ || $tag$b$tag$"),
            ("duckdb", "SELECT a AS key FROM t"),
            ("duckdb", "SELECT INTERVAL (a) week FROM t"),
            ("duckdb", "SELECT * FROM (FROM t WHERE a)"),
            ("duckdb", "SELECT * FROM t WHERE a IN (FROM t ORDER BY a)"),
            ("postgres", "SELECT string_agg(a, ',' ORDER BY a) FROM t"),
            ("tsql", "SELECT string_agg(DISTINCT a, ',') FROM t"),
        ] {
            assert_eq!(ro_d(sql, dialect), Some(true), "{dialect}: {sql}");
        }
        // Side effects and syntax the dialect lacks must ask.
        for (dialect, sql) in [
            ("duckdb", "SELECT i//**/(u)"),
            ("mysql", "SELECT $$x$$$$y$$"),
            ("duckdb", "SELECT TOP 5 * FROM t"),
            ("tsql", "SELECT TOP 2.5 a FROM t"),
            ("postgres", "SELECT count(*) FILTER (a > 1) FROM t"),
            ("postgres", "SELECT column_name FROM (DESCRIBE t)"),
            ("duckdb", "FROM t WHERE x UNION ALL SELECT 1"),
            ("duckdb", "SELECT INTERVAL (query('DROP TABLE t')) SECOND"),
            (
                "duckdb",
                "SELECT * FROM read_csv('x.csv', header=query('x'))",
            ),
            ("duckdb", "SELECT * FROM read_text('x.csv')"),
            ("duckdb", "SELECT (a, writefile('x', 'y')) FROM t"),
            ("duckdb", "SELECT * FROM query('DELETE FROM t')"),
            ("duckdb", "SELECT nextval('s')"),
            ("postgres", "SELECT pg_catalog.lower(a) FROM t"),
            ("postgres", "SELECT \"lower\"(a) FROM t"),
            ("postgres", "SELECT a FROM t FOR UPDATE"),
            ("postgres", "SELECT $1"),
            ("duckdb", "SELECT CAST(a AS my_type) FROM t"),
            (
                "postgres",
                "WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d",
            ),
            ("postgres", "SELECT a INTO b FROM t"),
        ] {
            assert_ne!(ro_d(sql, dialect), Some(true), "{dialect}: {sql}");
        }
    }

    #[test]
    fn duckdb_rewrites_for_verification() {
        for sql in [
            "SELECT count(*) FILTER (a > 1), max(b) FILTER (WHERE c) FROM t",
            "WITH top AS (SELECT 1 a) SELECT a FROM top",
            "SELECT * FROM (DESCRIBE t)",
            "SELECT a FROM t WHERE b USING SAMPLE 8 ROWS",
            "SELECT TRY_CAST(a AS INET) <<= b::INET FROM t",
            "FROM t WHERE x",
            "SELECT count(*) operator FROM t ORDER BY hash(a || b)",
        ] {
            assert_eq!(ro_duckdb(sql), Some(true), "{sql}");
        }
        // A rewrite never hides an effect.
        for sql in [
            "SELECT count(*) FILTER (writefile('x', 'y')) FROM t",
            "SELECT * FROM (DESCRIBE t) WHERE query('DROP TABLE t')",
            "FROM query('DROP TABLE t')",
            "WITH top AS (SELECT writefile('x', 'y')) SELECT 1",
        ] {
            assert_ne!(ro_duckdb(sql), Some(true), "{sql}");
        }
    }
}
