//! Type context for managing types and type interning

use super::ty::{Type, TypeId};
use super::error::TypeError;
use super::discriminant::{Discriminant, DiscriminantInference};
use rustc_hash::FxHashMap;
use std::sync::Arc;

/// Type context that manages all types in a program
///
/// This uses type interning to ensure that identical types have the same TypeId,
/// which enables efficient equality checking and memory usage.
#[derive(Debug, Clone)]
pub struct TypeContext {
    /// Storage for all types, indexed by TypeId
    types: Vec<Arc<Type>>,

    /// Reverse mapping from Type to TypeId for interning
    type_to_id: FxHashMap<Type, TypeId>,

    /// Named type definitions (type aliases, classes, interfaces)
    named_types: FxHashMap<String, TypeId>,
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeContext {
    /// Create a new empty type context
    pub fn new() -> Self {
        let mut ctx = TypeContext {
            types: Vec::new(),
            type_to_id: FxHashMap::default(),
            named_types: FxHashMap::default(),
        };

        // Pre-intern common primitive types
        use super::ty::PrimitiveType;
        ctx.intern(Type::Primitive(PrimitiveType::Number));
        ctx.intern(Type::Primitive(PrimitiveType::String));
        ctx.intern(Type::Primitive(PrimitiveType::Boolean));
        ctx.intern(Type::Primitive(PrimitiveType::Null));
        ctx.intern(Type::Primitive(PrimitiveType::Void));
        ctx.intern(Type::Never);
        ctx.intern(Type::Unknown);

        ctx
    }

    /// Intern a type, returning its TypeId
    ///
    /// If the type already exists, returns the existing TypeId.
    /// Otherwise, allocates a new TypeId and stores the type.
    pub fn intern(&mut self, ty: Type) -> TypeId {
        if let Some(&id) = self.type_to_id.get(&ty) {
            return id;
        }

        let id = TypeId(self.types.len() as u32);
        self.types.push(Arc::new(ty.clone()));
        self.type_to_id.insert(ty, id);
        id
    }

    /// Get a type by its TypeId
    pub fn get(&self, id: TypeId) -> Option<&Type> {
        self.types.get(id.0 as usize).map(|arc| arc.as_ref())
    }

    /// Get a type by its TypeId, panicking if it doesn't exist
    ///
    /// # Panics
    ///
    /// Panics if the TypeId is invalid
    pub fn get_unchecked(&self, id: TypeId) -> &Type {
        self.get(id).expect("Invalid TypeId")
    }

    /// Register a named type (type alias, class, interface)
    pub fn register_named_type(&mut self, name: String, ty: TypeId) {
        self.named_types.insert(name, ty);
    }

    /// Look up a named type by name
    pub fn lookup_named_type(&self, name: &str) -> Option<TypeId> {
        self.named_types.get(name).copied()
    }

    /// Resolve a named type, returning an error if not found
    pub fn resolve_named_type(&self, name: &str) -> Result<TypeId, TypeError> {
        self.lookup_named_type(name).ok_or_else(|| TypeError::UndefinedType {
            name: name.to_string(),
        })
    }

    // Convenience methods for creating common types

    /// Get the number type
    pub fn number_type(&mut self) -> TypeId {
        self.intern(Type::Primitive(super::ty::PrimitiveType::Number))
    }

    /// Get the string type
    pub fn string_type(&mut self) -> TypeId {
        self.intern(Type::Primitive(super::ty::PrimitiveType::String))
    }

    /// Get the boolean type
    pub fn boolean_type(&mut self) -> TypeId {
        self.intern(Type::Primitive(super::ty::PrimitiveType::Boolean))
    }

    /// Get the null type
    pub fn null_type(&mut self) -> TypeId {
        self.intern(Type::Primitive(super::ty::PrimitiveType::Null))
    }

    /// Get the void type
    pub fn void_type(&mut self) -> TypeId {
        self.intern(Type::Primitive(super::ty::PrimitiveType::Void))
    }

    /// Get the never type
    pub fn never_type(&mut self) -> TypeId {
        self.intern(Type::Never)
    }

    /// Get the unknown type
    pub fn unknown_type(&mut self) -> TypeId {
        self.intern(Type::Unknown)
    }

    /// Create a string literal type
    pub fn string_literal(&mut self, value: impl Into<String>) -> TypeId {
        self.intern(Type::StringLiteral(value.into()))
    }

    /// Create a number literal type
    pub fn number_literal(&mut self, value: f64) -> TypeId {
        self.intern(Type::NumberLiteral(value))
    }

    /// Create a boolean literal type
    pub fn boolean_literal(&mut self, value: bool) -> TypeId {
        self.intern(Type::BooleanLiteral(value))
    }

    /// Create a type variable (type parameter)
    pub fn type_variable(&mut self, name: impl Into<String>) -> TypeId {
        self.intern(Type::TypeVar(super::ty::TypeVar {
            name: name.into(),
            constraint: None,
            default: None,
        }))
    }

    /// Create an array type
    pub fn array_type(&mut self, element: TypeId) -> TypeId {
        self.intern(Type::Array(super::ty::ArrayType { element }))
    }

    /// Create a task type (for async functions)
    pub fn task_type(&mut self, result: TypeId) -> TypeId {
        self.intern(Type::Task(super::ty::TaskType { result }))
    }

    /// Create a tuple type
    pub fn tuple_type(&mut self, elements: Vec<TypeId>) -> TypeId {
        self.intern(Type::Tuple(super::ty::TupleType { elements }))
    }

    /// Create a function type
    pub fn function_type(&mut self, params: Vec<TypeId>, return_type: TypeId, is_async: bool) -> TypeId {
        self.intern(Type::Function(super::ty::FunctionType {
            params,
            return_type,
            is_async,
        }))
    }

    /// Create a union type
    ///
    /// Automatically infers discriminant field if the union is discriminated.
    pub fn union_type(&mut self, members: Vec<TypeId>) -> TypeId {
        // Normalize: remove duplicates and flatten nested unions
        let mut normalized_members = Vec::new();
        for &member in &members {
            if let Some(Type::Union(u)) = self.get(member) {
                // Flatten nested union
                normalized_members.extend_from_slice(&u.members);
            } else {
                normalized_members.push(member);
            }
        }

        // Remove duplicates
        normalized_members.sort_unstable_by_key(|id| id.0);
        normalized_members.dedup();

        // Single member union is just the member
        if normalized_members.len() == 1 {
            return normalized_members[0];
        }

        // Check if this is a bare primitive union
        let detector = super::bare_union::BareUnionDetector::new(self);
        let is_bare = detector.is_bare_primitive_union(&normalized_members);

        let (discriminant, internal_union) = if is_bare {
            // Transform bare union to internal representation
            let primitives = detector.extract_primitives(&normalized_members);

            // Validate no duplicates
            if let Err(_) = detector.validate_no_duplicates(&primitives) {
                // If there are duplicates, treat as regular union (will error later)
                let disc = {
                    let inference = DiscriminantInference::new(self);
                    inference.infer(&normalized_members).ok()
                };
                (disc, None)
            } else {
                // Transform to internal discriminated union
                let mut transform = super::bare_union::BareUnionTransform::new(self);
                let internal = transform.transform(&primitives);
                (None, Some(internal))
            }
        } else {
            // Try to infer discriminant for regular discriminated unions
            let disc = {
                let inference = DiscriminantInference::new(self);
                inference.infer(&normalized_members).ok()
            };
            (disc, None)
        };

        self.intern(Type::Union(super::ty::UnionType {
            members: normalized_members,
            discriminant,
            is_bare,
            internal_union,
        }))
    }

    /// Get discriminant information for a union type
    pub fn get_discriminant(&self, union_id: TypeId) -> Option<&Discriminant> {
        if let Some(Type::Union(union)) = self.get(union_id) {
            union.discriminant.as_ref()
        } else {
            None
        }
    }

    /// Get the internal representation of a bare union
    ///
    /// For bare unions, this returns the internal discriminated union TypeId.
    /// Returns None if the type is not a bare union or doesn't have an internal representation.
    pub fn get_bare_union_internal(&self, union_id: TypeId) -> Option<TypeId> {
        if let Some(Type::Union(union)) = self.get(union_id) {
            if union.is_bare {
                union.internal_union
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get a display string for a type
    pub fn display(&self, id: TypeId) -> String {
        self.get(id)
            .map(|ty| format!("{}", ty))
            .unwrap_or_else(|| format!("InvalidType({})", id.0))
    }

    /// Get the number of types in the context
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Check if the context is empty
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ty::{PrimitiveType, Type};

    #[test]
    fn test_type_interning() {
        let mut ctx = TypeContext::new();

        let num1 = ctx.number_type();
        let num2 = ctx.number_type();

        // Same type should have same ID
        assert_eq!(num1, num2);
    }

    #[test]
    fn test_primitive_types() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        let str = ctx.string_type();
        let bool = ctx.boolean_type();
        let null = ctx.null_type();
        let void = ctx.void_type();

        assert_eq!(ctx.get(num), Some(&Type::Primitive(PrimitiveType::Number)));
        assert_eq!(ctx.get(str), Some(&Type::Primitive(PrimitiveType::String)));
        assert_eq!(ctx.get(bool), Some(&Type::Primitive(PrimitiveType::Boolean)));
        assert_eq!(ctx.get(null), Some(&Type::Primitive(PrimitiveType::Null)));
        assert_eq!(ctx.get(void), Some(&Type::Primitive(PrimitiveType::Void)));
    }

    #[test]
    fn test_array_type() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        let arr = ctx.array_type(num);

        match ctx.get(arr) {
            Some(Type::Array(a)) => assert_eq!(a.element, num),
            _ => panic!("Expected array type"),
        }
    }

    #[test]
    fn test_tuple_type() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        let str = ctx.string_type();
        let tuple = ctx.tuple_type(vec![num, str]);

        match ctx.get(tuple) {
            Some(Type::Tuple(t)) => {
                assert_eq!(t.elements.len(), 2);
                assert_eq!(t.elements[0], num);
                assert_eq!(t.elements[1], str);
            }
            _ => panic!("Expected tuple type"),
        }
    }

    #[test]
    fn test_function_type() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        let str = ctx.string_type();
        let func = ctx.function_type(vec![num, str], num, false);

        match ctx.get(func) {
            Some(Type::Function(f)) => {
                assert_eq!(f.params.len(), 2);
                assert_eq!(f.params[0], num);
                assert_eq!(f.params[1], str);
                assert_eq!(f.return_type, num);
                assert!(!f.is_async);
            }
            _ => panic!("Expected function type"),
        }
    }

    #[test]
    fn test_union_type() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        let str = ctx.string_type();
        let union = ctx.union_type(vec![num, str]);

        match ctx.get(union) {
            Some(Type::Union(u)) => {
                assert_eq!(u.members.len(), 2);
                assert!(u.members.contains(&num));
                assert!(u.members.contains(&str));
            }
            _ => panic!("Expected union type"),
        }
    }

    #[test]
    fn test_union_flattening() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        let str = ctx.string_type();
        let bool = ctx.boolean_type();

        // Create num | str
        let union1 = ctx.union_type(vec![num, str]);

        // Create (num | str) | bool - should flatten to num | str | bool
        let union2 = ctx.union_type(vec![union1, bool]);

        match ctx.get(union2) {
            Some(Type::Union(u)) => {
                assert_eq!(u.members.len(), 3);
                assert!(u.members.contains(&num));
                assert!(u.members.contains(&str));
                assert!(u.members.contains(&bool));
            }
            _ => panic!("Expected union type"),
        }
    }

    #[test]
    fn test_union_deduplication() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        let str = ctx.string_type();

        // Create num | str | num - should deduplicate to num | str
        let union = ctx.union_type(vec![num, str, num]);

        match ctx.get(union) {
            Some(Type::Union(u)) => {
                assert_eq!(u.members.len(), 2);
                assert!(u.members.contains(&num));
                assert!(u.members.contains(&str));
            }
            _ => panic!("Expected union type"),
        }
    }

    #[test]
    fn test_single_member_union() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();

        // Single member union should just return the member
        let union = ctx.union_type(vec![num]);
        assert_eq!(union, num);
    }

    #[test]
    fn test_named_types() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        ctx.register_named_type("MyNumber".to_string(), num);

        assert_eq!(ctx.lookup_named_type("MyNumber"), Some(num));
        assert_eq!(ctx.lookup_named_type("Unknown"), None);
    }

    #[test]
    fn test_resolve_named_type() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        ctx.register_named_type("MyNumber".to_string(), num);

        assert_eq!(ctx.resolve_named_type("MyNumber"), Ok(num));
        assert!(ctx.resolve_named_type("Unknown").is_err());
    }

    #[test]
    fn test_display() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        assert_eq!(ctx.display(num), "number");

        let arr = ctx.array_type(num);
        let arr_display = ctx.display(arr);
        eprintln!("Array display: {}", arr_display);
        // Array type displays as TypeId(N)[], not number[]
        // because Display for Type shows TypeId, not the actual type
        assert!(arr_display.contains("[]"));
    }
}
