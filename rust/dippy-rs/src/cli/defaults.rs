//! Port of `src/dippy/cli/defaults.py`.
//!
//! defaults reads and writes macOS user configuration.
//! read/read-type/domains/find/help are safe, write/rename/delete are not.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["defaults"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_ACTIONS: &[&str] = &["read", "read-type", "domains", "find", "help"];

/// Global flags that can appear before the action.
const GLOBAL_FLAGS: &[&str] = &["-currentHost", "-host"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    // Find the action (skip global flags)
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if GLOBAL_FLAGS.contains(&token) {
            i += if token == "-host" { 2 } else { 1 };
            continue;
        }
        break;
    }

    let Some(action) = tokens.get(i) else {
        return Classification::ask_desc("defaults");
    };

    if SAFE_ACTIONS.contains(&action.as_str()) {
        return Classification::allow_desc(format!("defaults {action}"));
    }

    Classification::ask_desc(format!("defaults {action}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn reads_allow() {
        assert_eq!(run(&["defaults", "read", "d"]).action, Action::Allow);
        assert_eq!(
            run(&["defaults", "-host", "h", "-currentHost", "domains"]).action,
            Action::Allow
        );
    }

    #[test]
    fn writes_ask() {
        assert_eq!(
            run(&["defaults", "write", "d", "k", "v"]).action,
            Action::Ask
        );
        assert_eq!(run(&["defaults"]).action, Action::Ask);
        assert_eq!(run(&["defaults", "-host", "read"]).action, Action::Ask);
    }
}
