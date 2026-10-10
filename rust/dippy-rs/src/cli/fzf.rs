//! Port of `src/dippy/cli/fzf.py`.
//!
//! Fzf is a fuzzy finder; most operations are read-only. `--listen-unsafe`
//! asks, and `--bind` with execute/execute-silent/become delegates to the
//! inner command.

use std::sync::LazyLock;

use regex::Regex;

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["fzf"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Bind actions that execute external commands (`EXEC_BIND_ACTIONS`).
const EXEC_BIND_ACTIONS: &[&str] = &["execute", "execute-silent", "become"];

/// `PAREN_PATTERN`: `action(cmd)` syntax (greedy, `.` excludes newline).
static PAREN_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(execute|execute-silent|become)\((.+)\)").unwrap());

/// `COLON_PATTERN`: `action:cmd` syntax.
static COLON_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(execute|execute-silent|become):(\S+)").unwrap());

/// Extract the command from execute/execute-silent/become actions.
fn extract_exec_command(bind_value: &str) -> Option<String> {
    if let Some(m) = PAREN_PATTERN.captures(bind_value) {
        return Some(m[2].to_string());
    }
    if let Some(m) = COLON_PATTERN.captures(bind_value) {
        return Some(m[2].to_string());
    }
    None
}

/// Check if a bind value contains execute/execute-silent/become actions.
fn has_exec_bind_action(bind_value: &str) -> bool {
    for action in EXEC_BIND_ACTIONS {
        if bind_value.contains(&format!("{action}(")) {
            return true;
        }
        if bind_value.contains(&format!("{action}:")) {
            return true;
        }
        let replaced = bind_value.replace([',', '+'], ":");
        if replaced.split(':').any(|part| part == *action) {
            return true;
        }
    }
    false
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("fzf");
    for (i, token) in tokens.iter().enumerate() {
        if token == "--listen-unsafe" || token.starts_with("--listen-unsafe=") {
            return Classification::ask_desc(format!("{base} --listen-unsafe"));
        }
        if token == "--bind" || token.starts_with("--bind=") {
            let bind_value = if token == "--bind" {
                match tokens.get(i + 1) {
                    Some(v) => v.as_str(),
                    None => continue,
                }
            } else {
                &token["--bind=".len()..]
            };
            if has_exec_bind_action(bind_value) {
                // Python: `if inner_cmd:` - an empty match is falsy.
                if let Some(inner) = extract_exec_command(bind_value).filter(|c| !c.is_empty()) {
                    return Classification::delegate(inner).desc(format!("{base} --bind"));
                }
                return Classification::ask_desc(format!("{base} --bind"));
            }
        }
    }
    Classification::allow_desc(base)
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn safe_binds_allow() {
        for value in [
            "--bind=ctrl-x:abort",
            "--bind=ctrl-r:reload(find .)",
            "--bind=change:reload:rg --files",
            "--bind=focus:transform-header:echo\\ {}",
            "--bind=enter:print-query",
            "--bind=ctrl-p:put(text)",
            "--query=execute",
            "--preview=execute cat {}",
        ] {
            assert_eq!(run(&["fzf", value]).action, Action::Allow, "{value}");
        }
    }

    #[test]
    fn listen_unsafe_asks() {
        assert_eq!(run(&["fzf", "--listen-unsafe"]).action, Action::Ask);
        assert_eq!(run(&["fzf", "--listen-unsafe=6266"]).action, Action::Ask);
        assert_eq!(
            run(&["fzf", "--listen=6266", "--listen-unsafe"]).action,
            Action::Ask
        );
    }

    #[test]
    fn exec_binds_delegate() {
        let c = run(&["fzf", "--bind=enter:execute(vim {})"]);
        assert_eq!(c.action, Action::Delegate);
        assert_eq!(c.inner_command.as_deref(), Some("vim {}"));
        assert_eq!(c.description.as_deref(), Some("fzf --bind"));
        let c = run(&["fzf", "--bind", "enter:execute-silent(rm {})"]);
        assert_eq!(c.inner_command.as_deref(), Some("rm {}"));
        let c = run(&["fzf", "--bind=enter:execute:vim\\ {}"]);
        assert_eq!(c.inner_command.as_deref(), Some("vim\\"));
        let c = run(&["fzf", "--bind=enter:become:cat"]);
        assert_eq!(c.inner_command.as_deref(), Some("cat"));
        // Greedy paren match spans to the last ')'.
        let c = run(&["fzf", "--bind=a:execute(x),b:execute(y)"]);
        assert_eq!(c.inner_command.as_deref(), Some("x),b:execute(y"));
    }

    #[test]
    fn exec_action_without_command_asks() {
        // Bare action name in a chain: detected but not extractable.
        let c = run(&["fzf", "--bind=enter:accept+execute"]);
        assert_eq!(c.action, Action::Ask);
        assert_eq!(run(&["fzf", "--bind=enter:execute()"]).action, Action::Ask);
    }
}
