//! Port of `src/dippy/cli/dippy.py`: Dippy's own CLI handler.
//!
//! `dippy audit` only queries the audit log; `dippy trust` is denied, and
//! every other subcommand needs approval, unless a rule allows it.

use super::{Action, Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["dippy"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() > 1 && tokens[1] == "audit" {
        return Classification::allow_desc("dippy audit");
    }
    if subcommand(tokens) == Some("trust") {
        return Classification::new(Action::Deny).desc("dippy trust is not allowed");
    }
    Classification::ask()
}

/// Global options of `dippy` that take a value.
const VALUE_OPTIONS: &[&str] = &["--cmd", "--cwd", "--config", "--config-only", "--agent"];

/// First word after the global options.
fn subcommand(tokens: &[String]) -> Option<&str> {
    let mut i = 1;
    while let Some(token) = tokens.get(i) {
        if VALUE_OPTIONS.contains(&token.as_str()) {
            i += 2;
        } else if token.starts_with('-') {
            i += 1;
        } else {
            return Some(token);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    #[test]
    fn audit_only() {
        let r = classify(&HandlerContext::new(&["dippy", "audit", "--last"]));
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("dippy audit"));
        assert_eq!(
            classify(&HandlerContext::new(&["dippy"])).action,
            Action::Ask
        );
        assert_eq!(
            classify(&HandlerContext::new(&["dippy", "dashboard"])).action,
            Action::Ask
        );
        for cmd in [
            &["dippy", "trust", "x.sh"][..],
            &["dippy", "--cwd", "/p", "--json", "trust", "x.sh"],
            &["dippy", "--config-only", "c", "trust", "x.sh"],
        ] {
            assert_eq!(classify(&HandlerContext::new(cmd)).action, Action::Deny);
        }
        assert_eq!(
            classify(&HandlerContext::new(&["dippy", "--cmd", "trust"])).action,
            Action::Ask
        );
    }
}
