//! Port of `src/dippy/cli/plutil.py`.
//!
//! macOS property list utility.
//! - -p (print) and -lint (check syntax) are safe
//! - -convert modifies files (in-place or with -o)
//! - -insert, -replace, -remove modify plist contents
//! - -extract just prints extracted value (safe)

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["plutil"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Actions that modify files.
const UNSAFE_ACTIONS: &[&str] = &["-convert", "-insert", "-replace", "-remove"];

/// Extract the output file from -o flag.
fn extract_output_file(tokens: &[String]) -> Option<&String> {
    for (i, t) in tokens.iter().enumerate() {
        if t == "-o" && i + 1 < tokens.len() {
            return Some(&tokens[i + 1]);
        }
    }
    None
}

/// Extract input files that would be modified in-place.
fn extract_input_files(tokens: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    let mut i = 1;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        // Skip flags with arguments
        if matches!(
            t,
            "-o" | "-convert" | "-insert" | "-replace" | "-remove" | "-extract" | "-type"
        ) {
            i += 2;
            continue;
        }
        // Skip standalone flags
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        // Non-flag argument is a file
        files.push(t.to_string());
        i += 1;
    }
    files
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let Some(action) = tokens
        .iter()
        .skip(1)
        .find(|t| UNSAFE_ACTIONS.contains(&t.as_str()))
    else {
        return Classification::allow_desc("plutil");
    };
    // Check for -o flag (explicit output file)
    if let Some(output_file) = extract_output_file(tokens).filter(|f| !f.is_empty()) {
        return Classification::allow_desc(format!("plutil {action}"))
            .redirects(vec![output_file.clone()]);
    }
    // No -o means in-place modification of input files
    let input_files = extract_input_files(tokens);
    if !input_files.is_empty() {
        return Classification::allow_desc(format!("plutil {action}")).redirects(input_files);
    }
    Classification::ask_desc(format!("plutil {action}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    fn targets(r: &Classification) -> Vec<String> {
        r.redirect_targets.clone().unwrap_or_default()
    }

    #[test]
    fn plutil_convert_inplace_has_redirect_target() {
        let r = run(&["plutil", "-convert", "xml1", "file.plist"]);
        assert!(targets(&r).contains(&"file.plist".to_string()));
    }

    #[test]
    fn plutil_convert_output_has_redirect_target() {
        let r = run(&["plutil", "-convert", "json", "-o", "out.json", "file.plist"]);
        assert!(targets(&r).contains(&"out.json".to_string()));
    }

    #[test]
    fn plutil_insert_has_redirect_target() {
        let r = run(&["plutil", "-insert", "key", "-string", "value", "file.plist"]);
        assert!(targets(&r).contains(&"file.plist".to_string()));
    }

    #[test]
    fn read_only_allows_without_targets() {
        let r = run(&["plutil", "-p", "file.plist"]);
        assert_eq!(r.action, Action::Allow);
        assert!(r.redirect_targets.is_none());
    }

    #[test]
    fn no_target_asks() {
        assert_eq!(run(&["plutil", "-convert", "xml1"]).action, Action::Ask);
    }
}
