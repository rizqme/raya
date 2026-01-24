//! Expression AST nodes
//!
//! This module will be fully implemented in Phase 2.
//! For now, it provides a minimal Expression type to satisfy statement dependencies.

use super::*;
use crate::token::Span;

/// Expression (produces a value)
///
/// This is a minimal placeholder. Full implementation in Phase 2.
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// Integer literal: 42
    IntLiteral(IntLiteral),

    /// Float literal: 3.14
    FloatLiteral(FloatLiteral),

    /// String literal: "hello"
    StringLiteral(StringLiteral),

    /// Boolean literal: true, false
    BooleanLiteral(BooleanLiteral),

    /// Null literal
    NullLiteral(Span),

    /// Identifier
    Identifier(Identifier),

    /// Placeholder for other expression types (Phase 2)
    #[doc(hidden)]
    __Placeholder(Span),
}

impl Expression {
    pub fn span(&self) -> &Span {
        match self {
            Expression::IntLiteral(e) => &e.span,
            Expression::FloatLiteral(e) => &e.span,
            Expression::StringLiteral(e) => &e.span,
            Expression::BooleanLiteral(e) => &e.span,
            Expression::NullLiteral(span) => span,
            Expression::Identifier(e) => &e.span,
            Expression::__Placeholder(span) => span,
        }
    }

    /// Check if this expression is a literal
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            Expression::IntLiteral(_)
                | Expression::FloatLiteral(_)
                | Expression::StringLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
        )
    }
}

// ============================================================================
// Literal Expressions (minimal set for Phase 1)
// ============================================================================

/// Integer literal: 42, 0xFF, 0b1010
#[derive(Debug, Clone, PartialEq)]
pub struct IntLiteral {
    pub value: i64,
    pub span: Span,
}

/// Float literal: 3.14, 1.0e10
#[derive(Debug, Clone, PartialEq)]
pub struct FloatLiteral {
    pub value: f64,
    pub span: Span,
}

/// String literal: "hello"
#[derive(Debug, Clone, PartialEq)]
pub struct StringLiteral {
    pub value: String,
    pub span: Span,
}

/// Boolean literal: true, false
#[derive(Debug, Clone, PartialEq)]
pub struct BooleanLiteral {
    pub value: bool,
    pub span: Span,
}
