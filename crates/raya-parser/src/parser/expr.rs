//! Expression parsing

use super::{ParseError, Parser};
use crate::ast::Expression;

/// Parse an expression.
pub fn parse_expression(parser: &mut Parser) -> Result<Expression, ParseError> {
    todo!("Implement expression parsing")
}

/// Parse a primary expression (literal, identifier, grouped expression, etc.).
pub fn parse_primary(parser: &mut Parser) -> Result<Expression, ParseError> {
    todo!("Implement primary expression parsing")
}
