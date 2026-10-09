fn main() {
    let src = std::env::args().nth(1).unwrap_or_default();
    match rable::parse(&src, false) {
        Ok(nodes) => {
            for n in nodes {
                println!("{}..{} {:?}", n.span.start, n.span.end, n.source_text(&src))
            }
        }
        Err(e) => println!("ERR {e}"),
    }
}
