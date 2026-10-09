//! Port of `src/dippy/cli/tee.py`: tee writes stdin to files.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["tee"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = "tee";
    let mut targets: Vec<String> = Vec::new();
    let mut i = 1;
    while i < tokens.len() {
        let t = &tokens[i];
        if t == "--" {
            targets.extend(tokens[i + 1..].iter().cloned());
            break;
        } else if t.starts_with('-') {
            i += 1;
            continue;
        } else {
            targets.push(t.clone());
        }
        i += 1;
    }
    if targets.is_empty() {
        return Classification::allow_desc(base);
    }
    let desc = if targets.len() == 1 {
        format!("{base} {}", targets[0])
    } else {
        format!("{base} {} files", targets.len())
    };
    Classification::allow_desc(desc).redirects(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn no_files() {
        for cmd in [vec!["tee"], vec!["tee", "-a"], vec!["tee", "--help"]] {
            let c = run(&cmd);
            assert_eq!(c.redirect_targets, None);
            assert_eq!(c.description.as_deref(), Some("tee"));
        }
    }

    #[test]
    fn targets() {
        let c = run(&["tee", "-a", "/tmp/out.txt"]);
        assert_eq!(c.redirect_targets, Some(vec!["/tmp/out.txt".to_string()]));
        assert_eq!(c.description.as_deref(), Some("tee /tmp/out.txt"));
        let c = run(&["tee", "a.txt", "b.txt"]);
        assert_eq!(c.redirect_targets.map(|t| t.len()), Some(2));
        assert_eq!(c.description.as_deref(), Some("tee 2 files"));
        let c = run(&["tee", "--", "-weird", "x"]);
        assert_eq!(
            c.redirect_targets,
            Some(vec!["-weird".to_string(), "x".to_string()])
        );
    }
}
