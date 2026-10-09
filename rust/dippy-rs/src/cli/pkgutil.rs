//! Port of `src/dippy/cli/pkgutil.py`.
//!
//! macOS package utility for querying and manipulating installer packages.
//! - Query commands (--packages, --files, --pkg-info, etc.) are safe
//! - --forget, --learn modify receipt database
//! - --expand, --flatten, --bom create files

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["pkgutil"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe query commands.
const SAFE_COMMANDS: &[&str] = &[
    "--packages",
    "--pkgs",
    "--pkgs-plist",
    "--files",
    "--export-plist",
    "--pkg-info",
    "--pkg-info-plist",
    "--pkg-groups",
    "--groups",
    "--groups-plist",
    "--group-pkgs",
    "--file-info",
    "--file-info-plist",
    "--payload-files",
    "--check-signature",
    "--help",
    "-h",
];

/// Commands that modify state or create files.
const UNSAFE_COMMANDS: &[&str] = &[
    "--forget",  // Discards receipt data
    "--learn",   // Updates ACLs in receipt
    "--expand",  // Expands package to directory
    "--flatten", // Creates flat package
    "--bom",     // Extracts BOM files to /tmp
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    for t in ctx.tokens.iter().skip(1) {
        if UNSAFE_COMMANDS.contains(&t.as_str()) {
            return Classification::ask_desc(format!("pkgutil {t}"));
        }
        if SAFE_COMMANDS.contains(&t.as_str()) {
            return Classification::allow_desc(format!("pkgutil {t}"));
        }
        // Handle --pkgs=REGEXP form
        if t.starts_with("--pkgs=") {
            return Classification::allow_desc("pkgutil --pkgs");
        }
    }
    // No recognized command, default to ask
    Classification::ask_desc("pkgutil")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn commands() {
        assert_eq!(run(&["pkgutil", "--pkgs"]).action, Action::Allow);
        assert_eq!(
            run(&["pkgutil", "--pkgs=com.apple.*"]).action,
            Action::Allow
        );
        assert_eq!(run(&["pkgutil", "--forget", "x"]).action, Action::Ask);
        assert_eq!(run(&["pkgutil", "--expand", "a", "b"]).action, Action::Ask);
        assert_eq!(run(&["pkgutil"]).action, Action::Ask);
        // First recognized command wins
        assert_eq!(run(&["pkgutil", "--forget", "--pkgs"]).action, Action::Ask);
    }
}
