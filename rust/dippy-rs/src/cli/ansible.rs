//! Port of `src/dippy/cli/ansible.py` - NOT PORTED YET (always asks).

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &[
    "ansible",
    "ansible-playbook",
    "ansible-vault",
    "ansible-galaxy",
    "ansible-inventory",
    "ansible-doc",
    "ansible-pull",
    "ansible-config",
    "ansible-console",
    "ansible-lint",
    "ansible-test",
];
pub const PORTED: bool = false;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(_ctx: &HandlerContext) -> Classification {
    Classification::ask()
}
