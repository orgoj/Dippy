//! Structural SQL checks on the `sqlparser` AST: the stand-ins for the
//! SQLGlot parts of `dippy.core.sql` (`_verify_query_ast`, the temp-table
//! parse, `_verified_main_write` and the COPY export check).
//!
//! A statement is verified only when `sqlparser` parses it completely and
//! every node is on an allowlist: query clauses, expression kinds, table
//! factors, types, and functions from [`FUNCTIONS`] (SQLGlot-known functions
//! that Python verifies, with argument counts) or [`DUCKDB_PURE_FUNCTIONS`].
//! A nested statement, `SELECT ... INTO`, a user-defined type or a function
//! outside the lists is a write (Python: `DML`/`Into`/`USERDEFINED`/
//! `Anonymous` nodes are `False`); any other unlisted node is unverified.
//! Both ask; only the handler description differs.
//!
//! Intended divergences from Python (`rust/parity/sql_compare.py` reports
//! them as "wider"): Rust verifies queries whose syntax SQLGlot does not
//! parse or regenerate faithfully, and the DuckDB functions in
//! [`DUCKDB_PURE_FUNCTIONS`]. It asks for syntax `sqlparser` lacks, for nodes
//! outside the allowlist (`PIVOT`, ClickHouse/Hive/Snowflake clauses, query
//! parameters, ...) and for main-database writes other than plain CREATE
//! TABLE, INSERT, UPDATE, DELETE, DROP TABLE and ALTER TABLE ADD COLUMN.
//! `duckdb -readonly` also allows a read-only local ATTACH before verified
//! reads (Python asks; `rust/parity/audit_replay.py` reports it as "wider").

use std::collections::HashSet;
use std::ops::ControlFlow;

use sqlparser::ast::{
    AlterTable, AlterTableOperation, ArrayElemTypeDef, AssignmentTarget,
    AttachDuckDBDatabaseOption, ColumnDef, CopyOption, CopySource, CopyTarget, CreateTable,
    DataType, Delete, Expr, FromTable, Function, FunctionArgumentClause, FunctionArguments, Ident,
    Insert, LimitClause, ObjectName, ObjectNamePart, ObjectType, Query, Select, SetExpr, Statement,
    TableFactor, TableObject, TableWithJoins, TypedString, Update, Value, Visit, Visitor,
};
use sqlparser::dialect::{
    DuckDbDialect, GenericDialect, MsSqlDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect,
};
use sqlparser::keywords::{ALL_KEYWORDS, ALL_KEYWORDS_INDEX, Keyword, RESERVED_FOR_TABLE_ALIAS};
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

use super::functions::{FUNCTIONS, TYPES};

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

use Dialect::*;

impl Dialect {
    /// SQLGlot dialect name; unknown names fail like `get_or_raise`.
    pub fn parse(name: Option<&str>) -> Option<Self> {
        Some(match name {
            None => Base,
            Some("postgres") => Postgres,
            Some("mysql") => Mysql,
            Some("sqlite") => Sqlite,
            Some("tsql") => Tsql,
            Some("duckdb") => Duckdb,
            Some("athena") => Athena,
            Some(_) => return None,
        })
    }

    fn index(self) -> usize {
        self as usize
    }

    /// The single statement `sql` parses to, else `None`.
    fn parse_one(self, sql: &str) -> Option<Statement> {
        let mut statements = match self {
            Base | Athena => parse_with(&GenericDialect {}, sql, self),
            Postgres => parse_with(&PostgreSqlDialect {}, sql, self),
            Mysql => parse_with(&MySqlDialect {}, sql, self),
            Sqlite => parse_with(&SQLiteDialect {}, sql, self),
            Tsql => parse_with(&MsSqlDialect {}, sql, self),
            Duckdb => parse_with(&DuckDbDialect {}, sql, self),
        }?;
        if statements.len() != 1 {
            return None;
        }
        statements.pop()
    }
}

fn parse_with(
    dialect: &dyn sqlparser::dialect::Dialect,
    sql: &str,
    d: Dialect,
) -> Option<Vec<Statement>> {
    let tokens = Tokenizer::new(dialect, sql).tokenize().ok()?;
    Parser::new(dialect)
        .with_tokens(normalize(tokens, d))
        .parse_statements()
        .ok()
}

/// A `SELECT` at this nesting level before it closes.
fn select_follows(tokens: &[Token]) -> bool {
    let mut depth = 0usize;
    for token in tokens {
        match token {
            Token::LParen => depth += 1,
            Token::RParen if depth == 0 => return false,
            Token::RParen => depth -= 1,
            Token::SemiColon if depth == 0 => return false,
            _ if depth == 0 && is_word(Some(token), Keyword::SELECT) => return true,
            _ => {}
        }
    }
    false
}

fn is_word(token: Option<&Token>, keyword: Keyword) -> bool {
    matches!(token, Some(Token::Word(w)) if w.quote_style.is_none() && w.keyword == keyword)
}

/// Rewrite syntax `sqlparser` lacks, for verification only. A rewrite adds a
/// keyword, quotes a word that is no keyword in the dialect, swaps one
/// comparison operator for another or drops a sampling clause, so it never
/// hides an effect:
///
/// * PostgreSQL/DuckDB inet containment `<<=`/`>>=` becomes `<=`/`>=`;
/// * DuckDB `FILTER (cond)` after a call gets the optional `WHERE`;
/// * DuckDB words in [`DUCKDB_NON_KEYWORDS`] become quoted identifiers;
/// * DuckDB `(DESCRIBE t)` becomes `(SELECT * FROM t)`;
/// * DuckDB `USING SAMPLE n [ROWS | PERCENT | %]` is dropped;
/// * DuckDB `FROM t ...` without `SELECT` gets the implied `SELECT *`.
fn normalize(tokens: Vec<Token>, d: Dialect) -> Vec<Token> {
    let t: Vec<Token> = tokens
        .into_iter()
        .filter(|t| !matches!(t, Token::Whitespace(_)))
        .collect();
    let mut out: Vec<Token> = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        let token = &t[i];
        if matches!(d, Postgres | Duckdb)
            && matches!(token, Token::ShiftLeft | Token::ShiftRight)
            && t.get(i + 1) == Some(&Token::Eq)
        {
            out.push(if *token == Token::ShiftLeft {
                Token::LtEq
            } else {
                Token::GtEq
            });
            i += 2;
            continue;
        }
        if d == Duckdb {
            match token {
                Token::Word(w)
                    if w.quote_style.is_none()
                        && w.keyword == Keyword::FROM
                        && matches!(out.last(), None | Some(Token::LParen))
                        && !select_follows(&t[i + 1..]) =>
                {
                    out.extend([Token::make_keyword("SELECT"), Token::Mul]);
                }
                Token::Word(w)
                    if w.quote_style.is_none()
                        && DUCKDB_NON_KEYWORDS
                            .iter()
                            .any(|k| w.value.eq_ignore_ascii_case(k)) =>
                {
                    out.push(Token::make_word(&w.value, Some('"')));
                    i += 1;
                    continue;
                }
                Token::LParen
                    if is_word(out.last(), Keyword::FILTER)
                        && out.len() >= 2
                        && out[out.len() - 2] == Token::RParen
                        && !is_word(t.get(i + 1), Keyword::WHERE) =>
                {
                    out.extend([Token::LParen, Token::make_keyword("WHERE")]);
                    i += 1;
                    continue;
                }
                Token::LParen
                    if is_word(t.get(i + 1), Keyword::DESCRIBE)
                        && matches!(t.get(i + 2), Some(Token::Word(_)))
                        && t.get(i + 3) == Some(&Token::RParen) =>
                {
                    out.extend([
                        Token::LParen,
                        Token::make_keyword("SELECT"),
                        Token::Mul,
                        Token::make_keyword("FROM"),
                        t[i + 2].clone(),
                        Token::RParen,
                    ]);
                    i += 4;
                    continue;
                }
                Token::Word(w)
                    if w.quote_style.is_none()
                        && w.keyword == Keyword::USING
                        && is_word(t.get(i + 1), Keyword::SAMPLE)
                        && matches!(t.get(i + 2), Some(Token::Number(..))) =>
                {
                    i += 3;
                    if is_word(t.get(i), Keyword::ROWS)
                        || is_word(t.get(i), Keyword::PERCENT)
                        || t.get(i) == Some(&Token::Mod)
                    {
                        i += 1;
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(token.clone());
        i += 1;
    }
    out
}

/// DuckDB functions without side effects that SQLGlot does not know
/// (Python asks for them). Any argument count.
const DUCKDB_PURE_FUNCTIONS: [&str; 3] = ["hash", "url_decode", "url_encode"];

/// Words `sqlparser` treats as keywords that DuckDB syntax does not use
/// (`TOP` is T-SQL, `OPERATOR(...)` PostgreSQL), so in DuckDB they are
/// identifiers.
const DUCKDB_NON_KEYWORDS: [&str; 2] = ["operator", "top"];

fn function_known(name: &str, count: usize, d: Dialect) -> bool {
    let lower = name.to_ascii_lowercase();
    if d == Duckdb && DUCKDB_PURE_FUNCTIONS.contains(&lower.as_str()) {
        return true;
    }
    let mask = FUNCTIONS
        .binary_search_by(|(n, _)| n.cmp(&lower.as_str()))
        .map_or(0, |i| FUNCTIONS[i].1[d.index()]);
    match count {
        0..=4 => mask & (1 << count) != 0,
        // More than four arguments: only variadic functions (3 and 4 work).
        _ => mask & 0b11000 == 0b11000,
    }
}

/// A type SQLGlot knows in the dialect: the leading word of its rendering
/// (`DOUBLE` of `DOUBLE PRECISION`, `INT` of `INT[]`) is in [`TYPES`], and
/// so are element and field types. Anything else, including a quoted name,
/// is SQLGlot's `USERDEFINED`.
fn type_ok(t: &DataType, d: Dialect) -> bool {
    let word = t
        .to_string()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_ascii_uppercase();
    let known = TYPES
        .binary_search_by(|(n, _)| n.cmp(&word.as_str()))
        .is_ok_and(|i| TYPES[i].1 & (1 << d.index()) != 0);
    known
        && match t {
            DataType::Unspecified => false,
            DataType::Array(
                ArrayElemTypeDef::AngleBracket(t)
                | ArrayElemTypeDef::SquareBracket(t, _)
                | ArrayElemTypeDef::Parenthesis(t)
                | ArrayElemTypeDef::Qualified(t, _),
            ) => type_ok(t, d),
            DataType::Struct(fields, _) => fields
                .iter()
                .all(|f| f.options.is_none() && type_ok(&f.field_type, d)),
            DataType::Map(k, v, _) => type_ok(k, d) && type_ok(v, d),
            _ => true,
        }
}

/// Query bodies; `INSERT`/`UPDATE`/... bodies are statements, which the
/// walk reports as writes.
fn set_expr_ok(e: &SetExpr) -> bool {
    match e {
        SetExpr::SetOperation { left, right, .. } => set_expr_ok(left) && set_expr_ok(right),
        SetExpr::Table(_) => false,
        _ => true,
    }
}

/// Walk state. `Break` is a write; other unlisted nodes set `unverified`.
struct Check {
    d: Dialect,
    statements: usize,
    unverified: bool,
}

impl Check {
    fn require(&mut self, ok: bool) {
        self.unverified |= !ok;
    }

    fn function(&self, name: &ObjectName, count: usize) -> ControlFlow<()> {
        let known = match name.0.as_slice() {
            [ObjectNamePart::Identifier(ident)] => {
                ident.quote_style.is_none() && function_known(&ident.value, count, self.d)
            }
            _ => false,
        };
        if known {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    fn call(&mut self, f: &Function) -> ControlFlow<()> {
        self.require(!f.uses_odbc_syntax && matches!(f.parameters, FunctionArguments::None));
        let count = match &f.args {
            FunctionArguments::None => 0,
            FunctionArguments::Subquery(_) => {
                self.require(false);
                1
            }
            FunctionArguments::List(list) => {
                self.require(list.clauses.iter().all(|c| {
                    matches!(
                        c,
                        FunctionArgumentClause::OrderBy(_)
                            | FunctionArgumentClause::IgnoreOrRespectNulls(_)
                            | FunctionArgumentClause::Separator(_)
                    )
                }));
                list.args.len()
            }
        };
        self.function(&f.name, count)
    }
}

impl Visitor for Check {
    type Break = ();

    fn pre_visit_statement(&mut self, _: &Statement) -> ControlFlow<()> {
        self.statements += 1;
        if self.statements > 1 {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
        let limit_ok = match &q.limit_clause {
            Some(LimitClause::LimitOffset { limit_by, .. }) => limit_by.is_empty(),
            _ => true,
        };
        self.require(
            q.locks.is_empty()
                && q.for_clause.is_none()
                && q.settings.is_none()
                && q.format_clause.is_none()
                && q.pipe_operators.is_empty()
                && q.with
                    .as_ref()
                    .is_none_or(|w| w.cte_tables.iter().all(|c| c.from.is_none()))
                && q.order_by.as_ref().is_none_or(|o| o.interpolate.is_none())
                && limit_ok
                && set_expr_ok(&q.body),
        );
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, s: &Select) -> ControlFlow<()> {
        if s.into.is_some() {
            return ControlFlow::Break(());
        }
        self.require(
            s.optimizer_hints.is_empty()
                && s.select_modifiers.is_none()
                && s.lateral_views.is_empty()
                && s.prewhere.is_none()
                && s.connect_by.is_empty()
                && s.cluster_by.is_empty()
                && s.distribute_by.is_empty()
                && s.sort_by.is_empty()
                && s.value_table_mode.is_none(),
        );
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, t: &TableFactor) -> ControlFlow<()> {
        match t {
            TableFactor::Table {
                name,
                args,
                with_hints,
                version,
                partitions,
                json_path,
                index_hints,
                ..
            } => {
                self.require(
                    with_hints.is_empty()
                        && version.is_none()
                        && partitions.is_empty()
                        && json_path.is_none()
                        && index_hints.is_empty(),
                );
                if let Some(args) = args {
                    self.require(args.settings.is_none());
                    return self.function(name, args.args.len());
                }
            }
            TableFactor::Function { name, args, .. } => return self.function(name, args.len()),
            TableFactor::Derived { .. }
            | TableFactor::UNNEST { .. }
            | TableFactor::NestedJoin { .. } => {}
            _ => self.require(false),
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
        match e {
            Expr::Function(f) => return self.call(f),
            Expr::Cast {
                data_type, format, ..
            } => {
                if !type_ok(data_type, self.d) {
                    return ControlFlow::Break(());
                }
                self.require(format.is_none());
            }
            Expr::TypedString(TypedString { data_type, .. }) => {
                if !type_ok(data_type, self.d) {
                    return ControlFlow::Break(());
                }
            }
            Expr::Value(v) => self.require(!matches!(v.value, Value::Placeholder(_))),
            Expr::Identifier(_)
            | Expr::CompoundIdentifier(_)
            | Expr::CompoundFieldAccess { .. }
            | Expr::JsonAccess { .. }
            | Expr::IsFalse(_)
            | Expr::IsNotFalse(_)
            | Expr::IsTrue(_)
            | Expr::IsNotTrue(_)
            | Expr::IsNull(_)
            | Expr::IsNotNull(_)
            | Expr::IsUnknown(_)
            | Expr::IsNotUnknown(_)
            | Expr::IsDistinctFrom(..)
            | Expr::IsNotDistinctFrom(..)
            | Expr::InList { .. }
            | Expr::InSubquery { .. }
            | Expr::InUnnest { .. }
            | Expr::Between { .. }
            | Expr::BinaryOp { .. }
            | Expr::Like { .. }
            | Expr::ILike { .. }
            | Expr::SimilarTo { .. }
            | Expr::RLike { .. }
            | Expr::AnyOp { .. }
            | Expr::AllOp { .. }
            | Expr::UnaryOp { .. }
            | Expr::AtTimeZone { .. }
            | Expr::Extract { .. }
            | Expr::Ceil { .. }
            | Expr::Floor { .. }
            | Expr::Position { .. }
            | Expr::Substring { .. }
            | Expr::Trim { .. }
            | Expr::Overlay { .. }
            | Expr::Collate { .. }
            | Expr::Nested(_)
            | Expr::Case { .. }
            | Expr::Exists { .. }
            | Expr::Subquery(_)
            | Expr::GroupingSets(_)
            | Expr::Cube(_)
            | Expr::Rollup(_)
            | Expr::Tuple(_)
            | Expr::Struct { .. }
            | Expr::Dictionary(_)
            | Expr::Map(_)
            | Expr::Array(_)
            | Expr::Interval(_)
            | Expr::Wildcard(_)
            | Expr::QualifiedWildcard(..)
            | Expr::Lambda(_) => {}
            _ => self.require(false),
        }
        ControlFlow::Continue(())
    }
}

/// `Some(true)` verified, `Some(false)` a write, `None` unverified.
fn walk(statement: &Statement, d: Dialect) -> Option<bool> {
    let mut check = Check {
        d,
        statements: 0,
        unverified: false,
    };
    if statement.visit(&mut check).is_break() {
        return Some(false);
    }
    (!check.unverified).then_some(true)
}

/// Stand-in for `_verify_query_ast` after the lexical checks: `true` for one
/// verified read-only query, `false` for a write node (an unknown function),
/// `None` otherwise.
pub fn verify_query(sql: &str, d: Dialect) -> Option<bool> {
    // Python accepts a bare `SELECT`; sqlparser does not parse it.
    if sql
        .trim_matches(super::py_isspace)
        .eq_ignore_ascii_case("SELECT")
    {
        return Some(true);
    }
    let statement = d.parse_one(sql)?;
    if !matches!(statement, Statement::Query(_)) {
        return None;
    }
    walk(&statement, d)
}

/// Stand-in for the temp-table `sqlglot.parse_one` check; the caller has
/// matched the `CREATE TEMP TABLE name AS` prefix and verifies the query.
pub fn verify_temp_create(sql: &str, d: Dialect) -> bool {
    matches!(d.parse_one(sql), Some(Statement::CreateTable(ct)) if ct.temporary && ct.query.is_some())
}

/// An unquoted keyword that cannot be a table name, e.g. `SELECT`.
fn reserved(ident: &Ident) -> bool {
    let upper = ident.value.to_ascii_uppercase();
    ident.quote_style.is_none()
        && ALL_KEYWORDS
            .binary_search(&upper.as_str())
            .is_ok_and(|i| RESERVED_FOR_TABLE_ALIAS.contains(&ALL_KEYWORDS_INDEX[i]))
}

/// `_main_table_target`: `t` or `main.t`.
fn main_target(name: &ObjectName) -> bool {
    match name.0.as_slice() {
        [ObjectNamePart::Identifier(table)] => !reserved(table),
        [
            ObjectNamePart::Identifier(db),
            ObjectNamePart::Identifier(table),
        ] => db.value.eq_ignore_ascii_case("main") && !reserved(table),
        _ => false,
    }
}

fn join<T: std::fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn where_clause(selection: &Option<Expr>) -> String {
    selection
        .as_ref()
        .map(|s| format!(" WHERE {s}"))
        .unwrap_or_default()
}

fn column_ok(c: &ColumnDef) -> bool {
    c.options.is_empty() && type_ok(&c.data_type, Duckdb)
}

/// A main-database table without alias, arguments or joins.
fn plain_table(t: &TableWithJoins) -> Option<&ObjectName> {
    match &t.relation {
        TableFactor::Table { name, .. } if t.joins.is_empty() && main_target(name) => Some(name),
        _ => None,
    }
}

fn create_table(ct: &CreateTable) -> Option<String> {
    if !main_target(&ct.name) || !ct.constraints.is_empty() {
        return None;
    }
    let head = format!(
        "CREATE {}TABLE {}{}",
        if ct.or_replace { "OR REPLACE " } else { "" },
        if ct.if_not_exists {
            "IF NOT EXISTS "
        } else {
            ""
        },
        ct.name
    );
    match (&ct.query, ct.columns.as_slice()) {
        (Some(query), []) => Some(format!("{head} AS {query}")),
        (None, [_, ..]) if ct.columns.iter().all(column_ok) => {
            Some(format!("{head} ({})", join(&ct.columns)))
        }
        _ => None,
    }
}

fn insert(i: &Insert) -> Option<String> {
    let TableObject::TableName(name) = &i.table else {
        return None;
    };
    let source = i.source.as_ref()?;
    if !i.into || !main_target(name) {
        return None;
    }
    let columns = if i.columns.is_empty() {
        String::new()
    } else {
        format!(" ({})", join(&i.columns))
    };
    Some(format!("INSERT INTO {name}{columns} {source}"))
}

fn update(u: &Update) -> Option<String> {
    let name = plain_table(&u.table)?;
    let columns_only = u
        .assignments
        .iter()
        .all(|a| matches!(&a.target, AssignmentTarget::ColumnName(c) if c.0.len() == 1));
    if u.assignments.is_empty() || !columns_only {
        return None;
    }
    Some(format!(
        "UPDATE {name} SET {}{}",
        join(&u.assignments),
        where_clause(&u.selection)
    ))
}

fn delete(d: &Delete) -> Option<String> {
    let FromTable::WithFromKeyword(from) = &d.from else {
        return None;
    };
    let [table] = from.as_slice() else {
        return None;
    };
    let name = plain_table(table)?;
    Some(format!("DELETE FROM {name}{}", where_clause(&d.selection)))
}

fn alter_table(a: &AlterTable) -> Option<String> {
    let [
        AlterTableOperation::AddColumn {
            column_keyword,
            if_not_exists: false,
            column_def,
            column_position: None,
        },
    ] = a.operations.as_slice()
    else {
        return None;
    };
    if !main_target(&a.name) || !column_ok(column_def) {
        return None;
    }
    Some(format!(
        "ALTER TABLE {} ADD {}{column_def}",
        a.name,
        if *column_keyword { "COLUMN " } else { "" }
    ))
}

/// Stand-in for the SQLGlot part of `_verified_main_write`: one plain write
/// of the `root` kind to a main-database table whose expressions and source
/// query verify. The statement must render exactly as the canonical form
/// built from the checked parts, so no other clause is present.
pub fn verify_main_write(sql: &str, root: &str) -> bool {
    let Some(statement) = Duckdb.parse_one(sql) else {
        return false;
    };
    let canonical = match (root, &statement) {
        ("CREATE", Statement::CreateTable(ct)) => create_table(ct),
        ("INSERT", Statement::Insert(i)) => insert(i),
        ("UPDATE", Statement::Update(u)) => update(u),
        ("DELETE", Statement::Delete(d)) => delete(d),
        (
            "DROP",
            Statement::Drop {
                object_type: ObjectType::Table,
                if_exists,
                names,
                ..
            },
        ) => match names.as_slice() {
            [name] if main_target(name) => Some(format!(
                "DROP TABLE {}{name}",
                if *if_exists { "IF EXISTS " } else { "" }
            )),
            _ => None,
        },
        ("ALTER", Statement::AlterTable(a)) => alter_table(a),
        _ => None,
    };
    canonical.is_some_and(|c| c == statement.to_string()) && walk(&statement, Duckdb) == Some(true)
}

/// Stand-in for the SQLGlot part of `duckdb_copy_export_target`: the target
/// of `COPY (query) TO 'file'` with CSV options only.
pub fn copy_export_target(sql: &str) -> Option<String> {
    let statement = Duckdb.parse_one(sql)?;
    let Statement::Copy {
        source: CopySource::Query(_),
        to: true,
        target: CopyTarget::File { filename },
        options,
        legacy_options,
        values,
    } = &statement
    else {
        return None;
    };
    if !legacy_options.is_empty() || !values.is_empty() {
        return None;
    }
    let mut seen = HashSet::new();
    for option in options {
        let ok = match option {
            CopyOption::Format(format) => format.value.eq_ignore_ascii_case("csv"),
            CopyOption::Header(_)
            | CopyOption::Delimiter(_)
            | CopyOption::Quote(_)
            | CopyOption::Escape(_)
            | CopyOption::Null(_) => true,
            _ => false,
        };
        if !ok || !seen.insert(std::mem::discriminant(option)) {
            return None;
        }
    }
    if walk(&statement, Duckdb) != Some(true) {
        return None;
    }
    let target = filename.clone();
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

/// `ATTACH 'file' [AS alias] (READ_ONLY)` of a local database file. A path
/// with `:` (URL, `md:`, `s3:`) or a `TYPE` option asks.
pub fn verify_readonly_attach(sql: &str) -> bool {
    let Some(Statement::AttachDuckDBDatabase {
        database_path,
        attach_options,
        ..
    }) = Duckdb.parse_one(sql)
    else {
        return false;
    };
    database_path.quote_style == Some('\'')
        && !database_path.value.trim().is_empty()
        && !database_path.value.contains(':')
        && matches!(
            attach_options.as_slice(),
            [AttachDuckDBDatabaseOption::ReadOnly(None | Some(true))]
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_table_is_sorted_for_binary_search() {
        assert!(FUNCTIONS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(TYPES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn main_writes() {
        for (root, sql) in [
            ("CREATE", "CREATE TABLE t AS SELECT 1 AS a"),
            (
                "CREATE",
                "CREATE OR REPLACE TABLE main.t AS SELECT a FROM u",
            ),
            (
                "CREATE",
                "CREATE TABLE IF NOT EXISTS t (a INTEGER, b VARCHAR)",
            ),
            ("INSERT", "INSERT INTO t VALUES (1, 'x')"),
            ("INSERT", "INSERT INTO main.t (a, b) SELECT a, b FROM u"),
            ("UPDATE", "UPDATE t SET a = 1, b = lower(b) WHERE c > 2"),
            ("DELETE", "DELETE FROM t WHERE a IN (SELECT a FROM u)"),
            ("DROP", "DROP TABLE IF EXISTS t"),
            ("ALTER", "ALTER TABLE t ADD COLUMN c INTEGER"),
        ] {
            assert!(verify_main_write(sql, root), "{sql}");
        }
        for (root, sql) in [
            ("CREATE", "CREATE TABLE other.t AS SELECT 1"),
            ("CREATE", "CREATE TABLE t (a INTEGER PRIMARY KEY)"),
            ("CREATE", "CREATE TABLE t AS SELECT writefile('x', 'y')"),
            ("CREATE", "CREATE VIEW v AS SELECT 1"),
            ("INSERT", "INSERT INTO t VALUES (1) RETURNING *"),
            ("INSERT", "INSERT INTO t VALUES (1) ON CONFLICT DO NOTHING"),
            ("INSERT", "INSERT OR REPLACE INTO t VALUES (1)"),
            (
                "INSERT",
                "INSERT INTO t SELECT * FROM query('DROP TABLE u')",
            ),
            ("UPDATE", "UPDATE t SET a = 1 FROM u WHERE t.b = u.b"),
            ("UPDATE", "UPDATE other.t SET a = 1"),
            ("DELETE", "DELETE FROM t USING u WHERE t.a = u.a"),
            ("DELETE", "DELETE FROM SELECT"),
            ("DROP", "DROP TABLE t, u"),
            ("DROP", "DROP TABLE t CASCADE"),
            ("ALTER", "ALTER TABLE t DROP COLUMN c"),
            ("UPDATE", "INSERT INTO t VALUES (1)"),
        ] {
            assert!(!verify_main_write(sql, root), "{sql}");
        }
    }

    #[test]
    fn copy_exports() {
        assert_eq!(
            copy_export_target("COPY (SELECT 1) TO 'out.csv' (FORMAT CSV, HEADER true)").as_deref(),
            Some("out.csv")
        );
        for sql in [
            "COPY (SELECT 1) TO 'out.parquet' (FORMAT PARQUET)",
            "COPY (SELECT 1) TO 'out.csv' (FORMAT CSV, FORMAT CSV)",
            "COPY (SELECT writefile('x', 'y')) TO 'out.csv'",
            "COPY t TO 'out.csv'",
            "COPY (SELECT 1) TO '/dev/stdout'",
            "COPY (SELECT 1) TO 'a*.csv'",
            "COPY t FROM 'in.csv'",
        ] {
            assert_eq!(copy_export_target(sql), None, "{sql}");
        }
    }

    #[test]
    fn readonly_attaches() {
        for sql in [
            "ATTACH 'other.db' AS other (READ_ONLY)",
            "ATTACH DATABASE 'data/other.db' AS o (READ_ONLY true)",
            "ATTACH IF NOT EXISTS 'other.db' (READ_ONLY)",
        ] {
            assert!(verify_readonly_attach(sql), "{sql}");
        }
        for sql in [
            "ATTACH 'other.db' AS other",
            "ATTACH 'other.db' AS other (READ_ONLY false)",
            "ATTACH 'other.db' AS other (TYPE sqlite, READ_ONLY)",
            "ATTACH 'https://example.com/x.db' AS x (READ_ONLY)",
            "ATTACH 'md:my_db' AS x (READ_ONLY)",
            "ATTACH 's3://bucket/x.db' AS x (READ_ONLY)",
            "ATTACH '' AS x (READ_ONLY)",
            "ATTACH other AS x (READ_ONLY)",
            "DETACH other",
        ] {
            assert!(!verify_readonly_attach(sql), "{sql}");
        }
    }

    #[test]
    fn argument_counts() {
        assert!(function_known("LOWER", 1, Base));
        assert!(!function_known("lower", 2, Base));
        assert!(function_known("coalesce", 7, Base));
        assert!(!function_known("substr", 7, Base));
        assert!(function_known("url_decode", 1, Duckdb));
        assert!(!function_known("url_decode", 1, Postgres));
        assert!(!function_known("load_extension", 1, Sqlite));
    }
}
