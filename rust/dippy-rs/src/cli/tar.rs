//! Port of `src/dippy/cli/tar.py`.
//!
//! Tar can list, create, or extract archives. Only listing (-t/--list) is
//! safe. --to-command delegates to inner command check.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["tar"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Operation flags to human-readable names (Python dict order).
const OPERATIONS: &[(char, &str)] = &[
    ('c', "create"),
    ('x', "extract"),
    ('r', "append"),
    ('u', "update"),
    ('t', "list"),
];

fn first_operation(t: &str) -> Option<&'static str> {
    OPERATIONS
        .iter()
        .find(|(ch, _)| t.contains(*ch))
        .map(|(_, op)| *op)
}

/// Detect which tar operation is being performed.
fn detect_operation(tokens: &[String]) -> Option<&'static str> {
    for t in tokens.iter().skip(1) {
        match t.as_str() {
            "--create" => return Some("create"),
            "--extract" | "--get" => return Some("extract"),
            "--append" => return Some("append"),
            "--update" => return Some("update"),
            "--list" => return Some("list"),
            "--delete" => return Some("delete"),
            _ => {}
        }
        if t.starts_with('-') && !t.starts_with("--") {
            if let Some(op) = first_operation(t) {
                return Some(op);
            }
        }
    }
    // Old-style (no dash) like "cvf", "xzf"
    if let Some(first_arg) = tokens.get(1) {
        if !first_arg.starts_with('-') {
            if let Some(op) = first_operation(first_arg) {
                return Some(op);
            }
        }
    }
    None
}

/// Extract the command from --to-command flag.
fn extract_to_command(tokens: &[String]) -> Option<String> {
    for (i, t) in tokens.iter().enumerate().skip(1) {
        if let Some(rest) = t.strip_prefix("--to-command=") {
            return Some(rest.to_string());
        }
        if t == "--to-command" && i + 1 < tokens.len() {
            return Some(tokens[i + 1].clone());
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("tar");

    if let Some(to_command) = extract_to_command(tokens) {
        if !to_command.is_empty() {
            return Classification::delegate(to_command).desc(format!("{base} --to-command"));
        }
    }

    match detect_operation(tokens) {
        Some("list") => Classification::allow_desc(format!("{base} list")),
        Some(op) => Classification::ask_desc(format!("{base} {op}")),
        None => Classification::ask_desc(base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn list_allows() {
        for t in [
            vec!["tar", "-tf", "a.tar"],
            vec!["tar", "-tvf", "a.tar"],
            vec!["tar", "tf", "a.tar"],
            vec!["tar", "--list", "-f", "a.tar"],
        ] {
            let r = run(&t);
            assert_eq!(r.action, Action::Allow, "{t:?}");
            assert_eq!(r.description.as_deref(), Some("tar list"));
        }
    }

    #[test]
    fn other_operations_ask() {
        let cases: &[(&[&str], &str)] = &[
            (&["tar", "-czf", "a.tgz", "d"], "tar create"),
            (&["tar", "-xzf", "a.tgz"], "tar extract"),
            (&["tar", "xvf", "a.tar"], "tar extract"),
            (&["tar", "--get", "-f", "a.tar"], "tar extract"),
            (&["tar", "--delete", "-f", "a.tar", "x"], "tar delete"),
            (&["tar", "-rf", "a.tar", "x"], "tar append"),
            (&["tar"], "tar"),
            (&["tar", "-f", "a.tar"], "tar"),
        ];
        for (tokens, desc) in cases {
            let r = run(tokens);
            assert_eq!(r.action, Action::Ask, "{tokens:?}");
            assert_eq!(r.description.as_deref(), Some(*desc));
        }
    }

    #[test]
    fn to_command_delegates() {
        let r = run(&["tar", "-xf", "a.tar", "--to-command=cat"]);
        assert_eq!(r.action, Action::Delegate);
        assert_eq!(r.inner_command.as_deref(), Some("cat"));
        assert_eq!(r.description.as_deref(), Some("tar --to-command"));
        let r = run(&["tar", "-xf", "a.tar", "--to-command", "rm -rf /"]);
        assert_eq!(r.inner_command.as_deref(), Some("rm -rf /"));
        // Empty --to-command falls through to the operation check.
        assert_eq!(
            run(&["tar", "-xf", "a.tar", "--to-command="]).action,
            Action::Ask
        );
    }
}
