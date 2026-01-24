//! Type annotation AST nodes
//!
//! This module will be fully implemented in Phase 3.
//! For now, it provides minimal type structures to satisfy statement dependencies.

use super::*;
use crate::token::Span;

/// Type annotation (compile-time type)
///
/// This is a minimal placeholder. Full implementation in Phase 3.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAnnotation {
    pub ty: Type,
    pub span: Span,
}

/// Type
///
/// This is a minimal placeholder. Full implementation in Phase 3.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Primitive types: number, string, boolean, null, void
    Primitive(PrimitiveType),

    /// Type reference: MyClass, Point<T>
    Reference(TypeReference),

    /// Placeholder for other type variants (Phase 3)
    #[doc(hidden)]
    __Placeholder(Span),
}

/// Primitive type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveType {
    Number,
    String,
    Boolean,
    Null,
    Void,
}

/// Type reference: Point, Map<K, V>
#[derive(Debug, Clone, PartialEq)]
pub struct TypeReference {
    pub name: Identifier,
    pub type_args: Option<Vec<TypeAnnotation>>,
}

/// Type parameter (generic): T, K extends string
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParameter {
    pub name: Identifier,
    pub constraint: Option<TypeAnnotation>,
    pub default: Option<TypeAnnotation>,
    pub span: Span,
}
