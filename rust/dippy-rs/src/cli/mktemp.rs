//! Port of `src/dippy/cli/mktemp.py`.
//!
//! mktemp creates temporary files/directories; -u is a dry run that just
//! prints a name without creating it.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["mktemp"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    if ctx.tokens.iter().any(|t| t == "-u") {
        return Classification::allow_desc("mktemp -u");
    }
    Classification::ask_desc("mktemp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn cases() {
        assert_eq!(run(&["mktemp", "-u"]).action, Action::Allow);
        assert_eq!(run(&["mktemp", "-d", "-u"]).action, Action::Allow);
        assert_eq!(run(&["mktemp"]).action, Action::Ask);
        assert_eq!(run(&["mktemp", "-d"]).action, Action::Ask);
        assert_eq!(run(&["mktemp", "-du"]).action, Action::Ask);
    }
}
