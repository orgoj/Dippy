//! Port of `src/dippy/cli/pytest.py`.
//!
//! Pytest runs arbitrary Python code, so test execution requires approval.
//! Safe operations like --version, --help, --collect-only are auto-approved.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["pytest"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_FLAGS: &[&str] = &["--version", "-V", "--help", "-h", "--collect-only", "--co"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::ask_desc("pytest run");
    }
    // Check if any safe flag is present
    for token in &tokens[1..] {
        if SAFE_FLAGS.contains(&token.as_str()) {
            return Classification::allow_desc(format!("pytest {token}"));
        }
    }
    Classification::ask_desc("pytest run")
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
            ("pytest", false),
            ("pytest tests/", false),
            ("pytest -x -q", false),
            ("pytest --version", true),
            ("pytest -V", true),
            ("pytest --help", true),
            ("pytest -h", true),
            ("pytest --collect-only tests/", true),
            ("pytest --co", true),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(action(cmd), expected, "{cmd}");
        }
    }
}
