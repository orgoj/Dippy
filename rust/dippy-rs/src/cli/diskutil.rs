//! Port of `src/dippy/cli/diskutil.py`.
//!
//! diskutil manipulates local disks, partitions, and volumes.
//! list/info/activity/listFilesystems are safe read operations.
//! mount/unmount/erase/partition etc modify disk state.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["diskutil"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Read-only verbs (case-insensitive matching done below).
const SAFE_VERBS: &[&str] = &["list", "info", "information", "activity", "listfilesystems"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        return Classification::ask_desc("diskutil");
    }

    let verb = tokens[1].to_lowercase();

    if SAFE_VERBS.contains(&verb.as_str()) {
        return Classification::allow_desc(format!("diskutil {verb}"));
    }

    Classification::ask_desc("diskutil")
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
        let r = run(&["diskutil", "listFilesystems"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("diskutil listfilesystems"));
        assert_eq!(run(&["diskutil", "eraseDisk", "x"]).action, Action::Ask);
        assert_eq!(run(&["diskutil"]).action, Action::Ask);
    }
}
