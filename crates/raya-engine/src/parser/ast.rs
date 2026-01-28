//! Abstract Syntax Tree (AST) for the Raya programming language.
//!
//! This module defines the complete AST structure for Raya, including:
//! - Module and program structure
//! - Statements (declarations, control flow, etc.)
//! - Expressions (literals, operators, function calls, etc.)
//! - Type annotations
//! - Patterns (for destructuring)
//!
//! Every AST node includes a `Span` for precise source location tracking.

use crate::parser::token::Span;

// Re-export submodules
pub mod statement;
pub mod expression;
pub mod types;
pub mod pattern;
pub mod visitor;

pub use statement::*;
pub use expression::*;
pub use types::*;
pub use pattern::*;
pub use visitor::*;

/// Root node: a Raya source file (module)
///
/// # Example
/// ```
/// use raya_parser::ast::*;
/// use raya_parser::token::Span;
///
/// let module = Module::new(
///     vec![],
///     Span::new(0, 0, 1, 1),
/// );
/// assert!(module.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Top-level statements (declarations, imports, exports)
    pub statements: Vec<Statement>,

    /// Span covering the entire module
    pub span: Span,
}

impl Module {
    /// Create a new module
    pub fn new(statements: Vec<Statement>, span: Span) -> Self {
        Self { statements, span }
    }

    /// Check if the module is empty
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }

    /// Get the number of top-level statements
    pub fn len(&self) -> usize {
        self.statements.len()
    }
}

/// Identifier
///
/// Represents a name for a variable, function, class, etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier {
    pub name: crate::parser::interner::Symbol,
    pub span: Span,
}

impl Identifier {
    pub fn new(name: crate::parser::interner::Symbol, span: Span) -> Self {
        Self { name, span }
    }
}
