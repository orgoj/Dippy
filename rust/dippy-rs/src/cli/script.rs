//! Port of `src/dippy/cli/script.py`.
//!
//! The script command records terminal sessions or runs commands with a
//! pseudo-TTY. When running a command, delegates to inner command check.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["script"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const FLAGS_WITH_ARG: &[&str] = &["-t", "-T"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::ask_desc("script interactive");
    }

    let mut i = 1;
    while i < tokens.len() {
        let tok = tokens[i].as_str();
        if tok == "--" {
            i += 1;
            break;
        }
        if tok.starts_with('-') {
            // Python: known no-arg flags, clusters and anything else all
            // advance by one; only FLAGS_WITH_ARG consume an argument.
            if FLAGS_WITH_ARG.contains(&tok) {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            break;
        }
    }

    if i >= tokens.len() {
        return Classification::ask_desc("script interactive");
    }

    // tokens[i] is the file, tokens[i+1:] is the command (if any)
    let command_tokens = &tokens[i + 1..];

    if command_tokens.is_empty() {
        let is_playback = tokens[1..i]
            .iter()
            .any(|t| t == "-p" || (t.starts_with('-') && t.contains('p') && !t.starts_with("--")));
        if is_playback {
            return Classification::allow_desc("script -p (playback)");
        }
        return Classification::ask_desc("script interactive");
    }

    Classification::delegate(bash_join(command_tokens))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn delegates_command() {
        let cases: &[(&[&str], &str)] = &[
            (&["script", "-q", "/dev/null", "ls"], "ls"),
            (&["script", "/tmp/out.log", "ls", "-la"], "ls -la"),
            (&["script", "-aq", "/dev/null", "ls"], "ls"),
            (
                &["script", "-q", "-a", "/dev/null", "git", "status"],
                "git status",
            ),
            (
                &["script", "-q", "/dev/null", "bash", "-c", "rm -rf /"],
                "bash -c 'rm -rf /'",
            ),
        ];
        for (tokens, inner) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Delegate, "{tokens:?}");
            assert_eq!(r.inner_command.as_deref(), Some(*inner));
        }
    }

    #[test]
    fn playback_and_interactive() {
        for t in [
            vec!["script", "-p", "typescript"],
            vec!["script", "-dp", "typescript"],
            vec!["script", "-p", "-d", "typescript"],
        ] {
            assert_eq!(run(&t).action, Action::Allow, "{t:?}");
        }
        for t in [
            vec!["script"],
            vec!["script", "typescript"],
            vec!["script", "-a", "typescript"],
            vec!["script", "-q", "/dev/null"],
            vec!["script", "-t", "x"],
        ] {
            assert_eq!(run(&t).action, Action::Ask, "{t:?}");
        }
    }
}
