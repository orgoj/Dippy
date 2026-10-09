//! Port of `src/dippy/cli/sudo.py`.
//!
//! Handles sudo with command execution. Delegates to inner command check
//! with 'sudo' wrapper context.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["sudo", "doas", "pkexec"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const OPTS_WITH_ARG: &[&str] = &[
    "-C", "-D", "-g", "-h", "-p", "-R", "-r", "-T", "-t", "-U", "-u",
];
const INTERACTIVE_OPTS: &[&str] = &["-i", "-s", "--shell", "--login"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("sudo");
    if tokens.len() < 2 {
        return Classification::ask_desc(format!("{base} (no command)"));
    }

    let mut i = 1;
    while i < tokens.len() {
        let tok = tokens[i].as_str();
        if tok == "--" {
            i += 1;
            break;
        } else if INTERACTIVE_OPTS.contains(&tok) {
            return Classification::ask_desc(format!("{base} {tok}"));
        } else if tok.starts_with('-') {
            if OPTS_WITH_ARG.contains(&tok) {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            break;
        }
    }

    if i >= tokens.len() {
        return Classification::ask_desc(format!("{base} (no command)"));
    }

    // sudo execs argv directly: re-quote so quoted metacharacters stay
    // arguments.
    let inner_cmd = bash_join(&tokens[i..]);
    Classification::delegate(inner_cmd)
        .desc(base)
        .wrapper(vec!["sudo".into()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn sets_wrapper_context() {
        for base in ["sudo", "doas", "pkexec"] {
            let r = run(&[base, "rm", "/tmp/x"]);
            assert_eq!(r.action, Action::Delegate);
            assert_eq!(r.inner_command.as_deref(), Some("rm /tmp/x"));
            assert_eq!(r.wrapper_context, Some(vec!["sudo".to_string()]));
            assert_eq!(r.description.as_deref(), Some(base));
        }
    }

    #[test]
    fn no_wrapper_context_for_interactive() {
        let r = run(&["sudo", "-i"]);
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.wrapper_context, None);
        assert_eq!(r.description.as_deref(), Some("sudo -i"));
    }

    #[test]
    fn options_and_quoting() {
        assert_eq!(
            run(&["sudo", "-u", "root", "ls"]).inner_command.as_deref(),
            Some("ls")
        );
        assert_eq!(
            run(&["sudo", "--", "ls"]).inner_command.as_deref(),
            Some("ls")
        );
        assert_eq!(
            run(&["sudo", "echo", "a;zonk"]).inner_command.as_deref(),
            Some("echo 'a;zonk'")
        );
        assert_eq!(run(&["sudo"]).action, Action::Ask);
        assert_eq!(run(&["sudo", "-v"]).action, Action::Ask);
        assert_eq!(run(&["sudo", "-u", "root", "-i"]).action, Action::Ask);
    }
}
