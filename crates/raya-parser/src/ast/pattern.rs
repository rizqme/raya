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

/// Array destructuring pattern
/// Examples: [a, b], [x, , z], [first, ...rest]
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayPattern {
    /// Pattern elements (None for skipped elements)
    pub elements: Vec<Option<PatternElement>>,
    /// Rest element: ...rest
    pub rest: Option<Box<Pattern>>,
    pub span: Span,
}

/// Object destructuring pattern
/// Examples: { x, y }, { x: newX, y = 0 }, { a, ...rest }
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPattern {
    pub properties: Vec<ObjectPatternProperty>,
    /// Rest properties: ...rest
    pub rest: Option<Identifier>,
    pub span: Span,
}

/// Pattern element with optional default value
#[derive(Debug, Clone, PartialEq)]
pub struct PatternElement {
    pub pattern: Pattern,
    /// Default value: pattern = expr
    pub default: Option<Expression>,
    pub span: Span,
}

/// Object pattern property
/// Examples: x, x: y, x = 10, x: y = 10
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPatternProperty {
    /// Property key
    pub key: Identifier,
    /// Binding pattern (can be renamed)
    pub value: Pattern,
    /// Default value
    pub default: Option<Expression>,
    pub span: Span,
}
