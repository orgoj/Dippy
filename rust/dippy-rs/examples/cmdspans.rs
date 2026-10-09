use rable::ast::{Node, NodeKind as K};
fn walk(n: &Node, src: &str) {
    match &n.kind {
        K::Command {
            assignments,
            words,
            redirects,
        } => {
            println!("CMD {}..{}", n.span.start, n.span.end);
            for c in assignments.iter().chain(words).chain(redirects) {
                println!(
                    "   {:?} {}..{} {:?}",
                    std::mem::discriminant(&c.kind),
                    c.span.start,
                    c.span.end,
                    c.source_text(src)
                );
            }
        }
        K::List { items } => items.iter().for_each(|i| walk(&i.command, src)),
        K::Pipeline { commands, .. } => commands.iter().for_each(|c| walk(c, src)),
        K::While {
            condition, body, ..
        } => {
            walk(condition, src);
            walk(body, src)
        }
        K::BraceGroup { body, .. } | K::Subshell { body, .. } => walk(body, src),
        _ => println!("other {:?}", n.span),
    }
}
fn main() {
    let src = std::env::args().nth(1).unwrap();
    for n in rable::parse(&src, false).unwrap() {
        walk(&n, &src)
    }
}
