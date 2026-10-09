//! Port of `src/dippy/cli/compression_tool.py`.
//!
//! macOS compression utility: -encode/-decode write to stdout unless -o
//! names an output file, which is reported as a redirect target.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["compression_tool"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Extract the output file from -o/--o flag.
fn extract_output_file(tokens: &[String]) -> Option<String> {
    for (i, t) in tokens.iter().enumerate() {
        if (t == "-o" || t == "--o") && i + 1 < tokens.len() {
            return Some(tokens[i + 1].clone());
        }
    }
    None
}

/// Check if -encode or -decode is present.
fn has_operation(tokens: &[String]) -> bool {
    tokens
        .iter()
        .skip(1)
        .any(|t| matches!(t.as_str(), "-encode" | "-decode" | "--encode" | "--decode"))
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.iter().any(|t| t == "-h" || t == "--h") {
        return Classification::allow_desc("compression_tool");
    }
    if !has_operation(tokens) {
        return Classification::allow_desc("compression_tool");
    }
    if let Some(output_file) = extract_output_file(tokens) {
        if !output_file.is_empty() {
            return Classification::allow_desc("compression_tool").redirects(vec![output_file]);
        }
    }
    // No -o means stdout, which is safe
    Classification::allow_desc("compression_tool")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn output_has_redirect_target() {
        let r = run(&[
            "compression_tool",
            "-encode",
            "-i",
            "input.dat",
            "-o",
            "output.lzfse",
        ]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.redirect_targets, Some(vec!["output.lzfse".to_string()]));
    }

    #[test]
    fn stdout_and_help() {
        for t in [
            vec!["compression_tool"],
            vec!["compression_tool", "-h"],
            vec!["compression_tool", "-encode", "-i", "f"],
            vec!["compression_tool", "-o", "out"],
            vec!["compression_tool", "-h", "-encode", "-o", "out"],
        ] {
            let r = run(&t);
            assert_eq!(r.action, Action::Allow, "{t:?}");
            assert_eq!(r.redirect_targets, None, "{t:?}");
        }
    }
}
