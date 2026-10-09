//! Port of `src/dippy/cli/awk.py`.
//!
//! Awk is safe for text processing, but `-f` runs script files and the
//! program can contain `system()`, pipes and output redirects.
//!
//! Regex notes: Python's `\s` on `str` also matches U+001C..U+001F, which the
//! `regex` crate's `\s` does not, so `PY_S` spells the class out. None of the
//! patterns use look-around or backreferences, and the `regex` crate's
//! leftmost-first semantics give the same spans and groups as Python's
//! backtracking engine for `findall`.

use std::sync::LazyLock;

use regex::Regex;

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["awk", "gawk", "mawk", "nawk"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Python `\s` for `str` patterns.
const PY_S: &str = r"[\s\x1C-\x1F]";

/// `FILE_REDIRECT_PATTERN`.
static FILE_REDIRECT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?:print|printf){PY_S}*[^}}]*{PY_S}*(?:>{PY_S}*["']|>>{PY_S}*["']|>{PY_S}*\(|>>{PY_S}*\(|>{PY_S}*\$)"#
    ))
    .unwrap()
});

/// `LITERAL_REDIRECT_PATTERN`: `> "/path"` or `>> '/path'`.
static LITERAL_REDIRECT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?:print|printf){PY_S}*[^}}]*{PY_S}*>>?{PY_S}*["']([^"']+)["']"#
    ))
    .unwrap()
});

/// `PIPE_PATTERN`: print/printf piped to a quoted command.
static PIPE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?:print|printf){PY_S}*[^}}]*{PY_S}*\|{PY_S}*["']"#
    ))
    .unwrap()
});

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("awk");
    for t in tokens.iter().skip(1) {
        if t.starts_with("-f") {
            return Classification::ask_desc(format!("{base} -f"));
        }
        if t == "--file" || t.starts_with("--file=") {
            return Classification::ask_desc(format!("{base} --file"));
        }
    }

    let mut program: Option<&str> = None;
    let mut i = 1;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if t.starts_with('-') {
            if t == "-F" || t == "-v" || t == "--field-separator" {
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        program = Some(t);
        break;
    }

    // Python: `if not program:` - an empty program is falsy.
    let Some(program) = program.filter(|p| !p.is_empty()) else {
        return Classification::allow_desc(base);
    };

    if program.contains("system(") {
        return Classification::ask_desc(format!("{base} system()"));
    }
    if PIPE_PATTERN.is_match(program) {
        return Classification::ask_desc(format!("{base} pipe"));
    }
    if FILE_REDIRECT_PATTERN.is_match(program) {
        let literal_targets: Vec<String> = LITERAL_REDIRECT_PATTERN
            .captures_iter(program)
            .map(|c| c[1].to_string())
            .collect();
        if !literal_targets.is_empty() {
            return Classification::allow_desc(format!("{base} redirect"))
                .redirects(literal_targets);
        }
        return Classification::ask_desc(format!("{base} redirect"));
    }
    Classification::allow_desc(base)
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn safe_programs_allow() {
        for cmd in [
            vec!["awk", "{print $1}", "file.txt"],
            vec!["awk", "-F:", "{print $1}", "/etc/passwd"],
            vec!["awk", "-F", ",", "{print $2}"],
            vec!["awk", "-v", "x=1", "{print x}"],
            vec!["awk", "{if ($1 > 5) print $1}"],
            vec!["awk", "{print $1 > 5}"],
            vec!["awk", "-F"],
            vec!["gawk", "NR==1"],
        ] {
            let c = run(&cmd);
            assert_eq!(c.action, Action::Allow, "{cmd:?}");
            assert_eq!(c.redirect_targets, None, "{cmd:?}");
        }
    }

    #[test]
    fn script_files_ask() {
        assert_eq!(
            run(&["awk", "-f", "s.awk"]).description.as_deref(),
            Some("awk -f")
        );
        assert_eq!(run(&["awk", "-fs.awk"]).action, Action::Ask);
        assert_eq!(
            run(&["mawk", "--file=s.awk"]).description.as_deref(),
            Some("mawk --file")
        );
    }

    #[test]
    fn system_and_pipes_ask() {
        let c = run(&["awk", "{system(\"rm \" $1)}"]);
        assert_eq!(c.description.as_deref(), Some("awk system()"));
        let c = run(&["awk", "{print $1 | \"sh\"}"]);
        assert_eq!(c.description.as_deref(), Some("awk pipe"));
        let c = run(&["awk", "{print $1 |\x1f'sh'}"]);
        assert_eq!(c.description.as_deref(), Some("awk pipe"));
    }

    #[test]
    fn redirects() {
        let c = run(&["awk", "{print $1 > \"/tmp/out.txt\"}"]);
        assert_eq!(c.action, Action::Allow);
        assert_eq!(c.redirect_targets, Some(s(&["/tmp/out.txt"])));
        assert_eq!(c.description.as_deref(), Some("awk redirect"));
        let c = run(&["awk", "{print >> 'a'; printf \"x\" > \"b\"}"]);
        assert_eq!(c.redirect_targets, Some(s(&["b"])));
        let c = run(&["awk", "{print > \"a\"} {print > \"b\"}"]);
        assert_eq!(c.redirect_targets, Some(s(&["a", "b"])));
        for program in ["{print > $2}", "{print > (\"x\" $1)}", "{print >> ($1)}"] {
            let c = run(&["awk", program]);
            assert_eq!(c.action, Action::Ask, "{program}");
            assert_eq!(c.description.as_deref(), Some("awk redirect"));
        }
    }
}
