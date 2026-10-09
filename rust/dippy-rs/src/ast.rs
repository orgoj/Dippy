//! Parable-shaped AST built from Rable.
//!
//! Python Dippy walks Parable's Python node objects. Rable mirrors Parable's
//! S-expressions but its Rust tree differs in shape: redirects are normalised
//! (`2>&1` becomes op `>&`, fd 2, target `1`), lists nest by precedence, word
//! parts include literal segments and `[[ ]]` operands carry no parts. This
//! module rebuilds the subset of Parable's attribute tree that the analyzer
//! reads, so the analyzer port can follow the Python code line by line.
//!
//! Anything the adapter cannot rebuild faithfully becomes [`Node::Unsupported`],
//! which the analyzer treats as `ask` (fail closed).

use rable::ast::{ListOperator, Node as RNode, NodeKind as K, PipeSep};

use crate::scan::{self, SubKind};

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    /// Raw word text, including shell quoting (Parable `Word.value`).
    pub value: String,
    /// Expansion parts only (Parable never stores literal segments).
    pub parts: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Word(Word),
    Command {
        words: Vec<Word>,
        redirects: Vec<Node>,
    },
    Pipeline {
        commands: Vec<Node>,
    },
    /// Flat list: commands interleaved with `Operator` nodes.
    List {
        parts: Vec<Node>,
    },
    Operator {
        op: String,
    },
    PipeBoth,
    Redirect {
        op: String,
        target: Word,
    },
    HereDoc {
        content: String,
        quoted: bool,
        fd: Option<i32>,
    },
    Subshell {
        body: Box<Node>,
        redirects: Vec<Node>,
    },
    BraceGroup {
        body: Box<Node>,
        redirects: Vec<Node>,
    },
    If {
        condition: Box<Node>,
        then_body: Box<Node>,
        else_body: Option<Box<Node>>,
        redirects: Vec<Node>,
    },
    While {
        condition: Box<Node>,
        body: Box<Node>,
        redirects: Vec<Node>,
    },
    Until {
        condition: Box<Node>,
        body: Box<Node>,
        redirects: Vec<Node>,
    },
    For {
        words: Option<Vec<Word>>,
        body: Box<Node>,
        redirects: Vec<Node>,
    },
    ForArith {
        init: String,
        cond: String,
        incr: String,
        body: Box<Node>,
        redirects: Vec<Node>,
    },
    Select {
        words: Option<Vec<Word>>,
        body: Box<Node>,
        redirects: Vec<Node>,
    },
    Case {
        word: Word,
        bodies: Vec<Option<Node>>,
        redirects: Vec<Node>,
    },
    Function {
        body: Box<Node>,
    },
    Param {
        arg: Option<String>,
    },
    ParamLen,
    ParamIndirect {
        arg: Option<String>,
    },
    CmdSub {
        command: Box<Node>,
    },
    ProcSub {
        direction: String,
        command: Box<Node>,
    },
    /// `$(( ))` in a word; only the command substitutions inside matter.
    Arith {
        cmdsubs: Vec<Node>,
    },
    ArithCmd {
        cmdsubs: Vec<Node>,
        redirects: Vec<Node>,
    },
    AnsiC,
    Locale,
    ArithDeprecated,
    Negation {
        pipeline: Box<Node>,
    },
    Time {
        pipeline: Box<Node>,
    },
    CondExpr {
        body: Box<Node>,
        redirects: Vec<Node>,
    },
    UnaryTest {
        operand: Word,
    },
    BinaryTest {
        left: Word,
        right: Word,
    },
    CondAnd {
        left: Box<Node>,
        right: Box<Node>,
    },
    CondOr {
        left: Box<Node>,
        right: Box<Node>,
    },
    CondNot {
        operand: Box<Node>,
    },
    CondParen {
        inner: Box<Node>,
    },
    Coproc {
        command: Box<Node>,
    },
    Comment,
    Empty,
    Array,
    /// A construct the adapter cannot map; analyzed as `ask`.
    Unsupported(String),
}

impl Node {
    /// Parable `kind` string.
    pub fn kind(&self) -> &str {
        match self {
            Node::Word(_) => "word",
            Node::Command { .. } => "command",
            Node::Pipeline { .. } => "pipeline",
            Node::List { .. } => "list",
            Node::Operator { .. } => "operator",
            Node::PipeBoth => "pipe-both",
            Node::Redirect { .. } => "redirect",
            Node::HereDoc { .. } => "heredoc",
            Node::Subshell { .. } => "subshell",
            Node::BraceGroup { .. } => "brace-group",
            Node::If { .. } => "if",
            Node::While { .. } => "while",
            Node::Until { .. } => "until",
            Node::For { .. } => "for",
            Node::ForArith { .. } => "for-arith",
            Node::Select { .. } => "select",
            Node::Case { .. } => "case",
            Node::Function { .. } => "function",
            Node::Param { .. } => "param",
            Node::ParamLen => "param-len",
            Node::ParamIndirect { .. } => "param-indirect",
            Node::CmdSub { .. } => "cmdsub",
            Node::ProcSub { .. } => "procsub",
            Node::Arith { .. } => "arith",
            Node::ArithCmd { .. } => "arith-cmd",
            Node::AnsiC => "ansi-c",
            Node::ArithDeprecated => "arith-deprecated",
            Node::Locale => "locale",
            Node::Negation { .. } => "negation",
            Node::Time { .. } => "time",
            Node::CondExpr { .. } => "cond-expr",
            Node::UnaryTest { .. } => "unary-test",
            Node::BinaryTest { .. } => "binary-test",
            Node::CondAnd { .. } => "cond-and",
            Node::CondOr { .. } => "cond-or",
            Node::CondNot { .. } => "cond-not",
            Node::CondParen { .. } => "cond-paren",
            Node::Coproc { .. } => "coproc",
            Node::Comment => "comment",
            Node::Empty => "empty",
            Node::Array => "array",
            Node::Unsupported(_) => "unsupported",
        }
    }

    /// Redirects attached to a node (Parable `node.redirects`).
    pub fn redirects(&self) -> &[Node] {
        match self {
            Node::Command { redirects, .. }
            | Node::Subshell { redirects, .. }
            | Node::BraceGroup { redirects, .. }
            | Node::If { redirects, .. }
            | Node::While { redirects, .. }
            | Node::Until { redirects, .. }
            | Node::For { redirects, .. }
            | Node::ForArith { redirects, .. }
            | Node::Select { redirects, .. }
            | Node::Case { redirects, .. }
            | Node::ArithCmd { redirects, .. }
            | Node::CondExpr { redirects, .. } => redirects,
            _ => &[],
        }
    }
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
}

/// Parse a command string into Parable-shaped nodes.
pub fn parse(source: &str) -> Result<Vec<Node>, ParseError> {
    if source.contains(MASK) {
        return Ok(vec![Node::Unsupported(
            "reserved character in input".into(),
        )]);
    }
    // Rable treats `$'` inside double quotes as ANSI-C quoting (and can drop
    // the rest of the command); in Bash and Parable it is literal. Hide the
    // quote behind a same-width placeholder and restore it in every string.
    let masked = mask_dquoted_ansi_c(source);
    let source = masked.as_str();
    let nodes = rable::parse(source, false).map_err(|e| ParseError {
        message: rable_error_message(&e),
    })?;
    // Rable recovers from some syntax errors by dropping text (`ls ;; rm`
    // parses as `ls`). Parable rejects them; so do we.
    check_coverage(source, &nodes)?;
    let conv = Converter { src: source };
    let mut out = Vec::new();
    for n in &nodes {
        conv.top_level(n, &mut out);
    }
    if out.is_empty() && source.trim_start().starts_with('#') {
        // Parable returns an empty node for a comment-only input.
        out.push(Node::Empty);
    }
    Ok(out)
}

/// Parse the body of a command or process substitution.
fn parse_inner(text: &str) -> Node {
    match parse(text) {
        Ok(mut nodes) => match nodes.len() {
            0 => Node::Empty,
            1 => nodes.remove(0),
            _ => {
                // Parable keeps a multi-line substitution body as one list.
                let mut parts = Vec::new();
                let count = nodes.len();
                for (i, n) in nodes.into_iter().enumerate() {
                    match n {
                        Node::List { parts: inner } => parts.extend(inner),
                        other => parts.push(other),
                    }
                    if i + 1 < count && !matches!(parts.last(), Some(Node::Operator { .. })) {
                        parts.push(Node::Operator { op: "\n".into() });
                    }
                }
                Node::List { parts }
            }
        },
        Err(e) => Node::Unsupported(format!("substitution parse error: {}", e.message)),
    }
}

fn rable_error_message(e: &rable::error::RableError) -> String {
    match e {
        rable::error::RableError::Parse { message, .. } => message.clone(),
        rable::error::RableError::MatchedPair { message, .. } => message.clone(),
    }
}

struct Converter<'a> {
    src: &'a str,
}

fn boxed(n: Node) -> Box<Node> {
    Box::new(n)
}

impl Converter<'_> {
    /// Operator written right after a node (`;` or `&`), if any.
    fn trailing_op(&self, end: usize) -> Option<&'static str> {
        let mut it = self
            .src
            .chars()
            .skip(end)
            .skip_while(|c| *c == ' ' || *c == '\t');
        match (it.next(), it.next()) {
            (Some(';'), Some(';' | '&')) => None,
            (Some(';'), _) => Some(";"),
            (Some('&'), Some('&' | '>')) => None,
            (Some('&'), _) => Some("&"),
            _ => None,
        }
    }

    /// Parable keeps a trailing `;`/`&` as a list in top-level, brace-group
    /// and subshell bodies; Rable drops it.
    fn with_trailing(&self, n: &RNode) -> Node {
        let node = self.node(n);
        let Some(op) = self.trailing_op(n.span.end) else {
            return node;
        };
        match node {
            Node::List { mut parts } => {
                if !matches!(parts.last(), Some(Node::Operator { .. })) {
                    parts.push(Node::Operator { op: op.into() });
                }
                Node::List { parts }
            }
            other => Node::List {
                parts: vec![other, Node::Operator { op: op.into() }],
            },
        }
    }

    /// Parable starts a new top-level node at each newline; Rable joins
    /// newline-separated commands into one list.
    fn top_level(&self, n: &RNode, out: &mut Vec<Node>) {
        let K::List { items } = &n.kind else {
            out.push(self.with_trailing(n));
            return;
        };
        let mut group: Vec<Node> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if matches!(item.command.kind, K::List { .. }) {
                self.flatten_list(&item.command, &mut group);
            } else {
                group.push(self.list_item(&item.command));
            }
            let end = item.command.span.end;
            let next_start = items.get(i + 1).map(|x| x.command.span.start);
            let between: String = match next_start {
                Some(start) => self
                    .src
                    .chars()
                    .skip(end)
                    .take(start.saturating_sub(end))
                    .collect(),
                None => self.src.chars().skip(end).collect(),
            };
            let op_text = between.trim_start_matches([' ', '\t']);
            match item.operator {
                Some(op) => {
                    let op = list_op(op);
                    if op_text.starts_with(op) {
                        group.push(Node::Operator { op: op.into() });
                    }
                    if next_start.is_some() && is_separator(op) && between.contains('\n') {
                        out.push(finish_group(std::mem::take(&mut group)));
                    } else if !op_text.starts_with(op) {
                        group.push(Node::Operator { op: op.into() });
                    }
                }
                None => {
                    if let Some(op) = self.trailing_op(end) {
                        group.push(Node::Operator { op: op.into() });
                    }
                }
            }
        }
        if !group.is_empty() {
            out.push(finish_group(group));
        }
    }

    /// A list member; Rable fills a dangling `&&`/`||` with an empty node
    /// where Bash and Parable report a syntax error.
    fn list_item(&self, n: &RNode) -> Node {
        match n.kind {
            K::Empty => Node::Unsupported("empty command in list".into()),
            _ => self.node(n),
        }
    }

    fn node(&self, n: &RNode) -> Node {
        match &n.kind {
            K::Word { .. } => Node::Word(self.word(n)),
            K::Command {
                assignments,
                words,
                redirects,
            } if assignments.is_empty() && words.is_empty() && redirects.is_empty() => {
                Node::Unsupported("empty command".into())
            }
            K::Command {
                assignments,
                words,
                redirects,
            } => Node::Command {
                words: assignments
                    .iter()
                    .chain(words.iter())
                    .map(|w| self.word(w))
                    .collect(),
                // `a |& b` gets a synthetic, span-less `2>&1` in Rable only.
                redirects: redirects
                    .iter()
                    .filter(|r| !r.span.is_empty() || matches!(r.kind, K::HereDoc { .. }))
                    .map(|r| self.redirect(r))
                    .collect(),
            },
            K::Pipeline {
                commands,
                separators,
            } => {
                let mut out = Vec::new();
                for (i, c) in commands.iter().enumerate() {
                    if i > 0 && separators.get(i - 1) == Some(&PipeSep::PipeBoth) {
                        out.push(Node::PipeBoth);
                    }
                    out.push(self.node(c));
                }
                Node::Pipeline { commands: out }
            }
            K::List { .. } => {
                let mut parts = Vec::new();
                self.flatten_list(n, &mut parts);
                Node::List { parts }
            }
            K::If {
                condition,
                then_body,
                else_body,
                redirects,
            } => Node::If {
                condition: boxed(self.node(condition)),
                then_body: boxed(self.node(then_body)),
                else_body: else_body.as_ref().map(|e| boxed(self.node(e))),
                redirects: self.redirects(redirects),
            },
            K::While {
                condition,
                body,
                redirects,
            } => Node::While {
                condition: boxed(self.node(condition)),
                body: boxed(self.node(body)),
                redirects: self.redirects(redirects),
            },
            K::Until {
                condition,
                body,
                redirects,
            } => Node::Until {
                condition: boxed(self.node(condition)),
                body: boxed(self.node(body)),
                redirects: self.redirects(redirects),
            },
            K::For {
                words,
                body,
                redirects,
                ..
            } => Node::For {
                words: words
                    .as_ref()
                    .map(|ws| ws.iter().map(|w| self.word(w)).collect()),
                body: boxed(self.node(body)),
                redirects: self.redirects(redirects),
            },
            K::ForArith {
                init,
                cond,
                incr,
                body,
                redirects,
            } => Node::ForArith {
                init: fix(init),
                cond: fix(cond),
                incr: fix(incr),
                body: boxed(self.node(body)),
                redirects: self.redirects(redirects),
            },
            K::Select {
                words,
                body,
                redirects,
                ..
            } => Node::Select {
                words: words
                    .as_ref()
                    .map(|ws| ws.iter().map(|w| self.word(w)).collect()),
                body: boxed(self.node(body)),
                redirects: self.redirects(redirects),
            },
            K::Case {
                word,
                patterns,
                redirects,
            } => Node::Case {
                word: self.word(word),
                bodies: patterns
                    .iter()
                    .map(|p| p.body.as_ref().map(|b| self.node(b)))
                    .collect(),
                redirects: self.redirects(redirects),
            },
            K::Function { body, .. } => Node::Function {
                body: boxed(self.node(body)),
            },
            K::Subshell { body, redirects } => Node::Subshell {
                body: boxed(self.with_trailing(body)),
                redirects: self.redirects(redirects),
            },
            K::BraceGroup { body, redirects } => Node::BraceGroup {
                body: boxed(self.with_trailing(body)),
                redirects: self.redirects(redirects),
            },
            K::Coproc { command, .. } => Node::Coproc {
                command: boxed(self.node(command)),
            },
            K::Redirect { .. } | K::HereDoc { .. } => self.redirect(n),
            K::ParamExpansion { arg, .. } => Node::Param {
                arg: arg.as_deref().map(fix),
            },
            K::ParamLength { .. } => Node::ParamLen,
            K::ParamIndirect { arg, .. } => Node::ParamIndirect {
                arg: arg.as_deref().map(fix),
            },
            K::CommandSubstitution { command, .. } => Node::CmdSub {
                command: boxed(self.node(command)),
            },
            K::ProcessSubstitution { direction, command } => Node::ProcSub {
                direction: direction.clone(),
                command: boxed(self.node(command)),
            },
            K::AnsiCQuote { .. } => Node::AnsiC,
            K::LocaleString { .. } => Node::Locale,
            K::ArithmeticExpansion { .. } => Node::Arith {
                cmdsubs: Vec::new(),
            },
            K::ArithmeticCommand {
                redirects,
                raw_content,
                ..
            } => Node::ArithCmd {
                cmdsubs: arith_cmdsubs(raw_content),
                redirects: self.redirects(redirects),
            },
            K::ConditionalExpr { body, redirects } => Node::CondExpr {
                body: boxed(self.cond(body)),
                redirects: self.redirects(redirects),
            },
            K::Negation { pipeline } => match pipeline.kind {
                // Parable stores a bare `!` with no pipeline (None).
                K::Empty => Node::Unsupported("negation without command".into()),
                _ => Node::Negation {
                    pipeline: boxed(self.node(pipeline)),
                },
            },
            K::Time { pipeline, .. } => Node::Time {
                pipeline: boxed(self.node(pipeline)),
            },
            K::Array { .. } => Node::Array,
            K::Empty => Node::Empty,
            K::Comment { .. } => Node::Comment,
            other => Node::Unsupported(format!("{other:?}").chars().take(40).collect()),
        }
    }

    fn redirects(&self, rs: &[RNode]) -> Vec<Node> {
        rs.iter().map(|r| self.redirect(r)).collect()
    }

    fn flatten_list(&self, n: &RNode, out: &mut Vec<Node>) {
        if let K::List { items } = &n.kind {
            for item in items {
                if matches!(item.command.kind, K::List { .. }) {
                    self.flatten_list(&item.command, out);
                } else {
                    out.push(self.list_item(&item.command));
                }
                if let Some(op) = item.operator {
                    out.push(Node::Operator {
                        op: list_op(op).into(),
                    });
                }
            }
        }
    }

    fn word(&self, n: &RNode) -> Word {
        match &n.kind {
            K::Word { value, parts, .. } => {
                let value = fix(value);
                Word {
                    parts: self.word_parts(&value, parts),
                    value,
                }
            }
            K::CondTerm { value, .. } => self.cond_term(&fix(value)),
            _ => Word {
                value: String::new(),
                parts: vec![Node::Unsupported("non-word".into())],
            },
        }
    }

    /// Word expansion parts in Parable form. Command, process and arithmetic
    /// substitutions come from scanning the raw word: Rable misses backticks
    /// inside double quotes and gives substitution bodies relative spans.
    fn word_parts(&self, value: &str, parts: &[RNode]) -> Vec<Node> {
        let subs = substitution_nodes(value);
        let mut out = Vec::new();
        let mut inserted = false;
        for p in parts {
            match &p.kind {
                K::WordLiteral { .. } | K::BraceExpansion { .. } => {}
                K::CommandSubstitution { .. }
                | K::ProcessSubstitution { .. }
                | K::ArithmeticExpansion { .. } => {
                    if !inserted {
                        out.extend(subs.iter().cloned());
                        inserted = true;
                    }
                }
                _ => out.push(self.node(p)),
            }
        }
        if !inserted {
            out.extend(subs);
        }
        if is_array_assignment(value) && !out.iter().any(|n| matches!(n, Node::Array)) {
            out.push(Node::Array);
        }
        out
    }

    /// Rable keeps `[[ ]]` operands as raw text; reparse them as a word.
    fn cond_term(&self, value: &str) -> Word {
        let probe = mask_dquoted_ansi_c(&format!(": {value}"));
        let parts = match rable::parse(&probe, false) {
            Ok(nodes) if nodes.len() == 1 => match &nodes[0].kind {
                K::Command { words, .. } if words.len() == 2 => match &words[1].kind {
                    K::Word { value, parts, .. } => self.word_parts(&fix(value), parts),
                    _ => vec![Node::Unsupported("cond-term".into())],
                },
                _ => vec![Node::Unsupported("cond-term".into())],
            },
            _ => vec![Node::Unsupported("cond-term".into())],
        };
        Word {
            value: value.to_string(),
            parts,
        }
    }

    fn cond(&self, n: &RNode) -> Node {
        match &n.kind {
            K::UnaryTest { operand, .. } => Node::UnaryTest {
                operand: self.word(operand),
            },
            K::BinaryTest { left, right, .. } => Node::BinaryTest {
                left: self.word(left),
                right: self.word(right),
            },
            K::CondAnd { left, right } => Node::CondAnd {
                left: boxed(self.cond(left)),
                right: boxed(self.cond(right)),
            },
            K::CondOr { left, right } => Node::CondOr {
                left: boxed(self.cond(left)),
                right: boxed(self.cond(right)),
            },
            K::CondNot { operand } => Node::CondNot {
                operand: boxed(self.cond(operand)),
            },
            K::CondParen { inner } => Node::CondParen {
                inner: boxed(self.cond(inner)),
            },
            K::CondTerm { value, .. } => Node::Word(self.cond_term(&fix(value))),
            _ => self.node(n),
        }
    }

    /// Rebuild Parable's textual redirect (`op` keeps the fd prefix and drops
    /// `&`; the target keeps `&`), e.g. `2>&1` -> op `2>`, target `&1`.
    fn redirect(&self, n: &RNode) -> Node {
        match &n.kind {
            K::HereDoc {
                content,
                quoted,
                fd,
                ..
            } => Node::HereDoc {
                content: fix(content),
                quoted: *quoted,
                fd: if *fd >= 0 { Some(*fd) } else { None },
            },
            K::Redirect { target, varfd, .. } => {
                let text = n.source_text(self.src);
                let prefix = match varfd {
                    Some(name) => format!("{{{name}}}"),
                    None => self.fd_prefix(n.span.start),
                };
                match split_redirect(text) {
                    Some((op, rest)) => {
                        // `>& file` (space after `&`) keeps `&` in Parable's op.
                        let (op, rest) = match rest.strip_prefix('&') {
                            Some(after) if after.starts_with([' ', '\t']) => {
                                (format!("{op}&"), after)
                            }
                            _ => (op.to_string(), rest),
                        };
                        let target_word = self.word(target);
                        let value = fix(rest.trim_start());
                        let parts = if value == target_word.value
                            || value.strip_prefix('&') == Some(target_word.value.as_str())
                        {
                            target_word.parts
                        } else {
                            reparse_word_parts(self, value.trim_start_matches('&'))
                        };
                        Node::Redirect {
                            op: format!("{prefix}{op}"),
                            target: Word { value, parts },
                        }
                    }
                    None => Node::Unsupported(format!("redirect {text}")),
                }
            }
            _ => self.node(n),
        }
    }

    /// Digits immediately before a redirect span (the explicit fd).
    fn fd_prefix(&self, start: usize) -> String {
        let chars: Vec<char> = self.src.chars().take(start).collect();
        let mut i = chars.len();
        while i > 0 && chars[i - 1].is_ascii_digit() {
            i -= 1;
        }
        if i < chars.len() && (i == 0 || chars[i - 1].is_whitespace() || is_meta(chars[i - 1])) {
            chars[i..].iter().collect()
        } else {
            String::new()
        }
    }
}

fn is_meta(c: char) -> bool {
    matches!(c, ';' | '&' | '|' | '(' | ')' | '<' | '>')
}

fn reparse_word_parts(conv: &Converter, value: &str) -> Vec<Node> {
    if value.is_empty() {
        return Vec::new();
    }
    conv.cond_term(value).parts
}

/// Split `>&1`, `>> f`, `<<< "x"` into (Parable op, remainder).
fn split_redirect(text: &str) -> Option<(&str, &str)> {
    const OPS: [&str; 8] = ["&>>", "<<<", ">>", ">|", "&>", "<>", ">", "<"];
    for op in OPS {
        if let Some(rest) = text.strip_prefix(op) {
            return Some((op, rest));
        }
    }
    None
}

fn list_op(op: ListOperator) -> &'static str {
    match op {
        ListOperator::And => "&&",
        ListOperator::Or => "||",
        ListOperator::Semi => ";",
        ListOperator::Background => "&",
    }
}

fn is_separator(op: &str) -> bool {
    op == ";" || op == "&"
}

fn finish_group(mut group: Vec<Node>) -> Node {
    if group.len() == 1 {
        group.remove(0)
    } else {
        Node::List { parts: group }
    }
}

/// Parable-style nodes for the substitutions in a raw word.
fn substitution_nodes(value: &str) -> Vec<Node> {
    match scan::substitutions(value) {
        Ok(subs) => subs
            .into_iter()
            .map(|sub| match sub.kind {
                SubKind::Command => Node::CmdSub {
                    command: boxed(parse_inner(&sub.inner)),
                },
                SubKind::Process(direction) => Node::ProcSub {
                    direction: direction.to_string(),
                    command: boxed(parse_inner(&sub.inner)),
                },
                SubKind::Arithmetic => Node::Arith {
                    cmdsubs: arith_cmdsubs(&sub.inner),
                },
                SubKind::Deprecated => Node::ArithDeprecated,
            })
            .collect(),
        Err(why) => vec![Node::Unsupported(why)],
    }
}

/// Command substitutions inside an arithmetic expression.
fn arith_cmdsubs(text: &str) -> Vec<Node> {
    substitution_nodes(text)
        .into_iter()
        .flat_map(|n| match n {
            Node::Arith { cmdsubs } => cmdsubs,
            other => vec![other],
        })
        .collect()
}

/// Placeholder for a `'` that follows `$` inside double quotes.
const MASK: char = '\u{E000}';

fn fix(s: &str) -> String {
    s.replace(MASK, "'")
}

/// Replace `'` in `$'` inside double-quoted regions with [`MASK`].
fn mask_dquoted_ansi_c(src: &str) -> String {
    let mut chars: Vec<char> = src.chars().collect();
    let mut in_dq = false;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '\'' if !in_dq => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '"' => in_dq = !in_dq,
            '$' if in_dq && chars.get(i + 1) == Some(&'\'') => {
                chars[i + 1] = MASK;
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    chars.into_iter().collect()
}

/// `name=(...)` / `name+=(...)`: Parable records an array part.
fn is_array_assignment(value: &str) -> bool {
    let Some(eq) = value.find("=(") else {
        return false;
    };
    let name = value[..eq].trim_end_matches('+');
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && value.ends_with(')')
}

fn collect_heredocs(n: &RNode, out: &mut Vec<(String, bool)>) {
    match &n.kind {
        K::HereDoc {
            delimiter,
            strip_tabs,
            ..
        } => out.push((delimiter.clone(), *strip_tabs)),
        K::Command { redirects, .. } => redirects.iter().for_each(|r| collect_heredocs(r, out)),
        K::Pipeline { commands, .. } => commands.iter().for_each(|c| collect_heredocs(c, out)),
        K::List { items } => items.iter().for_each(|i| collect_heredocs(&i.command, out)),
        K::If {
            condition,
            then_body,
            else_body,
            redirects,
        } => {
            collect_heredocs(condition, out);
            collect_heredocs(then_body, out);
            if let Some(e) = else_body {
                collect_heredocs(e, out);
            }
            redirects.iter().for_each(|r| collect_heredocs(r, out));
        }
        K::While {
            condition,
            body,
            redirects,
        }
        | K::Until {
            condition,
            body,
            redirects,
        } => {
            collect_heredocs(condition, out);
            collect_heredocs(body, out);
            redirects.iter().for_each(|r| collect_heredocs(r, out));
        }
        K::For {
            body, redirects, ..
        }
        | K::ForArith {
            body, redirects, ..
        }
        | K::Select {
            body, redirects, ..
        }
        | K::Subshell { body, redirects }
        | K::BraceGroup { body, redirects } => {
            collect_heredocs(body, out);
            redirects.iter().for_each(|r| collect_heredocs(r, out));
        }
        K::Case {
            patterns,
            redirects,
            ..
        } => {
            for p in patterns {
                if let Some(b) = &p.body {
                    collect_heredocs(b, out);
                }
            }
            redirects.iter().for_each(|r| collect_heredocs(r, out));
        }
        K::Function { body, .. } => collect_heredocs(body, out),
        K::Coproc { command, .. } => collect_heredocs(command, out),
        K::Negation { pipeline } | K::Time { pipeline, .. } => collect_heredocs(pipeline, out),
        _ => {}
    }
}

/// Span end corrected for Rable nodes whose own span stops early.
fn effective_end(src: &[char], n: &RNode) -> Option<usize> {
    match &n.kind {
        K::ArithmeticCommand {
            raw_content,
            redirects,
            ..
        } => {
            let start = (n.span.start..src.len().saturating_sub(1))
                .find(|&i| src[i] == '(' && src[i + 1] == '(')?;
            let end = start + 2 + raw_content.chars().count();
            if src.get(end) != Some(&')') || src.get(end + 1) != Some(&')') {
                return None;
            }
            Some(
                redirects
                    .iter()
                    .map(|r| r.span.end)
                    .fold(end + 2, usize::max),
            )
        }
        K::List { items } => {
            let last = items.last()?;
            Some(n.span.end.max(effective_end(src, &last.command)?))
        }
        K::Pipeline { commands, .. } => {
            let last = commands.last()?;
            Some(n.span.end.max(effective_end(src, last)?))
        }
        _ => Some(n.span.end),
    }
}

/// Text outside the top-level node spans may only be blanks, separators,
/// comments and here-document bodies; consecutive nodes need a separator.
fn check_coverage(src: &str, nodes: &[RNode]) -> Result<(), ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let fail = || ParseError {
        message: "syntax error: unparsed input".into(),
    };
    let mut pending: Vec<(String, bool)> = Vec::new();
    let mut pos = 0usize;
    for (ni, n) in nodes.iter().enumerate() {
        // Rable starts function and `for` spans after the first token.
        let loose_start = matches!(
            n.kind,
            K::Function { .. } | K::For { .. } | K::ForArith { .. } | K::Select { .. }
        );
        let separated = scan_gap(
            &chars,
            pos,
            n.span.start.max(pos),
            &mut pending,
            loose_start,
        )?;
        if ni > 0 && !separated {
            return Err(fail());
        }
        collect_heredocs(n, &mut pending);
        pos = pos.max(effective_end(&chars, n).ok_or_else(fail)?);
    }
    scan_gap(&chars, pos, chars.len(), &mut pending, false)?;
    Ok(())
}

/// Check one gap; returns whether it contained a command separator.
fn scan_gap(
    chars: &[char],
    start: usize,
    end: usize,
    pending: &mut Vec<(String, bool)>,
    loose_tail: bool,
) -> Result<bool, ParseError> {
    let fail = || ParseError {
        message: "syntax error: unparsed input".into(),
    };
    let mut separated = false;
    let mut at_word_start = true;
    let mut i = start;
    let end = end.min(chars.len());
    while i < end {
        match chars[i] {
            ' ' | '\t' => at_word_start = true,
            ';' | '&' => {
                at_word_start = true;
                separated = true;
            }
            '\\' if chars.get(i + 1) == Some(&'\n') => i += 1,
            '\n' => {
                at_word_start = true;
                separated = true;
                while !pending.is_empty() {
                    let (delim, strip) = pending.remove(0);
                    loop {
                        let line_start = i + 1;
                        if line_start >= chars.len() {
                            break;
                        }
                        let line_end = chars[line_start..]
                            .iter()
                            .position(|c| *c == '\n')
                            .map_or(chars.len(), |p| line_start + p);
                        let line: String = chars[line_start..line_end].iter().collect();
                        i = line_end;
                        let line = if strip {
                            line.trim_start_matches('\t')
                        } else {
                            &line
                        };
                        if line == delim {
                            break;
                        }
                    }
                }
            }
            '#' if at_word_start => {
                while i + 1 < chars.len() && chars[i + 1] != '\n' {
                    i += 1;
                }
            }
            c if loose_tail && (c.is_alphanumeric() || c == '_' || c == '-' || c == '.') => {
                // Unspanned leading keyword or function name.
                if chars[i..end]
                    .iter()
                    .all(|c| !matches!(c, ';' | '&' | '|' | '<' | '>' | '(' | ')' | '\n'))
                {
                    return Ok(separated);
                }
                return Err(fail());
            }
            _ => return Err(fail()),
        }
        i += 1;
    }
    Ok(separated)
}
