//! Port of `dippy.core.sql`: read-only vs write SQL classification.
//!
//! The lexical layer (masking of literals and comments, statement splitting,
//! keyword checks, CTE skipping, `SELECT ... INTO`, temp-table and EXPLAIN
//! prefixes) is an exact port of the Python code.
//!
//! Python verifies query structure with SQLGlot (`_verify_query_ast`,
//! `_verified_main_write`, the COPY export check and the temp-table parse).
//! SQLGlot has no Rust equivalent, so [`grammar`] accepts a conservative
//! subset of SQL instead: it answers "verified" only for token streams that
//! it parses completely and whose every construct (functions with argument
//! counts, types, clauses, per-dialect quirks) was checked against SQLGlot.
//! Everything else is "unverified", which callers turn into `None`/`false`,
//! i.e. an approval prompt. Accepted divergences (Rust asks where Python
//! allows) are documented on [`grammar`].

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
fn py_isspace(c: char) -> bool {
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
    let mut k = i;
    for p in pat.chars() {
        if s.get(k) != Some(&p) {
            return false;
        }
        k += 1;
    }
    true
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
            let parsed = grammar::Dialect::parse(dialect)
                .is_some_and(|d| grammar::verify_temp_create(&to_string(statement), d));
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
    let d = grammar::Dialect::parse(parser_dialect)?;
    grammar::verify_query(sql, d).then_some(true)
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
    grammar::verify_main_write(statement, &root)
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
    grammar::copy_export_target(&statements[0])
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
    ctx.word_has_expansions = raw.iter().map(|w| w.contains(['$', '`'])).collect();
    ctx.raw_words = raw;
    ctx
}

// ---------------------------------------------------------------------------

/// Conservative replacement for the SQLGlot structural checks.
///
/// Accepts only a subset of SQL that SQLGlot parses into the same verdict
/// (checked by `rust/parity/sql_fuzz.py`). Accepted divergences, where Python
/// returns `True` (or a COPY target) and Rust does not:
///
/// * functions outside [`FUNCTIONS`] (or with other argument counts), and
///   function call syntax other than plain comma-separated arguments,
///   `count(*)`, `DISTINCT` in simple aggregates, `FILTER` and `OVER`;
/// * types outside the small built-in list (arrays only in DuckDB);
/// * identifiers in [`RESERVED`] (e.g. `key`, `value`, `user`), even where
///   SQLGlot would accept them;
/// * dollar-quoted strings, prefixed strings (`N'..'`, `E'..'`), strings with
///   backslashes, numbers other than plain decimals/exponents, parameters,
///   `@@variables`, `#`/`--x` MySQL comments and operators outside
///   `= <> != < > <= >= || :: + - * / %` (plus `<<=`/`>>=` in PostgreSQL and
///   DuckDB), including adjacent operators such as `=-1`;
/// * clauses outside SELECT/FROM/JOIN/WHERE/GROUP BY/HAVING/ORDER BY/LIMIT/
///   OFFSET/set operations/CTEs, DuckDB `QUALIFY`, `USING SAMPLE`, `FROM`-first
///   and `VALUES`; e.g. `NULLS FIRST`, `TOP`, `FETCH`, window frames, lateral
///   joins, table functions, `COLLATE`, `INTERVAL`, `EXTRACT`;
/// * DuckDB writes other than plain CREATE TABLE (AS query or column list),
///   INSERT, UPDATE, DELETE, DROP TABLE and ALTER TABLE ADD COLUMN;
/// * COPY exports with options other than FORMAT/HEADER/DELIMITER/QUOTE/
///   ESCAPE/NULL.
mod grammar {
    use std::collections::HashSet;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Dialect {
        Base,
        Postgres,
        Mysql,
        Sqlite,
        Tsql,
        Duckdb,
        Athena,
    }

    impl Dialect {
        /// SQLGlot dialect name; unknown names fail like `get_or_raise`.
        pub fn parse(name: Option<&str>) -> Option<Self> {
            Some(match name {
                None => Self::Base,
                Some("postgres") => Self::Postgres,
                Some("mysql") => Self::Mysql,
                Some("sqlite") => Self::Sqlite,
                Some("tsql") => Self::Tsql,
                Some("duckdb") => Self::Duckdb,
                Some("athena") => Self::Athena,
                Some(_) => return None,
            })
        }

        fn index(self) -> usize {
            self as usize
        }
    }

    use Dialect::*;

    /// Generated by `rust/parity/sql_tables.py`.
    const RESERVED: [&str; 329] = [
        "ADD",
        "AGAINST",
        "ALL",
        "ALTER",
        "ALWAYS",
        "ANALYZE",
        "AND",
        "ANTI",
        "ANY",
        "APPLY",
        "ARRAY",
        "AS",
        "ASC",
        "ASOF",
        "AT",
        "ATTACH",
        "BEGIN",
        "BERNOULLI",
        "BETWEEN",
        "BIGINT",
        "BINARY",
        "BLOB",
        "BLOCK",
        "BOTH",
        "BY",
        "CALL",
        "CASE",
        "CAST",
        "CHAR",
        "CHARACTER",
        "CHARSET",
        "COLLATE",
        "COLUMN",
        "COLUMNS",
        "CONNECT",
        "CONSTRAINT",
        "CONVERT",
        "COPY",
        "CREATE",
        "CROSS",
        "CUBE",
        "CURRENT",
        "CURRENT_DATE",
        "CURRENT_TIME",
        "CURRENT_TIMESTAMP",
        "CURRENT_USER",
        "DATABASE",
        "DEALLOCATE",
        "DEC",
        "DECIMAL",
        "DECLARE",
        "DEFAULT",
        "DELETE",
        "DENSE_RANK",
        "DESC",
        "DESCRIBE",
        "DETACH",
        "DETERMINISTIC",
        "DISTINCT",
        "DIV",
        "DO",
        "DOUBLE",
        "DROP",
        "DUMPFILE",
        "ELSE",
        "ELSEIF",
        "END",
        "ESCAPE",
        "EXCEPT",
        "EXCLUDE",
        "EXECUTE",
        "EXISTS",
        "EXPLAIN",
        "EXPORT",
        "EXTRACT",
        "FALSE",
        "FETCH",
        "FILTER",
        "FINAL",
        "FIRST",
        "FLOAT",
        "FLOAT4",
        "FLOAT8",
        "FOLLOWING",
        "FOR",
        "FORCE",
        "FOREIGN",
        "FORMAT",
        "FROM",
        "FULL",
        "FUNCTION",
        "GET",
        "GLOB",
        "GRANT",
        "GROUP",
        "GROUPING",
        "GROUPS",
        "HAVING",
        "IGNORE",
        "ILIKE",
        "IN",
        "INCLUDE",
        "INDEX",
        "INNER",
        "INSERT",
        "INT",
        "INT1",
        "INT2",
        "INT4",
        "INT8",
        "INTEGER",
        "INTERSECT",
        "INTERVAL",
        "INTO",
        "IS",
        "ISNULL",
        "JOIN",
        "KEEP",
        "KEY",
        "KILL",
        "LAST",
        "LATERAL",
        "LEADING",
        "LEFT",
        "LEVEL",
        "LIKE",
        "LIMIT",
        "LIST",
        "LOAD",
        "LOCAL",
        "LOCALTIME",
        "LOCALTIMESTAMP",
        "LOCK",
        "LOCKED",
        "LONG",
        "LONGBLOB",
        "LONGTEXT",
        "MAP",
        "MATCH",
        "MATCHED",
        "MATCH_RECOGNIZE",
        "MEDIUMBLOB",
        "MEDIUMINT",
        "MEDIUMTEXT",
        "MEMBER",
        "MERGE",
        "METHOD",
        "MINUS",
        "MOD",
        "MODE",
        "NATURAL",
        "NOCYCLE",
        "NOT",
        "NOTNULL",
        "NOWAIT",
        "NULL",
        "NULLABLE",
        "NULLS",
        "NUMERIC",
        "OF",
        "OFFSET",
        "ON",
        "ONLY",
        "OPERATOR",
        "OPTIMIZE",
        "OPTION",
        "OR",
        "ORDER",
        "ORDINALITY",
        "OUTER",
        "OUTFILE",
        "OUTPUT",
        "OVER",
        "OVERLAPS",
        "OVERLAY",
        "PARTITION",
        "PARTITIONED_BY",
        "PERCENT",
        "PERCENTILE_CONT",
        "PERCENTILE_DISC",
        "PIVOT",
        "POSITION",
        "POSITIONAL",
        "PRAGMA",
        "PRECEDING",
        "PRECISION",
        "PREPARE",
        "PREWHERE",
        "PRIMARY",
        "PRIOR",
        "PROCEDURE",
        "QUALIFY",
        "RANGE",
        "RANK",
        "REAL",
        "RECURSIVE",
        "REFERENCES",
        "REGEXP",
        "REINDEX",
        "RENAME",
        "REPEATABLE",
        "REPLACE",
        "RESPECT",
        "RETURNING",
        "REVOKE",
        "RIGHT",
        "RLIKE",
        "ROLLBACK",
        "ROLLUP",
        "ROW",
        "ROWNUM",
        "ROWS",
        "SAMPLE",
        "SCHEMA",
        "SEED",
        "SELECT",
        "SEMI",
        "SEPARATOR",
        "SERDEPROPERTIES",
        "SESSION_USER",
        "SET",
        "SETTINGS",
        "SHARE",
        "SHOW",
        "SIMILAR",
        "SKIP",
        "SMALLINT",
        "SOME",
        "SOUNDS",
        "STAR",
        "START",
        "STRAIGHT_JOIN",
        "STRUCT",
        "SUBSTRING",
        "SUMMARIZE",
        "SYSDATE",
        "SYSTEM",
        "SYSTEM_TIME",
        "SYSTEM_USER",
        "SYSTIMESTAMP",
        "TABLE",
        "TABLESAMPLE",
        "THEN",
        "TIES",
        "TIME",
        "TIMESTAMP",
        "TINYBLOB",
        "TINYINT",
        "TINYTEXT",
        "TO",
        "TOP",
        "TRAILING",
        "TRIGGER",
        "TRIM",
        "TRUE",
        "TRUNCATE",
        "TRY_CAST",
        "UNBOUNDED",
        "UNCACHE",
        "UNION",
        "UNIQUE",
        "UNKNOWN",
        "UNLOAD",
        "UNLOCK",
        "UNNEST",
        "UNPIVOT",
        "UNSIGNED",
        "UPDATE",
        "USE",
        "USER",
        "USING",
        "VACUUM",
        "VALUE",
        "VALUES",
        "VARBINARY",
        "VARCHAR",
        "VARYING",
        "VIEW",
        "WHEN",
        "WHERE",
        "WINDOW",
        "WITH",
        "WITHIN",
        "WITHOUT",
        "XML",
        "XOR",
        "ZONE",
        "_ARMSCII8",
        "_ASCII",
        "_BIG5",
        "_BINARY",
        "_CP1250",
        "_CP1251",
        "_CP1256",
        "_CP1257",
        "_CP850",
        "_CP852",
        "_CP866",
        "_CP932",
        "_DEC8",
        "_EUCJPMS",
        "_EUCKR",
        "_GB18030",
        "_GB2312",
        "_GBK",
        "_GEOSTD8",
        "_GREEK",
        "_HEBREW",
        "_HP8",
        "_KEYBCS2",
        "_KOI8R",
        "_KOI8U",
        "_LATIN1",
        "_LATIN2",
        "_LATIN5",
        "_LATIN7",
        "_MACCE",
        "_MACROMAN",
        "_SJIS",
        "_SWE7",
        "_TIS620",
        "_UCS2",
        "_UJIS",
        "_UTF16",
        "_UTF16LE",
        "_UTF32",
        "_UTF8",
        "_UTF8MB3",
        "_UTF8MB4",
    ];

    /// Generated by `rust/parity/sql_tables.py`: argument-count bit masks
    /// per parser dialect (base, postgres, mysql, sqlite, tsql, duckdb, athena).
    const FUNCTIONS: [(&str, [u8; 7]); 122] = [
        ("abs", [2, 2, 2, 2, 2, 2, 2]),
        ("any_value", [2, 2, 2, 2, 2, 2, 2]),
        ("approx_count_distinct", [6, 6, 6, 6, 6, 2, 6]),
        ("arg_max", [12, 12, 12, 12, 12, 12, 12]),
        ("arg_min", [12, 12, 12, 12, 12, 12, 12]),
        ("array_agg", [2, 2, 2, 2, 2, 2, 2]),
        ("array_length", [2, 6, 2, 2, 2, 6, 2]),
        ("avg", [2, 2, 2, 2, 2, 2, 2]),
        ("bit_and", [0, 0, 0, 0, 0, 31, 0]),
        ("bit_or", [0, 0, 0, 0, 0, 31, 0]),
        ("bit_xor", [0, 0, 0, 0, 0, 31, 0]),
        ("bool_and", [2, 2, 2, 2, 2, 2, 2]),
        ("bool_or", [2, 2, 2, 2, 2, 2, 2]),
        ("ceil", [7, 7, 7, 7, 7, 7, 7]),
        ("ceiling", [6, 6, 6, 6, 6, 6, 6]),
        ("char_length", [14, 14, 2, 14, 2, 2, 14]),
        ("character_length", [14, 14, 2, 14, 2, 2, 14]),
        ("coalesce", [30, 30, 30, 30, 30, 30, 30]),
        ("concat", [30, 30, 30, 30, 30, 30, 30]),
        ("concat_ws", [30, 30, 30, 30, 30, 30, 30]),
        ("contains", [4, 4, 4, 4, 4, 4, 4]),
        ("corr", [4, 4, 4, 4, 4, 4, 4]),
        ("count", [31, 31, 31, 31, 31, 31, 31]),
        ("covar_pop", [4, 4, 4, 4, 4, 4, 4]),
        ("covar_samp", [4, 4, 4, 4, 4, 4, 4]),
        ("ends_with", [4, 4, 4, 4, 4, 4, 4]),
        ("entropy", [0, 0, 0, 0, 0, 31, 0]),
        ("exp", [2, 2, 2, 2, 2, 2, 2]),
        ("first_value", [2, 2, 2, 2, 2, 2, 2]),
        ("floor", [7, 7, 7, 7, 7, 7, 7]),
        ("fsum", [0, 0, 0, 0, 0, 31, 0]),
        ("geometric_mean", [0, 0, 0, 0, 0, 31, 0]),
        ("greatest", [30, 30, 30, 30, 30, 30, 30]),
        ("group_concat", [14, 6, 30, 6, 6, 6, 6]),
        ("histogram", [0, 0, 0, 0, 0, 31, 0]),
        ("ifnull", [30, 30, 30, 30, 30, 30, 30]),
        ("iif", [12, 12, 12, 12, 12, 12, 12]),
        ("initcap", [6, 6, 6, 6, 6, 6, 2]),
        ("instr", [28, 12, 12, 12, 12, 12, 28]),
        ("isnull", [0, 0, 0, 0, 30, 0, 0]),
        ("kahan_sum", [0, 0, 0, 0, 0, 31, 0]),
        ("kurtosis", [0, 0, 0, 0, 0, 31, 0]),
        ("kurtosis_pop", [0, 0, 0, 0, 0, 31, 0]),
        ("lag", [14, 14, 14, 14, 14, 14, 14]),
        ("last_value", [2, 2, 2, 2, 2, 2, 2]),
        ("lead", [14, 14, 14, 14, 14, 14, 14]),
        ("least", [30, 30, 30, 26, 30, 30, 30]),
        ("left", [4, 4, 4, 4, 4, 4, 4]),
        ("len", [14, 14, 2, 14, 2, 2, 14]),
        ("length", [14, 6, 2, 14, 2, 2, 14]),
        ("list_sum", [0, 0, 0, 0, 0, 31, 0]),
        ("list_value", [0, 0, 0, 0, 0, 31, 0]),
        ("ln", [2, 2, 2, 2, 2, 2, 2]),
        ("log", [6, 6, 6, 6, 6, 6, 6]),
        ("log10", [3, 3, 3, 3, 3, 3, 3]),
        ("lower", [2, 2, 2, 2, 2, 2, 2]),
        ("lpad", [12, 12, 12, 12, 12, 12, 12]),
        ("ltrim", [6, 6, 6, 6, 6, 6, 6]),
        ("mad", [0, 0, 0, 0, 0, 31, 0]),
        ("max", [30, 30, 30, 30, 30, 30, 30]),
        ("md5", [2, 2, 2, 2, 2, 2, 2]),
        ("median", [2, 2, 2, 2, 2, 2, 2]),
        ("min", [30, 30, 30, 30, 30, 30, 30]),
        ("mod", [4, 4, 4, 4, 4, 4, 4]),
        ("mode", [0, 0, 0, 0, 0, 31, 0]),
        ("nullif", [4, 4, 4, 4, 4, 4, 4]),
        ("nvl", [30, 30, 30, 30, 30, 30, 30]),
        ("pow", [4, 4, 4, 4, 4, 4, 4]),
        ("power", [4, 4, 4, 4, 4, 4, 4]),
        ("product", [0, 0, 0, 0, 0, 31, 0]),
        ("regexp_extract", [28, 28, 28, 28, 28, 28, 12]),
        ("regexp_replace", [28, 28, 28, 28, 28, 28, 12]),
        ("regr_avgx", [0, 0, 0, 0, 0, 31, 0]),
        ("regr_avgy", [0, 0, 0, 0, 0, 31, 0]),
        ("regr_count", [0, 0, 0, 0, 0, 31, 0]),
        ("regr_intercept", [0, 0, 0, 0, 0, 31, 0]),
        ("regr_r2", [0, 0, 0, 0, 0, 31, 0]),
        ("regr_slope", [0, 0, 0, 0, 0, 31, 0]),
        ("regr_sxx", [0, 0, 0, 0, 0, 31, 0]),
        ("regr_sxy", [0, 0, 0, 0, 0, 31, 0]),
        ("regr_syy", [0, 0, 0, 0, 0, 31, 0]),
        ("repeat", [4, 4, 4, 4, 4, 4, 4]),
        ("replace", [12, 12, 12, 12, 12, 12, 12]),
        ("reservoir_quantile", [0, 0, 0, 0, 0, 31, 0]),
        ("right", [4, 4, 4, 4, 4, 4, 4]),
        ("round", [14, 14, 14, 14, 14, 14, 14]),
        ("row_number", [3, 3, 3, 3, 3, 3, 3]),
        ("rpad", [12, 12, 12, 12, 12, 12, 12]),
        ("rtrim", [6, 6, 6, 6, 6, 6, 6]),
        ("sha1", [2, 2, 2, 2, 2, 2, 2]),
        ("sign", [2, 2, 2, 2, 2, 2, 2]),
        ("skewness", [0, 0, 0, 0, 0, 31, 0]),
        ("split_part", [8, 8, 8, 8, 0, 8, 8]),
        ("sqrt", [2, 2, 2, 2, 2, 2, 2]),
        ("starts_with", [4, 4, 4, 4, 4, 4, 4]),
        ("stats", [0, 0, 0, 0, 0, 31, 0]),
        ("stddev", [2, 2, 2, 2, 2, 2, 2]),
        ("stddev_pop", [2, 2, 2, 2, 2, 2, 2]),
        ("stddev_samp", [2, 2, 2, 2, 2, 2, 2]),
        ("string_agg", [14, 6, 6, 6, 6, 6, 6]),
        ("strpos", [28, 12, 12, 12, 12, 12, 12]),
        ("substr", [14, 14, 14, 14, 14, 14, 14]),
        ("substring", [14, 14, 14, 14, 14, 14, 14]),
        ("sum", [2, 2, 2, 2, 2, 2, 2]),
        ("to_centuries", [0, 0, 0, 0, 0, 31, 0]),
        ("to_days", [2, 2, 0, 2, 2, 2, 2]),
        ("to_decades", [0, 0, 0, 0, 0, 31, 0]),
        ("to_hours", [0, 0, 0, 0, 0, 31, 0]),
        ("to_microseconds", [0, 0, 0, 0, 0, 31, 0]),
        ("to_milliseconds", [0, 0, 0, 0, 0, 31, 0]),
        ("to_minutes", [0, 0, 0, 0, 0, 31, 0]),
        ("to_months", [0, 0, 0, 0, 0, 31, 0]),
        ("to_nanoseconds", [0, 0, 0, 0, 0, 31, 0]),
        ("to_seconds", [0, 0, 0, 0, 0, 31, 0]),
        ("to_weeks", [0, 0, 0, 0, 0, 31, 0]),
        ("to_years", [0, 0, 0, 0, 0, 31, 0]),
        ("trim", [6, 6, 6, 6, 6, 6, 6]),
        ("unnest", [2, 30, 2, 2, 2, 30, 2]),
        ("upper", [2, 2, 2, 2, 2, 2, 2]),
        ("var_pop", [2, 2, 2, 2, 2, 2, 2]),
        ("var_samp", [2, 2, 2, 2, 2, 2, 2]),
        ("variance", [2, 2, 2, 2, 2, 2, 2]),
    ];

    fn is_reserved(word: &str) -> bool {
        RESERVED.binary_search(&word).is_ok()
    }

    fn function_mask(name: &str, d: Dialect) -> u8 {
        let lower = name.to_ascii_lowercase();
        FUNCTIONS
            .binary_search_by(|(n, _)| n.cmp(&lower.as_str()))
            .map(|i| FUNCTIONS[i].1[d.index()])
            .unwrap_or(0)
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Tok {
        /// Unquoted ASCII word, upper-cased.
        Word(String),
        /// Quoted identifier.
        Ident(String),
        /// String literal (decoded).
        Str(String),
        /// Number literal; `true` for plain integers.
        Num(bool),
        Op(&'static str),
    }

    const OPS: [&str; 16] = [
        "=", "<>", "!=", "<", ">", "<=", ">=", "||", "::", "+", "-", "*", "/", "%", "<<=", ">>=",
    ];

    /// Read a quoted token; `backslash` rejects backslashes (escape
    /// characters in some SQLGlot dialects).
    fn quoted(s: &[char], i: &mut usize, close: char, backslash: bool) -> Option<String> {
        let mut out = String::new();
        *i += 1;
        loop {
            let c = *s.get(*i)?;
            if c == close {
                if s.get(*i + 1) == Some(&close) {
                    out.push(c);
                    *i += 2;
                    continue;
                }
                *i += 1;
                break;
            }
            if (backslash && c == '\\') || c == '\r' {
                return None;
            }
            out.push(c);
            *i += 1;
        }
        // A literal glued to a following word or quote is a prefix/suffix
        // form whose meaning differs per dialect.
        if s.get(*i)
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '\'' | '"' | '`' | '$'))
        {
            return None;
        }
        Some(out)
    }

    fn tokenize(sql: &str, d: Dialect) -> Option<Vec<Tok>> {
        let s: Vec<char> = sql.chars().collect();
        let n = s.len();
        let mut out = Vec::new();
        let mut i = 0;
        while i < n {
            let c = s[i];
            let glued = i > 0 && (s[i - 1].is_alphanumeric() || s[i - 1] == '_');
            match c {
                ' ' | '\t' | '\n' => i += 1,
                '-' if s.get(i + 1) == Some(&'-') => {
                    if d == Mysql && !matches!(s.get(i + 2), None | Some(' ' | '\t' | '\n')) {
                        return None;
                    }
                    let start = i;
                    while i < n && s[i] != '\n' {
                        i += 1;
                    }
                    // SQLGlot does not end a line comment at a newline when
                    // it contains block comment markers.
                    let body = &s[start..i];
                    if super::find_seq(body, &['/', '*'], 0).is_some()
                        || super::find_seq(body, &['*', '/'], 0).is_some()
                    {
                        return None;
                    }
                }
                '/' if s.get(i + 1) == Some(&'*') => {
                    if d == Mysql && s.get(i + 2) == Some(&'!') {
                        return None;
                    }
                    let end = super::find_seq(&s, &['*', '/'], i + 2)?;
                    if super::find_seq(&s[i + 2..end], &['/', '*'], 0).is_some() {
                        return None;
                    }
                    i = end + 2;
                }
                '\'' => {
                    if glued {
                        return None;
                    }
                    let backslash = matches!(d, Mysql | Athena);
                    out.push(Tok::Str(quoted(&s, &mut i, '\'', backslash)?));
                }
                '"' | '`' => {
                    if glued || (c == '"') == (d == Mysql) {
                        return None;
                    }
                    let name = quoted(&s, &mut i, c, true)?;
                    if name.is_empty() {
                        return None;
                    }
                    out.push(Tok::Ident(name));
                }
                '[' if d == Tsql => {
                    if glued {
                        return None;
                    }
                    let name = quoted(&s, &mut i, ']', true)?;
                    if name.is_empty() {
                        return None;
                    }
                    out.push(Tok::Ident(name));
                }
                '[' | ']' if d == Duckdb => {
                    out.push(Tok::Op(if c == '[' { "[" } else { "]" }));
                    i += 1;
                }
                '0'..='9' => {
                    if glued {
                        return None;
                    }
                    let start = i;
                    while i < n && s[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i < n && s[i] == '.' {
                        i += 1;
                        if !s.get(i).is_some_and(char::is_ascii_digit) {
                            return None;
                        }
                        while i < n && s[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    if i < n && matches!(s[i], 'e' | 'E') {
                        i += 1;
                        if i < n && matches!(s[i], '+' | '-') {
                            i += 1;
                        }
                        if !s.get(i).is_some_and(char::is_ascii_digit) {
                            return None;
                        }
                        while i < n && s[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    if s.get(i)
                        .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\'' | '"'))
                    {
                        return None;
                    }
                    out.push(Tok::Num(s[start..i].iter().all(char::is_ascii_digit)));
                }
                'A'..='Z' | 'a'..='z' | '_' => {
                    let start = i;
                    while i < n && (s[i].is_ascii_alphanumeric() || s[i] == '_') {
                        i += 1;
                    }
                    if s.get(i).is_some_and(|c| !c.is_ascii() || *c == '$') {
                        return None;
                    }
                    let word: String = s[start..i].iter().collect();
                    out.push(Tok::Word(word.to_ascii_uppercase()));
                }
                '(' | ')' | ',' | '.' => {
                    out.push(Tok::Op(match c {
                        '(' => "(",
                        ')' => ")",
                        ',' => ",",
                        _ => ".",
                    }));
                    i += 1;
                }
                _ => {
                    const OP_CHARS: &str = "<>=!|:+-*/%&^~@#?";
                    let start = i;
                    while i < n
                        && OP_CHARS.contains(s[i])
                        && !(i > start
                            && (super::starts_with(&s, i, "--") || super::starts_with(&s, i, "/*")))
                    {
                        i += 1;
                    }
                    if i == start {
                        return None;
                    }
                    let run: String = s[start..i].iter().collect();
                    let op = OPS.iter().find(|op| **op == run)?;
                    if (*op == "<<=" || *op == ">>=") && !matches!(d, Postgres | Duckdb) {
                        return None;
                    }
                    out.push(Tok::Op(op));
                }
            }
        }
        Some(out)
    }

    struct Parser<'a> {
        toks: &'a [Tok],
        pos: usize,
        d: Dialect,
        depth: usize,
        /// DuckDB `VALUES` may form the outermost query (set operands only).
        root_values: bool,
    }

    const MAX_DEPTH: usize = 48;

    impl<'a> Parser<'a> {
        fn new(toks: &'a [Tok], d: Dialect) -> Self {
            Self {
                toks,
                pos: 0,
                d,
                depth: 0,
                root_values: false,
            }
        }

        fn done(&self) -> bool {
            self.pos == self.toks.len()
        }

        fn peek(&self) -> Option<&Tok> {
            self.toks.get(self.pos)
        }

        fn peek_at(&self, k: usize) -> Option<&Tok> {
            self.toks.get(self.pos + k)
        }

        fn is_kw(&self, kw: &str) -> bool {
            matches!(self.peek(), Some(Tok::Word(w)) if w == kw)
        }

        fn eat_kw(&mut self, kw: &str) -> bool {
            let hit = self.is_kw(kw);
            self.pos += usize::from(hit);
            hit
        }

        fn kw(&mut self, kw: &str) -> Option<()> {
            self.eat_kw(kw).then_some(())
        }

        fn is_op(&self, op: &str) -> bool {
            matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
        }

        fn eat_op(&mut self, op: &str) -> bool {
            let hit = self.is_op(op);
            self.pos += usize::from(hit);
            hit
        }

        fn op(&mut self, op: &str) -> Option<()> {
            self.eat_op(op).then_some(())
        }

        fn num(&mut self) -> Option<()> {
            if matches!(self.peek(), Some(Tok::Num(_))) {
                self.pos += 1;
                return Some(());
            }
            None
        }

        fn integer(&mut self) -> Option<()> {
            if self.peek() == Some(&Tok::Num(true)) {
                self.pos += 1;
                return Some(());
            }
            None
        }

        fn string(&mut self) -> Option<String> {
            if let Some(Tok::Str(s)) = self.peek() {
                let s = s.clone();
                self.pos += 1;
                return Some(s);
            }
            None
        }

        fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
            if self.depth >= MAX_DEPTH {
                return None;
            }
            self.depth += 1;
            let result = f(self);
            self.depth -= 1;
            result
        }

        fn in_dialects(&self, dialects: &[Dialect]) -> Option<()> {
            dialects.contains(&self.d).then_some(())
        }

        fn not_in_dialects(&self, dialects: &[Dialect]) -> Option<()> {
            (!dialects.contains(&self.d)).then_some(())
        }

        fn is_ident(&self) -> bool {
            match self.peek() {
                Some(Tok::Word(w)) => !is_reserved(w),
                Some(Tok::Ident(_)) => true,
                _ => false,
            }
        }

        /// Identifier; returns the upper-cased word for unquoted names.
        fn ident(&mut self) -> Option<Option<String>> {
            let result = match self.peek()? {
                Tok::Word(w) if !is_reserved(w) => Some(w.clone()),
                Tok::Ident(_) => None,
                _ => return None,
            };
            self.pos += 1;
            Some(result)
        }

        fn ident_list(&mut self) -> Option<()> {
            self.op("(")?;
            loop {
                self.ident()?;
                if !self.eat_op(",") {
                    break;
                }
            }
            self.op(")")
        }

        fn starts_query(&self) -> bool {
            self.is_kw("SELECT") || self.is_kw("WITH")
        }

        // -- queries ------------------------------------------------------

        fn query(&mut self) -> Option<()> {
            self.nested(|p| {
                let with = p.is_kw("WITH");
                if p.eat_kw("WITH") {
                    if p.eat_kw("RECURSIVE") {
                        p.not_in_dialects(&[Tsql, Athena])?;
                    }
                    loop {
                        p.cte()?;
                        if !p.eat_op(",") {
                            break;
                        }
                    }
                }
                let bare_values = p.is_kw("VALUES");
                let terms = p.set_expr()?;
                if bare_values
                    && (with || terms == 1)
                    && (p.is_kw("ORDER") || p.is_kw("LIMIT") || with)
                {
                    return None;
                }
                p.order_limit()
            })
        }

        fn cte(&mut self) -> Option<()> {
            self.ident()?;
            if self.is_op("(") {
                self.not_in_dialects(&[Sqlite])?;
                self.ident_list()?;
            }
            self.kw("AS")?;
            self.op("(")?;
            if self.is_kw("VALUES") {
                self.in_dialects(&[Duckdb])?;
                self.values()?;
            } else {
                self.query()?;
            }
            self.op(")")
        }

        /// Set operation chain; returns the number of operands.
        fn set_expr(&mut self) -> Option<usize> {
            self.term()?;
            let mut terms = 1;
            loop {
                if self.eat_kw("UNION") {
                    self.eat_kw("ALL");
                } else if !(self.eat_kw("INTERSECT") || self.eat_kw("EXCEPT")) {
                    return Some(terms);
                }
                self.term()?;
                terms += 1;
            }
        }

        fn term(&mut self) -> Option<()> {
            if self.eat_op("(") {
                self.query()?;
                return self.op(")");
            }
            if self.is_kw("SELECT") {
                return self.select_core();
            }
            if self.d == Duckdb && self.root_values && self.depth == 1 && self.is_kw("VALUES") {
                return self.values();
            }
            if self.d == Duckdb && self.eat_kw("FROM") {
                // One table only: SQLGlot rejects a SELECT tail after
                // joined FROM-first items.
                self.table_factor()?;
                if self.eat_kw("SELECT") {
                    self.select_list()?;
                }
                return self.select_tail();
            }
            None
        }

        fn values(&mut self) -> Option<()> {
            self.kw("VALUES")?;
            loop {
                self.op("(")?;
                self.expr_list()?;
                self.op(")")?;
                if !self.eat_op(",") {
                    return Some(());
                }
            }
        }

        fn order_limit(&mut self) -> Option<()> {
            if self.eat_kw("ORDER") {
                self.kw("BY")?;
                self.order_list()?;
            }
            if self.eat_kw("LIMIT") {
                self.not_in_dialects(&[Tsql])?;
                self.integer()?;
                if self.eat_kw("OFFSET") {
                    self.integer()?;
                }
            }
            Some(())
        }

        fn order_list(&mut self) -> Option<()> {
            loop {
                self.expr()?;
                if !self.eat_kw("ASC") {
                    self.eat_kw("DESC");
                }
                if !self.eat_op(",") {
                    return Some(());
                }
            }
        }

        fn select_core(&mut self) -> Option<()> {
            self.kw("SELECT")?;
            if self.eat_kw("DISTINCT") && self.eat_kw("ON") {
                self.in_dialects(&[Base, Postgres, Duckdb])?;
                self.op("(")?;
                self.expr_list()?;
                self.op(")")?;
            }
            self.select_list()?;
            if self.eat_kw("FROM") {
                self.from_list()?;
            }
            self.select_tail()
        }

        fn select_tail(&mut self) -> Option<()> {
            if self.eat_kw("WHERE") {
                self.expr()?;
            }
            if self.eat_kw("GROUP") {
                self.kw("BY")?;
                self.expr_list()?;
            }
            if self.eat_kw("HAVING") {
                self.expr()?;
            }
            if self.eat_kw("QUALIFY") {
                self.in_dialects(&[Base, Duckdb])?;
                self.expr()?;
            }
            if self.d == Duckdb && self.eat_kw("USING") {
                self.kw("SAMPLE")?;
                self.num()?;
                if !self.eat_kw("ROWS") {
                    self.eat_kw("PERCENT");
                }
            }
            Some(())
        }

        fn select_list(&mut self) -> Option<()> {
            loop {
                self.select_item()?;
                if !self.eat_op(",") {
                    return Some(());
                }
            }
        }

        fn select_item(&mut self) -> Option<()> {
            if self.eat_op("*") {
                return Some(());
            }
            if self.is_ident()
                && matches!(self.peek_at(1), Some(Tok::Op(".")))
                && matches!(self.peek_at(2), Some(Tok::Op("*")))
            {
                self.pos += 3;
                return Some(());
            }
            self.expr()?;
            self.alias()
        }

        fn alias(&mut self) -> Option<()> {
            if self.eat_kw("AS") {
                self.ident()?;
            } else if self.is_ident() {
                self.pos += 1;
            }
            Some(())
        }

        fn from_list(&mut self) -> Option<()> {
            loop {
                self.from_item()?;
                if !self.eat_op(",") {
                    return Some(());
                }
            }
        }

        fn from_item(&mut self) -> Option<()> {
            self.table_factor()?;
            loop {
                if self.eat_kw("CROSS") {
                    self.kw("JOIN")?;
                    self.table_factor()?;
                    continue;
                }
                if self.eat_kw("LEFT") || self.eat_kw("RIGHT") {
                    self.eat_kw("OUTER");
                } else if self.eat_kw("FULL") {
                    self.not_in_dialects(&[Mysql])?;
                    self.eat_kw("OUTER");
                } else if !self.eat_kw("INNER") && !self.is_kw("JOIN") {
                    return Some(());
                }
                self.kw("JOIN")?;
                self.table_factor()?;
                if self.eat_kw("ON") {
                    self.expr()?;
                } else {
                    self.kw("USING")?;
                    self.ident_list()?;
                }
            }
        }

        fn table_factor(&mut self) -> Option<()> {
            if self.eat_op("(") {
                self.query()?;
                self.op(")")?;
            } else {
                self.table_name()?;
            }
            self.alias()
        }

        fn qualified_name(&mut self) -> Option<Vec<Option<String>>> {
            let mut parts = vec![self.ident()?];
            while parts.len() < 3 && self.eat_op(".") {
                parts.push(self.ident()?);
            }
            (!self.is_op(".")).then_some(parts)
        }

        /// Table reference in FROM: not a table function.
        fn table_name(&mut self) -> Option<Vec<Option<String>>> {
            let parts = self.qualified_name()?;
            (!self.is_op("(")).then_some(parts)
        }

        // -- expressions --------------------------------------------------

        fn expr_list(&mut self) -> Option<()> {
            loop {
                self.expr()?;
                if !self.eat_op(",") {
                    return Some(());
                }
            }
        }

        fn expr(&mut self) -> Option<()> {
            self.nested(|p| {
                p.and_expr()?;
                while p.eat_kw("OR") {
                    p.and_expr()?;
                }
                Some(())
            })
        }

        fn and_expr(&mut self) -> Option<()> {
            self.not_expr()?;
            while self.eat_kw("AND") {
                self.not_expr()?;
            }
            Some(())
        }

        fn not_expr(&mut self) -> Option<()> {
            if self.eat_kw("NOT") {
                return self.nested(|p| p.not_expr());
            }
            self.predicate()
        }

        fn predicate(&mut self) -> Option<()> {
            // SQLGlot loses a negation after an operand that starts with a
            // parenthesis (`(a) IS NOT NULL`, `(a) NOT LIKE 'x'`).
            let paren_start = self.is_op("(");
            self.additive()?;
            if let Some(Tok::Op(op)) = self.peek()
                && matches!(
                    *op,
                    "=" | "<>" | "!=" | "<" | ">" | "<=" | ">=" | "<<=" | ">>="
                )
            {
                self.pos += 1;
                return self.additive();
            }
            let negated = self.eat_kw("NOT");
            if negated && paren_start {
                return None;
            }
            if self.eat_kw("LIKE") {
                self.additive()?;
                return self.escape();
            }
            if self.eat_kw("ILIKE") {
                self.in_dialects(&[Base, Postgres, Duckdb])?;
                self.additive()?;
                return self.escape();
            }
            if self.eat_kw("IN") {
                self.op("(")?;
                if self.starts_query() {
                    self.query()?;
                } else {
                    self.expr_list()?;
                }
                return self.op(")");
            }
            if self.eat_kw("BETWEEN") {
                self.additive()?;
                self.kw("AND")?;
                return self.additive();
            }
            if negated {
                return None;
            }
            if self.eat_kw("IS") {
                if self.eat_kw("NOT") && paren_start {
                    return None;
                }
                if self.eat_kw("NULL") {
                    return Some(());
                }
                self.not_in_dialects(&[Tsql, Athena])?;
                if self.eat_kw("TRUE") || self.eat_kw("FALSE") {
                    return Some(());
                }
                return None;
            }
            Some(())
        }

        fn escape(&mut self) -> Option<()> {
            if self.eat_kw("ESCAPE") {
                self.not_in_dialects(&[Mysql, Athena])?;
                self.string()?;
            }
            Some(())
        }

        fn additive(&mut self) -> Option<()> {
            self.multiplicative()?;
            while self.eat_op("+") || self.eat_op("-") || self.eat_op("||") {
                self.multiplicative()?;
            }
            Some(())
        }

        fn multiplicative(&mut self) -> Option<()> {
            self.unary()?;
            while self.eat_op("*") || self.eat_op("/") || self.eat_op("%") {
                self.unary()?;
            }
            Some(())
        }

        fn unary(&mut self) -> Option<()> {
            if self.eat_op("-") {
                return self.nested(|p| p.unary());
            }
            self.primary()?;
            while self.eat_op("::") {
                self.type_name()?;
            }
            Some(())
        }

        fn primary(&mut self) -> Option<()> {
            match self.peek()? {
                Tok::Num(_) => self.pos += 1,
                Tok::Str(_) => {
                    self.pos += 1;
                    if matches!(self.peek(), Some(Tok::Str(_))) {
                        return None;
                    }
                }
                Tok::Op("(") => {
                    self.pos += 1;
                    if self.starts_query() {
                        self.query()?;
                    } else {
                        self.expr()?;
                    }
                    self.op(")")?;
                }
                Tok::Op("[") => {
                    self.pos += 1;
                    if !self.eat_op("]") {
                        self.expr_list()?;
                        self.op("]")?;
                    }
                }
                Tok::Word(w) => {
                    let w = w.clone();
                    match w.as_str() {
                        "NULL" => self.pos += 1,
                        "TRUE" | "FALSE" => {
                            self.not_in_dialects(&[Tsql])?;
                            self.pos += 1;
                        }
                        "CASE" => self.case_expr()?,
                        "CAST" | "TRY_CAST" => {
                            self.pos += 1;
                            self.op("(")?;
                            self.expr()?;
                            self.kw("AS")?;
                            self.type_name()?;
                            self.op(")")?;
                        }
                        "EXISTS" => {
                            self.pos += 1;
                            self.op("(")?;
                            self.query()?;
                            self.op(")")?;
                        }
                        _ if matches!(self.peek_at(1), Some(Tok::Op("("))) => {
                            self.function_call(&w)?;
                        }
                        _ => self.column_ref()?,
                    }
                }
                Tok::Ident(_) => self.column_ref()?,
                Tok::Op(_) => return None,
            }
            Some(())
        }

        fn column_ref(&mut self) -> Option<()> {
            self.ident()?;
            let mut parts = 1;
            while parts < 4 && self.is_op(".") && !matches!(self.peek_at(1), Some(Tok::Op("*"))) {
                self.pos += 1;
                self.ident()?;
                parts += 1;
            }
            if self.is_op("(") || self.is_op(".") {
                return None;
            }
            Some(())
        }

        fn case_expr(&mut self) -> Option<()> {
            self.kw("CASE")?;
            if !self.is_kw("WHEN") {
                self.expr()?;
            }
            self.kw("WHEN")?;
            loop {
                self.expr()?;
                self.kw("THEN")?;
                self.expr()?;
                if !self.eat_kw("WHEN") {
                    break;
                }
            }
            if self.eat_kw("ELSE") {
                self.expr()?;
            }
            self.kw("END")
        }

        fn function_call(&mut self, name: &str) -> Option<()> {
            let mask = function_mask(name, self.d);
            if mask == 0 {
                return None;
            }
            self.pos += 1;
            self.op("(")?;
            let mut args = 0;
            if self.eat_op("*") {
                if name != "COUNT" {
                    return None;
                }
                args = 1;
            } else if !self.is_op(")") {
                if self.eat_kw("DISTINCT")
                    && !matches!(name, "COUNT" | "SUM" | "AVG" | "MIN" | "MAX")
                {
                    return None;
                }
                loop {
                    self.expr()?;
                    args += 1;
                    if !self.eat_op(",") {
                        break;
                    }
                }
            }
            self.op(")")?;
            if args > 4 || mask & (1 << args) == 0 {
                return None;
            }
            let aggregate = matches!(name, "COUNT" | "SUM" | "AVG" | "MIN" | "MAX");
            if self.eat_kw("FILTER") {
                if !aggregate {
                    return None;
                }
                self.op("(")?;
                self.kw("WHERE")?;
                self.expr()?;
                self.op(")")?;
            }
            if self.eat_kw("OVER") {
                let window = matches!(
                    name,
                    "ROW_NUMBER" | "LAG" | "LEAD" | "FIRST_VALUE" | "LAST_VALUE"
                );
                if !(aggregate || window) {
                    return None;
                }
                self.op("(")?;
                if self.eat_kw("PARTITION") {
                    self.kw("BY")?;
                    self.expr_list()?;
                }
                if self.eat_kw("ORDER") {
                    self.kw("BY")?;
                    self.order_list()?;
                }
                self.op(")")?;
            }
            Some(())
        }

        fn type_name(&mut self) -> Option<()> {
            let Some(Tok::Word(w)) = self.peek() else {
                return None;
            };
            let w = w.clone();
            self.pos += 1;
            match w.as_str() {
                "INT" | "INTEGER" | "BIGINT" | "SMALLINT" | "TINYINT" | "BOOL" | "BOOLEAN"
                | "TEXT" | "FLOAT" | "REAL" | "BLOB" | "JSON" | "UUID" | "TIME" | "INET" => {}
                "VARCHAR" | "CHAR" | "DECIMAL" | "NUMERIC" => self.type_args()?,
                "CHARACTER" => {
                    self.kw("VARYING")?;
                    self.type_args()?;
                }
                "DOUBLE" => {
                    self.eat_kw("PRECISION");
                }
                "DATE" => self.not_in_dialects(&[Sqlite])?,
                "TIMESTAMPTZ" => self.not_in_dialects(&[Mysql])?,
                "TIMESTAMP" => {
                    self.not_in_dialects(&[Mysql])?;
                    if self.eat_kw("WITH") {
                        self.not_in_dialects(&[Tsql])?;
                        self.kw("TIME")?;
                        self.kw("ZONE")?;
                    }
                }
                _ => return None,
            }
            while self.eat_op("[") {
                self.op("]")?;
            }
            Some(())
        }

        fn type_args(&mut self) -> Option<()> {
            if self.eat_op("(") {
                self.num()?;
                if self.eat_op(",") {
                    self.num()?;
                }
                self.op(")")?;
            }
            Some(())
        }

        // -- DuckDB writes ------------------------------------------------

        /// Mutation target in the main database (`_main_table_target`).
        fn main_target(&mut self) -> Option<()> {
            let parts = self.qualified_name()?;
            match parts.as_slice() {
                [_] => Some(()),
                [Some(db), _] if db == "MAIN" => Some(()),
                _ => None,
            }
        }

        fn column_def(&mut self) -> Option<()> {
            self.ident()?;
            self.type_name()
        }

        fn main_write(&mut self, root: &str) -> Option<()> {
            match root {
                "CREATE" => {
                    self.kw("CREATE")?;
                    if self.eat_kw("OR") {
                        self.kw("REPLACE")?;
                    }
                    self.kw("TABLE")?;
                    if self.eat_kw("IF") {
                        self.kw("NOT")?;
                        self.kw("EXISTS")?;
                    }
                    self.main_target()?;
                    if self.eat_kw("AS") {
                        if !self.starts_query() {
                            return None;
                        }
                        self.query()
                    } else {
                        self.op("(")?;
                        loop {
                            self.column_def()?;
                            if !self.eat_op(",") {
                                break;
                            }
                        }
                        self.op(")")
                    }
                }
                "INSERT" => {
                    self.kw("INSERT")?;
                    self.kw("INTO")?;
                    self.main_target()?;
                    if self.is_op("(")
                        && !matches!(self.peek_at(1), Some(Tok::Word(w)) if w == "SELECT" || w == "WITH")
                    {
                        self.ident_list()?;
                    }
                    if self.is_kw("VALUES") {
                        self.values()
                    } else if self.starts_query() {
                        self.query()
                    } else {
                        None
                    }
                }
                "UPDATE" => {
                    self.kw("UPDATE")?;
                    self.main_target()?;
                    self.kw("SET")?;
                    loop {
                        self.ident()?;
                        self.op("=")?;
                        self.nested(|p| p.not_expr())?;
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    if self.eat_kw("WHERE") {
                        self.expr()?;
                    }
                    Some(())
                }
                "DELETE" => {
                    self.kw("DELETE")?;
                    self.kw("FROM")?;
                    self.main_target()?;
                    if self.eat_kw("WHERE") {
                        self.expr()?;
                    }
                    Some(())
                }
                "DROP" => {
                    self.kw("DROP")?;
                    self.kw("TABLE")?;
                    if self.eat_kw("IF") {
                        self.kw("EXISTS")?;
                    }
                    self.main_target()
                }
                "ALTER" => {
                    self.kw("ALTER")?;
                    self.kw("TABLE")?;
                    self.main_target()?;
                    self.kw("ADD")?;
                    self.eat_kw("COLUMN");
                    self.column_def()
                }
                _ => None,
            }
        }

        // -- COPY export --------------------------------------------------

        fn copy_export(&mut self) -> Option<String> {
            self.kw("COPY")?;
            self.op("(")?;
            if !self.starts_query() {
                return None;
            }
            self.query()?;
            self.op(")")?;
            self.kw("TO")?;
            let target = self.string()?;
            if self.eat_op("(") {
                let mut seen = HashSet::new();
                loop {
                    let Some(Tok::Word(name)) = self.peek() else {
                        return None;
                    };
                    let name = name.clone();
                    self.pos += 1;
                    if !seen.insert(name.clone()) {
                        return None;
                    }
                    match name.as_str() {
                        "HEADER" => {
                            if !(self.eat_kw("TRUE") || self.eat_kw("FALSE")) {
                                return None;
                            }
                        }
                        "FORMAT" => {
                            let value = match self.peek()? {
                                Tok::Word(w) => w.clone(),
                                Tok::Str(s) => s.to_uppercase(),
                                _ => return None,
                            };
                            if value != "CSV" {
                                return None;
                            }
                            self.pos += 1;
                        }
                        "DELIMITER" | "QUOTE" | "ESCAPE" | "NULL" => {
                            self.string()?;
                        }
                        _ => return None,
                    }
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.op(")")?;
            }
            Some(target)
        }
    }

    fn parse_all(sql: &str, d: Dialect, f: impl FnOnce(&mut Parser) -> Option<()>) -> bool {
        let Some(toks) = tokenize(sql, d) else {
            return false;
        };
        let mut parser = Parser::new(&toks, d);
        f(&mut parser).is_some() && parser.done()
    }

    /// Stand-in for `_verify_query_ast` after the lexical checks: the whole
    /// statement is one verified read-only query.
    pub fn verify_query(sql: &str, d: Dialect) -> bool {
        if tokenize(sql, d).is_some_and(|t| t == [Tok::Word("SELECT".into())]) {
            return true;
        }
        parse_all(sql, d, |p| {
            p.root_values = true;
            p.query()
        })
    }

    /// Stand-in for the temp-table `sqlglot.parse_one` check.
    pub fn verify_temp_create(sql: &str, d: Dialect) -> bool {
        // `parse_one` uses the stock DuckDB parser, which lacks `<<=`/`>>=`.
        if d == Duckdb
            && tokenize(sql, d).is_some_and(|toks| {
                toks.iter()
                    .any(|t| matches!(t, Tok::Op("<<=") | Tok::Op(">>=")))
            })
        {
            return false;
        }
        parse_all(sql, d, |p| {
            p.kw("CREATE")?;
            if p.eat_kw("OR") {
                p.kw("REPLACE")?;
            }
            if !(p.eat_kw("TEMP") || p.eat_kw("TEMPORARY")) {
                return None;
            }
            p.kw("TABLE")?;
            if p.eat_kw("IF") {
                p.kw("NOT")?;
                p.kw("EXISTS")?;
            }
            p.table_name()?;
            p.kw("AS")?;
            if !p.starts_query() {
                return None;
            }
            p.query()
        })
    }

    /// Stand-in for the SQLGlot part of `_verified_main_write`.
    pub fn verify_main_write(sql: &str, root: &str) -> bool {
        parse_all(sql, Duckdb, |p| p.main_write(root))
    }

    /// Stand-in for the SQLGlot part of `duckdb_copy_export_target`.
    pub fn copy_export_target(sql: &str) -> Option<String> {
        let toks = tokenize(sql, Duckdb)?;
        let mut parser = Parser::new(&toks, Duckdb);
        let target = parser.copy_export()?;
        if !parser.done() {
            return None;
        }
        if target.chars().all(super::py_isspace)
            || target == "-"
            || target.chars().any(|c| ":*?[]".contains(c))
            || target.chars().any(|c| (c as u32) < 32 || c as u32 == 127)
            || target.starts_with("/dev/")
            || target.starts_with("/proc/")
        {
            return None;
        }
        Some(target)
    }
}
