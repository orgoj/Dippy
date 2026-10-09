//! Port of `dippy.core.allowlists` (generated from the Python source by
//! `build.rs`).

use std::collections::HashSet;
use std::sync::LazyLock;

include!(concat!(env!("OUT_DIR"), "/allowlists.rs"));

static SIMPLE_SAFE: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| SIMPLE_SAFE_LIST.iter().copied().collect());
static WRAPPER_COMMANDS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| WRAPPER_COMMANDS_LIST.iter().copied().collect());

/// Known read-only commands.
pub fn is_simple_safe(name: &str) -> bool {
    SIMPLE_SAFE.contains(name)
}

/// Transparent wrappers whose inner command is analysed instead.
pub fn is_wrapper_command(name: &str) -> bool {
    WRAPPER_COMMANDS.contains(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_lists() {
        assert!(is_simple_safe("cat"));
        assert!(!is_simple_safe("rm"));
        assert!(is_wrapper_command("timeout"));
        assert_eq!(WRAPPER_COMMANDS.len(), 13);
    }
}
