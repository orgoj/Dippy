//! Port of `dippy.core.analyzer`: one recursive walk of the Parable-shaped
//! AST; decisions combine as deny > ask > allow.
//!
//! Deliberate, fail-closed differences from Python (Rust asks where Python
//! allows):
//! - command substitutions inside `$(( ))` word parts and array literals are
//!   analysed (Python ignores `arith` and `array` parts);
//! - constructs the Rable adapter cannot rebuild ([`Node::Unsupported`]) ask.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::allowlists::{is_simple_safe, is_wrapper_command};
use crate::ast::{self, Node, Word};
use crate::cli::{self, Action as HAction, HandlerContext};
use crate::config::{self, Config, Flags, SimpleCommand, WrapperInfo};
use crate::parser::strip_quotes;
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Ask,
    Deny,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Allow => "allow",
            Action::Ask => "ask",
            Action::Deny => "deny",
        }
    }
}

/// Result of analyzing an AST node.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub action: Action,
    pub reason: String,
    pub context_flags: Option<Flags>,
    /// Env-stripped command for the audit log suggestion (ask only).
    pub suggestion: Option<String>,
}

impl Decision {
    fn new(action: Action, reason: impl Into<String>) -> Self {
        Self {
            action,
            reason: reason.into(),
            context_flags: None,
            suggestion: None,
        }
    }
    fn allow(reason: impl Into<String>) -> Self {
        Self::new(Action::Allow, reason)
    }
    fn ask(reason: impl Into<String>) -> Self {
        Self::new(Action::Ask, reason)
    }
    fn flags(mut self, flags: &Flags) -> Self {
        self.context_flags = Some(flags.clone());
        self
    }
    fn suggest(mut self, suggestion: &str) -> Self {
        self.suggestion = Some(suggestion.to_string());
        self
    }
}

fn with(flags: &Flags, extra: &[&str]) -> Flags {
    let mut f = flags.clone();
    f.extend(extra.iter().map(|s| s.to_string()));
    f
}

const SAFE_REDIRECT_TARGETS: [&str; 4] = ["/dev/null", "-", "/dev/stdout", "/dev/stdin"];
const FILE_WRITING_REDIRECTS: [&str; 7] = [">", ">>", ">|", ">&", "&>", "&>>", "<>"];

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_numeric())
}

/// Whether a redirect operator can open its target for writing.
fn redirect_writes_file(op: &str) -> bool {
    let op = if op.starts_with('{') {
        match op.find('}') {
            Some(end) => &op[end + 1..],
            None => op,
        }
    } else {
        op.trim_start_matches(|c: char| c.is_ascii_digit())
    };
    FILE_WRITING_REDIRECTS.contains(&op)
}

/// Whether a redirect target is a sink or literal fd operation.
fn redirect_target_is_safe(target: &str) -> bool {
    if SAFE_REDIRECT_TARGETS.contains(&target) || target == "&-" {
        return true;
    }
    if let Some(d) = target.strip_prefix('&') {
        return is_digits(d.strip_suffix('-').unwrap_or(d));
    }
    false
}

/// One literal stdin heredoc, allowing separately checked output redirects.
fn quoted_heredoc_content(redirects: &[Node]) -> Option<String> {
    let heredocs: Vec<&Node> = redirects
        .iter()
        .filter(|r| matches!(r, Node::HereDoc { .. }))
        .collect();
    if heredocs.len() != 1 {
        return None;
    }
    let Node::HereDoc {
        content,
        quoted,
        fd,
    } = heredocs[0]
    else {
        return None;
    };
    if !matches!(fd, None | Some(0)) || !quoted || content.trim().is_empty() {
        return None;
    }
    for other in redirects {
        let Node::Redirect { op, target } = other else {
            if std::ptr::eq(other, heredocs[0]) {
                continue;
            }
            // Another heredoc cannot exist (len == 1); anything else is odd.
            return None;
        };
        let operator = op.trim_start_matches(|c: char| c.is_ascii_digit());
        let descriptor = &op[..op.len() - operator.len()];
        if !redirect_writes_file(op)
            || operator == "<>"
            || op.starts_with('{')
            || (!descriptor.is_empty() && descriptor.parse::<u64>().ok() == Some(0))
        {
            return None;
        }
        let target = strip_quotes(&target.value);
        if target.starts_with('&') || operator == ">&" {
            let fd_target = target.strip_prefix('&').unwrap_or(target);
            if fd_target != "-" && !is_digits(fd_target.strip_suffix('-').unwrap_or(fd_target)) {
                return None;
            }
            if let Some(n) = fd_target.strip_suffix('-') {
                if is_digits(n) && n.parse::<u64>().ok() == Some(0) {
                    return None;
                }
            }
        }
    }
    Some(content.clone())
}

/// `analyze`: parse and analyse a command string.
pub fn analyze(
    command: &str,
    config: &Config,
    cwd: &Path,
    context_flags: Option<&Flags>,
    remote: bool,
) -> Decision {
    let command = command.trim();
    if command.is_empty() {
        return Decision::ask("empty command");
    }
    let nodes = match ast::parse(command) {
        Ok(n) => n,
        Err(e) => return Decision::ask(format!("parse error: {}", e.message)),
    };
    if nodes.is_empty() {
        return Decision::ask("empty command");
    }
    let mut flags = context_flags.cloned().unwrap_or_default();
    flags.extend(config::env_context_flags(config));
    let decisions: Vec<Decision> = nodes
        .iter()
        .map(|n| analyze_node(n, config, cwd, &flags, remote))
        .collect();
    combine(decisions)
}

fn analyze_node(node: &Node, config: &Config, cwd: &Path, flags: &Flags, remote: bool) -> Decision {
    let recurse = |n: &Node, f: &Flags| analyze_node(n, config, cwd, f, remote);
    let redirects =
        |n: &Node, f: &Flags| analyze_redirects(n.redirects(), config, cwd, Some(f), remote);
    match node {
        Node::Command { .. } => analyze_command(node, config, cwd, flags, remote),
        Node::Pipeline { commands } => {
            let pf = with(flags, &["@pipeline", "@compound"]);
            let decisions: Vec<Decision> = commands.iter().map(|c| recurse(c, &pf)).collect();
            let reasons: Vec<String> = decisions.iter().map(|d| d.reason.clone()).collect();
            let result = combine(decisions);
            if result.action == Action::Allow {
                return Decision::allow(reasons.join(", ")).flags(&pf);
            }
            result
        }
        Node::List { parts } => {
            let lf = with(flags, &["@compound"]);
            let parts: Vec<&Node> = parts
                .iter()
                .filter(|p| !matches!(p, Node::Operator { .. }))
                .collect();
            let mut effective_cwd = cwd.to_path_buf();
            if !parts.is_empty() && !remote {
                if let Some(target) = extract_cd_target(parts[0]) {
                    effective_cwd = resolve_cd_target(&target, cwd);
                }
            }
            let mut decisions = Vec::new();
            for p in parts {
                let is_cd = matches!(p, Node::Command { words, .. } if words.first().is_some_and(|w| strip_quotes(&w.value) == "cd"));
                if remote && is_cd {
                    continue;
                }
                decisions.push(analyze_node(p, config, &effective_cwd, &lf, remote));
            }
            if decisions.is_empty() {
                return Decision::allow("empty list").flags(&lf);
            }
            let reasons: Vec<String> = decisions.iter().map(|d| d.reason.clone()).collect();
            let result = combine(decisions);
            if result.action == Action::Allow {
                return Decision::allow(reasons.join(", ")).flags(&lf);
            }
            result
        }
        Node::If {
            condition,
            then_body,
            else_body,
            ..
        } => {
            let mut d = vec![recurse(condition, flags), recurse(then_body, flags)];
            if let Some(e) = else_body {
                d.push(recurse(e, flags));
            }
            d.extend(redirects(node, flags));
            combine(d)
        }
        Node::While {
            condition, body, ..
        }
        | Node::Until {
            condition, body, ..
        } => {
            let mut d = vec![recurse(condition, flags), recurse(body, flags)];
            d.extend(redirects(node, flags));
            combine(d)
        }
        Node::For { words, body, .. } | Node::Select { words, body, .. } => {
            let mut d = vec![recurse(body, flags)];
            for w in words.iter().flatten() {
                d.extend(analyze_word_parts(w, config, cwd, flags, remote));
            }
            d.extend(redirects(node, flags));
            combine(d)
        }
        Node::ForArith {
            init,
            cond,
            incr,
            body,
            ..
        } => {
            let mut d = vec![recurse(body, flags)];
            for expr in [init, cond, incr] {
                if !expr.is_empty() {
                    d.extend(analyze_string_cmdsubs(
                        expr,
                        config,
                        cwd,
                        Some(flags),
                        remote,
                    ));
                }
            }
            d.extend(redirects(node, flags));
            combine(d)
        }
        Node::Case { word, bodies, .. } => {
            let mut d = analyze_word_parts(word, config, cwd, flags, remote);
            for b in bodies.iter().flatten() {
                d.push(recurse(b, flags));
            }
            d.extend(redirects(node, flags));
            if d.is_empty() {
                Decision::allow("empty case")
            } else {
                combine(d)
            }
        }
        Node::Function { body } => recurse(body, flags),
        Node::Subshell { body, .. } => {
            let sf = with(flags, &["@subshell", "@compound"]);
            let mut d = vec![recurse(body, &sf)];
            d.extend(redirects(node, &sf));
            combine(d)
        }
        Node::BraceGroup { body, .. } => {
            let bf = with(flags, &["@bracegroup", "@compound"]);
            let mut d = vec![recurse(body, &bf)];
            d.extend(redirects(node, &bf));
            combine(d)
        }
        Node::Time { pipeline } | Node::Negation { pipeline } => recurse(pipeline, flags),
        Node::Coproc { command } => recurse(command, flags),
        Node::CondExpr { body, .. } => {
            let mut d = analyze_cond_node(body, config, cwd, flags, remote);
            d.extend(redirects(node, flags));
            if d.is_empty() {
                Decision::allow("conditional")
            } else {
                combine(d)
            }
        }
        Node::ArithCmd { cmdsubs, .. } => {
            let mut d = Vec::new();
            for c in cmdsubs {
                d.push(substitution_decision(
                    c,
                    config,
                    cwd,
                    flags,
                    remote,
                    "arithmetic cmdsub",
                ));
            }
            d.extend(redirects(node, flags));
            if d.is_empty() {
                Decision::allow("arithmetic")
            } else {
                combine(d)
            }
        }
        Node::Comment => Decision::allow("comment"),
        Node::Empty => Decision::allow("empty"),
        Node::Unsupported(what) => Decision::ask(format!("unrecognized construct: {what}")),
        other => Decision::ask(format!("unrecognized construct: {}", other.kind())),
    }
}

/// Decision for one command/process substitution part (or an unsupported
/// part), prefixed like Python's messages when not allowed.
fn substitution_decision(
    part: &Node,
    config: &Config,
    cwd: &Path,
    flags: &Flags,
    remote: bool,
    label: &str,
) -> Decision {
    let inner = match part {
        Node::CmdSub { command } | Node::ProcSub { command, .. } => {
            analyze_node(command, config, cwd, flags, remote)
        }
        Node::Unsupported(what) => Decision::ask(format!("unrecognized construct: {what}")),
        _ => return Decision::allow("expansion"),
    };
    if inner.action != Action::Allow {
        let label = match part {
            Node::ProcSub { direction, .. } => format!("{label} {direction}(...)"),
            _ => label.to_string(),
        };
        return Decision::new(inner.action, format!("{label}: {}", inner.reason));
    }
    inner
}

/// Command substitutions hidden in parts Python skips (`$(( ))`, unparsed).
fn hidden_parts(part: &Node) -> Vec<&Node> {
    match part {
        Node::Arith { cmdsubs } => cmdsubs.iter().collect(),
        Node::Unsupported(_) => vec![part],
        _ => Vec::new(),
    }
}

/// `(destination, inner_command, context_value)` for a custom wrapper.
fn extract_wrapper_args(
    tokens: &[String],
    info: Option<&WrapperInfo>,
) -> (Option<String>, String, Option<String>) {
    if tokens.len() < 2 {
        return (None, String::new(), None);
    }
    let trigger = info.and_then(|i| i.trigger.clone());
    let target_flag = info.and_then(|i| i.target_flag.clone());
    let context_flag = info.and_then(|i| i.context_flag.clone());
    const OPTS_WITH_ARG: [&str; 23] = [
        "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o", "-p",
        "-Q", "-R", "-S", "-W", "-w", "-t", "", "",
    ];
    let takes_arg = |t: &str| !t.is_empty() && OPTS_WITH_ARG.contains(&t);
    let index_in = |needle: &str, start: usize, end: usize| -> Option<usize> {
        tokens[start..end.min(tokens.len())]
            .iter()
            .position(|t| t == needle)
            .map(|p| p + start)
    };
    if let Some(trigger) = trigger {
        let Some(idx) = index_in(&trigger, 0, tokens.len()) else {
            return (None, String::new(), None);
        };
        let inner = if idx + 1 < tokens.len() {
            tokens[idx + 1..].join(" ")
        } else {
            String::new()
        };
        let mut context_value = None;
        if let Some(cf) = &context_flag {
            if let Some(c) = index_in(cf, 0, idx) {
                if c + 1 < idx {
                    context_value = Some(tokens[c + 1].clone());
                }
            }
        }
        let mut dest = None;
        if let Some(tf) = &target_flag {
            if let Some(t) = index_in(tf, 0, idx) {
                if t + 1 < idx {
                    dest = Some(tokens[t + 1].clone());
                }
            }
            if dest.is_none() {
                let mut i = 1;
                while i < idx {
                    let tok = &tokens[i];
                    if tok.starts_with('-') {
                        i += if takes_arg(tok) { 2 } else { 1 };
                    } else {
                        dest = Some(tok.clone());
                        break;
                    }
                }
            }
        }
        return (dest, inner, context_value);
    }
    let mut i = 1;
    let mut dest = None;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok == "--" {
            i += 1;
            if i < tokens.len() {
                dest = Some(tokens[i].clone());
                i += 1;
            }
            break;
        } else if tok.starts_with('-') {
            i += if takes_arg(tok) { 2 } else { 1 };
        } else {
            dest = Some(tok.clone());
            i += 1;
            break;
        }
    }
    let Some(dest) = dest else {
        return (None, String::new(), None);
    };
    let inner = if i < tokens.len() {
        tokens[i..].join(" ")
    } else {
        String::new()
    };
    let mut context_value = None;
    if let Some(cf) = &context_flag {
        if let Some(c) = index_in(cf, 0, tokens.len()) {
            if c + 1 < tokens.len() {
                context_value = Some(tokens[c + 1].clone());
            }
        }
    }
    (Some(dest), inner, context_value)
}

fn is_catch_all_pattern(pattern: &str) -> bool {
    matches!(pattern.trim(), "*" | "**")
}

static DURATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d+(\.\d+)?[smhdSMHD]?$").unwrap());
static NICE_ADJ_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+-]\d+$").unwrap());
static CPU_MASK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(0x[0-9a-fA-F]+|\d+([,-]\d+)*)$").unwrap());

fn or_zero(k: usize, len: usize) -> usize {
    if k < len { k } else { 0 }
}

/// Index where the inner command of a transparent wrapper starts (0: none).
fn find_transparent_inner_idx(tokens: &[String], config: Option<&Config>) -> usize {
    if tokens.len() < 2 {
        return 0;
    }
    let base = tokens[0].as_str();
    let transparent = is_wrapper_command(base)
        || config.is_some_and(|c| c.wrappers.get(base).is_some_and(|w| w.transparent));
    if !transparent {
        return 0;
    }
    let n = tokens.len();
    let t = |k: usize| tokens[k].as_str();
    match base {
        "command" => {
            if matches!(t(1), "-v" | "-V") {
                return 0;
            }
            let mut k = 1;
            while k < n && t(k).starts_with('-') {
                if t(k) == "--" {
                    k += 1;
                    break;
                }
                k += 1;
            }
            or_zero(k, n)
        }
        "nohup" | "builtin" => {
            let mut k = 1;
            if k < n && t(k) == "--" {
                k += 1;
            }
            or_zero(k, n)
        }
        "timeout" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(tok, "-s" | "--signal" | "-k" | "--kill-after") {
                    k += 2;
                    continue;
                }
                if (tok.starts_with("-s") || tok.starts_with("-k")) && tok.chars().count() > 2 {
                    k += 1;
                    continue;
                }
                if tok.starts_with("--signal=")
                    || tok.starts_with("--kill-after=")
                    || tok.starts_with('-')
                {
                    k += 1;
                    continue;
                }
                break;
            }
            if k < n && DURATION_RE.is_match(t(k)) {
                k += 1;
                if k < n && t(k) == "--" {
                    k += 1;
                }
                return or_zero(k, n);
            }
            0
        }
        "nice" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(tok, "-n" | "--adjustment") {
                    k += 2;
                    continue;
                }
                if (tok.starts_with("-n") && tok.chars().count() > 2)
                    || tok.starts_with("--adjustment=")
                    || NICE_ADJ_RE.is_match(tok)
                    || tok.starts_with('-')
                {
                    k += 1;
                    continue;
                }
                break;
            }
            or_zero(k, n)
        }
        "ionice" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(
                    tok,
                    "-c" | "--class" | "-n" | "--classdata" | "-p" | "--pid"
                ) {
                    k += 2;
                    continue;
                }
                if ((tok.starts_with("-c") || tok.starts_with("-n") || tok.starts_with("-p"))
                    && tok.chars().count() > 2)
                    || tok.starts_with("--class=")
                    || tok.starts_with("--classdata=")
                    || tok.starts_with("--pid=")
                    || tok.starts_with('-')
                {
                    k += 1;
                    continue;
                }
                break;
            }
            or_zero(k, n)
        }
        "time" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(tok, "-f" | "--format" | "-o" | "--output") {
                    k += 2;
                    continue;
                }
                if tok.starts_with("--format=")
                    || tok.starts_with("--output=")
                    || tok.starts_with('-')
                {
                    k += 1;
                    continue;
                }
                break;
            }
            or_zero(k, n)
        }
        "taskset" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(tok, "-c" | "--cpu-list") {
                    k += 2;
                    continue;
                }
                if (tok.starts_with("-c") || tok.starts_with("--cpu-list="))
                    && tok.chars().count() > 2
                {
                    k += 1;
                    continue;
                }
                if matches!(tok, "-p" | "--pid") {
                    return 0;
                }
                if tok.starts_with('-') {
                    k += 1;
                    continue;
                }
                if CPU_MASK_RE.is_match(tok) {
                    k += 1;
                }
                break;
            }
            if k < n && t(k) == "--" {
                k += 1;
            }
            or_zero(k, n)
        }
        "chrt" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(tok, "-p" | "--pid") {
                    return 0;
                }
                if tok.starts_with('-') {
                    k += 1;
                    continue;
                }
                if is_digits(tok) {
                    k += 1;
                }
                break;
            }
            if k < n && t(k) == "--" {
                k += 1;
            }
            or_zero(k, n)
        }
        "stdbuf" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(tok, "-i" | "--input" | "-o" | "--output" | "-e" | "--error") {
                    k += 2;
                    continue;
                }
                if ["-i", "-o", "-e", "--input=", "--output=", "--error="]
                    .iter()
                    .any(|p| tok.starts_with(p))
                    || tok.starts_with('-')
                {
                    k += 1;
                    continue;
                }
                break;
            }
            or_zero(k, n)
        }
        "flock" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(
                    tok,
                    "-w" | "--wait" | "--timeout" | "-E" | "--conflict-exit-code"
                ) {
                    k += 2;
                    continue;
                }
                if ["--wait=", "--timeout=", "--conflict-exit-code="]
                    .iter()
                    .any(|p| tok.starts_with(p))
                    || tok.starts_with('-')
                {
                    k += 1;
                    continue;
                }
                k += 1;
                break;
            }
            if k < n && t(k) == "--" {
                k += 1;
            }
            or_zero(k, n)
        }
        "strace" | "ltrace" => {
            let mut k = 1;
            while k < n {
                let tok = t(k);
                if tok == "--" {
                    k += 1;
                    break;
                }
                if matches!(
                    tok,
                    "-e" | "-o" | "-p" | "-s" | "-u" | "-E" | "-P" | "-y" | "-Y"
                ) {
                    k += 2;
                    continue;
                }
                if tok.starts_with('-') {
                    k += 1;
                    continue;
                }
                break;
            }
            or_zero(k, n)
        }
        _ => {
            let mut k = 1;
            while k < n && t(k).starts_with('-') {
                if t(k) == "--" {
                    k += 1;
                    break;
                }
                k += 1;
            }
            or_zero(k, n)
        }
    }
}

fn unwrap_all_transparent_wrappers(tokens: &[String], config: Option<&Config>) -> usize {
    let mut offset = 0;
    let mut current = tokens;
    while current.len() > 1 {
        let idx = find_transparent_inner_idx(current, config);
        if idx == 0 {
            break;
        }
        offset += idx;
        current = &current[idx..];
    }
    offset
}

fn word_value(w: &Word) -> String {
    strip_quotes(&w.value).to_string()
}

fn base_index(words: &[String]) -> usize {
    let mut i = 0;
    while i < words.len() && words[i].contains('=') && !words[i].starts_with('-') {
        i += 1;
    }
    i
}

/// `str.strip("$()")`.
fn strip_dollar_parens(s: &str) -> &str {
    s.trim_matches(|c| c == '$' || c == '(' || c == ')')
}

fn analyze_command(
    node: &Node,
    config: &Config,
    cwd: &Path,
    flags: &Flags,
    remote: bool,
) -> Decision {
    let Node::Command {
        words: node_words,
        redirects,
    } = node
    else {
        return Decision::ask("unrecognized construct: command");
    };
    let mut decisions = Vec::new();
    let words: Vec<String> = node_words.iter().map(word_value).collect();
    let raw_words: Vec<String> = node_words.iter().map(|w| w.value.clone()).collect();
    let has_exp: Vec<bool> = node_words.iter().map(|w| !w.parts.is_empty()).collect();
    let base_idx = base_index(&words);
    let base = words.get(base_idx).cloned().unwrap_or_default();
    let handler = cli::get_handler(&base);
    let simple_safe = is_simple_safe(&base);

    for (position, word) in node_words.iter().enumerate() {
        let parts = &word.parts;
        let is_pure_cmdsub = parts.len() == 1
            && matches!(parts[0], Node::CmdSub { .. })
            && word.value.starts_with("$(")
            && word.value.ends_with(')');
        for part in parts {
            match part {
                Node::ProcSub { direction, command } => {
                    let inner = analyze_node(command, config, cwd, flags, remote);
                    if inner.action != Action::Allow {
                        return Decision::new(
                            inner.action,
                            format!("process substitution {direction}(...): {}", inner.reason),
                        );
                    }
                    decisions.push(inner);
                }
                Node::CmdSub { command } => {
                    let inner = analyze_node(command, config, cwd, flags, remote);
                    if inner.action != Action::Allow {
                        return Decision::new(
                            inner.action,
                            format!("command substitution: {}", inner.reason),
                        );
                    }
                    decisions.push(inner);
                    if is_pure_cmdsub && !simple_safe && position > base_idx {
                        if let Some(h) = handler {
                            let ctx = HandlerContext {
                                tokens: words[base_idx..].to_vec(),
                                remote,
                                cwd: cwd.to_path_buf(),
                                config: None,
                                word_has_expansions: Vec::new(),
                                raw_words: Vec::new(),
                            };
                            if (h.classify)(&ctx).action != HAction::Allow {
                                let inner_cmd = strip_dollar_parens(&word_value(word)).to_string();
                                return Decision::ask(format!(
                                    "cmdsub injection risk: {inner_cmd}"
                                ));
                            }
                        }
                    }
                }
                Node::Param { arg } => {
                    if let Some(arg) = arg.as_deref().filter(|a| !a.is_empty()) {
                        let pds = analyze_string_cmdsubs(arg, config, cwd, Some(flags), remote);
                        if let Some(pd) = pds.iter().find(|d| d.action != Action::Allow) {
                            return pd.clone();
                        }
                        decisions.extend(pds);
                    }
                }
                other => {
                    // Fail closed where Python ignores the part.
                    for hidden in hidden_parts(other) {
                        let d = substitution_decision(
                            hidden,
                            config,
                            cwd,
                            flags,
                            remote,
                            "command substitution",
                        );
                        if d.action != Action::Allow {
                            return d;
                        }
                        decisions.push(d);
                    }
                }
            }
        }
    }

    let redirect_decisions = analyze_redirects(redirects, config, cwd, Some(flags), remote);
    if let Some(rd) = redirect_decisions
        .iter()
        .find(|d| d.action != Action::Allow)
    {
        return rd.clone();
    }
    decisions.extend(redirect_decisions);

    if words.is_empty() {
        return Decision::allow("empty command");
    }
    if base == "[" || base == "test" {
        decisions.push(Decision::allow("conditional test"));
        return combine(decisions);
    }
    decisions.push(analyze_simple_command(
        &words, config, cwd, flags, remote, &has_exp, &raw_words, redirects,
    ));
    combine(decisions)
}

fn analyze_redirects(
    redirects: &[Node],
    config: &Config,
    cwd: &Path,
    flags: Option<&Flags>,
    remote: bool,
) -> Vec<Decision> {
    let empty = Flags::new();
    let flags_ref = flags.unwrap_or(&empty);
    let mut decisions = Vec::new();
    for r in redirects {
        match r {
            Node::HereDoc {
                content, quoted, ..
            } => {
                if !quoted && !content.is_empty() {
                    decisions.extend(analyze_string_cmdsubs(content, config, cwd, flags, remote));
                }
            }
            Node::Redirect { op, target } => {
                let target_value = word_value(target);
                // Python analyses redirect-target substitutions without flags.
                decisions.extend(analyze_word_parts(
                    target,
                    config,
                    cwd,
                    &Flags::new(),
                    remote,
                ));
                if remote {
                    if flags_ref.contains("ssh")
                        && redirect_writes_file(op)
                        && !redirect_target_is_safe(&target_value)
                    {
                        decisions.push(Decision::ask(format!("remote redirect to {target_value}")));
                    }
                    continue;
                }
                if redirect_target_is_safe(&target_value) {
                    continue;
                }
                if redirect_writes_file(op) {
                    match config::match_redirect(&target_value, config, cwd, flags_ref, false) {
                        Some(m) if m.decision == "allow" => {
                            decisions.push(Decision::allow(format!("redirect to {target_value}")))
                        }
                        Some(m) => {
                            let msg = m.message.clone().unwrap_or(m.pattern.clone());
                            let action = if m.decision == "deny" {
                                Action::Deny
                            } else {
                                Action::Ask
                            };
                            decisions.push(Decision::new(
                                action,
                                format!("redirect to {target_value}: {msg}"),
                            ));
                        }
                        None => {
                            decisions.push(Decision::ask(format!("redirect to {target_value}")))
                        }
                    }
                }
            }
            Node::Unsupported(what) => {
                decisions.push(Decision::ask(format!("unrecognized construct: {what}")))
            }
            _ => {}
        }
    }
    decisions
}

fn config_match_decision(
    m: &config::Match,
    base: &str,
    flags: &Flags,
    suggestion: &str,
) -> Option<Decision> {
    match m.decision.as_str() {
        "allow" => {
            let reason = if m.pattern.starts_with(&format!("{base} ")) {
                m.pattern.clone()
            } else {
                format!("{base} ({})", m.pattern)
            };
            Some(Decision::allow(reason).flags(flags))
        }
        "deny" => {
            let msg = m.message.clone().unwrap_or(m.pattern.clone());
            Some(Decision::new(Action::Deny, format!("{base}: {msg}")).flags(flags))
        }
        "ask" => {
            let msg = m.message.clone().unwrap_or(m.pattern.clone());
            Some(
                Decision::ask(format!("{base}: {msg}"))
                    .flags(flags)
                    .suggest(suggestion),
            )
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze_simple_command(
    words: &[String],
    config: &Config,
    cwd: &Path,
    flags: &Flags,
    remote: bool,
    has_exp: &[bool],
    raw_words: &[String],
    redirects: &[Node],
) -> Decision {
    if words.is_empty() {
        return Decision::allow("empty").flags(flags);
    }
    let i = base_index(words);
    if i >= words.len() {
        return Decision::allow("env assignment").flags(flags);
    }
    let base = words[i].as_str();
    let tokens = &words[i..];
    let suggestion = tokens.join(" ");
    let slice_bool = |v: &[bool], k: usize| -> Vec<bool> {
        v.get(k..).map(<[bool]>::to_vec).unwrap_or_default()
    };
    let slice_str = |v: &[String], k: usize| -> Vec<String> {
        v.get(k..).map(<[String]>::to_vec).unwrap_or_default()
    };
    let sc = SimpleCommand {
        words,
        raw_words,
        word_has_expansions: has_exp,
    };

    let wrapper_offset = unwrap_all_transparent_wrappers(tokens, Some(config));
    if wrapper_offset > 0 {
        if let Some(m) = config::match_command(&sc, config, cwd, flags, remote) {
            if !is_catch_all_pattern(&m.pattern) {
                if let Some(d) = config_match_decision(&m, base, flags, &suggestion) {
                    return d;
                }
            }
        }
        let k = i + wrapper_offset;
        return analyze_simple_command(
            &words[k..],
            config,
            cwd,
            flags,
            remote,
            &slice_bool(has_exp, k),
            &slice_str(raw_words, k),
            redirects,
        );
    }

    // 1. Config rules (highest priority); delegate falls through.
    if let Some(m) = config::match_command(&sc, config, cwd, flags, remote) {
        if let Some(d) = config_match_decision(&m, base, flags, &suggestion) {
            return d;
        }
    }

    // 2. Dippy execution subcommands with one literal script on stdin.
    if base == "dippy" && tokens.len() > 1 && (tokens[1] == "run" || tokens[1] == "run-on-server") {
        let content = quoted_heredoc_content(redirects);
        if tokens[1] == "run" && tokens.len() == 2 {
            if let Some(content) = &content {
                return analyze(content, config, cwd, Some(flags), remote);
            }
        }
        if tokens[1] == "run-on-server"
            && tokens.len() == 3
            && content.is_some()
            && !(!has_exp.is_empty() && has_exp.get(i + 2).copied().unwrap_or(false))
        {
            let inner_flags = with(flags, &["run-on-server", tokens[2].as_str()]);
            return analyze(
                content.as_deref().unwrap_or(""),
                config,
                cwd,
                Some(&inner_flags),
                true,
            );
        }
        return Decision::ask(format!(
            "dippy {}: requires one non-empty quoted heredoc",
            tokens[1]
        ))
        .flags(flags)
        .suggest(&suggestion);
    }

    // 3. Wrapper commands (inner command analysed).
    if is_wrapper_command(base) && tokens.len() > 1 {
        if base == "command" && (tokens[1] == "-v" || tokens[1] == "-V") {
            return Decision::allow("command -v").flags(flags);
        }
        let mut j = 1;
        while j < tokens.len() {
            let token = tokens[j].as_str();
            if is_digits(token) || is_digits(&token.replace('.', "")) {
                j += 1;
                continue;
            }
            if token.starts_with('-') && token != "--" {
                j += 1;
                continue;
            }
            if token == "--" {
                j += 1;
            }
            break;
        }
        if j < tokens.len() {
            let k = i + j;
            return analyze_simple_command(
                &tokens[j..],
                config,
                cwd,
                flags,
                remote,
                &slice_bool(has_exp, k),
                &slice_str(raw_words, k),
                &[],
            );
        }
        return Decision::ask(base).flags(flags).suggest(&suggestion);
    }

    // 4. Simple safe commands.
    if is_simple_safe(base) {
        return Decision::allow(base).flags(flags);
    }

    // 5. Version/help checks.
    if is_version_or_help(tokens) {
        return Decision::allow(format!("{base} --help")).flags(flags);
    }

    // 6. Custom wrapper commands.
    if let Some(info) = config.wrappers.get(base) {
        if info.transparent {
            return Decision::ask(base).flags(flags).suggest(&suggestion);
        }
        let (dest, inner_cmd, context_value) = extract_wrapper_args(tokens, Some(info));
        if inner_cmd.is_empty() {
            let reason = match &dest {
                Some(d) if !d.is_empty() => format!("{base} {d}"),
                _ => base.to_string(),
            };
            return Decision::ask(reason).flags(flags).suggest(&suggestion);
        }
        let mut wrapper_context = vec![base.to_string()];
        if let Some(d) = dest.as_ref().filter(|d| !d.is_empty()) {
            if info.context_first {
                wrapper_context.push(d.clone());
            }
        }
        if let Some(c) = context_value.as_ref().filter(|c| !c.is_empty()) {
            wrapper_context.push(c.clone());
        }
        let mut inner_flags = flags.clone();
        inner_flags.extend(wrapper_context);

        if let Some(marker) = &info.script_stdin_marker {
            let trigger_idx = match &info.trigger {
                Some(t) => tokens.iter().position(|x| x == t),
                None => Some(0),
            };
            let marker_idx = trigger_idx.and_then(|ti| {
                tokens
                    .get(ti + 1..)
                    .and_then(|rest| rest.iter().position(|x| x == marker).map(|p| p + ti + 1))
            });
            if let (Some(trigger_idx), Some(marker_idx)) = (trigger_idx, marker_idx) {
                let content = quoted_heredoc_content(redirects);
                let has_expansions = !has_exp.is_empty()
                    && (0..=marker_idx).any(|k| has_exp.get(i + k).copied().unwrap_or(false));
                let mut valid_options = true;
                let mut prev_was_flag = false;
                for opt in &tokens[trigger_idx + 1..marker_idx] {
                    if opt == "--" {
                        valid_options = false;
                        break;
                    }
                    if opt.starts_with('-') {
                        prev_was_flag = true;
                    } else if prev_was_flag {
                        prev_was_flag = false;
                    } else {
                        valid_options = false;
                        break;
                    }
                }
                let valid = marker_idx == tokens.len() - 1
                    && !has_expansions
                    && valid_options
                    && content.is_some();
                if !valid {
                    return Decision::ask(format!(
                        "{base} {marker}: requires one non-empty quoted heredoc"
                    ))
                    .flags(&inner_flags);
                }
                return analyze(
                    content.as_deref().unwrap_or(""),
                    config,
                    cwd,
                    Some(&inner_flags),
                    true,
                );
            }
        }
        return analyze(&inner_cmd, config, cwd, Some(&inner_flags), true);
    }

    // 7. CLI-specific handlers.
    if let Some(handler) = cli::get_handler(base) {
        let ctx = HandlerContext {
            tokens: tokens.to_vec(),
            remote,
            cwd: cwd.to_path_buf(),
            config: Some(config),
            word_has_expansions: slice_bool(has_exp, i),
            raw_words: slice_str(raw_words, i),
        };
        let result = (handler.classify)(&ctx);
        let desc = result
            .description
            .clone()
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| cli::get_description(tokens, Some(base)));
        if let Some(targets) = &result.redirect_targets {
            for target in targets {
                if redirect_target_is_safe(target) {
                    continue;
                }
                if flags.contains("ssh") {
                    return Decision::ask(format!("remote redirect to {target}"));
                }
                match config::match_redirect(target, config, cwd, flags, false) {
                    Some(m) if m.decision == "deny" => {
                        let msg = m.message.clone().unwrap_or(m.pattern.clone());
                        return Decision::new(Action::Deny, format!("{desc}: {msg}"));
                    }
                    Some(m) if m.decision == "ask" => {
                        let msg = m.message.clone().unwrap_or(m.pattern.clone());
                        return Decision::ask(format!("{desc}: {msg}"));
                    }
                    Some(_) => {}
                    None => return Decision::ask(desc),
                }
            }
        }
        return match result.action {
            HAction::Allow => Decision::allow(desc),
            HAction::Delegate
                if result
                    .inner_command
                    .as_deref()
                    .is_some_and(|c| !c.is_empty()) =>
            {
                let mut inner_flags = flags.clone();
                if let Some(wc) = &result.wrapper_context {
                    inner_flags.extend(wc.iter().cloned());
                }
                let mut inner = analyze(
                    result.inner_command.as_deref().unwrap_or(""),
                    config,
                    cwd,
                    Some(&inner_flags),
                    remote || result.remote,
                );
                if !suggestion.is_empty()
                    && inner.action == Action::Ask
                    && inner.suggestion.is_some()
                    && result.replace_suggestion
                {
                    inner.suggestion = Some(suggestion.clone());
                }
                inner
            }
            _ => Decision::ask(desc).flags(flags).suggest(&suggestion),
        };
    }

    // 8. sh/bash -c (only reached when no handler claims the shell).
    if matches!(base, "sh" | "bash" | "zsh") && tokens.len() >= 3 && tokens[1] == "-c" {
        return analyze(strip_quotes(&tokens[2]), config, cwd, Some(flags), remote);
    }

    // 9. Unknown command.
    Decision::ask(cli::get_description(tokens, Some(base)))
        .flags(flags)
        .suggest(&suggestion)
}

fn is_version_or_help(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return false;
    }
    if tokens.len() == 2
        && matches!(
            tokens[1].as_str(),
            "help" | "version" | "--version" | "--help" | "-h"
        )
    {
        return true;
    }
    let last = tokens[tokens.len() - 1].as_str();
    if (last == "--help" || last == "-h") && tokens.len() <= 4 {
        return !tokens.iter().any(|t| t == "-c" || t == "-m");
    }
    false
}

fn analyze_cond_node(
    node: &Node,
    config: &Config,
    cwd: &Path,
    flags: &Flags,
    remote: bool,
) -> Vec<Decision> {
    match node {
        Node::UnaryTest { operand } => analyze_word_parts(operand, config, cwd, flags, remote),
        Node::BinaryTest { left, right } => {
            let mut d = analyze_word_parts(left, config, cwd, flags, remote);
            d.extend(analyze_word_parts(right, config, cwd, flags, remote));
            d
        }
        Node::CondAnd { left, right } | Node::CondOr { left, right } => {
            let mut d = analyze_cond_node(left, config, cwd, flags, remote);
            d.extend(analyze_cond_node(right, config, cwd, flags, remote));
            d
        }
        Node::CondNot { operand } => analyze_cond_node(operand, config, cwd, flags, remote),
        Node::CondParen { inner } => analyze_cond_node(inner, config, cwd, flags, remote),
        // A bare term (`[[ $x ]]`): Python ignores it; fail closed on
        // substitutions inside it.
        Node::Word(w) => analyze_word_parts(w, config, cwd, flags, remote),
        Node::Unsupported(what) => vec![Decision::ask(format!("unrecognized construct: {what}"))],
        _ => Vec::new(),
    }
}

/// Command/process substitutions in a word's parts, including nested ones.
fn analyze_word_parts(
    word: &Word,
    config: &Config,
    cwd: &Path,
    flags: &Flags,
    remote: bool,
) -> Vec<Decision> {
    let mut decisions = Vec::new();
    for part in &word.parts {
        match part {
            Node::CmdSub { .. } => decisions.push(substitution_decision(
                part, config, cwd, flags, remote, "cmdsub",
            )),
            Node::ProcSub { .. } => decisions.push(substitution_decision(
                part, config, cwd, flags, remote, "procsub",
            )),
            Node::Param { arg } => {
                if let Some(arg) = arg.as_deref().filter(|a| !a.is_empty()) {
                    decisions.extend(analyze_string_cmdsubs(
                        arg,
                        config,
                        cwd,
                        Some(flags),
                        remote,
                    ));
                }
            }
            other => {
                for hidden in hidden_parts(other) {
                    decisions.push(substitution_decision(
                        hidden, config, cwd, flags, remote, "cmdsub",
                    ));
                }
            }
        }
    }
    decisions
}

/// Command substitutions in a raw string (`$(...)` with nesting, backticks).
fn analyze_string_cmdsubs(
    s: &str,
    config: &Config,
    cwd: &Path,
    flags: Option<&Flags>,
    remote: bool,
) -> Vec<Decision> {
    let chars: Vec<char> = s.chars().collect();
    let mut decisions = Vec::new();
    let push = |inner_cmd: String, decisions: &mut Vec<Decision>| {
        let inner = analyze(&inner_cmd, config, cwd, flags, remote);
        if inner.action != Action::Allow {
            decisions.push(Decision::new(
                inner.action,
                format!("cmdsub: {}", inner.reason),
            ));
        } else {
            decisions.push(inner);
        }
    };
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && chars.get(i + 1) == Some(&'(') {
            let mut depth = 1;
            let start = i + 2;
            let mut j = start;
            while j < chars.len() && depth > 0 {
                if chars[j] == '$' && chars.get(j + 1) == Some(&'(') {
                    depth += 1;
                    j += 2;
                } else if chars[j] == ')' {
                    depth -= 1;
                    j += 1;
                } else {
                    j += 1;
                }
            }
            if depth == 0 {
                push(chars[start..j - 1].iter().collect(), &mut decisions);
                i = j;
            } else {
                i += 1;
            }
        } else if chars[i] == '`' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '`' {
                j += 1;
            }
            if j < chars.len() {
                push(chars[i + 1..j].iter().collect(), &mut decisions);
                i = j + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    decisions
}

/// Target of a `cd <literal>` command.
fn extract_cd_target(node: &Node) -> Option<String> {
    let Node::Command { words, .. } = node else {
        return None;
    };
    if words.len() != 2 || word_value(&words[0]) != "cd" {
        return None;
    }
    if words[1].parts.iter().any(|p| {
        matches!(
            p,
            Node::CmdSub { .. }
                | Node::Param { .. }
                | Node::ProcSub { .. }
                | Node::Unsupported(_)
                | Node::Arith { .. }
        )
    }) {
        return None;
    }
    Some(word_value(&words[1]))
}

fn resolve_cd_target(target: &str, cwd: &Path) -> PathBuf {
    if target.starts_with('~') {
        let home = paths::home();
        if target == "~" {
            return home;
        }
        // Python: home / target[2:] (also for ~user, sic).
        return home.join(target.chars().skip(2).collect::<String>());
    }
    if target.starts_with('/') {
        return PathBuf::from(target);
    }
    paths::resolve(&cwd.join(target))
}

/// Most restrictive wins; all reasons at that level.
fn combine(decisions: Vec<Decision>) -> Decision {
    if decisions.is_empty() {
        return Decision::allow("empty");
    }
    let mut context_flags = Flags::new();
    for d in &decisions {
        if let Some(f) = &d.context_flags {
            context_flags.extend(f.iter().cloned());
        }
    }
    let reasons = |a: Action| {
        decisions
            .iter()
            .filter(|d| d.action == a)
            .map(|d| d.reason.clone())
            .collect::<Vec<_>>()
    };
    let deny = reasons(Action::Deny);
    if !deny.is_empty() {
        return Decision::new(Action::Deny, deny.join(", ")).flags(&context_flags);
    }
    let ask = reasons(Action::Ask);
    if !ask.is_empty() {
        let suggestion = decisions
            .iter()
            .find(|d| d.action == Action::Ask && d.suggestion.is_some())
            .and_then(|d| d.suggestion.clone());
        let mut d = Decision::ask(ask.join(", ")).flags(&context_flags);
        d.suggestion = suggestion;
        return d;
    }
    Decision::allow(reasons(Action::Allow).join(", ")).flags(&context_flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(cmd: &str) -> Action {
        analyze(cmd, &Config::default(), Path::new("/tmp"), None, false).action
    }

    #[test]
    fn basics() {
        assert_eq!(decide("ls -la | grep x"), Action::Allow);
        assert_eq!(decide("rm -rf x"), Action::Ask);
        assert_eq!(decide("ls > out.txt"), Action::Ask);
        assert_eq!(decide("ls > /dev/null 2>&1"), Action::Allow);
        assert_eq!(decide("echo $(rm x)"), Action::Ask);
        assert_eq!(decide("echo \"`rm x`\""), Action::Ask);
        assert_eq!(decide("timeout 5 ls"), Action::Allow);
        assert_eq!(decide("FOO=1"), Action::Allow);
    }

    #[test]
    fn fail_closed_where_python_misses() {
        assert_eq!(decide("echo $(( $(rm -rf x) ))"), Action::Ask);
        assert_eq!(decide("a=(1 2 $(rm x))"), Action::Ask);
        assert_eq!(decide("ls ;; rm x"), Action::Ask);
        assert_eq!(decide("mysql -e \"SELECT 'a$'\""), Action::Ask);
    }
}
