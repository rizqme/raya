//! Type annotation parsing

use super::{ParseError, Parser};
use crate::ast::TypeAnnotation;

/// Parse a type annotation.
pub fn parse_type_annotation(parser: &mut Parser) -> Result<TypeAnnotation, ParseError> {
    todo!("Implement type annotation parsing")
}
