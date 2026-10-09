//! Port of `src/dippy/cli/gh.py`: approve read-only GitHub CLI operations.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["gh"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Actions that only read data (`SAFE_ACTIONS`).
const SAFE_ACTIONS: &[&str] = &[
    "list",
    "view",
    "status",
    "diff",
    "checks",
    "get",
    "search",
    "download",
    "watch",
    "verify",
    "verify-asset",
    "trusted-root",
    "token",
    "logs",
    "ports",
    "field-list",
    "item-list",
    "check",
];

/// Flags that take an argument (`FLAGS_WITH_ARG`).
const FLAGS_WITH_ARG: &[&str] = &["-R", "--repo", "-B", "--branch"];

/// Get the action from a gh command, skipping global flags.
fn get_action(tokens: &[String]) -> Option<&str> {
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        if let Some(next_token) = tokens.get(i + 1)
            && !next_token.starts_with('-')
        {
            return Some(next_token);
        }
        return Some(token);
    }
    None
}

/// `gh api`: approve GET requests, block mutations.
fn check_api(tokens: &[String]) -> bool {
    let args: &[String] = if tokens.len() > 2 { &tokens[2..] } else { &[] };

    let mut method: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "-X" || arg == "--method" {
            if let Some(m) = args.get(i + 1) {
                method = Some(m.to_uppercase());
            }
            i += 2;
        } else if let Some(rest) = arg.strip_prefix("-X")
            && !rest.is_empty()
        {
            method = Some(rest.to_uppercase());
            i += 1;
        } else if let Some(rest) = arg.strip_prefix("--method=") {
            method = Some(rest.to_uppercase());
            i += 1;
        } else {
            i += 1;
        }
    }

    if method.as_deref().is_some_and(|m| m != "GET") {
        return false;
    }

    let mut is_graphql_query = false;
    for (i, arg) in args.iter().enumerate() {
        if (arg == "-f" || arg == "--raw-field")
            && let Some(val) = args.get(i + 1)
            && let Some(query_content) = val.strip_prefix("query=")
        {
            let lower = query_content.to_lowercase();
            if lower.contains("mutation") {
                return false;
            }
            is_graphql_query = lower.contains("query") || query_content.contains('{');
        }
        if arg.starts_with("--raw-field=query=") || arg.starts_with("-f=query=") {
            // `arg.split("=", 2)[2]`: everything after the second '='.
            let query_content = arg.splitn(3, '=').nth(2).unwrap_or("");
            let lower = query_content.to_lowercase();
            if lower.contains("mutation") {
                return false;
            }
            is_graphql_query = lower.contains("query") || query_content.contains('{');
        }
    }

    if is_graphql_query {
        return true;
    }

    let has_mutation_flags = args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-f" | "--raw-field" | "-F" | "--field" | "--input"
        ) || arg.starts_with("--raw-field=")
            || arg.starts_with("--field=")
            || arg.starts_with("--input=")
    });

    if has_mutation_flags && method.as_deref() != Some("GET") {
        return false;
    }
    true
}

/// Get the subcommand from a gh command, skipping global flags.
fn get_subcommand(tokens: &[String]) -> Option<&str> {
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(token);
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("gh");
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    // Python: `if not subcommand:` - an empty token is falsy.
    let Some(subcommand) = get_subcommand(tokens).filter(|s| !s.is_empty()) else {
        return Classification::ask_desc(base);
    };

    let mut desc = format!("{base} {subcommand}");

    if subcommand == "api" {
        if check_api(tokens) {
            return Classification::allow_desc(desc);
        }
        return Classification::ask_desc(desc);
    }

    if matches!(subcommand, "status" | "browse" | "search") {
        return Classification::allow_desc(desc);
    }

    let action = get_action(tokens);
    if let Some(action) = action.filter(|a| !a.is_empty()) {
        desc = format!("{base} {subcommand} {action}");
    }
    if action.is_some_and(|a| SAFE_ACTIONS.contains(&a)) {
        return Classification::allow_desc(desc);
    }
    Classification::ask_desc(desc)
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn run(cmd: &str) -> Classification {
        let tokens: Vec<&str> = cmd.split(' ').collect();
        classify(&HandlerContext::new(&tokens))
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn read_only_allows() {
        for cmd in [
            "gh pr list",
            "gh pr view 123",
            "gh issue list --state open",
            "gh run watch 1",
            "gh -R owner/repo pr list",
            "gh repo view",
            "gh status",
            "gh browse",
            "gh search code foo",
            "gh auth token",
            "gh project item-list 1",
            "gh ruleset check",
            "gh release download v1",
        ] {
            assert_eq!(run(cmd).action, Action::Allow, "{cmd}");
        }
        assert_eq!(run("gh pr list").description.as_deref(), Some("gh pr list"));
    }

    #[test]
    fn mutations_ask() {
        for cmd in [
            "gh",
            "gh pr create",
            "gh pr merge 1",
            "gh issue comment 1",
            "gh repo delete x",
            "gh alias import aliases.yml",
            "gh --repo",
            "gh -R x",
        ] {
            assert_eq!(run(cmd).action, Action::Ask, "{cmd}");
        }
        // Action is the token after the subcommand unless it is a flag.
        assert_eq!(run("gh pr --web").description.as_deref(), Some("gh pr pr"));
    }

    #[test]
    fn api_methods() {
        assert!(check_api(&s(&["gh", "api", "repos/o/r"])));
        assert!(check_api(&s(&["gh", "api", "-X", "get", "x"])));
        assert!(check_api(&s(&["gh", "api", "--method=GET", "x"])));
        assert!(!check_api(&s(&["gh", "api", "-XPOST", "x"])));
        assert!(!check_api(&s(&["gh", "api", "--method", "DELETE", "x"])));
        assert!(!check_api(&s(&["gh", "api", "x", "-f", "a=b"])));
        assert!(!check_api(&s(&["gh", "api", "x", "--input=f.json"])));
        assert!(check_api(&s(&["gh", "api", "-X", "GET", "x", "-F", "a=b"])));
        // A later -X POST overrides an earlier GET.
        assert!(!check_api(&s(&[
            "gh", "api", "-X", "GET", "-X", "POST", "x"
        ])));
    }

    #[test]
    fn api_graphql() {
        assert!(check_api(&s(&[
            "gh",
            "api",
            "graphql",
            "-f",
            "query={ viewer { login } }"
        ])));
        assert!(!check_api(&s(&[
            "gh",
            "api",
            "graphql",
            "-f",
            "query=Mutation { x }"
        ])));
        assert!(check_api(&s(&[
            "gh",
            "api",
            "graphql",
            "--raw-field=query=query{a}"
        ])));
        assert!(!check_api(&s(&[
            "gh",
            "api",
            "graphql",
            "-f=query=mutation{a}"
        ])));
        // A later non-query field resets the GraphQL flag only via query=.
        assert!(!check_api(&s(&[
            "gh", "api", "graphql", "-f", "query=x", "-f", "a=b"
        ])));
    }
}
