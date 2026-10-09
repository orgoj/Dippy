//! Locate command, process and arithmetic substitutions in a raw shell word.
//!
//! Only top-level substitutions are returned; their bodies are parsed again,
//! which finds nested ones. Unbalanced text is an error so the caller can fail
//! closed.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubKind {
    Command,
    Process(char),
    Arithmetic,
    /// Deprecated `$[expr]` arithmetic.
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substitution {
    pub kind: SubKind,
    pub inner: String,
}

/// Index of the `)` closing the `(` at `open`, honouring quotes and escapes.
fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '`' => {
                i += 1;
                while i < chars.len() && chars[i] != '`' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Index of the `}` closing a `${` whose `{` is at `open`.
fn matching_brace(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '(' => {
                i = matching_paren(chars, i)?;
            }
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Body of a backtick substitution with bash's backslash unescaping.
fn backtick_body(chars: &[char], open: usize) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut i = open + 1;
    while i < chars.len() {
        match chars[i] {
            '`' => return Some((out, i)),
            '\\' if i + 1 < chars.len() && matches!(chars[i + 1], '$' | '`' | '\\') => {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            c => out.push(c),
        }
        i += 1;
    }
    None
}

pub fn substitutions(value: &str) -> Result<Vec<Substitution>, String> {
    let chars: Vec<char> = value.chars().collect();
    let mut out = Vec::new();
    let mut in_dq = false;
    let mut i = 0;
    let text = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '\\' => i += 1,
            '\'' if !in_dq => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '$' if !in_dq && next == Some('\'') => {
                i += 2;
                while i < chars.len() && chars[i] != '\'' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '"' => in_dq = !in_dq,
            '$' if next == Some('{') => {
                i = matching_brace(&chars, i + 1).ok_or("unbalanced ${")?;
            }
            '$' if next == Some('[') => {
                let mut j = i + 2;
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                out.push(Substitution {
                    kind: SubKind::Deprecated,
                    inner: text((i + 2).min(chars.len()), j.min(chars.len())),
                });
                i = j;
            }
            '$' if next == Some('(') => {
                let close = matching_paren(&chars, i + 1).ok_or("unbalanced $(")?;
                let arithmetic = chars.get(i + 2) == Some(&'(')
                    && close >= 1
                    && chars[close - 1] == ')'
                    && matching_paren(&chars, i + 2) == Some(close - 1);
                if arithmetic {
                    out.push(Substitution {
                        kind: SubKind::Arithmetic,
                        inner: text(i + 3, close - 1),
                    });
                } else {
                    out.push(Substitution {
                        kind: SubKind::Command,
                        inner: text(i + 2, close),
                    });
                }
                i = close;
            }
            '`' => {
                let (body, close) = backtick_body(&chars, i).ok_or("unbalanced backtick")?;
                out.push(Substitution {
                    kind: SubKind::Command,
                    inner: body,
                });
                i = close;
            }
            '<' | '>' if !in_dq && next == Some('(') => {
                let close =
                    matching_paren(&chars, i + 1).ok_or("unbalanced process substitution")?;
                out.push(Substitution {
                    kind: SubKind::Process(c),
                    inner: text(i + 2, close),
                });
                i = close;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(v: &str) -> Vec<(SubKind, String)> {
        substitutions(v)
            .unwrap()
            .into_iter()
            .map(|s| (s.kind, s.inner))
            .collect()
    }

    #[test]
    fn finds_backticks_in_double_quotes() {
        assert_eq!(kinds("\"`rm x`\""), vec![(SubKind::Command, "rm x".into())]);
    }

    #[test]
    fn ignores_single_quotes() {
        assert!(kinds("'$(rm x)'").is_empty());
        assert_eq!(
            kinds("\"'$(rm x)'\""),
            vec![(SubKind::Command, "rm x".into())]
        );
    }

    #[test]
    fn arithmetic_vs_subshell() {
        assert_eq!(kinds("$((1+2))"), vec![(SubKind::Arithmetic, "1+2".into())]);
        assert_eq!(
            kinds("$( (ls) )"),
            vec![(SubKind::Command, " (ls) ".into())]
        );
    }

    #[test]
    fn process_substitution() {
        assert_eq!(kinds("<(ls)"), vec![(SubKind::Process('<'), "ls".into())]);
        assert!(kinds("\"<(ls)\"").is_empty());
    }

    #[test]
    fn param_expansion_is_skipped() {
        assert!(kinds("${x:-$(rm y)}").is_empty());
    }

    #[test]
    fn unbalanced_is_error() {
        assert!(substitutions("$(ls").is_err());
    }
}
