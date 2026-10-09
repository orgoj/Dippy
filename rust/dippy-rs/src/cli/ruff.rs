//! Port of `src/dippy/cli/ruff.py`.
//!
//! Ruff is a Python linter/formatter. Read-only commands are safe,
//! but format/clean and --fix modify files.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["ruff"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const UNSAFE_ACTIONS: &[&str] = &[
    "format", // Modifies code
    "clean",  // Removes cache files
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("ruff");
    if tokens.len() < 2 {
        return Classification::allow_desc(base); // Just "ruff" shows help
    }
    let action = &tokens[1];
    let desc = format!("{base} {action}");

    // format and clean are unsafe
    if UNSAFE_ACTIONS.contains(&action.as_str()) {
        return Classification::ask_desc(desc);
    }

    // --fix and --fix-only flags modify code
    if tokens.iter().any(|t| t == "--fix" || t == "--fix-only") {
        return Classification::ask_desc(format!("{desc} --fix"));
    }

    Classification::allow_desc(desc)
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn action(cmd: &str) -> Action {
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        classify(&HandlerContext::new(&tokens)).action
    }

    #[test]
    fn classifies() {
        for (cmd, allowed) in [
            ("ruff", true),
            ("ruff --help", true),
            ("ruff -h", true),
            ("ruff help", true),
            ("ruff version", true),
            ("ruff --version", true),
            ("ruff check", true),
            ("ruff check .", true),
            ("ruff check src/ tests/", true),
            ("ruff check --select E501", true),
            ("ruff check --fix", false),
            ("ruff check --fix-only", false),
            ("ruff check --unsafe-fixes", true),
            ("ruff check --show-fixes", true),
            ("ruff check --diff", true),
            ("ruff check --output-format=json", true),
            ("ruff check --watch", true),
            ("ruff lint src/", true),
            ("ruff format", false),
            ("ruff format .", false),
            ("ruff format --check", false),
            ("ruff format --diff", false),
            ("ruff rule E501", true),
            ("ruff rule --all", true),
            ("ruff linter", true),
            ("ruff config", true),
            ("ruff clean", false),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(action(cmd), expected, "{cmd}");
        }
    }

    #[test]
    fn description() {
        let c = classify(&HandlerContext::new(&["ruff", "check", "--fix"]));
        assert_eq!(c.description.as_deref(), Some("ruff check --fix"));
    }
}
