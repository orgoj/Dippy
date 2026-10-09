use std::io::BufRead;
use std::path::PathBuf;

use dippy_rs::cli::{self, HandlerContext};
use dippy_rs::config::Config;
use dippy_rs::dump;
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--dump-ast-jsonl") => {
            for case in jsonl_lines() {
                println!("{}", dump::dump_command(case["cmd"].as_str().unwrap_or("")));
            }
        }
        Some("--handler-jsonl") => handler_jsonl(),
        _ => {
            eprintln!("usage: dippy-rs --cmd CMD [--json] [--cwd DIR] [--config FILE]");
            std::process::exit(2);
        }
    }
}
