//! Port of `src/dippy/cli/dippy.py`: Dippy's own CLI handler.
//!
//! `dippy audit` only queries the audit log; every other subcommand needs
//! approval unless a rule allows it.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["dippy"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() > 1 && tokens[1] == "audit" {
        return Classification::allow_desc("dippy audit");
    }
    Classification::ask()
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
    }
}
