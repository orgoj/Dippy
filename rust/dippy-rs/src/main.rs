use std::io::BufRead;
use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, CommandFactory, FromArgMatches, Parser};
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

const AFTER_HELP: &str = "\
Exit codes (CLI mode):
  0 = allow (command is safe)
  1 = deny (blocked by rule)
  2 = ask (needs user approval)

Without --cmd, --stdin or a subcommand, dippy runs as an agent hook and reads
the hook payload from stdin.

Examples:
  dippy --cmd 'rm -rf /'
  dippy --cmd 'ls -la' --json
  echo 'git status' | dippy --stdin --cwd /repo";

#[derive(Parser)]
#[command(
    name = "dippy",
    version,
    about = "Validate shell commands against Dippy rules.",
    after_help = AFTER_HELP
)]
struct Cli {
    /// Command to validate
    #[arg(
        long,
        value_name = "COMMAND",
        conflicts_with = "stdin",
        allow_hyphen_values = true
    )]
    cmd: Option<String>,
    /// Read command from stdin (plain text, not JSON)
    #[arg(long)]
    stdin: bool,
    /// Working directory (default: current)
    #[arg(long, value_name = "PATH")]
    cwd: Option<String>,
    /// Output as JSON
    #[arg(long)]
    json: bool,
    /// Config file path override
    #[arg(long, value_name = "PATH", conflicts_with = "config_only")]
    config: Option<String>,
    /// Load only this config file, skipping user and project config
    #[arg(long, value_name = "PATH")]
    config_only: Option<String>,
    /// Agent name for audit log
    #[arg(long, value_name = "NAME")]
    agent: Option<String>,
    /// Remote context (skip local path checks)
    #[arg(long)]
    remote: bool,
    #[command(subcommand)]
    command: Option<Subcommand>,
}

#[derive(clap::Subcommand)]
enum Subcommand {
    /// Query the audit log (read-only)
    #[command(
        after_help = "Filters the configured audit log and its daily rotations. \
                      Filters combine with AND;\ndates are UTC."
    )]
    Audit(dippy_rs::audit::Query),
    /// Manage Dippy configuration
    Config(dippy_rs::admin::ConfigArgs),
}

/// The derived parser plus the hidden agent hook flags (`--claude`, ...).
fn parse_cli() -> (Cli, ArgMatches) {
    let command = hook::mode_flag_names().fold(Cli::command(), |c, name| {
        c.arg(
            Arg::new(name)
                .long(name)
                .hide(true)
                .action(ArgAction::SetTrue),
        )
    });
    let matches = command.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    (cli, matches)
}

const EXIT_ALLOW: i32 = 0;
const EXIT_DENY: i32 = 1;
const EXIT_ASK: i32 = 2;

/// Port of `dippy.dippy.cli_mode`.
fn cli_mode(args: &Cli) -> i32 {
    let command = match args.cmd.clone() {
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
    config::print_warnings();
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

/// Port of `handle_audit_subcommand`.
fn run_audit(global: &Cli, query: &dippy_rs::audit::Query) -> i32 {
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
    config::print_warnings();
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
    match dippy_rs::audit::query(log, query, &process_cwd) {
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
        _ => {
            let (cli, matches) = parse_cli();
            let code = match &cli.command {
                Some(Subcommand::Audit(query)) => run_audit(&cli, query),
                Some(Subcommand::Config(args)) => dippy_rs::admin::run(args),
                None if cli.cmd.is_some() || cli.stdin => cli_mode(&cli),
                None => hook_mode(&matches),
            };
            std::process::exit(code);
        }
    }
}

/// Agent hook: the payload arrives on stdin.
fn hook_mode(matches: &ArgMatches) -> i32 {
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let flags: Vec<String> = hook::mode_flag_names()
        .filter(|name| matches.get_flag(name))
        .map(|name| format!("--{name}"))
        .collect();
    let mode = hook::mode_from_flags(&flags, |name| std::env::var(name).ok());
    let out = hook::run_hook(mode, &input);
    if let Some(stdout) = out.stdout {
        println!("{stdout}");
    }
    if let Some(stderr) = out.stderr {
        eprintln!("{stderr}");
    }
    out.exit_code
}
