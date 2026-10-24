//! Port of `src/dippy/cli/wget.py`.
//!
//! Wget downloads files by default, so most operations are unsafe. Only
//! --spider mode and a download to -O/--output-document are approved. Every
//! file wget writes is returned in redirect_targets for config rule checking;
//! data sends and executed commands ask.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["wget"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Long options that take an argument (GNU Wget 1.21 `--help`).
const LONG_WITH_ARG: &[&str] = &[
    "accept",
    "accept-regex",
    "append-output",
    "backups",
    "base",
    "bind-address",
    "body-data",
    "body-file",
    "ca-certificate",
    "ca-directory",
    "certificate",
    "certificate-type",
    "ciphers",
    "compression",
    "config",
    "connect-timeout",
    "crl-file",
    "cut-dirs",
    "default-page",
    "directory-prefix",
    "dns-timeout",
    "domains",
    "exclude-directories",
    "exclude-domains",
    "execute",
    "follow-tags",
    "ftp-password",
    "ftp-user",
    "header",
    "hsts-file",
    "http-password",
    "http-user",
    "ignore-tags",
    "include-directories",
    "input-file",
    "level",
    "limit-rate",
    "load-cookies",
    "local-encoding",
    "method",
    "output-document",
    "output-file",
    "password",
    "pinnedpubkey",
    "post-data",
    "post-file",
    "prefer-family",
    "private-key",
    "private-key-type",
    "progress",
    "proxy-password",
    "proxy-user",
    "quota",
    "random-file",
    "read-timeout",
    "referer",
    "regex-type",
    "reject",
    "rejected-log",
    "reject-regex",
    "remote-encoding",
    "report-speed",
    "restrict-file-names",
    "retry-on-http-error",
    "save-cookies",
    "secure-protocol",
    "start-pos",
    "timeout",
    "tries",
    "use-askpass",
    "user",
    "user-agent",
    "wait",
    "waitretry",
    "warc-dedup",
    "warc-file",
    "warc-header",
    "warc-max-size",
    "warc-tempdir",
];

/// Argument-less long options this handler reads.
const LONG_FLAGS: &[&str] = &["spider", "background"];

/// Short options that take an argument (`-nv`: `-n` takes `v`).
const SHORT_WITH_ARG: &str = "aABDeiIlnoOPQRtTUwX";

/// Short options this handler reads, by their long name.
const SHORT_NAMES: &[(char, &str)] = &[
    ('O', "output-document"),
    ('o', "output-file"),
    ('a', "append-output"),
    ('e', "execute"),
    ('b', "background"),
];

/// Options that write the file named by their argument.
const WRITE_FILE_FLAGS: &[&str] = &[
    "output-document",
    "output-file",
    "append-output",
    "save-cookies",
    "rejected-log",
    "hsts-file",
];

/// Options that send data, run a command or a config, or write files the
/// handler cannot name (`--warc-file` adds suffixes, `-b` writes wget-log*).
const ASK_FLAGS: &[&str] = &[
    "post-data",
    "post-file",
    "body-data",
    "body-file",
    "execute",
    "config",
    "use-askpass",
    "warc-file",
    "background",
];

/// Safe HTTP methods (read-only).
const SAFE_METHODS: &[&str] = &["GET", "HEAD", "OPTIONS", "TRACE"];

/// `getopt_long` resolution: an exact name, else the one known option it is
/// a prefix of. An ambiguous or unknown name stays as written (wget exits).
fn long_name(name: &str) -> String {
    let known = || LONG_WITH_ARG.iter().chain(LONG_FLAGS);
    if known().any(|k| *k == name) {
        return name.to_string();
    }
    let mut matches = known().filter(|k| k.starts_with(name));
    match (matches.next(), matches.next()) {
        (Some(only), None) if !name.is_empty() => only.to_string(),
        _ => name.to_string(),
    }
}

/// Split wget arguments the way `getopt_long` does: short options bundle
/// (`-qO f`), long options may be abbreviated, `--` ends the options.
/// Returns the options with their argument: the long name without `--`
/// (short options this handler reads are mapped to it; others stay `-c`).
fn parse_options(tokens: &[String]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut words = tokens.iter().skip(1);
    while let Some(t) = words.next() {
        if t == "--" {
            break;
        } else if t == "-" || !t.starts_with('-') {
            continue;
        } else if let Some(long) = t.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, value)) => (long_name(name), Some(value.to_string())),
                None => {
                    let name = long_name(long);
                    let value = if LONG_WITH_ARG.contains(&name.as_str()) {
                        words.next().cloned()
                    } else {
                        None
                    };
                    (name, value)
                }
            };
            out.push((name, value));
        } else {
            for (pos, c) in t.char_indices().skip(1) {
                let name = SHORT_NAMES
                    .iter()
                    .find(|(short, _)| *short == c)
                    .map_or_else(|| format!("-{c}"), |(_, long)| long.to_string());
                if SHORT_WITH_ARG.contains(c) {
                    let attached = &t[pos + c.len_utf8()..];
                    let value = if attached.is_empty() {
                        words.next().cloned()
                    } else {
                        Some(attached.to_string())
                    };
                    out.push((name, value));
                    break;
                }
                out.push((name, None));
            }
        }
    }
    out
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("wget");
    let mut spider = false;
    let mut output_document = false;
    let mut targets: Vec<String> = Vec::new();
    for (name, value) in parse_options(tokens) {
        if ASK_FLAGS.contains(&name.as_str()) {
            return Classification::ask_desc(format!("{base} --{name}"));
        }
        match (name.as_str(), value) {
            ("method", Some(method)) => {
                let method = method.to_uppercase();
                if !SAFE_METHODS.contains(&method.as_str()) {
                    return Classification::ask_desc(format!("{base} {method}"));
                }
            }
            ("spider", _) => spider = true,
            (flag, Some(file)) if WRITE_FILE_FLAGS.contains(&flag) && !file.is_empty() => {
                output_document |= flag == "output-document";
                targets.push(file);
            }
            _ => {}
        }
    }

    let allow = if spider {
        Classification::allow_desc(format!("{base} --spider"))
    } else if output_document {
        Classification::allow_desc(format!("{base} download"))
    } else {
        return Classification::ask_desc(format!("{base} download"));
    };
    if targets.is_empty() {
        allow
    } else {
        allow.redirects(targets)
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

    #[test]
    fn written_files_are_redirect_targets() {
        for (tokens, targets) in [
            (&["wget", "-qO", "f", "u"][..], "f"),
            (&["wget", "-qOf", "u"][..], "f"),
            (&["wget", "-qO-", "u"][..], "-"),
            (&["wget", "-nv", "-O", "f", "u"][..], "f"),
            (&["wget", "--output-doc=f", "u"][..], "f"),
            (&["wget", "-O", "f", "-o", "log", "u"][..], "f log"),
            (&["wget", "-O", "f", "--output-f", "log", "u"][..], "f log"),
            (&["wget", "-O", "f", "-a", "log", "u"][..], "f log"),
            (&["wget", "-O", "f", "--save-cookies", "c", "u"][..], "f c"),
            (&["wget", "-O", "f", "--rejected-log=r", "u"][..], "f r"),
            (&["wget", "-O", "f", "--hsts-file", "h", "u"][..], "f h"),
            (&["wget", "-O", "f", "--", "-o", "u"][..], "f"),
            (&["wget", "--spider", "-o", "log", "u"][..], "log"),
        ] {
            let r = run(tokens);
            assert_eq!(r.action, Action::Allow, "{tokens:?}");
            let got = r.redirect_targets.unwrap_or_default().join(" ");
            assert_eq!(got, targets, "{tokens:?}");
        }
        assert_eq!(run(&["wget", "--spid", "u"]).action, Action::Allow);
        let r = run(&["wget", "-O", "-", "--method=get", "u"]);
        assert_eq!(r.action, Action::Allow);
    }

    #[test]
    fn data_sends_and_commands_ask() {
        let cases: &[(&[&str], &str)] = &[
            (
                &["wget", "-O", "-", "--post-data=x", "u"],
                "wget --post-data",
            ),
            (&["wget", "-O", "-", "--post-d=x", "u"], "wget --post-data"),
            (
                &["wget", "--spider", "--post-file", "f", "u"],
                "wget --post-file",
            ),
            (
                &["wget", "-O", "-", "--body-data", "x", "u"],
                "wget --body-data",
            ),
            (
                &["wget", "-O", "-", "--body-file=f", "u"],
                "wget --body-file",
            ),
            (&["wget", "-O", "-", "--method", "post", "u"], "wget POST"),
            (
                &["wget", "-qe", "robots=off", "-O", "f", "u"],
                "wget --execute",
            ),
            (&["wget", "--config", "c", "--spider", "u"], "wget --config"),
            (
                &["wget", "--use-askpass", "x", "--spider", "u"],
                "wget --use-askpass",
            ),
            (
                &["wget", "--warc-file", "w", "-O", "f", "u"],
                "wget --warc-file",
            ),
            (&["wget", "-bO", "f", "u"], "wget --background"),
            // An option argument is not an option.
            (&["wget", "--header", "--spider", "u"], "wget download"),
            (&["wget", "-U", "--spider", "u"], "wget download"),
            (&["wget", "--", "--spider", "u"], "wget download"),
        ];
        for (tokens, desc) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Ask, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some(*desc), "{tokens:?}");
        }
    }
}
