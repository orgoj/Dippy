//! Port of `src/dippy/cli/spctl.py`.
//!
//! spctl manages the security assessment policy subsystem (Gatekeeper).
//! --assess/--status/--disable-status are safe read operations.
//! --global-enable/--global-disable/--add/--remove etc modify policy.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["spctl"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Read-only options.
const SAFE_OPTIONS: &[&str] = &["--assess", "-a", "--status", "--disable-status"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        return Classification::ask_desc("spctl");
    }

    // Check if any safe option is present
    for token in tokens.iter().skip(1) {
        if SAFE_OPTIONS.contains(&token.as_str()) {
            let opt_name = token.trim_start_matches('-');
            return Classification::allow_desc(format!("spctl {opt_name}"));
        }
    }

    Classification::ask_desc("spctl")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn options() {
        let r = run(&["spctl", "-vv", "--assess", "app"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("spctl assess"));
        assert_eq!(run(&["spctl", "--global-disable"]).action, Action::Ask);
        assert_eq!(run(&["spctl"]).action, Action::Ask);
    }
}
