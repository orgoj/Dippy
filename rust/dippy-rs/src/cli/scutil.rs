//! Port of `src/dippy/cli/scutil.py`.
//!
//! scutil manages system configuration parameters.
//! --get/--dns/--proxy/-r/-w are safe read operations.
//! --set/--renew/--prefs/--nc and interactive mode are unsafe.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["scutil"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Read-only options.
const SAFE_OPTIONS: &[&str] = &["--get", "--dns", "--proxy", "-r", "-w"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        // Interactive mode
        return Classification::ask_desc("scutil");
    }

    let option = &tokens[1];

    if SAFE_OPTIONS.contains(&option.as_str()) {
        let opt_name = option.trim_start_matches('-');
        return Classification::allow_desc(format!("scutil {opt_name}"));
    }

    Classification::ask_desc("scutil")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn options() {
        let r = run(&["scutil", "--get", "HostName"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("scutil get"));
        assert_eq!(
            run(&["scutil", "--set", "HostName", "x"]).action,
            Action::Ask
        );
        assert_eq!(run(&["scutil"]).action, Action::Ask);
    }
}
