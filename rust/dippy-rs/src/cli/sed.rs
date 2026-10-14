//! Port of `src/dippy/cli/sed.py`.
//!
//! Sed is safe for text processing, but `-i` edits files in place, the `w`
//! command writes files and GNU `e` executes the pattern space.
//!
//! Scripts are scanned command by command (GNU syntax), so a `w` or `e`
//! inside an address, regex or replacement is never taken for a command.
//! A script the scanner does not understand asks.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["sed"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that take a separate argument (`FLAGS_WITH_ARG`).
const FLAGS_WITH_ARG: &[&str] = &["-e", "--expression", "-f", "--file"];

/// Commands that take no argument.
const PLAIN_COMMANDS: &str = "{}=dDgGhHnNpPxzF";

/// What a sed script does beyond reading its input.
#[derive(Debug, Default, PartialEq)]
struct Effects {
    writes: Vec<String>,
    executes: bool,
}

/// Cursor over one sed script.
struct Scanner {
    chars: Vec<char>,
    pos: usize,
}

impl Scanner {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    /// Skip spaces and tabs (GNU `ISBLANK`).
    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    fn skip_digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    /// Consume up to the next newline; return the text before it.
    fn rest_of_line(&mut self) -> String {
        let mut text = String::new();
        while let Some(c) = self.peek().filter(|&c| c != '\n') {
            text.push(c);
            self.pos += 1;
        }
        text
    }

    /// Consume up to the next `;` or newline (labels).
    fn skip_label(&mut self) {
        while !matches!(self.peek(), None | Some(';' | '\n')) {
            self.pos += 1;
        }
    }

    /// Consume `a`/`i`/`c` text: up to a newline not escaped by `\`.
    fn skip_text(&mut self) {
        while let Some(c) = self.peek().filter(|&c| c != '\n') {
            self.pos += if c == '\\' { 2 } else { 1 };
        }
    }

    /// Consume a regex or replacement up to and including the closing
    /// `delim`; `None` when it is unterminated.
    fn delimited(&mut self, delim: char) -> Option<()> {
        loop {
            match self.bump()? {
                c if c == delim => return Some(()),
                '\\' => {
                    self.bump()?;
                }
                '\n' => return None,
                _ => {}
            }
        }
    }

    /// Consume an opening delimiter (`s`, `y`, `\c` address).
    fn delimiter(&mut self) -> Option<char> {
        self.bump().filter(|&c| c != '\n' && c != '\\')
    }

    /// Consume one address; `Some(false)` when none starts here.
    fn address(&mut self) -> Option<bool> {
        match self.peek() {
            Some(c) if c.is_ascii_digit() => {
                self.skip_digits();
                if self.peek() == Some('~') {
                    self.pos += 1;
                    self.skip_digits();
                }
                return Some(true);
            }
            Some('$') => {
                self.pos += 1;
                return Some(true);
            }
            Some('/') => {
                self.pos += 1;
                self.delimited('/')?;
            }
            Some('\\') => {
                self.pos += 1;
                let delim = self.delimiter()?;
                self.delimited(delim)?;
            }
            _ => return Some(false),
        }
        while matches!(self.peek(), Some('I' | 'M')) {
            self.pos += 1;
        }
        Some(true)
    }

    /// Consume an optional `addr1[,addr2]` and any `!`.
    fn addresses(&mut self) -> Option<()> {
        if self.address()? {
            self.skip_blanks();
            if self.peek() == Some(',') {
                self.pos += 1;
                self.skip_blanks();
                if matches!(self.peek(), Some('+' | '~')) {
                    self.pos += 1;
                    self.skip_digits();
                } else if !self.address()? {
                    return None;
                }
            }
        }
        self.skip_blanks();
        while self.peek() == Some('!') {
            self.pos += 1;
            self.skip_blanks();
        }
        Some(())
    }

    /// Consume a `w`/`W` file name (the rest of the line).
    fn write_target(&mut self, effects: &mut Effects) -> Option<()> {
        self.skip_blanks();
        let path = self.rest_of_line();
        if path.is_empty() {
            return None;
        }
        effects.writes.push(path);
        Some(())
    }

    /// Consume the `s` flags after the replacement.
    fn substitute_flags(&mut self, effects: &mut Effects) -> Option<()> {
        while let Some(c) = self.peek() {
            match c {
                'g' | 'p' | 'i' | 'I' | 'm' | 'M' | '0'..='9' => self.pos += 1,
                'e' => {
                    self.pos += 1;
                    effects.executes = true;
                }
                'w' => {
                    self.pos += 1;
                    return self.write_target(effects);
                }
                _ => break,
            }
        }
        Some(())
    }

    /// Consume one command after its addresses.
    fn command(&mut self, effects: &mut Effects) -> Option<()> {
        let cmd = self.bump()?;
        match cmd {
            _ if PLAIN_COMMANDS.contains(cmd) => {}
            'q' | 'Q' | 'l' | 'L' => {
                self.skip_blanks();
                self.skip_digits();
            }
            ':' | 'b' | 't' | 'T' | 'v' => self.skip_label(),
            'a' | 'i' | 'c' => self.skip_text(),
            '#' | 'r' | 'R' => {
                self.rest_of_line();
            }
            'w' | 'W' => return self.write_target(effects),
            'e' => {
                effects.executes = true;
                self.rest_of_line();
            }
            's' => {
                let delim = self.delimiter()?;
                self.delimited(delim)?;
                self.delimited(delim)?;
                return self.substitute_flags(effects);
            }
            'y' => {
                let delim = self.delimiter()?;
                self.delimited(delim)?;
                self.delimited(delim)?;
            }
            _ => return None,
        }
        Some(())
    }
}

/// Scan a sed script; `None` when its syntax is not understood.
fn scan_script(script: &str, effects: &mut Effects) -> Option<()> {
    let mut sc = Scanner {
        chars: script.chars().collect(),
        pos: 0,
    };
    loop {
        while sc.peek().is_some_and(|c| c == ';' || c.is_whitespace()) {
            sc.pos += 1;
        }
        if sc.peek().is_none() {
            return Some(());
        }
        sc.addresses()?;
        sc.command(effects)?;
        sc.skip_blanks();
        if !matches!(sc.peek(), None | Some(';' | '\n' | '}' | '#')) {
            return None;
        }
    }
}

/// Scan all scripts; `None` when any is not understood.
fn scan_scripts(scripts: &[String]) -> Option<Effects> {
    let mut effects = Effects::default();
    for script in scripts {
        scan_script(script, &mut effects)?;
    }
    Some(effects)
}

/// Extract sed script strings from command tokens.
fn extract_scripts(tokens: &[String]) -> Vec<String> {
    let mut scripts = Vec::new();
    let mut i = 1;
    let mut found_script_arg = false;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if t == "-e" || t == "--expression" {
            if let Some(next) = tokens.get(i + 1) {
                scripts.push(next.clone());
                found_script_arg = true;
            }
            i += 2;
            continue;
        }
        if let Some(rest) = t.strip_prefix("--expression=") {
            scripts.push(rest.to_string());
            found_script_arg = true;
            i += 1;
            continue;
        }
        if t == "-f" || t == "--file" {
            found_script_arg = true;
            i += 2;
            continue;
        }
        if t.starts_with("--file=") {
            found_script_arg = true;
            i += 1;
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        if !found_script_arg && scripts.is_empty() {
            scripts.push(t.to_string());
            found_script_arg = true;
        }
        i += 1;
    }
    scripts
}

/// Extract input files that will be modified by `-i`.
fn extract_inplace_files(tokens: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    let mut found_script = false;
    let has_e_flag = tokens[1..]
        .iter()
        .any(|t| t == "-e" || t == "--expression" || t.starts_with("--expression="));
    let mut i = 1;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if FLAGS_WITH_ARG.contains(&t) {
            i += 2;
            continue;
        }
        if t.starts_with("--expression=") || t.starts_with("--file=") {
            i += 1;
            continue;
        }
        if t.starts_with("-i") || t.starts_with("--in-place") {
            i += 1;
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        if !has_e_flag && !found_script {
            found_script = true;
            i += 1;
            continue;
        }
        files.push(t.to_string());
        i += 1;
    }
    files
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("sed");

    let Some(effects) = scan_scripts(&extract_scripts(tokens)) else {
        return Classification::ask_desc(format!("{base} (unparsed script)"));
    };
    if effects.executes {
        return Classification::ask_desc(format!("{base} e (execute)"));
    }
    let write_targets = effects.writes;

    let has_inplace = tokens
        .iter()
        .skip(1)
        .any(|t| t.starts_with("-i") || t.starts_with("--in-place"));

    let mut redirect_targets: Vec<String> = Vec::new();
    if has_inplace {
        redirect_targets.extend(extract_inplace_files(tokens));
    }
    redirect_targets.extend(write_targets);

    if !redirect_targets.is_empty() {
        let desc = if has_inplace {
            format!("{base} -i")
        } else {
            format!("{base} w")
        };
        return Classification::allow_desc(desc).redirects(redirect_targets);
    }
    if has_inplace {
        return Classification::ask_desc(format!("{base} -i"));
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

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn safe_scripts_allow() {
        for script in [
            "s/foo/bar/",
            "s/foo/bar/g",
            "/pattern/d",
            "1,10p",
            "$d",
            "y/abc/xyz/",
            "1!G;h;$!d",
            ":a;N;$!ba;s/\\n/ /g",
            "s/new/old/",
        ] {
            let c = run(&["sed", script, "file.txt"]);
            assert_eq!(c.action, Action::Allow, "{script}");
            assert_eq!(c.redirect_targets, None, "{script}");
        }
        assert_eq!(run(&["sed", "-f", "x.sed", "f"]).action, Action::Allow);
    }

    #[test]
    fn inplace_reports_files() {
        let c = run(&["sed", "-i", "s/a/b/", "f1", "f2"]);
        assert_eq!(c.action, Action::Allow);
        assert_eq!(c.redirect_targets, Some(s(&["f1", "f2"])));
        assert_eq!(c.description.as_deref(), Some("sed -i"));
        let c = run(&["sed", "-e", "s/a/b/", "-i", "f"]);
        assert_eq!(c.redirect_targets, Some(s(&["f"])));
        let c = run(&["sed", "--in-place=.bak", "s/a/b/", "f"]);
        assert_eq!(c.redirect_targets, Some(s(&["f"])));
        // -i'' with the script glued: no files found -> ask.
        let c = run(&["sed", "-is/foo/bar/"]);
        assert_eq!(c.action, Action::Ask);
        // macOS `-i ''`: '' is taken as the script, then the real script is a file.
        let c = run(&["sed", "-i", "", "s/a/b/", "f"]);
        assert_eq!(c.redirect_targets, Some(s(&["s/a/b/", "f"])));
    }

    fn scan(script: &str) -> Effects {
        scan_scripts(&s(&[script])).unwrap_or_else(|| panic!("unparsed: {script:?}"))
    }

    #[test]
    fn write_pattern() {
        assert_eq!(scan("s/foo/bar/w output.txt").writes, s(&["output.txt"]));
        assert_eq!(scan("/pattern/w matches.txt").writes, s(&["matches.txt"]));
        assert_eq!(scan("w out").writes, s(&["out"]));
        // GNU: the file name is the rest of the line.
        assert_eq!(scan("s/a/b/w x y").writes, s(&["x y"]));
        assert_eq!(scan("w a;p\np").writes, s(&["a;p"]));
        assert!(scan("s/new x/y/").writes.is_empty());
        assert!(scan("s/w/x/").writes.is_empty());
        let c = run(&["sed", "s/foo/bar/w out.txt", "in.txt"]);
        assert_eq!(c.description.as_deref(), Some("sed w"));
        assert_eq!(c.redirect_targets, Some(s(&["out.txt"])));
    }

    #[test]
    fn execute_pattern() {
        for script in [
            "s/foo/ls/e",
            "e",
            "e date",
            "1d;e",
            "s/a/b/e;p",
            "/x/e  ",
            "s/a/b/e\n",
            "1d; e\tls",
            "e\x1c",
            "s/a/b/eg",
            "1e",
            // GNU runs `cho`.
            "echo",
        ] {
            assert!(scan(script).executes, "{script:?}");
        }
        for script in ["s/e/f/", "/e/d", "y/e/f/", "/a; e x/p"] {
            assert!(!scan(script).executes, "{script:?}");
        }
        assert_eq!(run(&["sed", "-e", "p", "-e", "e"]).action, Action::Ask);
    }

    #[test]
    fn w_inside_address_is_not_a_command() {
        for script in [
            "/w x/p",
            "/A w/,/C/p",
            "\\,w x,p",
            "\\|w x|,$p",
            "/w x/I,+2p",
            "s|new x|y|",
            "s/a/w x/",
            "y/w x/abc/",
            "1a w x",
            "$a\\\nw x\\\nw y\np",
            "b w;p # w x",
        ] {
            let c = run(&["sed", "-n", script, "f.md"]);
            assert_eq!(c.action, Action::Allow, "{script}");
            assert_eq!(c.redirect_targets, None, "{script}");
        }
    }

    #[test]
    fn real_writes_after_addresses() {
        for (script, target) in [
            ("/w x/w out", "out"),
            ("\\,w x,w out", "out"),
            ("1,/w x/W out", "out"),
            ("s,w x,y,w out", "out"),
            ("s|a|b|gw out", "out"),
            ("p;w out", "out"),
        ] {
            let c = run(&["sed", "-n", script, "f.md"]);
            assert_eq!(c.redirect_targets, Some(s(&[target])), "{script}");
            assert_eq!(c.description.as_deref(), Some("sed w"), "{script}");
        }
    }

    #[test]
    fn unparsed_script_asks() {
        for script in ["/w x", "s/a/b", "s/a/b/w", "1,", "Kx"] {
            assert_eq!(run(&["sed", script, "f"]).action, Action::Ask, "{script}");
        }
    }

    #[test]
    fn extract_scripts_order() {
        assert_eq!(extract_scripts(&s(&["sed", "-n", "p", "e"])), s(&["p"]));
        assert_eq!(
            extract_scripts(&s(&["sed", "--expression=e", "x"])),
            s(&["e"])
        );
        assert_eq!(extract_scripts(&s(&["sed", "-e"])), Vec::<String>::new());
    }
}
