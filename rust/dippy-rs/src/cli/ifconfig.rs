//! Port of `src/dippy/cli/ifconfig.py`.
//!
//! Ifconfig is safe for viewing, but modification commands need
//! confirmation.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["ifconfig"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("ifconfig");
    // "ifconfig", "ifconfig -a" or "ifconfig eth0" are safe; any further
    // argument is a modification.
    if tokens.len() <= 2 {
        return Classification::allow_desc(base);
    }
    Classification::ask_desc(format!("{base} (modify interface)"))
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
        assert_eq!(run(&["ifconfig"]).action, Action::Allow);
        assert_eq!(run(&["ifconfig", "-a"]).action, Action::Allow);
        assert_eq!(run(&["ifconfig", "eth0"]).action, Action::Allow);
        let r = run(&["ifconfig", "eth0", "up"]);
        assert_eq!(r.action, Action::Ask);
        assert_eq!(
            r.description.as_deref(),
            Some("ifconfig (modify interface)")
        );
    }
}
