//! Port of `src/dippy/cli/auth0.py`.
//!
//! Auth0 commands for identity management.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["auth0"];
pub const PORTED: bool = true;
/// The Python module defines no `get_description`.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe Auth0 actions (read-only).
const SAFE_ACTION_KEYWORDS: &[&str] = &[
    "list",
    "ls",
    "show",
    "get",
    "search",
    "search-by-email",
    "tail",  // logs tail
    "diff",  // actions diff
    "stats", // event-streams stats
    "--help",
    "-h", // help flags
];

const UNSAFE_ACTION_KEYWORDS: &[&str] = &[
    "create",
    "delete",
    "update",
    "import",
    "export",
    "rm", // alias for delete
    "add",
    "remove",    // for permissions
    "download",  // quickstarts download
    "use",       // tenants use
    "customize", // universal-login customize
    "verify",    // domains verify
    "deploy",    // actions deploy
    "enable",
    "disable", // rules enable/disable
];

/// Global flags that take an argument.
const GLOBAL_FLAGS_WITH_ARG: &[&str] = &["--tenant", "-t", "--debug"];

/// Check auth0 api command - approve GET requests only.
fn check_api(tokens: &[String]) -> bool {
    let args: &[String] = tokens.get(2..).unwrap_or(&[]);
    for arg in args {
        if matches!(arg.as_str(), "post" | "put" | "patch" | "delete") {
            return false;
        }
        if matches!(arg.as_str(), "-d" | "--data") {
            return false;
        }
    }
    true
}

/// Classify auth0 command.
pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map_or("auth0", String::as_str);
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    let parts = extract_parts(&tokens[1..]);
    if parts.is_empty() {
        return Classification::ask_desc(base);
    }

    let subcommand = parts[0];
    let desc = format!("{base} {subcommand}");

    if subcommand == "api" {
        if check_api(tokens) {
            return Classification::allow_desc(desc);
        }
        return Classification::ask_desc(desc);
    }

    if parts.iter().any(|p| SAFE_ACTION_KEYWORDS.contains(p)) {
        return Classification::allow_desc(desc);
    }

    if parts.iter().any(|p| UNSAFE_ACTION_KEYWORDS.contains(p)) {
        return Classification::ask_desc(desc);
    }

    Classification::ask_desc(desc)
}

/// Extract command parts, keeping help flags but skipping other flags.
fn extract_parts(tokens: &[String]) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if token.starts_with('-') {
            // Keep help flags as they affect safety decision
            if token == "--help" || token == "-h" {
                parts.push(token);
            } else if GLOBAL_FLAGS_WITH_ARG.contains(&token) && i + 1 < tokens.len() {
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        parts.push(token);
        i += 1;
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_with_data_flag_asks() {
        let r = classify(&HandlerContext::new(&[
            "auth0", "api", "clients", "-d", "{}",
        ]));
        assert_eq!(r.action.as_str(), "ask");
        let r = classify(&HandlerContext::new(&["auth0", "api", "clients"]));
        assert_eq!(r.action.as_str(), "allow");
    }

    /// Generated from the Python handler on the commands of
    /// `tests/cli/test_auth0.py` (see `rust/parity/cases/auth0.txt`).
    #[allow(clippy::type_complexity)]
    const PY_CASES: &[(&[&str], &[&str], &[bool], &str, Option<&str>)] = &[
        (
            &["auth0", "apps", "list"],
            &["auth0", "apps", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "ls"],
            &["auth0", "apps", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "show", "app_12345"],
            &["auth0", "apps", "show", "app_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "create"],
            &["auth0", "apps", "create"],
            &[false, false, false],
            "ask",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "create", "--name", "myapp"],
            &["auth0", "apps", "create", "--name", "myapp"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "update", "app_12345"],
            &["auth0", "apps", "update", "app_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "delete", "app_12345"],
            &["auth0", "apps", "delete", "app_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "rm", "app_12345"],
            &["auth0", "apps", "rm", "app_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "open", "app_12345"],
            &["auth0", "apps", "open", "app_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "users", "search"],
            &["auth0", "users", "search"],
            &[false, false, false],
            "allow",
            Some("auth0 users"),
        ),
        (
            &[
                "auth0",
                "users",
                "search",
                "--query",
                "email:user@example.com",
            ],
            &[
                "auth0",
                "users",
                "search",
                "--query",
                "email:user@example.com",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 users"),
        ),
        (
            &["auth0", "users", "search-by-email", "user@example.com"],
            &["auth0", "users", "search-by-email", "user@example.com"],
            &[false, false, false, false],
            "allow",
            Some("auth0 users"),
        ),
        (
            &["auth0", "users", "show", "user_12345"],
            &["auth0", "users", "show", "user_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 users"),
        ),
        (
            &["auth0", "users", "create"],
            &["auth0", "users", "create"],
            &[false, false, false],
            "ask",
            Some("auth0 users"),
        ),
        (
            &["auth0", "users", "update", "auth0"],
            &["auth0", "users", "update", "auth0"],
            &[false, false, false, false],
            "ask",
            Some("auth0 users"),
        ),
        (
            &["auth0", "users", "delete", "auth0"],
            &["auth0", "users", "delete", "auth0"],
            &[false, false, false, false],
            "ask",
            Some("auth0 users"),
        ),
        (
            &["auth0", "users", "import"],
            &["auth0", "users", "import"],
            &[false, false, false],
            "ask",
            Some("auth0 users"),
        ),
        (
            &["auth0", "users", "import", "--connection", "myconn"],
            &["auth0", "users", "import", "--connection", "myconn"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 users"),
        ),
        (
            &["auth0", "logs", "list"],
            &["auth0", "logs", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 logs"),
        ),
        (
            &["auth0", "logs", "ls"],
            &["auth0", "logs", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 logs"),
        ),
        (
            &["auth0", "logs", "tail"],
            &["auth0", "logs", "tail"],
            &[false, false, false],
            "allow",
            Some("auth0 logs"),
        ),
        (
            &["auth0", "logs", "tail", "--filter", "type:s"],
            &["auth0", "logs", "tail", "--filter", "type:s"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 logs"),
        ),
        (
            &["auth0", "actions", "list"],
            &["auth0", "actions", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "actions", "ls"],
            &["auth0", "actions", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "actions", "show", "act_12345"],
            &["auth0", "actions", "show", "act_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "actions", "diff", "act_12345"],
            &["auth0", "actions", "diff", "act_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "actions", "create"],
            &["auth0", "actions", "create"],
            &[false, false, false],
            "ask",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "actions", "update", "act_12345"],
            &["auth0", "actions", "update", "act_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "actions", "delete", "act_12345"],
            &["auth0", "actions", "delete", "act_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "actions", "rm", "act_12345"],
            &["auth0", "actions", "rm", "act_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "actions", "deploy", "act_12345"],
            &["auth0", "actions", "deploy", "act_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 actions"),
        ),
        (
            &["auth0", "apis", "list"],
            &["auth0", "apis", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "apis", "ls"],
            &["auth0", "apis", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "apis", "show", "api_12345"],
            &["auth0", "apis", "show", "api_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "apis", "create"],
            &["auth0", "apis", "create"],
            &[false, false, false],
            "ask",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "apis", "update", "api_12345"],
            &["auth0", "apis", "update", "api_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "apis", "delete", "api_12345"],
            &["auth0", "apis", "delete", "api_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "apis", "rm", "api_12345"],
            &["auth0", "apis", "rm", "api_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "apis", "scopes", "list", "api_12345"],
            &["auth0", "apis", "scopes", "list", "api_12345"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "apis", "scopes", "ls", "api_12345"],
            &["auth0", "apis", "scopes", "ls", "api_12345"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 apis"),
        ),
        (
            &["auth0", "roles", "list"],
            &["auth0", "roles", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "ls"],
            &["auth0", "roles", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "show", "rol_12345"],
            &["auth0", "roles", "show", "rol_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "create"],
            &["auth0", "roles", "create"],
            &[false, false, false],
            "ask",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "update", "rol_12345"],
            &["auth0", "roles", "update", "rol_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "delete", "rol_12345"],
            &["auth0", "roles", "delete", "rol_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "rm", "rol_12345"],
            &["auth0", "roles", "rm", "rol_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "permissions", "list", "rol_12345"],
            &["auth0", "roles", "permissions", "list", "rol_12345"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "perms", "list", "rol_12345"],
            &["auth0", "roles", "perms", "list", "rol_12345"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "permissions", "add", "rol_12345"],
            &["auth0", "roles", "permissions", "add", "rol_12345"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "permissions", "remove", "rol_12345"],
            &["auth0", "roles", "permissions", "remove", "rol_12345"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "roles", "perms", "rm", "rol_12345"],
            &["auth0", "roles", "perms", "rm", "rol_12345"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 roles"),
        ),
        (
            &["auth0", "orgs", "list"],
            &["auth0", "orgs", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "orgs", "ls"],
            &["auth0", "orgs", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "orgs", "show", "org_12345"],
            &["auth0", "orgs", "show", "org_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "orgs", "create"],
            &["auth0", "orgs", "create"],
            &[false, false, false],
            "ask",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "orgs", "update", "org_12345"],
            &["auth0", "orgs", "update", "org_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "orgs", "delete", "org_12345"],
            &["auth0", "orgs", "delete", "org_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "orgs", "rm", "org_12345"],
            &["auth0", "orgs", "rm", "org_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "orgs", "members", "list", "org_12345"],
            &["auth0", "orgs", "members", "list", "org_12345"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "orgs", "members", "ls", "org_12345"],
            &["auth0", "orgs", "members", "ls", "org_12345"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 orgs"),
        ),
        (
            &["auth0", "rules", "list"],
            &["auth0", "rules", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "rules", "ls"],
            &["auth0", "rules", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "rules", "show", "rul_12345"],
            &["auth0", "rules", "show", "rul_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "rules", "create"],
            &["auth0", "rules", "create"],
            &[false, false, false],
            "ask",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "rules", "update", "rul_12345"],
            &["auth0", "rules", "update", "rul_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "rules", "delete", "rul_12345"],
            &["auth0", "rules", "delete", "rul_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "rules", "rm", "rul_12345"],
            &["auth0", "rules", "rm", "rul_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "rules", "enable", "rul_12345"],
            &["auth0", "rules", "enable", "rul_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "rules", "disable", "rul_12345"],
            &["auth0", "rules", "disable", "rul_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 rules"),
        ),
        (
            &["auth0", "domains", "list"],
            &["auth0", "domains", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 domains"),
        ),
        (
            &["auth0", "domains", "ls"],
            &["auth0", "domains", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 domains"),
        ),
        (
            &["auth0", "domains", "show", "cd_12345"],
            &["auth0", "domains", "show", "cd_12345"],
            &[false, false, false, false],
            "allow",
            Some("auth0 domains"),
        ),
        (
            &["auth0", "domains", "create"],
            &["auth0", "domains", "create"],
            &[false, false, false],
            "ask",
            Some("auth0 domains"),
        ),
        (
            &["auth0", "domains", "update", "cd_12345"],
            &["auth0", "domains", "update", "cd_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 domains"),
        ),
        (
            &["auth0", "domains", "delete", "cd_12345"],
            &["auth0", "domains", "delete", "cd_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 domains"),
        ),
        (
            &["auth0", "domains", "rm", "cd_12345"],
            &["auth0", "domains", "rm", "cd_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 domains"),
        ),
        (
            &["auth0", "domains", "verify", "cd_12345"],
            &["auth0", "domains", "verify", "cd_12345"],
            &[false, false, false, false],
            "ask",
            Some("auth0 domains"),
        ),
        (
            &["auth0", "protection", "breached-password-detection", "show"],
            &["auth0", "protection", "breached-password-detection", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "protection", "bpd", "show"],
            &["auth0", "protection", "bpd", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "protection", "brute-force-protection", "show"],
            &["auth0", "protection", "brute-force-protection", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "protection", "bfp", "show"],
            &["auth0", "protection", "bfp", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "protection", "suspicious-ip-throttling", "show"],
            &["auth0", "protection", "suspicious-ip-throttling", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "protection", "sit", "show"],
            &["auth0", "protection", "sit", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "ap", "bpd", "show"],
            &["auth0", "ap", "bpd", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 ap"),
        ),
        (
            &["auth0", "attack-protection", "bpd", "show"],
            &["auth0", "attack-protection", "bpd", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 attack-protection"),
        ),
        (
            &[
                "auth0",
                "protection",
                "breached-password-detection",
                "update",
            ],
            &[
                "auth0",
                "protection",
                "breached-password-detection",
                "update",
            ],
            &[false, false, false, false],
            "ask",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "protection", "bpd", "update"],
            &["auth0", "protection", "bpd", "update"],
            &[false, false, false, false],
            "ask",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "protection", "brute-force-protection", "update"],
            &["auth0", "protection", "brute-force-protection", "update"],
            &[false, false, false, false],
            "ask",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "protection", "suspicious-ip-throttling", "update"],
            &["auth0", "protection", "suspicious-ip-throttling", "update"],
            &[false, false, false, false],
            "ask",
            Some("auth0 protection"),
        ),
        (
            &["auth0", "tenants", "list"],
            &["auth0", "tenants", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 tenants"),
        ),
        (
            &["auth0", "tenants", "ls"],
            &["auth0", "tenants", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 tenants"),
        ),
        (
            &["auth0", "tenants", "use", "mytenant"],
            &["auth0", "tenants", "use", "mytenant"],
            &[false, false, false, false],
            "ask",
            Some("auth0 tenants"),
        ),
        (
            &["auth0", "quickstarts", "list"],
            &["auth0", "quickstarts", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 quickstarts"),
        ),
        (
            &["auth0", "qs", "list"],
            &["auth0", "qs", "list"],
            &[false, false, false],
            "allow",
            Some("auth0 qs"),
        ),
        (
            &["auth0", "quickstarts", "ls"],
            &["auth0", "quickstarts", "ls"],
            &[false, false, false],
            "allow",
            Some("auth0 quickstarts"),
        ),
        (
            &["auth0", "quickstarts", "download"],
            &["auth0", "quickstarts", "download"],
            &[false, false, false],
            "ask",
            Some("auth0 quickstarts"),
        ),
        (
            &["auth0", "qs", "download"],
            &["auth0", "qs", "download"],
            &[false, false, false],
            "ask",
            Some("auth0 qs"),
        ),
        (
            &["auth0", "email", "templates", "show"],
            &["auth0", "email", "templates", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 email"),
        ),
        (
            &["auth0", "email", "templates", "update"],
            &["auth0", "email", "templates", "update"],
            &[false, false, false, false],
            "ask",
            Some("auth0 email"),
        ),
        (
            &["auth0", "email", "provider", "show"],
            &["auth0", "email", "provider", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 email"),
        ),
        (
            &["auth0", "email", "provider", "create"],
            &["auth0", "email", "provider", "create"],
            &[false, false, false, false],
            "ask",
            Some("auth0 email"),
        ),
        (
            &["auth0", "email", "provider", "update"],
            &["auth0", "email", "provider", "update"],
            &[false, false, false, false],
            "ask",
            Some("auth0 email"),
        ),
        (
            &["auth0", "email", "provider", "delete"],
            &["auth0", "email", "provider", "delete"],
            &[false, false, false, false],
            "ask",
            Some("auth0 email"),
        ),
        (
            &["auth0", "email", "provider", "rm"],
            &["auth0", "email", "provider", "rm"],
            &[false, false, false, false],
            "ask",
            Some("auth0 email"),
        ),
        (
            &["auth0", "universal-login", "show"],
            &["auth0", "universal-login", "show"],
            &[false, false, false],
            "allow",
            Some("auth0 universal-login"),
        ),
        (
            &["auth0", "ul", "show"],
            &["auth0", "ul", "show"],
            &[false, false, false],
            "allow",
            Some("auth0 ul"),
        ),
        (
            &["auth0", "universal-login", "update"],
            &["auth0", "universal-login", "update"],
            &[false, false, false],
            "ask",
            Some("auth0 universal-login"),
        ),
        (
            &["auth0", "ul", "update"],
            &["auth0", "ul", "update"],
            &[false, false, false],
            "ask",
            Some("auth0 ul"),
        ),
        (
            &["auth0", "universal-login", "customize"],
            &["auth0", "universal-login", "customize"],
            &[false, false, false],
            "ask",
            Some("auth0 universal-login"),
        ),
        (
            &["auth0", "ul", "customize"],
            &["auth0", "ul", "customize"],
            &[false, false, false],
            "ask",
            Some("auth0 ul"),
        ),
        (
            &["auth0", "universal-login", "templates", "show"],
            &["auth0", "universal-login", "templates", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 universal-login"),
        ),
        (
            &["auth0", "ul", "templates", "show"],
            &["auth0", "ul", "templates", "show"],
            &[false, false, false, false],
            "allow",
            Some("auth0 ul"),
        ),
        (
            &["auth0", "universal-login", "templates", "update"],
            &["auth0", "universal-login", "templates", "update"],
            &[false, false, false, false],
            "ask",
            Some("auth0 universal-login"),
        ),
        (
            &["auth0", "ul", "templates", "update"],
            &["auth0", "ul", "templates", "update"],
            &[false, false, false, false],
            "ask",
            Some("auth0 ul"),
        ),
        (
            &["auth0", "universal-login", "prompts", "show", "login"],
            &["auth0", "universal-login", "prompts", "show", "login"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 universal-login"),
        ),
        (
            &["auth0", "ul", "prompts", "show", "login"],
            &["auth0", "ul", "prompts", "show", "login"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 ul"),
        ),
        (
            &["auth0", "universal-login", "prompts", "update", "login"],
            &["auth0", "universal-login", "prompts", "update", "login"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 universal-login"),
        ),
        (
            &["auth0", "test", "token"],
            &["auth0", "test", "token"],
            &[false, false, false],
            "ask",
            Some("auth0 test"),
        ),
        (
            &["auth0", "test", "login"],
            &["auth0", "test", "login"],
            &[false, false, false],
            "ask",
            Some("auth0 test"),
        ),
        (
            &["auth0", "api", "get", "clients"],
            &["auth0", "api", "get", "clients"],
            &[false, false, false, false],
            "allow",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "get", "tenants/settings"],
            &["auth0", "api", "get", "tenants/settings"],
            &[false, false, false, false],
            "allow",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "clients"],
            &["auth0", "api", "clients"],
            &[false, false, false],
            "allow",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "stats/daily"],
            &["auth0", "api", "stats/daily"],
            &[false, false, false],
            "allow",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "get", "users", "-q", "search_engine:v3"],
            &["auth0", "api", "get", "users", "-q", "search_engine:v3"],
            &[false, false, false, false, false, false],
            "allow",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "post", "clients"],
            &["auth0", "api", "post", "clients"],
            &[false, false, false, false],
            "ask",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "put", "clients/client_id"],
            &["auth0", "api", "put", "clients/client_id"],
            &[false, false, false, false],
            "ask",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "patch", "clients/client_id"],
            &["auth0", "api", "patch", "clients/client_id"],
            &[false, false, false, false],
            "ask",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "delete", "actions/actions/act_id"],
            &["auth0", "api", "delete", "actions/actions/act_id"],
            &[false, false, false, false],
            "ask",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "clients", "--data", "{\"name\":\"test\"}"],
            &["auth0", "api", "clients", "--data", "'{\"name\":\"test\"}'"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 api"),
        ),
        (
            &["auth0", "api", "clients", "-d", "{\"name\":\"test\"}"],
            &["auth0", "api", "clients", "-d", "'{\"name\":\"test\"}'"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 api"),
        ),
        (
            &["auth0", "--tenant", "mytenant", "apps", "list"],
            &["auth0", "--tenant", "mytenant", "apps", "list"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "-t", "mytenant", "apps", "list"],
            &["auth0", "-t", "mytenant", "apps", "list"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "apps", "list", "--tenant", "mytenant"],
            &["auth0", "apps", "list", "--tenant", "mytenant"],
            &[false, false, false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "--tenant", "mytenant", "apps", "create"],
            &["auth0", "--tenant", "mytenant", "apps", "create"],
            &[false, false, false, false, false],
            "ask",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "--debug", "apps", "list"],
            &["auth0", "--debug", "apps", "list"],
            &[false, false, false, false],
            "allow",
            Some("auth0 list"),
        ),
        (
            &["auth0", "--no-color", "apps", "list"],
            &["auth0", "--no-color", "apps", "list"],
            &[false, false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "--no-input", "apps", "list"],
            &["auth0", "--no-input", "apps", "list"],
            &[false, false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (&["auth0"], &["auth0"], &[false], "ask", Some("auth0")),
        (
            &["auth0", "--help"],
            &["auth0", "--help"],
            &[false, false],
            "allow",
            Some("auth0 --help"),
        ),
        (
            &["auth0", "apps", "--help"],
            &["auth0", "apps", "--help"],
            &[false, false, false],
            "allow",
            Some("auth0 apps"),
        ),
        (
            &["auth0", "unknown-command"],
            &["auth0", "unknown-command"],
            &[false, false],
            "ask",
            Some("auth0 unknown-command"),
        ),
    ];

    #[test]
    fn matches_python_handler() {
        for (tokens, raw, exp, action, desc) in PY_CASES {
            let mut ctx = HandlerContext::new(tokens);
            ctx.raw_words = raw.iter().map(|s| s.to_string()).collect();
            ctx.word_has_expansions = exp.to_vec();
            let result = classify(&ctx);
            assert_eq!(result.action.as_str(), *action, "{tokens:?}");
            assert_eq!(result.description.as_deref(), *desc, "{tokens:?}");
        }
    }
}
