//! Core type definitions for the Raya type system

use std::fmt;

/// Unique identifier for a type in the type context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub(crate) u32);

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypeId({})", self.0)
    }
}

/// Primitive types in Raya
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    /// The `number` type (IEEE 754 double precision)
    Number,
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
            PrimitiveType::String => write!(f, "string"),
            PrimitiveType::Boolean => write!(f, "boolean"),
            PrimitiveType::Null => write!(f, "null"),
            PrimitiveType::Void => write!(f, "void"),
        }
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
    /// Optional discriminant field name for discriminated unions
    pub discriminant: Option<String>,
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
}

/// Array type: T[]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArrayType {
    /// Element type
    pub element: TypeId,
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
}

/// Class type (nominal typing)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassType {
    /// Class name
    pub name: String,
    /// Type parameters for generic classes
    pub type_params: Vec<String>,
    /// Properties
    pub properties: Vec<PropertySignature>,
    /// Methods
    pub methods: Vec<MethodSignature>,
    /// Parent class (if any)
    pub extends: Option<TypeId>,
    /// Implemented interfaces
    pub implements: Vec<TypeId>,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_display() {
        assert_eq!(format!("{}", PrimitiveType::Number), "number");
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
