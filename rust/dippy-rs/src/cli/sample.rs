//! Port of `src/dippy/cli/sample.py`.
//!
//! The sample command profiles a process and writes output to a file.
//! By default it writes to /tmp which is safe; custom paths need approval.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["sample"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::ask_desc("sample (no target)");
    }

    // Look for -file flag
    let mut i = 1;
    while i < tokens.len() {
        if tokens[i] == "-file" && i + 1 < tokens.len() {
            let filepath = &tokens[i + 1];
            // /tmp writes are safe (default behavior)
            if filepath.starts_with("/tmp/") || filepath.starts_with("/tmp") {
                return Classification::allow_desc("sample -file /tmp/...");
            }
            // Custom paths need approval
            return Classification::ask_desc(format!("sample -file {filepath}"));
        }
        i += 1;
    }

    // No -file flag means default /tmp output, which is safe
    Classification::allow_desc("sample (default /tmp output)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn file_paths() {
        assert_eq!(run(&["sample", "Finder"]).action, Action::Allow);
        assert_eq!(
            run(&["sample", "Finder", "-file", "/tmp/out.txt"]).action,
            Action::Allow
        );
        assert_eq!(
            run(&["sample", "Finder", "-file", "out.txt"]).action,
            Action::Ask
        );
        assert_eq!(run(&["sample"]).action, Action::Ask);
        assert_eq!(run(&["sample", "Finder", "-file"]).action, Action::Allow);
    }
}
