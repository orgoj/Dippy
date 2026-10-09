//! Port of `src/dippy/cli/helm.py`.
//!
//! Helm is the Kubernetes package manager. Safe operations are read-only
//! queries (list, get, show, status, history, search) and dry-run modes.
//! Unsafe operations mutate cluster state, local files, or remote registries.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["helm"];
pub const PORTED: bool = true;
/// The Python module defines no `get_description`.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe top-level commands (read-only).
const SAFE_COMMANDS: &[&str] = &[
    "completion",
    "env",
    "get",
    "help",
    "history",
    "lint",
    "list",
    "ls",
    "search",
    "show",
    "inspect", // alias for show
    "status",
    "template",
    "verify",
    "version",
];

/// Unsafe top-level commands (mutate cluster, files, or remote).
const UNSAFE_COMMANDS: &[&str] = &[
    "create",
    "install",
    "package",
    "pull",
    "fetch", // alias for pull
    "push",
    "rollback",
    "test",
    "uninstall",
    "delete", // alias for uninstall
    "del",    // alias for uninstall
    "un",     // alias for uninstall
    "upgrade",
];

/// Short aliases that need expansion for clarity (`ACTION_ALIASES`).
fn action_alias(action: &str) -> &str {
    match action {
        "del" => "delete",
        "un" => "uninstall",
        "fetch" => "pull",
        other => other,
    }
}

/// Commands with subcommands that need further inspection.
const NESTED_COMMANDS: &[&str] = &["dependency", "dep", "plugin", "registry", "repo"];

/// Safe subcommands for nested commands (`SAFE_SUBCOMMANDS`).
fn safe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "dependency" | "dep" => Some(&["list", "ls"]),
        "plugin" => Some(&["list", "ls", "verify"]),
        "repo" => Some(&["list", "ls"]),
        _ => None,
    }
}

/// Unsafe subcommands for nested commands (`UNSAFE_SUBCOMMANDS`).
fn unsafe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "dependency" | "dep" => Some(&["build", "update", "up"]),
        "plugin" => Some(&["install", "uninstall", "update", "package"]),
        "registry" => Some(&["login", "logout"]),
        "repo" => Some(&["add", "remove", "rm", "update", "up", "index"]),
        _ => None,
    }
}

/// Global flags that take an argument.
const GLOBAL_FLAGS_WITH_ARG: &[&str] = &[
    "-n",
    "--namespace",
    "--kube-context",
    "--kube-apiserver",
    "--kube-as-user",
    "--kube-ca-file",
    "--kube-token",
    "--kubeconfig",
    "--registry-config",
    "--repository-cache",
    "--repository-config",
    "--content-cache",
    "--burst-limit",
    "--qps",
    "--kube-tls-server-name",
];

/// Classify helm command.
pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map_or("helm", String::as_str);
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    // Handle global flags before subcommand
    let mut idx = 1;
    while idx < tokens.len() {
        let token = tokens[idx].as_str();

        // Skip global flags
        if token.starts_with('-') {
            // Flags that take arguments
            if GLOBAL_FLAGS_WITH_ARG.contains(&token) {
                idx += 2;
                continue;
            }
            if token.starts_with("--kube-as-group") {
                idx += 2;
                continue;
            }
            idx += 1;
            continue;
        }

        // Found the subcommand
        break;
    }

    if idx >= tokens.len() {
        return Classification::ask_desc(base);
    }

    let action = tokens[idx].as_str();
    let rest: &[String] = tokens.get(idx + 1..).unwrap_or(&[]);
    let desc = format!("{base} {action}");

    // Help and version flags are always safe
    if matches!(action, "-h" | "--help" | "--version") {
        return Classification::allow_desc(desc);
    }

    // Check for --help anywhere in the command
    if tokens.iter().any(|t| t == "-h" || t == "--help") {
        return Classification::allow_desc(format!("{base} --help"));
    }

    // Simple safe commands
    if SAFE_COMMANDS.contains(&action) {
        return Classification::allow_desc(desc);
    }

    // Check for dry-run flag (makes install/upgrade/uninstall/rollback safe)
    if matches!(
        action,
        "install" | "upgrade" | "uninstall" | "delete" | "del" | "un" | "rollback"
    ) {
        let display_desc = format!("{base} {}", action_alias(action));
        for t in rest {
            if t == "--dry-run" || t.starts_with("--dry-run=") {
                return Classification::allow_desc(format!("{display_desc} --dry-run"));
            }
        }
        return Classification::ask_desc(display_desc);
    }

    // Nested commands - check subcommand
    if NESTED_COMMANDS.contains(&action) {
        // Find the subcommand (skip flags)
        if let Some(t) = rest
            .iter()
            .map(String::as_str)
            .find(|t| !t.starts_with('-'))
        {
            let nested_desc = format!("{desc} {t}");
            if safe_subcommands(action).is_some_and(|s| s.contains(&t)) {
                return Classification::allow_desc(nested_desc);
            }
            if unsafe_subcommands(action).is_some_and(|s| s.contains(&t)) {
                return Classification::ask_desc(nested_desc);
            }
            // Unknown subcommand - be safe
            return Classification::ask_desc(nested_desc);
        }
        // No subcommand found
        return Classification::ask_desc(desc);
    }

    // Known unsafe commands
    if UNSAFE_COMMANDS.contains(&action) {
        return Classification::ask_desc(format!("{base} {}", action_alias(action)));
    }

    // Unknown command - require confirmation
    Classification::ask_desc(desc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_alias_description() {
        let r = classify(&HandlerContext::new(&[
            "helm",
            "del",
            "x",
            "--dry-run=client",
        ]));
        assert_eq!(r.action.as_str(), "allow");
        assert_eq!(r.description.as_deref(), Some("helm delete --dry-run"));
    }

    /// Generated from the Python handler on the commands of
    /// `tests/cli/test_helm.py` (see `rust/parity/cases/helm.txt`).
    #[allow(clippy::type_complexity)]
    const PY_CASES: &[(&[&str], &[&str], &[bool], &str, Option<&str>)] = &[
        (
            &["helm", "--help"],
            &["helm", "--help"],
            &[false, false],
            "ask",
            Some("helm"),
        ),
        (
            &["helm", "-h"],
            &["helm", "-h"],
            &[false, false],
            "ask",
            Some("helm"),
        ),
        (
            &["helm", "--version"],
            &["helm", "--version"],
            &[false, false],
            "ask",
            Some("helm"),
        ),
        (
            &["helm", "version"],
            &["helm", "version"],
            &[false, false],
            "allow",
            Some("helm version"),
        ),
        (
            &["helm", "help"],
            &["helm", "help"],
            &[false, false],
            "allow",
            Some("helm help"),
        ),
        (
            &["helm", "help", "install"],
            &["helm", "help", "install"],
            &[false, false, false],
            "allow",
            Some("helm help"),
        ),
        (
            &["helm", "install", "--help"],
            &["helm", "install", "--help"],
            &[false, false, false],
            "allow",
            Some("helm --help"),
        ),
        (
            &["helm", "env"],
            &["helm", "env"],
            &[false, false],
            "allow",
            Some("helm env"),
        ),
        (
            &["helm", "completion", "bash"],
            &["helm", "completion", "bash"],
            &[false, false, false],
            "allow",
            Some("helm completion"),
        ),
        (
            &["helm", "completion", "zsh"],
            &["helm", "completion", "zsh"],
            &[false, false, false],
            "allow",
            Some("helm completion"),
        ),
        (
            &["helm", "completion", "fish"],
            &["helm", "completion", "fish"],
            &[false, false, false],
            "allow",
            Some("helm completion"),
        ),
        (
            &["helm", "completion", "powershell"],
            &["helm", "completion", "powershell"],
            &[false, false, false],
            "allow",
            Some("helm completion"),
        ),
        (
            &["helm", "list"],
            &["helm", "list"],
            &[false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "ls"],
            &["helm", "ls"],
            &[false, false],
            "allow",
            Some("helm ls"),
        ),
        (
            &["helm", "list", "-A"],
            &["helm", "list", "-A"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--all-namespaces"],
            &["helm", "list", "--all-namespaces"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "-n", "production"],
            &["helm", "list", "-n", "production"],
            &[false, false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--namespace", "production"],
            &["helm", "list", "--namespace", "production"],
            &[false, false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "-o", "json"],
            &["helm", "list", "-o", "json"],
            &[false, false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--output", "yaml"],
            &["helm", "list", "--output", "yaml"],
            &[false, false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--filter", "nginx"],
            &["helm", "list", "--filter", "nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--deployed"],
            &["helm", "list", "--deployed"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--failed"],
            &["helm", "list", "--failed"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--pending"],
            &["helm", "list", "--pending"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--superseded"],
            &["helm", "list", "--superseded"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--uninstalled"],
            &["helm", "list", "--uninstalled"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--uninstalling"],
            &["helm", "list", "--uninstalling"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "get", "all", "myrelease"],
            &["helm", "get", "all", "myrelease"],
            &[false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "hooks", "myrelease"],
            &["helm", "get", "hooks", "myrelease"],
            &[false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "manifest", "myrelease"],
            &["helm", "get", "manifest", "myrelease"],
            &[false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "metadata", "myrelease"],
            &["helm", "get", "metadata", "myrelease"],
            &[false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "notes", "myrelease"],
            &["helm", "get", "notes", "myrelease"],
            &[false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "values", "myrelease"],
            &["helm", "get", "values", "myrelease"],
            &[false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "values", "myrelease", "-a"],
            &["helm", "get", "values", "myrelease", "-a"],
            &[false, false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "values", "myrelease", "--all"],
            &["helm", "get", "values", "myrelease", "--all"],
            &[false, false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "values", "myrelease", "-n", "production"],
            &["helm", "get", "values", "myrelease", "-n", "production"],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "get", "all", "myrelease", "--revision", "3"],
            &["helm", "get", "all", "myrelease", "--revision", "3"],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm get"),
        ),
        (
            &["helm", "show", "all", "nginx/nginx"],
            &["helm", "show", "all", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm show"),
        ),
        (
            &["helm", "show", "chart", "nginx/nginx"],
            &["helm", "show", "chart", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm show"),
        ),
        (
            &["helm", "show", "crds", "nginx/nginx"],
            &["helm", "show", "crds", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm show"),
        ),
        (
            &["helm", "show", "readme", "nginx/nginx"],
            &["helm", "show", "readme", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm show"),
        ),
        (
            &["helm", "show", "values", "nginx/nginx"],
            &["helm", "show", "values", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm show"),
        ),
        (
            &["helm", "inspect", "all", "nginx/nginx"],
            &["helm", "inspect", "all", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm inspect"),
        ),
        (
            &["helm", "inspect", "chart", "nginx/nginx"],
            &["helm", "inspect", "chart", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm inspect"),
        ),
        (
            &["helm", "inspect", "values", "nginx/nginx"],
            &["helm", "inspect", "values", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm inspect"),
        ),
        (
            &["helm", "show", "all", "./mychart"],
            &["helm", "show", "all", "./mychart"],
            &[false, false, false, false],
            "allow",
            Some("helm show"),
        ),
        (
            &["helm", "show", "values", "./mychart", "--version", "1.2.3"],
            &["helm", "show", "values", "./mychart", "--version", "1.2.3"],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm show"),
        ),
        (
            &["helm", "status", "myrelease"],
            &["helm", "status", "myrelease"],
            &[false, false, false],
            "allow",
            Some("helm status"),
        ),
        (
            &["helm", "status", "myrelease", "-n", "production"],
            &["helm", "status", "myrelease", "-n", "production"],
            &[false, false, false, false, false],
            "allow",
            Some("helm status"),
        ),
        (
            &["helm", "status", "myrelease", "--revision", "3"],
            &["helm", "status", "myrelease", "--revision", "3"],
            &[false, false, false, false, false],
            "allow",
            Some("helm status"),
        ),
        (
            &["helm", "status", "myrelease", "-o", "json"],
            &["helm", "status", "myrelease", "-o", "json"],
            &[false, false, false, false, false],
            "allow",
            Some("helm status"),
        ),
        (
            &["helm", "history", "myrelease"],
            &["helm", "history", "myrelease"],
            &[false, false, false],
            "allow",
            Some("helm history"),
        ),
        (
            &["helm", "history", "myrelease", "-n", "production"],
            &["helm", "history", "myrelease", "-n", "production"],
            &[false, false, false, false, false],
            "allow",
            Some("helm history"),
        ),
        (
            &["helm", "history", "myrelease", "--max", "10"],
            &["helm", "history", "myrelease", "--max", "10"],
            &[false, false, false, false, false],
            "allow",
            Some("helm history"),
        ),
        (
            &["helm", "history", "myrelease", "-o", "json"],
            &["helm", "history", "myrelease", "-o", "json"],
            &[false, false, false, false, false],
            "allow",
            Some("helm history"),
        ),
        (
            &["helm", "search", "hub", "nginx"],
            &["helm", "search", "hub", "nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm search"),
        ),
        (
            &["helm", "search", "repo", "nginx"],
            &["helm", "search", "repo", "nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm search"),
        ),
        (
            &["helm", "search", "hub", "nginx", "--max-col-width", "80"],
            &["helm", "search", "hub", "nginx", "--max-col-width", "80"],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm search"),
        ),
        (
            &["helm", "search", "repo", "nginx", "--versions"],
            &["helm", "search", "repo", "nginx", "--versions"],
            &[false, false, false, false, false],
            "allow",
            Some("helm search"),
        ),
        (
            &["helm", "search", "repo", "nginx", "-l"],
            &["helm", "search", "repo", "nginx", "-l"],
            &[false, false, false, false, false],
            "allow",
            Some("helm search"),
        ),
        (
            &["helm", "search", "repo", "nginx", "--version", "^2.0.0"],
            &["helm", "search", "repo", "nginx", "--version", "^2.0.0"],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm search"),
        ),
        (
            &["helm", "template", "myrelease", "nginx/nginx"],
            &["helm", "template", "myrelease", "nginx/nginx"],
            &[false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &["helm", "template", "myrelease", "./mychart"],
            &["helm", "template", "myrelease", "./mychart"],
            &[false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--set",
                "key=value",
            ],
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--set",
                "key=value",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "-f",
                "values.yaml",
            ],
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "-f",
                "values.yaml",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--values",
                "values.yaml",
            ],
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--values",
                "values.yaml",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--version",
                "1.2.3",
            ],
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--version",
                "1.2.3",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--namespace",
                "production",
            ],
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--namespace",
                "production",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--include-crds",
            ],
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--include-crds",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--skip-crds",
            ],
            &[
                "helm",
                "template",
                "myrelease",
                "nginx/nginx",
                "--skip-crds",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &["helm", "template", "myrelease", "nginx/nginx", "--no-hooks"],
            &["helm", "template", "myrelease", "nginx/nginx", "--no-hooks"],
            &[false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &["helm", "template", "myrelease", "nginx/nginx", "--validate"],
            &["helm", "template", "myrelease", "nginx/nginx", "--validate"],
            &[false, false, false, false, false],
            "allow",
            Some("helm template"),
        ),
        (
            &["helm", "lint", "./mychart"],
            &["helm", "lint", "./mychart"],
            &[false, false, false],
            "allow",
            Some("helm lint"),
        ),
        (
            &["helm", "lint", "./mychart", "--strict"],
            &["helm", "lint", "./mychart", "--strict"],
            &[false, false, false, false],
            "allow",
            Some("helm lint"),
        ),
        (
            &["helm", "lint", "./mychart", "--with-subcharts"],
            &["helm", "lint", "./mychart", "--with-subcharts"],
            &[false, false, false, false],
            "allow",
            Some("helm lint"),
        ),
        (
            &["helm", "lint", "./mychart", "--set", "key=value"],
            &["helm", "lint", "./mychart", "--set", "key=value"],
            &[false, false, false, false, false],
            "allow",
            Some("helm lint"),
        ),
        (
            &["helm", "lint", "./mychart", "-f", "values.yaml"],
            &["helm", "lint", "./mychart", "-f", "values.yaml"],
            &[false, false, false, false, false],
            "allow",
            Some("helm lint"),
        ),
        (
            &["helm", "verify", "./mychart-1.0.0.tgz"],
            &["helm", "verify", "./mychart-1.0.0.tgz"],
            &[false, false, false],
            "allow",
            Some("helm verify"),
        ),
        (
            &[
                "helm",
                "verify",
                "./mychart-1.0.0.tgz",
                "--keyring",
                "pubring.gpg",
            ],
            &[
                "helm",
                "verify",
                "./mychart-1.0.0.tgz",
                "--keyring",
                "pubring.gpg",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("helm verify"),
        ),
        (
            &["helm", "repo", "list"],
            &["helm", "repo", "list"],
            &[false, false, false],
            "allow",
            Some("helm repo list"),
        ),
        (
            &["helm", "repo", "ls"],
            &["helm", "repo", "ls"],
            &[false, false, false],
            "allow",
            Some("helm repo ls"),
        ),
        (
            &["helm", "repo", "list", "-o", "json"],
            &["helm", "repo", "list", "-o", "json"],
            &[false, false, false, false, false],
            "allow",
            Some("helm repo list"),
        ),
        (
            &["helm", "dependency", "list", "./mychart"],
            &["helm", "dependency", "list", "./mychart"],
            &[false, false, false, false],
            "allow",
            Some("helm dependency list"),
        ),
        (
            &["helm", "dep", "list", "./mychart"],
            &["helm", "dep", "list", "./mychart"],
            &[false, false, false, false],
            "allow",
            Some("helm dep list"),
        ),
        (
            &["helm", "dependency", "ls", "./mychart"],
            &["helm", "dependency", "ls", "./mychart"],
            &[false, false, false, false],
            "allow",
            Some("helm dependency ls"),
        ),
        (
            &["helm", "dep", "ls", "./mychart"],
            &["helm", "dep", "ls", "./mychart"],
            &[false, false, false, false],
            "allow",
            Some("helm dep ls"),
        ),
        (
            &["helm", "plugin", "list"],
            &["helm", "plugin", "list"],
            &[false, false, false],
            "allow",
            Some("helm plugin list"),
        ),
        (
            &["helm", "plugin", "ls"],
            &["helm", "plugin", "ls"],
            &[false, false, false],
            "allow",
            Some("helm plugin ls"),
        ),
        (
            &["helm", "plugin", "verify", "./myplugin"],
            &["helm", "plugin", "verify", "./myplugin"],
            &[false, false, false, false],
            "allow",
            Some("helm plugin verify"),
        ),
        (
            &["helm", "install", "myrelease", "nginx/nginx", "--dry-run"],
            &["helm", "install", "myrelease", "nginx/nginx", "--dry-run"],
            &[false, false, false, false, false],
            "allow",
            Some("helm install --dry-run"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--dry-run=client",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--dry-run=client",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("helm install --dry-run"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--dry-run=server",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--dry-run=server",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("helm install --dry-run"),
        ),
        (
            &["helm", "upgrade", "myrelease", "nginx/nginx", "--dry-run"],
            &["helm", "upgrade", "myrelease", "nginx/nginx", "--dry-run"],
            &[false, false, false, false, false],
            "allow",
            Some("helm upgrade --dry-run"),
        ),
        (
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--dry-run=client",
            ],
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--dry-run=client",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("helm upgrade --dry-run"),
        ),
        (
            &["helm", "uninstall", "myrelease", "--dry-run"],
            &["helm", "uninstall", "myrelease", "--dry-run"],
            &[false, false, false, false],
            "allow",
            Some("helm uninstall --dry-run"),
        ),
        (
            &["helm", "rollback", "myrelease", "2", "--dry-run"],
            &["helm", "rollback", "myrelease", "2", "--dry-run"],
            &[false, false, false, false, false],
            "allow",
            Some("helm rollback --dry-run"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "-n",
                "prod",
                "--dry-run",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "-n",
                "prod",
                "--dry-run",
            ],
            &[false, false, false, false, false, false, false],
            "allow",
            Some("helm install --dry-run"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--set",
                "foo=bar",
                "--dry-run",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--set",
                "foo=bar",
                "--dry-run",
            ],
            &[false, false, false, false, false, false, false],
            "allow",
            Some("helm install --dry-run"),
        ),
        (
            &[
                "helm",
                "upgrade",
                "--install",
                "myrelease",
                "nginx/nginx",
                "--dry-run",
            ],
            &[
                "helm",
                "upgrade",
                "--install",
                "myrelease",
                "nginx/nginx",
                "--dry-run",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("helm upgrade --dry-run"),
        ),
        (
            &["helm", "install", "myrelease", "nginx/nginx"],
            &["helm", "install", "myrelease", "nginx/nginx"],
            &[false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &["helm", "install", "myrelease", "./mychart"],
            &["helm", "install", "myrelease", "./mychart"],
            &[false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "-n",
                "production",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "-n",
                "production",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--namespace",
                "production",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--namespace",
                "production",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--create-namespace",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--create-namespace",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--set",
                "key=value",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--set",
                "key=value",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "-f",
                "values.yaml",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "-f",
                "values.yaml",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--values",
                "values.yaml",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--values",
                "values.yaml",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--version",
                "1.2.3",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--version",
                "1.2.3",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &["helm", "install", "myrelease", "nginx/nginx", "--wait"],
            &["helm", "install", "myrelease", "nginx/nginx", "--wait"],
            &[false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--timeout",
                "5m",
            ],
            &[
                "helm",
                "install",
                "myrelease",
                "nginx/nginx",
                "--timeout",
                "5m",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &["helm", "install", "myrelease", "nginx/nginx", "--atomic"],
            &["helm", "install", "myrelease", "nginx/nginx", "--atomic"],
            &[false, false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &["helm", "install", "nginx/nginx", "--generate-name"],
            &["helm", "install", "nginx/nginx", "--generate-name"],
            &[false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &["helm", "install", "nginx/nginx", "-g"],
            &["helm", "install", "nginx/nginx", "-g"],
            &[false, false, false, false],
            "ask",
            Some("helm install"),
        ),
        (
            &["helm", "upgrade", "myrelease", "nginx/nginx"],
            &["helm", "upgrade", "myrelease", "nginx/nginx"],
            &[false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &["helm", "upgrade", "myrelease", "./mychart"],
            &["helm", "upgrade", "myrelease", "./mychart"],
            &[false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "-n",
                "production",
            ],
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "-n",
                "production",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &["helm", "upgrade", "myrelease", "nginx/nginx", "--install"],
            &["helm", "upgrade", "myrelease", "nginx/nginx", "--install"],
            &[false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &["helm", "upgrade", "--install", "myrelease", "nginx/nginx"],
            &["helm", "upgrade", "--install", "myrelease", "nginx/nginx"],
            &[false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--set",
                "key=value",
            ],
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--set",
                "key=value",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "-f",
                "values.yaml",
            ],
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "-f",
                "values.yaml",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--reuse-values",
            ],
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--reuse-values",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--reset-values",
            ],
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--reset-values",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &["helm", "upgrade", "myrelease", "nginx/nginx", "--force"],
            &["helm", "upgrade", "myrelease", "nginx/nginx", "--force"],
            &[false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--cleanup-on-fail",
            ],
            &[
                "helm",
                "upgrade",
                "myrelease",
                "nginx/nginx",
                "--cleanup-on-fail",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("helm upgrade"),
        ),
        (
            &["helm", "uninstall", "myrelease"],
            &["helm", "uninstall", "myrelease"],
            &[false, false, false],
            "ask",
            Some("helm uninstall"),
        ),
        (
            &["helm", "delete", "myrelease"],
            &["helm", "delete", "myrelease"],
            &[false, false, false],
            "ask",
            Some("helm delete"),
        ),
        (
            &["helm", "del", "myrelease"],
            &["helm", "del", "myrelease"],
            &[false, false, false],
            "ask",
            Some("helm delete"),
        ),
        (
            &["helm", "un", "myrelease"],
            &["helm", "un", "myrelease"],
            &[false, false, false],
            "ask",
            Some("helm uninstall"),
        ),
        (
            &["helm", "uninstall", "myrelease", "-n", "production"],
            &["helm", "uninstall", "myrelease", "-n", "production"],
            &[false, false, false, false, false],
            "ask",
            Some("helm uninstall"),
        ),
        (
            &["helm", "uninstall", "myrelease", "--keep-history"],
            &["helm", "uninstall", "myrelease", "--keep-history"],
            &[false, false, false, false],
            "ask",
            Some("helm uninstall"),
        ),
        (
            &["helm", "uninstall", "myrelease", "--no-hooks"],
            &["helm", "uninstall", "myrelease", "--no-hooks"],
            &[false, false, false, false],
            "ask",
            Some("helm uninstall"),
        ),
        (
            &["helm", "uninstall", "myrelease", "--wait"],
            &["helm", "uninstall", "myrelease", "--wait"],
            &[false, false, false, false],
            "ask",
            Some("helm uninstall"),
        ),
        (
            &["helm", "rollback", "myrelease"],
            &["helm", "rollback", "myrelease"],
            &[false, false, false],
            "ask",
            Some("helm rollback"),
        ),
        (
            &["helm", "rollback", "myrelease", "2"],
            &["helm", "rollback", "myrelease", "2"],
            &[false, false, false, false],
            "ask",
            Some("helm rollback"),
        ),
        (
            &["helm", "rollback", "myrelease", "2", "-n", "production"],
            &["helm", "rollback", "myrelease", "2", "-n", "production"],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm rollback"),
        ),
        (
            &["helm", "rollback", "myrelease", "2", "--force"],
            &["helm", "rollback", "myrelease", "2", "--force"],
            &[false, false, false, false, false],
            "ask",
            Some("helm rollback"),
        ),
        (
            &["helm", "rollback", "myrelease", "2", "--no-hooks"],
            &["helm", "rollback", "myrelease", "2", "--no-hooks"],
            &[false, false, false, false, false],
            "ask",
            Some("helm rollback"),
        ),
        (
            &["helm", "rollback", "myrelease", "2", "--wait"],
            &["helm", "rollback", "myrelease", "2", "--wait"],
            &[false, false, false, false, false],
            "ask",
            Some("helm rollback"),
        ),
        (
            &["helm", "test", "myrelease"],
            &["helm", "test", "myrelease"],
            &[false, false, false],
            "ask",
            Some("helm test"),
        ),
        (
            &["helm", "test", "myrelease", "-n", "production"],
            &["helm", "test", "myrelease", "-n", "production"],
            &[false, false, false, false, false],
            "ask",
            Some("helm test"),
        ),
        (
            &["helm", "test", "myrelease", "--logs"],
            &["helm", "test", "myrelease", "--logs"],
            &[false, false, false, false],
            "ask",
            Some("helm test"),
        ),
        (
            &["helm", "test", "myrelease", "--timeout", "5m"],
            &["helm", "test", "myrelease", "--timeout", "5m"],
            &[false, false, false, false, false],
            "ask",
            Some("helm test"),
        ),
        (
            &["helm", "create", "mychart"],
            &["helm", "create", "mychart"],
            &[false, false, false],
            "ask",
            Some("helm create"),
        ),
        (
            &["helm", "create", "./path/to/mychart"],
            &["helm", "create", "./path/to/mychart"],
            &[false, false, false],
            "ask",
            Some("helm create"),
        ),
        (
            &["helm", "create", "mychart", "--starter", "mystarter"],
            &["helm", "create", "mychart", "--starter", "mystarter"],
            &[false, false, false, false, false],
            "ask",
            Some("helm create"),
        ),
        (
            &["helm", "package", "./mychart"],
            &["helm", "package", "./mychart"],
            &[false, false, false],
            "ask",
            Some("helm package"),
        ),
        (
            &["helm", "package", "./mychart", "-d", "./output"],
            &["helm", "package", "./mychart", "-d", "./output"],
            &[false, false, false, false, false],
            "ask",
            Some("helm package"),
        ),
        (
            &["helm", "package", "./mychart", "--destination", "./output"],
            &["helm", "package", "./mychart", "--destination", "./output"],
            &[false, false, false, false, false],
            "ask",
            Some("helm package"),
        ),
        (
            &["helm", "package", "./mychart", "--version", "1.2.3"],
            &["helm", "package", "./mychart", "--version", "1.2.3"],
            &[false, false, false, false, false],
            "ask",
            Some("helm package"),
        ),
        (
            &["helm", "package", "./mychart", "--app-version", "2.0.0"],
            &["helm", "package", "./mychart", "--app-version", "2.0.0"],
            &[false, false, false, false, false],
            "ask",
            Some("helm package"),
        ),
        (
            &["helm", "package", "./mychart", "--sign"],
            &["helm", "package", "./mychart", "--sign"],
            &[false, false, false, false],
            "ask",
            Some("helm package"),
        ),
        (
            &["helm", "package", "./mychart", "-u"],
            &["helm", "package", "./mychart", "-u"],
            &[false, false, false, false],
            "ask",
            Some("helm package"),
        ),
        (
            &["helm", "package", "./mychart", "--dependency-update"],
            &["helm", "package", "./mychart", "--dependency-update"],
            &[false, false, false, false],
            "ask",
            Some("helm package"),
        ),
        (
            &["helm", "pull", "nginx/nginx"],
            &["helm", "pull", "nginx/nginx"],
            &[false, false, false],
            "ask",
            Some("helm pull"),
        ),
        (
            &["helm", "fetch", "nginx/nginx"],
            &["helm", "fetch", "nginx/nginx"],
            &[false, false, false],
            "ask",
            Some("helm pull"),
        ),
        (
            &["helm", "pull", "nginx/nginx", "--untar"],
            &["helm", "pull", "nginx/nginx", "--untar"],
            &[false, false, false, false],
            "ask",
            Some("helm pull"),
        ),
        (
            &["helm", "pull", "nginx/nginx", "--untardir", "./charts"],
            &["helm", "pull", "nginx/nginx", "--untardir", "./charts"],
            &[false, false, false, false, false],
            "ask",
            Some("helm pull"),
        ),
        (
            &["helm", "pull", "nginx/nginx", "-d", "./charts"],
            &["helm", "pull", "nginx/nginx", "-d", "./charts"],
            &[false, false, false, false, false],
            "ask",
            Some("helm pull"),
        ),
        (
            &["helm", "pull", "nginx/nginx", "--destination", "./charts"],
            &["helm", "pull", "nginx/nginx", "--destination", "./charts"],
            &[false, false, false, false, false],
            "ask",
            Some("helm pull"),
        ),
        (
            &["helm", "pull", "nginx/nginx", "--version", "1.2.3"],
            &["helm", "pull", "nginx/nginx", "--version", "1.2.3"],
            &[false, false, false, false, false],
            "ask",
            Some("helm pull"),
        ),
        (
            &["helm", "pull", "oci://registry.example.com/charts/nginx"],
            &["helm", "pull", "oci://registry.example.com/charts/nginx"],
            &[false, false, false],
            "ask",
            Some("helm pull"),
        ),
        (
            &[
                "helm",
                "push",
                "mychart-1.0.0.tgz",
                "oci://registry.example.com/charts",
            ],
            &[
                "helm",
                "push",
                "mychart-1.0.0.tgz",
                "oci://registry.example.com/charts",
            ],
            &[false, false, false, false],
            "ask",
            Some("helm push"),
        ),
        (
            &[
                "helm",
                "push",
                "./mychart-1.0.0.tgz",
                "oci://registry.example.com/charts",
            ],
            &[
                "helm",
                "push",
                "./mychart-1.0.0.tgz",
                "oci://registry.example.com/charts",
            ],
            &[false, false, false, false],
            "ask",
            Some("helm push"),
        ),
        (
            &[
                "helm",
                "repo",
                "add",
                "stable",
                "https://charts.helm.sh/stable",
            ],
            &[
                "helm",
                "repo",
                "add",
                "stable",
                "https://charts.helm.sh/stable",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("helm repo add"),
        ),
        (
            &[
                "helm",
                "repo",
                "add",
                "bitnami",
                "https://charts.bitnami.com/bitnami",
            ],
            &[
                "helm",
                "repo",
                "add",
                "bitnami",
                "https://charts.bitnami.com/bitnami",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("helm repo add"),
        ),
        (
            &[
                "helm",
                "repo",
                "add",
                "myrepo",
                "https://example.com/charts",
                "--username",
                "user",
                "--password",
                "pass",
            ],
            &[
                "helm",
                "repo",
                "add",
                "myrepo",
                "https://example.com/charts",
                "--username",
                "user",
                "--password",
                "pass",
            ],
            &[
                false, false, false, false, false, false, false, false, false,
            ],
            "ask",
            Some("helm repo add"),
        ),
        (
            &["helm", "repo", "remove", "stable"],
            &["helm", "repo", "remove", "stable"],
            &[false, false, false, false],
            "ask",
            Some("helm repo remove"),
        ),
        (
            &["helm", "repo", "rm", "stable"],
            &["helm", "repo", "rm", "stable"],
            &[false, false, false, false],
            "ask",
            Some("helm repo rm"),
        ),
        (
            &["helm", "repo", "update"],
            &["helm", "repo", "update"],
            &[false, false, false],
            "ask",
            Some("helm repo update"),
        ),
        (
            &["helm", "repo", "up"],
            &["helm", "repo", "up"],
            &[false, false, false],
            "ask",
            Some("helm repo up"),
        ),
        (
            &["helm", "repo", "index", "./charts"],
            &["helm", "repo", "index", "./charts"],
            &[false, false, false, false],
            "ask",
            Some("helm repo index"),
        ),
        (
            &[
                "helm",
                "repo",
                "index",
                "./charts",
                "--url",
                "https://example.com/charts",
            ],
            &[
                "helm",
                "repo",
                "index",
                "./charts",
                "--url",
                "https://example.com/charts",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm repo index"),
        ),
        (
            &["helm", "repo", "index", "./charts", "--merge", "index.yaml"],
            &["helm", "repo", "index", "./charts", "--merge", "index.yaml"],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm repo index"),
        ),
        (
            &["helm", "dependency", "update", "./mychart"],
            &["helm", "dependency", "update", "./mychart"],
            &[false, false, false, false],
            "ask",
            Some("helm dependency update"),
        ),
        (
            &["helm", "dep", "update", "./mychart"],
            &["helm", "dep", "update", "./mychart"],
            &[false, false, false, false],
            "ask",
            Some("helm dep update"),
        ),
        (
            &["helm", "dep", "up", "./mychart"],
            &["helm", "dep", "up", "./mychart"],
            &[false, false, false, false],
            "ask",
            Some("helm dep up"),
        ),
        (
            &["helm", "dependency", "build", "./mychart"],
            &["helm", "dependency", "build", "./mychart"],
            &[false, false, false, false],
            "ask",
            Some("helm dependency build"),
        ),
        (
            &["helm", "dep", "build", "./mychart"],
            &["helm", "dep", "build", "./mychart"],
            &[false, false, false, false],
            "ask",
            Some("helm dep build"),
        ),
        (
            &[
                "helm",
                "dependency",
                "update",
                "./mychart",
                "--skip-refresh",
            ],
            &[
                "helm",
                "dependency",
                "update",
                "./mychart",
                "--skip-refresh",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("helm dependency update"),
        ),
        (
            &[
                "helm",
                "plugin",
                "install",
                "https://github.com/example/helm-plugin",
            ],
            &[
                "helm",
                "plugin",
                "install",
                "https://github.com/example/helm-plugin",
            ],
            &[false, false, false, false],
            "ask",
            Some("helm plugin install"),
        ),
        (
            &["helm", "plugin", "install", "./path/to/plugin"],
            &["helm", "plugin", "install", "./path/to/plugin"],
            &[false, false, false, false],
            "ask",
            Some("helm plugin install"),
        ),
        (
            &["helm", "plugin", "uninstall", "myplugin"],
            &["helm", "plugin", "uninstall", "myplugin"],
            &[false, false, false, false],
            "ask",
            Some("helm plugin uninstall"),
        ),
        (
            &["helm", "plugin", "update", "myplugin"],
            &["helm", "plugin", "update", "myplugin"],
            &[false, false, false, false],
            "ask",
            Some("helm plugin update"),
        ),
        (
            &["helm", "plugin", "package", "./myplugin"],
            &["helm", "plugin", "package", "./myplugin"],
            &[false, false, false, false],
            "ask",
            Some("helm plugin package"),
        ),
        (
            &["helm", "registry", "login", "registry.example.com"],
            &["helm", "registry", "login", "registry.example.com"],
            &[false, false, false, false],
            "ask",
            Some("helm registry login"),
        ),
        (
            &[
                "helm",
                "registry",
                "login",
                "registry.example.com",
                "-u",
                "user",
            ],
            &[
                "helm",
                "registry",
                "login",
                "registry.example.com",
                "-u",
                "user",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("helm registry login"),
        ),
        (
            &[
                "helm",
                "registry",
                "login",
                "registry.example.com",
                "--username",
                "user",
                "--password",
                "pass",
            ],
            &[
                "helm",
                "registry",
                "login",
                "registry.example.com",
                "--username",
                "user",
                "--password",
                "pass",
            ],
            &[false, false, false, false, false, false, false, false],
            "ask",
            Some("helm registry login"),
        ),
        (
            &["helm", "registry", "logout", "registry.example.com"],
            &["helm", "registry", "logout", "registry.example.com"],
            &[false, false, false, false],
            "ask",
            Some("helm registry logout"),
        ),
        (&["helm"], &["helm"], &[false], "ask", Some("helm")),
        (
            &["helm", "--debug", "list"],
            &["helm", "--debug", "list"],
            &[false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "-n", "production", "list"],
            &["helm", "-n", "production", "list"],
            &[false, false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "--namespace", "production", "list"],
            &["helm", "--namespace", "production", "list"],
            &[false, false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "--kube-context", "dev", "list"],
            &["helm", "--kube-context", "dev", "list"],
            &[false, false, false, false],
            "allow",
            Some("helm list"),
        ),
        (
            &["helm", "list", "--debug"],
            &["helm", "list", "--debug"],
            &[false, false, false],
            "allow",
            Some("helm list"),
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
