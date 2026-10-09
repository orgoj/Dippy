//! Port of `src/dippy/cli/ip.py`.
//!
//! The ip command is safe for viewing network info, but modification
//! commands need confirmation.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["ip"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe ip subcommands (read-only).
const SAFE_SUBCOMMANDS: &[&str] = &[
    "addr", "address", "a", "link", "l", "route", "r", "rule", "ru", "neigh", "neighbor", "n",
    "tunnel", "tuntap", "tunt", "maddr", "maddress", "m", "mroute", "monitor", "mo", "netns",
    "netconf", "netc", "stats", "st",
];

/// Subcommand actions that modify state.
const MODIFY_ACTIONS: &[&str] = &[
    "add", "del", "delete", "change", "replace", "set", "flush", "exec",
];

/// Global flags that take an argument (need to skip).
const GLOBAL_FLAGS_WITH_ARG: &[&str] = &[
    "-n", "-netns", "--netns", "-b", "-batch", "--batch", "-rc", "-rcvbuf", "--rcvbuf",
];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("ip");
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    let mut parts: Vec<&str> = Vec::new();
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if GLOBAL_FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        parts.push(token);
        i += 1;
    }

    let Some(subcommand) = parts.first() else {
        return Classification::allow_desc(base); // Just "ip -flags"
    };
    let desc = format!("{base} {subcommand}");

    for part in &parts[1..] {
        if MODIFY_ACTIONS.contains(part) {
            return Classification::ask_desc(format!("{desc} {part}"));
        }
    }

    if SAFE_SUBCOMMANDS.contains(subcommand) {
        return Classification::allow_desc(desc);
    }
    Classification::ask_desc(desc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn read_only_allows() {
        for t in [
            vec!["ip", "addr"],
            vec!["ip", "a"],
            vec!["ip", "-4", "addr", "show"],
            vec!["ip", "-br", "link"],
            vec!["ip", "route", "show"],
            vec!["ip", "-n", "ns1", "addr"],
            vec!["ip", "netns", "list"],
            vec!["ip", "-s"],
        ] {
            assert_eq!(run(&t).action, Action::Allow, "{t:?}");
        }
    }

    #[test]
    fn modifications_ask() {
        let cases: &[(&[&str], &str)] = &[
            (
                &["ip", "addr", "add", "10.0.0.1/24", "dev", "eth0"],
                "ip addr add",
            ),
            (&["ip", "link", "set", "eth0", "up"], "ip link set"),
            (&["ip", "route", "flush", "all"], "ip route flush"),
            (&["ip", "netns", "exec", "ns1", "bash"], "ip netns exec"),
            (&["ip", "xfrm", "state"], "ip xfrm"),
            (&["ip"], "ip"),
        ];
        for (tokens, desc) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Ask, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some(*desc));
        }
    }
}
