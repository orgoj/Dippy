//! Port of `dippy.cli`: handler context, classification and registry.
//!
//! Each handler module mirrors one `src/dippy/cli/*.py` file and exports
//! `COMMANDS`, `PORTED`, `DESCRIPTION` and `classify`.

use std::path::PathBuf;

use crate::config::Config;

mod registry;
pub use registry::HANDLERS;

pub mod ansible;
pub mod arch;
pub mod auth0;
pub mod awk;
pub mod aws;
pub mod azure;
pub mod binhex;
pub mod black;
pub mod brew;
pub mod caffeinate;
pub mod cargo;
pub mod cdk;
pub mod codesign;
pub mod compression_tool;
pub mod copy_move;
pub mod curl;
pub mod defaults;
pub mod dippy_cli;
pub mod diskutil;
pub mod dmesg;
pub mod docker;
pub mod dscl;
pub mod duckdb;
pub mod env;
pub mod fd;
pub mod find;
pub mod fzf;
pub mod gcloud;
pub mod gh;
pub mod git;
pub mod gzip;
pub mod hdiutil;
pub mod helm;
pub mod iconv;
pub mod ifconfig;
pub mod ip;
pub mod isort;
pub mod journalctl;
pub mod kubectl;
pub mod launchctl;
pub mod lipo;
pub mod mdimport;
pub mod mktemp;
pub mod mysql;
pub mod networksetup;
pub mod npm;
pub mod open;
pub mod openssl;
pub mod packer;
pub mod pip;
pub mod pkgutil;
pub mod plutil;
pub mod pre_commit;
pub mod profiles;
pub mod prometheus;
pub mod psql;
pub mod pytest;
pub mod python;
pub mod qlmanage;
pub mod rtk;
pub mod ruff;
pub mod sample;
pub mod say;
pub mod script;
pub mod scutil;
pub mod security;
pub mod sed;
pub mod sevenz;
pub mod shell;
pub mod sips;
pub mod sort;
pub mod spctl;
pub mod sqlcmd;
pub mod sqlite3;
pub mod ssh;
pub mod sudo;
pub mod symbols;
pub mod sysctl;
pub mod tar;
pub mod tee;
pub mod terraform;
pub mod textutil;
pub mod tmutil;
pub mod uv;
pub mod wget;
pub mod xargs;
pub mod xattr;
pub mod xxd;
pub mod yq;

/// Module-level `get_description(tokens)`.
pub type Describe = fn(&[String]) -> String;

/// Context passed to handlers (`HandlerContext`).
#[derive(Debug, Clone)]
pub struct HandlerContext<'a> {
    pub tokens: Vec<String>,
    /// Whether the command runs in a remote context (container, ssh, ...).
    pub remote: bool,
    /// Working directory for relative path resolution.
    pub cwd: PathBuf,
    pub config: Option<&'a Config>,
    /// Per-token flag: the original word contained bash expansions.
    pub word_has_expansions: Vec<bool>,
    /// Unmodified shell word values, aligned with tokens when available.
    pub raw_words: Vec<String>,
}

impl<'a> HandlerContext<'a> {
    /// Context with only tokens (handler unit tests).
    pub fn new<S: AsRef<str>>(tokens: &[S]) -> Self {
        Self {
            tokens: tokens.iter().map(|t| t.as_ref().to_string()).collect(),
            remote: false,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            config: None,
            word_has_expansions: Vec::new(),
            raw_words: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Ask,
    Delegate,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Allow => "allow",
            Action::Ask => "ask",
            Action::Delegate => "delegate",
        }
    }
}

/// Result of classifying a command (`Classification`).
#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    pub action: Action,
    /// Required when action is `Delegate`.
    pub inner_command: Option<String>,
    /// Overrides the default description.
    pub description: Option<String>,
    /// File targets to check against redirect rules.
    pub redirect_targets: Option<Vec<String>>,
    /// Context flags for wrapper commands (ssh, sudo).
    pub wrapper_context: Option<Vec<String>>,
    /// Inner command runs in a remote context.
    pub remote: bool,
    /// Outer command is the policy surface (replace the suggestion).
    pub replace_suggestion: bool,
}

impl Classification {
    pub fn new(action: Action) -> Self {
        Self {
            action,
            inner_command: None,
            description: None,
            redirect_targets: None,
            wrapper_context: None,
            remote: false,
            replace_suggestion: false,
        }
    }
    pub fn allow() -> Self {
        Self::new(Action::Allow)
    }
    pub fn ask() -> Self {
        Self::new(Action::Ask)
    }
    pub fn delegate(inner: impl Into<String>) -> Self {
        Self {
            inner_command: Some(inner.into()),
            ..Self::new(Action::Delegate)
        }
    }
    /// `Classification("allow", description=...)`.
    pub fn allow_desc(description: impl Into<String>) -> Self {
        Self::allow().desc(description)
    }
    /// `Classification("ask", description=...)`.
    pub fn ask_desc(description: impl Into<String>) -> Self {
        Self::ask().desc(description)
    }
    pub fn desc(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
    pub fn redirects(mut self, targets: Vec<String>) -> Self {
        self.redirect_targets = Some(targets);
        self
    }
    pub fn wrapper(mut self, context: Vec<String>) -> Self {
        self.wrapper_context = Some(context);
        self
    }
    pub fn remote(mut self, remote: bool) -> Self {
        self.remote = remote;
        self
    }
}

pub struct Handler {
    pub module: &'static str,
    pub commands: &'static [&'static str],
    pub classify: fn(&HandlerContext) -> Classification,
    pub description: Option<Describe>,
    pub ported: bool,
}

/// `get_handler(name)`: every Python handler command has an entry, ported
/// or not; unported ones always ask.
pub fn get_handler(command: &str) -> Option<&'static Handler> {
    HANDLERS.iter().find(|h| h.commands.contains(&command))
}

/// Description depth per handler (`DESCRIPTION_DEPTH`).
fn description_depth(name: &str) -> usize {
    match name {
        "aws" | "gcloud" | "az" => 3,
        _ => 2,
    }
}

/// `get_description(tokens, handler_name)`.
pub fn get_description(tokens: &[String], handler_name: Option<&str>) -> String {
    if tokens.is_empty() {
        return "unknown".into();
    }
    let name = handler_name.unwrap_or(&tokens[0]);
    if let Some(describe) = get_handler(name).and_then(|h| h.description) {
        return describe(tokens);
    }
    let depth = description_depth(name);
    tokens[..tokens.len().min(depth)].join(" ")
}
