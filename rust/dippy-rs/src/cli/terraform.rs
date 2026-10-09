//! Port of `src/dippy/cli/terraform.py`.
//!
//! Handles terraform and tofu (OpenTofu) commands.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["terraform", "tf"];
pub const PORTED: bool = true;
/// The Python module defines no `get_description`.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe read-only actions.
const SAFE_ACTIONS: &[&str] = &[
    "version",
    "help",
    "fmt",       // Formatting (can modify files but typically wanted)
    "validate",  // Syntax check only
    "plan",      // Shows changes without applying
    "show",      // Show state
    "state",     // State inspection (some subcommands are safe)
    "output",    // Show outputs
    "graph",     // Generate dependency graph
    "providers", // List providers
    "console",   // Interactive console (read-only)
    "workspace", // Some subcommands are safe
    "get",       // Downloads modules (doesn't modify infra)
    "modules",   // Shows module information
    "metadata",  // Shows metadata (like functions)
    "test",      // Runs tests (doesn't modify infra)
    "refresh",   // Updates state to match real-world (read-only for infra)
];

/// Safe subcommands (`SAFE_SUBCOMMANDS`).
fn safe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "state" => Some(&["list", "show", "pull"]),
        // select changes context but not resources
        "workspace" => Some(&["list", "show", "select"]),
        _ => None,
    }
}

/// Unsafe subcommands (`UNSAFE_SUBCOMMANDS`).
fn unsafe_subcommands(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "state" => Some(&["mv", "rm", "push", "replace-provider"]),
        "workspace" => Some(&["new", "delete"]),
        _ => None,
    }
}

/// Classify terraform command.
pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map_or("terraform", String::as_str);
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    // Check for -help flag anywhere (common pattern: terraform -help)
    if tokens
        .iter()
        .any(|t| t == "-help" || t == "--help" || t == "-h")
    {
        return Classification::allow_desc(format!("{base} --help"));
    }

    // Find action (skip global flags)
    let mut action: Option<&str> = None;
    let mut action_idx = 1;

    while action_idx < tokens.len() {
        let token = tokens[action_idx].as_str();

        if token.starts_with('-') {
            if matches!(token, "-chdir" | "-var" | "-var-file") {
                action_idx += 2;
                continue;
            }
            action_idx += 1;
            continue;
        }

        action = Some(token);
        break;
    }

    // Python `if not action` (None or empty string)
    let action = match action {
        Some(a) if !a.is_empty() => a,
        _ => return Classification::ask_desc(base),
    };

    let rest: &[String] = tokens.get(action_idx + 1..).unwrap_or(&[]);
    let desc = format!("{base} {action}");

    // Check subcommands
    if let Some(safe) = safe_subcommands(action)
        && !rest.is_empty()
        && let Some(subcommand) = find_subcommand(rest)
        && !subcommand.is_empty()
    {
        let sub_desc = format!("{desc} {subcommand}");
        if safe.contains(&subcommand) {
            return Classification::allow_desc(sub_desc);
        }
        if unsafe_subcommands(action).is_some_and(|u| u.contains(&subcommand)) {
            return Classification::ask_desc(sub_desc);
        }
    }

    if let Some(unsafe_subs) = unsafe_subcommands(action)
        && !rest.is_empty()
        && let Some(subcommand) = find_subcommand(rest)
        && !subcommand.is_empty()
        && unsafe_subs.contains(&subcommand)
    {
        return Classification::ask_desc(format!("{desc} {subcommand}"));
    }

    // Simple safe actions
    if SAFE_ACTIONS.contains(&action) {
        return Classification::allow_desc(desc);
    }

    Classification::ask_desc(desc)
}

/// Find the first non-flag token (the subcommand).
fn find_subcommand(rest: &[String]) -> Option<&str> {
    rest.iter()
        .map(String::as_str)
        .find(|token| !token.starts_with('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_action_asks() {
        let r = classify(&HandlerContext::new(&["terraform", ""]));
        assert_eq!(r.action.as_str(), "ask");
        assert_eq!(r.description.as_deref(), Some("terraform"));
    }

    #[test]
    fn flag_value_skipped_past_end() {
        let r = classify(&HandlerContext::new(&["terraform", "-chdir"]));
        assert_eq!(r.action.as_str(), "ask");
        assert_eq!(r.description.as_deref(), Some("terraform"));
    }

    /// Generated from the Python handler on the commands of
    /// `tests/cli/test_terraform.py` (see `rust/parity/cases/terraform.txt`).
    #[allow(clippy::type_complexity)]
    const PY_CASES: &[(&[&str], &[&str], &[bool], &str, Option<&str>)] = &[
        (
            &["terraform", "plan"],
            &["terraform", "plan"],
            &[false, false],
            "allow",
            Some("terraform plan"),
        ),
        (
            &["terraform", "plan", "-out=plan.tfplan"],
            &["terraform", "plan", "-out=plan.tfplan"],
            &[false, false, false],
            "allow",
            Some("terraform plan"),
        ),
        (
            &["terraform", "plan", "-var", "name=value"],
            &["terraform", "plan", "-var", "'name=value'"],
            &[false, false, false, false],
            "allow",
            Some("terraform plan"),
        ),
        (
            &["terraform", "plan", "-var-file=vars.tfvars"],
            &["terraform", "plan", "-var-file=vars.tfvars"],
            &[false, false, false],
            "allow",
            Some("terraform plan"),
        ),
        (
            &["terraform", "plan", "-target=aws_instance.foo"],
            &["terraform", "plan", "-target=aws_instance.foo"],
            &[false, false, false],
            "allow",
            Some("terraform plan"),
        ),
        (
            &["terraform", "plan", "-destroy"],
            &["terraform", "plan", "-destroy"],
            &[false, false, false],
            "allow",
            Some("terraform plan"),
        ),
        (
            &["terraform", "plan", "-refresh-only"],
            &["terraform", "plan", "-refresh-only"],
            &[false, false, false],
            "allow",
            Some("terraform plan"),
        ),
        (
            &["terraform", "plan", "-json"],
            &["terraform", "plan", "-json"],
            &[false, false, false],
            "allow",
            Some("terraform plan"),
        ),
        (
            &["terraform", "show"],
            &["terraform", "show"],
            &[false, false],
            "allow",
            Some("terraform show"),
        ),
        (
            &["terraform", "show", "plan.tfplan"],
            &["terraform", "show", "plan.tfplan"],
            &[false, false, false],
            "allow",
            Some("terraform show"),
        ),
        (
            &["terraform", "show", "-json"],
            &["terraform", "show", "-json"],
            &[false, false, false],
            "allow",
            Some("terraform show"),
        ),
        (
            &["terraform", "show", "-json", "plan.tfplan"],
            &["terraform", "show", "-json", "plan.tfplan"],
            &[false, false, false, false],
            "allow",
            Some("terraform show"),
        ),
        (
            &["terraform", "state", "list"],
            &["terraform", "state", "list"],
            &[false, false, false],
            "allow",
            Some("terraform state list"),
        ),
        (
            &["terraform", "state", "list", "aws_instance.foo"],
            &["terraform", "state", "list", "aws_instance.foo"],
            &[false, false, false, false],
            "allow",
            Some("terraform state list"),
        ),
        (
            &["terraform", "state", "show", "aws_instance.foo"],
            &["terraform", "state", "show", "aws_instance.foo"],
            &[false, false, false, false],
            "allow",
            Some("terraform state show"),
        ),
        (
            &["terraform", "state", "show", "-json", "aws_instance.foo"],
            &["terraform", "state", "show", "-json", "aws_instance.foo"],
            &[false, false, false, false, false],
            "allow",
            Some("terraform state show"),
        ),
        (
            &["terraform", "state", "pull"],
            &["terraform", "state", "pull"],
            &[false, false, false],
            "allow",
            Some("terraform state pull"),
        ),
        (
            &["terraform", "validate"],
            &["terraform", "validate"],
            &[false, false],
            "allow",
            Some("terraform validate"),
        ),
        (
            &["terraform", "validate", "-json"],
            &["terraform", "validate", "-json"],
            &[false, false, false],
            "allow",
            Some("terraform validate"),
        ),
        (
            &["terraform", "validate", "-no-color"],
            &["terraform", "validate", "-no-color"],
            &[false, false, false],
            "allow",
            Some("terraform validate"),
        ),
        (
            &["terraform", "fmt"],
            &["terraform", "fmt"],
            &[false, false],
            "allow",
            Some("terraform fmt"),
        ),
        (
            &["terraform", "fmt", "-check"],
            &["terraform", "fmt", "-check"],
            &[false, false, false],
            "allow",
            Some("terraform fmt"),
        ),
        (
            &["terraform", "fmt", "-diff"],
            &["terraform", "fmt", "-diff"],
            &[false, false, false],
            "allow",
            Some("terraform fmt"),
        ),
        (
            &["terraform", "fmt", "-recursive"],
            &["terraform", "fmt", "-recursive"],
            &[false, false, false],
            "allow",
            Some("terraform fmt"),
        ),
        (
            &["terraform", "fmt", "-write=false"],
            &["terraform", "fmt", "-write=false"],
            &[false, false, false],
            "allow",
            Some("terraform fmt"),
        ),
        (
            &["terraform", "fmt", "-list=false"],
            &["terraform", "fmt", "-list=false"],
            &[false, false, false],
            "allow",
            Some("terraform fmt"),
        ),
        (
            &["terraform", "output"],
            &["terraform", "output"],
            &[false, false],
            "allow",
            Some("terraform output"),
        ),
        (
            &["terraform", "output", "my_output"],
            &["terraform", "output", "my_output"],
            &[false, false, false],
            "allow",
            Some("terraform output"),
        ),
        (
            &["terraform", "output", "-json"],
            &["terraform", "output", "-json"],
            &[false, false, false],
            "allow",
            Some("terraform output"),
        ),
        (
            &["terraform", "output", "-raw", "my_output"],
            &["terraform", "output", "-raw", "my_output"],
            &[false, false, false, false],
            "allow",
            Some("terraform output"),
        ),
        (
            &["terraform", "output", "-state=terraform.tfstate"],
            &["terraform", "output", "-state=terraform.tfstate"],
            &[false, false, false],
            "allow",
            Some("terraform output"),
        ),
        (
            &["terraform", "providers"],
            &["terraform", "providers"],
            &[false, false],
            "allow",
            Some("terraform providers"),
        ),
        (
            &["terraform", "providers", "lock"],
            &["terraform", "providers", "lock"],
            &[false, false, false],
            "allow",
            Some("terraform providers"),
        ),
        (
            &["terraform", "providers", "mirror", "./providers"],
            &["terraform", "providers", "mirror", "./providers"],
            &[false, false, false, false],
            "allow",
            Some("terraform providers"),
        ),
        (
            &["terraform", "providers", "schema", "-json"],
            &["terraform", "providers", "schema", "-json"],
            &[false, false, false, false],
            "allow",
            Some("terraform providers"),
        ),
        (
            &["terraform", "graph"],
            &["terraform", "graph"],
            &[false, false],
            "allow",
            Some("terraform graph"),
        ),
        (
            &["terraform", "graph", "-type=plan"],
            &["terraform", "graph", "-type=plan"],
            &[false, false, false],
            "allow",
            Some("terraform graph"),
        ),
        (
            &["terraform", "graph", "-draw-cycles"],
            &["terraform", "graph", "-draw-cycles"],
            &[false, false, false],
            "allow",
            Some("terraform graph"),
        ),
        (
            &["terraform", "console"],
            &["terraform", "console"],
            &[false, false],
            "allow",
            Some("terraform console"),
        ),
        (
            &["terraform", "console", "-var", "name=value"],
            &["terraform", "console", "-var", "'name=value'"],
            &[false, false, false, false],
            "allow",
            Some("terraform console"),
        ),
        (
            &["terraform", "get"],
            &["terraform", "get"],
            &[false, false],
            "allow",
            Some("terraform get"),
        ),
        (
            &["terraform", "get", "-update"],
            &["terraform", "get", "-update"],
            &[false, false, false],
            "allow",
            Some("terraform get"),
        ),
        (
            &["terraform", "version"],
            &["terraform", "version"],
            &[false, false],
            "allow",
            Some("terraform version"),
        ),
        (
            &["terraform", "version", "-json"],
            &["terraform", "version", "-json"],
            &[false, false, false],
            "allow",
            Some("terraform version"),
        ),
        (
            &["terraform", "modules"],
            &["terraform", "modules"],
            &[false, false],
            "allow",
            Some("terraform modules"),
        ),
        (
            &["terraform", "modules", "-json"],
            &["terraform", "modules", "-json"],
            &[false, false, false],
            "allow",
            Some("terraform modules"),
        ),
        (
            &["terraform", "metadata", "functions"],
            &["terraform", "metadata", "functions"],
            &[false, false, false],
            "allow",
            Some("terraform metadata"),
        ),
        (
            &["terraform", "metadata", "functions", "-json"],
            &["terraform", "metadata", "functions", "-json"],
            &[false, false, false, false],
            "allow",
            Some("terraform metadata"),
        ),
        (
            &["terraform", "test"],
            &["terraform", "test"],
            &[false, false],
            "allow",
            Some("terraform test"),
        ),
        (
            &["terraform", "test", "-filter=test_file.tftest.hcl"],
            &["terraform", "test", "-filter=test_file.tftest.hcl"],
            &[false, false, false],
            "allow",
            Some("terraform test"),
        ),
        (
            &["terraform", "test", "-json"],
            &["terraform", "test", "-json"],
            &[false, false, false],
            "allow",
            Some("terraform test"),
        ),
        (
            &["terraform", "refresh"],
            &["terraform", "refresh"],
            &[false, false],
            "allow",
            Some("terraform refresh"),
        ),
        (
            &["terraform", "refresh", "-target=aws_instance.foo"],
            &["terraform", "refresh", "-target=aws_instance.foo"],
            &[false, false, false],
            "allow",
            Some("terraform refresh"),
        ),
        (
            &["terraform", "--help"],
            &["terraform", "--help"],
            &[false, false],
            "allow",
            Some("terraform --help"),
        ),
        (
            &["terraform", "-help"],
            &["terraform", "-help"],
            &[false, false],
            "allow",
            Some("terraform --help"),
        ),
        (
            &["terraform", "plan", "--help"],
            &["terraform", "plan", "--help"],
            &[false, false, false],
            "allow",
            Some("terraform --help"),
        ),
        (
            &["terraform", "--version"],
            &["terraform", "--version"],
            &[false, false],
            "ask",
            Some("terraform"),
        ),
        (
            &["terraform", "workspace", "list"],
            &["terraform", "workspace", "list"],
            &[false, false, false],
            "allow",
            Some("terraform workspace list"),
        ),
        (
            &["terraform", "workspace", "show"],
            &["terraform", "workspace", "show"],
            &[false, false, false],
            "allow",
            Some("terraform workspace show"),
        ),
        (
            &["terraform", "workspace", "select", "default"],
            &["terraform", "workspace", "select", "default"],
            &[false, false, false, false],
            "allow",
            Some("terraform workspace select"),
        ),
        (
            &["terraform", "workspace", "select", "-or-create", "dev"],
            &["terraform", "workspace", "select", "-or-create", "dev"],
            &[false, false, false, false, false],
            "allow",
            Some("terraform workspace select"),
        ),
        (
            &["terraform", "apply"],
            &["terraform", "apply"],
            &[false, false],
            "ask",
            Some("terraform apply"),
        ),
        (
            &["terraform", "apply", "-auto-approve"],
            &["terraform", "apply", "-auto-approve"],
            &[false, false, false],
            "ask",
            Some("terraform apply"),
        ),
        (
            &["terraform", "apply", "plan.tfplan"],
            &["terraform", "apply", "plan.tfplan"],
            &[false, false, false],
            "ask",
            Some("terraform apply"),
        ),
        (
            &["terraform", "apply", "-var", "name=value"],
            &["terraform", "apply", "-var", "'name=value'"],
            &[false, false, false, false],
            "ask",
            Some("terraform apply"),
        ),
        (
            &["terraform", "apply", "-target=aws_instance.foo"],
            &["terraform", "apply", "-target=aws_instance.foo"],
            &[false, false, false],
            "ask",
            Some("terraform apply"),
        ),
        (
            &["terraform", "destroy"],
            &["terraform", "destroy"],
            &[false, false],
            "ask",
            Some("terraform destroy"),
        ),
        (
            &["terraform", "destroy", "-auto-approve"],
            &["terraform", "destroy", "-auto-approve"],
            &[false, false, false],
            "ask",
            Some("terraform destroy"),
        ),
        (
            &["terraform", "destroy", "-target=aws_instance.foo"],
            &["terraform", "destroy", "-target=aws_instance.foo"],
            &[false, false, false],
            "ask",
            Some("terraform destroy"),
        ),
        (
            &["terraform", "init"],
            &["terraform", "init"],
            &[false, false],
            "ask",
            Some("terraform init"),
        ),
        (
            &["terraform", "init", "-upgrade"],
            &["terraform", "init", "-upgrade"],
            &[false, false, false],
            "ask",
            Some("terraform init"),
        ),
        (
            &["terraform", "init", "-reconfigure"],
            &["terraform", "init", "-reconfigure"],
            &[false, false, false],
            "ask",
            Some("terraform init"),
        ),
        (
            &["terraform", "init", "-migrate-state"],
            &["terraform", "init", "-migrate-state"],
            &[false, false, false],
            "ask",
            Some("terraform init"),
        ),
        (
            &["terraform", "init", "-backend=false"],
            &["terraform", "init", "-backend=false"],
            &[false, false, false],
            "ask",
            Some("terraform init"),
        ),
        (
            &["terraform", "import", "aws_instance.foo", "i-123"],
            &["terraform", "import", "aws_instance.foo", "i-123"],
            &[false, false, false, false],
            "ask",
            Some("terraform import"),
        ),
        (
            &[
                "terraform",
                "import",
                "-var",
                "name=value",
                "aws_instance.foo",
                "i-123",
            ],
            &[
                "terraform",
                "import",
                "-var",
                "'name=value'",
                "aws_instance.foo",
                "i-123",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("terraform import"),
        ),
        (
            &[
                "terraform",
                "state",
                "mv",
                "aws_instance.foo",
                "aws_instance.bar",
            ],
            &[
                "terraform",
                "state",
                "mv",
                "aws_instance.foo",
                "aws_instance.bar",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("terraform state mv"),
        ),
        (
            &["terraform", "state", "rm", "aws_instance.foo"],
            &["terraform", "state", "rm", "aws_instance.foo"],
            &[false, false, false, false],
            "ask",
            Some("terraform state rm"),
        ),
        (
            &["terraform", "state", "push", "terraform.tfstate"],
            &["terraform", "state", "push", "terraform.tfstate"],
            &[false, false, false, false],
            "ask",
            Some("terraform state push"),
        ),
        (
            &[
                "terraform",
                "state",
                "replace-provider",
                "hashicorp/aws",
                "registry.example.com/aws",
            ],
            &[
                "terraform",
                "state",
                "replace-provider",
                "hashicorp/aws",
                "registry.example.com/aws",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("terraform state replace-provider"),
        ),
        (
            &["terraform", "taint", "aws_instance.foo"],
            &["terraform", "taint", "aws_instance.foo"],
            &[false, false, false],
            "ask",
            Some("terraform taint"),
        ),
        (
            &["terraform", "untaint", "aws_instance.foo"],
            &["terraform", "untaint", "aws_instance.foo"],
            &[false, false, false],
            "ask",
            Some("terraform untaint"),
        ),
        (
            &["terraform", "workspace", "new", "dev"],
            &["terraform", "workspace", "new", "dev"],
            &[false, false, false, false],
            "ask",
            Some("terraform workspace new"),
        ),
        (
            &["terraform", "workspace", "delete", "dev"],
            &["terraform", "workspace", "delete", "dev"],
            &[false, false, false, false],
            "ask",
            Some("terraform workspace delete"),
        ),
        (
            &["terraform", "force-unlock", "1234-5678"],
            &["terraform", "force-unlock", "1234-5678"],
            &[false, false, false],
            "ask",
            Some("terraform force-unlock"),
        ),
        (
            &["terraform", "login"],
            &["terraform", "login"],
            &[false, false],
            "ask",
            Some("terraform login"),
        ),
        (
            &["terraform", "login", "app.terraform.io"],
            &["terraform", "login", "app.terraform.io"],
            &[false, false, false],
            "ask",
            Some("terraform login"),
        ),
        (
            &["terraform", "logout"],
            &["terraform", "logout"],
            &[false, false],
            "ask",
            Some("terraform logout"),
        ),
        (
            &["terraform", "logout", "app.terraform.io"],
            &["terraform", "logout", "app.terraform.io"],
            &[false, false, false],
            "ask",
            Some("terraform logout"),
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
