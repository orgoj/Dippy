//! Port of `src/dippy/cli/xxd.py`.
//!
//! xxd is a hex dump tool. Safe for reading, but -r (revert) mode writes
//! files.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["xxd"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const UNSAFE_FLAGS: &[&str] = &["-r", "-revert"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.is_empty() {
        return Classification::ask_desc("xxd (no args)");
    }
    if tokens
        .iter()
        .skip(1)
        .any(|t| UNSAFE_FLAGS.contains(&t.as_str()))
    {
        return Classification::ask_desc("xxd -r (write binary)");
    }
    Classification::allow_desc("xxd")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn cases() {
        assert_eq!(run(&["xxd", "f.bin"]).action, Action::Allow);
        assert_eq!(run(&["xxd", "-l", "16", "f.bin"]).action, Action::Allow);
        assert_eq!(run(&["xxd", "-r", "f.hex"]).action, Action::Ask);
        assert_eq!(run(&["xxd", "-revert", "f.hex"]).action, Action::Ask);
    }
}
