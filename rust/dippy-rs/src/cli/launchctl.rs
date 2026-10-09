//! Port of `src/dippy/cli/launchctl.py`.
//!
//! launchctl controls Apple's launchd manager for daemons and agents.
//! list/print*/blame/plist/procinfo/hostinfo/dumpstate/manager*/version/help/getenv are safe.
//! bootstrap/bootout/enable/disable/start/stop/load/unload/kill etc modify state.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["launchctl"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Read-only subcommands.
const SAFE_SUBCOMMANDS: &[&str] = &[
    "list",
    "print",
    "print-cache",
    "print-disabled",
    "print-token",
    "plist",
    "procinfo",
    "hostinfo",
    "resolveport",
    "blame",
    "dumpstate",
    "dump-xsc",
    "dumpjpcategory",
    "managerpid",
    "manageruid",
    "managername",
    "error",
    "variant",
    "version",
    "help",
    "getenv",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        return Classification::ask_desc("launchctl");
    }

    let subcommand = &tokens[1];

    if SAFE_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return Classification::allow_desc(format!("launchctl {subcommand}"));
    }

    Classification::ask_desc("launchctl")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn subcommands() {
        assert_eq!(run(&["launchctl", "list"]).action, Action::Allow);
        assert_eq!(run(&["launchctl", "print", "system"]).action, Action::Allow);
        assert_eq!(
            run(&["launchctl", "bootout", "gui/501"]).action,
            Action::Ask
        );
        assert_eq!(run(&["launchctl"]).action, Action::Ask);
    }
}
