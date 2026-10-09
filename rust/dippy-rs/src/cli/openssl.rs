//! Port of `src/dippy/cli/openssl.py`.
//!
//! Some openssl commands are read-only (viewing certs); others modify files
//! or do crypto operations.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["openssl"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_COMMANDS: &[&str] = &["version", "help", "list"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("openssl");
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }

    let subcommand = tokens[1].as_str();

    if SAFE_COMMANDS.contains(&subcommand) {
        return Classification::allow_desc(format!("{base} {subcommand}"));
    }

    // x509 with -noout is just viewing
    if subcommand == "x509" && tokens.iter().any(|t| t == "-noout") {
        return Classification::allow_desc(format!("{base} x509"));
    }

    // s_client for connection testing
    if subcommand == "s_client" {
        return Classification::allow_desc(format!("{base} s_client"));
    }

    Classification::ask_desc(format!("{base} {subcommand}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn cases() {
        for t in [
            vec!["openssl", "version"],
            vec!["openssl", "help"],
            vec!["openssl", "list", "-digest-commands"],
            vec!["openssl", "x509", "-in", "c.pem", "-noout", "-text"],
            vec!["openssl", "s_client", "-connect", "host:443"],
        ] {
            assert_eq!(run(&t).action, Action::Allow, "{t:?}");
        }
        for t in [
            vec!["openssl"],
            vec!["openssl", "x509", "-in", "c.pem", "-out", "o.pem"],
            vec!["openssl", "genrsa", "-out", "k.pem"],
            vec!["openssl", "req", "-new"],
            vec!["openssl", "enc", "-aes256"],
        ] {
            assert_eq!(run(&t).action, Action::Ask, "{t:?}");
        }
        assert_eq!(
            run(&["openssl", "genrsa"]).description.as_deref(),
            Some("openssl genrsa")
        );
    }
}
