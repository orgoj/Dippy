//! Port of `src/dippy/cli/iconv.py`.
//!
//! iconv converts text encoding. Safe by default (writes to stdout), but
//! -o/--output writes to a file which needs redirect rule checking.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["iconv"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;

    let mut output_file: Option<String> = None;
    let mut i = 1;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if t == "-o" || t == "--output" {
            if i + 1 < tokens.len() {
                output_file = Some(tokens[i + 1].clone());
            }
            i += 2;
            continue;
        }
        if let Some(rest) = t.strip_prefix("-o") {
            output_file = Some(rest.to_string());
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("--output=") {
            output_file = Some(rest.to_string());
            i += 1;
            continue;
        }
        i += 1;
    }

    match output_file {
        Some(f) if !f.is_empty() => Classification::allow_desc("iconv -o").redirects(vec![f]),
        _ => Classification::allow_desc("iconv"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn stdout_allows() {
        let r = run(&["iconv", "-f", "UTF-8", "-t", "ASCII", "f.txt"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.redirect_targets, None);
        assert_eq!(r.description.as_deref(), Some("iconv"));
        assert_eq!(run(&["iconv", "-o"]).redirect_targets, None);
    }

    #[test]
    fn output_is_redirect_target() {
        let cases: &[(&[&str], &str)] = &[
            (&["iconv", "-f", "a", "-o", "out.txt", "in"], "out.txt"),
            (&["iconv", "-oout.txt", "in"], "out.txt"),
            (&["iconv", "--output", "out.txt", "in"], "out.txt"),
            (&["iconv", "--output=out.txt", "in"], "out.txt"),
            (&["iconv", "-o", "a", "-o", "b"], "b"),
        ];
        for (tokens, target) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Allow, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some("iconv -o"));
            assert_eq!(r.redirect_targets, Some(vec![target.to_string()]));
        }
    }
}
