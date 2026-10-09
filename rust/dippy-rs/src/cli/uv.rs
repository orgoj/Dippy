//! Port of `src/dippy/cli/uv.py`: UV command handler.
//!
//! UV is a Python package manager with various commands.
//! Some commands need special handling for inner command checking.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["uv", "uvx"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe uv commands.
const SAFE_COMMANDS: &[&str] = &[
    "sync", // Sync dependencies
    "lock", // Generate lockfile
    "tree", // Show dependency tree
    "version",
    "help",
    "--version",
    "--help",
    "venv",   // Create virtual environment
    "export", // Export lockfile to requirements.txt (read-only)
];

/// UV pip subcommands that need confirmation.
const UV_PIP_UNSAFE: &[&str] = &["install", "uninstall", "sync", "compile"];

/// UV pip safe subcommands (handled separately).
const UV_PIP_SAFE: &[&str] = &["list", "freeze", "show", "check", "tree"];

/// Commands with subcommands (`SAFE_SUBCOMMANDS`).
fn safe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "cache" => Some(&["dir"]),
        "python" => Some(&["list", "find", "dir"]),
        _ => None,
    }
}

/// `UNSAFE_SUBCOMMANDS`.
fn unsafe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "cache" => Some(&["clean", "prune"]),
        "python" => Some(&["install", "uninstall", "pin"]),
        _ => None,
    }
}

/// UV run flags that take an argument.
const RUN_FLAGS_WITH_ARG: &[&str] = &[
    "--python",
    "-p",
    "--with",
    "--with-requirements",
    "--project",
    "--directory",
    "--group",
    "--extra",
    "--package",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::allow(); // Just "uv" shows help
    }

    let action = tokens[1].as_str();

    // Version/help checks
    if matches!(
        action,
        "--version" | "-v" | "--help" | "-h" | "version" | "help"
    ) {
        return Classification::allow_desc(format!("uv {action}"));
    }

    // Safe commands
    if SAFE_COMMANDS.contains(&action) {
        return Classification::allow_desc(format!("uv {action}"));
    }

    // Check commands with subcommands
    let safe = safe_subcommands(action);
    let unsafe_subs = unsafe_subcommands(action);
    if safe.is_some() || unsafe_subs.is_some() {
        if let Some(subcommand) = tokens.get(2) {
            let subcommand = subcommand.as_str();
            if safe.is_some_and(|s| s.contains(&subcommand)) {
                return Classification::allow_desc(format!("uv {action} {subcommand}"));
            }
            if unsafe_subs.is_some_and(|s| s.contains(&subcommand)) {
                return Classification::ask_desc(format!("uv {action} {subcommand}"));
            }
        }
        if safe.is_some() {
            return Classification::allow_desc(format!("uv {action}"));
        }
        return Classification::ask_desc(format!("uv {action}"));
    }

    // Handle "uv pip" - check subcommand
    if action == "pip" {
        if let Some(subcommand) = tokens.get(2) {
            let subcommand = subcommand.as_str();
            if UV_PIP_UNSAFE.contains(&subcommand) {
                return Classification::ask_desc(format!("uv pip {subcommand}"));
            }
            if UV_PIP_SAFE.contains(&subcommand) {
                return Classification::allow_desc(format!("uv pip {subcommand}"));
            }
            return Classification::ask_desc(format!("uv pip {subcommand}"));
        }
        return Classification::allow_desc("uv pip"); // Just "uv pip" shows help
    }

    // Handle "uv run" - need to check the inner command
    if action == "run" {
        return classify_uv_run(tokens);
    }

    // Handle "uv tool" - always need confirmation
    if action == "tool" {
        let subcommand = tokens.get(2).map(String::as_str).unwrap_or("");
        return Classification::ask_desc(format!("uv tool {subcommand}").trim().to_string());
    }

    Classification::ask_desc(format!("uv {action}"))
}

/// `_classify_uv_run`: extract and delegate the inner command.
fn classify_uv_run(tokens: &[String]) -> Classification {
    let mut i = 2; // Start after "uv run"
    while i < tokens.len() {
        let token = tokens[i].as_str();

        if token.starts_with('-') {
            if RUN_FLAGS_WITH_ARG.contains(&token) && i + 1 < tokens.len() {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        let inner_tokens = &tokens[i..];
        let inner_cmd_name = &inner_tokens[0];

        // Delegate to inner command check (re-quote so an argument containing
        // shell metacharacters does not turn back into syntax)
        let inner_cmd = bash_join(inner_tokens);
        let mut result =
            Classification::delegate(inner_cmd).desc(format!("uv run {inner_cmd_name}"));
        result.replace_suggestion = true;
        return result;
    }

    Classification::ask_desc("uv run")
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn classify_cmd(cmd: &str) -> Classification {
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        classify(&HandlerContext::new(&tokens))
    }

    #[test]
    fn classifies() {
        for (cmd, allowed) in [
            ("uv", true),
            ("uv --version", true),
            ("uv -v", true),
            ("uv help", true),
            ("uv sync", true),
            ("uv lock", true),
            ("uv tree", true),
            ("uv venv", true),
            ("uv export --format requirements-txt", true),
            ("uv pip", true),
            ("uv pip list", true),
            ("uv pip freeze", true),
            ("uv pip show requests", true),
            ("uv pip check", true),
            ("uv pip tree", true),
            ("uv cache dir", true),
            ("uv cache", true),
            ("uv python list", true),
            ("uv python find", true),
            ("uv python dir", true),
            ("uv python", true),
            ("uv pip install requests", false),
            ("uv pip uninstall requests", false),
            ("uv pip sync requirements.txt", false),
            ("uv pip compile requirements.in", false),
            ("uv pip unknown", false),
            ("uv add requests", false),
            ("uv remove requests", false),
            ("uv cache clean", false),
            ("uv cache prune", false),
            ("uv python install 3.12", false),
            ("uv python pin 3.12", false),
            ("uv tool list", false),
            ("uv tool install ruff", false),
            ("uv tool", false),
            ("uv self update", false),
            ("uv run", false),
            ("uv run --python 3.12", false),
            ("uvx ruff", false),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(classify_cmd(cmd).action, expected, "{cmd}");
        }
    }

    #[test]
    fn run_delegates() {
        let c = classify_cmd("uv run --with requests python script.py");
        assert_eq!(c.action, Action::Delegate);
        assert_eq!(c.inner_command.as_deref(), Some("python script.py"));
        assert_eq!(c.description.as_deref(), Some("uv run python"));
        assert!(c.replace_suggestion);
        assert!(!c.remote);

        let c = classify_cmd("uv run --frozen pytest -x");
        assert_eq!(c.inner_command.as_deref(), Some("pytest -x"));

        // A flag with an argument consumes it; "--python=3.12" consumes nothing.
        let c = classify_cmd("uv run --python=3.12 node --version");
        assert_eq!(c.inner_command.as_deref(), Some("node --version"));
        let c = classify_cmd("uv run -p 3.12 node");
        assert_eq!(c.inner_command.as_deref(), Some("node"));
    }

    #[test]
    fn run_requotes_metacharacters() {
        let c = classify(&HandlerContext::new(&["uv", "run", "echo", "a;zonk"]));
        assert_eq!(c.inner_command.as_deref(), Some("echo 'a;zonk'"));
        let c = classify(&HandlerContext::new(&["uv", "run", "echo", "(a)"]));
        assert_eq!(c.inner_command.as_deref(), Some("echo '(a)'"));
    }

    #[test]
    fn tool_description() {
        assert_eq!(
            classify_cmd("uv tool").description.as_deref(),
            Some("uv tool")
        );
        assert_eq!(
            classify_cmd("uv tool run x").description.as_deref(),
            Some("uv tool run")
        );
    }
}
