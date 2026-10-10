//! Port of `src/dippy/cli/binhex.py`.
//!
//! macOS file encoding utilities (binhex, applesingle, macbinary): probe is
//! safe, -c/--pipe uses stdout, -o names an output file (redirect target),
//! encode/decode otherwise write files.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["binhex", "applesingle", "macbinary"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Check if -c/--pipe/--to-stdout flag is present.
fn has_pipe_flag(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|t| matches!(t.as_str(), "-c" | "--pipe" | "--from-stdin" | "--to-stdout"))
}

/// Value following the first of `flags` that has one.
fn flag_value(tokens: &[String], flags: &[&str]) -> Option<String> {
    for (i, t) in tokens.iter().enumerate() {
        if flags.contains(&t.as_str()) && i + 1 < tokens.len() {
            return Some(tokens[i + 1].clone());
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("binhex");
    if tokens
        .iter()
        .any(|t| matches!(t.as_str(), "-h" | "--help" | "-V" | "--version"))
    {
        return Classification::allow_desc(base);
    }
    if tokens.len() > 1 && tokens[1] == "probe" {
        return Classification::allow_desc(format!("{base} probe"));
    }
    if has_pipe_flag(tokens) {
        return Classification::allow_desc(base);
    }
    if let Some(output_file) = flag_value(tokens, &["-o", "--rename"])
        && !output_file.is_empty()
    {
        return Classification::allow_desc(base).redirects(vec![output_file]);
    }
    if let Some(output_dir) = flag_value(tokens, &["-C", "--directory"])
        && !output_dir.is_empty()
    {
        return Classification::ask_desc(base);
    }
    if tokens.len() > 1 && (tokens[1] == "encode" || tokens[1] == "decode") {
        return Classification::ask_desc(format!("{base} {}", tokens[1]));
    }
    Classification::ask_desc(base)
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
        let r = run(&["binhex", "encode", "-o", "output.hqx", "file.txt"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.redirect_targets, Some(vec!["output.hqx".to_string()]));
    }

    #[test]
    fn cases() {
        for t in [
            vec!["binhex", "-h"],
            vec!["applesingle", "--version"],
            vec!["macbinary", "probe", "f"],
            vec!["binhex", "encode", "-c", "f"],
            vec!["binhex", "decode", "--pipe", "f"],
        ] {
            assert_eq!(run(&t).action, Action::Allow, "{t:?}");
        }
        let cases: &[(&[&str], &str)] = &[
            (&["binhex", "encode", "f"], "binhex encode"),
            (&["binhex", "decode", "-C", "dir", "f"], "binhex"),
            (&["binhex", "f"], "binhex"),
            (&["binhex", "encode", "-o"], "binhex encode"),
        ];
        for (tokens, desc) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Ask, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some(*desc));
        }
    }
}
