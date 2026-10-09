//! Port of `src/dippy/cli/shell.py`.
//!
//! Handles bash, sh, zsh with -c flag (inline commands); delegates to the
//! inner command check.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["bash", "sh", "zsh", "dash", "ksh", "fish"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("shell");
    if tokens.len() < 2 {
        return Classification::ask_desc(format!("{base} interactive"));
    }

    // Find -c flag (standalone or combined like -lc, -cl, -xcl, etc.)
    let c_idx = tokens
        .iter()
        .position(|tok| tok.starts_with('-') && !tok.starts_with("--") && tok.contains('c'));

    let Some(c_idx) = c_idx else {
        return Classification::ask_desc(format!("{base} interactive"));
    };

    let Some(inner_cmd) = tokens.get(c_idx + 1) else {
        return Classification::ask_desc(format!("{base} -c (no command)"));
    };
    if inner_cmd.is_empty() {
        return Classification::ask_desc(format!("{base} -c (no command)"));
    }

    Classification::delegate(inner_cmd.clone()).remote(ctx.remote)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn delegates_c_flag() {
        for flag in ["-c", "-lc", "-cl", "-xc", "-exc"] {
            let r = run(&["bash", flag, "ls -la"]);
            assert_eq!(r.action, Action::Delegate);
            assert_eq!(r.inner_command.as_deref(), Some("ls -la"));
            assert!(!r.remote);
        }
    }

    #[test]
    fn interactive_or_script_asks() {
        for t in [
            vec!["bash"],
            vec!["bash", "script.sh"],
            vec!["bash", "-l"],
            vec!["bash", "--login"],
            vec!["bash", "-i"],
            vec!["bash", "--interactive"],
            vec!["bash", "-c", ""],
            vec!["bash", "-c"],
        ] {
            assert_eq!(run(&t).action, Action::Ask, "{t:?}");
        }
        assert_eq!(
            run(&["sh", "-c"]).description.as_deref(),
            Some("sh -c (no command)")
        );
    }

    #[test]
    fn preserves_remote() {
        let mut ctx = HandlerContext::new(&["sh", "-c", "ls"]);
        ctx.remote = true;
        assert!(classify(&ctx).remote);
    }
}
