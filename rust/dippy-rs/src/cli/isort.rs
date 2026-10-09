//! Port of `src/dippy/cli/isort.py`.
//!
//! isort is a Python import sorter. It modifies files in place by default,
//! but --check-only, --check, -c, --diff, -d are read-only modes.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["isort"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_FLAGS: &[&str] = &["--check-only", "--check", "-c", "--diff", "-d"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.is_empty() {
        return Classification::ask_desc("isort");
    }
    for token in &tokens[1..] {
        if SAFE_FLAGS.contains(&token.as_str()) {
            return Classification::allow_desc(format!("isort {token}"));
        }
    }
    Classification::ask_desc("isort")
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn action(cmd: &str) -> Action {
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        classify(&HandlerContext::new(&tokens)).action
    }

    #[test]
    fn classifies() {
        for (cmd, allowed) in [
            ("isort --check-only", true),
            ("isort --check-only .", true),
            ("isort --check", true),
            ("isort -c", true),
            ("isort -c src/", true),
            ("isort --diff", true),
            ("isort -d", true),
            ("isort --diff file.py", true),
            ("isort --check-only --diff", true),
            ("isort", false),
            ("isort .", false),
            ("isort src/", false),
            ("isort file.py", false),
            ("isort --profile black .", false),
            ("isort --line-length 100 .", false),
            ("isort --atomic .", false),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(action(cmd), expected, "{cmd}");
        }
    }

    #[test]
    fn description() {
        let c = classify(&HandlerContext::new(&["isort", "-d"]));
        assert_eq!(c.description.as_deref(), Some("isort -d"));
    }
}
