//! Port of `src/dippy/cli/azure.py` - NOT PORTED YET (always asks).

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["az"];
pub const PORTED: bool = false;
/// Module-level `get_description`, if the Python module defines one (it does).
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(_ctx: &HandlerContext) -> Classification {
    Classification::ask()
}
