//! Port of `src/dippy/cli/arch.py`.
//!
//! arch without arguments prints architecture type (safe).
//! arch with arguments runs a program under a specific architecture (delegate).

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["arch"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that take no argument.
const FLAGS_NO_ARG: &[&str] = &["-32", "-64", "-c", "-h"];

/// Flags that take an argument.
const FLAGS_WITH_ARG: &[&str] = &["-arch", "--arch", "-d", "-e"];

/// Architecture specifiers (used as -x86_64, -arm64, etc.).
const ARCH_FLAGS: &[&str] = &["-i386", "-x86_64", "-x86_64h", "-arm64", "-arm64e"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() == 1 {
        return Classification::allow_desc("arch");
    }

    // Find where the inner command starts
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();

        if FLAGS_NO_ARG.contains(&token) || ARCH_FLAGS.contains(&token) {
            i += 1;
            continue;
        }

        if FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }

        // Architecture flag without hyphen handled by -arch
        if token.starts_with('-') {
            i += 1;
            continue;
        }

        break;
    }

    if i >= tokens.len() {
        return Classification::allow_desc("arch");
    }

    // Delegate to inner command
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
    fn bare_arch_allows() {
        assert_eq!(run(&["arch"]).action, Action::Allow);
        assert_eq!(run(&["arch", "-x86_64"]).action, Action::Allow);
        assert_eq!(run(&["arch", "--arch", "x86_64"]).action, Action::Allow);
    }

    #[test]
    fn delegates_inner_command() {
        let r = run(&["arch", "-x86_64", "rm", "-rf", "/"]);
        assert_eq!(r.action, Action::Delegate);
        assert_eq!(r.inner_command.as_deref(), Some("rm -rf /"));
        let r = run(&["arch", "--arch", "x86_64", "pwd"]);
        assert_eq!(r.inner_command.as_deref(), Some("pwd"));
    }

    #[test]
    fn delegation_preserves_quoting() {
        let r = run(&["arch", "-x86_64", "echo", "a;zonk"]);
        assert_eq!(r.inner_command.as_deref(), Some("echo 'a;zonk'"));
        let r = run(&["arch", "-x86_64", "echo", "(a)"]);
        assert_eq!(r.inner_command.as_deref(), Some("echo '(a)'"));
    }
}
