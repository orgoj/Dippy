//! Port of `src/dippy/cli/gzip.py`.
//!
//! gzip compresses files, gunzip decompresses them; by default both modify
//! files in-place (unsafe). Safe when using stdout, list, or test modes.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["gzip", "gunzip"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that make the command read-only/safe.
const SAFE_FLAGS: &[&str] = &[
    "-c",
    "--stdout",
    "--to-stdout",
    "-l",
    "--list",
    "-t",
    "--test",
    "--help",
    "--version",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let Some(cmd) = tokens.first() else {
        return Classification::ask_desc("gzip");
    };
    for token in tokens.iter().skip(1) {
        // Combined short flags like -lv, -tv, -dc
        if token.starts_with('-') && !token.starts_with("--") {
            for ch in token[1..].chars() {
                if SAFE_FLAGS.contains(&format!("-{ch}").as_str()) {
                    return Classification::allow_desc(cmd.clone());
                }
            }
        }
        if SAFE_FLAGS.contains(&token.as_str()) {
            return Classification::allow_desc(cmd.clone());
        }
    }
    Classification::ask_desc(cmd.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn cases() {
        for t in [
            vec!["gzip", "-c", "f"],
            vec!["gunzip", "-dc", "f.gz"],
            vec!["gzip", "-lv", "f.gz"],
            vec!["gzip", "--test", "f.gz"],
            vec!["gzip", "--to-stdout", "f"],
            vec!["gzip", "--version"],
        ] {
            assert_eq!(run(&t).action, Action::Allow, "{t:?}");
        }
        for t in [
            vec!["gzip", "f"],
            vec!["gunzip", "f.gz"],
            vec!["gzip", "-9", "f"],
            vec!["gzip", "--force", "f"],
            vec!["gzip"],
        ] {
            assert_eq!(run(&t).action, Action::Ask, "{t:?}");
        }
        assert_eq!(
            run(&["gunzip", "-k", "f"]).description.as_deref(),
            Some("gunzip")
        );
    }
}
