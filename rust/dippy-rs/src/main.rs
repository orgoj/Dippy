use std::io::BufRead;
use std::path::PathBuf;

use dippy_rs::analyzer::{self, Action};
use dippy_rs::cli::{self, HandlerContext};
use dippy_rs::config::{self, Config};
use dippy_rs::hook::{self, py_json_str};
use dippy_rs::{dump, paths};
use serde_json::{Value, json};

fn jsonl_lines() -> impl Iterator<Item = Value> {
    std::io::stdin()
        .lock()
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            if line.trim().is_empty() {
                None
            } else {
                Some(serde_json::from_str(&line).unwrap_or(Value::Null))
            }
        })
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|s| s.as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Development mode: run one handler on tokens prepared by handler_compare.py.
fn handler_jsonl() {
    let config = Config::default();
    for case in jsonl_lines() {
        let tokens = strings(&case["tokens"]);
        let Some(handler) = tokens.first().and_then(|t| cli::get_handler(t)) else {
            println!("{}", json!({"error": "no handler"}));
            continue;
        };
        let ctx = HandlerContext {
            tokens,
            remote: false,
            cwd: PathBuf::from(case["cwd"].as_str().unwrap_or("/")),
            config: Some(&config),
            word_has_expansions: case["exp"]
                .as_array()
                .map(|a| a.iter().map(|b| b.as_bool().unwrap_or(true)).collect())
                .unwrap_or_default(),
            raw_words: strings(&case["raw"]),
        };
        let r = (handler.classify)(&ctx);
        println!(
            "{}",
            json!({
                "ported": handler.ported,
                "action": r.action.as_str(),
                "inner_command": r.inner_command,
                "description": r.description,
                "redirect_targets": r.redirect_targets,
                "wrapper_context": r.wrapper_context,
                "remote": r.remote,
                "replace_suggestion": r.replace_suggestion,
            })
        );
    }
}

#[derive(Default)]
struct CliArgs {
    cmd: Option<String>,
    stdin: bool,
    json: bool,
    cwd: Option<String>,
    config: Option<String>,
    config_only: Option<String>,
    remote: bool,
}

fn parse_args(args: &[String]) -> Result<CliArgs, String> {
    let mut out = CliArgs::default();
    let mut i = 0;
    while i < args.len() {
        let (flag, inline) = match args[i].split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (args[i].clone(), None),
        };
        let mut value = || -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--cmd" => out.cmd = Some(value()?),
            "--cwd" => out.cwd = Some(value()?),
            "--config" => out.config = Some(value()?),
            "--config-only" => out.config_only = Some(value()?),
            "--agent" => {
                value()?;
            }
            "--stdin" => out.stdin = true,
            "--json" => out.json = true,
            "--remote" => out.remote = true,
            other => return Err(format!("unrecognized argument: {other}")),
        }
        i += 1;
    }
    if out.cmd.is_some() == out.stdin {
        return Err("exactly one of --cmd or --stdin is required".into());
    }
    Ok(out)
}

const EXIT_ALLOW: i32 = 0;
const EXIT_DENY: i32 = 1;
const EXIT_ASK: i32 = 2;

/// Config warnings: Python's `logging.warning` falls back to `basicConfig`.
fn print_config_warnings() {
    for warning in config::take_warnings() {
        eprintln!("WARNING:root:{warning}");
    }
}

/// Port of `dippy.dippy.cli_mode`.
fn cli_mode(args: CliArgs) -> i32 {
    let command = match args.cmd {
        Some(c) => c,
        None => {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut s);
            s.trim().to_string()
        }
    };
    if command.is_empty() {
        eprintln!("Error: empty command");
        return EXIT_ASK;
    }
    let cwd = match &args.cwd {
        Some(c) => paths::resolve(&PathBuf::from(c)),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
    };
    let loaded = config::load_config(&cwd, args.config.as_deref(), args.config_only.as_deref());
    print_config_warnings();
    let config = match loaded {
        Ok(c) => c,
        Err(e) => {
            if args.json {
                println!(
                    "{{\"decision\": \"ask\", \"reason\": {}}}",
                    py_json_str(&format!("config error: {e}"))
                );
            } else {
                println!("ask: config error: {e}");
            }
            return EXIT_ASK;
        }
    };
    // The notifier (an external program) is out of scope and never run.
    let result = analyzer::analyze(&command, &config, &cwd, None, args.remote);
    let action = result.action.as_str();
    if args.json {
        println!(
            "{{\"decision\": {}, \"reason\": {}}}",
            py_json_str(action),
            py_json_str(&result.reason)
        );
    } else {
        println!("{action}: {}", result.reason);
    }
    match result.action {
        Action::Allow => EXIT_ALLOW,
        Action::Deny => EXIT_DENY,
        Action::Ask => EXIT_ASK,
    }
}

/// `dippy [--cwd DIR] [--config FILE | --config-only FILE] audit ...`;
/// `None` when the arguments are not an audit invocation.
fn audit_main(args: &[String]) -> Option<i32> {
    let mut global = CliArgs::default();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "audit" {
            return Some(run_audit(&global, &args[i + 1..]));
        }
        let (flag, inline) = match args[i].split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (args[i].as_str(), None),
        };
        let slot = match flag {
            "--cwd" => &mut global.cwd,
            "--config" => &mut global.config,
            "--config-only" => &mut global.config_only,
            _ => return None,
        };
        if inline.is_none() {
            i += 1;
        }
        *slot = inline.or_else(|| args.get(i).cloned());
        i += 1;
    }
    None
}

/// Port of `handle_audit_subcommand`.
fn run_audit(global: &CliArgs, args: &[String]) -> i32 {
    let query = match dippy_rs::audit::parse_args(args) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("usage: dippy-rs audit [options]\ndippy-rs audit: error: {e}");
            return 2;
        }
    };
    let process_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let cwd = match &global.cwd {
        Some(c) => paths::resolve(&PathBuf::from(c)),
        None => process_cwd.clone(),
    };
    let loaded = config::load_config(
        &cwd,
        global.config.as_deref(),
        global.config_only.as_deref(),
    );
    print_config_warnings();
    let config = match loaded {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {e}");
            return 1;
        }
    };
    let Some(log) = &config.log else {
        eprintln!("audit log is not configured (set log PATH)");
        return 1;
    };
    match dippy_rs::audit::query(log, &query, &process_cwd) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            0
        }
        Err(e) => {
            eprintln!("audit: {e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--dump-ast-jsonl") => {
            for case in jsonl_lines() {
                println!("{}", dump::dump_command(case["cmd"].as_str().unwrap_or("")));
            }
        }
        Some("--handler-jsonl") => handler_jsonl(),
        _ if let Some(code) = audit_main(&args) => std::process::exit(code),
        _ if args.iter().all(|a| hook::is_mode_flag(a)) => {
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            let mode = hook::mode_from_flags(&args, |name| std::env::var(name).ok());
            let out = hook::run_hook(mode, &input);
            if let Some(stdout) = out.stdout {
                println!("{stdout}");
            }
            if let Some(stderr) = out.stderr {
                eprintln!("{stderr}");
            }
            std::process::exit(out.exit_code);
        }
        _ => match parse_args(&args) {
            Ok(a) => std::process::exit(cli_mode(a)),
            Err(e) => {
                eprintln!(
                    "usage: dippy-rs --cmd CMD [--json] [--cwd DIR] [--config FILE]\nerror: {e}"
                );
                std::process::exit(EXIT_ASK);
            }
        },
    }
}
