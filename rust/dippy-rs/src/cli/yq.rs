//! Port of `src/dippy/cli/yq.py`: yq writes to stdout unless `-i/--inplace`.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["yq"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.is_empty() {
        return Classification::ask_desc("yq");
    }
    for token in &tokens[1..] {
        if token == "-i" || token == "--inplace" {
            return Classification::ask_desc("yq -i");
        }
        if token.starts_with("-i=") || token.starts_with("--inplace=") {
            return Classification::ask_desc("yq -i");
        }
    }
    Classification::allow_desc("yq")
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn action(tokens: &[&str]) -> Action {
        classify(&HandlerContext::new(tokens)).action
    }

    #[test]
    fn safe() {
        assert_eq!(action(&["yq"]), Action::Allow);
        assert_eq!(action(&["yq", ".key", "file.yaml"]), Action::Allow);
        assert_eq!(action(&["yq", "-o", "json", "f.yaml"]), Action::Allow);
        assert_eq!(action(&["yq", "eval-all", ".k", "f.yaml"]), Action::Allow);
        assert_eq!(action(&["yq", "-C", ".k"]), Action::Allow);
    }

    #[test]
    fn inplace() {
        assert_eq!(action(&["yq", "-i", ".k=1", "f.yaml"]), Action::Ask);
        assert_eq!(action(&["yq", "--inplace", ".k=1"]), Action::Ask);
        assert_eq!(action(&["yq", "-i=true", ".k=1"]), Action::Ask);
        assert_eq!(action(&["yq", "--inplace=true", ".k=1"]), Action::Ask);
        assert_eq!(action(&["yq", ".k=1", "-i", "f.yaml"]), Action::Ask);
        assert_eq!(action(&["yq", "eval", "-i", ".k=1"]), Action::Ask);
    }
}
