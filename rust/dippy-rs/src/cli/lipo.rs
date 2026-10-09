//! Port of `src/dippy/cli/lipo.py`.
//!
//! macOS universal binary tool.
//! - -archs, -info, -detailed_info, -verify_arch are safe read operations
//! - -create, -extract, -extract_family, -remove, -replace, -thin write to -output

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["lipo"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Commands that only read/display info.
const SAFE_COMMANDS: &[&str] = &["-archs", "-info", "-detailed_info", "-verify_arch"];

/// Commands that write to output file.
const WRITE_COMMANDS: &[&str] = &[
    "-create",
    "-extract",
    "-extract_family",
    "-remove",
    "-replace",
    "-thin",
];

/// Extract the output file from -output flag.
fn extract_output_file(tokens: &[String]) -> Option<&String> {
    for (i, t) in tokens.iter().enumerate() {
        if (t == "-output" || t == "-o") && i + 1 < tokens.len() {
            return Some(&tokens[i + 1]);
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let mut has_write_command = false;
    for t in tokens.iter().skip(1) {
        if SAFE_COMMANDS.contains(&t.as_str()) {
            return Classification::allow_desc(format!("lipo {t}"));
        }
        if WRITE_COMMANDS.contains(&t.as_str()) {
            has_write_command = true;
        }
    }
    if has_write_command {
        if let Some(output_file) = extract_output_file(tokens).filter(|f| !f.is_empty()) {
            return Classification::allow_desc("lipo").redirects(vec![output_file.clone()]);
        }
        return Classification::ask_desc("lipo");
    }
    // No recognized command, default to allow for info queries
    Classification::allow_desc("lipo")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn lipo_output_has_redirect_target() {
        let r = run(&[
            "lipo",
            "-create",
            "x86.o",
            "arm64.o",
            "-output",
            "universal.o",
        ]);
        assert_eq!(r.action, Action::Allow);
        assert!(
            r.redirect_targets
                .unwrap()
                .contains(&"universal.o".to_string())
        );
    }

    #[test]
    fn write_without_output_asks() {
        assert_eq!(run(&["lipo", "-create", "a.o", "b.o"]).action, Action::Ask);
        assert_eq!(
            run(&["lipo", "-create", "a.o", "-output", ""]).action,
            Action::Ask
        );
    }

    #[test]
    fn info_allows() {
        let r = run(&["lipo", "-info", "bin"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("lipo -info"));
        assert!(r.redirect_targets.is_none());
    }
}
