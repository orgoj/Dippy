//! Canonical JSON dump of the Parable-shaped AST (mirrors rust/parity/ast_dump.py).

use crate::ast::{Node, Word};
use serde_json::{Map, Value, json};

fn words(ws: &[Word]) -> Value {
    Value::Array(ws.iter().map(word).collect())
}

fn word(w: &Word) -> Value {
    json!({"kind": "word", "value": w.value, "parts": nodes(&w.parts)})
}

fn nodes(ns: &[Node]) -> Value {
    Value::Array(ns.iter().map(node).collect())
}

fn opt_words(ws: &Option<Vec<Word>>) -> Value {
    ws.as_ref().map_or(Value::Null, |w| words(w))
}

pub fn node(n: &Node) -> Value {
    let mut m = Map::new();
    m.insert("kind".into(), Value::String(n.kind().to_string()));
    let mut put = |k: &str, v: Value| {
        m.insert(k.to_string(), v);
    };
    match n {
        Node::Word(w) => return word(w),
        Node::Command {
            words: ws,
            redirects,
        } => {
            put("words", words(ws));
            put("redirects", nodes(redirects));
        }
        Node::Pipeline { commands } => put("commands", nodes(commands)),
        Node::List { parts } => put("parts", nodes(parts)),
        Node::Operator { .. }
        | Node::PipeBoth
        | Node::ParamLen
        | Node::AnsiC
        | Node::Locale
        | Node::Comment
        | Node::Empty
        | Node::Array
        | Node::ArithDeprecated => {}
        Node::Redirect { op, target } => {
            put("op", json!(op));
            put("target", word(target));
        }
        Node::HereDoc {
            content,
            quoted,
            fd,
        } => {
            put("content", json!(content));
            put("quoted", json!(quoted));
            put("fd", fd.map_or(Value::Null, |f| json!(f)));
        }
        Node::Subshell { body, redirects } | Node::BraceGroup { body, redirects } => {
            put("body", node(body));
            put("redirects", nodes(redirects));
        }
        Node::If {
            condition,
            then_body,
            else_body,
            redirects,
        } => {
            put("condition", node(condition));
            put("then_body", node(then_body));
            put(
                "else_body",
                else_body.as_ref().map_or(Value::Null, |e| node(e)),
            );
            put("redirects", nodes(redirects));
        }
        Node::While {
            condition,
            body,
            redirects,
        }
        | Node::Until {
            condition,
            body,
            redirects,
        } => {
            put("condition", node(condition));
            put("body", node(body));
            put("redirects", nodes(redirects));
        }
        Node::For {
            words: ws,
            body,
            redirects,
        }
        | Node::Select {
            words: ws,
            body,
            redirects,
        } => {
            put("words", opt_words(ws));
            put("body", node(body));
            put("redirects", nodes(redirects));
        }
        Node::ForArith {
            init,
            cond,
            incr,
            body,
            redirects,
        } => {
            put("init", json!(init));
            put("cond", json!(cond));
            put("incr", json!(incr));
            put("body", node(body));
            put("redirects", nodes(redirects));
        }
        Node::Case {
            word: w,
            bodies,
            redirects,
        } => {
            put("word", word(w));
            put(
                "patterns",
                Value::Array(
                    bodies
                        .iter()
                        .map(|b| json!({"kind": "pattern", "body": b.as_ref().map_or(Value::Null, node)}))
                        .collect(),
                ),
            );
            put("redirects", nodes(redirects));
        }
        Node::Function { body } => put("body", node(body)),
        Node::Param { arg } | Node::ParamIndirect { arg } => put("arg", json!(arg)),
        Node::CmdSub { command } | Node::Coproc { command } => put("command", node(command)),
        Node::ProcSub { direction, command } => {
            put("direction", json!(direction));
            put("command", node(command));
        }
        Node::Arith { cmdsubs } => put("cmdsubs", nodes(cmdsubs)),
        Node::ArithCmd { cmdsubs, redirects } => {
            put("cmdsubs", nodes(cmdsubs));
            put("redirects", nodes(redirects));
        }
        Node::Negation { pipeline } | Node::Time { pipeline } => put("pipeline", node(pipeline)),
        Node::CondExpr { body, redirects } => {
            put("body", node(body));
            put("redirects", nodes(redirects));
        }
        Node::UnaryTest { operand } => put("operand", word(operand)),
        Node::BinaryTest { left, right } => {
            put("left", word(left));
            put("right", word(right));
        }
        Node::CondAnd { left, right } | Node::CondOr { left, right } => {
            put("left", node(left));
            put("right", node(right));
        }
        Node::CondNot { operand } => put("operand", node(operand)),
        Node::CondParen { inner } => put("inner", node(inner)),
        Node::Unsupported(what) => {
            put("unknown", json!(true));
            put("what", json!(what));
        }
    }
    Value::Object(m)
}

pub fn dump_command(cmd: &str) -> Value {
    match crate::ast::parse(cmd.trim()) {
        Ok(ns) => json!({"nodes": nodes(&ns)}),
        Err(_) => json!({"error": "parse"}),
    }
}
