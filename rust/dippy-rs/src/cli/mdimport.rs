//! Port of `src/dippy/cli/mdimport.py`.
//!
//! mdimport imports files to Spotlight index.
//! -t is test mode (doesn't store), -L/-A/-X list plugins/schema.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["mdimport"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that are read-only. Python iterates a frozenset here, so when
/// several are present the reported flag (description only) may differ.
const SAFE_FLAGS: &[&str] = &["-t", "-L", "-A", "-X"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    for flag in SAFE_FLAGS {
        if tokens.iter().any(|t| t == flag) {
            return Classification::allow_desc(format!("mdimport {flag}"));
        }
    }

    Classification::ask_desc("mdimport")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn flags() {
        assert_eq!(run(&["mdimport", "-L"]).action, Action::Allow);
        assert_eq!(run(&["mdimport", "-t", "-d1", "f"]).action, Action::Allow);
        assert_eq!(run(&["mdimport", "file"]).action, Action::Ask);
        assert_eq!(run(&["mdimport", "-r", "plugin"]).action, Action::Ask);
    }
}
