//! Python `fnmatch.fnmatchcase` semantics (POSIX `fnmatch.fnmatch` is the same).
//!
//! `*` matches any run of characters (including `/` and newlines), `?` one
//! character, `[...]` a class (`!` negates; `^` is literal; `]` first is
//! literal; an unclosed `[` is a literal). Backslash is not special.
//!
//! `wordmatch` matches one command argument: only a run of two or more stars
//! crosses `/`.

#[derive(Debug, Clone)]
enum Tok {
    Star,
    /// A run of two or more stars (`fnmatchcase` treats it as `Star`).
    DoubleStar,
    Any,
    Lit(char),
    Class {
        negate: bool,
        items: Vec<(char, char)>,
    },
}

fn compile(pattern: &str) -> Vec<Tok> {
    let p: Vec<char> = pattern.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < p.len() {
        let c = p[i];
        i += 1;
        match c {
            '*' => match out.last() {
                Some(Tok::Star) => {
                    out.pop();
                    out.push(Tok::DoubleStar);
                }
                Some(Tok::DoubleStar) => {}
                _ => out.push(Tok::Star),
            },
            '?' => out.push(Tok::Any),
            '[' => {
                let mut j = i;
                if j < p.len() && p[j] == '!' {
                    j += 1;
                }
                if j < p.len() && p[j] == ']' {
                    j += 1;
                }
                while j < p.len() && p[j] != ']' {
                    j += 1;
                }
                if j >= p.len() {
                    out.push(Tok::Lit('['));
                    continue;
                }
                let mut body: &[char] = &p[i..j];
                i = j + 1;
                let negate = body.first() == Some(&'!');
                if negate {
                    body = &body[1..];
                }
                let mut items = Vec::new();
                let mut k = 0;
                while k < body.len() {
                    if k + 2 < body.len() && body[k + 1] == '-' {
                        // Python drops reversed ranges (they match nothing).
                        if body[k] <= body[k + 2] {
                            items.push((body[k], body[k + 2]));
                        }
                        k += 3;
                    } else {
                        items.push((body[k], body[k]));
                        k += 1;
                    }
                }
                if items.is_empty() && !negate {
                    // Empty class after dropping ranges matches nothing.
                    out.push(Tok::Class {
                        negate: false,
                        items: Vec::new(),
                    });
                } else {
                    out.push(Tok::Class { negate, items });
                }
            }
            other => out.push(Tok::Lit(other)),
        }
    }
    out
}

fn tok_matches(t: &Tok, c: char) -> bool {
    match t {
        Tok::Any => true,
        Tok::Lit(l) => *l == c,
        Tok::Class { negate, items } => {
            let hit = items.iter().any(|(a, b)| *a <= c && c <= *b);
            hit != *negate
        }
        Tok::Star | Tok::DoubleStar => true,
    }
}

/// Case-sensitive shell-style match of the whole string.
pub fn fnmatchcase(name: &str, pattern: &str) -> bool {
    let toks = compile(pattern);
    let s: Vec<char> = name.chars().collect();
    // Iterative matcher with single-star backtracking.
    let (mut si, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if ti < toks.len() {
            match &toks[ti] {
                Tok::Star | Tok::DoubleStar => {
                    star = Some((ti, si));
                    ti += 1;
                    continue;
                }
                t if tok_matches(t, s[si]) => {
                    si += 1;
                    ti += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((st, ss)) => {
                ti = st + 1;
                si = ss + 1;
                star = Some((st, ss + 1));
            }
            None => return false,
        }
    }
    while ti < toks.len() && matches!(toks[ti], Tok::Star | Tok::DoubleStar) {
        ti += 1;
    }
    ti == toks.len()
}

/// Match one command argument: `*`, `?` and a negated class never match `/`;
/// a run of two or more stars matches anything.
pub fn wordmatch(name: &str, pattern: &str) -> bool {
    let s: Vec<char> = name.chars().collect();
    // reach[i]: the tokens so far match s[..i].
    let mut reach = vec![false; s.len() + 1];
    reach[0] = true;
    for t in compile(pattern) {
        let mut next = vec![false; s.len() + 1];
        for i in 0..=s.len() {
            next[i] = match t {
                Tok::Star => reach[i] || (i > 0 && next[i - 1] && s[i - 1] != '/'),
                Tok::DoubleStar => reach[i] || (i > 0 && next[i - 1]),
                Tok::Any | Tok::Class { negate: true, .. } if i > 0 && s[i - 1] == '/' => false,
                _ => i > 0 && reach[i - 1] && tok_matches(&t, s[i - 1]),
            };
        }
        reach = next;
    }
    reach[s.len()]
}

#[cfg(test)]
mod tests {
    use super::fnmatchcase as m;

    #[test]
    fn basics() {
        assert!(m("git status", "git *"));
        assert!(m("a/b", "*"));
        assert!(m("abc", "a?c"));
        assert!(!m("abc", "a?"));
        assert!(m("a", "[abc]"));
        assert!(!m("d", "[abc]"));
        assert!(m("d", "[!abc]"));
        assert!(m("^", "[^x]"));
        assert!(!m("y", "[^x]"));
        assert!(m("]", "[]]"));
        assert!(m("[", "["));
        assert!(m("[ab", "[ab"));
        assert!(m("b", "[a-c]"));
        assert!(!m("b", "[c-a]"));
        assert!(m("-", "[a-]"));
        assert!(m("a\\b", "a\\b"));
        assert!(m("", "*"));
        assert!(!m("", "?"));
        assert!(m("x\ny", "x*y"));
        assert!(m("aXbXc", "a*b*c"));
    }

    #[test]
    fn word_globs_cross_slash_only_with_doublestar() {
        use super::wordmatch as w;
        assert!(w("tmp/a", "tmp/*"));
        assert!(!w("tmp/a/b", "tmp/*"));
        assert!(w("tmp/a/b", "tmp/**"));
        assert!(w("tmp/", "tmp/*"));
        assert!(w("--x=a b", "--x=*"));
        assert!(!w("--x=a/b", "--x=*"));
        assert!(w("--x=a/b", "--x=**"));
        assert!(w("/usr/bin/hcom", "/**/hcom"));
        assert!(!w("/hcom", "/**/hcom"));
        assert!(w("/usr/bin/hcom", "/**hcom"));
        assert!(!w("a/b", "a?b"));
        assert!(!w("a/b", "a[!x]b"));
        assert!(w("axb", "a?b"));
        assert!(w("a/b", "a[/]b"));
        assert!(w("abc", "a*b*c"));
        assert!(w("", "*"));
        assert!(!w("", "?"));
    }
}
