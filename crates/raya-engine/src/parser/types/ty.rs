//! Core type definitions for the Raya type system

use super::discriminant::Discriminant;
use std::fmt;

/// Unique identifier for a type in the type context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub(crate) u32);

impl TypeId {
    /// Create a new TypeId from a raw value
    ///
    /// Note: This should generally only be used internally or for interop.
    /// Prefer using TypeContext methods to get well-known type IDs.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw value of this TypeId
    pub const fn as_u32(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypeId({})", self.0)
    }
}

/// Primitive types in Raya
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    /// The `number` type (IEEE 754 double precision, f64). `float` is an alias.
    Number,
    /// The `int` type (32-bit signed integer, i32)
    Int,
    /// The `string` type
    String,
    /// The `boolean` type
    Boolean,
    /// The `null` type
    Null,
    /// The `void` type (for functions with no return value)
    Void,
}

impl fmt::Display for PrimitiveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrimitiveType::Number => write!(f, "number"),
            PrimitiveType::Int => write!(f, "int"),
            PrimitiveType::String => write!(f, "string"),
            PrimitiveType::Boolean => write!(f, "boolean"),
            PrimitiveType::Null => write!(f, "null"),
            PrimitiveType::Void => write!(f, "void"),
        }
    }
}

impl PrimitiveType {
    /// Get the string representation for typeof operator
    pub fn type_name(&self) -> &'static str {
        match self {
            PrimitiveType::Number => "number",
            PrimitiveType::Int => "int",
            PrimitiveType::String => "string",
            PrimitiveType::Boolean => "boolean",
            PrimitiveType::Null => "null",
            PrimitiveType::Void => "void",
        }
    }

    /// Check if this is a valid bare union primitive
    ///
    /// Bare unions can only contain: number, string, boolean, null
    /// Void is excluded because it doesn't represent a value type.
    pub fn is_bare_union_primitive(&self) -> bool {
        matches!(
            self,
            PrimitiveType::Number
                | PrimitiveType::Int
                | PrimitiveType::String
                | PrimitiveType::Boolean
                | PrimitiveType::Null
        )
    }
}

/// Type reference to a named type (class, interface, type alias)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeReference {
    /// Name of the referenced type
    pub name: String,
    /// Type arguments for generic types
    pub type_args: Option<Vec<TypeId>>,
}

/// Union type: T1 | T2 | ... | Tn
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnionType {
    /// Members of the union
    pub members: Vec<TypeId>,
    /// Optional discriminant information for discriminated unions
    pub discriminant: Option<Discriminant>,
    /// Flag indicating if this is a bare primitive union
    pub is_bare: bool,
    /// Internal representation for bare unions (transformed to discriminated union)
    pub internal_union: Option<TypeId>,
}

/// Function type: (T1, T2, ..., Tn) => R
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionType {
    /// Parameter types
    pub params: Vec<TypeId>,
    /// Return type
    pub return_type: TypeId,
    /// Whether this is an async function (returns Task<R>)
    pub is_async: bool,
    /// Minimum number of required parameters (params without default values)
    pub min_params: usize,
    /// Rest parameter type (if present), e.g., Some(string[]) for ...args: string[]
    pub rest_param: Option<TypeId>,
}

/// Array type: T[]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArrayType {
    /// Element type
    pub element: TypeId,
}

/// Task type: Task<T> - represents an async computation that yields T
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskType {
    /// Result type of the task
    pub result: TypeId,
}

/// Map type: Map<K, V> - key-value dictionary
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapType {
    /// Key type
    pub key: TypeId,
    /// Value type
    pub value: TypeId,
}

/// Set type: Set<T> - collection of unique values
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SetType {
    /// Element type
    pub element: TypeId,
}

/// Channel type: Channel<T> - inter-task communication
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelType {
    /// Message type
    pub message: TypeId,
}

/// Tuple type: [T1, T2, ..., Tn]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TupleType {
    /// Element types
    pub elements: Vec<TypeId>,
}

/// Object type property
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PropertySignature {
    /// Property name
    pub name: String,
    /// Property type
    pub ty: TypeId,
    /// Whether the property is optional
    pub optional: bool,
    /// Whether the property is readonly
    pub readonly: bool,
    /// Visibility (private/protected/public) — only meaningful for class fields
    pub visibility: crate::parser::ast::Visibility,
}

/// Object type: { prop1: T1, prop2: T2, ... }
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectType {
    /// Object properties
    pub properties: Vec<PropertySignature>,
    /// Index signature: [key: string]: T
    pub index_signature: Option<(String, TypeId)>,
}

/// Method signature
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodSignature {
    /// Method name
    pub name: String,
    /// Method type (should be a FunctionType)
    pub ty: TypeId,
    /// Type parameters for generic methods (e.g., `withLock<R>`)
    pub type_params: Vec<String>,
    /// Visibility (private/protected/public)
    pub visibility: crate::parser::ast::Visibility,
}

/// Class type (nominal typing)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassType {
    /// Class name
    pub name: String,
    /// Type parameters for generic classes
    pub type_params: Vec<String>,
    /// Instance properties
    pub properties: Vec<PropertySignature>,
    /// Instance methods
    pub methods: Vec<MethodSignature>,
    /// Static properties
    pub static_properties: Vec<PropertySignature>,
    /// Static methods
    pub static_methods: Vec<MethodSignature>,
    /// Parent class (if any)
    pub extends: Option<TypeId>,
    /// Implemented interfaces
    pub implements: Vec<TypeId>,
    /// Whether this is an abstract class (cannot be instantiated directly)
    pub is_abstract: bool,
}

/// Interface type (structural typing for object shapes)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceType {
    /// Interface name
    pub name: String,
    /// Type parameters for generic interfaces
    pub type_params: Vec<String>,
    /// Properties
    pub properties: Vec<PropertySignature>,
    /// Methods
    pub methods: Vec<MethodSignature>,
    /// Extended interfaces
    pub extends: Vec<TypeId>,
}

/// Type variable for generics: T, U, etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeVar {
    /// Type variable name
    pub name: String,
    /// Constraint: T extends C
    pub constraint: Option<TypeId>,
    /// Default type: T = D
    pub default: Option<TypeId>,
}

/// Generic type instantiation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericType {
    /// Base type (must be a TypeReference with type parameters)
    pub base: TypeId,
    /// Concrete type arguments
    pub type_args: Vec<TypeId>,
}

/// The core type representation in Raya
#[derive(Debug, Clone)]
pub enum Type {
    /// Primitive type (number, string, boolean, null, void)
    Primitive(PrimitiveType),

    /// Type reference (class, interface, type alias)
    Reference(TypeReference),

    /// Union type: T1 | T2 | ... | Tn
    Union(UnionType),

    /// Function type: (params) => return
    Function(FunctionType),

    /// Array type: T[]
    Array(ArrayType),

    /// Task type: Task<T>
    Task(TaskType),

    /// Mutex type: Mutex
    Mutex,

    /// RegExp type: RegExp (regular expression primitive)
    RegExp,

    /// Channel type: Channel<T> for inter-task communication
    Channel(ChannelType),

    /// Map type: Map<K, V> key-value dictionary
    Map(MapType),

    /// Set type: Set<T> collection of unique values
    Set(SetType),

    /// Date type: Date for date/time handling
    Date,

    /// Buffer type: Buffer for raw binary data
    Buffer,

    /// JSON type: dynamic JSON value from JSON.parse()
    /// Supports duck typing - property access returns json, not a fixed type
    Json,

    /// Tuple type: [T1, T2, ..., Tn]
    Tuple(TupleType),

    /// Object type: { prop: T }
    Object(ObjectType),

    /// Class type (nominal)
    Class(ClassType),

    /// Interface type (structural)
    Interface(InterfaceType),

    /// Type variable: T
    TypeVar(TypeVar),

    /// Generic instantiation: Map<string, number>
    Generic(GenericType),

    /// String literal type: "hello"
    StringLiteral(String),

    /// Number literal type: 42, 3.14
    NumberLiteral(f64),

    /// Boolean literal type: true, false
    BooleanLiteral(bool),

    /// Bottom type (unreachable code)
    Never,

    /// Top type (unknown type)
    Unknown,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Primitive(p) => write!(f, "{}", p),
            Type::Reference(r) => {
                write!(f, "{}", r.name)?;
                if let Some(args) = &r.type_args {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            Type::Union(u) => {
                for (i, member) in u.members.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{}", member)?;
                }
                Ok(())
            }
            Type::Function(func) => {
                write!(f, "(")?;
                for (i, param) in func.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ") => ")?;
                if func.is_async {
                    write!(f, "Task<{}>", func.return_type)
                } else {
                    write!(f, "{}", func.return_type)
                }
            }
            Type::Array(a) => write!(f, "{}[]", a.element),
            Type::Task(t) => write!(f, "Task<{}>", t.result),
            Type::Mutex => write!(f, "Mutex"),
            Type::RegExp => write!(f, "RegExp"),
            Type::Channel(c) => write!(f, "Channel<{}>", c.message),
            Type::Map(m) => write!(f, "Map<{}, {}>", m.key, m.value),
            Type::Set(s) => write!(f, "Set<{}>", s.element),
            Type::Date => write!(f, "Date"),
            Type::Buffer => write!(f, "Buffer"),
            Type::Json => write!(f, "json"),
            Type::Tuple(t) => {
                write!(f, "[")?;
                for (i, elem) in t.elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, "]")
            }
            Type::Object(o) => {
                write!(f, "{{ ")?;
                for (i, prop) in o.properties.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if prop.readonly {
                        write!(f, "readonly ")?;
                    }
                    write!(f, "{}", prop.name)?;
                    if prop.optional {
                        write!(f, "?")?;
                    }
                    write!(f, ": {}", prop.ty)?;
                }
                write!(f, " }}")
            }
            Type::Class(c) => {
                write!(f, "class {}", c.name)?;
                if !c.type_params.is_empty() {
                    write!(f, "<")?;
                    for (i, param) in c.type_params.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", param)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            Type::Interface(i) => {
                write!(f, "interface {}", i.name)?;
                if !i.type_params.is_empty() {
                    write!(f, "<")?;
                    for (i, param) in i.type_params.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", param)?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            Type::TypeVar(tv) => {
                write!(f, "{}", tv.name)?;
                if let Some(constraint) = &tv.constraint {
                    write!(f, " extends {}", constraint)?;
                }
                Ok(())
            }
            Type::Generic(g) => {
                write!(f, "{}[", g.base)?;
                for (i, arg) in g.type_args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, "]")
            }
            Type::StringLiteral(s) => write!(f, "\"{}\"", s),
            Type::NumberLiteral(n) => write!(f, "{}", n),
            Type::BooleanLiteral(b) => write!(f, "{}", b),
            Type::Never => write!(f, "never"),
            Type::Unknown => write!(f, "unknown"),
        }
    }
}

impl Type {
    /// Check if this type is a primitive type
    pub fn is_primitive(&self) -> bool {
        matches!(self, Type::Primitive(_))
    }

    /// Check if this type is a function type
    pub fn is_function(&self) -> bool {
        matches!(self, Type::Function(_))
    }

    /// Check if this type is a union type
    pub fn is_union(&self) -> bool {
        matches!(self, Type::Union(_))
    }

    /// Check if this type is the never type
    pub fn is_never(&self) -> bool {
        matches!(self, Type::Never)
    }

    /// Check if this type is the unknown type
    pub fn is_unknown(&self) -> bool {
        matches!(self, Type::Unknown)
    }

    /// Check if this type is the json type (dynamic JSON value)
    pub fn is_json(&self) -> bool {
        matches!(self, Type::Json)
    }

    /// Get the primitive type if this is a primitive
    pub fn as_primitive(&self) -> Option<PrimitiveType> {
        match self {
            Type::Primitive(p) => Some(*p),
            _ => None,
        }
    }

    /// Get the union type if this is a union
    pub fn as_union(&self) -> Option<&UnionType> {
        match self {
            Type::Union(u) => Some(u),
            _ => None,
        }
    }

    /// Get the function type if this is a function
    pub fn as_function(&self) -> Option<&FunctionType> {
        match self {
            Type::Function(f) => Some(f),
            _ => None,
        }
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Type::Primitive(a), Type::Primitive(b)) => a == b,
            (Type::Reference(a), Type::Reference(b)) => a == b,
            (Type::Union(a), Type::Union(b)) => a == b,
            (Type::Function(a), Type::Function(b)) => a == b,
            (Type::Array(a), Type::Array(b)) => a == b,
            (Type::Task(a), Type::Task(b)) => a == b,
            (Type::Mutex, Type::Mutex) => true,
            (Type::RegExp, Type::RegExp) => true,
            (Type::Channel(a), Type::Channel(b)) => a == b,
            (Type::Map(a), Type::Map(b)) => a == b,
            (Type::Set(a), Type::Set(b)) => a == b,
            (Type::Date, Type::Date) => true,
            (Type::Buffer, Type::Buffer) => true,
            (Type::Json, Type::Json) => true,
            (Type::Tuple(a), Type::Tuple(b)) => a == b,
            (Type::Object(a), Type::Object(b)) => a == b,
            (Type::Class(a), Type::Class(b)) => a == b,
            (Type::Interface(a), Type::Interface(b)) => a == b,
            (Type::TypeVar(a), Type::TypeVar(b)) => a == b,
            (Type::Generic(a), Type::Generic(b)) => a == b,
            (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
            (Type::NumberLiteral(a), Type::NumberLiteral(b)) => {
                // Compare f64 by bits for exact equality
                a.to_bits() == b.to_bits()
            }
            (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,
            (Type::Never, Type::Never) => true,
            (Type::Unknown, Type::Unknown) => true,
            _ => false,
        }
    }
}

impl Eq for Type {}

impl std::hash::Hash for Type {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash discriminant
        std::mem::discriminant(self).hash(state);

        match self {
            Type::Primitive(p) => p.hash(state),
            Type::Reference(r) => r.hash(state),
            Type::Union(u) => u.hash(state),
            Type::Function(f) => f.hash(state),
            Type::Array(a) => a.hash(state),
            Type::Task(t) => t.hash(state),
            Type::Mutex => {}
            Type::RegExp => {}
            Type::Channel(c) => c.hash(state),
            Type::Map(m) => m.hash(state),
            Type::Set(s) => s.hash(state),
            Type::Date => {}
            Type::Buffer => {}
            Type::Json => {}
            Type::Tuple(t) => t.hash(state),
            Type::Object(o) => o.hash(state),
            Type::Class(c) => c.hash(state),
            Type::Interface(i) => i.hash(state),
            Type::TypeVar(tv) => tv.hash(state),
            Type::Generic(g) => g.hash(state),
            Type::StringLiteral(s) => s.hash(state),
            Type::NumberLiteral(n) => {
                // Hash f64 by converting to bits (this is safe for equality)
                n.to_bits().hash(state);
            }
            Type::BooleanLiteral(b) => b.hash(state),
            Type::Never => {}
            Type::Unknown => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_display() {
        assert_eq!(format!("{}", PrimitiveType::Number), "number");
        assert_eq!(format!("{}", PrimitiveType::Int), "int");
        assert_eq!(format!("{}", PrimitiveType::String), "string");
        assert_eq!(format!("{}", PrimitiveType::Boolean), "boolean");
        assert_eq!(format!("{}", PrimitiveType::Null), "null");
        assert_eq!(format!("{}", PrimitiveType::Void), "void");
    }

    #[test]
    fn test_type_display_primitive() {
        let ty = Type::Primitive(PrimitiveType::Number);
        assert_eq!(format!("{}", ty), "number");
    }

    #[test]
    fn test_type_is_methods() {
        let num_ty = Type::Primitive(PrimitiveType::Number);
        assert!(num_ty.is_primitive());
        assert!(!num_ty.is_function());
        assert!(!num_ty.is_union());

        let never_ty = Type::Never;
        assert!(never_ty.is_never());
        assert!(!never_ty.is_unknown());

        let unknown_ty = Type::Unknown;
        assert!(unknown_ty.is_unknown());
        assert!(!unknown_ty.is_never());
    }

    #[test]
    fn test_type_as_methods() {
        let num_ty = Type::Primitive(PrimitiveType::Number);
        assert_eq!(num_ty.as_primitive(), Some(PrimitiveType::Number));
        assert!(num_ty.as_union().is_none());
        assert!(num_ty.as_function().is_none());
    }
}
