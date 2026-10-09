//! Port of `src/dippy/cli/brew.py` (Homebrew CLI handler).

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["brew"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Actions that only read data.
const SAFE_ACTIONS: &[&str] = &[
    // Info and listing
    "list",
    "ls",
    "leaves",
    "info",
    "desc",
    "home",
    "homepage",
    "deps",
    "uses",
    "options",
    "search",
    "doctor",
    "config",
    "outdated",
    // Aliases
    "dr", // doctor alias
    "-S", // search alias
    // Additional info commands
    "missing",
    "tap-info",
    "formulae",
    "casks",
    "log",
    "cat",
    "commands",
    "fetch", // Just downloads, doesn't install
    "docs",
    "shellenv",
    // Note: analytics handled specially (on/off modify settings)
    // Help and version
    "--version",
    "-v",
    "help", // -v is version for brew specifically
];

/// Global flags that act like read-only commands.
const SAFE_GLOBAL_FLAGS: &[&str] = &[
    "--cache",
    "--cellar",
    "--caskroom",
    "--prefix",
    "--repository",
    "--repo",
    "--env",
    "--taps",
    "--config", // Same as config command
];

// Python also defines UNSAFE_ACTIONS, which no code path reads; every
// action outside SAFE_ACTIONS asks anyway, so it is not ported.

/// Safe subcommands for multi-level commands (`SAFE_SUBCOMMANDS`).
fn safe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "cask" => Some(&["list", "info", "search", "outdated", "home"]),
        "bundle" => Some(&["check", "list"]), // These are read-only
        _ => None,
    }
}

/// Unsafe subcommands that require confirmation (`UNSAFE_SUBCOMMANDS`).
fn unsafe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "cask" => Some(&["install", "uninstall", "upgrade", "zap"]),
        "services" => Some(&["start", "stop", "restart", "run", "cleanup"]),
        "bundle" => Some(&["install", "dump", "cleanup", "exec"]),
        _ => None,
    }
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("brew");
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    let action = tokens[1].as_str();
    let rest = &tokens[2..];
    let desc = format!("{base} {action}");

    // Check global flags that act like commands
    if SAFE_GLOBAL_FLAGS.contains(&action) {
        return Classification::allow_desc(desc);
    }

    // Check subcommands for multi-level commands
    if let Some(safe) = safe_subcommands(action)
        && !rest.is_empty()
        && let Some(subcommand) = find_subcommand(rest)
        && safe.contains(&subcommand)
    {
        return Classification::allow_desc(format!("{desc} {subcommand}"));
    }

    if let Some(unsafe_subs) = unsafe_subcommands(action)
        && !rest.is_empty()
    {
        if let Some(subcommand) = find_subcommand(rest)
            && unsafe_subs.contains(&subcommand)
        {
            return Classification::ask_desc(format!("{desc} {subcommand}"));
        }
        if action == "services" {
            return Classification::ask_desc(desc);
        }
    }

    if action == "services" {
        return Classification::ask_desc(desc);
    }

    if action == "bundle" {
        return Classification::ask_desc(desc);
    }

    // analytics: 'state' is safe, 'on'/'off' modify settings
    if action == "analytics" {
        if !rest.is_empty()
            && let Some(subcommand) = find_subcommand(rest)
            && matches!(subcommand, "on" | "off")
        {
            return Classification::ask_desc(format!("{desc} {subcommand}"));
        }
        return Classification::allow_desc(desc);
    }

    if SAFE_ACTIONS.contains(&action) {
        return Classification::allow_desc(desc);
    }

    Classification::ask_desc(desc)
}

/// Find the first non-flag token (the subcommand).
fn find_subcommand(rest: &[String]) -> Option<&str> {
    rest.iter()
        .find(|t| !t.starts_with('-'))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(cmd: &str) -> Classification {
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        classify(&HandlerContext::new(&tokens))
    }

    #[test]
    fn safe_actions() {
        assert_eq!(run("brew list").action, Action::Allow);
        assert_eq!(run("brew -S foo").action, Action::Allow);
        assert_eq!(run("brew --prefix openssl").action, Action::Allow);
        assert_eq!(run("brew").action, Action::Ask);
        assert_eq!(run("brew install wget").action, Action::Ask);
    }

    #[test]
    fn multi_level() {
        let r = run("brew cask --verbose info firefox");
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("brew cask info"));
        let r = run("brew cask zap firefox");
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("brew cask zap"));
        assert_eq!(run("brew cask").action, Action::Ask);
        assert_eq!(run("brew bundle check").action, Action::Allow);
        assert_eq!(run("brew bundle").action, Action::Ask);
        assert_eq!(run("brew services list").action, Action::Ask);
        assert_eq!(run("brew services").action, Action::Ask);
    }

    #[test]
    fn analytics() {
        assert_eq!(run("brew analytics").action, Action::Allow);
        assert_eq!(run("brew analytics state").action, Action::Allow);
        let r = run("brew analytics off");
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("brew analytics off"));
    }
}
