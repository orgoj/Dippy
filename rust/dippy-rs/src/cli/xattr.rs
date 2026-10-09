//! Port of `src/dippy/cli/xattr.py`.
//!
//! macOS extended attributes utility.
//! - -p (print) and -l (list) are safe read operations
//! - -w (write), -d (delete), -c (clear) modify file metadata

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["xattr"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that modify attributes.
const UNSAFE_FLAGS: &[&str] = &["-w", "-d", "-c"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    for t in ctx.tokens.iter().skip(1) {
        if UNSAFE_FLAGS.contains(&t.as_str()) {
            return Classification::ask_desc(format!("xattr {t}"));
        }
        // Handle combined flags like -wd
        if t.starts_with('-') && !t.starts_with("--") && t.chars().count() > 1 {
            for c in t.chars().skip(1) {
                if matches!(c, 'w' | 'd' | 'c') {
                    return Classification::ask_desc(format!("xattr -{c}"));
                }
            }
        }
    }
    Classification::allow_desc("xattr")
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
        assert_eq!(run(&["xattr", "-l", "f"]).action, Action::Allow);
        assert_eq!(run(&["xattr", "-p", "k", "f"]).action, Action::Allow);
        let r = run(&["xattr", "-rd", "com.apple.quarantine", "f"]);
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("xattr -d"));
        assert_eq!(run(&["xattr", "-c", "f"]).action, Action::Ask);
    }
}
