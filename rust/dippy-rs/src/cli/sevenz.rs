//! Port of `src/dippy/cli/7z.py`.
//!
//! Handles unzip, 7z, 7za, 7zr, 7zz commands. Read-only operations (list,
//! test, info) are safe, extraction/modification is not.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["unzip", "7z", "7za", "7zr", "7zz"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Unzip flags that are safe (read-only operations).
const UNZIP_SAFE_FLAGS: &[&str] = &["-l", "-v", "-t", "-z", "-Z", "-h", "-hh", "--help"];

/// 7z commands that are safe (read-only operations).
const SAFE_7Z_COMMANDS: &[&str] = &["l", "t", "h", "b", "i"];

/// Check if unzip command is safe (listing/testing only).
fn check_unzip(tokens: &[String]) -> bool {
    for t in tokens.iter().skip(1) {
        if UNZIP_SAFE_FLAGS.contains(&t.as_str()) {
            return true;
        }
        // Combined short flags like -lv, -tq, -Zl
        if t.starts_with('-') && !t.starts_with("--") && t.chars().count() > 1 {
            for ch in t[1..].chars() {
                let flag = format!("-{ch}");
                if UNZIP_SAFE_FLAGS.contains(&flag.as_str()) || "lvtZz".contains(ch) {
                    // Not if it has extract-related flags (o=overwrite, d=dir)
                    if t.contains('o') || t.contains('d') {
                        return false;
                    }
                    return true;
                }
            }
        }
    }
    false
}

/// Check if 7z command is safe (list/test/hash/benchmark/info).
fn check_7z(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return true; // Shows help
    }
    if tokens[1] == "--help" || tokens[1] == "-h" {
        return true;
    }
    SAFE_7Z_COMMANDS.contains(&tokens[1].as_str())
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let Some(cmd) = tokens.first() else {
        return Classification::ask_desc("archive");
    };

    let (safe, desc) = if cmd == "unzip" {
        let safe = check_unzip(tokens);
        let desc = if safe {
            format!("{cmd} list")
        } else {
            format!("{cmd} extract")
        };
        (safe, desc)
    } else if matches!(cmd.as_str(), "7z" | "7za" | "7zr" | "7zz") {
        let safe = check_7z(tokens);
        let desc = match tokens.get(1) {
            Some(sub) => format!("{cmd} {sub}"),
            None => cmd.clone(),
        };
        (safe, desc)
    } else {
        (false, format!("{cmd} extract"))
    };

    if safe {
        Classification::allow_desc(desc)
    } else {
        Classification::ask_desc(desc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn unzip() {
        for t in [
            vec!["unzip", "-l", "a.zip"],
            vec!["unzip", "-v", "a.zip"],
            vec!["unzip", "-t", "a.zip"],
            vec!["unzip", "-Z", "a.zip"],
            vec!["unzip", "-lv", "a.zip"],
            vec!["unzip", "-tq", "a.zip"],
            vec!["unzip", "--help"],
        ] {
            let r = run(&t);
            assert_eq!(r.action, Action::Allow, "{t:?}");
            assert_eq!(r.description.as_deref(), Some("unzip list"));
        }
        for t in [
            vec!["unzip", "a.zip"],
            vec!["unzip", "-o", "a.zip"],
            vec!["unzip", "-lo", "a.zip"],
            vec!["unzip", "-ld", "a.zip"],
            vec!["unzip", "-q", "a.zip"],
            vec!["unzip", "a.zip", "-d", "out"],
        ] {
            let r = run(&t);
            assert_eq!(r.action, Action::Ask, "{t:?}");
            assert_eq!(r.description.as_deref(), Some("unzip extract"));
        }
    }

    #[test]
    fn sevenz() {
        for t in [
            vec!["7z"],
            vec!["7z", "l", "a.7z"],
            vec!["7za", "t", "a.7z"],
            vec!["7zr", "h", "f"],
            vec!["7zz", "b"],
            vec!["7z", "i"],
            vec!["7z", "--help"],
            vec!["7z", "-h"],
        ] {
            assert_eq!(run(&t).action, Action::Allow, "{t:?}");
        }
        for t in [
            vec!["7z", "a", "a.7z", "f"],
            vec!["7z", "x", "a.7z"],
            vec!["7z", "e", "a.7z"],
            vec!["7z", "d", "a.7z", "f"],
        ] {
            assert_eq!(run(&t).action, Action::Ask, "{t:?}");
        }
        assert_eq!(run(&["7z"]).description.as_deref(), Some("7z"));
        assert_eq!(
            run(&["7z", "x", "a.7z"]).description.as_deref(),
            Some("7z x")
        );
    }
}
