//! Port of `dippy.core.parser`: tokenisation through the Parable-shaped AST.

use crate::ast::{self, Node};

/// Strip one pair of matching outer quotes (`_strip_quotes`).
pub fn strip_quotes(value: &str) -> &str {
    let b = value.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Tokenize bash; raw mode requires one simple command without redirects.
pub fn tokenize(command: &str, raw: bool) -> Vec<String> {
    if command.trim().is_empty() {
        return Vec::new();
    }
    let Ok(nodes) = ast::parse(command) else {
        return Vec::new();
    };
    if raw {
        return match nodes.as_slice() {
            [Node::Command { words, redirects }] if redirects.is_empty() => {
                words.iter().map(|w| w.value.clone()).collect()
            }
            _ => Vec::new(),
        };
    }
    extract_tokens(&nodes)
}

fn extract_tokens(nodes: &[Node]) -> Vec<String> {
    let mut tokens = Vec::new();
    for node in nodes {
        match node {
            Node::Word(w) => tokens.push(strip_quotes(&w.value).to_string()),
            Node::Command { words, .. } => {
                tokens.extend(words.iter().map(|w| strip_quotes(&w.value).to_string()))
            }
            Node::Pipeline { commands } => {
                if let Some(first) = commands.first() {
                    tokens.extend(extract_tokens(std::slice::from_ref(first)));
                }
            }
            Node::List { parts } => {
                if let Some(part) = parts.iter().find(|p| !matches!(p, Node::Operator { .. })) {
                    tokens.extend(extract_tokens(std::slice::from_ref(part)));
                }
            }
            _ => {}
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_basic() {
        assert_eq!(
            tokenize("git log --format='%H'", false),
            vec!["git", "log", "--format='%H'"]
        );
        assert_eq!(tokenize("ls 'a b' | wc", false), vec!["ls", "a b"]);
        assert_eq!(tokenize("echo 'x'", true), vec!["echo", "'x'"]);
        assert!(tokenize("echo x > f", true).is_empty());
        assert!(tokenize("", false).is_empty());
    }
}
