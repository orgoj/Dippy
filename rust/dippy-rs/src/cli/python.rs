//! Port of `src/dippy/cli/python.py` - NOT PORTED YET (always asks).

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &[
    "python",
    "python3",
    "python3.8",
    "python3.9",
    "python3.10",
    "python3.11",
    "python3.12",
    "python3.13",
    "python3.14",
    "python3.15",
    "python3.16",
    "python3.17",
    "python3.18",
    "python3.19",
];
pub const PORTED: bool = false;
/// Module-level `get_description`, if the Python module defines one (it does).
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(_ctx: &HandlerContext) -> Classification {
    Classification::ask()
}
