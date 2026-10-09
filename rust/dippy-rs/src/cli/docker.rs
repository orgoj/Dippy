//! Port of `src/dippy/cli/docker.py`: Docker command handler.
//!
//! Handles docker, docker-compose, and podman commands.

use super::{Classification, Describe, HandlerContext};
use crate::bash::bash_join;

pub const COMMANDS: &[&str] = &["docker", "docker-compose", "podman", "podman-compose"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one
/// (docker.py only has a private `_get_description`).
pub const DESCRIPTION: Option<Describe> = None;

/// Safe read-only actions (at the top level).
const SAFE_ACTIONS: &[&str] = &[
    "version", "help", "info", "ps", "images", "image", "inspect", "logs", "stats", "top", "port",
    "diff", "history", "search", "events", "system", "network", // Some subcommands are safe
    "volume",  // Some subcommands are safe
    "config",  // Some subcommands are safe
    "context", // Needs subcommand checking
    "export",  // Read-only (exports container to stdout)
    "save",    // Read-only (saves image to stdout)
];

/// Safe subcommands for multi-level commands (`SAFE_SUBCOMMANDS`).
fn safe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "image" => Some(&["ls", "list", "inspect", "history", "save"]),
        "container" => Some(&[
            "ls", "list", "inspect", "logs", "stats", "top", "port", "diff", "export",
        ]),
        "network" => Some(&["ls", "list", "inspect"]),
        "volume" => Some(&["ls", "list", "inspect"]),
        "system" => Some(&["df", "info", "events"]),
        "context" => Some(&["ls", "list", "inspect", "show"]),
        "config" => Some(&["ls", "inspect"]),
        "secret" => Some(&["ls", "inspect"]),
        "service" => Some(&["ls", "list", "inspect", "logs", "ps"]),
        "stack" => Some(&["ls", "ps", "services"]),
        "node" => Some(&["ls", "inspect", "ps"]),
        "compose" => Some(&[
            "ps", "logs", "config", "images", "ls", "top", "version", "port", "events",
        ]),
        "plugin" => Some(&["ls", "list", "inspect"]),
        "buildx" => Some(&["ls", "inspect", "du", "version"]), // imagetools handled specially
        "manifest" => Some(&["inspect"]),
        "trust" => Some(&["inspect"]),
        _ => None,
    }
}

/// Unsafe subcommands (`UNSAFE_SUBCOMMANDS`).
fn unsafe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "image" => Some(&[
            "rm", "prune", "build", "push", "pull", "tag", "import", "load",
        ]),
        "container" => Some(&[
            "rm", "prune", "create", "start", "stop", "restart", "kill", "exec",
        ]),
        "network" => Some(&["create", "rm", "prune", "connect", "disconnect"]),
        "volume" => Some(&["create", "rm", "prune"]),
        "system" => Some(&["prune"]),
        "context" => Some(&["create", "update", "use", "rm", "import"]),
        "compose" => Some(&[
            "up", "down", "start", "stop", "restart", "rm", "pull", "build", "exec", "run",
        ]),
        "config" => Some(&["create", "rm"]),
        "secret" => Some(&["create", "rm"]),
        "service" => Some(&["create", "rm", "scale", "update", "rollback"]),
        "stack" => Some(&["deploy", "rm"]),
        "node" => Some(&["update", "rm", "promote", "demote"]),
        "plugin" => Some(&[
            "install", "enable", "disable", "rm", "upgrade", "create", "push",
        ]),
        "buildx" => Some(&["build", "bake", "create", "rm", "use", "prune"]), // imagetools handled specially
        "manifest" => Some(&["create", "push", "annotate", "rm"]),
        "trust" => Some(&["sign", "revoke"]),
        "swarm" => Some(&[
            "init",
            "join",
            "join-token",
            "leave",
            "update",
            "ca",
            "unlock",
            "unlock-key",
        ]),
        _ => None,
    }
}

fn is_multi_level(action: &str) -> bool {
    safe_subcommands(action).is_some() || unsafe_subcommands(action).is_some()
}

/// Global flags that take an argument.
const GLOBAL_FLAGS_WITH_ARG: &[&str] = &[
    "-H",
    "--host",
    "-c",
    "--context",
    "-l",
    "--log-level",
    "--config",
    "--tlscacert",
    "--tlscert",
    "--tlskey",
];

/// Exec flags that take an argument.
const EXEC_FLAGS_WITH_ARG: &[&str] = &[
    "-e",
    "--env",
    "-w",
    "--workdir",
    "-u",
    "--user",
    "--env-file",
];

/// Compose flags that take an argument (`_check_compose`).
const COMPOSE_FLAGS_WITH_ARG: &[&str] = &[
    "-f",
    "--file",
    "-p",
    "--project-name",
    "--project-directory",
    "--env-file",
    "--profile",
    "--ansi",
];

const COMPOSE_SAFE_ACTIONS: &[&str] = &[
    "ps", "logs", "config", "images", "ls", "top", "version", "port", "events",
];

/// `_extract_exec_container_and_command`: container name and inner command.
fn extract_exec_container_and_command(tokens: &[String]) -> (Option<&str>, Option<&[String]>) {
    let mut i = 0;
    let mut container: Option<&str> = None;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if token == "--" {
            i += 1;
            if container.is_none() && i < tokens.len() {
                container = Some(tokens[i].as_str());
                i += 1;
            }
            break;
        }
        if EXEC_FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }
        if token.starts_with('-') {
            // --flag=value, boolean flag or unknown flag: skip one token
            i += 1;
            continue;
        }
        // First non-flag is container name
        container = Some(token);
        i += 1;
        break;
    }

    if i < tokens.len() && tokens[i] == "--" {
        i += 1;
    }

    let inner_tokens = if i < tokens.len() {
        Some(&tokens[i..])
    } else {
        None
    };
    (container, inner_tokens)
}

/// `_get_description`: description for a docker command.
fn docker_description(tokens: &[String]) -> String {
    let Some(first) = tokens.first() else {
        return "docker".into();
    };
    if tokens.len() < 2 {
        return first.clone();
    }
    let action_idx = find_action_idx(tokens);
    if action_idx >= tokens.len() {
        return first.clone();
    }
    let action = tokens[action_idx].as_str();
    let rest = &tokens[action_idx + 1..];
    // Only include subcommand in description for multi-level commands
    if is_multi_level(action) {
        if let Some(subcommand) = find_subcommand(rest) {
            if !subcommand.is_empty() {
                return format!("{first} {action} {subcommand}");
            }
        }
    }
    format!("{first} {action}")
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let Some(base) = tokens.first() else {
        // Python raises IndexError on empty tokens; fail closed.
        return Classification::ask_desc("docker");
    };
    let base = base.as_str();
    let mut desc = docker_description(tokens);

    if tokens.len() < 2 {
        return Classification::ask_desc(desc);
    }

    // Find action (skip global flags)
    let action_idx = find_action_idx(tokens);
    if action_idx >= tokens.len() {
        return Classification::ask_desc(desc);
    }

    let action = tokens[action_idx].as_str();
    let rest = &tokens[action_idx + 1..];

    // Handle docker-compose / docker compose
    if action == "compose" || base == "docker-compose" || base == "podman-compose" {
        let safe = check_compose(tokens, action_idx);
        return if safe {
            Classification::allow_desc(desc)
        } else {
            Classification::ask_desc(desc)
        };
    }

    // Check subcommands for multi-level commands
    if is_multi_level(action) {
        if let Some(subcommand) = find_subcommand(rest).filter(|s| !s.is_empty()) {
            // Handle nested subcommands (e.g., buildx imagetools inspect)
            if action == "buildx" && subcommand == "imagetools" {
                let sub_rest = match rest.iter().position(|t| t == subcommand) {
                    Some(pos) => &rest[pos + 1..],
                    None => &[],
                };
                let safe = find_subcommand(sub_rest) == Some("inspect");
                return if safe {
                    Classification::allow_desc(desc)
                } else {
                    Classification::ask_desc(desc)
                };
            }

            if safe_subcommands(action).is_some_and(|s| s.contains(&subcommand)) {
                // Special case: image save -o writes to file
                if action == "image" && subcommand == "save" && has_output_flag(rest) {
                    return Classification::ask_desc(desc);
                }
                return Classification::allow_desc(desc);
            }
            if unsafe_subcommands(action).is_some_and(|s| s.contains(&subcommand)) {
                return Classification::ask_desc(desc);
            }
        }
    }

    // Simple safe actions
    if SAFE_ACTIONS.contains(&action) {
        // export/save without -o writes to stdout (safe)
        if (action == "export" || action == "save") && has_output_flag(rest) {
            return Classification::ask_desc(desc);
        }
        return Classification::allow_desc(desc);
    }

    // Handle exec - delegate to inner command with remote mode
    if action == "exec" {
        let (container, inner_tokens) = extract_exec_container_and_command(rest);
        let container = container.filter(|c| !c.is_empty());
        if let Some(container) = container {
            desc = format!("{base} exec {container}");
        }
        if let Some(inner_tokens) = inner_tokens.filter(|t| !t.is_empty()) {
            let inner_cmd = bash_join(inner_tokens);
            let wrapper_name = if base.contains("podman") {
                "podman"
            } else {
                "docker"
            };
            let mut wrapper_context = vec![wrapper_name.to_string()];
            if let Some(container) = container {
                wrapper_context.push(container.to_string());
            }
            return Classification::delegate(inner_cmd)
                .desc(desc)
                .wrapper(wrapper_context)
                .remote(true);
        }
        return Classification::ask_desc(desc);
    }

    // Unsafe actions or unknown
    Classification::ask_desc(desc)
}

/// `_find_action_idx`: index of the docker action, skipping global flags.
fn find_action_idx(tokens: &[String]) -> usize {
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if token.starts_with('-') {
            if GLOBAL_FLAGS_WITH_ARG.contains(&token) && i + 1 < tokens.len() {
                i += 2; // Skip flag and its argument
            } else {
                i += 1; // Flag with value or boolean flag
            }
            continue;
        }
        return i;
    }
    tokens.len()
}

/// `_find_subcommand`: the first non-flag token.
fn find_subcommand(rest: &[String]) -> Option<&str> {
    rest.iter()
        .map(String::as_str)
        .find(|t| !t.starts_with('-'))
}

/// `_has_output_flag`: -o or --output flag is present.
fn has_output_flag(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|t| t == "-o" || t == "--output" || t.starts_with("-o") || t.starts_with("--output="))
}

/// `_check_compose`: docker-compose commands.
fn check_compose(tokens: &[String], start_idx: usize) -> bool {
    let mut i = if tokens[0] == "docker-compose" || tokens[0] == "podman-compose" {
        1
    } else if start_idx < tokens.len() && tokens[start_idx] == "compose" {
        start_idx + 1
    } else {
        start_idx
    };

    while i < tokens.len() {
        let token = tokens[i].as_str();
        if token.starts_with('-') {
            if COMPOSE_FLAGS_WITH_ARG.contains(&token) && i + 1 < tokens.len() {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        // Found the compose action
        return COMPOSE_SAFE_ACTIONS.contains(&token);
    }
    false
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
    fn exec_sets_wrapper_context() {
        let r = classify(&HandlerContext::new(&[
            "docker",
            "exec",
            "mycontainer",
            "fictional_cmd",
        ]));
        assert_eq!(r.action, Action::Delegate);
        assert_eq!(r.inner_command.as_deref(), Some("fictional_cmd"));
        assert_eq!(
            r.wrapper_context,
            Some(vec!["docker".to_string(), "mycontainer".to_string()])
        );
        assert_eq!(r.description.as_deref(), Some("docker exec mycontainer"));
        assert!(r.remote);
    }

    #[test]
    fn exec_with_flags_sets_wrapper_context() {
        let r = classify(&HandlerContext::new(&[
            "docker",
            "exec",
            "-it",
            "-u",
            "root",
            "--env",
            "FOO=bar",
            "mycontainer",
            "fictional_cmd",
            "arg",
        ]));
        assert_eq!(r.action, Action::Delegate);
        assert_eq!(r.inner_command.as_deref(), Some("fictional_cmd arg"));
        assert_eq!(
            r.wrapper_context,
            Some(vec!["docker".to_string(), "mycontainer".to_string()])
        );
        assert_eq!(r.description.as_deref(), Some("docker exec mycontainer"));
    }

    #[test]
    fn exec_with_double_dash_sets_wrapper_context() {
        let r = classify(&HandlerContext::new(&[
            "docker",
            "exec",
            "-it",
            "--",
            "mycontainer",
            "fictional_cmd",
        ]));
        assert_eq!(r.action, Action::Delegate);
        assert_eq!(r.inner_command.as_deref(), Some("fictional_cmd"));
        assert_eq!(
            r.wrapper_context,
            Some(vec!["docker".to_string(), "mycontainer".to_string()])
        );
    }

    #[test]
    fn podman_exec_sets_wrapper_context() {
        let r = classify(&HandlerContext::new(&[
            "podman",
            "exec",
            "mycontainer",
            "fictional_cmd",
        ]));
        assert_eq!(r.action, Action::Delegate);
        assert_eq!(r.inner_command.as_deref(), Some("fictional_cmd"));
        assert_eq!(
            r.wrapper_context,
            Some(vec!["podman".to_string(), "mycontainer".to_string()])
        );
        assert_eq!(r.description.as_deref(), Some("podman exec mycontainer"));
    }

    #[test]
    fn exec_without_inner_command_no_wrapper_context() {
        let r = classify(&HandlerContext::new(&["docker", "exec", "mycontainer"]));
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.wrapper_context, None);
        assert_eq!(r.description.as_deref(), Some("docker exec mycontainer"));
    }

    #[test]
    fn exec_requotes_inner_command() {
        let r = classify(&HandlerContext::new(&[
            "docker", "exec", "c", "--", "sh", "-c", "echo a;b",
        ]));
        assert_eq!(r.inner_command.as_deref(), Some("sh -c 'echo a;b'"));
    }

    #[test]
    fn classifies() {
        for (cmd, allowed) in [
            ("docker", false),
            ("docker ps", true),
            ("docker ps -a", true),
            ("docker images", true),
            ("docker --context prod ps", true),
            ("docker -H tcp://x:2375 ps", true),
            ("docker inspect x", true),
            ("docker logs -f x", true),
            ("docker version", true),
            ("docker info", true),
            ("docker export x", true),
            ("docker export -o out.tar x", false),
            ("docker save img", true),
            ("docker save -o out.tar img", false),
            ("docker save --output=out.tar img", false),
            ("docker image ls", true),
            ("docker image save img", true),
            ("docker image save -o x.tar img", false),
            ("docker image rm img", false),
            ("docker image", true),
            ("docker image unknown", true),
            ("docker container ls", true),
            ("docker container rm x", false),
            ("docker container", false),
            ("docker network ls", true),
            ("docker network create x", false),
            ("docker volume prune", false),
            ("docker system df", true),
            ("docker system prune", false),
            ("docker context use x", false),
            ("docker secret ls", true),
            ("docker secret create x", false),
            ("docker buildx ls", true),
            ("docker buildx build .", false),
            ("docker buildx imagetools inspect img", true),
            ("docker buildx imagetools create x", false),
            ("docker buildx imagetools", false),
            ("docker swarm init", false),
            ("docker compose ps", true),
            ("docker compose -f x.yml logs", true),
            ("docker compose up -d", false),
            ("docker compose", false),
            ("docker-compose ps", true),
            ("docker-compose --profile dev config", true),
            ("docker-compose up", false),
            ("podman-compose logs", true),
            ("podman ps", true),
            ("docker run ubuntu", false),
            ("docker rm x", false),
            ("docker pull img", false),
            ("docker build .", false),
            ("docker unknown", false),
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
    fn descriptions() {
        for (cmd, desc) in [
            ("docker", "docker"),
            ("docker --debug", "docker"),
            ("docker ps -a", "docker ps"),
            ("docker image ls", "docker image ls"),
            ("docker image", "docker image"),
            ("docker compose up", "docker compose up"),
            ("docker-compose up", "docker-compose up"),
            ("docker exec -it", "docker exec"),
        ] {
            assert_eq!(
                classify_cmd(cmd).description.as_deref(),
                Some(desc),
                "{cmd}"
            );
        }
    }
}
