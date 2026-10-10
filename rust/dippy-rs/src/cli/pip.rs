//! Port of `src/dippy/cli/pip.py`: Python package manager CLI handler.
//!
//! Handles pip, pip3, and uv commands.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["pip", "pip3"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_ACTIONS: &[&str] = &[
    "list",
    "freeze",
    "show",
    "search", // Deprecated but safe
    "check",
    "config",
    "help",
    "-h",
    "--help",
    "version",
    "-V",
    "--version",
    "debug",
    "cache",
    "index",
    "inspect", // Read-only environment inspection
    "hash",    // Read-only hash computation
];

/// `SAFE_SUBCOMMANDS` (dict order: cache, config, pip).
fn safe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "cache" => Some(&["dir", "info", "list"]),
        "config" => Some(&["list", "get", "debug"]),
        // uv specific
        "pip" => Some(&["list", "freeze", "show", "check"]),
        _ => None,
    }
}

/// `UNSAFE_SUBCOMMANDS`.
fn unsafe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "cache" => Some(&["purge", "remove"]),
        "config" => Some(&["set", "unset", "edit"]),
        "pip" => Some(&["install", "uninstall"]),
        _ => None,
    }
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        let base = tokens.first().map(String::as_str).unwrap_or("pip");
        return Classification::ask_desc(base);
    }

    let base = tokens[0].as_str();
    let mut action = tokens[1].as_str();
    let mut rest: &[String] = &tokens[2..];
    let mut desc = format!("{base} {action}");

    // Handle uv which wraps pip
    if base == "uv" {
        if action == "pip" {
            if let Some((first, tail)) = rest.split_first() {
                action = first.as_str();
                rest = tail;
                desc = format!("{base} pip {action}");
            } else {
                return Classification::ask_desc(desc);
            }
        } else if matches!(action, "run" | "tool" | "sync" | "lock" | "add" | "remove") {
            return Classification::ask_desc(desc); // uv-specific unsafe commands
        } else if matches!(action, "version" | "--version" | "-V" | "help" | "--help") {
            return Classification::allow_desc(desc);
        }
    }

    // Check subcommands
    if let Some(safe) = safe_subcommands(action)
        && !rest.is_empty()
    {
        for token in rest {
            if !token.starts_with('-') {
                if safe.contains(&token.as_str()) {
                    return Classification::allow_desc(format!("{desc} {token}"));
                }
                break;
            }
        }
    }

    if let Some(unsafe_subs) = unsafe_subcommands(action)
        && !rest.is_empty()
    {
        for token in rest {
            if !token.starts_with('-') {
                if unsafe_subs.contains(&token.as_str()) {
                    return Classification::ask_desc(format!("{desc} {token}"));
                }
                break;
            }
        }
    }

    if SAFE_ACTIONS.contains(&action) {
        return Classification::allow_desc(desc);
    }

    Classification::ask_desc(desc)
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn classify_cmd(cmd: &str) -> Classification {
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        classify(&HandlerContext::new(&tokens))
    }

    #[test]
    fn classifies() {
        for (cmd, allowed) in [
            ("pip", false),
            ("pip list", true),
            ("pip3 list --outdated", true),
            ("pip freeze", true),
            ("pip show requests", true),
            ("pip search foo", true),
            ("pip check", true),
            ("pip --version", true),
            ("pip -V", true),
            ("pip help install", true),
            ("pip debug", true),
            ("pip index versions requests", true),
            ("pip inspect", true),
            ("pip hash file.whl", true),
            ("pip cache dir", true),
            ("pip cache info", true),
            ("pip cache list", true),
            ("pip cache purge", false),
            ("pip cache remove foo", false),
            ("pip cache", true),
            ("pip config list", true),
            ("pip config get global.index-url", true),
            ("pip config set global.index-url x", false),
            ("pip config --user unset x", false),
            ("pip config edit", false),
            ("pip install requests", false),
            ("pip install -r requirements.txt", false),
            ("pip uninstall requests", false),
            ("pip download requests", false),
            ("pip wheel .", false),
            ("pip lock", false),
            ("pip unknown", false),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(classify_cmd(cmd).action, expected, "{cmd}");
        }
    }

    #[test]
    fn uv_branch() {
        // pip.py also handles tokens whose base is "uv".
        for (cmd, allowed, desc) in [
            ("uv pip", false, "uv pip"),
            ("uv pip list", true, "uv pip list"),
            ("uv pip install x", false, "uv pip install"),
            ("uv run python", false, "uv run"),
            ("uv sync", false, "uv sync"),
            ("uv --version", true, "uv --version"),
            ("uv cache dir", true, "uv cache dir"),
        ] {
            let c = classify_cmd(cmd);
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(c.action, expected, "{cmd}");
            assert_eq!(c.description.as_deref(), Some(desc), "{cmd}");
        }
    }

    #[test]
    fn descriptions() {
        assert_eq!(
            classify_cmd("pip cache -v dir").description.as_deref(),
            Some("pip cache dir")
        );
        assert_eq!(
            classify_cmd("pip config set x y").description.as_deref(),
            Some("pip config set")
        );
    }
}
