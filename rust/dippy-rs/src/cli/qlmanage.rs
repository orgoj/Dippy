//! Port of `src/dippy/cli/qlmanage.py`.
//!
//! macOS Quick Look Server debug and management tool.
//! - -m (info), -t (thumbnails), -p (previews), -h (help) are safe
//! - -r resets Quick Look Server (modifies system state)

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["qlmanage"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe read-only/display operations.
const SAFE_FLAGS: &[&str] = &["-m", "-t", "-p", "-h"];

/// Flags that modify system state.
const UNSAFE_FLAGS: &[&str] = &["-r"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    for t in ctx.tokens.iter().skip(1) {
        if UNSAFE_FLAGS.contains(&t.as_str()) {
            return Classification::ask_desc(format!("qlmanage {t}"));
        }
        if SAFE_FLAGS.contains(&t.as_str()) {
            return Classification::allow_desc(format!("qlmanage {t}"));
        }
    }
    // No recognized flag, default to allow (probably -h behavior)
    Classification::allow_desc("qlmanage")
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
        assert_eq!(run(&["qlmanage", "-p", "f"]).action, Action::Allow);
        assert_eq!(run(&["qlmanage", "-r"]).action, Action::Ask);
        assert_eq!(run(&["qlmanage", "-r", "cache"]).action, Action::Ask);
        assert_eq!(run(&["qlmanage"]).action, Action::Allow);
    }
}
