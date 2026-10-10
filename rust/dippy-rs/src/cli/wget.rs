//! Port of `src/dippy/cli/wget.py`.
//!
//! Wget downloads files by default, so most operations are unsafe. Only
//! --spider mode is safe. Output flags (-O, --output-document) return
//! redirect_targets for config rule checking.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["wget"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Extract the output file from -O/--output-document flag.
fn extract_output_file(tokens: &[String]) -> Option<String> {
    for (i, t) in tokens.iter().enumerate() {
        if t == "-O" && i + 1 < tokens.len() {
            return Some(tokens[i + 1].clone());
        }
        if t == "--output-document" && i + 1 < tokens.len() {
            return Some(tokens[i + 1].clone());
        }
        if let Some(rest) = t.strip_prefix("--output-document=") {
            return Some(rest.to_string());
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("wget");

    if tokens.iter().any(|t| t == "--spider") {
        return Classification::allow_desc(format!("{base} --spider"));
    }

    if let Some(output_file) = extract_output_file(tokens)
        && !output_file.is_empty()
    {
        return Classification::allow_desc(format!("{base} download")).redirects(vec![output_file]);
    }

    Classification::ask_desc(format!("{base} download"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn spider_allows() {
        let r = run(&["wget", "--spider", "https://example.com"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("wget --spider"));
    }

    #[test]
    fn download_asks() {
        for t in [
            vec!["wget", "https://example.com"],
            vec!["wget", "-O"],
            vec!["wget", "--output-document=", "u"],
            vec!["wget", "-q", "-c", "u"],
        ] {
            assert_eq!(run(&t).action, Action::Ask, "{t:?}");
        }
    }

    #[test]
    fn output_document_is_redirect_target() {
        let cases: &[(&[&str], &str)] = &[
            (&["wget", "-O", "out.html", "u"], "out.html"),
            (&["wget", "-O", "-", "u"], "-"),
            (&["wget", "-O", "/dev/null", "u"], "/dev/null"),
            (&["wget", "--output-document", "f", "u"], "f"),
            (&["wget", "--output-document=f", "u"], "f"),
        ];
        for (tokens, target) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Allow, "{tokens:?}");
            assert_eq!(r.redirect_targets, Some(vec![target.to_string()]));
        }
    }
}
