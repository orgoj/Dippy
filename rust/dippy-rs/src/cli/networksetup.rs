//! Port of `src/dippy/cli/networksetup.py`.
//!
//! networksetup is the configuration tool for network system preferences.
//! -get*/-list*/-show*/-is*/-version/-help/-printcommands are safe.
//! -set*/-create*/-delete*/-remove*/-add*/-rename* etc modify settings.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["networksetup"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe option prefixes (read-only operations).
const SAFE_PREFIXES: &[&str] = &["-get", "-list", "-show", "-is"];

/// Safe exact options.
const SAFE_OPTIONS: &[&str] = &["-version", "-help", "-printcommands"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        return Classification::ask_desc("networksetup");
    }

    let option = tokens[1].to_lowercase();
    let name = option.trim_start_matches('-');

    if SAFE_OPTIONS.contains(&option.as_str()) {
        return Classification::allow_desc(format!("networksetup {name}"));
    }

    for prefix in SAFE_PREFIXES {
        if option.starts_with(prefix) {
            return Classification::allow_desc(format!("networksetup {name}"));
        }
    }

    Classification::ask_desc("networksetup")
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
        let r = run(&["networksetup", "-listallnetworkservices"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(
            r.description.as_deref(),
            Some("networksetup listallnetworkservices")
        );
        assert_eq!(
            run(&["networksetup", "-getDNSServers", "Wi-Fi"]).action,
            Action::Allow
        );
        assert_eq!(run(&["networksetup", "-version"]).action, Action::Allow);
        assert_eq!(
            run(&["networksetup", "-setdnsservers", "Wi-Fi", "1.1.1.1"]).action,
            Action::Ask
        );
        assert_eq!(run(&["networksetup"]).action, Action::Ask);
    }
}
