//! Type annotation AST nodes
//!
//! This module defines the complete type system for Raya, including:
//! - Primitive types (number, string, boolean, null, void)
//! - Type references (MyClass, Point<T>)
//! - Union types (A | B | C)
//! - Function types ((x: number) => number)
//! - Array and tuple types
//! - Object types
//! - Type parameters (generics)

use super::*;
use crate::token::Span;

/// Type annotation (compile-time type)
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAnnotation {
    pub ty: Type,
    pub span: Span,
}

/// Type
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Primitive types: number, string, boolean, null, void
    Primitive(PrimitiveType),

    /// Type reference: MyClass, Point<T>
    Reference(TypeReference),

    /// Union type: number | string | null
    Union(UnionType),

    /// Function type: (x: number) => number
    Function(FunctionType),

    /// Array type: number[]
    Array(ArrayType),

    /// Tuple type: [number, string]
    Tuple(TupleType),

    /// Object type: { x: number; y: string }
    Object(ObjectType),

    /// Typeof type: typeof value (only for bare unions)
    Typeof(TypeofType),

    /// String literal type: "foo"
    StringLiteral(crate::interner::Symbol),

    /// Number literal type: 42
    NumberLiteral(f64),

    /// Boolean literal type: true | false
    BooleanLiteral(bool),

    /// Parenthesized type: (number | string)
    Parenthesized(Box<TypeAnnotation>),
}

impl Type {
    /// Check if this type is a primitive
    pub fn is_primitive(&self) -> bool {
        matches!(self, Type::Primitive(_))
    }

    /// Check if this type is a union
    pub fn is_union(&self) -> bool {
        matches!(self, Type::Union(_))
    }

    /// Check if this type is a function type
    pub fn is_function(&self) -> bool {
        matches!(self, Type::Function(_))
    }

    /// Get primitive type if this is a primitive
    pub fn as_primitive(&self) -> Option<PrimitiveType> {
        match self {
            Type::Primitive(p) => Some(*p),
            _ => None,
        }
    }
}

// ============================================================================
// Primitive Types
// ============================================================================

/// Primitive type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveType {
    Number,  // number
    String,  // string
    Boolean, // boolean
    Null,    // null
    Void,    // void
}

impl PrimitiveType {
    /// Get the string name of this primitive type
    pub fn name(&self) -> &'static str {
        match self {
            PrimitiveType::Number => "number",
            PrimitiveType::String => "string",
            PrimitiveType::Boolean => "boolean",
            PrimitiveType::Null => "null",
            PrimitiveType::Void => "void",
        }
    }
}

// ============================================================================
// Type Reference
// ============================================================================

/// Type reference: Point, Map<K, V>
#[derive(Debug, Clone, PartialEq)]
pub struct TypeReference {
    pub name: Identifier,
    pub type_args: Option<Vec<TypeAnnotation>>,
}

impl TypeReference {
    /// Create a simple type reference without type arguments
    pub fn simple(name: Identifier) -> Self {
        Self {
            name,
            type_args: None,
        }
    }

    /// Create a generic type reference with type arguments
    pub fn generic(name: Identifier, type_args: Vec<TypeAnnotation>) -> Self {
        Self {
            name,
            type_args: Some(type_args),
        }
    }

    /// Check if this is a generic type reference
    pub fn is_generic(&self) -> bool {
        self.type_args.is_some()
    }
}

// ============================================================================
// Union Type
// ============================================================================

/// Union type: A | B | C
#[derive(Debug, Clone, PartialEq)]
pub struct UnionType {
    pub types: Vec<TypeAnnotation>,
}

impl UnionType {
    /// Create a new union type
    pub fn new(types: Vec<TypeAnnotation>) -> Self {
        Self { types }
    }

    /// Get the number of types in this union
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Check if this union is empty
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Check if all types in this union are primitives (bare union)
    pub fn is_bare_union(&self) -> bool {
        self.types.iter().all(|t| t.ty.is_primitive())
    }
}

// ============================================================================
// Function Type
// ============================================================================

/// Function type: (x: number, y: string) => number
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
    pub params: Vec<FunctionTypeParam>,
    pub return_type: Box<TypeAnnotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionTypeParam {
    pub name: Option<Identifier>,
    pub ty: TypeAnnotation,
}

impl FunctionType {
    /// Get the number of parameters
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// Check if this function has no parameters
    pub fn is_nullary(&self) -> bool {
        self.params.is_empty()
    }
}

// ============================================================================
// Array Type
// ============================================================================

/// Array type: T[]
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayType {
    pub element_type: Box<TypeAnnotation>,
}

impl ArrayType {
    /// Create a new array type
    pub fn new(element_type: TypeAnnotation) -> Self {
        Self {
            element_type: Box::new(element_type),
        }
    }
}

// ============================================================================
// Tuple Type
// ============================================================================

/// Tuple type: [number, string, boolean]
#[derive(Debug, Clone, PartialEq)]
pub struct TupleType {
    pub element_types: Vec<TypeAnnotation>,
}

impl TupleType {
    /// Create a new tuple type
    pub fn new(element_types: Vec<TypeAnnotation>) -> Self {
        Self { element_types }
    }

    /// Get the number of elements in this tuple
    pub fn len(&self) -> usize {
        self.element_types.len()
    }

    /// Check if this tuple is empty
    pub fn is_empty(&self) -> bool {
        self.element_types.is_empty()
    }
}

// ============================================================================
// Object Type
// ============================================================================

/// Object type: { x: number; y: string }
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectType {
    pub members: Vec<ObjectTypeMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectTypeMember {
    Property(ObjectTypeProperty),
    Method(ObjectTypeMethod),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectTypeProperty {
    pub name: Identifier,
    pub ty: TypeAnnotation,
    pub optional: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectTypeMethod {
    pub name: Identifier,
    pub params: Vec<FunctionTypeParam>,
    pub return_type: TypeAnnotation,
    pub span: Span,
}

impl ObjectType {
    /// Create a new object type
    pub fn new(members: Vec<ObjectTypeMember>) -> Self {
        Self { members }
    }

    /// Get the number of members
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Check if this object type is empty
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

// ============================================================================
// Typeof Type
// ============================================================================

/// Typeof type: typeof value (for bare unions only)
#[derive(Debug, Clone, PartialEq)]
pub struct TypeofType {
    pub argument: Box<Expression>,
}

// ============================================================================
// Type Parameters (Generics)
// ============================================================================

/// Type parameter (generic): T, K extends string
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParameter {
    pub name: Identifier,
    pub constraint: Option<TypeAnnotation>,
    pub default: Option<TypeAnnotation>,
    pub span: Span,
}

impl TypeParameter {
    /// Create a simple type parameter without constraints or defaults
    pub fn simple(name: Identifier, span: Span) -> Self {
        Self {
            name,
            constraint: None,
            default: None,
            span,
        }
    }

    /// Check if this type parameter has a constraint
    pub fn is_constrained(&self) -> bool {
        self.constraint.is_some()
    }

    /// Check if this type parameter has a default
    pub fn has_default(&self) -> bool {
        self.default.is_some()
    }
}
