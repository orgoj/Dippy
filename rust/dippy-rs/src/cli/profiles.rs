//! Port of `src/dippy/cli/profiles.py`.
//!
//! profiles manages configuration and provisioning profiles on macOS.
//! help/status/list/show/validate/version are safe read operations.
//! remove/sync/renew modify installed profiles.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["profiles"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Read-only subcommands.
const SAFE_SUBCOMMANDS: &[&str] = &["help", "status", "list", "show", "validate", "version"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        return Classification::ask_desc("profiles");
    }

    let subcommand = &tokens[1];

    if SAFE_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return Classification::allow_desc(format!("profiles {subcommand}"));
    }

    Classification::ask_desc("profiles")
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
        assert_eq!(run(&["profiles", "list"]).action, Action::Allow);
        assert_eq!(run(&["profiles", "remove", "-all"]).action, Action::Ask);
        assert_eq!(run(&["profiles"]).action, Action::Ask);
    }
}
