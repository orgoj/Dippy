//! Port of `src/dippy/cli/symbols.py`.
//!
//! macOS symbol information display tool.
//! - Most operations display symbol info (safe)
//! - -saveSignature writes signature to file
//! - -symbolsPackageDir writes deep signatures to directory

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["symbols"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

/// Extract the argument for a given flag.
fn extract_flag_arg<'a>(tokens: &'a [String], flag: &str) -> Option<&'a String> {
    for (i, t) in tokens.iter().enumerate() {
        if t == flag && i + 1 < tokens.len() {
            return Some(&tokens[i + 1]);
        }
    }
    None
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if let Some(save_path) = extract_flag_arg(tokens, "-saveSignature").filter(|p| !p.is_empty()) {
        return Classification::allow_desc("symbols -saveSignature")
            .redirects(vec![save_path.clone()]);
    }
    if let Some(pkg_dir) = extract_flag_arg(tokens, "-symbolsPackageDir").filter(|p| !p.is_empty())
    {
        return Classification::allow_desc("symbols -symbolsPackageDir")
            .redirects(vec![pkg_dir.clone()]);
    }
    Classification::allow_desc("symbols")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    fn targets(r: &Classification) -> Vec<String> {
        r.redirect_targets.clone().unwrap_or_default()
    }

    #[test]
    fn symbols_save_has_redirect_target() {
        let r = run(&["symbols", "-saveSignature", "/tmp/sig.txt", "/usr/bin/ls"]);
        assert!(targets(&r).contains(&"/tmp/sig.txt".to_string()));
    }

    #[test]
    fn symbols_package_dir_has_redirect_target() {
        let r = run(&["symbols", "-symbolsPackageDir", "/tmp/pkg", "/usr/bin/ls"]);
        assert!(targets(&r).contains(&"/tmp/pkg".to_string()));
    }

    #[test]
    fn plain_allows() {
        let r = run(&["symbols", "/usr/bin/ls"]);
        assert_eq!(r.action, Action::Allow);
        assert!(r.redirect_targets.is_none());
    }
}
