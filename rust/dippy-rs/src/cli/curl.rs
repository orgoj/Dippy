//! Port of `src/dippy/cli/curl.py`.
//!
//! Approves GET/HEAD requests, blocks data-sending operations. Every file
//! curl writes (-o, -O, -D, -c, --trace, ...) is returned in redirect_targets
//! for config rule checking; names curl only learns later ask.

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

/// Options that write the file named by their argument (besides -o).
const WRITE_FILE_FLAGS: &[&str] = &[
    "-D",
    "--dump-header",
    "-c",
    "--cookie-jar",
    "--trace",
    "--trace-ascii",
    "--stderr",
    "--libcurl",
    "--etag-save",
    "--hsts",
    "--alt-svc",
];

/// Short options that take an argument (curl 8.x): the rest of the word, or
/// the next word when the option ends the word.
const SHORT_WITH_ARG: &str = "AbCcDdEeFHKmoPQrTtUuwXYyz";

/// Long options whose argument is read here, with WRITE_FILE_FLAGS; other
/// long options are taken as flags, so an unknown option's argument only adds
/// a URL.
const LONG_WITH_ARG: &[&str] = &[
    "--request",
    "--quote",
    "--output",
    "--output-dir",
    "--url",
    "--write-out",
];

/// Python `str.isspace()` for one character.
fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// One parsed curl argument.
enum Arg {
    /// Option as written (`-o`, `--output`) and its argument.
    Opt(String, Option<String>),
    Url(String),
}

/// Split curl arguments the way curl does: short options bundle (`-sLo f`),
/// `--` ends the options, and `--name=value` is kept as name and value.
fn parse_args(tokens: &[String]) -> Vec<Arg> {
    let mut out = Vec::new();
    let mut words = tokens.iter().skip(1);
    let mut options = true;
    while let Some(t) = words.next() {
        if !options || t == "-" || !t.starts_with('-') {
            out.push(Arg::Url(t.clone()));
        } else if t == "--" {
            options = false;
        } else if t.starts_with("--") {
            let (name, value) = match t.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None if LONG_WITH_ARG.contains(&t.as_str())
                    || WRITE_FILE_FLAGS.contains(&t.as_str()) =>
                {
                    (t.as_str(), words.next().cloned())
                }
                None => (t.as_str(), None),
            };
            out.push(Arg::Opt(name.to_string(), value));
        } else {
            for (pos, c) in t.char_indices().skip(1) {
                if SHORT_WITH_ARG.contains(c) {
                    let attached = &t[pos + c.len_utf8()..];
                    let value = if attached.is_empty() {
                        words.next().cloned()
                    } else {
                        Some(attached.to_string())
                    };
                    out.push(Arg::Opt(format!("-{c}"), value));
                    break;
                }
                out.push(Arg::Opt(format!("-{c}"), None));
            }
        }
    }
    out
}

/// File name `-O` saves a URL as: the last path segment without query or
/// fragment; curl 8.10+ uses `curl_response` when it is empty.
fn remote_file_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let path = rest.split_once('/').map_or("", |(_, path)| path);
    match path.rsplit('/').next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => "curl_response".to_string(),
    }
}

/// `-o f#1`: curl substitutes URL glob matches into the name.
fn has_glob_reference(name: &str) -> bool {
    name.split('#')
        .skip(1)
        .any(|s| s.starts_with(|c: char| c.is_ascii_digit()))
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
    let ask = |what: &str| Classification::ask_desc(format!("{base} {what}"));
    let mut urls: Vec<String> = Vec::new();
    let mut outputs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut output_dir: Option<String> = None;
    let mut remote_name = false;
    for arg in parse_args(tokens) {
        let (name, value) = match arg {
            Arg::Url(url) => {
                urls.push(url);
                continue;
            }
            Arg::Opt(name, value) => (name, value),
        };
        // curl 8.3+ accepts every option as --expand-<name> with {{variables}}
        // in its argument, so neither the option nor its value is known here.
        if UNSAFE_FLAGS.contains(&name.as_str())
            || DATA_FLAGS.contains(&name.as_str())
            || name.starts_with("--expand-")
        {
            return ask(&name);
        }
        match (name.as_str(), value) {
            ("-X" | "--request", Some(method)) => {
                let method = method.to_uppercase();
                if !SAFE_METHODS.contains(&method.as_str()) {
                    return ask(&method);
                }
            }
            ("-Q" | "--quote", Some(quote)) => match ftp_command(&quote) {
                Some(cmd) if SAFE_FTP_COMMANDS.contains(&cmd.as_str()) => {}
                // Unsafe command, or Python IndexError: fail closed.
                _ => return ask(&name),
            },
            // The server names the file.
            ("-J" | "--remote-header-name", _) => return ask("-J"),
            ("-w" | "--write-out", Some(format)) if format.contains("%output{") => {
                return ask("-w");
            }
            ("-O" | "--remote-name" | "--remote-name-all", _) => remote_name = true,
            ("-o" | "--output", Some(file)) => outputs.push(file),
            ("--output-dir", Some(dir)) => output_dir = Some(dir),
            ("--url", Some(url)) => urls.push(url),
            (flag, Some(file)) if WRITE_FILE_FLAGS.contains(&flag) => files.push(file),
            _ => {}
        }
    }

    if outputs.iter().any(|f| has_glob_reference(f)) {
        return ask("-o");
    }
    if remote_name && urls.iter().any(|u| u.contains(['[', '{'])) {
        return ask("-O");
    }
    let in_output_dir = |file: String| match &output_dir {
        Some(dir) if !file.starts_with('/') => format!("{}/{file}", dir.trim_end_matches('/')),
        _ => file,
    };
    let remote_names = urls
        .iter()
        .filter(|_| remote_name)
        .map(|u| remote_file_name(u));
    let targets: Vec<String> = outputs
        .into_iter()
        .filter(|f| !f.is_empty() && f != "-" && f != "/dev/null")
        .chain(remote_names)
        .map(in_output_dir)
        .chain(files.into_iter().filter(|f| f != "-" && f != "/dev/null"))
        .collect();
    if targets.is_empty() {
        return Classification::allow_desc(base);
    }
    Classification::allow_desc(base).redirects(targets)
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

    fn targets(tokens: &[&str]) -> Vec<String> {
        let r = run(tokens);
        assert_eq!(r.action, Action::Allow, "{tokens:?}");
        r.redirect_targets.unwrap_or_default()
    }

    #[test]
    fn bundled_and_attached_output_files_are_redirect_targets() {
        for (tokens, target) in [
            (&["curl", "-sLo", "/tmp/x.deb", "u"][..], "/tmp/x.deb"),
            (&["curl", "-sLo/tmp/x.deb", "u"][..], "/tmp/x.deb"),
            (&["curl", "-o=f", "u"][..], "=f"),
            (&["curl", "-Lof", "u"][..], "f"),
            (&["curl", "u", "-o", "a", "v", "--output", "b"][..], "a b"),
        ] {
            assert_eq!(targets(tokens).join(" "), target, "{tokens:?}");
        }
        // An option argument is not an option: -H takes "o".
        assert_eq!(targets(&["curl", "-Ho", "u"]), Vec::<String>::new());
        assert_eq!(targets(&["curl", "--", "-o", "u"]), Vec::<String>::new());
    }

    #[test]
    fn remote_names_are_redirect_targets() {
        for (tokens, target) in [
            (&["curl", "-O", "https://x/a/.bashrc"][..], ".bashrc"),
            (&["curl", "-sLO", "https://x/f.deb?v=1#top"][..], "f.deb"),
            (&["curl", "--remote-name", "x.org/f"][..], "f"),
            (&["curl", "-O", "https://x/"][..], "curl_response"),
            (&["curl", "-O", "https://x"][..], "curl_response"),
            (&["curl", "--remote-name-all", "u/a", "u/b"][..], "a b"),
            (&["curl", "-O", "--url", "u/a"][..], "a"),
            (&["curl", "--output-dir", "/d", "-O", "u/a"][..], "/d/a"),
            (&["curl", "-o", "f", "--output-dir", "/d", "u"][..], "/d/f"),
            (
                &["curl", "--output-dir", "/d", "-o", "/abs", "u"][..],
                "/abs",
            ),
        ] {
            assert_eq!(targets(tokens).join(" "), target, "{tokens:?}");
        }
    }

    #[test]
    fn other_written_files_are_redirect_targets() {
        for flag in [
            "-D",
            "--dump-header",
            "-c",
            "--cookie-jar",
            "--trace",
            "--trace-ascii",
            "--stderr",
            "--libcurl",
            "--etag-save",
            "--hsts",
            "--alt-svc",
        ] {
            assert_eq!(targets(&["curl", flag, "f", "u"]), ["f"], "{flag}");
            assert_eq!(targets(&["curl", flag, "-", "u"]), Vec::<String>::new());
        }
        assert_eq!(targets(&["curl", "-sDh", "u"]), ["h"]);
        assert_eq!(
            targets(&["curl", "-o", "/dev/null", "u"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn names_curl_cannot_tell_in_advance_ask() {
        let cases: &[(&[&str], &str)] = &[
            (&["curl", "-OJ", "u/a"], "curl -J"),
            (&["curl", "--remote-header-name", "-O", "u/a"], "curl -J"),
            (&["curl", "-O", "u/f[1-3]"], "curl -O"),
            (&["curl", "-O", "u/{a,b}"], "curl -O"),
            (&["curl", "-o", "x#1", "u/{a,b}"], "curl -o"),
            (&["curl", "-w", "%output{/x}%{url}", "u"], "curl -w"),
            (&["curl", "--write-out", "%output{>>/x}", "u"], "curl -w"),
        ];
        for (tokens, desc) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Ask, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some(*desc), "{tokens:?}");
        }
    }

    #[test]
    fn bundled_data_and_methods_ask() {
        let cases: &[(&[&str], &str)] = &[
            (&["curl", "-sd", "x", "u"], "curl -d"),
            (&["curl", "-sdx", "u"], "curl -d"),
            (&["curl", "-sF", "a=b", "u"], "curl -F"),
            (&["curl", "-sXPOST", "u"], "curl POST"),
            (&["curl", "-sX", "delete", "u"], "curl DELETE"),
            (&["curl", "-sK", "cfg", "u"], "curl -K"),
            (&["curl", "-sQ", "DELE f", "ftp://x"], "curl -Q"),
        ];
        for (tokens, desc) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Ask, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some(*desc), "{tokens:?}");
        }
        assert_eq!(run(&["curl", "-sXGET", "u"]).action, Action::Allow);
    }

    #[test]
    fn variable_expanded_options_ask() {
        // curl 8.3+ accepts any option prefixed with --expand-.
        let cases: &[(&[&str], &str)] = &[
            (&["curl", "--expand-data=x", "u"], "curl --expand-data"),
            (
                &["curl", "--expand-data-binary", "x", "u"],
                "curl --expand-data-binary",
            ),
            (
                &["curl", "--expand-output", "f", "u"],
                "curl --expand-output",
            ),
            (
                &["curl", "--expand-request", "POST", "u"],
                "curl --expand-request",
            ),
            (&["curl", "--expand-url", "{{u}}"], "curl --expand-url"),
        ];
        for (tokens, desc) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Ask, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some(*desc), "{tokens:?}");
        }
    }
}
