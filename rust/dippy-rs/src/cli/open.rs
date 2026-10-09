//! Port of `src/dippy/cli/open.py`.
//!
//! open launches files/directories/URLs in their default applications.
//! Only -R (reveal in Finder) is safe - everything else launches external apps.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["open"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    // -R reveals in Finder without launching apps
    if ctx.tokens.iter().any(|t| t == "-R") {
        return Classification::allow_desc("open -R");
    }

    Classification::ask_desc("open")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn reveal_only() {
        assert_eq!(run(&["open", "-R", "file"]).action, Action::Allow);
        assert_eq!(run(&["open", "file"]).action, Action::Ask);
        assert_eq!(run(&["open", "-a", "Safari"]).action, Action::Ask);
    }
}
