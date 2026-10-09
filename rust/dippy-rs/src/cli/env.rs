//! Port of `src/dippy/cli/env.py`.
//!
//! Env sets environment variables and runs commands; delegates to the
//! inner command check.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["env"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const FLAGS_WITH_ARG: &[&str] = &["-u", "--unset", "-S", "--split-string", "-C", "--chdir"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::allow(); // Just "env" prints environment
    }

    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if token == "--" {
            i += 1;
            break;
        }
        if FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        if token.contains('=') && !token.starts_with('-') {
            i += 1;
            continue;
        }
        break;
    }

    if i >= tokens.len() {
        return Classification::allow();
    }

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
    fn prints_environment() {
        assert_eq!(run(&["env"]).action, Action::Allow);
        assert_eq!(run(&["env", "-0"]).action, Action::Allow);
        assert_eq!(run(&["env", "FOO=bar"]).action, Action::Allow);
        // -S consumes its argument; nothing left to run.
        assert_eq!(run(&["env", "-S", "FOO=bar ls"]).action, Action::Allow);
    }

    #[test]
    fn delegates_inner() {
        let cases: &[(&[&str], &str)] = &[
            (&["env", "ls"], "ls"),
            (&["env", "FOO=bar", "BAZ=qux", "ls"], "ls"),
            (&["env", "-u", "PATH", "ls"], "ls"),
            (&["env", "--chdir=/tmp", "ls"], "ls"),
            (&["env", "-", "ls"], "ls"),
            (&["env", "FOO=bar", "--", "rm", "f"], "rm f"),
            (&["env", "FOO=1", "echo", "(a)"], "echo '(a)'"),
        ];
        for (tokens, inner) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Delegate, "{tokens:?}");
            assert_eq!(r.inner_command.as_deref(), Some(*inner), "{tokens:?}");
        }
    }
}
