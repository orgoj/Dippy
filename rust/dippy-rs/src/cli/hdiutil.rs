//! Port of `src/dippy/cli/hdiutil.py`.
//!
//! hdiutil manipulates disk images (attach, verify, create, etc).
//! help/info/verify/checksum/imageinfo/isencrypted/plugins/pmap are safe.
//! attach/detach/create/convert/mount etc modify or mount images.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["hdiutil"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Read-only verbs.
const SAFE_VERBS: &[&str] = &[
    "help",
    "info",
    "verify",
    "checksum",
    "imageinfo",
    "isencrypted",
    "plugins",
    "pmap",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        return Classification::ask_desc("hdiutil");
    }

    let verb = &tokens[1];

    if SAFE_VERBS.contains(&verb.as_str()) {
        return Classification::allow_desc(format!("hdiutil {verb}"));
    }

    Classification::ask_desc("hdiutil")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn verbs() {
        assert_eq!(run(&["hdiutil", "info"]).action, Action::Allow);
        assert_eq!(
            run(&["hdiutil", "imageinfo", "a.dmg"]).action,
            Action::Allow
        );
        assert_eq!(run(&["hdiutil", "attach", "a.dmg"]).action, Action::Ask);
        assert_eq!(run(&["hdiutil", "INFO"]).action, Action::Ask);
        assert_eq!(run(&["hdiutil"]).action, Action::Ask);
    }
}
