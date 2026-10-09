use rable::ast::{Node, NodeKind};
fn d(n: &Node, ind: usize) {
    let p = " ".repeat(ind);
    match &n.kind {
        NodeKind::Word { value, parts, .. } => {
            println!("{p}Word {value:?} span={}..{}", n.span.start, n.span.end);
            for x in parts {
                d(x, ind + 2)
            }
        }
        NodeKind::Command {
            assignments,
            words,
            redirects,
        } => {
            println!("{p}Command span={}..{}", n.span.start, n.span.end);
            for x in assignments {
                println!("{p} assign:");
                d(x, ind + 2)
            }
            for x in words {
                d(x, ind + 2)
            }
            for x in redirects {
                println!("{p} redir:");
                d(x, ind + 2)
            }
        }
        NodeKind::List { items } => {
            println!("{p}List span={}..{}", n.span.start, n.span.end);
            for it in items {
                d(&it.command, ind + 2);
                println!("{p}  op={:?}", it.operator)
            }
        }
        NodeKind::Pipeline { commands, .. } => {
            println!("{p}Pipeline");
            for x in commands {
                d(x, ind + 2)
            }
        }
        NodeKind::Redirect {
            op,
            target,
            fd,
            varfd,
        } => {
            println!(
                "{p}Redirect op={op:?} fd={fd} varfd={varfd:?} span={:?} src={:?}",
                n.span,
                n.source_text(src_get().as_str())
            );
            d(target, ind + 2)
        }
        NodeKind::CommandSubstitution { command, brace } => {
            println!(
                "{p}CmdSub brace={brace} span={}..{}",
                n.span.start, n.span.end
            );
            d(command, ind + 2)
        }
        NodeKind::ProcessSubstitution { direction, command } => {
            println!(
                "{p}ProcSub {direction} span={}..{}",
                n.span.start, n.span.end
            );
            d(command, ind + 2)
        }
        other => {
            let s = format!("{other:?}");
            println!("{p}{}", &s[..s.len().min(300)]);
        }
    }
}
static SRC: std::sync::OnceLock<String> = std::sync::OnceLock::new();
fn src_get() -> &'static String {
    SRC.get().unwrap()
}
fn main() {
    let src = std::env::args().nth(1).unwrap_or_default();
    SRC.set(src.clone()).unwrap();
    match rable::parse(&src, false) {
        Ok(nodes) => {
            for n in nodes {
                println!("{}", n);
                d(&n, 0)
            }
        }
        Err(e) => println!("ERR {e:?} / {e}"),
    }
}
