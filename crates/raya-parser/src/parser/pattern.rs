//! Pattern parsing (for destructuring and parameter bindings)

use super::{ParseError, Parser};
use crate::ast::Pattern;

/// Parse a pattern (identifier or destructuring).
pub fn parse_pattern(parser: &mut Parser) -> Result<Pattern, ParseError> {
    todo!("Implement pattern parsing")
}
