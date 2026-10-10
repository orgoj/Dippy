//! Port of `src/dippy/cli/kubectl.py`: Kubectl command handler.
//!
//! Handles kubectl and similar Kubernetes CLI tools.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["kubectl", "k"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe read-only actions.
const SAFE_ACTIONS: &[&str] = &[
    "get",
    "describe",
    "explain",
    "logs",
    "top",
    "cluster-info",
    "version",
    "api-resources",
    "api-versions",
    "config",     // Most config operations are read-only
    "auth",       // auth can-i is read-only
    "wait",       // Polling is read-only
    "diff",       // Shows differences without applying
    "plugin",     // Plugin management (list is read-only)
    "completion", // Shell completion scripts
    "kustomize",  // Build kustomize manifests (output only)
];

/// Safe subcommands for multi-level commands (`SAFE_SUBCOMMANDS`).
fn safe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "config" => Some(&[
            "view",
            "get-contexts",
            "get-clusters",
            "current-context",
            "get-users",
        ]),
        "auth" => Some(&["can-i", "whoami"]),
        "rollout" => Some(&["status", "history"]),
        _ => None,
    }
}

/// Unsafe subcommands (`UNSAFE_SUBCOMMANDS`).
fn unsafe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "config" => Some(&[
            "set",
            "set-context",
            "set-cluster",
            "set-credentials",
            "delete-context",
            "delete-cluster",
            "delete-user",
            "use-context",
            "use",
            "rename-context",
        ]),
        "rollout" => Some(&["restart", "pause", "resume", "undo"]),
        _ => None,
    }
}

const SECRET_RESOURCES: &[&str] = &["secret", "secrets"];

const SAFE_OUTPUT_FORMATS: &[&str] = &["name", "wide"];

/// Flags (after the verb) that consume the next token as a value.
const POST_VERB_FLAGS_WITH_ARG: &[&str] = &[
    "-o",
    "--output",
    "-n",
    "--namespace",
    "-l",
    "--selector",
    "-f",
    "--filename",
    "--field-selector",
    "--sort-by",
    "--template",
    "--context",
    "--cluster",
];

/// Global flags (before the verb) that consume the next token.
const GLOBAL_FLAGS_WITH_ARG: &[&str] = &[
    "-n",
    "--namespace",
    "-l",
    "--selector",
    "-o",
    "--output",
    "--context",
    "--cluster",
    "-f",
    "--filename",
];

/// `_is_secret_data_exposure`: a get command targets secrets with a
/// data-exposing output format (expansions are treated conservatively).
fn is_secret_data_exposure(
    tokens: &[String],
    rest: &[String],
    word_has_expansions: &[bool],
    rest_offset: usize,
) -> bool {
    // Find resource type: first non-flag token in rest
    let mut resource: Option<(&str, usize)> = None;
    let mut i = 0;
    while i < rest.len() {
        let token = rest[i].as_str();
        if POST_VERB_FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        resource = Some((token, rest_offset + i));
        break;
    }

    let Some((resource_type, resource_abs_pos)) = resource else {
        return false;
    };

    // If the resource came from an expansion, it could resolve to "secret"
    if has_expansion(word_has_expansions, resource_abs_pos) {
        return true;
    }

    // Handle comma-separated resources (e.g., "secret,configmap") and
    // type/name syntax (e.g., "secret/my-secret")
    if !resource_type
        .split(',')
        .any(|p| SECRET_RESOURCES.contains(&p.split('/').next().unwrap_or("")))
    {
        return false;
    }

    // Resource IS secrets -- if any remaining token came from an expansion, it
    // could inject a data-exposing format like -o yaml
    if has_expansion_after(word_has_expansions, resource_abs_pos + 1) {
        return true;
    }

    // Find output format from full token list (-o can appear before or after verb)
    let mut output_format: Option<&str> = None;
    for (j, token) in tokens.iter().enumerate() {
        if (token == "-o" || token == "--output") && j + 1 < tokens.len() {
            output_format = Some(tokens[j + 1].as_str());
            break;
        }
        if let Some(value) = token.strip_prefix("--output=") {
            output_format = Some(value);
            break;
        }
        // Python: len(token) > 2 and token[:2] == "-o" and token[2] != "-"
        if let Some(value) = token.strip_prefix("-o")
            && !value.is_empty()
            && !value.starts_with('-')
        {
            output_format = Some(value);
            break;
        }
    }

    let Some(output_format) = output_format else {
        return false;
    };

    // Extract format name before any = (e.g., "jsonpath='{.data}'" -> "jsonpath")
    let format_name = output_format.split('=').next().unwrap_or("");
    !SAFE_OUTPUT_FORMATS.contains(&format_name)
}

/// `_extract_exec_inner_command`: command after the `--` separator.
fn extract_exec_inner_command(tokens: &[String]) -> Option<&[String]> {
    let sep_idx = tokens.iter().position(|t| t == "--")?;
    let result = &tokens[sep_idx + 1..];
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// `_has_expansion`: the token at pos was built from a bash expansion.
fn has_expansion(word_has_expansions: &[bool], pos: usize) -> bool {
    word_has_expansions.get(pos).copied().unwrap_or(false)
}

/// `_has_expansion_after`: any token at or after start has an expansion.
fn has_expansion_after(word_has_expansions: &[bool], start: usize) -> bool {
    word_has_expansions
        .get(start..)
        .is_some_and(|s| s.iter().any(|&b| b))
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let expansions = ctx.word_has_expansions.as_slice();
    let base = tokens.first().map(String::as_str).unwrap_or("kubectl");
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    // Find the action (skip global flags)
    let mut action: Option<&str> = None;
    let mut action_idx = 1;

    while action_idx < tokens.len() {
        let token = tokens[action_idx].as_str();
        if token.starts_with('-') {
            if GLOBAL_FLAGS_WITH_ARG.contains(&token) {
                action_idx += 2;
                continue;
            }
            action_idx += 1;
            continue;
        }
        action = Some(token);
        break;
    }

    // Python `if not action` also rejects an empty-string token.
    let action = match action {
        Some(a) if !a.is_empty() => a,
        _ => return Classification::ask_desc(base),
    };

    let rest: &[String] = tokens.get(action_idx + 1..).unwrap_or(&[]);
    let rest_offset = action_idx + 1;
    let desc = format!("{base} {action}");

    // Check for subcommands first (config/auth/rollout)
    if let Some(safe) = safe_subcommands(action) {
        for (idx, token) in rest.iter().enumerate() {
            if token.starts_with('-') {
                continue;
            }
            let abs_pos = rest_offset + idx;
            if has_expansion(expansions, abs_pos) {
                return Classification::ask_desc(desc);
            }
            if safe.contains(&token.as_str()) {
                // config view --raw exposes unredacted kubeconfig credentials
                if action == "config"
                    && token == "view"
                    && (rest.iter().any(|t| t == "--raw")
                        || has_expansion_after(expansions, abs_pos + 1))
                {
                    return Classification::ask_desc(format!("{desc} {token}"));
                }
                return Classification::allow_desc(format!("{desc} {token}"));
            }
            break;
        }
    }

    if let Some(unsafe_subs) = unsafe_subcommands(action) {
        for (idx, token) in rest.iter().enumerate() {
            if token.starts_with('-') {
                continue;
            }
            let abs_pos = rest_offset + idx;
            if has_expansion(expansions, abs_pos) {
                return Classification::ask_desc(desc);
            }
            if unsafe_subs.contains(&token.as_str()) {
                return Classification::ask_desc(format!("{desc} {token}"));
            }
            break;
        }
    }

    // Sensitive data checks (before blanket safe-action approval)
    if action == "get" && is_secret_data_exposure(tokens, rest, expansions, rest_offset) {
        return Classification::ask_desc(format!("{desc} (secret data)"));
    }

    // Simple safe actions
    if SAFE_ACTIONS.contains(&action) {
        return Classification::allow_desc(desc);
    }

    // Handle exec - delegate to inner command with remote mode
    if action == "exec" {
        if let Some(inner_tokens) = extract_exec_inner_command(rest) {
            let inner_cmd = bash_join(inner_tokens);
            return Classification::delegate(inner_cmd).desc(desc).remote(true);
        }
        return Classification::ask_desc(desc);
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

    fn classify_exp(tokens: &[&str], exp: &[bool]) -> Classification {
        let mut ctx = HandlerContext::new(tokens);
        ctx.word_has_expansions = exp.to_vec();
        classify(&ctx)
    }

    #[test]
    fn classifies() {
        for (cmd, allowed) in [
            ("kubectl", false),
            ("kubectl get pods", true),
            ("k get pods -A", true),
            ("kubectl -n kube-system get pods", true),
            ("kubectl --context prod describe pod x", true),
            ("kubectl logs pod/x -f", true),
            ("kubectl top nodes", true),
            ("kubectl version", true),
            ("kubectl api-resources", true),
            ("kubectl config view", true),
            ("kubectl config view --raw", false),
            ("kubectl config get-contexts", true),
            ("kubectl config current-context", true),
            ("kubectl config use-context prod", false),
            ("kubectl config set-context x", false),
            ("kubectl config", true),
            ("kubectl auth can-i list pods", true),
            ("kubectl auth whoami", true),
            ("kubectl rollout status deploy/x", true),
            ("kubectl rollout history deploy/x", true),
            ("kubectl rollout restart deploy/x", false),
            ("kubectl rollout undo deploy/x", false),
            ("kubectl rollout", false),
            ("kubectl apply -f x.yaml", false),
            ("kubectl delete pod x", false),
            ("kubectl scale deploy x --replicas=3", false),
            ("kubectl port-forward svc/x 8080:80", false),
            ("kubectl exec -it pod -- bash", false),
            ("kubectl exec pod", false),
            ("kubectl -n", false),
            ("kubectl --verbose", false),
            ("kubectl get secrets", true),
            ("kubectl get secret x", true),
            ("kubectl get secret x -o yaml", false),
            ("kubectl get secret x -ojson", false),
            ("kubectl get secret x --output=jsonpath={.data}", false),
            ("kubectl -o yaml get secret x", false),
            ("kubectl get secret x -o name", true),
            ("kubectl get secret x -o wide", true),
            ("kubectl get secret,configmap -o yaml", false),
            ("kubectl get secrets/x -o json", false),
            ("kubectl get configmap -o yaml", true),
            ("kubectl get -n ns secret -o yaml", false),
            ("kubectl get pods -o yaml", true),
        ] {
            let c = classify_cmd(cmd);
            if allowed {
                assert_eq!(c.action, Action::Allow, "{cmd}");
            } else {
                assert_ne!(c.action, Action::Allow, "{cmd}");
            }
        }
    }

    #[test]
    fn exec_delegates_remote() {
        let c = classify_cmd("kubectl exec -it mypod -- ls -la /tmp");
        assert_eq!(c.action, Action::Delegate);
        assert_eq!(c.inner_command.as_deref(), Some("ls -la /tmp"));
        assert_eq!(c.description.as_deref(), Some("kubectl exec"));
        assert!(c.remote);
        let c = classify(&HandlerContext::new(&[
            "kubectl", "exec", "p", "--", "sh", "-c", "echo a;b",
        ]));
        assert_eq!(c.inner_command.as_deref(), Some("sh -c 'echo a;b'"));
        let c = classify_cmd("kubectl exec p --");
        assert_eq!(c.action, Action::Ask);
    }

    #[test]
    fn expansions_ask() {
        // Resource from an expansion could be a secret.
        let c = classify_exp(
            &["kubectl", "get", "$R", "-o", "yaml"],
            &[false, false, true],
        );
        assert_eq!(c.action, Action::Ask);
        // Secret with an expanded later token could inject -o yaml.
        let c = classify_exp(
            &["kubectl", "get", "secret", "$FMT"],
            &[false, false, false, true],
        );
        assert_eq!(c.action, Action::Ask);
        // Non-secret resource with expansions stays allowed.
        let c = classify_exp(
            &["kubectl", "get", "pods", "$X"],
            &[false, false, false, true],
        );
        assert_eq!(c.action, Action::Allow);
        // Expanded subcommand asks.
        let c = classify_exp(&["kubectl", "config", "$SUB"], &[false, false, true]);
        assert_eq!(c.action, Action::Ask);
        assert_eq!(c.description.as_deref(), Some("kubectl config"));
        // config view followed by an expansion could be --raw.
        let c = classify_exp(
            &["kubectl", "config", "view", "$F"],
            &[false, false, false, true],
        );
        assert_eq!(c.action, Action::Ask);
        let c = classify_exp(&["kubectl", "rollout", "$S"], &[false, false, true]);
        assert_eq!(c.action, Action::Ask);
    }

    #[test]
    fn descriptions() {
        assert_eq!(
            classify_cmd("kubectl get secret -o yaml")
                .description
                .as_deref(),
            Some("kubectl get (secret data)")
        );
        assert_eq!(
            classify_cmd("k config use-context x")
                .description
                .as_deref(),
            Some("k config use-context")
        );
        let c = classify(&HandlerContext::new(&["kubectl", "-v", ""]));
        assert_eq!(c.description.as_deref(), Some("kubectl"));
    }
}
