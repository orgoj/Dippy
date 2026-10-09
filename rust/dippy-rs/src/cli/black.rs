//! Port of `src/dippy/cli/black.py`.
//!
//! black is a Python code formatter. It modifies files in place by default,
//! but --check and --diff are read-only modes.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["black"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_FLAGS: &[&str] = &["--check", "--diff"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.is_empty() {
        return Classification::ask_desc("black");
    }
    for token in &tokens[1..] {
        if SAFE_FLAGS.contains(&token.as_str()) {
            return Classification::allow_desc(format!("black {token}"));
        }
    }
    Classification::ask_desc("black")
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
            ("black --check", true),
            ("black --check .", true),
            ("black --check src/", true),
            ("black --diff", true),
            ("black --diff file.py", true),
            ("black --check --diff", true),
            ("black", false),
            ("black .", false),
            ("black src/", false),
            ("black file.py", false),
            ("black --line-length 100 .", false),
            ("black --target-version py39 .", false),
            ("black --fast .", false),
            ("black --quiet .", false),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(action(cmd), expected, "{cmd}");
        }
    }

    #[test]
    fn description() {
        let c = classify(&HandlerContext::new(&["black", ".", "--diff"]));
        assert_eq!(c.description.as_deref(), Some("black --diff"));
        let c = classify(&HandlerContext::new(&Vec::<String>::new()));
        assert_eq!(c.action, Action::Ask);
    }
}
