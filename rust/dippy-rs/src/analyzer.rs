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
use crate::bash::{decode_literal_word, shell_values};
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

/// One part of a request that asked or denied, and what a rule must match
/// to change it.
#[derive(Debug, Clone, PartialEq)]
pub struct Cause {
    /// `command` | `redirect`
    pub kind: &'static str,
    /// The simple command (or redirect target) a new rule would match.
    pub target: String,
    /// The config rule that decided; `None` for built-in classification.
    pub rule: Option<config::Match>,
    /// The context flags active where it asked (a rule's `[flags]`).
    pub flags: Flags,
}

/// Result of analyzing an AST node.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub action: Action,
    pub reason: String,
    pub context_flags: Option<Flags>,
    /// Env-stripped command for the audit log suggestion (ask only).
    pub suggestion: Option<String>,
    /// Parts that produced `action` (ask/deny); empty when unknown.
    pub causes: Vec<Cause>,
}

impl Decision {
    fn new(action: Action, reason: impl Into<String>) -> Self {
        Self {
            action,
            reason: reason.into(),
            context_flags: None,
            suggestion: None,
            causes: Vec::new(),
        }
    }
    /// An outer construct's decision for an inner one: same action and causes.
    fn wrap(inner: Decision, reason: String) -> Self {
        let mut d = Self::new(inner.action, reason);
        d.causes = inner.causes;
        d
    }
    fn cause(
        mut self,
        kind: &'static str,
        target: &str,
        rule: Option<&config::Match>,
        flags: &Flags,
    ) -> Self {
        self.causes.push(Cause {
            kind,
            target: target.to_string(),
            rule: rule.cloned(),
            flags: flags.clone(),
        });
        self
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
    /// A command ask: the suggestion is also the cause's target; call after
    /// `flags`.
    fn suggest(mut self, suggestion: &str) -> Self {
        self.suggestion = Some(suggestion.to_string());
        if self.causes.is_empty() {
            let flags = self.context_flags.clone().unwrap_or_default();
            self = self.cause("command", suggestion, None, &flags);
        }
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

/// Flags for a redirect: `@remote` tells askpass the target is on a server.
fn redirect_cause_flags(flags: &Flags, remote: bool) -> Flags {
    if remote {
        with(flags, &["@remote"])
    } else {
        flags.clone()
    }
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_numeric())
}

/// Whether a redirect operator can open its target for writing.
fn redirect_writes_file(op: &str) -> bool {
    FILE_WRITING_REDIRECTS.contains(&redirect_operator(op))
}

/// Whether a redirect operator opens its target for reading.
fn redirect_reads_file(op: &str) -> bool {
    matches!(redirect_operator(op), "<" | "<>")
}

/// The operator without its fd number or `{var}` prefix.
fn redirect_operator(op: &str) -> &str {
    if op.starts_with('{') {
        match op.find('}') {
            Some(end) => &op[end + 1..],
            None => op,
        }
    } else {
        op.trim_start_matches(|c: char| c.is_ascii_digit())
    }
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
            if let Some(n) = fd_target.strip_suffix('-')
                && is_digits(n)
                && n.parse::<u64>().ok() == Some(0)
            {
                return None;
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
            let is_cd = |p: &Node| matches!(p, Node::Command { words, .. } if words.first().is_some_and(|w| strip_quotes(&w.value) == "cd"));
            // One cd moves the cwd of the commands after it; with several
            // (`cd a || cd b`) the cwd of later commands is uncertain.
            if !remote && parts.iter().filter(|p| is_cd(p)).count() > 1 {
                return Decision::new(
                    Action::Deny,
                    "Multiple cd in one command are not allowed: run them as separate commands",
                )
                .flags(&lf);
            }
            let mut effective_cwd = cwd.to_path_buf();
            let mut decisions = Vec::new();
            for p in parts {
                if remote && is_cd(p) {
                    continue;
                }
                decisions.push(analyze_node(p, config, &effective_cwd, &lf, remote));
                if !remote && let Some(target) = extract_cd_target(p) {
                    effective_cwd = resolve_cd_target(&target, &effective_cwd);
                }
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
        let reason = format!("{label}: {}", inner.reason);
        return Decision::wrap(inner, reason);
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
        if let Some(cf) = &context_flag
            && let Some(c) = index_in(cf, 0, idx)
            && c + 1 < idx
        {
            context_value = Some(tokens[c + 1].clone());
        }
        let mut dest = None;
        if let Some(tf) = &target_flag {
            if let Some(t) = index_in(tf, 0, idx)
                && t + 1 < idx
            {
                dest = Some(tokens[t + 1].clone());
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
    if let Some(cf) = &context_flag
        && let Some(c) = index_in(cf, 0, tokens.len())
        && c + 1 < tokens.len()
    {
        context_value = Some(tokens[c + 1].clone());
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
                        let reason =
                            format!("process substitution {direction}(...): {}", inner.reason);
                        return Decision::wrap(inner, reason);
                    }
                    decisions.push(inner);
                }
                Node::CmdSub { command } => {
                    let inner = analyze_node(command, config, cwd, flags, remote);
                    if inner.action != Action::Allow {
                        let reason = format!("command substitution: {}", inner.reason);
                        return Decision::wrap(inner, reason);
                    }
                    decisions.push(inner);
                    if is_pure_cmdsub
                        && !simple_safe
                        && position > base_idx
                        && let Some(h) = handler
                    {
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
                            return Decision::ask(format!("cmdsub injection risk: {inner_cmd}"));
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
    let raw_args = raw_words.get(base_idx + 1..).unwrap_or_default();
    let inner = base_idx + unwrap_all_transparent_wrappers(&words[base_idx..], Some(config));
    let object_paths = words.get(inner).is_some_and(|w| w == "git")
        && cli::git::find_action(&words[inner..])
            .is_some_and(|(_, action)| matches!(action, "show" | "cat-file"));
    decisions.extend(read_arg_decision(
        &base,
        raw_args,
        object_paths,
        config,
        cwd,
        flags,
        remote,
    ));
    combine(decisions)
}

/// Read rules on the literal path arguments of a shell command: a matching
/// `deny-read` or `ask-read` decides, any other result leaves the command
/// decision alone. Candidates are non-option words, `--opt=value` and `-fVALUE`
/// values, and every word after `--`, plus the path of `rev:path` when
/// `object_paths` (`git show`, `git cat-file`). Best effort: words with
/// expansions or globs, and files created after the check, are not seen.
fn read_arg_decision(
    base: &str,
    raw_args: &[String],
    object_paths: bool,
    config: &Config,
    cwd: &Path,
    flags: &Flags,
    remote: bool,
) -> Option<Decision> {
    let mut ask = None;
    let mut after_dashes = false;
    for word in raw_args.iter().filter_map(|raw| literal_arg(raw, remote)) {
        let path = if after_dashes || !word.starts_with('-') {
            word.as_str()
        } else if word == "--" {
            after_dashes = true;
            continue;
        } else if word.starts_with("--") {
            match word.split_once('=') {
                Some((_, value)) => value,
                None => continue,
            }
        } else {
            word.char_indices().nth(2).map_or("", |(i, _)| &word[i..])
        };
        let rev_path = path
            .split_once(':')
            .filter(|(rev, _)| object_paths && !rev.contains('/'))
            .map(|(_, p)| p);
        let decisions = [
            read_file_decision(base, path, config, cwd, flags, remote),
            rev_path.and_then(|p| read_rule_decision(base, p, config, cwd, flags, remote)),
        ];
        for d in decisions.into_iter().flatten() {
            if d.action == Action::Deny {
                return Some(d);
            }
            ask.get_or_insert(d);
        }
    }
    ask
}

/// `read_rule_decision` for a path a command opens. A missing local file
/// cannot be read, so grep patterns and messages pass; the server's files are
/// unknown, so every remote word is checked.
fn read_file_decision(
    what: &str,
    path: &str,
    config: &Config,
    cwd: &Path,
    flags: &Flags,
    remote: bool,
) -> Option<Decision> {
    if !remote && !config::path_exists(path, cwd) {
        return None;
    }
    read_rule_decision(what, path, config, cwd, flags, remote)
}

/// The deny or ask of the read rule matching `path`; `None` otherwise.
fn read_rule_decision(
    what: &str,
    path: &str,
    config: &Config,
    cwd: &Path,
    flags: &Flags,
    remote: bool,
) -> Option<Decision> {
    if path.is_empty() {
        return None;
    }
    let cause_flags = redirect_cause_flags(flags, remote);
    let m = config::match_read(path, config, cwd, &cause_flags, remote)?;
    let action = match m.decision.as_str() {
        "deny" => Action::Deny,
        "ask" => Action::Ask,
        _ => return None,
    };
    let msg = m.message.clone().unwrap_or(m.pattern.clone());
    Some(
        Decision::new(action, format!("{what}: {msg}"))
            .flags(flags)
            .cause("read", path, Some(&m), &cause_flags),
    )
}

/// The value Bash passes for a literal word: quote removal plus expansion of
/// a leading unquoted `~` or `~/` (kept literal when remote). A quoted `~`
/// becomes `./~`, so path matching never expands it. `None` for other
/// expansions and globs.
fn literal_arg(raw: &str, remote: bool) -> Option<String> {
    if raw == "~" || raw.starts_with("~/") {
        let rest = decode_literal_word(&raw[1..], true)?;
        let home = if remote {
            "~".to_string()
        } else {
            paths::home().to_string_lossy().into_owned()
        };
        return Some(format!("{home}{rest}"));
    }
    let value = decode_literal_word(raw, true)?;
    Some(if value.starts_with('~') {
        format!("./{value}")
    } else {
        value
    })
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
                if redirect_target_is_safe(&target_value) {
                    continue;
                }
                if redirect_reads_file(op)
                    && let Some(path) = literal_arg(&target.value, remote)
                {
                    decisions.extend(read_file_decision(
                        "input redirect",
                        &path,
                        config,
                        cwd,
                        flags_ref,
                        remote,
                    ));
                }
                if redirect_writes_file(op) {
                    let what = if remote {
                        "remote redirect"
                    } else {
                        "redirect"
                    };
                    let flags_ref = &redirect_cause_flags(flags_ref, remote);
                    match config::match_redirect(&target_value, config, cwd, flags_ref, remote) {
                        Some(m) if m.decision == "allow" => {
                            decisions.push(Decision::allow(format!("{what} to {target_value}")))
                        }
                        Some(m) => {
                            let msg = m.message.clone().unwrap_or(m.pattern.clone());
                            let action = if m.decision == "deny" {
                                Action::Deny
                            } else {
                                Action::Ask
                            };
                            decisions.push(
                                Decision::new(action, format!("{what} to {target_value}: {msg}"))
                                    .cause("redirect", &target_value, Some(&m), flags_ref),
                            );
                        }
                        None => decisions.push(
                            Decision::ask(format!("{what} to {target_value}")).cause(
                                "redirect",
                                &target_value,
                                None,
                                flags_ref,
                            ),
                        ),
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
            Some(
                Decision::new(Action::Deny, format!("{base}: {msg}"))
                    .flags(flags)
                    .cause("command", suggestion, Some(m), flags),
            )
        }
        "ask" => {
            let msg = m.message.clone().unwrap_or(m.pattern.clone());
            Some(
                Decision::ask(format!("{base}: {msg}"))
                    .flags(flags)
                    .cause("command", suggestion, Some(m), flags)
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
    if let Some(var) = words[..i].iter().find_map(|w| config::code_env_name(w)) {
        let sc = SimpleCommand {
            words,
            raw_words,
            word_has_expansions: has_exp,
        };
        if let Some(m) = config::match_command(&sc, config, cwd, flags, remote) {
            let msg = m.message.as_deref().unwrap_or(&m.pattern);
            let action = match m.decision.as_str() {
                "allow" => Some(Action::Allow),
                "ask" => Some(Action::Ask),
                "deny" => Some(Action::Deny),
                _ => None,
            };
            if let Some(action) = action {
                return Decision::new(action, format!("{var}: {msg}"))
                    .flags(flags)
                    .cause("command", &words.join(" "), Some(&m), flags);
            }
        }
        return Decision::ask(format!("{var}= changes which code runs"))
            .flags(flags)
            .cause("command", &words.join(" "), None, flags);
    }
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
        if let Some(m) = config::match_command(&sc, config, cwd, flags, remote)
            && !is_catch_all_pattern(&m.pattern)
            && let Some(d) = config_match_decision(&m, base, flags, &suggestion)
        {
            return d;
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
    if let Some(m) = config::match_command(&sc, config, cwd, flags, remote)
        && let Some(d) = config_match_decision(&m, base, flags, &suggestion)
    {
        return d;
    }

    // 2. `dippy run` with one literal script on stdin.
    if base == "dippy" && tokens.len() > 1 && tokens[1] == "run" {
        if tokens.len() == 2
            && let Some(content) = &quoted_heredoc_content(redirects)
        {
            return analyze(content, config, cwd, Some(flags), remote);
        }
        return Decision::ask("dippy run: requires one non-empty quoted heredoc")
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
    if let Some(info) = config.wrappers.get(base)
        && info
            .subcommand
            .as_ref()
            .is_none_or(|s| tokens.get(1) == Some(s))
    {
        if info.transparent {
            return Decision::ask(base).flags(flags).suggest(&suggestion);
        }
        let values = shell_values(tokens, &slice_str(raw_words, i));
        let (dest, inner_cmd, mut context_value) = extract_wrapper_args(&values, Some(info));
        // `(marker, start, marker index)`: the script marker follows the
        // trigger; marker `-` is a quoted heredoc after the options when the
        // trigger is absent.
        let trigger_idx = info
            .trigger
            .as_ref()
            .and_then(|t| tokens.iter().position(|x| x == t));
        let script = info.script_stdin_marker.as_ref().and_then(|marker| {
            if marker == "-" {
                let start = usize::from(info.subcommand.is_some());
                return (trigger_idx.is_none() && quoted_heredoc_content(redirects).is_some())
                    .then_some((marker, start, tokens.len()));
            }
            let start = if info.trigger.is_some() {
                trigger_idx?
            } else {
                0
            };
            let p = tokens.get(start + 1..)?.iter().position(|x| x == marker)?;
            Some((marker, start, p + start + 1))
        });
        if let Some((_, _, marker_idx)) = script
            && context_value.is_none()
            && let Some(cf) = &info.context_flag
            && let Some(c) = values[..marker_idx].iter().position(|t| t == cf)
            && c + 1 < marker_idx
        {
            context_value = Some(values[c + 1].clone());
        }
        if inner_cmd.is_empty() && script.is_none() {
            let reason = match &dest {
                Some(d) if !d.is_empty() => format!("{base} {d}"),
                _ => base.to_string(),
            };
            return Decision::ask(reason).flags(flags).suggest(&suggestion);
        }
        let mut wrapper_context = vec![base.to_string()];
        if let Some(d) = dest.as_ref().filter(|d| !d.is_empty())
            && info.context_first
        {
            wrapper_context.push(d.clone());
        }
        if let Some(c) = context_value.as_ref().filter(|c| !c.is_empty()) {
            wrapper_context.push(c.clone());
        }
        let mut inner_flags = flags.clone();
        inner_flags.extend(wrapper_context);

        if let Some((marker, start, marker_idx)) = script {
            let content = quoted_heredoc_content(redirects);
            let has_expansions = !has_exp.is_empty()
                && (0..=marker_idx).any(|k| has_exp.get(i + k).copied().unwrap_or(false));
            let mut valid_options = true;
            let mut prev_was_flag = false;
            for opt in &tokens[start + 1..marker_idx] {
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
            let valid = (marker == "-" || marker_idx == tokens.len() - 1)
                && !has_expansions
                && valid_options
                && content.is_some();
            if !valid {
                let what = if marker == "-" {
                    tokens[..=start].join(" ")
                } else {
                    format!("{base} {marker}")
                };
                return Decision::ask(format!("{what}: requires one non-empty quoted heredoc"))
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
            let flags = &redirect_cause_flags(flags, remote);
            for target in targets {
                if redirect_target_is_safe(target) {
                    continue;
                }
                match config::match_redirect(target, config, cwd, flags, remote) {
                    Some(m) if m.decision == "deny" => {
                        let msg = m.message.clone().unwrap_or(m.pattern.clone());
                        return Decision::new(Action::Deny, format!("{desc}: {msg}")).cause(
                            "redirect",
                            target,
                            Some(&m),
                            flags,
                        );
                    }
                    Some(m) if m.decision == "ask" => {
                        let msg = m.message.clone().unwrap_or(m.pattern.clone());
                        return Decision::ask(format!("{desc}: {msg}")).cause(
                            "redirect",
                            target,
                            Some(&m),
                            flags,
                        );
                    }
                    Some(_) => {}
                    None => return Decision::ask(desc).cause("redirect", target, None, flags),
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
            let reason = format!("cmdsub: {}", inner.reason);
            decisions.push(Decision::wrap(inner, reason));
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
    let target = match words.len() {
        2 => &words[1],
        3 if word_value(&words[1]) == "--" => &words[2],
        _ => return None,
    };
    if word_value(&words[0]) != "cd" {
        return None;
    }
    if target.parts.iter().any(|p| {
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
    Some(word_value(target))
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
    let causes = |a: Action| {
        decisions
            .iter()
            .filter(|d| d.action == a)
            .flat_map(|d| d.causes.iter().cloned())
            .collect::<Vec<_>>()
    };
    let deny = reasons(Action::Deny);
    if !deny.is_empty() {
        let mut d = Decision::new(Action::Deny, deny.join(", ")).flags(&context_flags);
        d.causes = causes(Action::Deny);
        return d;
    }
    let ask = reasons(Action::Ask);
    if !ask.is_empty() {
        let suggestion = decisions
            .iter()
            .find(|d| d.action == Action::Ask && d.suggestion.is_some())
            .and_then(|d| d.suggestion.clone());
        let mut d = Decision::ask(ask.join(", ")).flags(&context_flags);
        d.suggestion = suggestion;
        d.causes = causes(Action::Ask);
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
        // Rable would drop everything after `$'` inside double quotes.
        assert_eq!(decide("echo \"x '$'\" > out.txt"), Action::Ask);
        assert_eq!(decide("mysql -e \"SELECT 'a$'\""), Action::Allow);
    }

    fn decide_with(rules: &str, cmd: &str) -> Decision {
        let config = config::parse_config(rules, None).unwrap();
        analyze(cmd, &config, Path::new("/tmp"), None, false)
    }

    #[test]
    fn remote_writes_ask_unless_a_flagged_rule_allows() {
        let base = "wrapper zorpwrap --cmd run --context -t --script-stdin --script\n\
                    ask [zorpwrap] *\n\
                    allow [zorpwrap] grep *\n\
                    delegate [zorpwrap] awk *\n\
                    allow-redirect /tmp/**\n\
                    allow-redirect ./**\n";
        let awk = "zorpwrap -t h run --script <<'R'\nawk '{print > \"/tmp/a\"}' f\nR";
        for cmd in [
            "zorpwrap -t h run \"grep x f > /etc/passwd\"",
            // A local allow-redirect never permits a server write.
            "zorpwrap -t h run \"grep x f > /tmp/a\"",
            "zorpwrap -t h run \"grep x f > out.txt\"",
            awk,
            "ssh h 'grep x f > /tmp/a'",
        ] {
            let d = decide_with(base, cmd);
            assert_eq!(d.action, Action::Ask, "{cmd}: {}", d.reason);
        }
        assert_eq!(
            decide_with(base, "zorpwrap -t h run \"grep x f > /dev/null 2>&1\"").action,
            Action::Allow
        );
        let d = decide_with(base, "zorpwrap -t h run \"grep x f > /tmp/a\"");
        assert!(d.causes[0].flags.contains("@remote"), "{:?}", d.causes);
        assert!(
            !decide_with(base, "grep x f > /etc/a").causes[0]
                .flags
                .contains("@remote")
        );

        let flagged =
            format!("{base}allow-redirect [zorpwrap] /tmp/**\ndeny-redirect /tmp/keep \"no\"\n");
        for cmd in ["zorpwrap -t h run \"grep x f > /tmp/a\"", awk] {
            assert_eq!(decide_with(&flagged, cmd).action, Action::Allow, "{cmd}");
        }
        for cmd in [
            "zorpwrap -t h run \"grep x f > /etc/passwd\"",
            "zorpwrap -t h run \"grep x f > /tmp/../etc/passwd\"",
        ] {
            assert_eq!(decide_with(&flagged, cmd).action, Action::Ask, "{cmd}");
        }
        assert_eq!(
            decide_with(&flagged, "zorpwrap -t h run \"grep x f > /tmp/keep\"").action,
            Action::Deny
        );
    }

    /// A fresh directory holding `files` (a trailing `/` makes a directory).
    fn fixture(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dippy-rs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            if let Some(d) = f.strip_suffix('/') {
                std::fs::create_dir_all(dir.join(d)).unwrap();
            } else {
                std::fs::write(dir.join(f), "").unwrap();
            }
        }
        dir.canonicalize().unwrap()
    }

    fn decide_in(dir: &Path, rules: &str, cmd: &str) -> Decision {
        let config = config::parse_config(rules, None).unwrap();
        analyze(cmd, &config, dir, None, false)
    }

    const READ_RULES: &str = "allow zorp *\n\
                              wrapper zorpwrap --cmd run --context -t\n\
                              allow [zorpwrap] cat *\n\
                              allow [zorpwrap] grep *\n\
                              deny-read **/.env* \"no env\"\n\
                              allow-read **/.env*.example*\n\
                              deny-read **/secrets/** \"no secrets\"\n\
                              deny-read **/*credentials* \"no credentials\"\n\
                              ask-read **/notes.txt \"check notes\"\n";

    #[test]
    fn read_rules_filter_existing_path_arguments() {
        let dir = fixture(
            "readarg",
            &[
                ".env",
                ".env.example",
                ".env x",
                ".envx",
                ".env;1",
                "notes.txt",
                "README.md",
                "secrets/",
                "secrets/key",
                "x/",
            ],
        );
        let abs = format!("cat {}/.env", dir.display());
        for cmd in [
            "cat .env",
            "cat '.env'",
            "cat .env\\ x",
            "cat \".env\"x",
            "cat .env\\;1",
            "cat -- .env",
            "head ./x/../.env",
            "head .//.env",
            abs.as_str(),
            "base64 .env",
            "zorp --file=.env",
            "zorp --file .env",
            "zorp -x.env",
            "tar czf out.tgz secrets/key",
            "(cd secrets && cat key)",
            "cd secrets; cat key",
            "timeout 5 cat .env",
            "bash -c 'cat .env'",
            "ls | zorp .env",
        ] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Deny, "{cmd}: {}", d.reason);
        }
        let d = decide_in(&dir, READ_RULES, "cat .env");
        assert!(d.reason.contains("no env"), "{}", d.reason);
        assert_eq!(d.causes[0].kind, "read", "{:?}", d.causes);
        let d = decide_in(&dir, READ_RULES, "zorp notes.txt");
        assert_eq!(d.action, Action::Ask, "{}", d.reason);
        assert!(d.reason.contains("check notes"), "{}", d.reason);
        for cmd in [
            "cat .env.example",
            "zorp README.md",
            "cat \"$F\"",
            // Globs and directory arguments (TASK-32) are not expanded.
            "cat .env*",
            "grep -r KEY .",
        ] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Allow, "{cmd}: {}", d.reason);
        }
        // No read rule: the command decision is unchanged.
        assert_eq!(
            decide_in(&dir, "allow zorp *", "zorp .env").action,
            Action::Allow
        );
        assert_eq!(decide_in(&dir, "", "zorp .env").action, Action::Ask);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_args_expand_only_an_unquoted_tilde() {
        let home = paths::home().to_string_lossy().into_owned();
        assert_eq!(literal_arg("~", false).as_deref(), Some(home.as_str()));
        assert_eq!(literal_arg("~/x", false), Some(format!("{home}/x")));
        assert_eq!(literal_arg("~/'a b'", false), Some(format!("{home}/a b")));
        // The server's home is unknown: the word stays literal.
        assert_eq!(literal_arg("~/x", true).as_deref(), Some("~/x"));
        for quoted in ["'~/x'", "\\~/x", "\"~\"/x"] {
            assert_eq!(
                literal_arg(quoted, false).as_deref(),
                Some("./~/x"),
                "{quoted}"
            );
        }
        assert_eq!(literal_arg("~user/x", false), None);
        assert_eq!(literal_arg("$HOME/x", false), None);
        let dir = fixture("readarg-tilde", &["~/", "~/.env"]);
        let d = decide_in(&dir, READ_RULES, "cat '~/.env'");
        assert_eq!(d.action, Action::Deny, "{}", d.reason);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_rules_skip_missing_local_files() {
        let dir = fixture("readarg-missing", &["src/"]);
        for cmd in [
            "cat .env",
            "zorp credentials src/",
            "zorp -n 'password|credentials' src",
            "git commit -m 'rotate credentials'",
            // Only `git show` reads `rev:path` from the object store.
            "zorp label:.env",
        ] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_ne!(d.action, Action::Deny, "{cmd}: {}", d.reason);
        }
        for cmd in [
            "git show HEAD:.env",
            "git show HEAD:./x/../.env",
            "git -C . show HEAD:.env",
            "git -c color.ui=false show HEAD:.env",
            "git --no-pager show HEAD:.env",
            "timeout 5 git show HEAD:.env",
            "command git show HEAD:.env",
            "git cat-file -p HEAD:.env",
        ] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Deny, "{cmd}: {}", d.reason);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_cd_moves_the_cwd_of_later_commands() {
        let dir = fixture("readarg-cd", &["secrets/", "secrets/key", "a/"]);
        for cmd in [
            "pwd; cd secrets; cat key",
            "cd -- secrets && cat key",
            "cd a && (cd ../secrets && cat key)",
        ] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Deny, "{cmd}: {}", d.reason);
        }
        // `key` before the cd is the starting directory's (missing) file.
        let d = decide_in(&dir, READ_RULES, "cat key; cd secrets");
        assert_ne!(d.action, Action::Deny, "{}", d.reason);
        let d = decide_in(&dir, READ_RULES, "cd a && cd ../secrets && cat key");
        assert_eq!(d.action, Action::Deny, "{}", d.reason);
        assert_eq!(
            d.reason,
            "Multiple cd in one command are not allowed: run them as separate commands"
        );
        let d = decide_in(&dir, READ_RULES, "cd a || cd b");
        assert_eq!(d.action, Action::Deny, "{}", d.reason);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_rules_filter_input_redirects_and_attached_values() {
        let dir = fixture("readarg-redirect", &[".env", "notes.txt", "README.md"]);
        for cmd in [
            "cat < .env",
            "cat 0< .env",
            "grep -f.env README.md",
            "grep -f .env README.md",
        ] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Deny, "{cmd}: {}", d.reason);
        }
        for cmd in ["cat < notes.txt", "zorp -fnotes.txt"] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Ask, "{cmd}: {}", d.reason);
        }
        for cmd in ["cat < README.md", "grep -rn x README.md", "zorp -x5"] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Allow, "{cmd}: {}", d.reason);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_rules_check_every_remote_word() {
        let dir = fixture("readarg-remote", &[]);
        for cmd in [
            "zorpwrap -t h run 'cat .env'",
            "zorpwrap -t h run 'grep credentials f'",
            // A local allow-read exception never permits a server read.
            "zorpwrap -t h run 'cat .env.example'",
        ] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Deny, "{cmd}: {}", d.reason);
        }
        let d = decide_in(&dir, READ_RULES, "zorpwrap -t h run 'cat README.md'");
        assert_eq!(d.action, Action::Allow, "{}", d.reason);
        let flagged = format!("{READ_RULES}allow-read [zorpwrap] /**\n");
        let d = decide_in(&dir, &flagged, "zorpwrap -t h run 'cat /etc/.env'");
        assert_eq!(d.action, Action::Allow, "{}", d.reason);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_rules_filter_symlink_arguments() {
        let dir = fixture("readarg-link", &["secrets/", "secrets/key", "secrets/sub/"]);
        std::os::unix::fs::symlink(dir.join(".env"), dir.join("dangling")).unwrap();
        std::os::unix::fs::symlink(dir.join("secrets/key"), dir.join("link")).unwrap();
        std::os::unix::fs::symlink(dir.join("secrets/sub"), dir.join("up")).unwrap();
        std::os::unix::fs::symlink(dir.join("cycle"), dir.join("cycle")).unwrap();
        // `..` after a directory link leaves the link's target, not `dir`.
        let abs_up = format!("cat {}/up/../key", dir.display());
        for cmd in ["cat dangling", "cat link", "cat up/../key", abs_up.as_str()] {
            let d = decide_in(&dir, READ_RULES, cmd);
            assert_eq!(d.action, Action::Deny, "{cmd}: {}", d.reason);
        }
        assert!(config::path_exists("dangling", &dir));
        assert!(!config::path_exists("missing", &dir));
        // A link cycle neither crashes nor matches anything else.
        let d = decide_in(&dir, READ_RULES, "cat cycle");
        assert_eq!(d.action, Action::Allow, "{}", d.reason);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrapper_subcommand_scopes_unwrapping() {
        let rules = "wrapper zorpcli --subcommand run --cmd -- --context -t --script-stdin -\n\
                     ask [zorpcli] *\n\
                     allow [zorpcli] grep *\n\
                     allow [zorpcli,gate2] zorpstat *\n\
                     allow zorpcli run --list\n";
        let script = |opts: &str, body: &str| format!("zorpcli run {opts} <<'R'\n{body}\nR");
        for cmd in [
            "zorpcli run --timeout 60 -t gate2 -- zorpstat x".to_string(),
            "zorpcli run -t gate2 -- 'grep x f | zorpstat'".to_string(),
            "zorpcli run -t other -- grep x f".to_string(),
            "zorpcli run --list".to_string(),
            script("--timeout 60 -t gate2", "grep x f\nzorpstat y"),
            // Without a server only rules without one apply.
            script("--all", "grep x f"),
            "zorpcli run --all -- grep x f".to_string(),
        ] {
            let d = decide_with(rules, &cmd);
            assert_eq!(d.action, Action::Allow, "{cmd}: {}", d.reason);
        }
        for cmd in [
            "zorpcli run -t other -- zorpstat x".to_string(),
            "zorpcli run -t gate2 -- 'grep x f; rm -rf /x'".to_string(),
            "zorpcli run --all -- zorpstat x".to_string(),
            script("-t other", "zorpstat y"),
            script("-t gate2", "grep x f\nrm -rf /x"),
            script("-t gate2 extra", "grep x f"),
            script("-t $SRV", "grep x f"),
            "zorpcli run -t gate2 <<R\ngrep x f\nR".to_string(),
            "zorpcli run -t gate2 zorpstat x".to_string(),
            "zorpcli run -t gate2".to_string(),
            // A positional SERVER is not the script form.
            "zorpcli run --create-file /etc/x gate2 <<'R'\ngrep x f\nR".to_string(),
            // Only the declared subcommand unwraps.
            "zorpcli exec -t gate2 -- grep x f".to_string(),
            "zorpcli -t gate2 run -- grep x f".to_string(),
            "zorpcli run --read gate2".to_string(),
        ] {
            let d = decide_with(rules, &cmd);
            assert_eq!(d.action, Action::Ask, "{cmd}: {}", d.reason);
        }
        let d = decide_with(rules, &script("-t gate2", "zorpstat y"));
        let flags = d.context_flags.unwrap_or_default();
        assert!(
            flags.contains("zorpcli") && flags.contains("gate2"),
            "{flags:?}"
        );
    }

    #[test]
    fn remote_path_words_collapse_dot_dot() {
        let rules = "wrapper zorpwrap --cmd run --context -t\n\
                     ask [zorpwrap] *\n\
                     allow [zorpwrap] zorprm -f /tmp/**+\n";
        let decide = |cmd| decide_with(rules, cmd).action;
        assert_eq!(
            decide("zorpwrap -t h run \"zorprm -f /tmp/a /tmp/b/c\""),
            Action::Allow
        );
        assert_eq!(
            decide("zorpwrap -t h run \"zorprm -f /tmp/../etc/passwd\""),
            Action::Ask
        );
        assert_eq!(
            decide("zorpwrap -t h run \"zorprm -f /tmp/a/../../etc\""),
            Action::Ask
        );
    }

    #[test]
    fn remote_command_in_double_quotes_is_decoded() {
        let base = "wrapper zorpwrap --cmd run --context -t\n\
                    ask [zorpwrap] *\n\
                    delegate [zorpwrap] awk *\n\
                    allow [zorpwrap] echo *\n";
        // The server runs awk '{print > "/etc/a"}' f, not the \" escapes.
        for cmd in [
            "zorpwrap -t h run \"awk '{print > \\\"/etc/a\\\"}' f\"",
            "ssh h \"awk '{print > \\\"/etc/a\\\"}' f\"",
        ] {
            let d = decide_with(base, cmd);
            assert_eq!(d.action, Action::Ask, "{cmd}: {}", d.reason);
            assert_eq!(d.reason, "awk redirect", "{cmd}");
        }
        // An expansion keeps the word as written.
        assert_eq!(
            decide_with(base, "zorpwrap -t h run \"echo $HOME\"").action,
            Action::Allow
        );
    }

    #[test]
    fn background_command_before_newline() {
        assert_eq!(decide("ls &\necho started"), Action::Allow);
        assert_eq!(decide("ls > /dev/null 2>&1 &\necho $!"), Action::Allow);
        assert_eq!(decide("ls &\n\necho a; echo b &\necho c"), Action::Allow);
        assert_eq!(decide("ls &\nrm -rf x"), Action::Ask);
        let d = decide_with("deny zorptool *", "ls &\nzorptool x");
        assert_eq!(d.action, Action::Deny);
        // The newline after `&` starts the here-document body.
        assert_eq!(decide("cat <<EOF &\nrm -rf x\nEOF\necho ok"), Action::Allow);
        assert_eq!(decide("cat <<EOF &\nbody\nEOF\nrm -rf x"), Action::Ask);
    }

    #[test]
    fn code_env_assignment_asks() {
        let cases = [
            ("LD_PRELOAD=/tmp/x.so git status", "LD_PRELOAD"),
            ("PATH=/tmp/evil ls", "PATH"),
            ("PYTHONPATH=src git status", "PYTHONPATH"),
            ("NODE_OPTIONS=--require=/tmp/x.js ls", "NODE_OPTIONS"),
            ("BASH_ENV=/tmp/evil ls", "BASH_ENV"),
            (
                "GIT_CONFIG_GLOBAL=/tmp/evil git status",
                "GIT_CONFIG_GLOBAL",
            ),
            ("GIT_CONFIG_KEY_0=core.pager git log", "GIT_CONFIG_KEY_0"),
            ("GIT_SSH_COMMAND=/tmp/evil git status", "GIT_SSH_COMMAND"),
            ("PAGER=/tmp/evil git log", "PAGER"),
            ("BASH_FUNC_ls%%=x ls", "BASH_FUNC_ls%%"),
            ("FOO=bar LD_PRELOAD=/tmp/x.so git status", "LD_PRELOAD"),
            ("env LD_PRELOAD=/tmp/x.so git status", "LD_PRELOAD"),
            ("env -i PATH=/tmp/evil ls", "PATH"),
            ("LD_PRELOAD=/tmp/x.so timeout 5 git status", "LD_PRELOAD"),
            ("PATH=/tmp/evil", "PATH"),
        ];
        for (cmd, var) in cases {
            let d = decide_with("", cmd);
            assert_eq!(d.action, Action::Ask, "{cmd}");
            assert!(d.reason.contains(var), "{cmd}: {}", d.reason);
        }
        assert_eq!(decide("FOO=bar env BAZ=1 git status"), Action::Allow);
    }

    #[test]
    fn code_env_assignment_rules() {
        let generic = decide_with("allow zorptool *", "LD_PRELOAD=/tmp/x.so zorptool run");
        assert_eq!(generic.action, Action::Ask);
        let spelled = decide_with(
            "allow PYTHONPATH=src zorptool *",
            "PYTHONPATH=src zorptool run",
        );
        assert_eq!(spelled.action, Action::Allow);
        let deny = decide_with("deny zorptool *", "LD_PRELOAD=/tmp/x.so zorptool run");
        assert_eq!(deny.action, Action::Deny);
    }

    /// Kind, target and the matching rule's pattern and line.
    type Summary = (&'static str, String, Option<(String, usize)>);

    fn causes(rules: &str, cmd: &str) -> Vec<Summary> {
        decide_with(rules, cmd)
            .causes
            .into_iter()
            .map(|c| {
                let rule = c.rule.map(|m| (m.pattern, m.line.unwrap_or(0)));
                (c.kind, c.target, rule)
            })
            .collect()
    }

    #[test]
    fn causes_name_each_asking_part() {
        assert_eq!(
            causes("", "zorp -x a; ls > /etc/foo"),
            [
                ("command", "zorp -x a".to_string(), None),
                ("redirect", "/etc/foo".to_string(), None),
            ]
        );
        // A command's asking redirect stops its analysis (as in Python).
        assert_eq!(
            causes("", "zorp -x a > /etc/foo"),
            [("redirect", "/etc/foo".to_string(), None)]
        );
        assert_eq!(
            causes("", "echo $(zorp x)"),
            [("command", "zorp x".to_string(), None)]
        );
        assert_eq!(
            causes("# c\nask frob *", "ls && FOO=1 frob x"),
            [(
                "command",
                "frob x".to_string(),
                Some(("frob *".to_string(), 2))
            )]
        );
        assert_eq!(
            causes("ask-redirect /etc/*", "ls > /etc/foo"),
            [(
                "redirect",
                "/etc/foo".to_string(),
                Some(("/etc/*".to_string(), 1))
            )]
        );
        assert!(causes("", "ls -la | grep x").is_empty());
    }

    #[test]
    fn causes_carry_the_flags_active_where_they_asked() {
        let flags = |cmd: &str, base: &[&str], remote: bool| -> Vec<Vec<String>> {
            let base: Flags = base.iter().map(|s| s.to_string()).collect();
            let config = Config::default();
            analyze(cmd, &config, Path::new("/"), Some(&base), remote)
                .causes
                .into_iter()
                .map(|c| c.flags.into_iter().collect())
                .collect()
        };
        assert_eq!(
            flags("zorp x", &["zorpwrap", "srv"], true),
            [["srv", "zorpwrap"]]
        );
        assert_eq!(flags("ssh host 'zorp x'", &[], false), [["host", "ssh"]]);
        assert_eq!(flags("ls > /etc/foo", &["srv"], false), [["srv"]]);
        assert_eq!(
            flags("zorp x | cat", &[], false),
            [["@compound", "@pipeline"]]
        );
    }

    #[test]
    fn causes_of_a_deny_are_only_the_denying_parts() {
        assert_eq!(
            causes("deny frob delete", "zorp a; frob delete"),
            [(
                "command",
                "frob delete".to_string(),
                Some(("frob delete".to_string(), 1))
            )]
        );
    }
}
