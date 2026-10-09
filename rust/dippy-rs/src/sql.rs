//! Port of `dippy.core.sql` - NOT PORTED YET.
//!
//! Until ported, nothing is recognised as read-only (callers ask).

/// `is_readonly_sql`: `Some(true)` read-only, `Some(false)` write, `None` unknown.
pub fn is_readonly_sql(_sql: &str, _opts: &ReadonlyOptions) -> Option<bool> {
    None
}

/// Keyword arguments of Python `is_readonly_sql`.
#[derive(Debug, Clone, Default)]
pub struct ReadonlyOptions {
    pub extra_readonly: Vec<String>,
    pub extra_write: Vec<String>,
    pub allow_multiple: bool,
    pub allow_temp_tables: bool,
    /// Python default is `true`.
    pub bracket_identifiers: bool,
    pub dialect: Option<String>,
}
