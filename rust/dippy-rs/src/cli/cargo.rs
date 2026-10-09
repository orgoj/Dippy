//! Port of `src/dippy/cli/cargo.py`: Cargo (Rust) CLI handler.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["cargo"];
pub const PORTED: bool = true;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

const SAFE_ACTIONS: &[&str] = &[
    "help",
    "-h",
    "--help",
    "version",
    "-V",
    "--version",
    "search",
    "info",
    "tree",
    "metadata",
    "read-manifest",
    "locate-project",
    "pkgid",
    "verify-project",
    "check",
    "c",      // Type checking only
    "clippy", // Linting only
    "fmt",    // Formatting
    "doc",    // Generate docs
    "fetch",  // Download deps
    "generate-lockfile",
    "update", // Update lockfile
    "vendor",
    "login",
    "logout",
    "owner",
];

/// Short aliases that need expansion for clarity (`ACTION_ALIASES`).
fn action_alias(action: &str) -> &str {
    match action {
        "r" => "run",
        "b" => "build",
        "t" => "test",
        other => other,
    }
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map(String::as_str).unwrap_or("cargo");
    if tokens.len() < 2 {
        return Classification::ask_desc(base);
    }
    let action = tokens[1].as_str();
    if SAFE_ACTIONS.contains(&action) {
        return Classification::allow_desc(format!("{base} {action}"));
    }
    // Expand short aliases for clarity
    let display_action = action_alias(action);
    Classification::ask_desc(format!("{base} {display_action}"))
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn action(cmd: &str) -> Action {
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        classify(&HandlerContext::new(&tokens)).action
    }

    #[test]
    fn classifies() {
        for (cmd, allowed) in [
            ("cargo help", true),
            ("cargo --help", true),
            ("cargo -h", true),
            ("cargo help build", true),
            ("cargo version", true),
            ("cargo --version", true),
            ("cargo -V", true),
            ("cargo search serde", true),
            ("cargo info serde", true),
            ("cargo tree --invert serde", true),
            ("cargo metadata --no-deps", true),
            ("cargo read-manifest", true),
            ("cargo locate-project", true),
            ("cargo pkgid serde", true),
            ("cargo verify-project", true),
            ("cargo check", true),
            ("cargo c", true),
            ("cargo clippy -- -D warnings", true),
            ("cargo clippy --fix", true),
            ("cargo fmt --check", true),
            ("cargo doc --open", true),
            ("cargo fetch --locked", true),
            ("cargo update -p serde", true),
            ("cargo generate-lockfile", true),
            ("cargo vendor vendor/", true),
            ("cargo login", true),
            ("cargo logout", true),
            ("cargo owner --list", true),
            ("cargo build", false),
            ("cargo b", false),
            ("cargo run -- arg1 arg2", false),
            ("cargo r", false),
            ("cargo test", false),
            ("cargo t", false),
            ("cargo bench", false),
            ("cargo install ripgrep", false),
            ("cargo install --list", false),
            ("cargo uninstall ripgrep", false),
            ("cargo publish --dry-run", false),
            ("cargo yank --version 1.0.0", false),
            ("cargo clean", false),
            ("cargo new myproject", false),
            ("cargo init", false),
            ("cargo add serde", false),
            ("cargo remove serde", false),
            ("cargo rm serde", false),
            ("cargo fix", false),
            ("cargo", false),
            ("cargo unknown-command", false),
            ("cargo +nightly build", false),
            ("cargo +stable test", false),
        ] {
            let expected = if allowed { Action::Allow } else { Action::Ask };
            assert_eq!(action(cmd), expected, "{cmd}");
        }
    }

    #[test]
    fn alias_description() {
        let c = classify(&HandlerContext::new(&["cargo", "r"]));
        assert_eq!(c.description.as_deref(), Some("cargo run"));
    }
}
