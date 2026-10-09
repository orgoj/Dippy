//! Port of `src/dippy/cli/dscl.py`.
//!
//! dscl is the Directory Service command line utility.
//! read/list/search/diff are safe, create/append/merge/delete/change/passwd are not.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["dscl"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Read-only commands (with or without leading dash).
const SAFE_COMMANDS: &[&str] = &[
    "read", "-read", "readall", "-readall", "readpl", "-readpl", "readpli", "-readpli", "list",
    "-list", "search", "-search", "diff", "-diff",
];

/// Options that appear before the datasource.
const OPTIONS: &[&str] = &["-p", "-u", "-P", "-f", "-raw", "-plist", "-url", "-q"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    // Skip options to find datasource and command
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if OPTIONS.contains(&token) {
            // -u, -P, -f take an argument
            if matches!(token, "-u" | "-P" | "-f") {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        break;
    }

    // Skip datasource (e.g., ".", "/Local/Default", "localhost")
    if i < tokens.len() {
        i += 1;
    }

    let Some(command) = tokens.get(i) else {
        return Classification::ask_desc("dscl");
    };

    if SAFE_COMMANDS.contains(&command.as_str()) {
        let cmd_name = command.trim_start_matches('-');
        return Classification::allow_desc(format!("dscl {cmd_name}"));
    }

    Classification::ask_desc("dscl")
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
        let r = run(&["dscl", ".", "-read", "/Users/me"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("dscl read"));
        assert_eq!(
            run(&["dscl", "-u", "admin", "-P", "pw", ".", "list", "/Users"]).action,
            Action::Allow
        );
    }

    #[test]
    fn writes_ask() {
        assert_eq!(
            run(&["dscl", ".", "-create", "/Users/x"]).action,
            Action::Ask
        );
        assert_eq!(run(&["dscl", "."]).action, Action::Ask);
        assert_eq!(run(&["dscl"]).action, Action::Ask);
    }
}
