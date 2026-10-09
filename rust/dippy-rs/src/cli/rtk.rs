//! Port of `src/dippy/cli/rtk.py`.
//!
//! rtk (Rust Token Killer) is a transparent wrapper: the inner command is
//! analysed directly. `gain`/`discover` are read-only; `proxy <cmd>` still
//! delegates to the inner command.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["rtk"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const READ_ONLY_SUBCOMMANDS: &[&str] = &["gain", "discover"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() == 1 {
        return Classification::ask_desc("rtk");
    }
    // Python would raise IndexError on empty tokens; fail closed.
    let Some(sub) = tokens.get(1) else {
        return Classification::ask();
    };

    if READ_ONLY_SUBCOMMANDS.contains(&sub.as_str()) {
        return Classification::allow_desc(format!("rtk {sub}"));
    }

    if sub == "proxy" {
        if tokens.len() == 2 {
            return Classification::ask_desc("rtk proxy");
        }
        return Classification::delegate(bash_join(&tokens[2..]));
    }

    if sub.starts_with('-') {
        return Classification::ask_desc(format!("rtk {sub}"));
    }

    Classification::delegate(bash_join(&tokens[1..]))
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
        assert_eq!(run(&["rtk"]).action, Action::Ask);
        assert_eq!(run(&["rtk", "gain", "--history"]).action, Action::Allow);
        assert_eq!(run(&["rtk", "discover"]).action, Action::Allow);
        assert_eq!(run(&["rtk", "proxy"]).action, Action::Ask);
        assert_eq!(run(&["rtk", "--version"]).action, Action::Ask);
        assert_eq!(
            run(&["rtk", "proxy", "git", "status"])
                .inner_command
                .as_deref(),
            Some("git status")
        );
        assert_eq!(
            run(&["rtk", "git", "log", "--oneline", "-5"])
                .inner_command
                .as_deref(),
            Some("git log --oneline -5")
        );
    }
}
