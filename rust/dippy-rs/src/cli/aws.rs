//! Port of `src/dippy/cli/aws.py` - NOT PORTED YET (always asks).

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["aws"];
pub const PORTED: bool = false;
/// Module-level `get_description`, if the Python module defines one (it does).
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(_ctx: &HandlerContext) -> Classification {
    Classification::ask()
}
