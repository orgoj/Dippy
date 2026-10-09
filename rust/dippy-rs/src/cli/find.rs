//! Port of `src/dippy/cli/find.py`.
//!
//! Find is mostly safe, but `-exec`/`-execdir` delegate to the inner command,
//! `-ok`/`-okdir`/`-delete` ask, and `-fprint*`/`-fls` surface their file
//! argument as a redirect target.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["find"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Actions that write find's output to a file (`FILE_WRITE_ACTIONS`).
const FILE_WRITE_ACTIONS: &[&str] = &["-fprint", "-fprint0", "-fprintf", "-fls"];

/// `FLAG_CONTEXT`.
fn flag_context(flag: &str) -> Option<&'static str> {
    match flag {
        "-ok" | "-okdir" => Some("execute with prompt"),
        _ => None,
    }
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("find");
    let mut write_targets: Vec<String> = Vec::new();

    for (i, token) in tokens.iter().enumerate() {
        let token = token.as_str();
        if token == "-ok" || token == "-okdir" {
            let context = flag_context(token).unwrap_or("None");
            return Classification::ask_desc(format!("{base} {token} ({context})"));
        }
        if token == "-delete" {
            return Classification::ask_desc(format!("{base} -delete"));
        }
        if FILE_WRITE_ACTIONS.contains(&token) {
            if i + 1 >= tokens.len() {
                return Classification::ask_desc(format!("{base} {token}"));
            }
            write_targets.push(tokens[i + 1].clone());
            continue;
        }
        if token == "-exec" || token == "-execdir" {
            let inner_tokens: Vec<&String> = tokens[i + 1..]
                .iter()
                .take_while(|t| *t != ";" && *t != "+")
                .collect();
            if inner_tokens.is_empty() {
                return Classification::ask_desc(format!("{base} {token}"));
            }
            let inner_cmd = bash_join(&inner_tokens);
            let inner_name = inner_tokens[0];
            return Classification::delegate(inner_cmd)
                .desc(format!("{base} {token} {inner_name}"));
        }
    }

    if !write_targets.is_empty() {
        return Classification::allow_desc(format!("{base} (write to file)"))
            .redirects(write_targets);
    }
    Classification::allow_desc(base)
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn search_allows() {
        let c = run(&["find", ".", "-name", "*.py"]);
        assert_eq!(c.action, Action::Allow);
        assert_eq!(c.redirect_targets, None);
        assert_eq!(c.description.as_deref(), Some("find"));
    }

    #[test]
    fn dangerous_actions_ask() {
        let c = run(&["find", ".", "-ok", "rm", "{}", ";"]);
        assert_eq!(c.action, Action::Ask);
        assert_eq!(
            c.description.as_deref(),
            Some("find -ok (execute with prompt)")
        );
        assert_eq!(run(&["find", ".", "-okdir", "x", ";"]).action, Action::Ask);
        let c = run(&["find", ".", "-delete"]);
        assert_eq!(c.description.as_deref(), Some("find -delete"));
        assert_eq!(run(&["find", ".", "-exec", ";"]).action, Action::Ask);
        assert_eq!(run(&["find", ".", "-fprint"]).action, Action::Ask);
    }

    #[test]
    fn exec_delegates() {
        let c = run(&["find", ".", "-exec", "grep", "-l", "a b", "{}", ";"]);
        assert_eq!(c.action, Action::Delegate);
        assert_eq!(c.inner_command.as_deref(), Some("grep -l 'a b' '{}'"));
        assert_eq!(c.description.as_deref(), Some("find -exec grep"));
        let c = run(&["find", ".", "-execdir", "cat", "{}", "+"]);
        assert_eq!(c.inner_command.as_deref(), Some("cat '{}'"));
        // Unterminated: everything after -exec.
        let c = run(&["find", ".", "-exec", "rm", "-rf", "{}"]);
        assert_eq!(c.inner_command.as_deref(), Some("rm -rf '{}'"));
    }

    #[test]
    fn file_writes_become_redirect_targets() {
        let c = run(&["find", ".", "-fprint", "out.txt", "-fls", "ls.txt"]);
        assert_eq!(c.action, Action::Allow);
        assert_eq!(
            c.redirect_targets,
            Some(vec!["out.txt".to_string(), "ls.txt".to_string()])
        );
        assert_eq!(c.description.as_deref(), Some("find (write to file)"));
        // A later -delete still asks.
        let c = run(&["find", ".", "-fprint", "out.txt", "-delete"]);
        assert_eq!(c.action, Action::Ask);
    }
}
