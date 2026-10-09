//! Port of `src/dippy/cli/ansible.py`.
//!
//! Handles ansible, ansible-playbook, ansible-vault, ansible-galaxy,
//! ansible-inventory, ansible-doc, ansible-pull, ansible-config,
//! ansible-console, ansible-lint, and ansible-test commands.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &[
    "ansible",
    "ansible-playbook",
    "ansible-vault",
    "ansible-galaxy",
    "ansible-inventory",
    "ansible-doc",
    "ansible-pull",
    "ansible-config",
    "ansible-console",
    "ansible-lint",
    "ansible-test",
];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Commands that are entirely safe (read-only).
const SAFE_COMMANDS: &[&str] = &[
    "ansible-doc",  // Documentation viewer
    "ansible-lint", // Linter (read-only)
];

fn has(tokens: &[String], flag: &str) -> bool {
    tokens.iter().any(|t| t == flag)
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let Some(cmd) = tokens.first() else {
        return Classification::ask_desc("ansible");
    };
    let cmd = cmd.as_str();

    // Check for help/version flags anywhere
    if has(tokens, "-h") || has(tokens, "--help") || has(tokens, "--version") {
        return Classification::allow_desc(cmd);
    }

    // Entirely safe commands
    if SAFE_COMMANDS.contains(&cmd) {
        return Classification::allow_desc(cmd);
    }

    // Route to specific handlers - returns (safe, action) tuple
    let (safe, action): (bool, String) = match cmd {
        "ansible" => (check_ansible(tokens), "run".into()),
        "ansible-playbook" => (check_ansible_playbook(tokens), "run".into()),
        "ansible-vault" => (check_ansible_vault(tokens), get_vault_action(tokens)),
        "ansible-galaxy" => (check_ansible_galaxy(tokens), get_galaxy_action(tokens)),
        "ansible-inventory" => (check_ansible_inventory(tokens), "write".into()),
        "ansible-pull" => (check_ansible_pull(tokens), "run".into()),
        "ansible-config" => (check_ansible_config(tokens), get_config_action(tokens)),
        "ansible-console" => (check_ansible_console(tokens), "interactive".into()),
        "ansible-test" => (check_ansible_test(tokens), get_test_action(tokens)),
        _ => (false, "run".into()),
    };

    if safe {
        return Classification::allow_desc(cmd);
    }
    Classification::ask_desc(format!("{cmd} {action}"))
}

/// Safe if: --list-hosts or --check/-C mode.
fn check_ansible(tokens: &[String]) -> bool {
    if has(tokens, "--list-hosts") {
        return true;
    }
    if has(tokens, "--check") || has(tokens, "-C") {
        return true;
    }
    false
}

/// Safe if: --syntax-check, --list-hosts, --list-tasks, --list-tags, or --check/-C.
fn check_ansible_playbook(tokens: &[String]) -> bool {
    const SAFE_FLAGS: &[&str] = &[
        "--syntax-check",
        "--list-hosts",
        "--list-tasks",
        "--list-tags",
        "--check",
        "-C",
    ];
    SAFE_FLAGS.iter().any(|flag| has(tokens, flag))
}

/// Safe if: view subcommand only.
fn check_ansible_vault(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return false;
    }

    // Find subcommand (skip flags)
    for token in &tokens[1..] {
        if token.starts_with('-') {
            continue;
        }
        // First non-flag is the subcommand
        return token == "view";
    }

    false
}

/// Safe if: list, search, info, verify actions.
fn check_ansible_galaxy(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return false;
    }

    // Find type (role/collection) and action
    let mut type_token: Option<&str> = None;
    let mut action_token: Option<&str> = None;

    for token in &tokens[1..] {
        if token.starts_with('-') {
            continue;
        }
        if type_token.is_none() {
            type_token = Some(token);
        } else if action_token.is_none() {
            action_token = Some(token);
            break;
        }
    }

    if !matches!(type_token, Some("role" | "collection")) {
        return false;
    }

    matches!(action_token, Some("list" | "search" | "info" | "verify"))
}

/// Safe if: --list, --host, or --graph WITHOUT --output.
fn check_ansible_inventory(tokens: &[String]) -> bool {
    // --output makes it unsafe (writes to file)
    if has(tokens, "--output") {
        return false;
    }

    ["--list", "--host", "--graph"]
        .iter()
        .any(|flag| has(tokens, flag))
}

/// Safe if: --list-hosts or --check (NOT -C which is --checkout).
fn check_ansible_pull(tokens: &[String]) -> bool {
    has(tokens, "--list-hosts") || has(tokens, "--check")
}

/// Safe if: list, dump, view, validate subcommands.
fn check_ansible_config(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return false;
    }

    // Find subcommand
    for token in &tokens[1..] {
        if token.starts_with('-') {
            continue;
        }
        return matches!(token.as_str(), "list" | "dump" | "view" | "validate");
    }

    false
}

/// Safe if: --list-hosts only.
fn check_ansible_console(tokens: &[String]) -> bool {
    has(tokens, "--list-hosts")
}

/// Safe if: env, sanity, units subcommands.
fn check_ansible_test(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return false;
    }

    // Find subcommand
    for token in &tokens[1..] {
        if token.starts_with('-') {
            continue;
        }
        return matches!(token.as_str(), "env" | "sanity" | "units");
    }

    false
}

/// Extract first non-flag token as subcommand.
fn get_subcommand(tokens: &[String]) -> &str {
    tokens
        .iter()
        .skip(1)
        .find(|t| !t.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("")
}

fn subcommand_or_run(tokens: &[String]) -> String {
    match get_subcommand(tokens) {
        "" => "run".into(),
        s => s.into(),
    }
}

/// Get ansible-vault action (encrypt, decrypt, view, etc.).
fn get_vault_action(tokens: &[String]) -> String {
    subcommand_or_run(tokens)
}

/// Get ansible-galaxy action (install, list, etc.).
fn get_galaxy_action(tokens: &[String]) -> String {
    // Format: ansible-galaxy role/collection action
    let parts: Vec<&String> = tokens
        .iter()
        .skip(1)
        .filter(|t| !t.starts_with('-'))
        .collect();
    if parts.len() >= 2 {
        return parts[1].clone(); // action is second non-flag
    }
    "run".into()
}

/// Get ansible-config action.
fn get_config_action(tokens: &[String]) -> String {
    subcommand_or_run(tokens)
}

/// Get ansible-test action.
fn get_test_action(tokens: &[String]) -> String {
    subcommand_or_run(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(cmd: &str) -> Classification {
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        classify(&HandlerContext::new(&tokens))
    }

    #[test]
    fn help_and_safe_commands() {
        assert_eq!(
            run("ansible-playbook site.yml --help").action,
            Action::Allow
        );
        assert_eq!(run("ansible-doc -l").action, Action::Allow);
        assert_eq!(run("ansible-lint site.yml").action, Action::Allow);
    }

    #[test]
    fn ad_hoc_and_playbook() {
        assert_eq!(run("ansible all -m ping").action, Action::Ask);
        assert_eq!(run("ansible all --list-hosts").action, Action::Allow);
        assert_eq!(run("ansible-playbook site.yml -C").action, Action::Allow);
        let r = run("ansible-playbook site.yml");
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("ansible-playbook run"));
        assert_eq!(run("ansible-pull -C main -U url").action, Action::Ask);
    }

    #[test]
    fn vault_galaxy_config_test() {
        assert_eq!(run("ansible-vault view x.yml").action, Action::Allow);
        let r = run("ansible-vault encrypt x.yml");
        assert_eq!(r.description.as_deref(), Some("ansible-vault encrypt"));
        assert_eq!(run("ansible-galaxy role list").action, Action::Allow);
        let r = run("ansible-galaxy collection install x");
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("ansible-galaxy install"));
        assert_eq!(
            run("ansible-galaxy").description.as_deref(),
            Some("ansible-galaxy run")
        );
        assert_eq!(run("ansible-config dump").action, Action::Allow);
        assert_eq!(run("ansible-config init").action, Action::Ask);
        assert_eq!(run("ansible-test sanity").action, Action::Allow);
        assert_eq!(run("ansible-test integration").action, Action::Ask);
    }

    #[test]
    fn inventory_and_console() {
        assert_eq!(run("ansible-inventory --list").action, Action::Allow);
        assert_eq!(
            run("ansible-inventory --list --output f").action,
            Action::Ask
        );
        assert_eq!(run("ansible-console --list-hosts").action, Action::Allow);
        assert_eq!(
            run("ansible-console").description.as_deref(),
            Some("ansible-console interactive")
        );
    }
}
