//! Port of `src/dippy/cli/tmutil.py`.
//!
//! tmutil is the Time Machine utility for managing backups.
//! help/version/destinationinfo/isexcluded/list*/latest*/machinedirectory/uniquesize/
//! verifychecksums/compare/calculatedrift are safe.
//! enable/disable/start/stop/set*/add*/remove*/delete*/restore etc modify state.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["tmutil"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Read-only subcommands.
const SAFE_SUBCOMMANDS: &[&str] = &[
    "help",
    "version",
    "destinationinfo",
    "isexcluded",
    "latestbackup",
    "listbackups",
    "listlocalsnapshotdates",
    "listlocalsnapshots",
    "machinedirectory",
    "uniquesize",
    "verifychecksums",
    "compare",
    "calculatedrift",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    if tokens.len() < 2 {
        return Classification::ask_desc("tmutil");
    }

    let subcommand = &tokens[1];

    if SAFE_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return Classification::allow_desc(format!("tmutil {subcommand}"));
    }

    Classification::ask_desc("tmutil")
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
        assert_eq!(run(&["tmutil", "listbackups"]).action, Action::Allow);
        assert_eq!(run(&["tmutil", "delete", "/x"]).action, Action::Ask);
        assert_eq!(run(&["tmutil"]).action, Action::Ask);
    }
}
