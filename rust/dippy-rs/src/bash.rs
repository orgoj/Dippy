//! Port of `dippy.core.bash`: Bash quoting utilities.

/// Apply Bash quote removal to one word; reject shell expansion.
pub fn decode_literal_word(raw: &str, reject_globs: bool) -> Option<String> {
    let chars: Vec<char> = raw.chars().collect();
    let mut output = String::new();
    let mut quote: Option<char> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            } else {
                output.push(c);
            }
        } else if quote.is_some() && Some(c) == quote {
            quote = None;
        } else if (c == '\'' || c == '"') && quote.is_none() {
            quote = Some(c);
        } else if c == '$' {
            if quote != Some('"')
                || chars
                    .get(i + 1)
                    .is_some_and(|n| n.is_alphanumeric() || "_({[*@#?-$!".contains(*n))
            {
                return None;
            }
            output.push(c);
        } else if c == '`'
            || (reject_globs && quote.is_none() && ("*?[{}".contains(c) || (c == '~' && i == 0)))
        {
            return None;
        } else if c == '\\' {
            let following = *chars.get(i + 1)?;
            if quote == Some('"') && !"\"\\$`\n".contains(following) {
                output.push(c);
            } else {
                if following != '\n' {
                    output.push(following);
                }
                i += 1;
            }
        } else {
            output.push(c);
        }
        i += 1;
    }
    if quote.is_some() { None } else { Some(output) }
}

/// Quote a string for safe use in bash (single quotes when needed).
pub fn bash_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if s.chars()
        .all(|c| c.is_alphanumeric() || "-_./=@:".contains(c))
    {
        return s.into();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// Join tokens into a bash command string with proper quoting.
pub fn bash_join<S: AsRef<str>>(tokens: &[S]) -> String {
    tokens
        .iter()
        .map(|t| bash_quote(t.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode() {
        assert_eq!(decode_literal_word("'a b'", false).as_deref(), Some("a b"));
        assert_eq!(
            decode_literal_word("\"a\\$b\"", false).as_deref(),
            Some("a$b")
        );
        assert_eq!(decode_literal_word("\"$x\"", false), None);
        assert_eq!(decode_literal_word("\"a$\"", false).as_deref(), Some("a$"));
        assert_eq!(decode_literal_word("a\\ b", false).as_deref(), Some("a b"));
        assert_eq!(decode_literal_word("*.txt", true), None);
        assert_eq!(decode_literal_word("'unterminated", false), None);
    }

    #[test]
    fn quote() {
        assert_eq!(bash_quote(""), "''");
        assert_eq!(bash_quote("abc-1/2"), "abc-1/2");
        assert_eq!(bash_quote("it's"), "'it'\"'\"'s'");
        assert_eq!(bash_join(&["echo", "a b"]), "echo 'a b'");
    }
}
