//! Port of `src/dippy/cli/sips.py`.
//!
//! macOS scriptable image processing system.
//! - -g/--getProperty, --verify are safe read operations
//! - Most other flags modify images: -s, -d, -e, -r, -f, -c, -p, -z, -Z, -i, etc.
//! - -o/--out specifies output file for modifications
//! - -x/--extractProfile extracts embedded profile to specified file

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["sips"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe read-only flags.
const SAFE_FLAGS: &[&str] = &[
    "-g",
    "--getProperty",
    "--verify",
    "-1",
    "--oneLine",
    "-h",
    "--help",
];

// Python also defines FLAGS_WITH_ARG, which no code path uses; it is not
// ported.

/// Extract the output file from -o/--out flag.
fn extract_output_file(tokens: &[String]) -> Option<&String> {
    for (i, t) in tokens.iter().enumerate() {
        if (t == "-o" || t == "--out") && i + 1 < tokens.len() {
            return Some(&tokens[i + 1]);
        }
    }
    None
}

/// Extract the profile file from -x/--extractProfile flag.
fn extract_profile_file(tokens: &[String]) -> Option<&String> {
    for (i, t) in tokens.iter().enumerate() {
        if (t == "-x" || t == "--extractProfile") && i + 1 < tokens.len() {
            return Some(&tokens[i + 1]);
        }
    }
    None
}

/// Check if command only uses read-only flags.
fn is_read_only(tokens: &[String]) -> bool {
    let mut i = 1;
    let mut has_operation = false;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if t.starts_with('-') {
            if SAFE_FLAGS.contains(&t) {
                has_operation = true;
                // -g takes one argument
                if t == "-g" || t == "--getProperty" {
                    i += 2;
                    continue;
                }
            } else {
                // Any other flag is potentially unsafe
                return false;
            }
        }
        i += 1;
    }
    has_operation || tokens.len() == 2 // Just "sips file" queries
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if is_read_only(tokens) {
        return Classification::allow_desc("sips");
    }
    // Check for extractProfile (writes to specified file)
    if let Some(profile_file) = extract_profile_file(tokens).filter(|f| !f.is_empty()) {
        return Classification::allow_desc("sips -x").redirects(vec![profile_file.clone()]);
    }
    // Check for output file
    if let Some(output_file) = extract_output_file(tokens).filter(|f| !f.is_empty()) {
        return Classification::allow_desc("sips").redirects(vec![output_file.clone()]);
    }
    // Modifying in place
    Classification::ask_desc("sips")
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
    fn sips_output_has_redirect_target() {
        let r = run(&[
            "sips",
            "-s",
            "format",
            "jpeg",
            "-o",
            "output.jpg",
            "image.png",
        ]);
        assert!(targets(&r).contains(&"output.jpg".to_string()));
    }

    #[test]
    fn sips_extract_profile_has_redirect_target() {
        let r = run(&["sips", "-x", "profile.icc", "image.png"]);
        assert!(targets(&r).contains(&"profile.icc".to_string()));
    }

    #[test]
    fn sips_extract_profile_long_has_redirect_target() {
        let r = run(&["sips", "--extractProfile", "profile.icc", "image.png"]);
        assert!(targets(&r).contains(&"profile.icc".to_string()));
    }

    #[test]
    fn read_only() {
        assert_eq!(run(&["sips", "image.png"]).action, Action::Allow);
        assert_eq!(
            run(&["sips", "-g", "pixelWidth", "image.png"]).action,
            Action::Allow
        );
        assert_eq!(run(&["sips", "-r", "90", "image.png"]).action, Action::Ask);
        assert_eq!(run(&["sips"]).action, Action::Ask);
    }
}
