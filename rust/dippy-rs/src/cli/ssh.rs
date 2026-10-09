//! Port of `src/dippy/cli/ssh.py`.
//!
//! Handles ssh with remote command execution. Delegates to inner command
//! check with `ssh` and exact target wrapper contexts.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["ssh"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const OPTIONS_WITH_ARG: &[&str] = &[
    "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o", "-p", "-Q",
    "-R", "-S", "-W", "-w",
];

/// Options that execute a local helper, write a file, create forwarding,
/// or manipulate a persistent control connection.
const RISKY_OPTIONS_WITH_ARG: &[&str] = &[
    "-D", "-E", "-F", "-I", "-J", "-L", "-O", "-R", "-S", "-W", "-o", "-w",
];
const RISKY_FLAGS: &[&str] = &["-A", "-f", "-K", "-M", "-N", "-X", "-Y"];

fn risky_option(token: &str) -> Option<String> {
    if !token.starts_with('-') || token.starts_with("--") {
        return None;
    }
    // OpenSSH accepts clusters such as -vA and -vL8080:host:80.
    for ch in token[1..].chars() {
        let option = format!("-{ch}");
        if RISKY_FLAGS.contains(&option.as_str())
            || RISKY_OPTIONS_WITH_ARG.contains(&option.as_str())
        {
            return Some(option);
        }
        if OPTIONS_WITH_ARG.contains(&option.as_str()) {
            break;
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::ask_desc("ssh (no target)");
    }

    let mut i = 1;
    let mut host: Option<&str> = None;
    while i < tokens.len() {
        let tok = tokens[i].as_str();
        if tok == "--" {
            i += 1;
            continue;
        } else if tok.starts_with('-') && host.is_none() {
            if let Some(risky) = risky_option(tok) {
                return Classification::ask_desc(format!("ssh option {risky}"));
            }
            if OPTIONS_WITH_ARG.contains(&tok) {
                i += 2;
            } else {
                i += 1;
            }
        } else if host.is_none() {
            host = Some(tok);
            i += 1;
        } else {
            break;
        }
    }

    // Python `if not host`: None or empty string.
    let host = match host {
        Some(h) if !h.is_empty() => h,
        _ => return Classification::ask_desc("ssh (no target)"),
    };

    if i >= tokens.len() {
        return Classification::ask_desc(format!("ssh {host}"));
    }

    if tokens[i] == "--" {
        i += 1;
    }

    if i >= tokens.len() {
        return Classification::ask_desc(format!("ssh {host}"));
    }

    // ssh concatenates its arguments with spaces for a remote shell; do not
    // re-quote (that would hide a remote compound command).
    let remote_cmd = tokens[i..].join(" ");

    Classification::delegate(remote_cmd)
        .desc(format!("ssh {host}"))
        .wrapper(vec!["ssh".into(), host.into()])
        .remote(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn sets_wrapper_context() {
        let r = run(&["ssh", "host", "rm", "/tmp/x"]);
        assert_eq!(r.action, Action::Delegate);
        assert_eq!(r.inner_command.as_deref(), Some("rm /tmp/x"));
        assert_eq!(r.wrapper_context, Some(vec!["ssh".into(), "host".into()]));
    }

    #[test]
    fn no_wrapper_context_for_interactive() {
        let r = run(&["ssh", "host"]);
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.wrapper_context, None);
        assert!(!r.remote);
    }

    #[test]
    fn delegates_with_remote_flag() {
        let r = run(&["ssh", "host", "ls", "/tmp"]);
        assert_eq!(r.action, Action::Delegate);
        assert!(r.remote);
    }

    #[test]
    fn options_and_separator() {
        let r = run(&["ssh", "-p", "22", "host", "--", "ls"]);
        assert_eq!(r.inner_command.as_deref(), Some("ls"));
        assert_eq!(r.description.as_deref(), Some("ssh host"));
        assert_eq!(
            run(&["ssh"]).description.as_deref(),
            Some("ssh (no target)")
        );
        assert_eq!(run(&["ssh", "host", "--"]).action, Action::Ask);
    }

    #[test]
    fn risky_options_ask() {
        for opt in [
            vec!["-o", "ProxyCommand=rm"],
            vec!["-L", "8080:localhost:80"],
            vec!["-R", "8080:localhost:80"],
            vec!["-vL8080:localhost:80"],
            vec!["-vR8080:localhost:80"],
            vec!["-vA"],
            vec!["-K"],
        ] {
            let mut t = vec!["ssh"];
            t.extend(opt);
            t.extend(["host", "tail /log/error"]);
            assert_eq!(run(&t).action, Action::Ask, "{t:?}");
        }
        // -p consumes the rest of the cluster
        assert_eq!(
            run(&["ssh", "-p2222A", "host", "ls"]).action,
            Action::Delegate
        );
    }

    #[test]
    fn joins_without_requoting() {
        let r = run(&["ssh", "host", "ls; rm -rf /"]);
        assert_eq!(r.inner_command.as_deref(), Some("ls; rm -rf /"));
    }
}
