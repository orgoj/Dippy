//! Port of `src/dippy/cli/packer.py`.
//!
//! Packer is a HashiCorp tool for building automated machine images. Safe
//! operations are read-only inspections and validations. Unsafe operations
//! include building images (external effects), installing plugins, and
//! modifying template files.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["packer"];
pub const PORTED: bool = true;
/// The Python module defines no `get_description`.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe read-only actions.
const SAFE_ACTIONS: &[&str] = &[
    "version", "validate", // Check template validity
    "inspect",  // Show template components
    "console",  // Interactive testing (read-only)
];

/// Safe subcommands for plugins.
const SAFE_PLUGINS_SUBCOMMANDS: &[&str] = &[
    "installed", // List installed plugins
    "required",  // List required plugins
];

/// Unsafe subcommands for plugins.
const UNSAFE_PLUGINS_SUBCOMMANDS: &[&str] = &[
    "install", // Installs plugins
    "remove",  // Removes plugins
];

/// Classify packer command.
pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map_or("packer", String::as_str);
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    // Check for help flags anywhere
    if tokens
        .iter()
        .any(|t| t == "--help" || t == "-help" || t == "-h")
    {
        return Classification::allow_desc(format!("{base} --help"));
    }

    // Check for version flag
    if tokens.iter().any(|t| t == "--version" || t == "-version") {
        return Classification::allow_desc(format!("{base} --version"));
    }

    // Find action (skip global flags)
    let mut action: Option<&str> = None;
    let mut action_idx = 1;

    while action_idx < tokens.len() {
        let token = tokens[action_idx].as_str();

        if token.starts_with('-') {
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
    let mut desc = format!("{base} {action}");

    // Handle plugins subcommand
    if action == "plugins" {
        let subcommand = find_subcommand(rest);
        if let Some(sub) = subcommand
            && !sub.is_empty()
        {
            desc = format!("{desc} {sub}");
        }
        if subcommand.is_some_and(|s| SAFE_PLUGINS_SUBCOMMANDS.contains(&s)) {
            return Classification::allow_desc(desc);
        }
        if subcommand.is_some_and(|s| UNSAFE_PLUGINS_SUBCOMMANDS.contains(&s)) {
            return Classification::ask_desc(desc);
        }
        return Classification::ask_desc(desc);
    }

    // Handle fmt - safe only with -check, -diff, or -write=false
    if action == "fmt" {
        if is_fmt_safe(rest) {
            return Classification::allow_desc(desc);
        }
        return Classification::ask_desc(desc);
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

/// Check if fmt command is safe (read-only mode).
fn is_fmt_safe(rest: &[String]) -> bool {
    rest.iter()
        .any(|t| t == "-check" || t == "-diff" || t.starts_with("-write=false"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_write_false_prefix() {
        let r = classify(&HandlerContext::new(&[
            "packer",
            "fmt",
            "-write=falsey",
            ".",
        ]));
        assert_eq!(r.action.as_str(), "allow");
        let r = classify(&HandlerContext::new(&["packer", "fmt", "."]));
        assert_eq!(r.action.as_str(), "ask");
    }

    /// Generated from the Python handler on the commands of
    /// `tests/cli/test_packer.py` (see `rust/parity/cases/packer.txt`).
    #[allow(clippy::type_complexity)]
    const PY_CASES: &[(&[&str], &[&str], &[bool], &str, Option<&str>)] = &[
        (
            &["packer", "--help"],
            &["packer", "--help"],
            &[false, false],
            "allow",
            Some("packer --help"),
        ),
        (
            &["packer", "-help"],
            &["packer", "-help"],
            &[false, false],
            "allow",
            Some("packer --help"),
        ),
        (
            &["packer", "--version"],
            &["packer", "--version"],
            &[false, false],
            "allow",
            Some("packer --version"),
        ),
        (
            &["packer", "version"],
            &["packer", "version"],
            &[false, false],
            "allow",
            Some("packer version"),
        ),
        (
            &["packer", "validate", "template.json"],
            &["packer", "validate", "template.json"],
            &[false, false, false],
            "allow",
            Some("packer validate"),
        ),
        (
            &["packer", "validate", "-syntax-only", "template.json"],
            &["packer", "validate", "-syntax-only", "template.json"],
            &[false, false, false, false],
            "allow",
            Some("packer validate"),
        ),
        (
            &["packer", "validate", "-var", "key=value", "template.json"],
            &["packer", "validate", "-var", "'key=value'", "template.json"],
            &[false, false, false, false, false],
            "allow",
            Some("packer validate"),
        ),
        (
            &["packer", "validate", "-var-file=vars.json", "template.json"],
            &["packer", "validate", "-var-file=vars.json", "template.json"],
            &[false, false, false, false],
            "allow",
            Some("packer validate"),
        ),
        (
            &["packer", "validate", "-except=amazon-ebs", "template.json"],
            &["packer", "validate", "-except=amazon-ebs", "template.json"],
            &[false, false, false, false],
            "allow",
            Some("packer validate"),
        ),
        (
            &["packer", "validate", "-only=docker", "template.json"],
            &["packer", "validate", "-only=docker", "template.json"],
            &[false, false, false, false],
            "allow",
            Some("packer validate"),
        ),
        (
            &["packer", "validate", "-machine-readable", "template.json"],
            &["packer", "validate", "-machine-readable", "template.json"],
            &[false, false, false, false],
            "allow",
            Some("packer validate"),
        ),
        (
            &["packer", "validate", "."],
            &["packer", "validate", "."],
            &[false, false, false],
            "allow",
            Some("packer validate"),
        ),
        (
            &["packer", "inspect", "template.json"],
            &["packer", "inspect", "template.json"],
            &[false, false, false],
            "allow",
            Some("packer inspect"),
        ),
        (
            &["packer", "inspect", "-machine-readable", "template.json"],
            &["packer", "inspect", "-machine-readable", "template.json"],
            &[false, false, false, false],
            "allow",
            Some("packer inspect"),
        ),
        (
            &["packer", "inspect", "."],
            &["packer", "inspect", "."],
            &[false, false, false],
            "allow",
            Some("packer inspect"),
        ),
        (
            &["packer", "fmt", "-check", "template.pkr.hcl"],
            &["packer", "fmt", "-check", "template.pkr.hcl"],
            &[false, false, false, false],
            "allow",
            Some("packer fmt"),
        ),
        (
            &["packer", "fmt", "-diff", "template.pkr.hcl"],
            &["packer", "fmt", "-diff", "template.pkr.hcl"],
            &[false, false, false, false],
            "allow",
            Some("packer fmt"),
        ),
        (
            &["packer", "fmt", "-write=false", "template.pkr.hcl"],
            &["packer", "fmt", "-write=false", "template.pkr.hcl"],
            &[false, false, false, false],
            "allow",
            Some("packer fmt"),
        ),
        (
            &["packer", "fmt", "-check", "-diff", "template.pkr.hcl"],
            &["packer", "fmt", "-check", "-diff", "template.pkr.hcl"],
            &[false, false, false, false, false],
            "allow",
            Some("packer fmt"),
        ),
        (
            &["packer", "fmt", "-check", "-recursive", "."],
            &["packer", "fmt", "-check", "-recursive", "."],
            &[false, false, false, false, false],
            "allow",
            Some("packer fmt"),
        ),
        (
            &["packer", "console"],
            &["packer", "console"],
            &[false, false],
            "allow",
            Some("packer console"),
        ),
        (
            &["packer", "console", "template.json"],
            &["packer", "console", "template.json"],
            &[false, false, false],
            "allow",
            Some("packer console"),
        ),
        (
            &["packer", "console", "-var", "key=value"],
            &["packer", "console", "-var", "'key=value'"],
            &[false, false, false, false],
            "allow",
            Some("packer console"),
        ),
        (
            &["packer", "console", "-var-file=vars.json"],
            &["packer", "console", "-var-file=vars.json"],
            &[false, false, false],
            "allow",
            Some("packer console"),
        ),
        (
            &["packer", "plugins", "installed"],
            &["packer", "plugins", "installed"],
            &[false, false, false],
            "allow",
            Some("packer plugins installed"),
        ),
        (
            &["packer", "plugins", "required", "template.json"],
            &["packer", "plugins", "required", "template.json"],
            &[false, false, false, false],
            "allow",
            Some("packer plugins required"),
        ),
        (
            &["packer", "build", "template.json"],
            &["packer", "build", "template.json"],
            &[false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "-var", "key=value", "template.json"],
            &["packer", "build", "-var", "'key=value'", "template.json"],
            &[false, false, false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "-var-file=vars.json", "template.json"],
            &["packer", "build", "-var-file=vars.json", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "-only=docker", "template.json"],
            &["packer", "build", "-only=docker", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "-except=amazon-ebs", "template.json"],
            &["packer", "build", "-except=amazon-ebs", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "-force", "template.json"],
            &["packer", "build", "-force", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "-on-error=abort", "template.json"],
            &["packer", "build", "-on-error=abort", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "-parallel-builds=1", "template.json"],
            &["packer", "build", "-parallel-builds=1", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "-debug", "template.json"],
            &["packer", "build", "-debug", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "build", "."],
            &["packer", "build", "."],
            &[false, false, false],
            "ask",
            Some("packer build"),
        ),
        (
            &["packer", "init", "template.json"],
            &["packer", "init", "template.json"],
            &[false, false, false],
            "ask",
            Some("packer init"),
        ),
        (
            &["packer", "init", "-upgrade", "template.json"],
            &["packer", "init", "-upgrade", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer init"),
        ),
        (
            &["packer", "init", "-force", "template.json"],
            &["packer", "init", "-force", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer init"),
        ),
        (
            &["packer", "init", "."],
            &["packer", "init", "."],
            &[false, false, false],
            "ask",
            Some("packer init"),
        ),
        (
            &["packer", "fix", "template.json"],
            &["packer", "fix", "template.json"],
            &[false, false, false],
            "ask",
            Some("packer fix"),
        ),
        (
            &["packer", "fix", "-validate=true", "template.json"],
            &["packer", "fix", "-validate=true", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer fix"),
        ),
        (
            &["packer", "fix", "-validate=false", "template.json"],
            &["packer", "fix", "-validate=false", "template.json"],
            &[false, false, false, false],
            "ask",
            Some("packer fix"),
        ),
        (
            &["packer", "fmt", "template.pkr.hcl"],
            &["packer", "fmt", "template.pkr.hcl"],
            &[false, false, false],
            "ask",
            Some("packer fmt"),
        ),
        (
            &["packer", "fmt", "-recursive", "."],
            &["packer", "fmt", "-recursive", "."],
            &[false, false, false, false],
            "ask",
            Some("packer fmt"),
        ),
        (
            &["packer", "fmt", "."],
            &["packer", "fmt", "."],
            &[false, false, false],
            "ask",
            Some("packer fmt"),
        ),
        (
            &["packer", "hcl2_upgrade", "template.json"],
            &["packer", "hcl2_upgrade", "template.json"],
            &[false, false, false],
            "ask",
            Some("packer hcl2_upgrade"),
        ),
        (
            &[
                "packer",
                "hcl2_upgrade",
                "-output-file=out.pkr.hcl",
                "template.json",
            ],
            &[
                "packer",
                "hcl2_upgrade",
                "-output-file=out.pkr.hcl",
                "template.json",
            ],
            &[false, false, false, false],
            "ask",
            Some("packer hcl2_upgrade"),
        ),
        (
            &[
                "packer",
                "hcl2_upgrade",
                "-with-annotations",
                "template.json",
            ],
            &[
                "packer",
                "hcl2_upgrade",
                "-with-annotations",
                "template.json",
            ],
            &[false, false, false, false],
            "ask",
            Some("packer hcl2_upgrade"),
        ),
        (
            &[
                "packer",
                "plugins",
                "install",
                "github.com/hashicorp/amazon",
            ],
            &[
                "packer",
                "plugins",
                "install",
                "github.com/hashicorp/amazon",
            ],
            &[false, false, false, false],
            "ask",
            Some("packer plugins install"),
        ),
        (
            &["packer", "plugins", "remove", "github.com/hashicorp/amazon"],
            &["packer", "plugins", "remove", "github.com/hashicorp/amazon"],
            &[false, false, false, false],
            "ask",
            Some("packer plugins remove"),
        ),
        (&["packer"], &["packer"], &[false], "ask", Some("packer")),
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
