//! Port of `src/dippy/cli/curl.py`.
//!
//! Approves GET/HEAD requests, blocks data-sending operations. Output flags
//! (-o, --output) return redirect_targets for config rule checking.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["curl"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that send data (always unsafe unless explicit GET).
const DATA_FLAGS: &[&str] = &[
    "-d",
    "--data",
    "--data-binary",
    "--data-raw",
    "--data-ascii",
    "--data-urlencode",
    "-F",
    "--form",
    "--form-string",
    "-T",
    "--upload-file",
    "--json",
];

/// Flags that are always unsafe.
const UNSAFE_FLAGS: &[&str] = &[
    "-K",
    "--config",
    "--ftp-create-dirs",
    "--mail-from",
    "--mail-rcpt",
];

/// Safe HTTP methods (read-only).
const SAFE_METHODS: &[&str] = &["GET", "HEAD", "OPTIONS", "TRACE"];

/// Safe FTP commands (read-only).
const SAFE_FTP_COMMANDS: &[&str] = &[
    "PWD", "LIST", "NLST", "STAT", "SIZE", "MDTM", "NOOP", "HELP", "SYST", "TYPE", "PASV", "CWD",
    "CDUP", "FEAT",
];

/// Python `str.isspace()` for one character.
fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Extract the output file from -o/--output flag.
fn extract_output_file(tokens: &[String]) -> Option<String> {
    for (i, t) in tokens.iter().enumerate() {
        if t == "-o" && i + 1 < tokens.len() {
            return Some(tokens[i + 1].clone());
        }
        if t.starts_with("-o") && t.chars().count() > 2 && !t.starts_with("-o=") {
            return Some(t[2..].to_string());
        }
        if t == "--output" && i + 1 < tokens.len() {
            return Some(tokens[i + 1].clone());
        }
        if let Some(rest) = t.strip_prefix("--output=") {
            return Some(rest.to_string());
        }
    }
    None
}

/// `tokens[i + 1].strip().strip("'\"").split()[0].upper()`; `None` where
/// Python raises IndexError (empty command).
fn ftp_command(arg: &str) -> Option<String> {
    let stripped = arg
        .trim_matches(py_isspace)
        .trim_matches(|c| c == '\'' || c == '"');
    stripped
        .split(py_isspace)
        .find(|s| !s.is_empty())
        .map(str::to_uppercase)
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("curl");
    for (i, t) in tokens.iter().enumerate() {
        let t = t.as_str();
        if UNSAFE_FLAGS.contains(&t) {
            return Classification::ask_desc(format!("{base} {t}"));
        }

        if DATA_FLAGS.contains(&t) {
            return Classification::ask_desc(format!("{base} {t}"));
        }

        // --flag=value variants (no flag is a prefix of another's "flag=").
        for flag in DATA_FLAGS {
            if t.starts_with(&format!("{flag}=")) {
                return Classification::ask_desc(format!("{base} {flag}"));
            }
        }

        if t == "-X" || t == "--request" {
            if let Some(next) = tokens.get(i + 1) {
                let method = next.to_uppercase();
                if !SAFE_METHODS.contains(&method.as_str()) {
                    return Classification::ask_desc(format!("{base} {method}"));
                }
            }
        }

        if let Some(rest) = t.strip_prefix("--request=") {
            let method = rest.to_uppercase();
            if !SAFE_METHODS.contains(&method.as_str()) {
                return Classification::ask_desc(format!("{base} {method}"));
            }
        }
        if t.starts_with("-X") && t.chars().count() > 2 && !t.starts_with("-X=") {
            let method = t[2..].to_uppercase();
            if !SAFE_METHODS.contains(&method.as_str()) {
                return Classification::ask_desc(format!("{base} {method}"));
            }
        }

        if t == "-Q" || t == "--quote" {
            if let Some(next) = tokens.get(i + 1) {
                match ftp_command(next) {
                    Some(cmd) if SAFE_FTP_COMMANDS.contains(&cmd.as_str()) => {}
                    // Unsafe command, or Python IndexError: fail closed.
                    _ => return Classification::ask_desc(format!("{base} {t}")),
                }
            }
        }
    }

    if let Some(output_file) = extract_output_file(tokens) {
        if !output_file.is_empty() && output_file != "-" && output_file != "/dev/null" {
            return Classification::allow_desc(base).redirects(vec![output_file]);
        }
    }

    Classification::allow_desc(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn safe_requests_allow() {
        for t in [
            vec!["curl", "https://example.com"],
            vec!["curl", "-I", "https://example.com"],
            vec!["curl", "-X", "GET", "https://example.com"],
            vec!["curl", "-XHEAD", "https://example.com"],
            vec!["curl", "--request=options", "https://example.com"],
            vec!["curl", "-Q", "PWD", "ftp://x"],
            vec!["curl", "-Q", "'list -a'", "ftp://x"],
            vec!["curl", "-o", "-", "https://example.com"],
            vec!["curl", "-o", "/dev/null", "https://example.com"],
        ] {
            let r = run(&t);
            assert_eq!(r.action, Action::Allow, "{t:?}");
            assert_eq!(r.redirect_targets, None, "{t:?}");
        }
    }

    #[test]
    fn data_and_methods_ask() {
        let cases: &[(&[&str], &str)] = &[
            (&["curl", "-d", "x", "u"], "curl -d"),
            (&["curl", "--data=x", "u"], "curl --data"),
            (&["curl", "--json={}", "u"], "curl --json"),
            (&["curl", "-X", "POST", "u"], "curl POST"),
            (&["curl", "-Xdelete", "u"], "curl DELETE"),
            (&["curl", "--request=PUT", "u"], "curl PUT"),
            (&["curl", "-K", "cfg"], "curl -K"),
            (&["curl", "-Q", "DELE file", "ftp://x"], "curl -Q"),
            (&["curl", "--quote", "  ", "ftp://x"], "curl --quote"),
        ];
        for (tokens, desc) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Ask, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some(*desc), "{tokens:?}");
        }
    }

    #[test]
    fn output_files_are_redirect_targets() {
        let cases: &[(&[&str], &str)] = &[
            (&["curl", "-o", "out.html", "u"], "out.html"),
            (&["curl", "-oout.html", "u"], "out.html"),
            (&["curl", "--output", "f", "u"], "f"),
            (&["curl", "--output=f", "u"], "f"),
            (&["curl", "-o", "/dev/stdout", "u"], "/dev/stdout"),
        ];
        for (tokens, target) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Allow, "{tokens:?}");
            assert_eq!(r.redirect_targets, Some(vec![target.to_string()]));
            assert_eq!(r.description.as_deref(), Some("curl"));
        }
    }
}
