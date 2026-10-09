//! Port of `src/dippy/cli/fd.py`.
//!
//! All fd searches are safe; `--exec`/`--exec-batch` delegate to the inner
//! command.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_quote;

pub const COMMANDS: &[&str] = &["fd"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Execution flags that take commands as arguments (`EXEC_FLAGS`).
const EXEC_FLAGS: &[&str] = &["-x", "--exec", "-X", "--exec-batch"];

/// `FLAG_DISPLAY.get(flag, flag)`.
fn flag_display(flag: &str) -> &str {
    match flag {
        "-x" => "-x (execute)",
        "-X" => "-X (execute batch)",
        "--exec" => "--exec (execute)",
        "--exec-batch" => "--exec-batch (execute batch)",
        other => other,
    }
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::allow_desc("fd");
    }
    let Some(exec_flag_idx) = tokens[1..]
        .iter()
        .position(|t| EXEC_FLAGS.contains(&t.as_str()))
        .map(|p| p + 1)
    else {
        return Classification::allow_desc("fd");
    };
    let exec_flag = tokens[exec_flag_idx].as_str();
    let flag_desc = flag_display(exec_flag);
    let inner_start = exec_flag_idx + 1;
    if inner_start >= tokens.len() {
        return Classification::ask_desc(format!("fd {flag_desc} (no command)"));
    }
    let inner_tokens = &tokens[inner_start..];
    let inner_cmd = inner_tokens
        .iter()
        .map(|t| bash_quote(t))
        .collect::<Vec<_>>()
        .join(" ");
    Classification::delegate(inner_cmd).desc(format!("fd {flag_desc} {}", inner_tokens[0]))
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
        assert_eq!(run(&["fd"]).action, Action::Allow);
        assert_eq!(run(&["fd", "-e", "py", "src"]).action, Action::Allow);
    }

    #[test]
    fn exec_delegates() {
        let c = run(&["fd", "-e", "py", "-x", "wc", "-l", "{}"]);
        assert_eq!(c.action, Action::Delegate);
        assert_eq!(c.inner_command.as_deref(), Some("wc -l '{}'"));
        assert_eq!(c.description.as_deref(), Some("fd -x (execute) wc"));
        let c = run(&["fd", "--exec-batch", "rm"]);
        assert_eq!(c.inner_command.as_deref(), Some("rm"));
        assert_eq!(
            c.description.as_deref(),
            Some("fd --exec-batch (execute batch) rm")
        );
    }

    #[test]
    fn exec_without_command_asks() {
        let c = run(&["fd", "pattern", "-X"]);
        assert_eq!(c.action, Action::Ask);
        assert_eq!(
            c.description.as_deref(),
            Some("fd -X (execute batch) (no command)")
        );
    }
}
