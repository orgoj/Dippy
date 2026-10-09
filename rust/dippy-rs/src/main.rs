mod ast;
mod dump;
mod scan;

use std::io::BufRead;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--dump-ast-jsonl") {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let line = line.unwrap_or_default();
            if line.trim().is_empty() {
                continue;
            }
            let case: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
            let cmd = case.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
            println!("{}", dump::dump_command(cmd));
        }
    }
}
