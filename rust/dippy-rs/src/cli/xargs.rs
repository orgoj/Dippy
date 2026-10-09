//! Port of `src/dippy/cli/xargs.py`: delegate to the inner command.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_quote;

pub const COMMANDS: &[&str] = &["xargs"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that take an argument (`FLAGS_WITH_ARG`).
const FLAGS_WITH_ARG: &[&str] = &[
    "-a",
    "--arg-file",
    "-d",
    "--delimiter",
    "-E",
    "-e",
    "--eof",
    "-I",
    "-J",
    "--replace",
    "-L",
    "-l",
    "--max-lines",
    "-n",
    "--max-args",
    "-P",
    "--max-procs",
    "-R",
    "-s",
    "-S",
    "--max-chars",
    "--process-slot-var",
];

/// Flags that make xargs interactive (`UNSAFE_FLAGS`).
const UNSAFE_FLAGS: &[&str] = &["-p", "--interactive", "-o", "--open-tty"];

/// `FLAG_CONTEXT`.
fn flag_context(flag: &str) -> Option<&'static str> {
    match flag {
        "-p" => Some("prompt before execute"),
        "-o" => Some("open tty"),
        _ => None,
    }
}

/// Skip flags and their arguments; return the index of the first non-flag.
fn skip_flags(tokens: &[String], flags_with_arg: &[&str], stop_at_double_dash: bool) -> usize {
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if stop_at_double_dash && token == "--" {
            return i + 1;
        }
        if !token.starts_with('-') {
            return i;
        }
        if flags_with_arg.contains(&token) {
            i += 2;
            continue;
        }
        let chars: Vec<char> = token.chars().collect();
        if chars.len() > 2 && chars[0] == '-' && chars[1] != '-' {
            let base_flag: String = chars[..2].iter().collect();
            if flags_with_arg.contains(&base_flag.as_str()) {
                i += 1;
                continue;
            }
        }
        // `if "=" in token: i += 1; continue` - same effect as falling through.
        i += 1;
    }
    i
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::ask_desc("xargs (no command)");
    }
    for token in &tokens[1..] {
        let token = token.as_str();
        if token == "--" {
            break;
        }
        if UNSAFE_FLAGS.contains(&token) {
            return match flag_context(token) {
                Some(context) => Classification::ask_desc(format!("xargs {token} ({context})")),
                None => Classification::ask_desc(format!("xargs {token}")),
            };
        }
        if token.starts_with("--interactive") {
            return Classification::ask_desc("xargs --interactive");
        }
        if token.starts_with("--open-tty") {
            return Classification::ask_desc("xargs --open-tty");
        }
    }

    let inner_start = 1 + skip_flags(&tokens[1..], FLAGS_WITH_ARG, true);
    if inner_start >= tokens.len() {
        return Classification::ask_desc("xargs (no command)");
    }
    let inner_cmd = tokens[inner_start..]
        .iter()
        .map(|t| bash_quote(t))
        .collect::<Vec<_>>()
        .join(" ");
    Classification::delegate(inner_cmd)
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn no_command_asks() {
        assert_eq!(run(&["xargs"]).action, Action::Ask);
        assert_eq!(run(&["xargs", "-n", "1"]).action, Action::Ask);
        assert_eq!(run(&["xargs", "--"]).action, Action::Ask);
    }

    #[test]
    fn interactive_flags_ask() {
        let c = run(&["xargs", "-p", "rm"]);
        assert_eq!(
            c.description.as_deref(),
            Some("xargs -p (prompt before execute)")
        );
        let c = run(&["xargs", "--open-tty", "vim"]);
        assert_eq!(c.description.as_deref(), Some("xargs --open-tty"));
        let c = run(&["xargs", "--interactive=yes", "rm"]);
        assert_eq!(c.description.as_deref(), Some("xargs --interactive"));
        // After `--` the flag belongs to the inner command.
        assert_eq!(run(&["xargs", "--", "ls", "-p"]).action, Action::Delegate);
    }

    #[test]
    fn delegates_inner_command() {
        let c = run(&["xargs", "-n", "1", "-I{}", "grep", "a b", "{}"]);
        assert_eq!(c.action, Action::Delegate);
        assert_eq!(c.inner_command.as_deref(), Some("grep 'a b' '{}'"));
        assert_eq!(c.description, None);
        let c = run(&["xargs", "-0", "--max-procs=4", "rm", "-rf"]);
        assert_eq!(c.inner_command.as_deref(), Some("rm -rf"));
        let c = run(&["xargs", "-I", "X", "--", "cp", "X", "/tmp"]);
        assert_eq!(c.inner_command.as_deref(), Some("cp X /tmp"));
    }
}
