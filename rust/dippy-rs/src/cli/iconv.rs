//! Port of `src/dippy/cli/iconv.py` - NOT PORTED YET (always asks).

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["iconv"];
pub const PORTED: bool = false;
/// Module-level `get_description`, if the Python module defines one.
pub const DESCRIPTION: Option<Describe> = None;

pub fn classify(_ctx: &HandlerContext) -> Classification {
    Classification::ask()
}
