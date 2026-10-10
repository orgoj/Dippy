//! Port of `dippy.cli.doctor`: installation, hook, config and log checks.
//!
//! Intentional divergences from Python: `--agent moltbot` is gone; a legacy
//! hook is one `hooks::legacy_command` finds (Python matched `dippy-hook` or
//! `/dippy` anywhere in the file); the pi-mono check shows no
//! `pi_wrapper.py` bridge, which dippy-rs does not use; Codex matchers come
//! from `matcher` (Python read a `matchers.tool_name` that Codex never
//! writes, so it showed none); `--agent` with only a project config works
//! (Python raised `UnboundLocalError`); a hook config that is not a JSON
//! object counts as missing (Python raised `AttributeError`);
//! `dippy --version` has no timeout.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};

use crate::config;
use crate::hooks::{self, AGENTS, Agent, InstallArgs};
use crate::paths::{self, py_path_str};

/// `--agent` choices of the argparse parser, without `moltbot`.
const AGENT_IDS: [&str; 8] = [
    "claude", "gemini", "agy", "cursor", "windsurf", "pi", "codex", "pearai",
];

/// The `AGENTS` entries that have no hook command.
const OTHER_AGENTS: [Agent; 2] = [
    Agent {
        id: "pi",
        name: "pi-mono",
        global: ".pi/config.json",
        project: ".pi/hooks.json",
    },
    Agent {
        id: "pearai",
        name: "PearAI",
        global: ".pearai/hooks.json",
        project: ".pearai/hooks.json",
    },
];

const NO_YOLO: &str = "Pure Dippy Control not active (YOLO mode disabled)";

#[derive(clap::Args)]
pub struct DoctorArgs {
    /// Show diagnostics for a specific agent
    #[arg(long, value_name = "AGENT", value_parser = AGENT_IDS)]
    agent: Option<String>,
    /// Show detailed diagnostic information
    #[arg(long)]
    verbose: bool,
    /// Output as structured JSON
    #[arg(long)]
    json: bool,
    /// Minimal output, exit code only
    #[arg(long)]
    quiet: bool,
    /// Auto-repair common issues
    #[arg(long)]
    fix: bool,
}

/// `HealthStatus`; the discriminant is the exit code.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Ok = 0,
    Warning = 1,
    Critical = 2,
}

impl Status {
    fn level(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warning => "warning",
            Status::Critical => "critical",
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Status::Ok => "+",
            Status::Warning => "?",
            Status::Critical => "!",
        }
    }
}

/// `CheckResult`; matchers keep the order of their hook types.
struct Check {
    name: String,
    status: Status,
    message: String,
    details: Option<String>,
    fix_command: Option<String>,
    matchers: Vec<(String, Vec<Value>)>,
}

impl Check {
    fn new(name: &str, status: Status, message: &str, details: Option<String>) -> Self {
        Check {
            name: name.to_string(),
            status,
            message: message.to_string(),
            details,
            fix_command: None,
            matchers: Vec::new(),
        }
    }

    fn fix(mut self, command: &str) -> Self {
        self.fix_command = Some(command.to_string());
        self
    }

    fn with_matchers(mut self, matchers: Vec<(String, Vec<Value>)>) -> Self {
        self.matchers = matchers;
        self
    }

    fn to_json(&self) -> Value {
        let matchers: Map<String, Value> = self
            .matchers
            .iter()
            .map(|(kind, patterns)| (kind.clone(), Value::Array(patterns.clone())))
            .collect();
        json!({
            "name": self.name,
            "status": self.status.level(),
            "message": self.message,
            "details": self.details,
            "fix_command": self.fix_command,
            "matchers": matchers,
        })
    }

    fn display(&self, verbose: bool) {
        println!("[{}] {}: {}", self.status.symbol(), self.name, self.message);
        if let Some(details) = self.details.as_ref().filter(|_| verbose) {
            for line in details.split('\n') {
                println!("    {line}");
            }
        }
        if let Some(fix) = &self.fix_command {
            println!("    Fix: {fix}");
        }
        if verbose {
            for (kind, patterns) in &self.matchers {
                println!("    {kind}:");
                for pattern in patterns {
                    println!("      - {}", hooks::py_str(pattern));
                }
            }
        }
    }
}

/// `run`; `cwd` is the global `--cwd`.
pub fn run(args: &DoctorArgs, cwd: Option<&str>) -> i32 {
    let cwd = match cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let home = paths::home();
    let mut checks = all_checks(args, &home, &cwd);
    if args.fix && apply_fixes(&checks, &cwd) {
        checks = all_checks(args, &home, &cwd);
    }
    let worst = checks.iter().map(|c| c.status).max().unwrap_or(Status::Ok);
    if args.quiet {
        return worst as i32;
    }
    let count = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let (ok, warnings, critical) = (
        count(Status::Ok),
        count(Status::Warning),
        count(Status::Critical),
    );
    if args.json {
        let report = json!({
            "summary": {"ok": ok, "warnings": warnings, "critical": critical},
            "checks": checks.iter().map(Check::to_json).collect::<Vec<_>>(),
            "overall_status": worst.level(),
        });
        println!("{}", hooks::py_pretty(&report, false));
    } else {
        println!("Dippy Installation Check");
        println!("{}", "=".repeat(40));
        println!();
        for check in &checks {
            check.display(args.verbose);
            println!();
        }
        let mut parts = Vec::new();
        if ok > 0 {
            parts.push(format!("{ok} OK"));
        }
        if warnings > 0 {
            parts.push(format!("{warnings} warnings"));
        }
        if critical > 0 {
            parts.push(format!("{critical} critical"));
        }
        let summary = if parts.is_empty() {
            "No checks".to_string()
        } else {
            parts.join(", ")
        };
        println!("Summary: {summary}");
    }
    worst as i32
}

fn all_checks(args: &DoctorArgs, home: &Path, cwd: &Path) -> Vec<Check> {
    let mut checks = vec![check_installation()];
    checks.extend(check_hook_status(home, cwd, args.verbose));
    checks.push(check_config_validation(cwd));
    checks.push(check_log_health(home, args.verbose));
    if let Some(agent) = &args.agent {
        checks.push(check_agent_specific(agent, home, cwd, args.verbose));
    }
    checks
}

/// `apply_auto_fixes`: run the `dippy hooks install` fix of each warning.
fn apply_fixes(checks: &[Check], cwd: &Path) -> bool {
    let mut fixed = false;
    for check in checks.iter().filter(|c| c.status == Status::Warning) {
        let Some(command) = check
            .fix_command
            .as_deref()
            .filter(|c| c.contains("dippy hooks install"))
        else {
            continue;
        };
        let parts: Vec<&str> = command.split_whitespace().collect();
        let Some(agent) = parts
            .iter()
            .position(|p| *p == "install")
            .and_then(|i| parts.get(i + 1))
        else {
            continue;
        };
        println!("Auto-fixing: {}", check.name);
        let install = InstallArgs {
            agent: Some(agent.to_string()),
            global: parts.contains(&"--global"),
            force: parts.contains(&"--force"),
            dry_run: false,
            no_backup: false,
            all: false,
        };
        if hooks::install(&install, Some(&py_path_str(cwd))) == 0 {
            fixed = true;
            println!("  Fixed: {}", check.name);
        } else {
            eprintln!("  Failed to fix: {}", check.name);
        }
    }
    fixed
}

/// `shutil.which("dippy")`.
fn which_dippy() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("dippy"))
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// `check_installation`.
fn check_installation() -> Check {
    const NAME: &str = "Installation";
    let Some(dippy) = which_dippy() else {
        return Check::new(
            NAME,
            Status::Critical,
            "dippy not found on PATH",
            Some("Install Dippy: uv tool install dippy or pip install dippy".into()),
        )
        .fix("uv tool install dippy");
    };
    match Command::new(&dippy).arg("--version").output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let version = stdout.trim().split('\n').next().unwrap_or("");
            Check::new(
                NAME,
                Status::Ok,
                &format!("Dippy installed ({version})"),
                Some(format!("Location: {}", dippy.display())),
            )
        }
        Ok(out) => Check::new(
            NAME,
            Status::Warning,
            "dippy found but not executable",
            Some(format!(
                "Return code: {}",
                out.status.code().map_or("None".into(), |c| c.to_string())
            )),
        ),
        Err(e) => Check::new(
            NAME,
            Status::Warning,
            &format!("dippy check failed: {e}"),
            Some(e.to_string()),
        ),
    }
}

/// `check_hook_status`: one result per agent, Codex approval policies and
/// the pi extension.
fn check_hook_status(home: &Path, cwd: &Path, verbose: bool) -> Vec<Check> {
    let mut results = Vec::new();
    let check_project = cwd != home;
    let global_toml = home.join(".codex/config.toml");
    let project_toml = cwd.join(".codex/config.toml");
    for agent in &AGENTS {
        let name = format!("Hook: {}", agent.name);
        let codex = agent.id == "codex";
        let global_path = home.join(agent.global);
        let project_path = cwd.join(agent.project);
        let global = hooks::load(&global_path).ok();
        let project = check_project
            .then(|| hooks::load(&project_path).ok())
            .flatten();
        let global_flag = codex && hooks::codex_feature_flag_enabled(&global_toml);
        let project_flag =
            codex && check_project && hooks::codex_feature_flag_enabled(&project_toml);
        let exists = global.is_some() || project.is_some() || global_flag || project_flag;
        if !exists {
            let mut paths = vec![format!("  {}", py_path_str(&global_path))];
            if check_project {
                paths.push(format!("  {}", py_path_str(&project_path)));
            }
            let details = format!("Config not found at:\n{}", paths.join("\n"));
            results.push(
                Check::new(&name, Status::Warning, "Not installed", Some(details))
                    .fix(&format!("dippy hooks install {} --global", agent.id)),
            );
            continue;
        }

        let hooked = |c: &Option<Map<String, Value>>| {
            c.as_ref()
                .is_some_and(|c| hooks::has_dippy_hook(c, agent.id))
        };
        let (global_hooked, project_hooked) = (hooked(&global), hooked(&project));
        let mut locations = Vec::new();
        let mut legacy = None;
        let mut matchers = Vec::new();
        for (scope, is_hooked, config) in [
            ("global", global_hooked, &global),
            ("project", project_hooked, &project),
        ] {
            let Some(config) = config.as_ref().filter(|_| is_hooked) else {
                continue;
            };
            locations.push(scope);
            if legacy.is_none() {
                legacy = hooks::legacy_command(&Value::Object(config.clone()));
            }
            if verbose && matchers.is_empty() {
                matchers = extract_matchers(config, agent.id);
            }
        }
        let shown = locations.join(", ");
        let global_flag_missing = global_hooked && codex && !global_flag;
        let project_flag_missing = project_hooked && codex && !project_flag;

        if locations.is_empty() {
            let install = format!("dippy hooks install {} --global", agent.id);
            results.push(
                Check::new(
                    &name,
                    Status::Warning,
                    "Agent present, hook not installed",
                    Some(format!("Install with: {install}")),
                )
                .fix(&install),
            );
        } else if let Some(legacy) = legacy {
            let details = format!(
                "Legacy command: {legacy}\nExpected command: dippy --{}",
                agent.id
            );
            results.push(
                Check::new(
                    &name,
                    Status::Warning,
                    &format!("Legacy hook ({shown})"),
                    Some(details),
                )
                .fix(&format!(
                    "dippy hooks install {} --global --force",
                    agent.id
                ))
                .with_matchers(matchers),
            );
        } else if global_flag_missing || project_flag_missing {
            let missing: Vec<&str> = [
                (global_flag_missing, "global"),
                (project_flag_missing, "project"),
            ]
            .iter()
            .filter(|(m, _)| *m)
            .map(|(_, s)| *s)
            .collect();
            results.push(
                Check::new(
                    &name,
                    Status::Warning,
                    &format!(
                        "Hook installed, Codex feature flag missing ({})",
                        missing.join(", ")
                    ),
                    Some("Run install again to enable hooks in config.toml".into()),
                )
                .fix(&format!(
                    "dippy hooks install {} --global --force",
                    agent.id
                ))
                .with_matchers(matchers),
            );
        } else {
            let details = verbose.then(|| {
                let config = if global_hooked {
                    &global_path
                } else {
                    &project_path
                };
                format!("Location: {shown}\nConfig: {}", py_path_str(config))
            });
            results.push(
                Check::new(&name, Status::Ok, &format!("Installed ({shown})"), details)
                    .with_matchers(matchers),
            );
        }

        if codex && !locations.is_empty() {
            let global_policy = hooks::codex_root_setting(&global_toml, "approval_policy");
            if global_hooked {
                results.push(policy_check(
                    "Codex approval policy (global)",
                    global_policy.as_deref(),
                    "Dippy PermissionRequest enforcement requires approval_policy = \"on-request\".",
                    "dippy hooks install codex --global",
                ));
            }
            if project_hooked {
                let effective =
                    hooks::codex_root_setting(&project_toml, "approval_policy").or(global_policy);
                results.push(policy_check(
                    "Codex approval policy (project)",
                    effective.as_deref(),
                    "Project policy overrides global policy when configured.",
                    "dippy hooks install codex",
                ));
            }
        }
    }
    results.push(check_pi_extension(home));
    results
}

fn policy_check(name: &str, policy: Option<&str>, details: &str, fix: &str) -> Check {
    if policy == Some("on-request") {
        return Check::new(
            name,
            Status::Ok,
            "Compatible (on-request)",
            Some(details.into()),
        );
    }
    let message = format!("Incompatible ({})", policy.unwrap_or("not configured"));
    Check::new(name, Status::Warning, &message, Some(details.into())).fix(fix)
}

fn check_pi_extension(home: &Path) -> Check {
    const NAME: &str = "Hook: pi-mono";
    let extension = home.join(".pi/agent/extensions/dippy-extension.ts");
    let shown = py_path_str(&extension);
    if !extension.exists() {
        return Check::new(
            NAME,
            Status::Warning,
            "Extension not found",
            Some(format!("Expected: {shown}")),
        );
    }
    let symlink = extension.is_symlink();
    let details = match extension.canonicalize() {
        Ok(target) if symlink => format!("{shown} -> {}", py_path_str(&target)),
        _ => shown,
    };
    let kind = if symlink { "symlink" } else { "file" };
    Check::new(
        NAME,
        Status::Ok,
        &format!("Extension installed ({kind})"),
        Some(details),
    )
}

/// `_extract_matchers_from_config`: every `matcher` of the agent's hook
/// types, Dippy's or not.
fn extract_matchers(config: &Map<String, Value>, agent: &str) -> Vec<(String, Vec<Value>)> {
    let kinds: &[&str] = match agent {
        "claude" => &["PreToolUse", "PostToolUse"],
        "gemini" => &["BeforeTool", "AfterTool"],
        "codex" => &["PreToolUse", "PermissionRequest", "PostToolUse", "Stop"],
        _ => return Vec::new(),
    };
    let Some(lists) = config.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    kinds
        .iter()
        .filter_map(|kind| {
            let entries = lists.get(*kind)?.as_array()?;
            let found: Vec<Value> = entries
                .iter()
                .filter_map(|e| e.get("matcher").cloned())
                .collect();
            (!found.is_empty()).then(|| (kind.to_string(), found))
        })
        .collect()
}

/// `check_config_validation`.
fn check_config_validation(cwd: &Path) -> Check {
    const NAME: &str = "Configuration";
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut found = Vec::new();
    let global = config::user_config_path();
    if global.exists() {
        found.push("global");
        let shown = py_path_str(&global);
        if let Err(e) = config::load_config(cwd, Some(&shown), None) {
            errors.push(format!("Global config: {shown}: {e}"));
        }
        config::print_warnings();
    } else {
        warnings.push("Global config not found (~/.dippy/config)".to_string());
    }
    let project = cwd.join(".dippy");
    if project.exists() {
        found.push("project");
        if let Err(e) = config::load_config(cwd, None, None) {
            errors.push(format!("Project config: {}: {e}", py_path_str(&project)));
        }
        config::print_warnings();
    }
    if !errors.is_empty() {
        let message = format!("{} error(s) found", errors.len());
        return Check::new(NAME, Status::Critical, &message, Some(errors.join("\n")));
    }
    if !warnings.is_empty() {
        let mut details = warnings.join("\n");
        if !found.is_empty() {
            details.push_str(&format!("\nValid configs: {}", found.join(", ")));
        }
        let message = format!("{} warning(s)", warnings.len());
        return Check::new(NAME, Status::Warning, &message, Some(details));
    }
    let message = format!("Configuration valid ({})", found.join(", "));
    Check::new(NAME, Status::Ok, &message, None)
}

/// `check_log_health`.
fn check_log_health(home: &Path, verbose: bool) -> Check {
    const NAME: &str = "Logs";
    let logs = [
        (".claude/hook-approvals.log", "Claude Code"),
        (".gemini/hook-approvals.log", "Gemini CLI"),
        (".codex/hook-approvals.log", "OpenAI Codex CLI"),
        (".dippy/audit.log", "Dippy audit"),
    ];
    let mut issues = Vec::new();
    let mut writable = Vec::new();
    let mut sizes = Vec::new();
    for (rel, name) in logs {
        let log = home.join(rel);
        let dir = log.parent().unwrap_or(home);
        if dir.exists() {
            let probe = dir.join(".dippy_write_test");
            let written = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&probe)
                .and_then(|_| std::fs::remove_file(&probe));
            let shown = py_path_str(dir);
            match written {
                Ok(()) => writable.push(name),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    issues.push(format!("{name}: log directory not writable ({shown})"));
                }
                Err(_) => issues.push(format!("{name}: cannot write to log directory ({shown})")),
            }
        }
        if let Ok(meta) = log.metadata() {
            let size_mb = meta.len() as f64 / (1024.0 * 1024.0);
            if size_mb > 10.0 {
                issues.push(format!(
                    "{name}: log file is {size_mb:.1}MB (consider rotation)"
                ));
                sizes.push(format!("{name}: {size_mb:.1}MB"));
            } else if verbose {
                sizes.push(format!("{name}: {size_mb:.2}MB"));
            }
        }
    }
    if !issues.is_empty() {
        let message = format!("{} issue(s) detected", issues.len());
        return Check::new(NAME, Status::Warning, &message, Some(issues.join("\n")));
    }
    if writable.is_empty() {
        return Check::new(
            NAME,
            Status::Ok,
            "Log files not created yet",
            Some("This is normal for new installations".into()),
        );
    }
    let details = verbose.then(|| {
        let mut d = format!("Writable: {}", writable.join(", "));
        if !sizes.is_empty() {
            d.push_str(&format!("\nSizes: {}", sizes.join(", ")));
        }
        d
    });
    Check::new(NAME, Status::Ok, "Log directories are writable", details)
}

/// Gemini's `approvalMode`, top level first, then under
/// `policyEngineConfig`.
fn approval_mode(config: &Map<String, Value>) -> Option<Value> {
    let top = config.get("approvalMode");
    if hooks::truthy(top) {
        return top.cloned();
    }
    config
        .get("policyEngineConfig")
        .and_then(|p| p.get("approvalMode"))
        .filter(|m| hooks::truthy(Some(m)))
        .cloned()
}

/// `check_agent_specific`.
fn check_agent_specific(id: &str, home: &Path, cwd: &Path, verbose: bool) -> Check {
    let agent = AGENTS
        .iter()
        .chain(&OTHER_AGENTS)
        .find(|a| a.id == id)
        .expect("clap restricts --agent to known agents");
    let has_hooks = AGENTS.iter().any(|a| a.id == id);
    let yolo = Value::from("yolo");
    let mut details = Vec::new();
    let mut issues: Vec<String> = Vec::new();
    let mut matchers = Vec::new();

    let global_path = home.join(agent.global);
    if global_path.exists() {
        details.push(format!("Global config: {}", py_path_str(&global_path)));
        if has_hooks {
            match hooks::load(&global_path) {
                Ok(config) => {
                    if id == "gemini" {
                        let mode = approval_mode(&config).unwrap_or_else(|| "default".into());
                        details.push(format!("Gemini approval mode: {}", hooks::py_str(&mode)));
                        if mode != yolo {
                            issues.push(NO_YOLO.into());
                            details.push(
                                "Tip: Run 'dippy hooks setup-gemini-yolo' to enable full control."
                                    .into(),
                            );
                        }
                    }
                    if !hooks::has_dippy_hook(&config, id) {
                        details.push("Dippy hook: not installed".into());
                        issues.push("Dippy hook not found in config".into());
                    } else if hooks::legacy_command(&Value::Object(config.clone())).is_some() {
                        details.push("Dippy hook: legacy (old 'dippy-hook')".into());
                        issues.push("Legacy hook detected - consider updating".into());
                    } else {
                        details.push("Dippy hook: installed".into());
                        if verbose {
                            matchers = extract_matchers(&config, id);
                        }
                    }
                }
                Err(_) => {
                    details.push("Dippy hook: unable to check (config read error)".into());
                }
            }
        }
    } else {
        issues.push(format!("{} not installed (no config found)", agent.name));
    }

    let project_path = cwd.join(agent.project);
    if project_path.exists() {
        details.push(format!("Project config: {}", py_path_str(&project_path)));
        if let Some(config) = has_hooks.then(|| hooks::load(&project_path).ok()).flatten() {
            if id == "gemini"
                && let Some(mode) = approval_mode(&config)
            {
                details.push(format!(
                    "Project Gemini approval mode: {}",
                    hooks::py_str(&mode)
                ));
                if mode == yolo {
                    issues.retain(|i| i != NO_YOLO);
                } else {
                    details.push("Project Gemini overrides global to non-YOLO".into());
                }
            }
            if hooks::has_dippy_hook(&config, id) {
                details.push("Dippy hook in project: installed".into());
            }
        }
    }

    let format_info = match id {
        "claude" => "PreToolUse/PostToolUse hooks",
        "cursor" | "windsurf" => "beforeShellExecution hook",
        "gemini" => "BeforeTool/AfterTool hooks",
        "codex" => "PreToolUse/PermissionRequest/PostToolUse/Stop hooks + hooks feature flag",
        "pi" => "TypeScript extension",
        _ => "Unknown",
    };
    details.push(format!("Hook format: {format_info}"));

    let details = verbose.then(|| details.join("\n"));
    let matchers = if verbose { matchers } else { Vec::new() };
    let (status, message) = if issues.is_empty() {
        (Status::Ok, format!("{} is configured", agent.name))
    } else {
        (Status::Warning, format!("Issues: {}", issues.join("; ")))
    };
    Check::new(agent.name, status, &message, details).with_matchers(matchers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dippy-rs-doctor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::create_dir_all(dir.join("project")).unwrap();
        (dir.join("home"), dir.join("project"))
    }

    fn write(path: PathBuf, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    const CODEX_HOOKS: &str = r#"{"hooks": {
        "PreToolUse": [{"matcher": "^Bash$", "hooks": [{"command": "dippy --codex"}]}],
        "Stop": [{"hooks": [{"command": "dippy --codex"}]}]}}"#;

    #[test]
    fn codex_matchers_come_from_matcher() {
        let config: Map<String, Value> = serde_json::from_str(CODEX_HOOKS).unwrap();
        assert_eq!(
            extract_matchers(&config, "codex"),
            vec![("PreToolUse".to_string(), vec![json!("^Bash$")])]
        );
    }

    #[test]
    fn unrelated_dippy_path_is_not_a_legacy_hook() {
        let (home, cwd) = sandbox("legacy");
        write(
            home.join(".claude/settings.json"),
            r#"{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [
                {"command": "dippy --claude"},
                {"command": "/home/u/dippy/scripts/check.sh"}]}]}}"#,
        );
        let checks = check_hook_status(&home, &cwd, false);
        assert_eq!(checks[0].message, "Installed (global)");
        assert!(checks[0].status == Status::Ok);
        write(
            home.join(".claude/settings.json"),
            r#"{"hooks": {"PreToolUse": [{"hooks": [{"command": "/usr/bin/dippy-hook"}]}]}}"#,
        );
        let checks = check_hook_status(&home, &cwd, false);
        assert_eq!(checks[0].message, "Legacy hook (global)");
        assert_eq!(
            checks[0].details.as_deref(),
            Some("Legacy command: /usr/bin/dippy-hook\nExpected command: dippy --claude")
        );
    }

    #[test]
    fn codex_policy_and_feature_flag() {
        let (home, cwd) = sandbox("codex");
        write(home.join(".codex/hooks.json"), CODEX_HOOKS);
        write(
            home.join(".codex/config.toml"),
            "approval_policy = \"never\"\n[features]\nhooks = true\n",
        );
        write(cwd.join(".codex/hooks.json"), CODEX_HOOKS);
        write(
            cwd.join(".codex/config.toml"),
            "[profiles.x]\napproval_policy = \"on-request\"\n",
        );
        let checks = check_hook_status(&home, &cwd, false);
        let codex: Vec<(&str, &str)> = checks
            .iter()
            .filter(|c| c.name.contains("Codex"))
            .map(|c| (c.name.as_str(), c.message.as_str()))
            .collect();
        assert_eq!(
            codex,
            [
                (
                    "Hook: OpenAI Codex CLI",
                    "Hook installed, Codex feature flag missing (project)"
                ),
                ("Codex approval policy (global)", "Incompatible (never)"),
                ("Codex approval policy (project)", "Incompatible (never)"),
            ]
        );
    }

    #[test]
    fn home_as_cwd_skips_project_configs() {
        let (home, _) = sandbox("home-cwd");
        let checks = check_hook_status(&home, &home, false);
        assert_eq!(
            checks[0].details.as_deref(),
            Some(
                format!(
                    "Config not found at:\n  {}/.claude/settings.json",
                    home.display()
                )
                .as_str()
            )
        );
    }

    #[test]
    fn gemini_project_yolo_clears_global_issue() {
        let (home, cwd) = sandbox("gemini");
        write(
            home.join(".gemini/settings.json"),
            r#"{"approvalMode": "default"}"#,
        );
        write(
            cwd.join(".gemini/settings.json"),
            r#"{"policyEngineConfig": {"approvalMode": "yolo"}}"#,
        );
        let check = check_agent_specific("gemini", &home, &cwd, true);
        assert_eq!(check.message, "Issues: Dippy hook not found in config");
        let details = check.details.unwrap();
        assert!(
            details.contains("Project Gemini approval mode: yolo"),
            "{details}"
        );
    }

    #[test]
    fn non_object_config_counts_as_missing() {
        let (home, cwd) = sandbox("non-object");
        write(home.join(".cursor/hooks.json"), "[1, 2]");
        let checks = check_hook_status(&home, &cwd, false);
        let cursor = checks
            .iter()
            .find(|c| c.name == "Hook: Cursor IDE")
            .unwrap();
        assert_eq!(cursor.message, "Not installed");
        let check = check_agent_specific("cursor", &home, &cwd, true);
        assert!(
            check
                .details
                .unwrap()
                .contains("Dippy hook: unable to check (config read error)")
        );
    }

    #[test]
    fn agent_without_hook_command_checks_files_only() {
        let (home, cwd) = sandbox("pearai");
        write(home.join(".pearai/hooks.json"), "not json");
        let check = check_agent_specific("pearai", &home, &cwd, false);
        assert_eq!(check.message, "PearAI is configured");
        assert!(check.details.is_none());
    }

    #[test]
    fn json_check_keeps_python_key_order() {
        let check = Check::new("X", Status::Warning, "m", None)
            .fix("f")
            .with_matchers(vec![("PreToolUse".into(), vec![json!("Bash")])]);
        let report = check.to_json();
        let keys: Vec<&String> = report.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "name",
                "status",
                "message",
                "details",
                "fix_command",
                "matchers"
            ]
        );
        assert_eq!(check.to_json()["matchers"], json!({"PreToolUse": ["Bash"]}));
    }
}
