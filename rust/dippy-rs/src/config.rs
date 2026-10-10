//! Port of `dippy.core.config` and `dippy.core.options` (the parts that
//! affect Bash command classification: parsing, scope loading, includes,
//! command/redirect rule matching).
//!
//! Struct layout mirrors the Python dataclasses. Settings that only drive
//! hooks, logging or execution are validated exactly like Python (so the
//! same lines are skipped or fatal) but not stored.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::bash::decode_literal_word;
use crate::fnmatch::fnmatchcase;
use crate::parser::tokenize;
use crate::paths;

pub type Flags = BTreeSet<String>;

thread_local! {
    static WARNINGS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Python's `logging.warning` while loading configs.
fn warn(message: String) {
    WARNINGS.with(|w| w.borrow_mut().push(message));
}

/// Drain the config warnings recorded on this thread, oldest first.
pub fn take_warnings() -> Vec<String> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

/// Print the drained warnings as Python's `logging.warning` does without
/// handlers (`basicConfig` fallback).
pub fn print_warnings() {
    for warning in take_warnings() {
        eprintln!("WARNING:root:{warning}");
    }
}

/// `validate_server` (execution.py): a safe SSH-config alias.
pub fn validate_server(server: &str) -> Result<(), String> {
    if SERVER_ALIAS_RE.is_match(server) && !server.contains('@') {
        Ok(())
    } else {
        Err(format!("invalid server alias: {}", py_repr(server)))
    }
}

/// Python `repr(str)` for messages.
fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

pub const PROJECT_CONFIG_NAME: &str = ".dippy";
pub const ENV_CONFIG: &str = "DIPPY_CONFIG";
pub const ENV_CONFIG_ONLY: &str = "DIPPY_CONFIG_ONLY";

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A single config rule with origin tracking.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rule {
    /// 'allow' | 'ask' | 'deny' | 'delegate'
    pub decision: String,
    pub pattern: String,
    pub message: Option<String>,
    pub source: Option<String>,
    pub scope: Option<String>,
    /// Pattern ended with `|` (exact match only).
    pub exact: bool,
    /// Option rules: items to match anywhere.
    pub items: Option<Vec<String>>,
    pub required_flags: Option<Flags>,
    pub negated_flags: Option<Flags>,
    /// Permitted optional switches/value globs; None keeps legacy matching.
    pub options: Option<Vec<(String, Option<String>)>>,
}

impl Rule {
    fn new(decision: &str, pattern: impl Into<String>) -> Self {
        Self {
            decision: decision.into(),
            pattern: pattern.into(),
            ..Self::default()
        }
    }
}

pub const DEFAULT_APPROVAL_WAIT_MESSAGE: &str =
    "Stop work and wait for the user unless you can continue safely without this command.";

/// Configuration for a wrapper command.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WrapperInfo {
    pub name: String,
    pub trigger: Option<String>,
    pub target_flag: Option<String>,
    pub context_flag: Option<String>,
    pub context_first: bool,
    pub script_stdin_marker: Option<String>,
    pub transparent: bool,
}

/// Parsed configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub rules: Vec<Rule>,
    pub redirect_rules: Vec<Rule>,
    /// `after` rules (PostToolUse feedback).
    pub after_rules: Vec<Rule>,
    pub mcp_rules: Vec<Rule>,
    pub after_mcp_rules: Vec<Rule>,
    pub edit_rules: Vec<Rule>,
    pub read_rules: Vec<Rule>,
    pub web_rules: Vec<Rule>,
    pub after_web_rules: Vec<Rule>,
    pub wrappers: BTreeMap<String, WrapperInfo>,
    /// Insertion-ordered (Python dict).
    pub aliases: Vec<(String, String)>,
    pub python_allow_modules: Vec<String>,
    pub python_deny_modules: Vec<String>,
    pub python_allow_symbols: Vec<String>,
    pub context_env: Vec<String>,
    pub servers: Vec<String>,
    pub configured_settings: BTreeSet<String>,
    pub path_rule_cwd: Option<PathBuf>,
    /// 'allow' | 'ask' | 'pass'
    pub default: String,
    pub final_path: Option<PathBuf>,
    pub notifier_command: Option<String>,
    pub notifier_include: Option<BTreeSet<String>>,
    /// External approval program (SSH_ASKPASS style).
    pub askpass: Option<PathBuf>,
    pub askpass_timeout: i64,
    pub approval_wait_message: String,
    /// 'ssh' | 'tmux' | 'herdr'
    pub run_on_server_backend: String,
    pub run_on_server_session: String,
    pub run_on_server_timeout: f64,
    pub run_on_server_poll_interval: f64,
    /// `None` for `none` or unset.
    pub run_on_server_ssh_config: Option<PathBuf>,
    /// `none` is kept as the string "none".
    pub run_on_server_ssh_auth_sock: Option<String>,
    /// Audit log (`set log`).
    pub log: Option<PathBuf>,
    pub log_full: bool,
    /// Days to keep rotated audit logs (0 disables rotation).
    pub log_rotate_max_days: i64,
    pub log_hook_approvals: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            redirect_rules: Vec::new(),
            after_rules: Vec::new(),
            mcp_rules: Vec::new(),
            after_mcp_rules: Vec::new(),
            edit_rules: Vec::new(),
            read_rules: Vec::new(),
            web_rules: Vec::new(),
            after_web_rules: Vec::new(),
            wrappers: BTreeMap::new(),
            aliases: Vec::new(),
            python_allow_modules: Vec::new(),
            python_deny_modules: Vec::new(),
            python_allow_symbols: Vec::new(),
            context_env: Vec::new(),
            servers: Vec::new(),
            configured_settings: BTreeSet::new(),
            path_rule_cwd: None,
            default: "ask".into(),
            final_path: None,
            notifier_command: None,
            notifier_include: None,
            askpass: None,
            askpass_timeout: 59,
            approval_wait_message: DEFAULT_APPROVAL_WAIT_MESSAGE.into(),
            run_on_server_backend: "ssh".into(),
            run_on_server_session: "dippy".into(),
            run_on_server_timeout: 300.0,
            run_on_server_poll_interval: 0.1,
            run_on_server_ssh_config: None,
            run_on_server_ssh_auth_sock: None,
            log: None,
            log_full: false,
            log_rotate_max_days: 30,
            log_hook_approvals: true,
        }
    }
}

/// Result of matching against config rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub decision: String,
    pub pattern: String,
    pub message: Option<String>,
    pub source: Option<String>,
    pub scope: Option<String>,
}

impl Match {
    fn from_rule(rule: &Rule) -> Self {
        Self {
            decision: rule.decision.clone(),
            pattern: rule.pattern.clone(),
            message: rule.message.clone(),
            source: rule.source.clone(),
            scope: rule.scope.clone(),
        }
    }
}

// === Python string helpers ===

/// `str.split(None, maxsplit)`.
fn py_split_max(s: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s.trim_start();
    while !rest.is_empty() {
        if out.len() == max {
            out.push(rest.to_string());
            break;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        out.push(rest[..end].to_string());
        rest = rest[end..].trim_start();
    }
    out
}

fn py_split(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// `_strip_quotes`.
pub fn strip_quotes(value: &str) -> &str {
    crate::parser::strip_quotes(value)
}

// === Options (dippy.core.options) ===

/// `pattern_words`: decode a simple rule pattern, keeping glob characters.
pub fn pattern_words(pattern: &str) -> Result<Vec<String>, String> {
    let raw_words = tokenize(pattern, true);
    let words: Vec<Option<String>> = raw_words
        .iter()
        .map(|w| decode_literal_word(w, false))
        .collect();
    if words.is_empty() || words.iter().any(Option::is_none) {
        return Err("requires a literal command pattern".into());
    }
    Ok(words.into_iter().flatten().collect())
}

static OPTION_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:-[A-Za-z0-9]|--[A-Za-z0-9][A-Za-z0-9_-]*)$").unwrap());

type Options = Vec<(String, Option<String>)>;

/// `extract_options`: one leading `[opts: ...]` block.
pub fn extract_options(pattern: &str) -> Result<(String, Option<Options>), String> {
    if !pattern.starts_with("[opts:") {
        return Ok((pattern.to_string(), None));
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut declarations: Vec<String> = Vec::new();
    let mut start = "[opts:".len();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut depth = 1;
    let mut remaining: Option<String> = None;
    let text = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    for i in "[opts:".len()..chars.len() {
        let c = chars[i];
        if escaped {
            escaped = false;
        } else if c == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '[' {
            depth += 1;
        } else if c == ']' {
            depth -= 1;
            if depth == 0 {
                declarations.push(text(start, i).trim().to_string());
                remaining = Some(text(i + 1, chars.len()).trim().to_string());
                break;
            }
        } else if c == ',' && depth == 1 {
            declarations.push(text(start, i).trim().to_string());
            start = i + 1;
        }
    }
    let Some(remaining) = remaining else {
        return Err("unterminated opts block".into());
    };
    if remaining.starts_with("[opts:") {
        return Err("only one opts block is permitted".into());
    }
    let mut options: Options = Vec::new();
    if declarations == [""] {
        return Ok((remaining, Some(options)));
    }
    for declaration in &declarations {
        let words = pattern_words(declaration)?;
        if words.len() != 1 {
            return Err("option declarations require one name or name=value".into());
        }
        let (name, value) = match words[0].split_once('=') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (words[0].clone(), None),
        };
        if !OPTION_NAME_RE.is_match(&name) {
            return Err("invalid option name".into());
        }
        if options.iter().any(|(n, _)| *n == name) {
            return Err("duplicate option declaration".into());
        }
        options.push((name, value));
    }
    Ok((remaining, Some(options)))
}

fn option_value<'a>(options: &'a Options, name: &str) -> Option<&'a Option<String>> {
    options.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// `positional_words`: remove declared options, checking their values.
pub fn positional_words(words: &[String], options: &Options) -> Option<Vec<String>> {
    if words.is_empty() {
        return None;
    }
    let mut positionals = vec![words[0].clone()];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut end_options = false;
    let mut i = 1;
    while i < words.len() {
        let word = &words[i];
        if end_options || word == "-" || !word.starts_with('-') {
            positionals.push(word.clone());
        } else if word == "--" {
            end_options = true;
        } else {
            let names: Vec<String>;
            let mut value: Option<String>;
            if word.starts_with("--") {
                match word.split_once('=') {
                    Some((n, v)) => {
                        names = vec![n.to_string()];
                        value = Some(v.to_string());
                    }
                    None => {
                        names = vec![word.clone()];
                        value = None;
                    }
                }
            } else {
                let mut ns = Vec::new();
                value = None;
                let chars: Vec<char> = word.chars().collect();
                for (j, ch) in chars.iter().enumerate().skip(1) {
                    let name = format!("-{ch}");
                    ns.push(name.clone());
                    let declared = option_value(options, &name)?;
                    if declared.is_some() {
                        let rest: String = chars[j + 1..].iter().collect();
                        value = if rest.is_empty() { None } else { Some(rest) };
                        break;
                    }
                }
                names = ns;
            }
            let last = names.last().cloned();
            for name in &names {
                let declared = option_value(options, name)?;
                if seen.contains(name) {
                    return None;
                }
                seen.insert(name.clone());
                match declared {
                    None => {
                        if value.is_some() && Some(name) == last.as_ref() {
                            return None;
                        }
                        continue;
                    }
                    Some(value_pattern) => {
                        if value.is_none() {
                            i += 1;
                            if i == words.len() {
                                return None;
                            }
                            value = Some(words[i].clone());
                        }
                        if !fnmatchcase(value.as_deref().unwrap_or(""), value_pattern) {
                            return None;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    Some(positionals)
}

// === Parsing ===

static MODULE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*$").unwrap());
static IDENTIFIER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap());
static SERVER_ALIAS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$").unwrap());

fn strip_comment(rest: &str) -> &str {
    match rest.find('#') {
        Some(i) => rest[..i].trim_end(),
        None => rest,
    }
}

fn parse_module_name(rest: &str) -> Result<String, String> {
    let rest = strip_comment(rest);
    if rest.is_empty() {
        return Err("requires a module name".into());
    }
    let parts = py_split(rest);
    if parts.len() != 1 {
        return Err(format!("requires exactly one module name, got: {rest:?}"));
    }
    if !MODULE_RE.is_match(&parts[0]) {
        return Err(format!("invalid Python module name: {:?}", parts[0]));
    }
    Ok(parts[0].clone())
}

fn parse_symbol_name(rest: &str) -> Result<String, String> {
    let rest = strip_comment(rest);
    if rest.is_empty() {
        return Err("requires a symbol name".into());
    }
    let parts = py_split(rest);
    if parts.len() != 1 {
        return Err(format!("requires exactly one symbol name, got: {rest:?}"));
    }
    let symbol = &parts[0];
    let (module, name) = match symbol.rfind('.') {
        Some(i) => (&symbol[..i], &symbol[i + 1..]),
        None => return Err(format!("invalid Python symbol name: {symbol:?}")),
    };
    if !MODULE_RE.is_match(module) || !IDENTIFIER_RE.is_match(name) {
        return Err(format!("invalid Python symbol name: {symbol:?}"));
    }
    Ok(symbol.clone())
}

/// `_extract_context_flags`: `[flag1,!flag2] pattern`.
fn extract_context_flags(s: &str) -> (String, Option<Flags>, Option<Flags>) {
    let s = s.trim();
    if s.starts_with("[opts:") || !s.starts_with('[') {
        return (s.to_string(), None, None);
    }
    let Some(end) = s.find(']') else {
        return (s.to_string(), None, None);
    };
    let flags_str = s[1..end].trim();
    let remaining = s[end + 1..].trim().to_string();
    if flags_str.is_empty() {
        return (remaining, None, None);
    }
    let mut required = Flags::new();
    let mut negated = Flags::new();
    for f in flags_str.split(',') {
        let f = f.trim();
        if f.is_empty() {
            continue;
        }
        if let Some(neg) = f.strip_prefix('!') {
            negated.insert(neg.to_string());
        } else {
            required.insert(f.to_string());
        }
    }
    (
        remaining,
        (!required.is_empty()).then_some(required),
        (!negated.is_empty()).then_some(negated),
    )
}

/// `_strip_exact_anchor`.
fn strip_exact_anchor(pattern: &str) -> (String, bool) {
    match pattern.strip_suffix('|') {
        Some(p) => (p.trim_end().to_string(), true),
        None => (pattern.to_string(), false),
    }
}

/// `_unescape`.
fn unescape(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && (chars[i + 1] == '"' || chars[i + 1] == '\\')
        {
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `_extract_message`: trailing `"message"` preceded by whitespace.
fn extract_message(s: &str) -> Result<(String, Option<String>), String> {
    let s = s.trim_end();
    if !s.ends_with('"') {
        return Ok((s.to_string(), None));
    }
    let chars: Vec<char> = s.chars().collect();
    let mut j = chars.len() as isize - 2;
    let mut num_bs = 0;
    while j >= 0 && chars[j as usize] == '\\' {
        num_bs += 1;
        j -= 1;
    }
    if num_bs % 2 == 1 {
        return Ok((s.to_string(), None));
    }
    let mut i = chars.len() as isize - 2;
    while i >= 0 {
        let iu = i as usize;
        if chars[iu] == '"' && (iu == 0 || chars[iu - 1].is_whitespace()) {
            let message = unescape(&chars[iu + 1..chars.len() - 1].iter().collect::<String>());
            let pattern: String = chars[..iu]
                .iter()
                .collect::<String>()
                .trim_end()
                .to_string();
            if pattern.is_empty() {
                return Err("pattern required before message".into());
            }
            return Ok((pattern, Some(message)));
        }
        i -= 1;
    }
    Ok((s.to_string(), None))
}

/// `_parse_option_rule`: `<prefix> <item1> <item2>...`.
fn parse_option_rule(decision: &str, rest: &str) -> Result<Rule, String> {
    let (pattern, message) = extract_message(rest)?;
    let mut parts = tokenize(&pattern, false);
    if parts.is_empty() {
        parts = py_split(&pattern);
    }
    if parts.is_empty() {
        return Err("option rule requires prefix and items".into());
    }
    let items: Vec<String> = parts[1..].to_vec();
    if items.is_empty() {
        return Err("option rule requires at least one item to match".into());
    }
    let mut rule = Rule::new(decision, parts[0].clone());
    rule.message = message;
    rule.items = Some(items);
    Ok(rule)
}

/// Settings collected by `_apply_setting` (decision-relevant values only).
#[derive(Default)]
struct Settings {
    names: BTreeSet<String>,
    default: Option<String>,
    final_path: Option<PathBuf>,
    context_env: Vec<String>,
    notifier_command: Option<String>,
    notifier_include: Option<BTreeSet<String>>,
    askpass: Option<PathBuf>,
    askpass_timeout: Option<i64>,
    approval_wait_message: Option<String>,
    run_on_server_backend: Option<String>,
    run_on_server_session: Option<String>,
    run_on_server_timeout: Option<f64>,
    run_on_server_poll_interval: Option<f64>,
    run_on_server_ssh_config: Option<PathBuf>,
    run_on_server_ssh_auth_sock: Option<String>,
    log: Option<PathBuf>,
    log_full: bool,
    log_rotate_max_days: Option<i64>,
    log_hook_approvals: Option<bool>,
}

/// `_local_profile_path`: relative paths resolve against the config file's
/// directory.
fn local_profile_path(value: &str, base: &Path) -> PathBuf {
    let path = PathBuf::from(paths::expanduser(value));
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    std::path::absolute(&path).unwrap_or(path)
}

/// Python `int(str)`.
fn py_int_ok(value: &str) -> bool {
    let v = value.trim();
    let v = v.strip_prefix(['+', '-']).unwrap_or(v);
    !v.is_empty()
        && !v.starts_with('_')
        && !v.ends_with('_')
        && !v.contains("__")
        && v.chars()
            .all(|c| c.is_ascii_digit() || c == '_' || c.is_numeric())
}

/// Python `float(str)`.
fn py_float(value: &str) -> Option<f64> {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    let body = lower.trim_start_matches(['+', '-']);
    if matches!(body, "inf" | "infinity" | "nan") {
        return lower.parse::<f64>().ok().or(Some(f64::NAN));
    }
    if v.contains("__") || v.starts_with('_') || v.ends_with('_') {
        return None;
    }
    let cleaned: String = v.chars().filter(|c| *c != '_').collect();
    if cleaned.is_empty() || cleaned.contains("inf") || cleaned.contains("nan") {
        return None;
    }
    cleaned.parse::<f64>().ok()
}

/// `_apply_setting`.
fn apply_setting(settings: &mut Settings, rest: &str, base: &Path) -> Result<(), String> {
    if rest.is_empty() {
        return Err("'set' requires a setting name".into());
    }
    let parts = py_split_max(rest, 1);
    let key = parts[0].to_lowercase();
    let value = parts.get(1).cloned();
    let key_n = key.replace('-', "_");
    let need =
        |what: &str| -> Result<String, String> { value.clone().ok_or_else(|| what.to_string()) };
    match key_n.as_str() {
        "log_full" => {
            if value.is_some() {
                return Err(format!("'{key}' takes no value"));
            }
            settings.log_full = true;
        }
        "log_hook_approvals" => {
            let v = need(&format!("'{key}' requires 'on' or 'off'"))?;
            settings.log_hook_approvals = match v.to_lowercase().as_str() {
                "on" => Some(true),
                "off" => Some(false),
                _ => return Err(format!("'{key}' must be 'on' or 'off', got '{v}'")),
            };
        }
        "default" => match value.as_deref() {
            Some(v @ ("allow" | "ask" | "pass")) => settings.default = Some(v.to_string()),
            other => {
                return Err(format!(
                    "'default' must be 'allow', 'ask' or 'pass', got '{other:?}'"
                ));
            }
        },
        "run_on_server_ssh_config" | "run_on_server_ssh_auth_sock" => {
            let v = value.clone().unwrap_or_default();
            if value.is_none() || strip_quotes(&v).trim().is_empty() {
                return Err(format!("'{key}' requires a path or none"));
            }
            let v = strip_quotes(&v);
            if v.contains(['\0', '\r', '\n']) {
                return Err(format!("'{key}' must be a single path"));
            }
            if key_n == "run_on_server_ssh_config" {
                settings.run_on_server_ssh_config =
                    (v != "none").then(|| local_profile_path(v, base));
            } else {
                settings.run_on_server_ssh_auth_sock = Some(if v == "none" {
                    v.to_string()
                } else {
                    paths::py_path_str(&local_profile_path(v, base))
                });
            }
        }
        "run_on_server_backend" => match value.as_deref() {
            Some(v @ ("ssh" | "tmux" | "herdr")) => {
                settings.run_on_server_backend = Some(v.to_string());
            }
            other => {
                return Err(format!(
                    "'run-on-server-backend' must be 'ssh', 'tmux' or 'herdr', got '{}'",
                    other.unwrap_or("None")
                ));
            }
        },
        "run_on_server_session" => {
            let v = need("'run-on-server-session' requires a name")?;
            if v.trim().is_empty() {
                return Err("'run-on-server-session' requires a name".into());
            }
            settings.run_on_server_session = Some(strip_quotes(&v).to_string());
        }
        "log" => {
            let v = need("'log' requires a path")?;
            settings.log = Some(PathBuf::from(paths::expanduser(&v)));
        }
        "final" => {
            let v = need("'final' requires a path")?;
            settings.final_path = Some(PathBuf::from(paths::expanduser(&v)));
        }
        "askpass" => {
            let v = need("'askpass' requires a path")?;
            settings.askpass = Some(PathBuf::from(paths::expanduser(&v)));
        }
        "approval_wait_message" => {
            let v = need("'approval-wait-message' requires a message")?;
            let message = strip_quotes(&v).trim();
            if message.is_empty() {
                return Err("'approval-wait-message' must not be empty".into());
            }
            settings.approval_wait_message = Some(message.to_string());
        }
        "log_rotate_max_days" | "askpass_timeout" => {
            let name = key_n.replace('_', "-");
            let v = need(&format!("'{name}' requires a number"))?;
            let invalid = || format!("'{name}' must be an integer, got '{v}'");
            if !py_int_ok(&v) {
                return Err(invalid());
            }
            // Python also accepts non-ASCII digits; those lines are skipped here.
            let n = v.trim().replace('_', "").parse::<i64>();
            let n = Some(n.map_err(|_| invalid())?);
            if key_n == "askpass_timeout" {
                settings.askpass_timeout = n;
            } else {
                settings.log_rotate_max_days = n;
            }
        }
        "run_on_server_timeout" | "run_on_server_poll_interval" => {
            let v = need(&format!("'{key}' requires a positive number"))?;
            let n = py_float(&v).ok_or_else(|| format!("'{key}' must be a number, got '{v}'"))?;
            if n <= 0.0 {
                return Err(format!("'{key}' must be positive"));
            }
            if key_n == "run_on_server_timeout" {
                settings.run_on_server_timeout = Some(n);
            } else {
                settings.run_on_server_poll_interval = Some(n);
            }
        }
        "notifier_command" => {
            let v = need("'notifier-command' requires a command string")?;
            settings.notifier_command = Some(strip_quotes(&v).to_string());
        }
        "notifier_include" => {
            let v = need("'notifier-include' requires a list of tools or commands")?;
            let items = strip_quotes(&v)
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            settings.notifier_include = Some(items);
        }
        "idle_notifier_command" | "deny_format" => {
            need("requires a value")?;
        }
        k if k.starts_with("deny_format_") => {
            need("requires a format template")?;
            settings.names.insert("deny_format_agents".into());
            return Ok(());
        }
        "context_env" => {
            let v = need("'context-env' requires an environment variable name")?;
            settings.context_env.push(strip_quotes(&v).to_string());
        }
        _ => return Err(format!("unknown setting '{key}'")),
    }
    settings.names.insert(key_n);
    Ok(())
}

fn expand_pattern_tildes(pattern: &str) -> String {
    py_split(pattern)
        .iter()
        .map(|t| expand_home_only(t))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Shared parsing of allow/ask/deny/delegate rule lines.
fn parse_rule(decision: &str, rest: &str, with_message: bool) -> Result<Rule, String> {
    if rest.is_empty() {
        return Err("requires a pattern".into());
    }
    let (pattern_part, flags, neg_flags) = extract_context_flags(rest);
    let (pattern_part, options) = extract_options(&pattern_part)?;
    if pattern_part.is_empty() {
        return Err("requires a pattern after flags".into());
    }
    let (pattern, message) = if with_message {
        extract_message(&pattern_part)?
    } else {
        (pattern_part, None)
    };
    let (pattern, exact) = strip_exact_anchor(&pattern);
    if options.is_some() {
        pattern_words(&pattern)?;
    }
    Ok(Rule {
        decision: decision.into(),
        pattern: if options.is_some() {
            pattern
        } else {
            expand_pattern_tildes(&pattern)
        },
        message,
        exact,
        required_flags: flags,
        negated_flags: neg_flags,
        options,
        ..Rule::default()
    })
}

/// `allow-/ask-/deny-` rule with context flags; only non-allow forms take a
/// message (edit, read, web).
fn parse_flagged_rule(directive: &str, rest: &str, expand_tildes: bool) -> Result<Rule, String> {
    let (pattern_part, flags, neg) = extract_context_flags(rest);
    let (pattern, message) = if directive.starts_with("allow-") {
        (pattern_part, None)
    } else {
        extract_message(&pattern_part)?
    };
    let decision = directive.split('-').next().unwrap_or("ask");
    let pattern = if expand_tildes {
        expand_pattern_tildes(&pattern)
    } else {
        pattern
    };
    let mut rule = Rule::new(decision, pattern);
    rule.message = message;
    rule.required_flags = flags;
    rule.negated_flags = neg;
    Ok(rule)
}

/// Rule with an optional trailing message and no flags (MCP, after-*).
fn parse_message_rule(decision: &str, rest: &str) -> Result<Rule, String> {
    if rest.is_empty() {
        return Err("requires a pattern".into());
    }
    let (pattern, message) = extract_message(rest)?;
    let mut rule = Rule::new(decision, pattern);
    rule.message = message;
    Ok(rule)
}

/// `parse_config`. Invalid lines are skipped (as in Python); invalid SSH
/// profile settings are fatal.
pub fn parse_config(text: &str, source: Option<&str>) -> Result<Config, ConfigError> {
    let mut cfg = Config::default();
    let mut settings = Settings::default();
    let prefix = source.map(|s| format!("{s}: ")).unwrap_or_default();
    // Base of relative SSH profile paths: the config file's directory.
    let base = match source.map(|s| Path::new(s).parent().unwrap_or(Path::new(s))) {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        Some(_) => PathBuf::from("."),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
    };
    for (idx, raw_line) in text.lines().enumerate() {
        let lineno = idx + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts = py_split_max(line, 1);
        let directive = parts[0].to_lowercase();
        let rest = parts
            .get(1)
            .map(|r| r.trim().to_string())
            .unwrap_or_default();
        let result: Result<(), String> = (|| {
            match directive.as_str() {
                "allow" => cfg.rules.push(parse_rule("allow", &rest, false)?),
                "ask" => cfg.rules.push(parse_rule("ask", &rest, true)?),
                "deny" => cfg.rules.push(parse_rule("deny", &rest, true)?),
                "delegate" => cfg.rules.push(parse_rule("delegate", &rest, false)?),
                "allow-redirect" | "ask-redirect" | "deny-redirect" => {
                    if rest.is_empty() {
                        return Err("requires a pattern".into());
                    }
                    let (pattern_part, flags, neg) = extract_context_flags(&rest);
                    let (pattern, message) = if directive != "allow-redirect" {
                        extract_message(&pattern_part)?
                    } else {
                        (pattern_part, None)
                    };
                    let decision = directive.split('-').next().unwrap_or("ask");
                    let mut rule = Rule::new(decision, expand_pattern_tildes(&pattern));
                    rule.message = message;
                    rule.required_flags = flags;
                    rule.negated_flags = neg;
                    cfg.redirect_rules.push(rule);
                }
                "after" => {
                    if rest.is_empty() {
                        return Err("requires a pattern".into());
                    }
                    let (pattern, message) = extract_message(&rest)?;
                    let mut rule = Rule::new("after", pattern);
                    rule.message = message;
                    cfg.after_rules.push(rule);
                }
                "ask-mcp" => cfg.mcp_rules.push(parse_message_rule("ask", &rest)?),
                "deny-mcp" => cfg.mcp_rules.push(parse_message_rule("deny", &rest)?),
                "after-mcp" => cfg
                    .after_mcp_rules
                    .push(parse_message_rule("after", &rest)?),
                "after-web" => cfg
                    .after_web_rules
                    .push(parse_message_rule("after", &rest)?),
                "allow-mcp" => {
                    if rest.is_empty() {
                        return Err("requires a pattern".into());
                    }
                    cfg.mcp_rules.push(Rule::new("allow", rest.clone()));
                }
                "allow-opt" | "ask-opt" | "deny-opt" => {
                    if rest.is_empty() {
                        return Err("requires a prefix and items".into());
                    }
                    let decision = directive.split('-').next().unwrap_or("ask");
                    cfg.rules.push(parse_option_rule(decision, &rest)?);
                }
                "alias" => {
                    let p = py_split(&rest);
                    if p.len() != 2 {
                        return Err("requires exactly two arguments: source target".into());
                    }
                    let source = expand_pattern_tildes(&p[0]);
                    match cfg.aliases.iter_mut().find(|(s, _)| *s == source) {
                        Some(entry) => {
                            warn(format!(
                                "{prefix}line {lineno}: alias '{}' redefined, overwriting",
                                p[0]
                            ));
                            entry.1 = p[1].clone()
                        }
                        None => cfg.aliases.push((source, p[1].clone())),
                    }
                }
                "wrapper" => {
                    if rest.is_empty() {
                        return Err("requires a command name".into());
                    }
                    let p = py_split(&rest);
                    let name = p[0].clone();
                    if name.starts_with('-') {
                        return Err(format!("wrapper name cannot start with '-': {name}"));
                    }
                    if cfg.wrappers.contains_key(&name) {
                        warn(format!(
                            "{prefix}line {lineno}: duplicate wrapper definition: {name}"
                        ));
                    }
                    let mut info = WrapperInfo {
                        name: name.clone(),
                        ..WrapperInfo::default()
                    };
                    let mut new_syntax = false;
                    let mut i = 1;
                    while i < p.len() {
                        let has_next = i + 1 < p.len();
                        match p[i].as_str() {
                            "--transparent" => {
                                new_syntax = true;
                                info.transparent = true;
                                i += 1;
                            }
                            "--cmd" if has_next => {
                                new_syntax = true;
                                info.trigger = Some(p[i + 1].clone());
                                i += 2;
                            }
                            "--flag" if has_next => {
                                new_syntax = true;
                                info.target_flag = Some(p[i + 1].clone());
                                i += 2;
                            }
                            "--context" if has_next => {
                                new_syntax = true;
                                info.context_flag = Some(p[i + 1].clone());
                                i += 2;
                            }
                            "--context-first" => {
                                new_syntax = true;
                                info.context_first = true;
                                i += 1;
                            }
                            "--script-stdin" if has_next => {
                                new_syntax = true;
                                info.script_stdin_marker = Some(p[i + 1].clone());
                                i += 2;
                            }
                            _ => i += 1,
                        }
                    }
                    if !new_syntax && p.len() >= 2 {
                        let mut j = 1;
                        if j < p.len() && !p[j].starts_with('-') {
                            info.trigger = Some(p[j].clone());
                            j += 1;
                        }
                        if j < p.len() && p[j].starts_with('-') {
                            info.target_flag = Some(p[j].clone());
                        }
                    }
                    if !new_syntax {
                        info.context_first = true;
                    }
                    cfg.wrappers.insert(name, info);
                }
                "allow-edit" | "ask-edit" | "deny-edit" | "allow-read" | "ask-read"
                | "deny-read" => {
                    if rest.is_empty() {
                        return Err("requires a pattern".into());
                    }
                    let rule = parse_flagged_rule(&directive, &rest, true)?;
                    if directive.ends_with("-edit") {
                        cfg.edit_rules.push(rule);
                    } else {
                        cfg.read_rules.push(rule);
                    }
                }
                "allow-web" | "ask-web" | "deny-web" => {
                    // Bare `allow-web` approves every query.
                    let rest = if rest.is_empty() {
                        if directive != "allow-web" {
                            return Err("requires a pattern".into());
                        }
                        "*".to_string()
                    } else {
                        rest.clone()
                    };
                    cfg.web_rules
                        .push(parse_flagged_rule(&directive, &rest, false)?);
                }
                "set" => apply_setting(&mut settings, &rest, &base)?,
                "server" => {
                    if rest.is_empty() || py_split(&rest).len() != 1 {
                        return Err("'server' requires exactly one SSH alias".into());
                    }
                    if !SERVER_ALIAS_RE.is_match(&rest) || rest.contains('@') {
                        return Err(format!("invalid server alias: {rest:?}"));
                    }
                    if !cfg.servers.contains(&rest) {
                        cfg.servers.push(rest.clone());
                    }
                }
                "python-allow-module" => cfg.python_allow_modules.push(parse_module_name(&rest)?),
                "python-deny-module" => cfg.python_deny_modules.push(parse_module_name(&rest)?),
                "python-allow-symbol" => cfg.python_allow_symbols.push(parse_symbol_name(&rest)?),
                _ => return Err(format!("unknown directive '{directive}'")),
            }
            Ok(())
        })();
        if let Err(e) = result {
            if directive == "set"
                && rest
                    .to_lowercase()
                    .replace('_', "-")
                    .starts_with("run-on-server-ssh-")
            {
                return Err(ConfigError(format!(
                    "{prefix}line {lineno}: invalid SSH profile: {e}"
                )));
            }
            warn(format!("{prefix}line {lineno}: {e} (skipped)"));
        }
    }
    cfg.default = settings.default.unwrap_or_else(|| "ask".into());
    cfg.final_path = settings.final_path;
    cfg.context_env = settings.context_env;
    cfg.notifier_command = settings.notifier_command;
    cfg.notifier_include = settings.notifier_include;
    cfg.askpass = settings.askpass;
    cfg.askpass_timeout = settings.askpass_timeout.unwrap_or(59);
    let d = Config::default();
    cfg.approval_wait_message = settings
        .approval_wait_message
        .unwrap_or(d.approval_wait_message);
    cfg.run_on_server_backend = settings
        .run_on_server_backend
        .unwrap_or(d.run_on_server_backend);
    cfg.run_on_server_session = settings
        .run_on_server_session
        .unwrap_or(d.run_on_server_session);
    cfg.run_on_server_timeout = settings
        .run_on_server_timeout
        .unwrap_or(d.run_on_server_timeout);
    cfg.run_on_server_poll_interval = settings
        .run_on_server_poll_interval
        .unwrap_or(d.run_on_server_poll_interval);
    cfg.run_on_server_ssh_config = settings.run_on_server_ssh_config;
    cfg.run_on_server_ssh_auth_sock = settings.run_on_server_ssh_auth_sock;
    cfg.log = settings.log;
    cfg.log_full = settings.log_full;
    cfg.log_rotate_max_days = settings.log_rotate_max_days.unwrap_or(30);
    cfg.log_hook_approvals = settings.log_hook_approvals.unwrap_or(true);
    cfg.configured_settings = settings.names;
    Ok(cfg)
}

// === Loading ===

fn merge_configs(base: Config, overlay: Config) -> Config {
    let overlay_settings = overlay.configured_settings.clone();
    let set = |name: &str| overlay_settings.contains(name);
    let mut aliases = base.aliases.clone();
    for (s, t) in overlay.aliases {
        match aliases.iter_mut().find(|(x, _)| *x == s) {
            Some(e) => e.1 = t,
            None => aliases.push((s, t)),
        }
    }
    let mut wrappers = base.wrappers.clone();
    wrappers.extend(overlay.wrappers);
    let mut servers = base.servers.clone();
    for s in overlay.servers {
        if !base.servers.contains(&s) {
            servers.push(s);
        }
    }
    Config {
        rules: [base.rules, overlay.rules].concat(),
        redirect_rules: [base.redirect_rules, overlay.redirect_rules].concat(),
        after_rules: [base.after_rules, overlay.after_rules].concat(),
        mcp_rules: [base.mcp_rules, overlay.mcp_rules].concat(),
        after_mcp_rules: [base.after_mcp_rules, overlay.after_mcp_rules].concat(),
        edit_rules: [base.edit_rules, overlay.edit_rules].concat(),
        read_rules: [base.read_rules, overlay.read_rules].concat(),
        web_rules: [base.web_rules, overlay.web_rules].concat(),
        after_web_rules: [base.after_web_rules, overlay.after_web_rules].concat(),
        wrappers,
        aliases,
        python_allow_modules: [base.python_allow_modules, overlay.python_allow_modules].concat(),
        python_deny_modules: [base.python_deny_modules, overlay.python_deny_modules].concat(),
        python_allow_symbols: [base.python_allow_symbols, overlay.python_allow_symbols].concat(),
        context_env: [base.context_env, overlay.context_env].concat(),
        servers,
        configured_settings: base
            .configured_settings
            .union(&overlay.configured_settings)
            .cloned()
            .collect(),
        path_rule_cwd: base.path_rule_cwd,
        // Python compares the value to the default here.
        default: if overlay.default != "ask" {
            overlay.default
        } else {
            base.default
        },
        final_path: overlay.final_path.or(base.final_path),
        notifier_command: overlay.notifier_command.or(base.notifier_command),
        notifier_include: overlay.notifier_include.or(base.notifier_include),
        askpass: overlay.askpass.or(base.askpass),
        askpass_timeout: if overlay.configured_settings.contains("askpass_timeout") {
            overlay.askpass_timeout
        } else {
            base.askpass_timeout
        },
        approval_wait_message: if set("approval_wait_message") {
            overlay.approval_wait_message
        } else {
            base.approval_wait_message
        },
        run_on_server_backend: if set("run_on_server_backend") {
            overlay.run_on_server_backend
        } else {
            base.run_on_server_backend
        },
        run_on_server_session: if set("run_on_server_session") {
            overlay.run_on_server_session
        } else {
            base.run_on_server_session
        },
        run_on_server_timeout: if set("run_on_server_timeout") {
            overlay.run_on_server_timeout
        } else {
            base.run_on_server_timeout
        },
        run_on_server_poll_interval: if set("run_on_server_poll_interval") {
            overlay.run_on_server_poll_interval
        } else {
            base.run_on_server_poll_interval
        },
        run_on_server_ssh_config: if set("run_on_server_ssh_config") {
            overlay.run_on_server_ssh_config
        } else {
            base.run_on_server_ssh_config
        },
        run_on_server_ssh_auth_sock: if set("run_on_server_ssh_auth_sock") {
            overlay.run_on_server_ssh_auth_sock
        } else {
            base.run_on_server_ssh_auth_sock
        },
        log: overlay.log.or(base.log),
        log_full: overlay.log_full || base.log_full,
        // Python compares these two to their defaults, so a higher scope
        // cannot restore the default there.
        log_rotate_max_days: if overlay.configured_settings.contains("log_rotate_max_days") {
            overlay.log_rotate_max_days
        } else {
            base.log_rotate_max_days
        },
        log_hook_approvals: if overlay.configured_settings.contains("log_hook_approvals") {
            overlay.log_hook_approvals
        } else {
            base.log_hook_approvals
        },
    }
}

pub(crate) fn tag_rules(mut config: Config, source: &str, scope: &str) -> Config {
    for r in config
        .rules
        .iter_mut()
        .chain(config.redirect_rules.iter_mut())
        .chain(config.after_rules.iter_mut())
        .chain(config.mcp_rules.iter_mut())
        .chain(config.after_mcp_rules.iter_mut())
        .chain(config.edit_rules.iter_mut())
        .chain(config.read_rules.iter_mut())
        .chain(config.web_rules.iter_mut())
        .chain(config.after_web_rules.iter_mut())
    {
        r.source = Some(source.into());
        r.scope = Some(scope.into());
    }
    config
}

/// `glob.glob` (non-recursive): hidden names need a leading `.` in the pattern.
fn glob_paths(pattern: &str) -> Vec<String> {
    let has_magic = |s: &str| s.contains(['*', '?', '[']);
    if !has_magic(pattern) {
        return if Path::new(pattern).exists() || std::fs::symlink_metadata(pattern).is_ok() {
            vec![pattern.to_string()]
        } else {
            Vec::new()
        };
    }
    let absolute = pattern.starts_with('/');
    let comps: Vec<&str> = pattern.split('/').filter(|c| !c.is_empty()).collect();
    let mut current: Vec<String> = vec![if absolute { "/".into() } else { String::new() }];
    for (i, comp) in comps.iter().enumerate() {
        let last = i + 1 == comps.len();
        let mut next = Vec::new();
        for base in &current {
            let join = |name: &str| {
                if base.is_empty() {
                    name.to_string()
                } else if base.ends_with('/') {
                    format!("{base}{name}")
                } else {
                    format!("{base}/{name}")
                }
            };
            if has_magic(comp) {
                let dir = if base.is_empty() {
                    ".".to_string()
                } else {
                    base.clone()
                };
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                let mut names: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| comp.starts_with('.') || !n.starts_with('.'))
                    .filter(|n| fnmatchcase(n, comp))
                    .collect();
                names.sort();
                for n in names {
                    let p = join(&n);
                    if last || Path::new(&p).is_dir() {
                        next.push(p);
                    }
                }
            } else {
                let p = join(comp);
                if (last && (Path::new(&p).exists() || std::fs::symlink_metadata(&p).is_ok()))
                    || (!last && Path::new(&p).is_dir())
                {
                    next.push(p);
                }
            }
        }
        current = next;
    }
    current
}

/// `_expand_includes` (the SSH-profile path rewrite does not affect
/// classification and is omitted).
fn expand_includes(
    text: &str,
    base_dir: &Path,
    current_file: &Path,
    included: &mut BTreeSet<PathBuf>,
) -> Result<String, ConfigError> {
    included.insert(paths::resolve(current_file));
    let mut out: Vec<String> = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        let stripped = line.trim();
        if !stripped.starts_with("include") {
            out.push(line.to_string());
            continue;
        }
        let after: Vec<char> = stripped.chars().skip(7).collect();
        if !after.is_empty() && !after[0].is_whitespace() {
            out.push(line.to_string());
            continue;
        }
        let pattern = after.iter().collect::<String>().trim().to_string();
        let file = current_file.display();
        if pattern.is_empty() {
            warn(format!("{file}:{lineno}: empty include pattern (skipped)"));
            continue;
        }
        let mut pattern_path = PathBuf::from(paths::expanduser(&pattern));
        if !pattern_path.is_absolute() {
            pattern_path = base_dir.join(pattern_path);
        }
        let mut matches = glob_paths(&pattern_path.to_string_lossy());
        if matches.is_empty() {
            warn(format!(
                "{file}:{lineno}: no files match '{pattern}' (skipped)"
            ));
        }
        matches.sort();
        for m in matches {
            let match_path = paths::resolve(Path::new(&m));
            if included.contains(&match_path) {
                return Err(ConfigError(format!(
                    "circular include: {} -> {}",
                    current_file.display(),
                    match_path.display()
                )));
            }
            let included_text = std::fs::read_to_string(&match_path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    ConfigError(format!(
                        "permission denied reading included file: {}",
                        match_path.display()
                    ))
                } else {
                    ConfigError(format!(
                        "cannot read included file {}: {e}",
                        match_path.display()
                    ))
                }
            })?;
            let parent = match_path.parent().unwrap_or(Path::new("/")).to_path_buf();
            let expanded = expand_includes(&included_text, &parent, &match_path, included)?;
            out.push(format!("# included from: {}", match_path.display()));
            out.push(expanded);
        }
    }
    Ok(out.join("\n"))
}

fn load_config_file(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            ConfigError(format!(
                "permission denied reading config: {}",
                path.display()
            ))
        } else {
            ConfigError(format!("cannot read config {}: {e}", path.display()))
        }
    })?;
    let mut included = BTreeSet::new();
    let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let text = expand_includes(&text, &parent, path, &mut included)?;
    parse_config(&text, Some(&path.to_string_lossy()))
}

fn find_project_config(cwd: &Path) -> Option<PathBuf> {
    let mut current = paths::resolve(cwd);
    loop {
        let candidate = current.join(PROJECT_CONFIG_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        let parent = current.parent()?.to_path_buf();
        if parent == current {
            return None;
        }
        current = parent;
    }
}

pub fn user_config_path() -> PathBuf {
    paths::home().join(".dippy").join("config")
}

fn load_normal_config(cwd: &Path, config_path: Option<&str>) -> Result<Config, ConfigError> {
    let mut config = Config::default();
    let user = user_config_path();
    if user.is_file() {
        let c = tag_rules(load_config_file(&user)?, &user.to_string_lossy(), "user");
        config = merge_configs(config, c);
    }
    if let Some(project) = find_project_config(cwd) {
        let c = tag_rules(
            load_config_file(&project)?,
            &project.to_string_lossy(),
            "project",
        );
        config = merge_configs(config, c);
    }
    let env_override = std::env::var(ENV_CONFIG).ok().filter(|s| !s.is_empty());
    let override_path = config.clone_override(config_path, env_override.as_deref());
    if let Some(p) = override_path {
        let path = PathBuf::from(paths::expanduser(&p));
        if path.is_file() {
            let c = tag_rules(load_config_file(&path)?, &path.to_string_lossy(), "env");
            config = merge_configs(config, c);
        } else if config_path.is_some_and(|c| !c.is_empty()) {
            return Err(ConfigError(format!(
                "config file not found: {}",
                path.display()
            )));
        }
    }
    Ok(config)
}

impl Config {
    fn clone_override(&self, config_path: Option<&str>, env: Option<&str>) -> Option<String> {
        match config_path {
            Some(p) if !p.is_empty() => Some(p.to_string()),
            _ => env.map(str::to_string),
        }
    }
}

/// `load_config`: user, project and override scopes, or one exclusive file.
pub fn load_config(
    cwd: &Path,
    config_path: Option<&str>,
    config_only_path: Option<&str>,
) -> Result<Config, ConfigError> {
    let exclusive = match config_only_path {
        Some(p) => Some(p.to_string()),
        None => std::env::var(ENV_CONFIG_ONLY).ok(),
    };
    let mut config = match exclusive {
        Some(p) => {
            let path = PathBuf::from(paths::expanduser(&p));
            if !path.is_file() {
                return Err(ConfigError(format!(
                    "config file not found: {}",
                    path.display()
                )));
            }
            tag_rules(load_config_file(&path)?, &path.to_string_lossy(), "env")
        }
        None => load_normal_config(cwd, config_path)?,
    };
    if let Some(final_path) = config.final_path.clone() {
        if final_path.is_file() {
            let c = tag_rules(
                load_config_file(&final_path)?,
                &final_path.to_string_lossy(),
                "final",
            );
            config = merge_configs(config, c);
        } else {
            warn(format!("Final config not found: {}", final_path.display()));
        }
    }
    rotate_logs_on(&config, chrono::Local::now().date_naive());
    Ok(config)
}

/// `_rotate_logs`: on the first load of a day (local time) rename the audit
/// log to `audit-<yesterday>.log` and delete rotations older than
/// `log-rotate-max-days`. File errors are ignored (Python raises).
fn rotate_logs_on(config: &Config, today: chrono::NaiveDate) {
    let Some(log) = &config.log else { return };
    if config.log_rotate_max_days <= 0 {
        return;
    }
    let dir = log
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let yesterday = today - chrono::Days::new(1);
    let rotated = dir.join(format!("audit-{}.log", yesterday.format("%Y-%m-%d")));
    if rotated.exists() {
        return;
    }
    if log.exists() {
        let _ = std::fs::rename(log, &rotated);
    }
    let cutoff = (today - chrono::Days::new(config.log_rotate_max_days as u64))
        .format("%Y-%m-%d")
        .to_string();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name
            .strip_suffix(".log")
            .filter(|_| name.starts_with("audit-"))
        else {
            continue;
        };
        // Python: "-".join(stem.split("-")[1:4]) compared as a string.
        let parts: Vec<&str> = stem.split('-').collect();
        if parts.len() >= 4 && parts[1..4].join("-") < cutoff {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// `env_context_flags`: `$NAME=value` for each watched, non-empty variable.
pub fn env_context_flags(config: &Config) -> Flags {
    config
        .context_env
        .iter()
        .filter_map(|name| {
            let value = std::env::var(name).ok().filter(|v| !v.is_empty())?;
            Some(format!("${name}={value}"))
        })
        .collect()
}

// === Path classification & expansion ===

#[derive(PartialEq)]
enum TokenKind {
    Url,
    Variable,
    Absolute,
    Home,
    UserHome,
    Relative,
    Bare,
}

fn classify_token(token: &str) -> TokenKind {
    if token.contains("://") {
        TokenKind::Url
    } else if token.starts_with('$') {
        TokenKind::Variable
    } else if token.starts_with('/') {
        TokenKind::Absolute
    } else if token == "~" || token.starts_with("~/") {
        TokenKind::Home
    } else if token.starts_with('~') {
        TokenKind::UserHome
    } else if token == "."
        || token == ".."
        || token.starts_with("./")
        || token.starts_with("../")
        || token.contains('/')
    {
        TokenKind::Relative
    } else {
        TokenKind::Bare
    }
}

/// `_collapse_path`.
pub fn collapse_path(path: &str) -> String {
    let normalized = paths::normpath(path);
    if normalized.starts_with("//") {
        format!("/{}", normalized.trim_start_matches('/'))
    } else {
        normalized
    }
}

fn home_str() -> String {
    paths::pathlib_str(&paths::home().to_string_lossy())
}

fn resolve_str(cwd: &Path, token: &str) -> String {
    paths::resolve(&cwd.join(token))
        .to_string_lossy()
        .into_owned()
}

fn expand_token(token: &str, cwd: &Path, force_path: bool) -> String {
    match classify_token(token) {
        TokenKind::Url | TokenKind::Variable | TokenKind::UserHome => token.to_string(),
        TokenKind::Absolute => collapse_path(token),
        TokenKind::Home => {
            if token.chars().count() > 1 {
                collapse_path(&format!("{}{}", home_str(), &token[1..]))
            } else {
                home_str()
            }
        }
        TokenKind::Relative => resolve_str(cwd, token),
        TokenKind::Bare => {
            if force_path {
                resolve_str(cwd, token)
            } else {
                token.to_string()
            }
        }
    }
}

fn expand_home_only(token: &str) -> String {
    if classify_token(token) == TokenKind::Home {
        if token.chars().count() > 1 {
            format!("{}{}", home_str(), &token[1..])
        } else {
            home_str()
        }
    } else {
        token.to_string()
    }
}

fn normalize_token(token: &str, cwd: &Path) -> String {
    expand_token(token, cwd, false)
}

fn normalize_words(words: &[String], cwd: &Path) -> String {
    words
        .iter()
        .map(|w| normalize_token(w, cwd))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_pattern(pattern: &str, cwd: &Path) -> String {
    py_split(pattern)
        .iter()
        .map(|t| normalize_token(t, cwd))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_path(path: &str, cwd: &Path) -> String {
    expand_token(path.trim_end_matches('/'), cwd, true)
}

fn path_components(path: &str) -> Vec<&str> {
    let mut parts = path.split('/');
    let first = parts.next().unwrap_or("");
    std::iter::once(first)
        .chain(parts.filter(|p| !p.is_empty()))
        .collect()
}

fn skip_empty_doublestars(states: &BTreeSet<usize>, pattern: &[String]) -> BTreeSet<usize> {
    let mut result = states.clone();
    for &start in states {
        let mut i = start;
        while i < pattern.len() && pattern[i] == "**" {
            i += 1;
            result.insert(i);
        }
    }
    result
}

static MULTI_STAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*{2,}").unwrap());

/// `_glob_match`: component-wise path glob.
pub fn glob_match(text: &str, pattern: &str) -> bool {
    let mut parts: Vec<String> = path_components(pattern)
        .into_iter()
        .map(|p| {
            if p == "**" {
                p.to_string()
            } else {
                MULTI_STAR.replace_all(p, "*").into_owned()
            }
        })
        .collect();
    if parts.last().map(String::as_str) == Some("**") {
        parts.pop();
        parts.push("*".into());
        parts.push("**".into());
    }
    let mut states = skip_empty_doublestars(&BTreeSet::from([0]), &parts);
    for component in path_components(text) {
        let next: BTreeSet<usize> = states
            .iter()
            .filter(|&&i| {
                i < parts.len() && (parts[i] == "**" || fnmatchcase(component, &parts[i]))
            })
            .map(|&i| if parts[i] == "**" { i } else { i + 1 })
            .collect();
        states = skip_empty_doublestars(&next, &parts);
        if states.is_empty() {
            return false;
        }
    }
    states.contains(&parts.len())
}

fn match_option_rule(rule: &Rule, words: &[String]) -> bool {
    let Some(items) = &rule.items else {
        return false;
    };
    if items.is_empty() {
        return false;
    }
    let prefix_words = py_split(&rule.pattern);
    if words.len() < prefix_words.len() {
        return false;
    }
    if prefix_words.iter().zip(words).any(|(p, w)| p != w) {
        return false;
    }
    words[prefix_words.len()..]
        .iter()
        .any(|w| items.contains(w))
}

fn has_glob_chars(pattern: &str) -> bool {
    pattern.contains(['*', '?', '['])
}

fn resolve_alias(word: &str, config: &Config, cwd: &Path) -> String {
    let normalized_word = normalize_token(word, cwd);
    let base = config.path_rule_cwd.as_deref().unwrap_or(cwd);
    for (source, target) in &config.aliases {
        if normalized_word == normalize_token(source, base) {
            return target.clone();
        }
    }
    word.to_string()
}

fn flag_pattern_matches(pattern: &str, active: &Flags) -> bool {
    if active.contains(pattern) {
        return true;
    }
    if pattern.contains(['*', '?', '[', ']']) {
        return active.iter().any(|f| fnmatchcase(f, pattern));
    }
    false
}

fn check_rule_context_flags(rule: &Rule, active: &Flags) -> bool {
    if let Some(req) = &rule.required_flags
        && !req.iter().all(|r| flag_pattern_matches(r, active))
    {
        return false;
    }
    if let Some(neg) = &rule.negated_flags
        && neg.iter().any(|n| flag_pattern_matches(n, active))
    {
        return false;
    }
    true
}

/// Variables that change which code a command runs: loader, interpreter and
/// shell startup paths, git config and helpers, pagers and editors.
const CODE_ENV_VARS: &[&str] = &[
    "PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONSTARTUP",
    "PERL5LIB",
    "PERL5OPT",
    "PERLLIB",
    "RUBYLIB",
    "RUBYOPT",
    "NODE_OPTIONS",
    "NODE_PATH",
    "BASH_ENV",
    "ENV",
    "GIT_DIR",
    "GIT_EXEC_PATH",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_EXTERNAL_DIFF",
    "GIT_PAGER",
    "PAGER",
    "GIT_EDITOR",
    "EDITOR",
    "VISUAL",
];
const CODE_ENV_PREFIXES: &[&str] = &["BASH_FUNC_", "GIT_CONFIG", "DYLD_"];

/// The variable name when `word` assigns a code-influencing variable.
pub fn code_env_name(word: &str) -> Option<&str> {
    let name = word.split('=').next().unwrap_or("").trim_end_matches('+');
    (CODE_ENV_VARS.contains(&name) || CODE_ENV_PREFIXES.iter().any(|p| name.starts_with(p)))
        .then_some(name)
}

fn match_option_block(
    rule: &Rule,
    words: Option<&[String]>,
    config: &Config,
    cwd: &Path,
    remote: bool,
) -> bool {
    let Some(words) = words.filter(|w| !w.is_empty()) else {
        return false;
    };
    let Some(options) = &rule.options else {
        return false;
    };
    let Some(mut positionals) = positional_words(words, options) else {
        return false;
    };
    let Ok(mut patterns) = pattern_words(&rule.pattern) else {
        return false;
    };
    if positionals.len() != patterns.len() {
        return false;
    }
    if !remote {
        positionals[0] = resolve_alias(&positionals[0], config, cwd);
        positionals = positionals
            .iter()
            .map(|w| normalize_token(w, cwd))
            .collect();
        let base = config.path_rule_cwd.as_deref().unwrap_or(cwd);
        patterns = patterns.iter().map(|w| normalize_token(w, base)).collect();
    }
    positionals
        .iter()
        .zip(&patterns)
        .all(|(w, p)| fnmatchcase(w, p))
}

/// Command words and per-word parser facts (`SimpleCommand`).
pub struct SimpleCommand<'a> {
    pub words: &'a [String],
    pub raw_words: &'a [String],
    pub word_has_expansions: &'a [bool],
}

fn match_words(
    cmd: &SimpleCommand,
    config: &Config,
    cwd: &Path,
    active: &Flags,
    remote: bool,
) -> Option<Match> {
    let words = cmd.words;
    let resolved: Vec<String> = if !words.is_empty() && !remote {
        let mut r = vec![resolve_alias(&words[0], config, cwd)];
        r.extend_from_slice(&words[1..]);
        r
    } else {
        words.to_vec()
    };
    let normalized_cmd = if remote {
        resolved.join(" ")
    } else {
        normalize_words(&resolved, cwd)
    };
    let mut result: Option<Match> = None;
    let mut literal_words: Option<Vec<String>> = None;
    if config.rules.iter().any(|r| r.options.is_some()) {
        let any_exp = cmd.word_has_expansions.iter().any(|b| *b);
        if !cmd.raw_words.is_empty() && cmd.raw_words.len() == words.len() {
            let decoded: Vec<Option<String>> = cmd
                .raw_words
                .iter()
                .map(|w| decode_literal_word(w, true))
                .collect();
            if !any_exp && decoded.iter().all(Option::is_some) {
                literal_words = Some(decoded.into_iter().flatten().collect());
            }
        } else if cmd.raw_words.is_empty() && !any_exp {
            literal_words = Some(words.to_vec());
        }
    }
    let mut i = 0;
    while i < words.len() && words[i].contains('=') && !words[i].starts_with('-') {
        i += 1;
    }
    // Without an assignment that changes which code runs, the stripped form may
    // still ask or deny, but only a rule spelling out the assignment allows.
    let strip_only_restricts = words[..i].iter().any(|w| code_env_name(w).is_some());
    let (stripped_words, normalized_stripped) = if i > 0 {
        let s = words[i..].to_vec();
        let n = if remote {
            s.join(" ")
        } else {
            normalize_words(&s, cwd)
        };
        (Some(s), Some(n))
    } else {
        (None, None)
    };
    let mut raw_deny_set = false;
    let pattern_base = config.path_rule_cwd.as_deref().unwrap_or(cwd);
    for rule in &config.rules {
        if !check_rule_context_flags(rule, active) {
            continue;
        }
        let may_strip = !strip_only_restricts || rule.decision == "ask" || rule.decision == "deny";
        if rule.options.is_some() {
            let raw_matched =
                match_option_block(rule, literal_words.as_deref(), config, cwd, remote);
            let stripped_matched = i > 0
                && may_strip
                && !raw_deny_set
                && literal_words.as_ref().is_some_and(|lw| {
                    match_option_block(rule, Some(&lw[i.min(lw.len())..]), config, cwd, remote)
                });
            if raw_matched || stripped_matched {
                result = Some(Match::from_rule(rule));
                if raw_matched {
                    raw_deny_set = rule.decision == "deny";
                }
            }
            continue;
        }
        if rule.items.is_some() {
            if match_option_rule(rule, words) {
                result = Some(Match::from_rule(rule));
                raw_deny_set = rule.decision == "deny";
                continue;
            }
            if let Some(sw) = &stripped_words
                && !sw.is_empty()
                && may_strip
                && !raw_deny_set
                && match_option_rule(rule, sw)
            {
                result = Some(Match::from_rule(rule));
            }
            continue;
        }
        let normalized_pattern = if remote {
            rule.pattern.clone()
        } else {
            normalize_pattern(&rule.pattern, pattern_base)
        };
        let mut raw_matched = false;
        let mut stripped_matched = false;
        let stripped = normalized_stripped.as_deref().filter(|s| !s.is_empty());
        if !rule.exact && !has_glob_chars(&normalized_pattern) {
            let prefix_pattern = format!("{normalized_pattern} *");
            if fnmatchcase(&normalized_cmd, &prefix_pattern) || normalized_cmd == normalized_pattern
            {
                raw_matched = true;
            }
            if !raw_matched && let Some(ns) = stripped {
                stripped_matched = fnmatchcase(ns, &prefix_pattern) || ns == normalized_pattern;
            }
        } else {
            raw_matched = fnmatchcase(&normalized_cmd, &normalized_pattern);
            if !raw_matched
                && let Some(base) = normalized_pattern.strip_suffix(" *")
                && !fnmatchcase("", base)
            {
                raw_matched = fnmatchcase(&normalized_cmd, base);
            }
            if !raw_matched && let Some(ns) = stripped {
                stripped_matched = fnmatchcase(ns, &normalized_pattern);
                if !stripped_matched
                    && let Some(base) = normalized_pattern.strip_suffix(" *")
                    && !fnmatchcase("", base)
                {
                    stripped_matched = fnmatchcase(ns, base);
                }
            }
        }
        if raw_matched {
            result = Some(Match::from_rule(rule));
            raw_deny_set = rule.decision == "deny";
        } else if stripped_matched && may_strip && !raw_deny_set {
            result = Some(Match::from_rule(rule));
        }
    }
    result
}

fn normalize_redirect_pattern(pattern: &str, cwd: &Path) -> String {
    if !pattern.contains("**") {
        return normalize_path(pattern, cwd);
    }
    let parts: Vec<&str> = pattern.split('/').collect();
    let idx = parts.iter().position(|p| p.contains("**")).unwrap_or(0);
    let prefix = parts[..idx].join("/");
    if !prefix.is_empty() {
        format!(
            "{}/{}",
            normalize_path(&prefix, cwd),
            parts[idx..].join("/")
        )
    } else {
        pattern.to_string()
    }
}

/// `match_redirect`: last matching redirect rule.
pub fn match_redirect(
    target: &str,
    config: &Config,
    cwd: &Path,
    active: &Flags,
    remote: bool,
) -> Option<Match> {
    let normalized_target = if remote {
        if target.starts_with('/') {
            collapse_path(target)
        } else {
            target.to_string()
        }
    } else {
        normalize_path(target, cwd)
    };
    let base = config.path_rule_cwd.as_deref().unwrap_or(cwd);
    let mut result = None;
    for rule in &config.redirect_rules {
        if !check_rule_context_flags(rule, active) {
            continue;
        }
        if glob_match(
            &normalized_target,
            &normalize_redirect_pattern(&rule.pattern, base),
        ) {
            result = Some(Match::from_rule(rule));
        }
    }
    result
}

/// `match_command` (the analyzer never passes redirects here).
pub fn match_command(
    cmd: &SimpleCommand,
    config: &Config,
    cwd: &Path,
    active: &Flags,
    remote: bool,
) -> Option<Match> {
    match_words(cmd, config, cwd, active, remote)
}

/// `match_after`: message of the last matching `after` rule ("" if silent).
pub fn match_after(words: &[String], config: &Config, cwd: &Path) -> Option<String> {
    let resolved: Vec<String> = match words.split_first() {
        Some((first, rest)) => std::iter::once(resolve_alias(first, config, cwd))
            .chain(rest.iter().cloned())
            .collect(),
        None => Vec::new(),
    };
    let normalized_cmd = normalize_words(&resolved, cwd);
    let base_cwd = config.path_rule_cwd.as_deref().unwrap_or(cwd);
    let mut result = None;
    for rule in &config.after_rules {
        let pattern = normalize_pattern(&rule.pattern, base_cwd);
        let mut matched = fnmatchcase(&normalized_cmd, &pattern);
        if !matched
            && let Some(base) = pattern.strip_suffix(" *")
            && !fnmatchcase("", base)
        {
            matched = fnmatchcase(&normalized_cmd, base);
        }
        if matched {
            result = Some(rule.message.clone().unwrap_or_default());
        }
    }
    result
}

/// Last rule whose pattern matches `value` (fnmatch), honouring context flags.
fn match_value(rules: &[Rule], value: &str, active: &Flags) -> Option<Match> {
    rules
        .iter()
        .rev()
        .find(|r| check_rule_context_flags(r, active) && fnmatchcase(value, &r.pattern))
        .map(Match::from_rule)
}

/// Message of the last matching `after-*` rule ("" if silent).
fn match_after_value(rules: &[Rule], value: &str) -> Option<String> {
    rules
        .iter()
        .rev()
        .find(|r| fnmatchcase(value, &r.pattern))
        .map(|r| r.message.clone().unwrap_or_default())
}

/// `match_mcp`: MCP rules carry no context flags.
pub fn match_mcp(tool_name: &str, config: &Config) -> Option<Match> {
    match_value(&config.mcp_rules, tool_name, &Flags::new())
}

pub fn match_after_mcp(tool_name: &str, config: &Config) -> Option<String> {
    match_after_value(&config.after_mcp_rules, tool_name)
}

pub fn match_web(query: &str, config: &Config, active: &Flags) -> Option<Match> {
    match_value(&config.web_rules, query, active)
}

pub fn match_after_web(query: &str, config: &Config) -> Option<String> {
    match_after_value(&config.after_web_rules, query)
}

/// Last path rule matching `file_path` (same globs as redirect rules).
fn match_path(
    rules: &[Rule],
    file_path: &str,
    config: &Config,
    cwd: &Path,
    active: &Flags,
) -> Option<Match> {
    let normalized_path = normalize_path(file_path, cwd);
    let base = config.path_rule_cwd.as_deref().unwrap_or(cwd);
    rules
        .iter()
        .rev()
        .find(|r| {
            check_rule_context_flags(r, active)
                && glob_match(
                    &normalized_path,
                    &normalize_redirect_pattern(&r.pattern, base),
                )
        })
        .map(Match::from_rule)
}

pub fn match_edit(file_path: &str, config: &Config, cwd: &Path, active: &Flags) -> Option<Match> {
    match_path(&config.edit_rules, file_path, config, cwd, active)
}

pub fn match_read(file_path: &str, config: &Config, cwd: &Path, active: &Flags) -> Option<Match> {
    match_path(&config.read_rules, file_path, config, cwd, active)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> Config {
        parse_config(text, None).unwrap()
    }

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn decide(config: &Config, cmd: &str) -> Option<String> {
        let w = words(cmd);
        let sc = SimpleCommand {
            words: &w,
            raw_words: &[],
            word_has_expansions: &[],
        };
        match_command(&sc, config, Path::new("/tmp"), &Flags::new(), false).map(|m| m.decision)
    }

    #[test]
    fn askpass_settings_merge_by_membership() {
        let base = cfg("set askpass /bin/ask\nset askpass-timeout 30");
        assert_eq!(base.askpass, Some(PathBuf::from("/bin/ask")));
        assert_eq!(base.askpass_timeout, 30);
        assert_eq!(Config::default().askpass_timeout, 59);
        let merged = merge_configs(base.clone(), cfg("set askpass-timeout 59"));
        assert_eq!(merged.askpass_timeout, 59);
        assert_eq!(merged.askpass, Some(PathBuf::from("/bin/ask")));
        assert_eq!(merge_configs(base, cfg("")).askpass_timeout, 30);
    }

    #[test]
    fn log_settings_parse_and_merge() {
        let d = Config::default();
        assert_eq!(
            (
                &d.log,
                d.log_full,
                d.log_rotate_max_days,
                d.log_hook_approvals
            ),
            (&None, false, 30, true)
        );
        let base = cfg(
            "set log /x/audit.log\nset log-full\nset log-rotate-max-days 7\n\
             set log-hook-approvals off",
        );
        assert_eq!(base.log, Some(PathBuf::from("/x/audit.log")));
        assert!(base.log_full);
        assert_eq!(base.log_rotate_max_days, 7);
        assert!(!base.log_hook_approvals);
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            cfg("set log ~/a.log").log,
            Some(PathBuf::from(format!("{home}/a.log")))
        );
        // A higher scope can explicitly restore every default.
        let merged = merge_configs(
            base.clone(),
            cfg("set log /y.log\nset log-rotate-max-days 30\nset log-hook-approvals on"),
        );
        assert_eq!(merged.log, Some(PathBuf::from("/y.log")));
        assert!(merged.log_full);
        assert_eq!(merged.log_rotate_max_days, 30);
        assert!(merged.log_hook_approvals);
        let kept = merge_configs(base, cfg(""));
        assert_eq!(kept.log, Some(PathBuf::from("/x/audit.log")));
        assert_eq!(kept.log_rotate_max_days, 7);
        assert!(!kept.log_hook_approvals);
    }

    #[test]
    fn warnings_like_python_logging() {
        take_warnings();
        parse_config(
            "alias a b\nalias a c\nwrapper w\nwrapper w\nbogus x\n",
            Some("/c"),
        )
        .unwrap();
        assert_eq!(
            take_warnings(),
            [
                "/c: line 2: alias 'a' redefined, overwriting",
                "/c: line 4: duplicate wrapper definition: w",
                "/c: line 5: unknown directive 'bogus' (skipped)",
            ]
        );
        let mut included = BTreeSet::new();
        expand_includes(
            "x\ninclude\ninclude /nonexistent/*.dippy",
            Path::new("/"),
            Path::new("/c"),
            &mut included,
        )
        .unwrap();
        assert_eq!(
            take_warnings(),
            [
                "/c:2: empty include pattern (skipped)",
                "/c:3: no files match '/nonexistent/*.dippy' (skipped)",
            ]
        );
        assert!(take_warnings().is_empty());
    }

    #[test]
    fn rotate_logs_daily_and_prunes() {
        let dir = std::env::temp_dir().join(format!("dippy-rs-rotate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("audit.log");
        let config = cfg(&format!(
            "set log {}\nset log-rotate-max-days 3",
            log.display()
        ));
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        std::fs::write(&log, "a\n").unwrap();
        for name in [
            "audit-2026-10-06.log",
            "audit-2026-10-07.log",
            "audit-x-y-z.log",
        ] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        rotate_logs_on(&config, today);
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "audit-2026-10-07.log",
                "audit-2026-10-09.log",
                "audit-x-y-z.log"
            ]
        );
        // Already rotated today: the new log stays.
        std::fs::write(&log, "b\n").unwrap();
        rotate_logs_on(&config, today);
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "b\n");
        assert_eq!(
            std::fs::read_to_string(dir.join("audit-2026-10-09.log")).unwrap(),
            "a\n"
        );
        // Rotation disabled.
        let off = cfg(&format!(
            "set log {}\nset log-rotate-max-days 0",
            log.display()
        ));
        rotate_logs_on(&off, today.succ_opt().unwrap());
        assert!(log.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn last_match_wins_and_prefix() {
        let c = cfg("allow frob\ndeny frob delete \"no\"");
        assert_eq!(decide(&c, "frob status").as_deref(), Some("allow"));
        assert_eq!(decide(&c, "frob delete x").as_deref(), Some("deny"));
        assert_eq!(c.rules[1].message.as_deref(), Some("no"));
        assert_eq!(decide(&c, "frobnicate"), None);
    }

    #[test]
    fn exact_anchor() {
        let c = cfg("allow frob status|");
        assert_eq!(decide(&c, "frob status").as_deref(), Some("allow"));
        assert_eq!(decide(&c, "frob status x"), None);
    }

    #[test]
    fn env_prefix_fallback_and_raw_deny() {
        let c = cfg("allow frob *\n");
        assert_eq!(decide(&c, "A=1 frob x").as_deref(), Some("allow"));
        let c = cfg("deny A=1 frob *\nallow frob *");
        assert_eq!(decide(&c, "A=1 frob x").as_deref(), Some("deny"));
    }

    #[test]
    fn option_block() {
        let c = cfg("allow [opts: -v, --out=*.txt] frob *");
        let sc_words = words("frob -v --out=a.txt x");
        let sc = SimpleCommand {
            words: &sc_words,
            raw_words: &[],
            word_has_expansions: &[],
        };
        assert!(match_command(&sc, &c, Path::new("/tmp"), &Flags::new(), false).is_some());
        assert_eq!(decide(&c, "frob -x a"), None);
    }

    #[test]
    fn glob_match_components() {
        assert!(glob_match("/a/b/c", "/a/**"));
        assert!(!glob_match("/a", "/a/**"));
        assert!(!glob_match("/a/b/c", "/a/*"));
        assert!(glob_match("/a/b", "/a/*"));
        assert!(glob_match("/a/x/y/out.log", "/a/**/out.log"));
        assert!(!glob_match("/a/xout.log", "/a/**/out.log"));
    }

    #[test]
    fn run_on_server_settings_parse_and_merge_by_membership() {
        let d = Config::default();
        assert_eq!(d.run_on_server_backend, "ssh");
        assert_eq!(d.run_on_server_session, "dippy");
        assert_eq!(d.run_on_server_timeout, 300.0);
        assert_eq!(d.run_on_server_poll_interval, 0.1);
        assert_eq!(d.run_on_server_ssh_config, None);
        assert_eq!(d.run_on_server_ssh_auth_sock, None);
        assert!(d.approval_wait_message.starts_with("Stop work and wait"));
        let c = parse_config(
            "set run-on-server-backend tmux\n\
             set run-on-server-session 'work'\n\
             set run-on-server-timeout 5\n\
             set run-on-server-poll-interval 1e-1\n\
             set run-on-server-ssh-config ssh/cfg\n\
             set run-on-server-ssh-auth-sock none\n\
             set approval-wait-message ' Wait. '",
            Some("/etc/dippy/config"),
        )
        .unwrap();
        assert_eq!(c.run_on_server_backend, "tmux");
        assert_eq!(c.run_on_server_session, "work");
        assert_eq!(c.run_on_server_timeout, 5.0);
        assert_eq!(c.run_on_server_poll_interval, 0.1);
        assert_eq!(
            c.run_on_server_ssh_config,
            Some(PathBuf::from("/etc/dippy/ssh/cfg"))
        );
        assert_eq!(c.run_on_server_ssh_auth_sock.as_deref(), Some("none"));
        assert_eq!(c.approval_wait_message, "Wait.");
        let none = cfg("set run-on-server-ssh-config none");
        assert_eq!(none.run_on_server_ssh_config, None);
        let restored = merge_configs(c.clone(), cfg("set run-on-server-backend ssh"));
        assert_eq!(restored.run_on_server_backend, "ssh");
        assert_eq!(restored.run_on_server_session, "work");
        let kept = merge_configs(c, cfg(""));
        assert_eq!(kept.run_on_server_timeout, 5.0);
        assert_eq!(kept.approval_wait_message, "Wait.");
    }

    #[test]
    fn ssh_profile_error_is_fatal() {
        assert!(parse_config("set run-on-server-ssh-config", None).is_err());
        assert!(parse_config("set unknown-thing 1", None).is_ok());
    }

    #[test]
    fn message_extraction() {
        assert_eq!(
            extract_message("x \"a \\\"b\\\"\"").unwrap(),
            ("x".into(), Some("a \"b\"".into()))
        );
        assert_eq!(extract_message("x\"y\"").unwrap(), ("x\"y\"".into(), None));
        assert!(extract_message("\"only\"").is_err());
    }

    fn flags(items: &[&str]) -> Flags {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn mcp_rules_last_match_wins() {
        let c = cfg("allow-mcp mcp__fake__*\ndeny-mcp mcp__fake__drop \"no drop\"");
        let m = match_mcp("mcp__fake__get", &c).unwrap();
        assert_eq!((m.decision.as_str(), m.message), ("allow", None));
        let m = match_mcp("mcp__fake__drop", &c).unwrap();
        assert_eq!(m.decision, "deny");
        assert_eq!(m.message.as_deref(), Some("no drop"));
        assert_eq!(match_mcp("mcp__other__get", &c), None);
    }

    #[test]
    fn after_mcp_and_web_messages() {
        let c = cfg("after-mcp mcp__fake__* \"done\"\nafter-web *rust* \"read docs\"");
        assert_eq!(match_after_mcp("mcp__fake__x", &c).as_deref(), Some("done"));
        assert_eq!(match_after_mcp("mcp__y", &c), None);
        assert_eq!(
            match_after_web("rust book", &c).as_deref(),
            Some("read docs")
        );
    }

    #[test]
    fn web_rules_with_context_flags() {
        let c = cfg("allow-web\ndeny-web *secret* \"no\"\nask-web [ci] *build* \"check\"");
        assert_eq!(
            match_web("hello", &c, &Flags::new()).unwrap().decision,
            "allow"
        );
        assert_eq!(
            match_web("a secret", &c, &Flags::new()).unwrap().decision,
            "deny"
        );
        assert_eq!(
            match_web("build", &c, &Flags::new()).unwrap().decision,
            "allow"
        );
        assert_eq!(
            match_web("build", &c, &flags(&["ci"])).unwrap().decision,
            "ask"
        );
    }

    #[test]
    fn edit_and_read_rules_use_path_globs() {
        let c = cfg("allow-edit /tmp/w/**\ndeny-edit /tmp/w/secret/* \"keep\"\nallow-read src/*");
        let cwd = Path::new("/tmp/w");
        let m = match_edit("/tmp/w/a/b.txt", &c, cwd, &Flags::new()).unwrap();
        assert_eq!(m.decision, "allow");
        let m = match_edit("/tmp/w/secret/k", &c, cwd, &Flags::new()).unwrap();
        assert_eq!(m.decision, "deny");
        assert_eq!(match_edit("/tmp/wx/a", &c, cwd, &Flags::new()), None);
        assert_eq!(
            match_read("src/x.rs", &c, cwd, &Flags::new())
                .unwrap()
                .pattern,
            "src/*"
        );
        assert_eq!(match_read("/tmp/w/srcx/y", &c, cwd, &Flags::new()), None);
        assert_eq!(match_read("/tmp/w/a/b.txt", &c, cwd, &Flags::new()), None);
    }

    #[test]
    fn tool_rules_are_tagged_and_merged() {
        let base = tag_rules(cfg("allow-mcp mcp__a"), "/u", "user");
        let overlay = tag_rules(cfg("deny-mcp mcp__a"), "/p", "project");
        let merged = merge_configs(base, overlay);
        let m = match_mcp("mcp__a", &merged).unwrap();
        assert_eq!(m.decision, "deny");
        assert_eq!(m.source.as_deref(), Some("/p"));
    }
}
