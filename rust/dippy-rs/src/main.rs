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
    let config =
        match config::load_config(&cwd, args.config.as_deref(), args.config_only.as_deref()) {
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--dump-ast-jsonl") => {
            for case in jsonl_lines() {
                println!("{}", dump::dump_command(case["cmd"].as_str().unwrap_or("")));
            }
        }
        Some("--handler-jsonl") => handler_jsonl(),
        Some("--claude") if args.len() == 1 => {
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            if let Some(out) = hook::claude_hook(&input) {
                println!("{out}");
            }
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
