//! Port of `dippy.cli.hooks`: list, install and uninstall Dippy hooks in the
//! agents' JSON settings, and switch Gemini's approval mode.
//!
//! Intentional divergences from Python: a hook is Dippy's only when its
//! program is `dippy` or `dippy-hook` (Python matched any command containing
//! `/dippy`, so install removed unrelated hooks such as
//! `/home/u/dippy/scripts/x.sh`), the upgrade hint has no double space, and
//! a malformed config is an error instead of a traceback.

use std::collections::{BTreeSet, HashMap};
use std::fs::{File, FileTimes};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::admin::py_float_repr;
use crate::hook::py_json_str;
use crate::paths::py_path_str;

pub(crate) struct Agent {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// Relative to the home directory.
    pub(crate) global: &'static str,
    /// Relative to the working directory.
    pub(crate) project: &'static str,
}

/// `HOOK_COMMANDS` in its order, with the `AGENTS` display names.
pub(crate) const AGENTS: [Agent; 6] = [
    Agent {
        id: "claude",
        name: "Claude Code",
        global: ".claude/settings.json",
        project: ".claude/settings.json",
    },
    Agent {
        id: "gemini",
        name: "Gemini CLI",
        global: ".gemini/settings.json",
        project: ".gemini/settings.json",
    },
    Agent {
        id: "agy",
        name: "Antigravity CLI / AGY",
        global: ".gemini/config/hooks.json",
        project: ".agents/hooks.json",
    },
    Agent {
        id: "cursor",
        name: "Cursor IDE",
        global: ".cursor/hooks.json",
        project: ".cursor/hooks.json",
    },
    Agent {
        id: "windsurf",
        name: "Windsurf",
        global: ".windsurf/hooks.json",
        project: ".windsurf/hooks.json",
    },
    Agent {
        id: "codex",
        name: "OpenAI Codex CLI",
        global: ".codex/hooks.json",
        project: ".codex/hooks.json",
    },
];

const AGENT_IDS: [&str; 6] = ["claude", "gemini", "agy", "cursor", "windsurf", "codex"];

const MAX_BACKUPS: usize = 5;

#[derive(clap::Args)]
pub struct HooksArgs {
    #[command(subcommand)]
    action: Option<Action>,
}

#[derive(clap::Subcommand)]
enum Action {
    /// List hook status (shows both global and project)
    List {
        /// Show detailed information (matchers, commands, paths)
        #[arg(long)]
        verbose: bool,
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
        /// Minimal output, exit code only
        #[arg(long)]
        quiet: bool,
    },
    /// Install Dippy hooks for an agent
    Install(InstallArgs),
    /// Uninstall Dippy hooks for an agent
    Uninstall {
        /// Agent to uninstall hooks for
        #[arg(value_parser = AGENT_IDS)]
        agent: String,
        /// Uninstall from global config instead of project-local
        #[arg(long)]
        global: bool,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
    /// Enable Gemini YOLO mode for Pure Dippy Control
    SetupGeminiYolo {
        /// Update global settings.json instead of project-local
        #[arg(long)]
        global: bool,
        /// Disable YOLO mode (set back to 'default')
        #[arg(long)]
        disable: bool,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(clap::Args)]
pub(crate) struct InstallArgs {
    /// Agent to install hooks for; omit with --all to install for all agents
    #[arg(value_parser = AGENT_IDS)]
    pub(crate) agent: Option<String>,
    /// Install to global config instead of project-local
    #[arg(long)]
    pub(crate) global: bool,
    /// Replace existing/legacy hooks
    #[arg(long)]
    pub(crate) force: bool,
    /// Show what would be done without making changes
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Skip config backup before install
    #[arg(long)]
    pub(crate) no_backup: bool,
    /// Install ALL supported hooks (PreToolUse, PostToolUse, Notification, Stop, etc.)
    #[arg(long)]
    pub(crate) all: bool,
}

/// `handle_hooks_subcommand`; `cwd` is the global `--cwd`.
pub fn run(args: &HooksArgs, cwd: Option<&str>) -> i32 {
    match &args.action {
        None => {
            eprintln!("Error: Please specify an action (list, install, uninstall)");
            1
        }
        Some(Action::List {
            verbose,
            json,
            quiet,
        }) => list_hooks(cwd, *verbose, *json, *quiet),
        Some(Action::Install(a)) => install(a, cwd),
        Some(Action::Uninstall {
            agent,
            global,
            dry_run,
        }) => uninstall(agent_info(agent), *global, *dry_run, cwd),
        Some(Action::SetupGeminiYolo {
            global,
            disable,
            dry_run,
        }) => setup_gemini_yolo(*global, *disable, *dry_run, cwd),
    }
}

fn agent_info(id: &str) -> &'static Agent {
    AGENTS
        .iter()
        .find(|a| a.id == id)
        .expect("clap limits agent ids")
}

// ---------------------------------------------------------------- hook sets

fn entry(matcher: Option<&str>, hook: Value) -> Value {
    let mut e = Map::new();
    if let Some(m) = matcher {
        e.insert("matcher".into(), m.into());
    }
    e.insert("hooks".into(), json!([hook]));
    Value::Object(e)
}

/// `MINIMAL_HOOKS`, or `ALL_HOOKS` with `all`: the `hooks` object to insert.
fn hook_set(agent: &str, all: bool) -> Map<String, Value> {
    let command = format!("dippy --{agent}");
    let plain = json!({"type": "command", "command": command});
    let named = |name: &str| json!({"name": name, "type": "command", "command": command});
    let mut set = Map::new();
    let mut add = |kind: &str, value: Value| {
        set.insert(kind.into(), json!([value]));
    };
    match agent {
        "claude" => {
            let pre = "Bash|Write|Edit|MultiEdit|Read|LS|Glob|Grep|Search|WebSearch|mcp__.*";
            add("PreToolUse", entry(Some(pre), plain.clone()));
            add(
                "PostToolUse",
                entry(Some("Bash|WebSearch|mcp__.*"), plain.clone()),
            );
            if all {
                let idle = "notification_type==idle_prompt";
                add("Notification", entry(Some(idle), plain.clone()));
                for kind in ["Stop", "SubagentStop", "AfterAgent"] {
                    add(kind, entry(None, plain.clone()));
                }
            }
        }
        "gemini" => {
            let before = "run_shell_command|write_file|replace|read_file|google_web_search";
            add("BeforeTool", entry(Some(before), named("dippy-approval")));
            let after = "run_shell_command|google_web_search";
            add("AfterTool", entry(Some(after), named("dippy-after")));
        }
        "agy" => {
            let pre =
                "run_command|write_to_file|replace_file_content|view_file|search_web|call_mcp_tool";
            let mut approval = named("dippy-approval");
            approval["timeout"] = json!(1800);
            add("PreToolUse", entry(Some(pre), approval));
            let post = "run_command|search_web|call_mcp_tool";
            add("PostToolUse", entry(Some(post), named("dippy-after")));
            if all {
                add("Stop", entry(None, named("dippy-stop")));
            }
        }
        "codex" => {
            for kind in ["PreToolUse", "PermissionRequest", "PostToolUse"] {
                add(kind, entry(Some("^Bash$"), plain.clone()));
            }
            if all {
                add("Stop", entry(None, plain.clone()));
            }
        }
        // preToolUse, not beforeShellExecution: the latter ignores an
        // "allow" answer and prompts anyway.
        "cursor" => {
            add(
                "preToolUse",
                json!({"matcher": "Shell", "command": command}),
            );
            add("afterShellExecution", json!({"command": command}));
        }
        _ => {
            add("beforeShellExecution", json!({"command": command}));
            add("afterShellExecution", json!({"command": command}));
        }
    }
    set
}

// ---------------------------------------------------------------- detection

/// The lowercased program of a hook command.
fn program(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase()
}

fn basename(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or("")
}

fn is_dippy_command(command: &str) -> bool {
    matches!(basename(&program(command)), "dippy" | "dippy-hook")
}

/// `_is_dippy_hook`: an object whose `command` runs Dippy.
fn is_dippy_hook(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(is_dippy_command)
}

/// `_is_legacy_codex_run_hook`: an old flat `{"run": ["dippy", ...]}` entry.
fn is_legacy_codex_run(hook: &Value) -> bool {
    hook.get("run")
        .and_then(Value::as_array)
        .and_then(|run| run.first())
        .and_then(Value::as_str)
        .is_some_and(|first| basename(&first.to_lowercase()) == "dippy")
}

fn nested_has_dippy(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(is_dippy_hook))
}

/// `_has_dippy_hook`.
pub(crate) fn has_dippy_hook(config: &Map<String, Value>, agent: &str) -> bool {
    let found = |lists: Option<&Value>| {
        lists.and_then(Value::as_object).is_some_and(|lists| {
            lists
                .values()
                .filter_map(Value::as_array)
                .flatten()
                .any(|e| e.is_object() && (is_dippy_hook(e) || nested_has_dippy(e)))
        })
    };
    (agent == "agy" && found(config.get("dippy"))) || found(config.get("hooks"))
}

/// `_detect_legacy_hook_command`: the first Dippy command, in file order,
/// that is not the bare `dippy` program.
pub(crate) fn legacy_command(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => map.iter().find_map(|(key, v)| match v {
            Value::String(c)
                if key == "command" && is_dippy_command(c) && program(c) != "dippy" =>
            {
                Some(c.clone())
            }
            _ => legacy_command(v),
        }),
        Value::Array(items) => items.iter().find_map(legacy_command),
        _ => None,
    }
}

/// The object under `key`, `None` when absent, an error for another type.
fn object_at<'a>(
    config: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a Map<String, Value>>, String> {
    match config.get(key) {
        None => Ok(None),
        Some(Value::Object(m)) => Ok(Some(m)),
        Some(_) => Err(format!("\"{key}\" is not an object")),
    }
}

fn array<'a>(value: &'a Value, kind: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("hooks \"{kind}\" is not a list"))
}

/// `_get_installed_dippy_hook_types`.
fn installed_types(config: &Map<String, Value>, agent: &str) -> Result<BTreeSet<String>, String> {
    let mut kinds = BTreeSet::new();
    if agent == "agy" {
        if let Some(Value::Object(block)) = config.get("dippy") {
            for (kind, list) in block {
                let found = list.as_array().is_some_and(|l| {
                    l.iter()
                        .any(|e| e.is_object() && (is_dippy_hook(e) || nested_has_dippy(e)))
                });
                if found {
                    kinds.insert(kind.clone());
                }
            }
        }
        return Ok(kinds);
    }
    let Some(hooks) = object_at(config, "hooks")? else {
        return Ok(kinds);
    };
    for (kind, list) in hooks {
        for e in array(list, kind)? {
            let found = match agent {
                "cursor" | "windsurf" => is_dippy_hook(e),
                "codex" => e.is_object() && (nested_has_dippy(e) || is_legacy_codex_run(e)),
                _ => match e {
                    Value::Object(m) if m.contains_key("hooks") => nested_has_dippy(e),
                    Value::Object(_) => is_dippy_hook(e),
                    _ => return Err(format!("a \"{kind}\" entry is not an object")),
                },
            };
            if found {
                kinds.insert(kind.clone());
            }
        }
    }
    Ok(kinds)
}

// ---------------------------------------------------------------- editing

const CLAUDE_KINDS: [&str; 6] = [
    "PreToolUse",
    "PostToolUse",
    "Notification",
    "Stop",
    "SubagentStop",
    "AfterAgent",
];
const CODEX_KINDS: [&str; 4] = ["PreToolUse", "PermissionRequest", "PostToolUse", "Stop"];

fn not_object(kind: &str) -> String {
    format!("a \"{kind}\" entry is not an object")
}

/// `_remove_dippy_hook`: drop every Dippy hook, keep everything else.
fn remove_dippy_hook(
    config: &Map<String, Value>,
    agent: &str,
) -> Result<Map<String, Value>, String> {
    let mut result = config.clone();
    if agent == "agy" {
        result.shift_remove("dippy");
        if let Some(Value::Object(hooks)) = result.get_mut("hooks") {
            for (kind, list) in hooks.iter_mut() {
                let Value::Array(entries) = list else {
                    continue;
                };
                if entries.iter().any(|e| !e.is_object()) {
                    return Err(not_object(kind));
                }
                entries.retain(|e| !(is_dippy_hook(e) || nested_has_dippy(e)));
            }
            hooks.retain(|_, list| !matches!(list, Value::Array(a) if a.is_empty()));
            if hooks.is_empty() {
                result.shift_remove("hooks");
            }
        }
        return Ok(result);
    }
    if let Some(hooks) = result.get_mut("hooks") {
        let Value::Object(hooks) = hooks else {
            return Err("\"hooks\" is not an object".into());
        };
        match agent {
            "cursor" | "windsurf" => {
                for (kind, list) in hooks.iter_mut() {
                    let entries = list
                        .as_array_mut()
                        .ok_or_else(|| format!("hooks \"{kind}\" is not a list"))?;
                    entries.retain(|e| !is_dippy_hook(e));
                }
                hooks.retain(|_, list| !matches!(list, Value::Array(a) if a.is_empty()));
            }
            "codex" => {
                for kind in CODEX_KINDS {
                    let Some(list) = hooks.get_mut(kind) else {
                        continue;
                    };
                    let entries = list
                        .as_array_mut()
                        .ok_or_else(|| format!("hooks \"{kind}\" is not a list"))?;
                    let mut kept = Vec::new();
                    for mut e in entries.drain(..) {
                        if is_legacy_codex_run(&e) {
                            continue;
                        }
                        if let Some(nested) = e.get_mut("hooks") {
                            let nested = nested
                                .as_array_mut()
                                .ok_or_else(|| format!("hooks \"{kind}\" is not a list"))?;
                            nested.retain(|h| !is_dippy_hook(h));
                            if nested.is_empty() {
                                continue;
                            }
                        }
                        kept.push(e);
                    }
                    if kept.is_empty() {
                        hooks.shift_remove(kind);
                    } else {
                        *list = Value::Array(kept);
                    }
                }
            }
            _ => {
                for kind in CLAUDE_KINDS {
                    let Some(list) = hooks.get_mut(kind) else {
                        continue;
                    };
                    let entries = list
                        .as_array_mut()
                        .ok_or_else(|| format!("hooks \"{kind}\" is not a list"))?;
                    for e in entries.iter_mut() {
                        let Value::Object(e) = e else {
                            return Err(not_object(kind));
                        };
                        if let Some(nested) = e.get_mut("hooks") {
                            let nested = nested.as_array_mut().ok_or_else(|| {
                                format!("a \"{kind}\" entry's hooks is not a list")
                            })?;
                            nested.retain(|h| !is_dippy_hook(h));
                        }
                    }
                    // Python keeps only entries with non-empty hooks.
                    entries.retain(|e| truthy(e.get("hooks")));
                    if entries.is_empty() {
                        hooks.shift_remove(kind);
                    }
                }
            }
        }
        if hooks.is_empty() {
            result.shift_remove("hooks");
        }
    }
    Ok(result)
}

/// `_merge_hook_entry`: remove the old Dippy hooks, then append `set`.
fn merge_hook_entry(
    config: &Map<String, Value>,
    set: Map<String, Value>,
    agent: &str,
) -> Result<Map<String, Value>, String> {
    let mut result = remove_dippy_hook(config, agent)?;
    if agent == "agy" {
        result.insert("dippy".into(), Value::Object(set));
        return Ok(result);
    }
    let hooks = result
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(hooks) = hooks else {
        return Err("\"hooks\" is not an object".into());
    };
    for (kind, list) in set {
        let target = hooks
            .entry(kind.clone())
            .or_insert_with(|| Value::Array(Vec::new()));
        let Value::Array(target) = target else {
            return Err(format!("hooks \"{kind}\" is not a list"));
        };
        target.extend(list.as_array().cloned().unwrap_or_default());
    }
    if agent == "cursor" && !result.contains_key("version") {
        result.insert("version".into(), json!(1));
    }
    Ok(result)
}

/// Python truthiness of an optional JSON value.
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

// ---------------------------------------------------------------- JSON text

fn py_number(n: &serde_json::Number) -> String {
    if n.is_f64() {
        py_float_repr(n.as_f64().unwrap_or(0.0))
    } else {
        n.to_string()
    }
}

/// Python `json.dumps(value, indent=2, sort_keys=sort)`.
pub fn py_pretty(value: &Value, sort: bool) -> String {
    let mut out = String::new();
    write_pretty(value, sort, 0, &mut out);
    out
}

fn write_pretty(value: &Value, sort: bool, level: usize, out: &mut String) {
    let pad = |level: usize| "  ".repeat(level);
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&py_number(n)),
        Value::String(s) => out.push_str(&py_json_str(s)),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad(level + 1));
                write_pretty(item, sort, level + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            if sort {
                entries.sort_by(|a, b| a.0.cmp(b.0));
            }
            out.push_str("{\n");
            for (i, (key, item)) in entries.iter().enumerate() {
                out.push_str(&pad(level + 1));
                out.push_str(&py_json_str(key));
                out.push_str(": ");
                write_pretty(item, sort, level + 1, out);
                out.push_str(if i + 1 < entries.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push('}');
        }
    }
}

fn config_text(config: &Map<String, Value>) -> String {
    py_pretty(&Value::Object(config.clone()), true)
}

// ---------------------------------------------------------------- difflib

type Opcode = (&'static str, usize, usize, usize, usize);

/// `difflib.SequenceMatcher(None, a, b)` (autojunk on, no junk function).
struct SequenceMatcher<'a> {
    a: &'a [&'a str],
    b: &'a [&'a str],
    b2j: HashMap<&'a str, Vec<usize>>,
}

impl<'a> SequenceMatcher<'a> {
    fn new(a: &'a [&'a str], b: &'a [&'a str]) -> Self {
        let mut b2j: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, line) in b.iter().enumerate() {
            b2j.entry(line).or_default().push(i);
        }
        if b.len() >= 200 {
            let ntest = b.len() / 100 + 1;
            b2j.retain(|_, indices| indices.len() <= ntest);
        }
        SequenceMatcher { a, b, b2j }
    }

    fn find_longest_match(
        &self,
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
    ) -> (usize, usize, usize) {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut next = HashMap::new();
            for &j in self.b2j.get(self.a[i]).map(Vec::as_slice).unwrap_or(&[]) {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = if j > 0 {
                    j2len.get(&(j - 1)).copied().unwrap_or(0)
                } else {
                    0
                } + 1;
                next.insert(j, k);
                if k > bestsize {
                    (besti, bestj, bestsize) = (i + 1 - k, j + 1 - k, k);
                }
            }
            j2len = next;
        }
        while besti > alo && bestj > blo && self.a[besti - 1] == self.b[bestj - 1] {
            (besti, bestj, bestsize) = (besti - 1, bestj - 1, bestsize + 1);
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && self.a[besti + bestsize] == self.b[bestj + bestsize]
        {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    }

    fn matching_blocks(&self) -> Vec<(usize, usize, usize)> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0, la, 0, lb)];
        let mut blocks = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.find_longest_match(alo, ahi, blo, bhi);
            if k > 0 {
                blocks.push((i, j, k));
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        blocks.sort();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        let mut merged = Vec::new();
        for (i2, j2, k2) in blocks {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 > 0 {
                    merged.push((i1, j1, k1));
                }
                (i1, j1, k1) = (i2, j2, k2);
            }
        }
        if k1 > 0 {
            merged.push((i1, j1, k1));
        }
        merged.push((la, lb, 0));
        merged
    }

    fn opcodes(&self) -> Vec<Opcode> {
        let (mut i, mut j) = (0, 0);
        let mut answer = Vec::new();
        for (ai, bj, size) in self.matching_blocks() {
            let tag = if i < ai && j < bj {
                "replace"
            } else if i < ai {
                "delete"
            } else if j < bj {
                "insert"
            } else {
                ""
            };
            if !tag.is_empty() {
                answer.push((tag, i, ai, j, bj));
            }
            (i, j) = (ai + size, bj + size);
            if size > 0 {
                answer.push(("equal", ai, i, bj, j));
            }
        }
        answer
    }

    fn grouped_opcodes(&self, n: usize) -> Vec<Vec<Opcode>> {
        let mut codes = self.opcodes();
        if codes.is_empty() {
            codes.push(("equal", 0, 1, 0, 1));
        }
        if let Some(first) = codes.first_mut()
            && first.0 == "equal"
        {
            let (tag, i1, i2, j1, j2) = *first;
            *first = (
                tag,
                i1.max(i2.saturating_sub(n)),
                i2,
                j1.max(j2.saturating_sub(n)),
                j2,
            );
        }
        if let Some(last) = codes.last_mut()
            && last.0 == "equal"
        {
            let (tag, i1, i2, j1, j2) = *last;
            *last = (tag, i1, i2.min(i1 + n), j1, j2.min(j1 + n));
        }
        let mut groups = Vec::new();
        let mut group = Vec::new();
        for (tag, mut i1, i2, mut j1, j2) in codes {
            if tag == "equal" && i2 - i1 > 2 * n {
                group.push((tag, i1, i2.min(i1 + n), j1, j2.min(j1 + n)));
                groups.push(std::mem::take(&mut group));
                (i1, j1) = (i1.max(i2.saturating_sub(n)), j1.max(j2.saturating_sub(n)));
            }
            group.push((tag, i1, i2, j1, j2));
        }
        if !group.is_empty() && !(group.len() == 1 && group[0].0 == "equal") {
            groups.push(group);
        }
        groups
    }
}

fn format_range(start: usize, stop: usize) -> String {
    let length = stop - start;
    match length {
        1 => format!("{}", start + 1),
        0 => format!("{start},0"),
        _ => format!("{},{length}", start + 1),
    }
}

/// `"".join(difflib.unified_diff(a, b, fromfile, tofile, lineterm=""))`
/// over lines that keep their newlines.
fn unified_diff(a: &[&str], b: &[&str], from: &str, to: &str) -> String {
    let mut out = String::new();
    for (index, group) in SequenceMatcher::new(a, b)
        .grouped_opcodes(3)
        .iter()
        .enumerate()
    {
        if index == 0 {
            out.push_str(&format!("--- {from}+++ {to}"));
        }
        let (first, last) = (group[0], group[group.len() - 1]);
        out.push_str(&format!(
            "@@ -{} +{} @@",
            format_range(first.1, last.2),
            format_range(first.3, last.4)
        ));
        for &(tag, i1, i2, j1, j2) in group {
            if tag == "equal" {
                a[i1..i2]
                    .iter()
                    .for_each(|l| out.push_str(&format!(" {l}")));
                continue;
            }
            if tag != "insert" {
                a[i1..i2]
                    .iter()
                    .for_each(|l| out.push_str(&format!("-{l}")));
            }
            if tag != "delete" {
                b[j1..j2]
                    .iter()
                    .for_each(|l| out.push_str(&format!("+{l}")));
            }
        }
    }
    out
}

/// `_diff_configs`.
fn diff_configs(old: &Map<String, Value>, new: &Map<String, Value>, path: &str) -> String {
    let (old, new) = (config_text(old), config_text(new));
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    unified_diff(&a, &b, &format!("a/{path}"), &format!("b/{path}"))
}

// ---------------------------------------------------------------- files

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(std::env::home_dir)
        .unwrap_or_default()
}

fn base_dir(cwd: Option<&str>) -> PathBuf {
    match cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

fn config_path(agent: &Agent, global: bool, cwd: Option<&str>) -> PathBuf {
    if global {
        home().join(agent.global)
    } else {
        base_dir(cwd).join(agent.project)
    }
}

fn codex_toml_path(global: bool, cwd: Option<&str>) -> PathBuf {
    if global {
        home().join(".codex/config.toml")
    } else {
        base_dir(cwd).join(".codex/config.toml")
    }
}

pub(crate) enum LoadError {
    Read(String),
    Json(String),
    NotObject,
}

impl LoadError {
    fn describe(&self, shown: &str) -> String {
        match self {
            LoadError::Read(e) => format!("Could not read {shown}: {e}"),
            LoadError::Json(e) => format!("Invalid JSON in {shown}: {e}"),
            LoadError::NotObject => structure(shown, "top level is not an object"),
        }
    }
}

fn structure(shown: &str, detail: &str) -> String {
    format!("Unexpected hook structure in {shown}: {detail}")
}

pub(crate) fn load(path: &Path) -> Result<Map<String, Value>, LoadError> {
    let text = std::fs::read_to_string(path).map_err(|e| LoadError::Read(e.to_string()))?;
    match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(LoadError::NotObject),
        Err(e) => Err(LoadError::Json(e.to_string())),
    }
}

fn fail(message: String) -> i32 {
    eprintln!("Error: {message}");
    1
}

/// `shutil.copy2`: content, mode and timestamps.
fn copy2(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::copy(src, dst)?;
    let meta = std::fs::metadata(src)?;
    let times = FileTimes::new()
        .set_accessed(meta.accessed()?)
        .set_modified(meta.modified()?);
    File::open(dst)?.set_times(times)
}

/// `_create_backup`: copy next to the file, keep the newest `MAX_BACKUPS`.
fn create_backup(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let backup = path.with_file_name(format!("{name}.dippy-backup-{stamp}"));
    match copy2(path, &backup) {
        Ok(()) => {
            cleanup_old_backups(path, &name);
            Some(backup)
        }
        Err(e) => {
            eprintln!("Warning: Could not create backup: {e}");
            None
        }
    }
}

fn cleanup_old_backups(path: &Path, name: &str) {
    let prefix = format!("{name}.dippy-backup-");
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut backups: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|e| Some((std::fs::metadata(e.path()).ok()?.modified().ok()?, e.path())))
        .collect();
    // Stable, like Python's sorted(reverse=True): ties keep directory order.
    backups.sort_by_key(|b| std::cmp::Reverse(b.0));
    for (_, old) in backups.iter().skip(MAX_BACKUPS) {
        let _ = std::fs::remove_file(old);
    }
}

fn write_config(path: &Path, config: &Map<String, Value>, shown: &str) -> Result<(), String> {
    std::fs::write(path, config_text(config))
        .map_err(|e| format!("Could not write to {shown}: {e}"))
}

// ---------------------------------------------------------------- codex TOML

const LINE_BREAKS: [char; 10] = [
    '\n', '\r', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}', '\u{2029}',
];

/// Python `str.splitlines(keepends=True)`.
fn py_lines(s: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if !LINE_BREAKS.contains(&c) {
            continue;
        }
        let mut end = i + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|&(_, n)| n == '\n') {
            chars.next();
            end += 1;
        }
        lines.push(s[start..end].to_string());
        start = end;
    }
    if start < s.len() {
        lines.push(s[start..].to_string());
    }
    lines
}

fn ends_with_newline(line: &str) -> bool {
    line.ends_with(['\n', '\r'])
}

fn newline_of(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// `_codex_root_setting`: a quoted root value, ignoring tables such as
/// profiles.
pub(crate) fn codex_root_setting(path: &Path, key: &str) -> Option<String> {
    let content = read_text(path).ok()?;
    let pattern = Regex::new(&format!(
        r#"^\s*{}\s*=\s*["']([^"']+)["']"#,
        regex::escape(key)
    ))
    .expect("valid pattern");
    for line in py_lines(&content) {
        let line = line.trim_end_matches(LINE_BREAKS);
        if line.trim_start().starts_with('[') {
            break;
        }
        if let Some(m) = pattern.captures(line) {
            return Some(m[1].to_string());
        }
    }
    None
}

/// `_set_codex_root_setting`: set a root key before the first table.
fn set_codex_root_setting(content: &str, key: &str, value: &str) -> String {
    let newline = newline_of(content);
    let mut lines = py_lines(content);
    let pattern = Regex::new(&format!(
        r#"^(\s*{}\s*=\s*)["'][^"']*["'](\s*(?:#.*)?)(\r?\n)?$"#,
        regex::escape(key)
    ))
    .expect("valid pattern");
    let mut first_table = lines.len();
    for (index, line) in lines.iter_mut().enumerate() {
        if line.trim_start().starts_with('[') {
            first_table = index;
            break;
        }
        if let Some(c) = pattern.captures(line) {
            let ending = c.get(3).map_or("", |m| m.as_str());
            *line = format!("{}\"{value}\"{}{ending}", &c[1], &c[2]);
            return lines.concat();
        }
    }
    let setting = format!("{key} = \"{value}\"{newline}");
    if lines.is_empty() {
        return setting;
    }
    if first_table == 0 {
        return format!("{setting}{newline}{content}");
    }
    if !ends_with_newline(&lines[first_table - 1]) {
        lines[first_table - 1].push_str(newline);
    }
    lines.insert(first_table, setting);
    lines.concat()
}

/// `_set_codex_hooks_feature`: `hooks = true` in `[features]`.
fn set_codex_hooks_feature(content: &str) -> String {
    let newline = newline_of(content);
    let mut lines = py_lines(content);
    let mut section = None;
    let mut section_end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        let stripped = line.trim();
        if stripped == "[features]" {
            section = Some(index);
            continue;
        }
        if section.is_some() && stripped.starts_with('[') {
            section_end = index;
            break;
        }
    }
    if let Some(start) = section {
        if !ends_with_newline(&lines[start]) {
            lines[start].push_str(newline);
        }
        let pattern =
            Regex::new(r"^(\s*)(?:hooks|codex_hooks)\s*=\s*(?:true|false)(\s*(?:#.*)?)(\r?\n)?$")
                .expect("valid pattern");
        for line in &mut lines[start + 1..section_end] {
            if let Some(c) = pattern.captures(line) {
                let ending = c.get(3).map_or("", |m| m.as_str());
                *line = format!("{}hooks = true{}{ending}", &c[1], &c[2]);
                return lines.concat();
            }
        }
        lines.insert(start + 1, format!("hooks = true{newline}"));
        return lines.concat();
    }
    let mut prefix = content.to_string();
    if !prefix.is_empty() && !ends_with_newline(&prefix) {
        prefix.push_str(newline);
    }
    if !prefix.is_empty() && !prefix.ends_with(&newline.repeat(2)) {
        prefix.push_str(newline);
    }
    format!("{prefix}[features]{newline}hooks = true{newline}")
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// `_prepare_codex_config`: the updated `config.toml` and whether it changed.
fn prepare_codex_config(path: &Path) -> Result<(String, bool), String> {
    let content = if path.exists() {
        read_text(path)?
    } else {
        String::new()
    };
    let updated = set_codex_root_setting(&content, "approval_policy", "on-request");
    let updated = set_codex_hooks_feature(&updated);
    if let Err(e) = toml::from_str::<toml::Table>(&updated) {
        return Err(format!(
            "Invalid Codex TOML at {}: {}",
            py_path_str(path),
            e.message()
        ));
    }
    let changed = updated != content;
    Ok((updated, changed))
}

/// `_ensure_codex_config`.
fn ensure_codex_config(
    path: &Path,
    dry_run: bool,
    no_backup: bool,
) -> Result<(bool, String), String> {
    let shown = py_path_str(path);
    let (content, modified) = prepare_codex_config(path)?;
    if !modified {
        return Ok((false, "Codex config already compatible".into()));
    }
    if dry_run {
        return Ok((
            true,
            format!("Would set approval_policy = \"on-request\" and hooks = true in {shown}"),
        ));
    }
    if path.exists() && !no_backup {
        create_backup(path);
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, content).map_err(|e| e.to_string())?;
    Ok((
        true,
        format!("Configured Codex approval policy and hooks in {shown}"),
    ))
}

/// `_codex_feature_flag_enabled`.
pub(crate) fn codex_feature_flag_enabled(path: &Path) -> bool {
    let Ok(content) = read_text(path) else {
        return false;
    };
    let pattern =
        Regex::new(r"^\s*(?:hooks|codex_hooks)\s*=\s*true\s*(?:#.*)?$").expect("valid pattern");
    let mut in_features = false;
    for line in py_lines(&content) {
        let line = line.trim_end_matches(LINE_BREAKS);
        let stripped = line.trim();
        if stripped.starts_with('[') {
            in_features = stripped == "[features]";
            continue;
        }
        if in_features && pattern.is_match(line) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------- install

pub(crate) fn install(a: &InstallArgs, cwd: Option<&str>) -> i32 {
    match &a.agent {
        Some(id) => install_one(agent_info(id), a, cwd),
        None if !a.all => fail("agent is required unless --all is specified".into()),
        None => AGENTS
            .iter()
            .map(|agent| install_one(agent, a, cwd))
            .fold(0, |code, result| if result != 0 { result } else { code }),
    }
}

fn print_already(agent: &Agent, shown: &str) {
    println!("Dippy hook already installed for {}", agent.name);
    println!("Config: {shown}");
}

/// `install` for one agent.
fn install_one(agent: &Agent, a: &InstallArgs, cwd: Option<&str>) -> i32 {
    let path = config_path(agent, a.global, cwd);
    let shown = py_path_str(&path);
    let parent = path.parent().unwrap_or(Path::new("."));
    if agent.id == "codex" && parent.exists() && !parent.is_dir() {
        eprintln!(
            "Error: Expected Codex config directory at {}, but found a file.",
            py_path_str(parent)
        );
        eprintln!("Remove or rename that file so Dippy can create .codex/hooks.json.");
        return 1;
    }
    if !parent.exists() {
        if a.global {
            println!(
                "Error: Agent config directory not found: {}",
                py_path_str(parent)
            );
            println!("  {} may not be installed.", agent.name);
            println!("  Run: dippy hooks install {} --global", agent.id);
            return 1;
        }
        if let Err(e) = std::fs::create_dir_all(parent) {
            return fail(format!("Could not create {}: {e}", py_path_str(parent)));
        }
    }
    let existing = if path.exists() {
        match load(&path) {
            Ok(config) => config,
            Err(e) => return fail(e.describe(&shown)),
        }
    } else {
        Map::new()
    };

    let set = hook_set(agent.id, a.all);
    let has_hook = has_dippy_hook(&existing, agent.id);
    let legacy = if has_hook {
        legacy_command(&Value::Object(existing.clone()))
    } else {
        None
    };
    let requested: BTreeSet<String> = set.keys().cloned().collect();
    let installed = match installed_types(&existing, agent.id) {
        Ok(kinds) => kinds,
        Err(e) => return fail(structure(&shown, &e)),
    };
    let toml_path = codex_toml_path(a.global, cwd);
    let mut codex_needs_update = false;
    if agent.id == "codex" {
        match prepare_codex_config(&toml_path) {
            Ok((_, changed)) => codex_needs_update = changed,
            Err(e) => return fail(e),
        }
    }

    if has_hook && !a.force {
        if let Some(command) = &legacy {
            println!("Legacy Dippy hook detected for {}", agent.name);
            println!("Config: {shown}");
            println!("Current command: {command}");
            println!("Expected command: dippy --{}", agent.id);
            let scope = if a.global { " --global" } else { "" };
            println!(
                "To upgrade, run: dippy hooks install {}{scope} --force",
                agent.id
            );
            return 0;
        }
        if requested.is_subset(&installed) {
            if requested == installed && !codex_needs_update {
                print_already(agent, &shown);
                return 0;
            }
            if requested != installed {
                // Requesting a subset of installed hooks requires --force.
                print_already(agent, &shown);
                println!("Use --force to replace existing hooks");
                return 0;
            }
        }
    }

    let updated = match merge_hook_entry(&existing, set, agent.id) {
        Ok(config) => config,
        Err(e) => return fail(structure(&shown, &e)),
    };
    let modified = updated != existing;

    if a.dry_run {
        if modified {
            println!("Would update: {shown}");
        }
        if let Some(command) = &legacy {
            println!("\nRemoving legacy hook:");
            println!("  - {command}");
        }
        println!("\nAdding hooks:");
        print_hook_summary(agent.id, &updated);
        println!();
        print_diff(&existing, &updated, &shown, "Diff:");
        if agent.id == "codex"
            && let Ok((true, message)) = ensure_codex_config(&toml_path, true, a.no_backup)
        {
            println!("\n{message}");
        }
        return 0;
    }

    if modified {
        let backup = if !a.no_backup && path.exists() {
            create_backup(&path)
        } else {
            None
        };
        if let Err(e) = write_config(&path, &updated, &shown) {
            return fail(e);
        }
        let verb = if legacy.is_some() {
            "Upgraded"
        } else {
            "Installed"
        };
        println!("{verb} Dippy hook for {}", agent.name);
        println!("Config: {shown}");
        if let Some(backup) = backup {
            println!("Backup: {}", py_path_str(&backup));
        }
        println!();
        print_hook_summary(agent.id, &updated);
    }

    if agent.id == "codex" {
        match ensure_codex_config(&toml_path, false, a.no_backup) {
            Ok((_, message)) => println!("\n{message}"),
            Err(e) => return fail(e),
        }
    }
    0
}

fn print_diff(old: &Map<String, Value>, new: &Map<String, Value>, shown: &str, title: &str) {
    let diff = diff_configs(old, new, shown);
    if diff.is_empty() {
        println!("(no changes)");
    } else {
        println!("{title}");
        println!("{}", "=".repeat(60));
        println!("{diff}");
    }
}

pub(crate) fn py_str(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_string)
}

/// `_count_tools_in_matcher`.
fn count_tools(matcher: &str) -> String {
    let parts: Vec<&str> = matcher.split('|').collect();
    let mcp = parts.iter().filter(|p| p.contains("mcp__")).count();
    let other = parts.len() - mcp;
    if mcp > 0 {
        format!("{other}+MCP")
    } else {
        other.to_string()
    }
}

/// `_print_hook_summary`.
fn print_hook_summary(agent: &str, config: &Map<String, Value>) {
    let kinds: &[&str] = match agent {
        "claude" => &CLAUDE_KINDS,
        "gemini" => &["BeforeTool", "AfterTool"],
        "agy" => &["PreToolUse", "PostToolUse", "Stop"],
        "codex" => &CODEX_KINDS,
        _ => &["preToolUse", "beforeShellExecution", "afterShellExecution"],
    };
    let data = config.get(if agent == "agy" { "dippy" } else { "hooks" });
    for kind in kinds {
        let Some(entries) = data.and_then(|d| d.get(kind)).and_then(Value::as_array) else {
            continue;
        };
        for e in entries.iter().filter(|e| e.is_object()) {
            let dippy = if e.get("hooks").is_some() {
                nested_has_dippy(e)
            } else {
                is_dippy_hook(e)
            };
            if !dippy {
                continue;
            }
            if let Some(matcher) = e.get("matcher") {
                let matcher = py_str(matcher);
                if agent == "codex" {
                    println!("  + {kind}: {matcher}");
                } else {
                    let head: String = matcher.chars().take(60).collect();
                    println!("  + {kind}: {head}... ({} tools)", count_tools(&matcher));
                }
            } else if let Some(command) = e.get("command") {
                println!("  + {kind}: {}", py_str(command));
            } else if e.get("run").is_some() {
                println!("  + {kind}: legacy flat Codex entry");
            } else {
                println!("  + {kind}: (all)");
            }
        }
    }
    println!("\nCommand: dippy --{agent}");
}

// ---------------------------------------------------------------- uninstall

fn uninstall(agent: &Agent, global: bool, dry_run: bool, cwd: Option<&str>) -> i32 {
    let path = config_path(agent, global, cwd);
    let shown = py_path_str(&path);
    if !path.exists() {
        println!("Config file not found: {shown}");
        return 0;
    }
    let existing = match load(&path) {
        Ok(config) => config,
        Err(e) => return fail(e.describe(&shown)),
    };
    if !has_dippy_hook(&existing, agent.id) {
        println!("Dippy hook not found for {}", agent.name);
        return 0;
    }
    let updated = match remove_dippy_hook(&existing, agent.id) {
        Ok(config) => config,
        Err(e) => return fail(structure(&shown, &e)),
    };
    if dry_run {
        println!("Would update: {shown}");
        print_diff(&existing, &updated, &shown, "\nDiff:");
        return 0;
    }
    if let Err(e) = write_config(&path, &updated, &shown) {
        return fail(e);
    }
    println!("Uninstalled Dippy hook for {}", agent.name);
    println!("Config: {shown}");
    0
}

// ---------------------------------------------------------------- gemini yolo

fn setup_gemini_yolo(global: bool, disable: bool, dry_run: bool, cwd: Option<&str>) -> i32 {
    let path = config_path(agent_info("gemini"), global, cwd);
    let shown = py_path_str(&path);
    let target = if disable { "default" } else { "yolo" };
    let existing = if !path.exists() {
        if disable {
            println!("No Gemini configuration found at {shown}");
            return 0;
        }
        Map::new()
    } else {
        match load(&path) {
            Ok(config) => config,
            Err(LoadError::Read(e) | LoadError::Json(e)) => {
                return fail(format!("Could not read {shown}: {e}"));
            }
            Err(e) => return fail(e.describe(&shown)),
        }
    };

    let mut current = existing.get("approvalMode");
    if !truthy(current) && existing.contains_key("policyEngineConfig") {
        match object_at(&existing, "policyEngineConfig") {
            Ok(policy) => current = policy.and_then(|p| p.get("approvalMode")),
            Err(e) => return fail(structure(&shown, &e)),
        }
    }
    if current.and_then(Value::as_str) == Some(target) {
        println!("Gemini approval mode is already '{target}' in {shown}");
        return 0;
    }
    if dry_run {
        println!("Would set Gemini approval mode to '{target}' in {shown}");
        return 0;
    }

    let mut updated = existing.clone();
    updated.insert("approvalMode".into(), target.into());
    if let Some(Value::Object(policy)) = updated.get_mut("policyEngineConfig")
        && policy.shift_remove("approvalMode").is_some()
        && policy.is_empty()
    {
        updated.shift_remove("policyEngineConfig");
    }
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .map_err(|e| format!("Could not write to {shown}: {e}"))
        .and_then(|()| write_config(&path, &updated, &shown));
    if let Err(e) = written {
        return fail(e);
    }
    let action = if disable { "Disabled" } else { "Enabled" };
    println!("{action} Gemini YOLO mode (Pure Dippy Control) in {shown}");
    0
}

// ---------------------------------------------------------------- list

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Installed,
    Legacy,
    NotInstalled,
    NoConfig,
    Error,
}

impl Status {
    fn value(self) -> &'static str {
        match self {
            Status::Installed => "installed",
            Status::Legacy => "legacy",
            Status::NotInstalled => "not_installed",
            Status::NoConfig => "no_config",
            Status::Error => "error",
        }
    }

    fn display(self) -> &'static str {
        match self {
            Status::Installed => "installed",
            Status::Legacy => "legacy",
            Status::Error => "error",
            Status::NotInstalled | Status::NoConfig => "-",
        }
    }

    fn hooked(self) -> bool {
        matches!(self, Status::Installed | Status::Legacy)
    }
}

/// One scope (global or project) of `HookInfo`.
struct ScopeInfo {
    status: Status,
    path: String,
    command: String,
    legacy: String,
    matchers: Vec<String>,
    feature_flag: Option<bool>,
    feature_flag_path: Option<String>,
}

fn scope_info(path: &Path, agent: &str, missing: Status) -> ScopeInfo {
    let mut info = ScopeInfo {
        status: Status::NotInstalled,
        path: py_path_str(path),
        command: String::new(),
        legacy: String::new(),
        matchers: Vec::new(),
        feature_flag: None,
        feature_flag_path: None,
    };
    if !path.exists() {
        info.status = missing;
        return info;
    }
    let Ok(config) = load(path) else {
        info.status = Status::Error;
        return info;
    };
    if has_dippy_hook(&config, agent) {
        match legacy_command(&Value::Object(config.clone())) {
            Some(legacy) => {
                info.status = Status::Legacy;
                info.command = legacy.clone();
                info.legacy = legacy;
            }
            None => {
                info.status = Status::Installed;
                info.command = format!("dippy --{agent}");
            }
        }
        info.matchers = extract_matchers(&config, agent);
    }
    info
}

/// `_extract_matchers_from_config`.
fn extract_matchers(config: &Map<String, Value>, agent: &str) -> Vec<String> {
    let (block, kinds): (&str, &[&str]) = match agent {
        "claude" => ("hooks", &CLAUDE_KINDS),
        "gemini" => ("hooks", &["BeforeTool", "AfterTool"]),
        "agy" => ("dippy", &["PreToolUse", "PostToolUse", "Stop"]),
        "codex" => ("hooks", &CODEX_KINDS),
        _ => return vec!["(all shell commands)".into()],
    };
    let mut matchers = Vec::new();
    let Some(Value::Object(lists)) = config.get(block) else {
        return matchers;
    };
    for kind in kinds {
        let Some(entries) = lists.get(*kind).and_then(Value::as_array) else {
            continue;
        };
        for e in entries.iter().filter(|e| e.is_object()) {
            let dippy = match agent {
                "agy" => is_dippy_hook(e) || nested_has_dippy(e),
                _ => nested_has_dippy(e),
            };
            if dippy {
                match e.get("matcher") {
                    Some(m) => matchers.push(format!("{kind}: {}", py_str(m))),
                    None => matchers.push(format!("{kind}: (all)")),
                }
            } else if agent == "codex" && is_legacy_codex_run(e) {
                matchers.push(format!("{kind}: legacy flat entry"));
            }
        }
    }
    matchers
}

struct HookInfo {
    agent: &'static Agent,
    global: ScopeInfo,
    project: ScopeInfo,
}

impl HookInfo {
    fn has_any_hook(&self) -> bool {
        self.global.status.hooked() || self.project.status.hooked()
    }

    fn has_legacy(&self) -> bool {
        self.global.status == Status::Legacy || self.project.status == Status::Legacy
    }
}

fn hook_info(agent: &'static Agent, cwd: &Path) -> HookInfo {
    let mut global = scope_info(&home().join(agent.global), agent.id, Status::NoConfig);
    let mut project = scope_info(&cwd.join(agent.project), agent.id, Status::NotInstalled);
    if agent.id == "codex" {
        let global_toml = home().join(".codex/config.toml");
        let project_toml = cwd.join(".codex/config.toml");
        global.feature_flag = Some(codex_feature_flag_enabled(&global_toml));
        global.feature_flag_path = Some(py_path_str(&global_toml));
        project.feature_flag = Some(codex_feature_flag_enabled(&project_toml));
        project.feature_flag_path = Some(py_path_str(&project_toml));
    }
    HookInfo {
        agent,
        global,
        project,
    }
}

/// `list_hooks`.
fn list_hooks(cwd: Option<&str>, verbose: bool, json: bool, quiet: bool) -> i32 {
    if quiet {
        return 0;
    }
    let cwd = base_dir(cwd);
    let infos: Vec<HookInfo> = AGENTS.iter().map(|agent| hook_info(agent, &cwd)).collect();
    let pi_extension = home().join(".pi/agent/extensions/dippy-extension.ts");
    if json {
        println!("{}", json_output(&infos, &pi_extension));
    } else {
        text_output(&infos, &pi_extension, verbose);
    }
    0
}

fn flag_status(flag: Option<bool>) -> &'static str {
    match flag {
        Some(true) => "enabled",
        Some(false) => "missing",
        None => "-",
    }
}

fn text_output(infos: &[HookInfo], pi_extension: &Path, verbose: bool) {
    println!("Dippy Hook Status");
    println!("{}", "=".repeat(60));
    println!();
    for info in infos {
        let codex = info.agent.id == "codex";
        let flag_missing = codex
            && ((info.has_any_hook() && info.global.feature_flag == Some(false))
                || (info.project.status == Status::Installed
                    && info.project.feature_flag == Some(false)));
        let indicator = if info.has_any_hook() && !info.has_legacy() && !flag_missing {
            "+"
        } else if info.has_legacy() || flag_missing {
            "?"
        } else {
            " "
        };
        let status = |scope: &ScopeInfo| {
            let mut s = scope.status.display().to_string();
            if verbose && !scope.command.is_empty() {
                s.push_str(&format!(" ({})", scope.command));
            }
            s
        };
        println!("[{indicator}] {}", info.agent.name);
        println!(
            "    global:  {:<20} {}",
            status(&info.global),
            info.global.path
        );
        println!(
            "    project: {:<20} {}",
            status(&info.project),
            info.project.path
        );
        let id = info.agent.id;
        match info.global.status {
            Status::Legacy => println!("    Run: dippy hooks install {id} --global --force"),
            Status::NoConfig => println!("    Run: dippy hooks install {id} --global"),
            _ if codex && info.global.feature_flag == Some(false) => {
                println!("    Run: dippy hooks install {id} --global --force")
            }
            _ => {}
        }
        if verbose {
            if !info.global.matchers.is_empty() {
                println!("    Matchers:");
                for m in &info.global.matchers {
                    println!("      - {m}");
                }
            }
            if !info.global.legacy.is_empty() {
                println!("    Legacy command: {}", info.global.legacy);
            }
        }
        if codex {
            let path = |scope: &ScopeInfo| scope.feature_flag_path.clone().unwrap_or_default();
            println!(
                "    global feature:  {:<20} {}",
                flag_status(info.global.feature_flag),
                path(&info.global)
            );
            println!(
                "    project feature: {:<20} {}",
                flag_status(info.project.feature_flag),
                path(&info.project)
            );
        }
        println!();
    }
    let shown = py_path_str(pi_extension);
    if pi_extension.exists() {
        println!("[+] pi-mono: extension installed");
        println!("    {shown}");
    } else {
        println!("[ ] pi-mono: extension not found");
        println!("    Expected: {shown}");
    }
}

fn json_output(infos: &[HookInfo], pi_extension: &Path) -> String {
    let non_empty = |s: &str| if s.is_empty() { Value::Null } else { s.into() };
    let scope = |s: &ScopeInfo| {
        json!({
            "status": s.status.value(),
            "path": s.path,
            "command": non_empty(&s.command),
            "legacy_command": non_empty(&s.legacy),
            "matchers": s.matchers,
            "feature_flag_enabled": s.feature_flag,
            "feature_flag_path": s.feature_flag_path,
        })
    };
    let agents: Vec<Value> = infos
        .iter()
        .map(|info| {
            json!({
                "id": info.agent.id,
                "name": info.agent.name,
                "global": scope(&info.global),
                "project": scope(&info.project),
            })
        })
        .collect();
    let output = json!({
        "agents": agents,
        "pi_mono": {
            "installed": pi_extension.exists(),
            "path": py_path_str(pi_extension),
        },
    });
    py_pretty(&output, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn dippy_hook_is_recognized_by_its_program() {
        for command in [
            "dippy --claude",
            "dippy",
            "/usr/local/bin/dippy --claude",
            "DIPPY --claude",
            "dippy-hook",
            "/opt/x/dippy-hook --gemini",
            "C:\\tools\\dippy --claude",
        ] {
            assert!(is_dippy_command(command), "{command}");
        }
        for command in [
            "/home/u/work/dippy/scripts/check.sh",
            "/home/u/dippy-tools/run",
            "memorix --hook",
            "python3 -m dippy.dippy",
            "",
        ] {
            assert!(!is_dippy_command(command), "{command}");
        }
    }

    #[test]
    fn install_keeps_unrelated_hook_under_a_dippy_directory() {
        let foreign = json!({"type": "command", "command": "/home/u/dippy/scripts/check.sh"});
        let config =
            obj(json!({"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [foreign]}]}}));
        assert!(!has_dippy_hook(&config, "claude"));
        assert_eq!(legacy_command(&Value::Object(config.clone())), None);
        let merged = merge_hook_entry(&config, hook_set("claude", false), "claude").unwrap();
        let pre = merged["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["hooks"][0], foreign);
        let removed = remove_dippy_hook(&merged, "claude").unwrap();
        assert_eq!(removed, config);
    }

    #[test]
    fn legacy_command_is_a_dippy_program_other_than_bare_dippy() {
        let config = json!({"hooks": {"PreToolUse": [{"hooks": [
            {"command": "dippy --claude"},
            {"command": "/usr/bin/dippy-hook --claude"},
        ]}]}});
        assert_eq!(
            legacy_command(&config).as_deref(),
            Some("/usr/bin/dippy-hook --claude")
        );
    }

    #[test]
    fn malformed_hooks_are_an_error() {
        let config = obj(json!({"hooks": {"PreToolUse": ["text"]}}));
        assert!(remove_dippy_hook(&config, "claude").is_err());
        let config = obj(json!({"hooks": []}));
        assert!(merge_hook_entry(&config, hook_set("cursor", false), "cursor").is_err());
    }

    #[test]
    fn pretty_json_matches_python() {
        let value = json!({"b": [1, 2.5, 1e20, {}], "a": {"é": "\u{2028}"}, "c": []});
        assert_eq!(
            py_pretty(&value, true),
            "{\n  \"a\": {\n    \"\\u00e9\": \"\\u2028\"\n  },\n  \"b\": [\n    1,\n    2.5,\n    1e+20,\n    {}\n  ],\n  \"c\": []\n}"
        );
    }

    #[test]
    fn unified_diff_matches_python() {
        // difflib.unified_diff(a, b, "a/x", "b/x", lineterm="") joined by "".
        let a_lines: Vec<String> = (1..15).map(|i| format!("{i}\n")).collect();
        let mut b_lines = a_lines.clone();
        b_lines[2] = "X\n".into();
        b_lines.push("15\n".into());
        let a: Vec<&str> = a_lines.iter().map(String::as_str).collect();
        let b: Vec<&str> = b_lines.iter().map(String::as_str).collect();
        assert_eq!(
            unified_diff(&a, &b, "a/x", "b/x"),
            "--- a/x+++ b/x@@ -1,6 +1,6 @@ 1\n 2\n-3\n+X\n 4\n 5\n 6\n@@ -12,3 +12,4 @@ 12\n 13\n 14\n+15\n"
        );
        assert_eq!(
            unified_diff(&a[..9], &b[..9], "a/x", "b/x"),
            "--- a/x+++ b/x@@ -1,6 +1,6 @@ 1\n 2\n-3\n+X\n 4\n 5\n 6\n"
        );
        assert_eq!(unified_diff(&a, &a, "a/x", "b/x"), "");
    }

    #[test]
    fn codex_toml_edits_keep_comments_and_newlines() {
        assert_eq!(
            set_codex_hooks_feature(&set_codex_root_setting("", "approval_policy", "on-request")),
            "approval_policy = \"on-request\"\n\n[features]\nhooks = true\n"
        );
        let crlf = "approval_policy = 'never' # keep\r\n[features]\r\ncodex_hooks = false\r\n";
        assert_eq!(
            set_codex_hooks_feature(&set_codex_root_setting(
                crlf,
                "approval_policy",
                "on-request"
            )),
            "approval_policy = \"on-request\" # keep\r\n[features]\r\nhooks = true\r\n"
        );
        let table_first = "[profiles.x]\napproval_policy = \"never\"\n";
        assert_eq!(
            set_codex_root_setting(table_first, "approval_policy", "on-request"),
            "approval_policy = \"on-request\"\n\n[profiles.x]\napproval_policy = \"never\"\n"
        );
    }
}
