//! Port of `src/dippy/cli/npm.py`: Node package manager CLI handler.
//!
//! Handles npm, yarn, and pnpm commands.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["npm", "yarn", "pnpm"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_ACTIONS: &[&str] = &[
    "list",
    "ls",
    "ll",
    "la",
    "info",
    "show",
    "view",
    "v",
    "search",
    "s",
    "find",
    "outdated",
    "help",
    "help-search",
    "-v",
    "--version",
    "get",
    "root",
    "prefix",
    "bin",
    "docs",
    "home",
    "bugs",
    "repo",
    "whoami",
    "ping",
    "explain",
    "why",
    "pack", // Creates tarball but doesn't publish
    "fund",
    "doctor",   // Health check
    "licenses", // yarn/pnpm licenses list
    "completion",
    "diff",
    "find-dupes",
    "query",
    "stars",
    "sbom",
];

/// Commands with subcommands that need special handling (`SAFE_SUBCOMMANDS`).
fn safe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "config" => Some(&["list", "ls", "get"]),
        "cache" => Some(&["ls", "list"]),
        "run" => Some(&["--list"]),
        "access" => Some(&["list", "get"]),
        "dist-tag" => Some(&["ls"]),
        "token" => Some(&["list"]),
        "profile" => Some(&["get"]),
        "pkg" => Some(&["get"]),
        "owner" => Some(&["ls"]),
        _ => None,
    }
}

/// Commands with unsafe subcommands (`UNSAFE_SUBCOMMANDS`).
fn unsafe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "config" => Some(&["set", "delete", "edit"]),
        "cache" => Some(&["clean", "add", "verify"]),
        "access" => Some(&["set", "grant", "revoke"]),
        "dist-tag" => Some(&["add", "rm"]),
        "token" => Some(&["create", "revoke"]),
        "profile" => Some(&["set", "enable-2fa", "disable-2fa"]),
        "pkg" => Some(&["set", "delete", "fix"]),
        "owner" => Some(&["add", "rm"]),
        "audit" => Some(&["fix"]),
        "version" => Some(&[
            "major",
            "minor",
            "patch",
            "premajor",
            "preminor",
            "prepatch",
            "prerelease",
        ]),
        _ => None,
    }
}

/// Short aliases that need expansion for clarity (`ACTION_ALIASES`).
fn action_alias(action: &str) -> &str {
    match action {
        "i" => "install",
        "rm" => "remove",
        "un" => "uninstall",
        "r" => "remove",
        "x" => "exec",
        "t" => "test",
        "s" => "search",
        "ddp" => "dedupe",
        "rb" => "rebuild",
        "c" => "config",
        other => other,
    }
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("npm");
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    let action = tokens[1].as_str();
    let rest: &[String] = &tokens[2..];
    // Expand short aliases for clarity in description
    let display_action = action_alias(action);
    let desc = format!("{base} {display_action}");

    // Handle "npm run" without arguments (just lists scripts)
    if action == "run" && rest.is_empty() {
        return Classification::allow_desc(desc);
    }

    // Handle "npm run --list" (safe)
    if action == "run" && rest.iter().any(|t| t == "--list") {
        return Classification::allow_desc(desc);
    }

    // Handle "npm version" - without arguments shows version, with args modifies
    if action == "version" {
        if rest.is_empty() {
            return Classification::allow_desc(desc);
        }
        return Classification::ask_desc(desc);
    }

    // Handle "npm audit" - safe for viewing, but "audit fix" is unsafe
    if action == "audit" {
        if rest.first().is_some_and(|t| t == "fix") {
            return Classification::ask_desc(format!("{desc} fix"));
        }
        return Classification::allow_desc(desc);
    }

    // Handle "npm config" / "npm c"
    if action == "config" || action == "c" {
        if let Some(subaction) = rest.first() {
            let subaction = subaction.as_str();
            if safe_subcommands("config").is_some_and(|s| s.contains(&subaction)) {
                return Classification::allow_desc(format!("{desc} {subaction}"));
            }
            if unsafe_subcommands("config").is_some_and(|s| s.contains(&subaction)) {
                return Classification::ask_desc(format!("{desc} {subaction}"));
            }
        }
        return Classification::allow_desc(desc); // "npm config" alone shows help
    }

    // Check commands with safe/unsafe subcommands
    if let Some(safe) = safe_subcommands(action) {
        if let Some(subaction) = rest.first() {
            let subaction = subaction.as_str();
            if safe.contains(&subaction) {
                return Classification::allow_desc(format!("{desc} {subaction}"));
            }
            if unsafe_subcommands(action).is_some_and(|s| s.contains(&subaction)) {
                return Classification::ask_desc(format!("{desc} {subaction}"));
            }
        }
        if action == "owner" {
            return Classification::allow_desc(desc); // "npm owner" alone lists
        }
        return Classification::ask_desc(desc);
    }

    if unsafe_subcommands(action).is_some() {
        return Classification::ask_desc(desc);
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
            ("npm", false),
            ("npm list", true),
            ("npm ls --depth=0", true),
            ("yarn info react", true),
            ("pnpm why lodash", true),
            ("npm view react version", true),
            ("npm outdated", true),
            ("npm --version", true),
            ("npm -v", true),
            ("npm pack", true),
            ("npm doctor", true),
            ("npm run", true),
            ("npm run --list", true),
            ("npm run build", false),
            ("npm run test --list", true),
            ("npm version", true),
            ("npm version patch", false),
            ("npm version 1.2.3", false),
            ("npm audit", true),
            ("npm audit --json", true),
            ("npm audit fix", false),
            ("npm config", true),
            ("npm config list", true),
            ("npm config get registry", true),
            ("npm c ls", true),
            ("npm config set registry x", false),
            ("npm config delete x", false),
            ("npm config edit", false),
            ("npm config unknown", true),
            ("npm cache ls", true),
            ("npm cache clean --force", false),
            ("npm cache verify", false),
            ("npm cache", false),
            ("npm access list packages", true),
            ("npm access grant x", false),
            ("npm dist-tag ls", true),
            ("npm dist-tag add x", false),
            ("npm token list", true),
            ("npm token create", false),
            ("npm profile get", true),
            ("npm profile set x y", false),
            ("npm pkg get name", true),
            ("npm pkg set name=x", false),
            ("npm owner", true),
            ("npm owner ls react", true),
            ("npm owner add user pkg", false),
            ("npm owner other", true),
            ("npm install", false),
            ("npm i react", false),
            ("npm ci", false),
            ("yarn add react", false),
            ("pnpm remove react", false),
            ("npm exec foo", false),
            ("npm x foo", false),
            ("npm test", false),
            ("npm publish", false),
            ("npm unknown", false),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(classify_cmd(cmd).action, expected, "{cmd}");
        }
    }

    #[test]
    fn descriptions() {
        for (cmd, desc) in [
            ("npm i react", "npm install"),
            ("npm c get x", "npm config get"),
            ("yarn audit fix", "yarn audit fix"),
            ("pnpm rb", "pnpm rebuild"),
            ("npm cache clean", "npm cache clean"),
        ] {
            assert_eq!(
                classify_cmd(cmd).description.as_deref(),
                Some(desc),
                "{cmd}"
            );
        }
    }
}
