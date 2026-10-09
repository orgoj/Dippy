//! Port of `src/dippy/cli/cdk.py`.
//!
//! CDK commands for infrastructure as code. Most commands modify
//! infrastructure, only a few are safe.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["cdk"];
pub const PORTED: bool = true;
/// The Python module defines no `get_description`.
pub const DESCRIPTION: Option<Describe> = None;

/// Safe CDK commands (read-only).
const SAFE_ACTIONS: &[&str] = &[
    "list",
    "ls",
    "diff",
    "synth",
    "synthesize", // Generates CloudFormation, no deployment
    "metadata",
    "context", // Needs special handling for --reset/--clear
    "docs",
    "doctor",
    "notices",
    "acknowledge",
    "ack",
];

/// Classify CDK command.
pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map_or("cdk", String::as_str);
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    let action = tokens[1].as_str();
    let desc = format!("{base} {action}");

    // Special handling for context command
    if action == "context" {
        if tokens.iter().any(|t| t == "--reset" || t == "--clear") {
            return Classification::ask_desc(desc);
        }
        return Classification::allow_desc(desc);
    }

    if SAFE_ACTIONS.contains(&action) {
        return Classification::allow_desc(desc);
    }
    Classification::ask_desc(desc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated from the Python handler on the commands of
    /// `tests/cli/test_cdk.py` (see `rust/parity/cases/cdk.txt`).
    #[allow(clippy::type_complexity)]
    const PY_CASES: &[(&[&str], &[&str], &[bool], &str, Option<&str>)] = &[
        (
            &["cdk", "list"],
            &["cdk", "list"],
            &[false, false],
            "allow",
            Some("cdk list"),
        ),
        (
            &["cdk", "ls"],
            &["cdk", "ls"],
            &[false, false],
            "allow",
            Some("cdk ls"),
        ),
        (
            &["cdk", "list", "--long"],
            &["cdk", "list", "--long"],
            &[false, false, false],
            "allow",
            Some("cdk list"),
        ),
        (
            &["cdk", "ls", "-l"],
            &["cdk", "ls", "-l"],
            &[false, false, false],
            "allow",
            Some("cdk ls"),
        ),
        (
            &["cdk", "list", "--app", "npx ts-node bin/app.ts"],
            &["cdk", "list", "--app", "'npx ts-node bin/app.ts'"],
            &[false, false, false, false],
            "allow",
            Some("cdk list"),
        ),
        (
            &["cdk", "diff"],
            &["cdk", "diff"],
            &[false, false],
            "allow",
            Some("cdk diff"),
        ),
        (
            &["cdk", "diff", "MyStack"],
            &["cdk", "diff", "MyStack"],
            &[false, false, false],
            "allow",
            Some("cdk diff"),
        ),
        (
            &["cdk", "diff", "--app", "npx ts-node bin/app.ts"],
            &["cdk", "diff", "--app", "'npx ts-node bin/app.ts'"],
            &[false, false, false, false],
            "allow",
            Some("cdk diff"),
        ),
        (
            &["cdk", "diff", "--template", "template.yaml"],
            &["cdk", "diff", "--template", "template.yaml"],
            &[false, false, false, false],
            "allow",
            Some("cdk diff"),
        ),
        (
            &["cdk", "diff", "--security-only"],
            &["cdk", "diff", "--security-only"],
            &[false, false, false],
            "allow",
            Some("cdk diff"),
        ),
        (
            &["cdk", "diff", "--fail"],
            &["cdk", "diff", "--fail"],
            &[false, false, false],
            "allow",
            Some("cdk diff"),
        ),
        (
            &["cdk", "synth"],
            &["cdk", "synth"],
            &[false, false],
            "allow",
            Some("cdk synth"),
        ),
        (
            &["cdk", "synthesize"],
            &["cdk", "synthesize"],
            &[false, false],
            "allow",
            Some("cdk synthesize"),
        ),
        (
            &["cdk", "synth", "MyStack"],
            &["cdk", "synth", "MyStack"],
            &[false, false, false],
            "allow",
            Some("cdk synth"),
        ),
        (
            &["cdk", "synth", "--quiet"],
            &["cdk", "synth", "--quiet"],
            &[false, false, false],
            "allow",
            Some("cdk synth"),
        ),
        (
            &["cdk", "synth", "--json"],
            &["cdk", "synth", "--json"],
            &[false, false, false],
            "allow",
            Some("cdk synth"),
        ),
        (
            &["cdk", "synth", "--app", "npx ts-node bin/app.ts"],
            &["cdk", "synth", "--app", "'npx ts-node bin/app.ts'"],
            &[false, false, false, false],
            "allow",
            Some("cdk synth"),
        ),
        (
            &["cdk", "synth", "--output", "cdk.out"],
            &["cdk", "synth", "--output", "cdk.out"],
            &[false, false, false, false],
            "allow",
            Some("cdk synth"),
        ),
        (
            &["cdk", "synth", "--exclusively"],
            &["cdk", "synth", "--exclusively"],
            &[false, false, false],
            "allow",
            Some("cdk synth"),
        ),
        (
            &["cdk", "docs"],
            &["cdk", "docs"],
            &[false, false],
            "allow",
            Some("cdk docs"),
        ),
        (
            &["cdk", "doctor"],
            &["cdk", "doctor"],
            &[false, false],
            "allow",
            Some("cdk doctor"),
        ),
        (
            &["cdk", "metadata"],
            &["cdk", "metadata"],
            &[false, false],
            "allow",
            Some("cdk metadata"),
        ),
        (
            &["cdk", "metadata", "MyStack"],
            &["cdk", "metadata", "MyStack"],
            &[false, false, false],
            "allow",
            Some("cdk metadata"),
        ),
        (
            &["cdk", "notices"],
            &["cdk", "notices"],
            &[false, false],
            "allow",
            Some("cdk notices"),
        ),
        (
            &["cdk", "notices", "--unacknowledged"],
            &["cdk", "notices", "--unacknowledged"],
            &[false, false, false],
            "allow",
            Some("cdk notices"),
        ),
        (
            &["cdk", "acknowledge", "12345"],
            &["cdk", "acknowledge", "12345"],
            &[false, false, false],
            "allow",
            Some("cdk acknowledge"),
        ),
        (
            &["cdk", "context"],
            &["cdk", "context"],
            &[false, false],
            "allow",
            Some("cdk context"),
        ),
        (
            &["cdk", "context", "--json"],
            &["cdk", "context", "--json"],
            &[false, false, false],
            "allow",
            Some("cdk context"),
        ),
        (
            &["cdk", "version"],
            &["cdk", "version"],
            &[false, false],
            "ask",
            Some("cdk version"),
        ),
        (
            &["cdk", "--version"],
            &["cdk", "--version"],
            &[false, false],
            "ask",
            Some("cdk --version"),
        ),
        (
            &["cdk", "--help"],
            &["cdk", "--help"],
            &[false, false],
            "ask",
            Some("cdk --help"),
        ),
        (
            &["cdk", "-h"],
            &["cdk", "-h"],
            &[false, false],
            "ask",
            Some("cdk -h"),
        ),
        (
            &["cdk", "deploy", "--help"],
            &["cdk", "deploy", "--help"],
            &[false, false, false],
            "ask",
            Some("cdk deploy"),
        ),
        (
            &["cdk", "deploy"],
            &["cdk", "deploy"],
            &[false, false],
            "ask",
            Some("cdk deploy"),
        ),
        (
            &["cdk", "deploy", "MyStack"],
            &["cdk", "deploy", "MyStack"],
            &[false, false, false],
            "ask",
            Some("cdk deploy"),
        ),
        (
            &["cdk", "deploy", "--all"],
            &["cdk", "deploy", "--all"],
            &[false, false, false],
            "ask",
            Some("cdk deploy"),
        ),
        (
            &["cdk", "deploy", "--require-approval", "never"],
            &["cdk", "deploy", "--require-approval", "never"],
            &[false, false, false, false],
            "ask",
            Some("cdk deploy"),
        ),
        (
            &["cdk", "deploy", "--hotswap"],
            &["cdk", "deploy", "--hotswap"],
            &[false, false, false],
            "ask",
            Some("cdk deploy"),
        ),
        (
            &["cdk", "deploy", "--force"],
            &["cdk", "deploy", "--force"],
            &[false, false, false],
            "ask",
            Some("cdk deploy"),
        ),
        (
            &["cdk", "deploy", "--app", "npx ts-node bin/app.ts"],
            &["cdk", "deploy", "--app", "'npx ts-node bin/app.ts'"],
            &[false, false, false, false],
            "ask",
            Some("cdk deploy"),
        ),
        (
            &["cdk", "destroy"],
            &["cdk", "destroy"],
            &[false, false],
            "ask",
            Some("cdk destroy"),
        ),
        (
            &["cdk", "destroy", "MyStack"],
            &["cdk", "destroy", "MyStack"],
            &[false, false, false],
            "ask",
            Some("cdk destroy"),
        ),
        (
            &["cdk", "destroy", "--all"],
            &["cdk", "destroy", "--all"],
            &[false, false, false],
            "ask",
            Some("cdk destroy"),
        ),
        (
            &["cdk", "destroy", "--force"],
            &["cdk", "destroy", "--force"],
            &[false, false, false],
            "ask",
            Some("cdk destroy"),
        ),
        (
            &["cdk", "bootstrap"],
            &["cdk", "bootstrap"],
            &[false, false],
            "ask",
            Some("cdk bootstrap"),
        ),
        (
            &["cdk", "bootstrap", "aws://123456789012/us-east-1"],
            &["cdk", "bootstrap", "aws://123456789012/us-east-1"],
            &[false, false, false],
            "ask",
            Some("cdk bootstrap"),
        ),
        (
            &["cdk", "bootstrap", "--trust", "123456789012"],
            &["cdk", "bootstrap", "--trust", "123456789012"],
            &[false, false, false, false],
            "ask",
            Some("cdk bootstrap"),
        ),
        (
            &["cdk", "init"],
            &["cdk", "init"],
            &[false, false],
            "ask",
            Some("cdk init"),
        ),
        (
            &["cdk", "init", "app"],
            &["cdk", "init", "app"],
            &[false, false, false],
            "ask",
            Some("cdk init"),
        ),
        (
            &["cdk", "init", "app", "--language", "typescript"],
            &["cdk", "init", "app", "--language", "typescript"],
            &[false, false, false, false, false],
            "ask",
            Some("cdk init"),
        ),
        (
            &["cdk", "init", "lib", "--language", "python"],
            &["cdk", "init", "lib", "--language", "python"],
            &[false, false, false, false, false],
            "ask",
            Some("cdk init"),
        ),
        (
            &["cdk", "init", "sample-app", "--language", "java"],
            &["cdk", "init", "sample-app", "--language", "java"],
            &[false, false, false, false, false],
            "ask",
            Some("cdk init"),
        ),
        (
            &["cdk", "import"],
            &["cdk", "import"],
            &[false, false],
            "ask",
            Some("cdk import"),
        ),
        (
            &["cdk", "import", "MyStack"],
            &["cdk", "import", "MyStack"],
            &[false, false, false],
            "ask",
            Some("cdk import"),
        ),
        (
            &["cdk", "migrate"],
            &["cdk", "migrate"],
            &[false, false],
            "ask",
            Some("cdk migrate"),
        ),
        (
            &["cdk", "migrate", "--from-path", "template.yaml"],
            &["cdk", "migrate", "--from-path", "template.yaml"],
            &[false, false, false, false],
            "ask",
            Some("cdk migrate"),
        ),
        (
            &["cdk", "migrate", "--from-stack", "MyCloudFormationStack"],
            &["cdk", "migrate", "--from-stack", "MyCloudFormationStack"],
            &[false, false, false, false],
            "ask",
            Some("cdk migrate"),
        ),
        (
            &["cdk", "watch"],
            &["cdk", "watch"],
            &[false, false],
            "ask",
            Some("cdk watch"),
        ),
        (
            &["cdk", "watch", "MyStack"],
            &["cdk", "watch", "MyStack"],
            &[false, false, false],
            "ask",
            Some("cdk watch"),
        ),
        (
            &["cdk", "watch", "--hotswap"],
            &["cdk", "watch", "--hotswap"],
            &[false, false, false],
            "ask",
            Some("cdk watch"),
        ),
        (
            &["cdk", "gc"],
            &["cdk", "gc"],
            &[false, false],
            "ask",
            Some("cdk gc"),
        ),
        (
            &["cdk", "gc", "--type", "all"],
            &["cdk", "gc", "--type", "all"],
            &[false, false, false, false],
            "ask",
            Some("cdk gc"),
        ),
        (
            &["cdk", "context", "--reset"],
            &["cdk", "context", "--reset"],
            &[false, false, false],
            "ask",
            Some("cdk context"),
        ),
        (
            &["cdk", "context", "--clear"],
            &["cdk", "context", "--clear"],
            &[false, false, false],
            "ask",
            Some("cdk context"),
        ),
        (
            &["cdk", "context", "--reset", "key"],
            &["cdk", "context", "--reset", "key"],
            &[false, false, false, false],
            "ask",
            Some("cdk context"),
        ),
        (
            &["cdk", "refactor"],
            &["cdk", "refactor"],
            &[false, false],
            "ask",
            Some("cdk refactor"),
        ),
        (
            &["cdk", "refactor", "--dry-run"],
            &["cdk", "refactor", "--dry-run"],
            &[false, false, false],
            "ask",
            Some("cdk refactor"),
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
