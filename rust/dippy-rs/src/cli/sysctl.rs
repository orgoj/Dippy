//! Port of `src/dippy/cli/sysctl.py`.
//!
//! sysctl reads or writes kernel state.
//! Reading (no = in args) is safe, writing (= in args or -w/-f flags) is not.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["sysctl"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    // -w explicitly writes, -f loads from file
    if tokens.iter().any(|t| t == "-w" || t == "-f") {
        return Classification::ask_desc("sysctl write");
    }

    // Check for name=value pattern (write operation)
    for token in tokens.iter().skip(1) {
        if token.starts_with('-') {
            continue;
        }
        if token.contains('=') {
            return Classification::ask_desc("sysctl write");
        }
    }

    Classification::allow_desc("sysctl")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn read_vs_write() {
        assert_eq!(run(&["sysctl", "-n", "hw.ncpu"]).action, Action::Allow);
        assert_eq!(run(&["sysctl", "-a"]).action, Action::Allow);
        assert_eq!(run(&["sysctl", "kern.x=1"]).action, Action::Ask);
        assert_eq!(run(&["sysctl", "-w", "kern.x"]).action, Action::Ask);
        assert_eq!(run(&["sysctl", "-f", "conf"]).action, Action::Ask);
    }
}
