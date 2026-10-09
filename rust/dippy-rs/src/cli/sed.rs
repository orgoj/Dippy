//! Port of `src/dippy/cli/sed.py`.
//!
//! Sed is safe for text processing, but `-i` edits files in place, the `w`
//! command writes files and GNU `e` executes the pattern space.
//!
//! Regex notes: Python's `\s` on `str` also matches U+001C..U+001F
//! (`str.isspace`), which the `regex` crate's `\s` does not, so `PY_S`/`PY_NS`
//! spell the class out. Python's `$` (no MULTILINE) also matches before a
//! trailing newline, but every `$` here follows `\s*`, which absorbs it, so
//! `\z` semantics give the same matches.

use std::sync::LazyLock;

use regex::Regex;

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["sed"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Python `\s` for `str` patterns.
const PY_S: &str = r"[\s\x1C-\x1F]";
/// Python `\S` for `str` patterns.
const PY_NS: &str = r"[^\s\x1C-\x1F]";

/// Flags that take a separate argument (`FLAGS_WITH_ARG`).
const FLAGS_WITH_ARG: &[&str] = &["-e", "--expression", "-f", "--file"];

/// `WRITE_PATTERN`: `/w file` or `w file`.
static WRITE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"/w{PY_S}+({PY_NS}+)|w{PY_S}+({PY_NS}+)")).unwrap());

/// `EXECUTE_PATTERN`: `s///e`, `/pat/e` or a standalone `e` command.
static EXECUTE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"/e{PY_S}*(?:$|;)|(?:^|;){PY_S}*e{PY_S}*(?:$|;|{PY_S})"
    ))
    .unwrap()
});

/// Extract sed script strings from command tokens.
fn extract_scripts(tokens: &[String]) -> Vec<String> {
    let mut scripts = Vec::new();
    let mut i = 1;
    let mut found_script_arg = false;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if t == "-e" || t == "--expression" {
            if let Some(next) = tokens.get(i + 1) {
                scripts.push(next.clone());
                found_script_arg = true;
            }
            i += 2;
            continue;
        }
        if let Some(rest) = t.strip_prefix("--expression=") {
            scripts.push(rest.to_string());
            found_script_arg = true;
            i += 1;
            continue;
        }
        if t == "-f" || t == "--file" {
            i += 2;
            continue;
        }
        if t.starts_with("--file=") {
            i += 1;
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        if !found_script_arg && scripts.is_empty() {
            scripts.push(t.to_string());
            found_script_arg = true;
        }
        i += 1;
    }
    scripts
}

/// Extract file paths from `w` commands in sed scripts.
fn extract_write_targets(scripts: &[String]) -> Vec<String> {
    let mut targets = Vec::new();
    for script in scripts {
        for caps in WRITE_PATTERN.captures_iter(script) {
            let path = caps
                .get(1)
                .filter(|m| !m.as_str().is_empty())
                .or_else(|| caps.get(2));
            if let Some(path) = path.filter(|m| !m.as_str().is_empty()) {
                targets.push(path.as_str().to_string());
            }
        }
    }
    targets
}

/// Check if any script contains the `e` command (shell execution).
fn has_execute_command(scripts: &[String]) -> bool {
    scripts.iter().any(|s| EXECUTE_PATTERN.is_match(s))
}

/// Extract input files that will be modified by `-i`.
fn extract_inplace_files(tokens: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    let mut found_script = false;
    let has_e_flag = tokens[1..]
        .iter()
        .any(|t| t == "-e" || t == "--expression" || t.starts_with("--expression="));
    let mut i = 1;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if FLAGS_WITH_ARG.contains(&t) {
            i += 2;
            continue;
        }
        if t.starts_with("--expression=") || t.starts_with("--file=") {
            i += 1;
            continue;
        }
        if t.starts_with("-i") || t.starts_with("--in-place") {
            i += 1;
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        if !has_e_flag && !found_script {
            found_script = true;
            i += 1;
            continue;
        }
        files.push(t.to_string());
        i += 1;
    }
    files
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("sed");

    let scripts = extract_scripts(tokens);
    if has_execute_command(&scripts) {
        return Classification::ask_desc(format!("{base} e (execute)"));
    }

    let write_targets = extract_write_targets(&scripts);

    let has_inplace = tokens
        .iter()
        .skip(1)
        .any(|t| t.starts_with("-i") || t.starts_with("--in-place"));

    let mut redirect_targets: Vec<String> = Vec::new();
    if has_inplace {
        redirect_targets.extend(extract_inplace_files(tokens));
    }
    redirect_targets.extend(write_targets);

    if !redirect_targets.is_empty() {
        let desc = if has_inplace {
            format!("{base} -i")
        } else {
            format!("{base} w")
        };
        return Classification::allow_desc(desc).redirects(redirect_targets);
    }
    if has_inplace {
        return Classification::ask_desc(format!("{base} -i"));
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
    fn safe_scripts_allow() {
        for script in [
            "s/foo/bar/",
            "s/foo/bar/g",
            "/pattern/d",
            "1,10p",
            "$d",
            "y/abc/xyz/",
            "1!G;h;$!d",
            ":a;N;$!ba;s/\\n/ /g",
            "s/new/old/",
        ] {
            let c = run(&["sed", script, "file.txt"]);
            assert_eq!(c.action, Action::Allow, "{script}");
            assert_eq!(c.redirect_targets, None, "{script}");
        }
        assert_eq!(run(&["sed", "-f", "x.sed", "f"]).action, Action::Allow);
    }

    #[test]
    fn inplace_reports_files() {
        let c = run(&["sed", "-i", "s/a/b/", "f1", "f2"]);
        assert_eq!(c.action, Action::Allow);
        assert_eq!(c.redirect_targets, Some(s(&["f1", "f2"])));
        assert_eq!(c.description.as_deref(), Some("sed -i"));
        let c = run(&["sed", "-e", "s/a/b/", "-i", "f"]);
        assert_eq!(c.redirect_targets, Some(s(&["f"])));
        let c = run(&["sed", "--in-place=.bak", "s/a/b/", "f"]);
        assert_eq!(c.redirect_targets, Some(s(&["f"])));
        // -i'' with the script glued: no files found -> ask.
        let c = run(&["sed", "-is/foo/bar/"]);
        assert_eq!(c.action, Action::Ask);
        // macOS `-i ''`: '' is taken as the script, then the real script is a file.
        let c = run(&["sed", "-i", "", "s/a/b/", "f"]);
        assert_eq!(c.redirect_targets, Some(s(&["s/a/b/", "f"])));
    }

    #[test]
    fn write_pattern() {
        assert_eq!(
            extract_write_targets(&s(&["s/foo/bar/w output.txt"])),
            s(&["output.txt"])
        );
        assert_eq!(
            extract_write_targets(&s(&["/pattern/w matches.txt"])),
            s(&["matches.txt"])
        );
        assert_eq!(extract_write_targets(&s(&["w out"])), s(&["out"]));
        assert_eq!(extract_write_targets(&s(&["s/a/b/w x y"])), s(&["x"]));
        // "w" inside a word followed by whitespace also matches (Python too).
        assert_eq!(extract_write_targets(&s(&["s/new x/y/"])), s(&["x/y/"]));
        assert!(extract_write_targets(&s(&["s/w/x/"])).is_empty());
        // Python \s includes U+001F.
        assert_eq!(extract_write_targets(&s(&["w\x1fout"])), s(&["out"]));
        assert_eq!(extract_write_targets(&s(&["w a\x1fb"])), s(&["a"]));
        let c = run(&["sed", "s/foo/bar/w out.txt", "in.txt"]);
        assert_eq!(c.description.as_deref(), Some("sed w"));
        assert_eq!(c.redirect_targets, Some(s(&["out.txt"])));
    }

    #[test]
    fn execute_pattern() {
        for script in [
            "s/foo/ls/e",
            "e",
            "e date",
            "1d;e",
            "s/a/b/e;p",
            "/x/e  ",
            "s/a/b/e\n",
            "1d; e\tls",
            "e\x1c",
        ] {
            assert!(has_execute_command(&s(&[script])), "{script:?}");
        }
        for script in ["s/e/f/", "echo", "s/a/b/eg", "1e", "/e/d"] {
            assert!(!has_execute_command(&s(&[script])), "{script:?}");
        }
        assert_eq!(run(&["sed", "-e", "p", "-e", "e"]).action, Action::Ask);
    }

    #[test]
    fn extract_scripts_order() {
        assert_eq!(extract_scripts(&s(&["sed", "-n", "p", "e"])), s(&["p"]));
        assert_eq!(
            extract_scripts(&s(&["sed", "--expression=e", "x"])),
            s(&["e"])
        );
        assert_eq!(extract_scripts(&s(&["sed", "-e"])), Vec::<String>::new());
    }
}
