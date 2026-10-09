//! Port of `src/dippy/cli/say.py`.
//!
//! macOS text-to-speech utility. Safe by default (speaks to audio output),
//! but -o flag writes audio to a file.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["say"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Extract the output file from -o/--output-file flag.
fn extract_output_file(tokens: &[String]) -> Option<&str> {
    for (i, t) in tokens.iter().enumerate() {
        if t == "-o" && i + 1 < tokens.len() {
            return Some(&tokens[i + 1]);
        }
        if t == "--output-file" && i + 1 < tokens.len() {
            return Some(&tokens[i + 1]);
        }
        if let Some(rest) = t.strip_prefix("--output-file=") {
            return Some(rest);
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    if let Some(output_file) = extract_output_file(&ctx.tokens).filter(|f| !f.is_empty()) {
        return Classification::allow_desc("say -o").redirects(vec![output_file.to_string()]);
    }
    Classification::allow_desc("say")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn say_output_has_redirect_target() {
        let r = run(&["say", "-o", "output.aiff", "hello"]);
        assert!(
            r.redirect_targets
                .unwrap()
                .contains(&"output.aiff".to_string())
        );
    }

    #[test]
    fn output_file_equals_form() {
        let r = run(&["say", "--output-file=a.aiff", "hi"]);
        assert_eq!(r.redirect_targets, Some(vec!["a.aiff".to_string()]));
        let r = run(&["say", "--output-file=", "hi"]);
        assert_eq!(r.action, Action::Allow);
        assert!(r.redirect_targets.is_none());
    }

    #[test]
    fn plain_allows() {
        let r = run(&["say", "hello"]);
        assert_eq!(r.action, Action::Allow);
        assert!(r.redirect_targets.is_none());
    }
}
