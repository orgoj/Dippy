//! Port of `src/dippy/cli/textutil.py`.
//!
//! macOS text file conversion utility.
//! - -info displays file information (safe)
//! - -convert/-cat write files (unsafe unless -stdout is used)
//! - -output specifies output file

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["textutil"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Commands that write files.
const WRITE_COMMANDS: &[&str] = &["-convert", "-cat"];

/// Check if -stdout flag is present.
fn has_stdout(tokens: &[String]) -> bool {
    tokens.iter().any(|t| t == "-stdout")
}

/// Extract the output file from -output flag.
fn extract_output_file(tokens: &[String]) -> Option<&String> {
    for (i, t) in tokens.iter().enumerate() {
        if t == "-output" && i + 1 < tokens.len() {
            return Some(&tokens[i + 1]);
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let has_write_command = tokens
        .iter()
        .skip(1)
        .any(|t| WRITE_COMMANDS.contains(&t.as_str()));
    if !has_write_command {
        // -info, -help, or no command
        return Classification::allow_desc("textutil");
    }
    // Has -convert or -cat
    if has_stdout(tokens) {
        return Classification::allow_desc("textutil");
    }
    if let Some(output_file) = extract_output_file(tokens).filter(|f| !f.is_empty()) {
        return Classification::allow_desc("textutil").redirects(vec![output_file.clone()]);
    }
    // Writes to input file location with new extension
    Classification::ask_desc("textutil")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn textutil_output_has_redirect_target() {
        let r = run(&[
            "textutil", "-convert", "txt", "-output", "out.txt", "foo.rtf",
        ]);
        assert!(r.redirect_targets.unwrap().contains(&"out.txt".to_string()));
    }

    #[test]
    fn branches() {
        assert_eq!(run(&["textutil", "-info", "a.rtf"]).action, Action::Allow);
        assert_eq!(
            run(&["textutil", "-convert", "txt", "-stdout", "a.rtf"]).action,
            Action::Allow
        );
        assert_eq!(
            run(&["textutil", "-convert", "txt", "a.rtf"]).action,
            Action::Ask
        );
    }
}
