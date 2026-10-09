//! Port of `dippy.core.config` (command-classification subset).
//!
//! Struct layout mirrors the Python dataclasses. Parsing, loading and
//! matching live further down in this module.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub type Flags = BTreeSet<String>;

#[derive(Debug)]
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
    pub options: Option<BTreeMap<String, Option<String>>>,
}

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

/// Parsed configuration (fields that affect Bash command classification;
/// the remaining Python settings are parsed and validated but only stored
/// as configured names).
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub rules: Vec<Rule>,
    pub redirect_rules: Vec<Rule>,
    pub wrappers: BTreeMap<String, WrapperInfo>,
    pub aliases: BTreeMap<String, String>,
    pub python_allow_modules: Vec<String>,
    pub python_deny_modules: Vec<String>,
    pub python_allow_symbols: Vec<String>,
    pub context_env: Vec<String>,
    pub servers: Vec<String>,
    pub configured_settings: BTreeSet<String>,
    /// 'allow' | 'ask' | 'pass'
    pub default: String,
    pub final_path: Option<PathBuf>,
    pub notifier_command: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            redirect_rules: Vec::new(),
            wrappers: BTreeMap::new(),
            aliases: BTreeMap::new(),
            python_allow_modules: Vec::new(),
            python_deny_modules: Vec::new(),
            python_allow_symbols: Vec::new(),
            context_env: Vec::new(),
            servers: Vec::new(),
            configured_settings: BTreeSet::new(),
            default: "ask".into(),
            final_path: None,
            notifier_command: None,
        }
    }
}
