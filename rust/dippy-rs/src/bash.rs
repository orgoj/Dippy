//! Port of `dippy.core.bash`: Bash quoting utilities.

use std::collections::BTreeSet;

/// Bash quote removal on one word.
#[derive(Debug, PartialEq, Eq)]
pub enum Decoded {
    /// The value Bash passes.
    Literal(String),
    /// A shell expansion (or a glob when rejected): the value is unknown.
    Expansion,
    /// Valid Bash whose value is not UTF-8 text (an ANSI-C escape).
    Unsupported,
}

/// Apply Bash quote removal to one word; reject shell expansion.
pub fn decode_literal_word(raw: &str, reject_globs: bool) -> Option<String> {
    match decode_word(raw, reject_globs) {
        Decoded::Literal(value) => Some(value),
        _ => None,
    }
}

/// Bash quote removal, including ANSI-C `$'...'` quoting, on one word.
pub fn decode_word(raw: &str, reject_globs: bool) -> Decoded {
    let chars: Vec<char> = raw.chars().collect();
    let mut output = String::new();
    let mut quote: Option<char> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '$' && quote.is_none() && chars.get(i + 1) == Some(&'\'') {
            match ansi_c(&chars, i + 2) {
                Ok((text, end)) => {
                    output.push_str(&text);
                    i = end;
                }
                Err(e) => return e,
            }
        } else if quote == Some('\'') {
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
                return Decoded::Expansion;
            }
            output.push(c);
        } else if c == '`'
            || (reject_globs && quote.is_none() && ("*?[{}".contains(c) || (c == '~' && i == 0)))
        {
            return Decoded::Expansion;
        } else if c == '\\' {
            let Some(&following) = chars.get(i + 1) else {
                return Decoded::Expansion;
            };
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
    if quote.is_some() {
        Decoded::Expansion
    } else {
        Decoded::Literal(output)
    }
}

/// The text of `$'...'` whose body starts at `start`, cut at a NUL as Bash
/// does, and the index of the closing quote.
fn ansi_c(chars: &[char], start: usize) -> Result<(String, usize), Decoded> {
    let mut text = String::new();
    let mut cut = false;
    let mut i = start;
    loop {
        let c = *chars.get(i).ok_or(Decoded::Expansion)?;
        if c == '\'' {
            return Ok((text, i));
        }
        i += 1;
        let decoded = if c != '\\' {
            Some(c)
        } else {
            let e = *chars.get(i).ok_or(Decoded::Expansion)?;
            i += 1;
            let code = match e {
                'a' => Some(0x07),
                'b' => Some(0x08),
                'e' | 'E' => Some(0x1b),
                'f' => Some(0x0c),
                'n' => Some(0x0a),
                'r' => Some(0x0d),
                't' => Some(0x09),
                'v' => Some(0x0b),
                '\\' | '\'' | '"' | '?' => Some(e as u32),
                '0'..='7' => {
                    i -= 1;
                    number(chars, &mut i, 8, 3)
                }
                // A `\xHH` or octal value above 0x7f is a raw byte.
                'x' if chars.get(i) == Some(&'{') => return Err(Decoded::Unsupported),
                'x' => number(chars, &mut i, 16, 2),
                'u' => number(chars, &mut i, 16, 4),
                'U' => number(chars, &mut i, 16, 8),
                'c' => match chars.get(i) {
                    Some('\\') => return Err(Decoded::Unsupported),
                    Some('?') => {
                        i += 1;
                        Some(0x7f)
                    }
                    Some(x) if x.is_ascii() && *x != '\'' => {
                        i += 1;
                        Some(x.to_ascii_uppercase() as u32 & 0x1f)
                    }
                    _ => None,
                },
                _ => None,
            };
            match code {
                None => {
                    if !cut {
                        text.push('\\');
                        text.push(e);
                    }
                    continue;
                }
                Some(0) => None,
                Some(n) if n > 0x7f && matches!(e, 'x' | '0'..='7') => {
                    return Err(Decoded::Unsupported);
                }
                Some(n) => Some(char::from_u32(n).ok_or(Decoded::Unsupported)?),
            }
        };
        match decoded {
            None => cut = true,
            Some(c) if !cut => text.push(c),
            Some(_) => {}
        }
    }
}

/// The value of up to `max` digits at `i`, advancing past them.
fn number(chars: &[char], i: &mut usize, radix: u32, max: usize) -> Option<u32> {
    let mut value = None;
    for _ in 0..max {
        let Some(d) = chars.get(*i).and_then(|c| c.to_digit(radix)) else {
            break;
        };
        value = Some(value.unwrap_or(0) * radix + d);
        *i += 1;
    }
    value
}

/// The argument Bash passes for `raw`, as command rules see it: quote removal
/// with unquoted globs and braces rejected. A leading unquoted `~` or `~/`
/// stays for path normalization; a quoted one becomes `./~`, so it is never
/// expanded.
pub fn shell_word(raw: &str) -> Decoded {
    if raw == "~" || raw.starts_with("~/") {
        return match decode_word(&raw[1..], true) {
            Decoded::Literal(rest) => Decoded::Literal(format!("~{rest}")),
            other => other,
        };
    }
    match decode_word(raw, true) {
        Decoded::Literal(value) if value.starts_with('~') => Decoded::Literal(format!("./{value}")),
        other => other,
    }
}

/// Whether a raw word is a Bash assignment: an unquoted `NAME=` or `NAME+=`.
fn is_assignment(raw: &str) -> bool {
    let end = raw
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(raw.len());
    let rest = &raw[end..];
    end > 0
        && !raw.starts_with(|c: char| c.is_ascii_digit())
        && (rest.starts_with('=') || rest.starts_with("+="))
}

/// The number of leading assignment words, judged on the raw words when they
/// match `words` one to one.
pub fn assignment_count(words: &[String], raw_words: &[String]) -> usize {
    let source = if raw_words.len() == words.len() {
        raw_words
    } else {
        words
    };
    source.iter().take_while(|w| is_assignment(w)).count()
}

/// The most words one brace expansion may produce before Dippy gives up.
const MAX_BRACE_WORDS: usize = 64;

/// The raw words Bash brace expansion makes of `raw` (`[raw]` without an
/// unquoted brace list); `None` for sequences (`{a..b}`) or more than
/// `MAX_BRACE_WORDS` words. `${...}` is a parameter, not a brace list.
pub fn expand_braces(raw: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = raw.chars().collect();
    // Indices of the unquoted characters outside `${...}`.
    let mut open = Vec::new();
    let mut quote: Option<char> = None;
    let mut param_depth = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('$'), '\\') => i += 1,
            (Some('$'), '\'') => quote = None,
            (Some('\''), _) | (Some('$'), _) => {}
            (_, '\\') => i += 1,
            (None, '\'') => quote = Some('\''),
            (None, '"') => quote = Some('"'),
            (None, '$') if chars.get(i + 1) == Some(&'\'') => {
                quote = Some('$');
                i += 1;
            }
            (_, '$') if chars.get(i + 1) == Some(&'{') => {
                param_depth += 1;
                i += 1;
            }
            (_, '{') if param_depth > 0 => param_depth += 1,
            (_, '}') if param_depth > 0 => param_depth -= 1,
            (None, _) if param_depth == 0 => open.push(i),
            _ => {}
        }
        i += 1;
    }
    let structural: BTreeSet<usize> = open.into_iter().collect();
    for &start in structural.iter().filter(|&&k| chars[k] == '{') {
        let mut depth = 0;
        let mut commas = Vec::new();
        let mut close = None;
        for &k in structural.range(start + 1..) {
            match chars[k] {
                '{' => depth += 1,
                '}' if depth == 0 => {
                    close = Some(k);
                    break;
                }
                '}' => depth -= 1,
                ',' if depth == 0 => commas.push(k),
                _ => {}
            }
        }
        let Some(close) = close else { continue };
        if commas.is_empty() {
            let body: String = chars[start + 1..close].iter().collect();
            if body.contains("..") {
                return None;
            }
            continue;
        }
        let text = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
        let (pre, post) = (text(0, start), text(close + 1, chars.len()));
        let mut bounds = vec![start];
        bounds.extend(&commas);
        bounds.push(close);
        let mut words = Vec::new();
        for pair in bounds.windows(2) {
            let piece = format!("{pre}{}{post}", text(pair[0] + 1, pair[1]));
            if piece.is_empty() {
                continue;
            }
            words.extend(expand_braces(&piece)?);
            if words.len() > MAX_BRACE_WORDS {
                return None;
            }
        }
        return Some(words);
    }
    Some(vec![raw.to_string()])
}

/// The values a command receives: each raw word after quote removal, or the
/// token as given when the word has an expansion or no raw word is known.
pub fn shell_values(tokens: &[String], raw_words: &[String]) -> Vec<String> {
    if raw_words.len() != tokens.len() {
        return tokens.to_vec();
    }
    tokens
        .iter()
        .zip(raw_words)
        .map(|(t, raw)| decode_literal_word(raw, false).unwrap_or_else(|| t.clone()))
        .collect()
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
    fn decode_ansi_c_quotes() {
        let lit = |s: &str| Decoded::Literal(s.to_string());
        for (raw, value) in [
            ("$'--dan\\x67er'", "--danger"),
            ("--dan$'\\x67'er", "--danger"),
            ("$'\\U67'", "g"),
            // Bash reads as many hex digits as fit: `\u67e` then `r`.
            ("$'\\u67er'", "\u{67e}r"),
            ("$'\\x7z'", "\x07z"),
            ("$'\\101x'", "Ax"),
            (
                "$'\\a\\b\\e\\E\\f\\n\\r\\t\\v\\\\\\\"\\?'",
                "\x07\x08\x1b\x1b\x0c\n\r\t\x0b\\\"?",
            ),
            ("$'it\\'s'", "it's"),
            ("$'\\cA\\c?'", "\x01\x7f"),
            // Unknown or incomplete escapes keep their backslash.
            ("$'\\q\\x\\8'", "\\q\\x\\8"),
            // A NUL ends the quoted part; the rest of the word follows.
            ("$'a\\0b'c", "ac"),
            ("$'a\\c@b'", "a"),
            ("$'*'", "*"),
            ("\"$'x'\"", "$'x'"),
        ] {
            assert_eq!(decode_word(raw, true), lit(value), "{raw}");
        }
        for raw in [
            "$'\\ud800'",
            "$'\\xff'",
            "$'\\U110000'",
            "$'\\x{41}'",
            "$'\\c\\\\'",
        ] {
            assert_eq!(decode_word(raw, true), Decoded::Unsupported, "{raw}");
            assert_eq!(decode_literal_word(raw, false), None, "{raw}");
        }
        for raw in ["$'open", "$\"x\"", "$x", "a*"] {
            assert_eq!(decode_word(raw, true), Decoded::Expansion, "{raw}");
        }
    }

    #[test]
    fn shell_word_keeps_only_an_unquoted_tilde() {
        let lit = |s: &str| Decoded::Literal(s.to_string());
        assert_eq!(shell_word("~"), lit("~"));
        assert_eq!(shell_word("~/'a b'"), lit("~/a b"));
        assert_eq!(shell_word("'~/x'"), lit("./~/x"));
        assert_eq!(shell_word("\\~"), lit("./~"));
        assert_eq!(shell_word("'*'"), lit("*"));
        assert_eq!(shell_word("~user"), Decoded::Expansion);
        assert_eq!(shell_word("--dan\\ger*"), Decoded::Expansion);
    }

    #[test]
    fn assignments_need_an_unquoted_name() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            assignment_count(&s(&["A=1", "B+=2", "zorp", "C=3"]), &[]),
            2
        );
        for raw in [
            "FOO\\=bar",
            "\"FOO=bar\"",
            "F\\OO=bar",
            "1A=x",
            "-x=1",
            "=x",
        ] {
            let words = s(&[raw, "zorp"]);
            assert_eq!(assignment_count(&words, &words), 0, "{raw}");
        }
        let words = s(&["FOO=$(x)", "B_1='a b'", "zorp"]);
        assert_eq!(assignment_count(&words, &words), 2);
    }

    #[test]
    fn brace_lists_expand_like_bash() {
        let s = |v: &[&str]| Some(v.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        for (raw, words) in [
            ("x", s(&["x"])),
            ("{a,b}", s(&["a", "b"])),
            ("x{a,b}y", s(&["xay", "xby"])),
            ("{a,{b,c}}", s(&["a", "b", "c"])),
            ("{a,b}{c,d}", s(&["ac", "ad", "bc", "bd"])),
            ("{a}{b,c}", s(&["{a}b", "{a}c"])),
            ("{a,\"b,c\"}", s(&["a", "\"b,c\""])),
            ("{--danger,}", s(&["--danger"])),
            ("a{,}b", s(&["ab", "ab"])),
            ("{a,''}", s(&["a", "''"])),
            ("{a,$x}", s(&["a", "$x"])),
            ("'{a,b}'", s(&["'{a,b}'"])),
            ("\\{a,b}", s(&["\\{a,b}"])),
            ("$'{a,b}'", s(&["$'{a,b}'"])),
            ("${x,y}", s(&["${x,y}"])),
            ("{a,b", s(&["{a,b"])),
            ("{1..3}", None),
            ("{a,b}{1..2}", None),
            ("{a,b}{c,d}{e,f}{g,h}{i,j}{k,l}{m,n}", None),
        ] {
            assert_eq!(expand_braces(raw), words, "{raw}");
        }
    }

    #[test]
    fn shell_values_decode_literal_words_only() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            shell_values(
                &s(&["awk '\\\"x\\\"'", "$HOME"]),
                &s(&["\"awk '\\\"x\\\"'\"", "$HOME"])
            ),
            s(&["awk '\"x\"'", "$HOME"])
        );
        // Without raw words (handler tests) the tokens stay as they are.
        assert_eq!(shell_values(&s(&["a\\b"]), &[]), s(&["a\\b"]));
    }

    #[test]
    fn quote() {
        assert_eq!(bash_quote(""), "''");
        assert_eq!(bash_quote("abc-1/2"), "abc-1/2");
        assert_eq!(bash_quote("it's"), "'it'\"'\"'s'");
        assert_eq!(bash_join(&["echo", "a b"]), "echo 'a b'");
    }
}
