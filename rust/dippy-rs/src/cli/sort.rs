//! Port of `src/dippy/cli/sort.py`.
//!
//! Sort is safe for text processing, but `-o` writes to a file.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["sort"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Extract the output file from `-o`/`--output`.
fn extract_output_file(tokens: &[String]) -> Option<String> {
    let mut i = 1;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if t == "-o" {
            return tokens.get(i + 1).cloned();
        }
        if let Some(rest) = t.strip_prefix("-o")
            && !rest.is_empty()
        {
            return Some(rest.to_string());
        }
        if t == "--output" {
            return tokens.get(i + 1).cloned();
        }
        if let Some(rest) = t.strip_prefix("--output=") {
            return Some(rest.to_string());
        }
        i += 1;
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("sort");
    // Python: `if output_file:` - an empty string is falsy.
    if let Some(output_file) = extract_output_file(tokens).filter(|f| !f.is_empty()) {
        return Classification::allow_desc(format!("{base} -o (write to file)"))
            .redirects(vec![output_file]);
    }
    Classification::allow_desc(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn plain_sort_allows_without_targets() {
        for cmd in [
            vec!["sort", "file.txt"],
            vec!["sort", "-nru", "file.txt"],
            vec!["sort", "-k2,2", "file.txt"],
            vec!["sort", "--key=2", "file.txt"],
        ] {
            let c = run(&cmd);
            assert_eq!(c.action, super::super::Action::Allow);
            assert_eq!(c.redirect_targets, None, "{cmd:?}");
            assert_eq!(c.description.as_deref(), Some("sort"));
        }
    }

    #[test]
    fn output_forms_report_target() {
        for (cmd, target) in [
            (vec!["sort", "-o", "out.txt", "file.txt"], "out.txt"),
            (vec!["sort", "--output", "out.txt", "file.txt"], "out.txt"),
            (vec!["sort", "--output=out.txt", "file.txt"], "out.txt"),
            (vec!["sort", "-oout.txt", "file.txt"], "out.txt"),
            (vec!["sort", "file.txt", "-o", "out.txt"], "out.txt"),
            (
                vec!["sort", "-n", "-o", "sorted.txt", "file.txt"],
                "sorted.txt",
            ),
        ] {
            let c = run(&cmd);
            assert_eq!(
                c.redirect_targets,
                Some(vec![target.to_string()]),
                "{cmd:?}"
            );
            assert_eq!(c.description.as_deref(), Some("sort -o (write to file)"));
        }
    }

    #[test]
    fn dangling_output_flag() {
        assert_eq!(run(&["sort", "-o"]).redirect_targets, None);
        assert_eq!(run(&["sort", "--output="]).redirect_targets, None);
    }
}
