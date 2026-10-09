//! Port of `src/dippy/cli/pre_commit.py`.
//!
//! pre-commit manages git pre-commit hooks. Most commands modify files or
//! hooks. Only validation and help commands are safe.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["pre-commit"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_ACTIONS: &[&str] = &["validate-config", "validate-manifest", "help"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::allow_desc("pre-commit");
    }
    let action = &tokens[1];
    if SAFE_ACTIONS.contains(&action.as_str()) {
        return Classification::allow_desc(format!("pre-commit {action}"));
    }
    Classification::ask_desc(format!("pre-commit {action}"))
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
            ("pre-commit", true),
            ("pre-commit help", true),
            ("pre-commit validate-config", true),
            ("pre-commit validate-manifest", true),
            ("pre-commit run", false),
            ("pre-commit run --all-files", false),
            ("pre-commit run --files foo.py", false),
            ("pre-commit install", false),
            ("pre-commit install --hook-type pre-push", false),
            ("pre-commit uninstall", false),
            ("pre-commit autoupdate", false),
            ("pre-commit clean", false),
            ("pre-commit gc", false),
            ("pre-commit migrate-config", false),
            ("pre-commit sample-config", false),
            ("pre-commit try-repo", false),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(action(cmd), expected, "{cmd}");
        }
    }
}
