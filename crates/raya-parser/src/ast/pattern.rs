//! Pattern AST nodes
//!
//! Patterns are used in variable declarations, function parameters, and destructuring.

use super::*;
use crate::token::Span;

/// Pattern (for destructuring and binding)
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Simple identifier: x
    Identifier(Identifier),

    /// Array destructuring: [x, y] (Phase 2)
    #[doc(hidden)]
    Array(ArrayPattern),

    /// Object destructuring: { x, y } (Phase 2)
    #[doc(hidden)]
    Object(ObjectPattern),
}

impl Pattern {
    pub fn span(&self) -> &Span {
        match self {
            Pattern::Identifier(id) => &id.span,
            Pattern::Array(p) => &p.span,
            Pattern::Object(p) => &p.span,
        }
    }
}

/// Array destructuring pattern (Phase 2 - placeholder)
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayPattern {
    pub elements: Vec<Option<Pattern>>,
    pub span: Span,
}

/// Object destructuring pattern (Phase 2 - placeholder)
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPattern {
    pub properties: Vec<ObjectPatternProperty>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPatternProperty {
    pub key: Identifier,
    pub value: Pattern,
    pub span: Span,
}
