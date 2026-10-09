//! Port of `src/dippy/cli/codesign.py`.
//!
//! macOS code signing utility.
//! - -d/--display, -v/--verify, -h, --validate-constraint are safe read operations
//! - -s/--sign modifies binaries (requires identity argument)
//! - --remove-signature modifies binaries

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["codesign"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Long flags that modify code.
const UNSAFE_LONG_FLAGS: &[&str] = &["--sign", "--remove-signature"];

/// Single-char flags that modify code.
const UNSAFE_SHORT_FLAGS: &[char] = &['s'];

pub fn classify(ctx: &HandlerContext) -> Classification {
    for t in ctx.tokens.iter().skip(1) {
        if UNSAFE_LONG_FLAGS.contains(&t.as_str()) {
            return Classification::ask_desc(format!("codesign {t}"));
        }
        if t == "-s" {
            return Classification::ask_desc("codesign -s");
        }
        // Handle combined flags like -fs, -vfs, etc.
        if t.starts_with('-') && !t.starts_with("--") && t.chars().count() > 1 {
            for c in t.chars().skip(1) {
                if UNSAFE_SHORT_FLAGS.contains(&c) {
                    return Classification::ask_desc(format!("codesign -{c}"));
                }
            }
        }
    }
    Classification::allow_desc("codesign")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn read_operations_allow() {
        assert_eq!(run(&["codesign", "-dv", "app"]).action, Action::Allow);
        assert_eq!(run(&["codesign", "--verify", "app"]).action, Action::Allow);
    }

    #[test]
    fn signing_asks() {
        let r = run(&["codesign", "-fs", "id", "app"]);
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("codesign -s"));
        assert_eq!(run(&["codesign", "--sign", "id"]).action, Action::Ask);
        assert_eq!(
            run(&["codesign", "--remove-signature", "a"]).action,
            Action::Ask
        );
    }
}
