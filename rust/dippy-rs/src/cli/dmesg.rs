//! Port of `src/dippy/cli/dmesg.py`.
//!
//! Dmesg is safe for viewing kernel messages, but -c/--clear clears the ring buffer.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["dmesg"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const UNSAFE_FLAGS: &[&str] = &[
    "-c",
    "--clear",
    "-C",
    "--console-off",
    "-D",
    "--console-on",
    "-E",
    "--console-level",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("dmesg");
    for token in tokens.iter().skip(1) {
        if UNSAFE_FLAGS.contains(&token.as_str()) {
            return Classification::ask_desc(format!("{base} {token}"));
        }
        // Handle combined short flags like -cT
        if token.starts_with('-') && !token.starts_with("--") {
            for c in token.chars().skip(1) {
                let flag = format!("-{c}");
                if UNSAFE_FLAGS.contains(&flag.as_str()) {
                    return Classification::ask_desc(format!("{base} {flag}"));
                }
            }
        }
    }
    Classification::allow_desc(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn view_allows() {
        assert_eq!(run(&["dmesg"]).action, Action::Allow);
        assert_eq!(run(&["dmesg", "-T", "--level=err"]).action, Action::Allow);
    }

    #[test]
    fn clear_asks() {
        let r = run(&["dmesg", "-Tc"]);
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("dmesg -c"));
        assert_eq!(run(&["dmesg", "--clear"]).action, Action::Ask);
        assert_eq!(run(&["dmesg", "-E"]).action, Action::Ask);
    }
}
