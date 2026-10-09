//! Port of `src/dippy/cli/caffeinate.py`.
//!
//! caffeinate without a utility just prevents sleep (safe).
//! caffeinate with a utility runs it while preventing sleep (delegate).

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["caffeinate"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that take no argument.
const FLAGS_NO_ARG: &[&str] = &["-d", "-i", "-m", "-s", "-u"];

/// Flags that take an argument.
const FLAGS_WITH_ARG: &[&str] = &["-t", "-w"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() == 1 {
        return Classification::allow_desc("caffeinate");
    }

    // Find where the utility starts
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();

        if FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }

        if FLAGS_NO_ARG.contains(&token) {
            i += 1;
            continue;
        }

        // Combined flags like -disu
        if let Some(rest) = token.strip_prefix('-')
            && rest.chars().all(|c| "dismu".contains(c))
        {
            i += 1;
            continue;
        }

        break;
    }

    if i >= tokens.len() {
        return Classification::allow_desc("caffeinate");
    }

    // Delegate to utility
    Classification::delegate(bash_join(&tokens[i..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn without_utility_allows() {
        assert_eq!(run(&["caffeinate"]).action, Action::Allow);
        assert_eq!(run(&["caffeinate", "-disu"]).action, Action::Allow);
        assert_eq!(run(&["caffeinate", "-t", "3600"]).action, Action::Allow);
        assert_eq!(run(&["caffeinate", "-"]).action, Action::Allow);
    }

    #[test]
    fn delegates_utility() {
        let r = run(&["caffeinate", "-i", "-w", "123", "make", "build"]);
        assert_eq!(r.action, Action::Delegate);
        assert_eq!(r.inner_command.as_deref(), Some("make build"));
        let r = run(&["caffeinate", "-dx", "ls"]);
        assert_eq!(r.inner_command.as_deref(), Some("-dx ls"));
    }

    #[test]
    fn delegation_preserves_quoting() {
        let r = run(&["caffeinate", "echo", "a;zonk"]);
        assert_eq!(r.inner_command.as_deref(), Some("echo 'a;zonk'"));
    }
}
