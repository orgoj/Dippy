//! Port of `src/dippy/cli/security.py`.
//!
//! security administers keychains, keys, certificates, and the Security framework.
//! find-*/get-*/show-*/dump-*/verify-*/list-smartcards/translocate-*/help/error/leaks are safe.
//! add-*/delete-*/create-*/set-*/import/export etc modify keychain state.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["security"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Purely read-only subcommands (no flags that modify state).
const SAFE_SUBCOMMANDS: &[&str] = &[
    "help",
    "show-keychain-info",
    "dump-keychain",
    "find-generic-password",
    "find-internet-password",
    "find-key",
    "find-certificate",
    "find-identity",
    "get-identity-preference",
    "dump-trust-settings",
    "verify-cert",
    "error",
    "leaks",
    "list-smartcards",
    "translocate-policy-check",
    "translocate-status-check",
    "translocate-original-path",
    "requirement-evaluate",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        return Classification::ask_desc("security");
    }

    let subcommand = &tokens[1];

    if SAFE_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return Classification::allow_desc(format!("security {subcommand}"));
    }

    Classification::ask_desc("security")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn subcommands() {
        assert_eq!(
            run(&["security", "find-identity", "-v"]).action,
            Action::Allow
        );
        assert_eq!(
            run(&["security", "delete-keychain", "k"]).action,
            Action::Ask
        );
        assert_eq!(run(&["security", "-v", "find-key"]).action, Action::Ask);
        assert_eq!(run(&["security"]).action, Action::Ask);
    }
}
