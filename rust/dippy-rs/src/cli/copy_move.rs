//! Port of `src/dippy/cli/copy_move.py`: cp/mv route the written paths
//! through redirect rules.
//!
//! Both commands write files, so their destination belongs to the same
//! rules that govern `>` and `tee`. mv also removes its sources, which is a
//! write too. Without a matching redirect rule the analyzer asks.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["cp", "mv"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Options whose argument is the destination directory.
const TARGET_OPTS: &[&str] = &["-t", "--target-directory"];
/// Options that take a separate argument which is not a path we care about.
const ARG_OPTS: &[&str] = &["-S", "--suffix"];

/// Long options that never take a separate argument.
const KNOWN_LONG_FLAGS: &[&str] = &[
    "--archive",
    "--attributes-only",
    "--backup",
    "--copy-contents",
    "--debug",
    "--dereference",
    "--exchange",
    "--force",
    "--help",
    "--interactive",
    "--link",
    "--no-clobber",
    "--no-dereference",
    "--no-target-directory",
    "--one-file-system",
    "--parents",
    "--preserve",
    "--recursive",
    "--reflink",
    "--remove-destination",
    "--sparse",
    "--strip-trailing-slashes",
    "--symbolic-link",
    "--update",
    "--verbose",
    "--version",
];

/// Last component of a path, ignoring trailing slashes.
fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or("");
    if last.is_empty() {
        "/".into()
    } else {
        last.into()
    }
}

/// Split tokens into (positional args, explicit target dir, parsed_ok).
fn parse(tokens: &[String]) -> (Vec<String>, Option<String>, bool) {
    let mut positional: Vec<String> = Vec::new();
    let mut target: Option<String> = None;
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();

        if token == "--" {
            positional.extend(tokens[i + 1..].iter().cloned());
            break;
        }

        if token.starts_with("--") {
            let (name, value) = match token.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (token, None),
            };
            if TARGET_OPTS.contains(&name) {
                if let Some(v) = value {
                    target = Some(v.to_string());
                } else if i + 1 < tokens.len() {
                    target = Some(tokens[i + 1].clone());
                    i += 1;
                } else {
                    return (positional, target, false);
                }
            } else if ARG_OPTS.contains(&name) {
                if value.is_none() {
                    i += 1; // skip its argument
                }
            } else if value.is_none() && !KNOWN_LONG_FLAGS.contains(&name) {
                // Unknown long option: guessing wrong would shift which token
                // is the destination.
                return (positional, target, false);
            }
        } else if token.starts_with('-') && token != "-" {
            if TARGET_OPTS.contains(&token) {
                if i + 1 < tokens.len() {
                    target = Some(tokens[i + 1].clone());
                    i += 1;
                } else {
                    return (positional, target, false);
                }
            } else if ARG_OPTS.contains(&token) {
                i += 1; // skip its argument
            } else if token.ends_with('t') || token.ends_with('S') {
                // Clustered form such as -rt DIR / -bS .bak
                if i + 1 >= tokens.len() {
                    return (positional, target, false);
                }
                if token.ends_with('t') {
                    target = Some(tokens[i + 1].clone());
                }
                i += 1;
            }
        } else {
            positional.push(token.to_string());
        }

        i += 1;
    }

    (positional, target, true)
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("cp");

    let (positional, target, parsed_ok) = parse(tokens);
    if !parsed_ok {
        return Classification::ask_desc(base);
    }

    let (sources, destination, into_directory) = match target {
        Some(t) => (positional, t, true),
        None => {
            if positional.len() < 2 {
                return Classification::ask_desc(base);
            }
            let mut sources = positional;
            let destination = sources.pop().unwrap_or_default();
            // Only a trailing slash (or . / ..) marks a directory without
            // touching the filesystem.
            let into = destination.ends_with('/') || destination == "." || destination == "..";
            (sources, destination, into)
        }
    };

    if sources.is_empty() {
        return Classification::ask_desc(base);
    }

    let mut targets: Vec<String> = if into_directory {
        let trimmed = destination.trim_end_matches('/');
        let prefix = if trimmed.is_empty() { "/" } else { trimmed };
        sources
            .iter()
            .map(|src| format!("{prefix}/{}", basename(src)))
            .collect()
    } else {
        vec![destination]
    };

    // mv unlinks its sources, so those paths are written as well.
    if base == "mv" {
        targets.extend(sources);
    }

    Classification::allow().redirects(targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    fn targets(tokens: &[&str]) -> Vec<String> {
        let r = run(tokens);
        assert_eq!(r.action, Action::Allow, "{tokens:?}");
        assert_eq!(r.description, None);
        r.redirect_targets.unwrap()
    }

    #[test]
    fn destination_as_written() {
        assert_eq!(targets(&["cp", "a", "b"]), vec!["b"]);
        assert_eq!(targets(&["cp", "-r", "a", "b"]), vec!["b"]);
        assert_eq!(targets(&["mv", "a", "b"]), vec!["b", "a"]);
    }

    #[test]
    fn into_directory() {
        assert_eq!(
            targets(&["cp", "x/a", "y/b/", "dir/"]),
            vec!["dir/a", "dir/b"]
        );
        assert_eq!(
            targets(&["cp", "x/a", "y/b/", "dir//"]),
            vec!["dir/a", "dir/b"]
        );
        assert_eq!(targets(&["cp", "a", "/"]), vec!["//a"]);
        assert_eq!(targets(&["cp", "a", "."]), vec!["./a"]);
        assert_eq!(targets(&["cp", "/", "d/"]), vec!["d//"]);
        assert_eq!(targets(&["cp", "-t", "d", "a", "b"]), vec!["d/a", "d/b"]);
        assert_eq!(targets(&["cp", "--target-directory=d", "a"]), vec!["d/a"]);
        assert_eq!(targets(&["cp", "-rt", "d", "a"]), vec!["d/a"]);
        assert_eq!(targets(&["mv", "-t", "d", "a"]), vec!["d/a", "a"]);
    }

    #[test]
    fn options_with_arguments() {
        assert_eq!(targets(&["cp", "-S", ".bak", "a", "b"]), vec!["b"]);
        assert_eq!(targets(&["cp", "-bS", ".bak", "a", "b"]), vec!["b"]);
        assert_eq!(targets(&["cp", "--suffix=.b", "a", "b"]), vec!["b"]);
        assert_eq!(targets(&["cp", "--", "-a", "-b"]), vec!["-b"]);
        assert_eq!(targets(&["cp", "--unknown=x", "a", "b"]), vec!["b"]);
    }

    #[test]
    fn unparseable_asks() {
        for t in [
            vec!["cp"],
            vec!["cp", "a"],
            vec!["cp", "-t"],
            vec!["cp", "-rt"],
            vec!["cp", "--target-directory"],
            vec!["cp", "--unknown", "a", "b"],
            vec!["cp", "-t", "d"],
        ] {
            let r = run(&t);
            assert_eq!(r.action, Action::Ask, "{t:?}");
            assert_eq!(r.description.as_deref(), Some("cp"));
        }
    }
}
