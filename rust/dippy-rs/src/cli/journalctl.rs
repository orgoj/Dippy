//! Port of `src/dippy/cli/journalctl.py`.
//!
//! Journalctl is safe for viewing logs, but modification flags need confirmation.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["journalctl"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const UNSAFE_FLAGS: &[&str] = &[
    "--rotate",
    "--vacuum-time",
    "--vacuum-size",
    "--vacuum-files",
    "--flush",
    "--sync",
    "--relinquish-var",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("journalctl");
    for token in tokens.iter().skip(1) {
        if UNSAFE_FLAGS.contains(&token.as_str()) {
            return Classification::ask_desc(format!("{base} {token}"));
        }
        for flag in UNSAFE_FLAGS {
            if token
                .strip_prefix(flag)
                .is_some_and(|rest| rest.starts_with('='))
            {
                return Classification::ask_desc(format!("{base} {flag}"));
            }
        }
    }
    Classification::allow_desc(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn view_allows() {
        assert_eq!(
            run(&["journalctl", "-u", "nginx", "-f"]).action,
            Action::Allow
        );
        assert_eq!(run(&["journalctl", "--vacuum"]).action, Action::Allow);
    }

    #[test]
    fn modification_asks() {
        assert_eq!(run(&["journalctl", "--rotate"]).action, Action::Ask);
        let r = run(&["journalctl", "--vacuum-size=1G"]);
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("journalctl --vacuum-size"));
    }
}
