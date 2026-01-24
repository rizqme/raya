# Milestone 2.4: Type System Foundation

**Duration:** 2-3 weeks
**Status:** ✅ Complete (All 4 Phases)
**Completion Date:** 2026-01-24
**Dependencies:** Milestone 2.3 (Parser) ✅ Complete
**Next Milestone:** 2.5 (Type Checker & Control Flow Analysis)

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Non-Goals](#non-goals)
4. [Design Principles](#design-principles)
5. [Phase 1: Type Representation](#phase-1-type-representation-days-1-4)
6. [Phase 2: Type Operations](#phase-2-type-operations-days-5-8)
7. [Phase 3: Generic Type System](#phase-3-generic-type-system-days-9-12)
8. [Phase 4: Utilities & Testing](#phase-4-utilities--testing-days-13-15)
9. [Success Criteria](#success-criteria)
10. [Testing Strategy](#testing-strategy)
11. [References](#references)

---

## Overview

Implement the foundational type system infrastructure for Raya, including type representation, subtyping rules, type operations, and generic type handling. This milestone provides the building blocks for the type checker (Milestone 2.5).

### What is a Type System?

A type system is a formal system that assigns types to program constructs and defines rules for how types interact. The type system foundation provides:
- **Type Representation**: Data structures for all Raya types
- **Subtyping**: Rules for when one type is a subtype of another
- **Type Operations**: Union, intersection, normalization
- **Generic Types**: Parameterized types with constraints

### Key Difference from TypeScript

Raya's type system is **fully static** with:
- **No `any` type** - every value has a specific type
- **No type assertions** (`as`) - except for safe casts with runtime checks
- **No escape hatches** - sound type system with no workarounds
- **Discriminated unions** - all union types must be discriminated (except bare primitives)
- **Implicit primitive coercions** - `number → string` is automatic

---

## Goals

### Primary Goals

1. **Complete Type Representation**: Define data structures for all Raya types
2. **Subtyping Rules**: Implement subtype checking for all type combinations
3. **Type Operations**: Union, intersection, normalization, simplification
4. **Generic Type System**: Type variables, constraints, instantiation
5. **Type Equality**: Structural equality for types
6. **Type Display**: Pretty-printing types for error messages

### Secondary Goals

1. **Type Normalization**: Simplify complex types to canonical forms
2. **Type Caching**: Efficient type storage and lookup
3. **Error Messages**: Clear type mismatch descriptions
4. **Documentation**: Comprehensive API docs for type system

---

## Non-Goals

1. **Type Checking**: Validating programs (Milestone 2.5)
2. **Type Inference**: Inferring types from expressions (Milestone 2.5)
3. **Control Flow Analysis**: Type narrowing (Milestone 2.5)
4. **Symbol Tables**: Name resolution (Milestone 2.5)
5. **Exhaustiveness Checking**: Discriminated union coverage (Milestone 2.5)

---

## Design Principles

### 1. Sound Type System

No `any` type, no escape hatches, no unsound casts:
- Every value has a specific type
- Type system guarantees are never violated
- Runtime checks inserted for JSON operations only

### 2. Structural Typing

Types are compared structurally, not nominally:
- Object types compared by their shape
- Classes use structural subtyping for interfaces
- Nominal typing only for classes with inheritance

### 3. Discriminated Unions

All union types must be discriminated (except bare primitives):
```typescript
// ✅ ALLOWED: Bare primitive union
type ID = string | number;  // OK: primitives use typeof

// ✅ REQUIRED: Discriminated union for objects
type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };

// ❌ ERROR: Non-discriminated object union
type Bad = { x: number } | { y: string };  // Rejected
```

### 4. Implicit Primitive Coercions

Primitives can be coerced implicitly:
```typescript
// number → string (automatic)
let x: string | number = 42;
function fn(s: string): void {}
fn(x);  // OK: number coerced to string

// string → number (ERROR)
let y: string | number = "hello";
function gn(n: number): void {}
gn(y);  // ERROR: Cannot coerce string to number
```

Coercion rules:
- `number → string` ✅
- `boolean → string` ✅
- `null → string` ✅ (becomes "null")
- `string → number` ❌
- `string → boolean` ❌

### 5. Monomorphization

Generics are specialized at compile time:
- Type variables resolved to concrete types
- Each instantiation generates separate code
- No runtime type parameters

---

## Phase 1: Type Representation (Days 1-4) ✅ Complete

**Completion Date:** 2026-01-24

### Goal
Define data structures for all Raya types with efficient representation and operations.

### Deliverables

#### 1.1 Core Type System (`raya-types/src/types.rs`)

**Type enum:**
```rust
use std::sync::Arc;
use rustc_hash::FxHashMap;

/// Core type representation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// Primitive types: number, string, boolean, null, void
    Primitive(PrimitiveType),

    /// Type reference: MyClass, Point<T>
    Reference(TypeReference),

    /// Union type: number | string | null
    Union(UnionType),

    /// Function type: (x: number) => string
    Function(FunctionType),

    /// Array type: number[], Array<string>
    Array(ArrayType),

    /// Tuple type: [number, string]
    Tuple(TupleType),

    /// Object type: { x: number; y: string }
    Object(ObjectType),

    /// Class type (nominal)
    Class(ClassType),

    /// Interface type (structural - deprecated, use type aliases)
    Interface(InterfaceType),

    /// Type variable (for generics): T, K extends string
    TypeVar(TypeVar),

    /// Generic instantiation: Array<number>, Map<K, V>
    Generic(GenericType),

    /// Never type (bottom type)
    Never,

    /// Unknown type (top type)
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Number,   // Unified number type (int or float at runtime)
    String,
    Boolean,
    Null,
    Void,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeReference {
    pub name: String,
    pub type_args: Option<Vec<Type>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnionType {
    pub variants: Vec<Type>,
    /// Discriminant info (if not a bare union)
    pub discriminant: Option<DiscriminantInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiscriminantInfo {
    pub field: String,
    pub values: FxHashMap<String, Type>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionType {
    pub params: Vec<FunctionParam>,
    pub return_type: Box<Type>,
    pub is_async: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionParam {
    pub name: Option<String>,
    pub ty: Type,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArrayType {
    pub element_type: Box<Type>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TupleType {
    pub element_types: Vec<Type>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectType {
    pub properties: Vec<Property>,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Property {
    pub name: String,
    pub ty: Type,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Method {
    pub name: String,
    pub type_params: Option<Vec<TypeParam>>,
    pub params: Vec<FunctionParam>,
    pub return_type: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassType {
    pub name: String,
    pub type_params: Option<Vec<TypeParam>>,
    pub super_class: Option<Box<Type>>,
    pub implements: Vec<Type>,
    pub fields: Vec<Property>,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceType {
    pub name: String,
    pub type_params: Option<Vec<TypeParam>>,
    pub extends: Vec<Type>,
    pub members: Vec<InterfaceMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InterfaceMember {
    Property(Property),
    Method(Method),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeVar {
    pub name: String,
    pub constraint: Option<Box<Type>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeParam {
    pub name: String,
    pub constraint: Option<Type>,
    pub default: Option<Type>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericType {
    pub base: Box<Type>,
    pub type_args: Vec<Type>,
}
```

#### 1.2 Type Display (`raya-types/src/display.rs`)

**Pretty-printing types for error messages:**
```rust
use std::fmt;

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Type::Primitive(p) => write!(f, "{}", p),
            Type::Union(u) => {
                let variants: Vec<_> = u.variants.iter()
                    .map(|t| t.to_string())
                    .collect();
                write!(f, "{}", variants.join(" | "))
            }
            Type::Function(func) => {
                let params: Vec<_> = func.params.iter()
                    .map(|p| match &p.name {
                        Some(n) => format!("{}: {}", n, p.ty),
                        None => p.ty.to_string(),
                    })
                    .collect();
                write!(f, "({}) => {}", params.join(", "), func.return_type)
            }
            Type::Array(arr) => write!(f, "{}[]", arr.element_type),
            Type::Tuple(tup) => {
                let types: Vec<_> = tup.element_types.iter()
                    .map(|t| t.to_string())
                    .collect();
                write!(f, "[{}]", types.join(", "))
            }
            Type::Object(obj) => {
                let props: Vec<_> = obj.properties.iter()
                    .map(|p| format!("{}{}: {}",
                        p.name,
                        if p.optional { "?" } else { "" },
                        p.ty))
                    .collect();
                write!(f, "{{ {} }}", props.join("; "))
            }
            Type::Class(cls) => write!(f, "{}", cls.name),
            Type::TypeVar(tv) => write!(f, "{}", tv.name),
            Type::Generic(g) => {
                let args: Vec<_> = g.type_args.iter()
                    .map(|t| t.to_string())
                    .collect();
                write!(f, "{}<{}>", g.base, args.join(", "))
            }
            Type::Never => write!(f, "never"),
            Type::Unknown => write!(f, "unknown"),
            _ => write!(f, "<type>"),
        }
    }
}

impl fmt::Display for PrimitiveType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PrimitiveType::Number => write!(f, "number"),
            PrimitiveType::String => write!(f, "string"),
            PrimitiveType::Boolean => write!(f, "boolean"),
            PrimitiveType::Null => write!(f, "null"),
            PrimitiveType::Void => write!(f, "void"),
        }
    }
}
```

### Tasks

- [ ] Define `Type` enum with all variants
- [ ] Define primitive, union, function, array, tuple types
- [ ] Define object, class, interface types
- [ ] Define type variable and generic types
- [ ] Implement `Display` trait for all types
- [ ] Add type equality (`PartialEq`, `Eq`)
- [ ] Add type hashing (`Hash`)
- [ ] Write 30+ unit tests for type construction
- [ ] Test type display formatting

---

## Phase 2: Type Operations (Days 5-8) ✅ Complete

**Completion Date:** 2026-01-24

### Goal
Implement core type operations: subtyping, assignability, coercion, normalization.

### Deliverables

#### 2.1 Subtyping (`raya-types/src/subtyping.rs`)

**Subtype checking:**
```rust
use super::Type;

pub struct SubtypingContext {
    /// Track type variable assignments during checking
    type_vars: FxHashMap<String, Type>,
}

impl SubtypingContext {
    pub fn new() -> Self {
        Self {
            type_vars: FxHashMap::default(),
        }
    }

    /// Check if `sub` is a subtype of `sup`
    pub fn is_subtype(&mut self, sub: &Type, sup: &Type) -> bool {
        match (sub, sup) {
            // Reflexivity: T <: T
            (a, b) if a == b => true,

            // Never is subtype of everything
            (Type::Never, _) => true,

            // Everything is subtype of unknown
            (_, Type::Unknown) => true,

            // Primitive subtyping (none - primitives are exact)
            (Type::Primitive(p1), Type::Primitive(p2)) => p1 == p2,

            // Union subtyping: A | B <: C if A <: C and B <: C
            (Type::Union(u), sup) => {
                u.variants.iter().all(|v| self.is_subtype(v, sup))
            }

            // Subtyping to union: A <: B | C if A <: B or A <: C
            (sub, Type::Union(u)) => {
                u.variants.iter().any(|v| self.is_subtype(sub, v))
            }

            // Function subtyping (contravariant in parameters, covariant in return)
            (Type::Function(f1), Type::Function(f2)) => {
                // Same arity
                if f1.params.len() != f2.params.len() {
                    return false;
                }

                // Contravariant in parameters: sup_param <: sub_param
                for (p1, p2) in f1.params.iter().zip(f2.params.iter()) {
                    if !self.is_subtype(&p2.ty, &p1.ty) {
                        return false;
                    }
                }

                // Covariant in return: sub_return <: sup_return
                self.is_subtype(&f1.return_type, &f2.return_type)
            }

            // Array subtyping: T[] <: U[] if T <: U
            (Type::Array(a1), Type::Array(a2)) => {
                self.is_subtype(&a1.element_type, &a2.element_type)
            }

            // Tuple subtyping: [T1, T2] <: [U1, U2] if T1 <: U1 and T2 <: U2
            (Type::Tuple(t1), Type::Tuple(t2)) => {
                if t1.element_types.len() != t2.element_types.len() {
                    return false;
                }
                t1.element_types.iter()
                    .zip(t2.element_types.iter())
                    .all(|(e1, e2)| self.is_subtype(e1, e2))
            }

            // Structural subtyping for objects
            (Type::Object(o1), Type::Object(o2)) => {
                self.is_object_subtype(o1, o2)
            }

            // Class subtyping (nominal + structural)
            (Type::Class(c1), Type::Class(c2)) => {
                self.is_class_subtype(c1, c2)
            }

            // Type variable
            (Type::TypeVar(tv), sup) => {
                if let Some(constraint) = &tv.constraint {
                    self.is_subtype(constraint, sup)
                } else {
                    false
                }
            }

            _ => false,
        }
    }

    /// Check object structural subtyping
    fn is_object_subtype(&mut self, sub: &ObjectType, sup: &ObjectType) -> bool {
        // sub must have all properties of sup
        for sup_prop in &sup.properties {
            match sub.properties.iter().find(|p| p.name == sup_prop.name) {
                Some(sub_prop) => {
                    // Property types must be compatible
                    if !self.is_subtype(&sub_prop.ty, &sup_prop.ty) {
                        return false;
                    }
                    // Optional mismatch
                    if sup_prop.optional && !sub_prop.optional {
                        return false;
                    }
                }
                None => {
                    // Missing required property
                    if !sup_prop.optional {
                        return false;
                    }
                }
            }
        }

        // Check methods similarly
        for sup_method in &sup.methods {
            match sub.methods.iter().find(|m| m.name == sup_method.name) {
                Some(sub_method) => {
                    // Method signatures must be compatible
                    // (contravariant in params, covariant in return)
                    if !self.is_method_compatible(sub_method, sup_method) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        true
    }

    fn is_class_subtype(&mut self, sub: &ClassType, sup: &ClassType) -> bool {
        // Nominal: same name
        if sub.name == sup.name {
            return true;
        }

        // Check superclass chain
        if let Some(super_class) = &sub.super_class {
            if self.is_subtype(super_class, &Type::Class(sup.clone())) {
                return true;
            }
        }

        // Check implements (structural for interfaces)
        for impl_ty in &sub.implements {
            if self.is_subtype(impl_ty, &Type::Class(sup.clone())) {
                return true;
            }
        }

        false
    }

    fn is_method_compatible(&mut self, sub: &Method, sup: &Method) -> bool {
        // Same arity
        if sub.params.len() != sup.params.len() {
            return false;
        }

        // Contravariant in parameters
        for (sub_param, sup_param) in sub.params.iter().zip(sup.params.iter()) {
            if !self.is_subtype(&sup_param.ty, &sub_param.ty) {
                return false;
            }
        }

        // Covariant in return type
        self.is_subtype(&sub.return_type, &sup.return_type)
    }
}
```

#### 2.2 Type Assignability with Coercions (`raya-types/src/assignability.rs`)

**Assignability checking with implicit coercions:**
```rust
use super::{Type, PrimitiveType};

pub struct AssignabilityContext {
    subtyping: SubtypingContext,
}

impl AssignabilityContext {
    pub fn new() -> Self {
        Self {
            subtyping: SubtypingContext::new(),
        }
    }

    /// Check if `source` can be assigned to `target`
    /// Includes implicit primitive coercions
    pub fn is_assignable(&mut self, source: &Type, target: &Type) -> bool {
        // First check subtyping (no coercion needed)
        if self.subtyping.is_subtype(source, target) {
            return true;
        }

        // Check implicit primitive coercions
        if self.can_coerce(source, target) {
            return true;
        }

        // Check union type coercion
        if let Type::Union(u) = source {
            // If all variants can coerce to target, allow it
            return u.variants.iter().all(|v| self.is_assignable(v, target));
        }

        false
    }

    /// Check if source can be implicitly coerced to target
    fn can_coerce(&self, source: &Type, target: &Type) -> bool {
        match (source, target) {
            // number → string
            (Type::Primitive(PrimitiveType::Number), Type::Primitive(PrimitiveType::String)) => true,

            // boolean → string
            (Type::Primitive(PrimitiveType::Boolean), Type::Primitive(PrimitiveType::String)) => true,

            // null → string (becomes "null")
            (Type::Primitive(PrimitiveType::Null), Type::Primitive(PrimitiveType::String)) => true,

            // No other primitive coercions
            _ => false,
        }
    }
}
```

#### 2.3 Type Normalization (`raya-types/src/normalize.rs`)

**Simplify and normalize types:**
```rust
use super::Type;

pub struct TypeNormalizer;

impl TypeNormalizer {
    /// Normalize a type to canonical form
    pub fn normalize(ty: &Type) -> Type {
        match ty {
            // Flatten nested unions: (A | B) | C → A | B | C
            Type::Union(u) => {
                let mut variants = Vec::new();
                for v in &u.variants {
                    match Self::normalize(v) {
                        Type::Union(inner) => variants.extend(inner.variants),
                        other => variants.push(other),
                    }
                }
                // Remove duplicates
                variants.sort_by(|a, b| format!("{}", a).cmp(&format!("{}", b)));
                variants.dedup();

                if variants.len() == 1 {
                    variants.into_iter().next().unwrap()
                } else {
                    Type::Union(UnionType {
                        variants,
                        discriminant: u.discriminant.clone(),
                    })
                }
            }

            // Normalize nested types
            Type::Array(arr) => Type::Array(ArrayType {
                element_type: Box::new(Self::normalize(&arr.element_type)),
            }),

            Type::Tuple(tup) => Type::Tuple(TupleType {
                element_types: tup.element_types.iter()
                    .map(|t| Self::normalize(t))
                    .collect(),
            }),

            Type::Function(func) => Type::Function(FunctionType {
                params: func.params.iter()
                    .map(|p| FunctionParam {
                        name: p.name.clone(),
                        ty: Self::normalize(&p.ty),
                        optional: p.optional,
                    })
                    .collect(),
                return_type: Box::new(Self::normalize(&func.return_type)),
                is_async: func.is_async,
            }),

            // Already normalized
            other => other.clone(),
        }
    }
}
```

### Tasks

- [ ] Implement subtype checking for all type combinations
- [ ] Implement primitive coercion rules (number → string, etc.)
- [ ] Implement type assignability with coercions
- [ ] Implement type normalization (flatten unions, remove duplicates)
- [ ] Add structural subtyping for objects
- [ ] Add nominal subtyping for classes
- [ ] Write 40+ unit tests for subtyping
- [ ] Write 20+ tests for coercion rules
- [ ] Write 15+ tests for normalization

---

## Phase 3: Generic Type System (Days 9-12) ✅ Complete

**Completion Date:** 2026-01-24

### Goal
Implement generic types, type parameters, constraints, and instantiation.

### Deliverables

#### 3.1 Generic Type Instantiation (`raya-types/src/generics.rs`)

**Instantiate generic types with concrete type arguments:**
```rust
use super::{Type, TypeVar, GenericType};
use rustc_hash::FxHashMap;

pub struct GenericInstantiator {
    substitutions: FxHashMap<String, Type>,
}

impl GenericInstantiator {
    pub fn new() -> Self {
        Self {
            substitutions: FxHashMap::default(),
        }
    }

    /// Instantiate a generic type with type arguments
    pub fn instantiate(&mut self, ty: &Type, type_args: &[Type]) -> Result<Type, GenericError> {
        match ty {
            Type::Generic(g) => {
                // Build substitution map
                self.build_substitutions(&g.type_args, type_args)?;

                // Apply substitutions to base type
                self.apply_substitutions(&g.base)
            }

            Type::TypeVar(tv) => {
                // Look up type variable
                match self.substitutions.get(&tv.name) {
                    Some(concrete) => Ok(concrete.clone()),
                    None => Err(GenericError::UnboundTypeVariable(tv.name.clone())),
                }
            }

            // Recursively instantiate nested types
            Type::Array(arr) => Ok(Type::Array(ArrayType {
                element_type: Box::new(self.instantiate(&arr.element_type, type_args)?),
            })),

            Type::Function(func) => Ok(Type::Function(FunctionType {
                params: func.params.iter()
                    .map(|p| Ok(FunctionParam {
                        name: p.name.clone(),
                        ty: self.instantiate(&p.ty, type_args)?,
                        optional: p.optional,
                    }))
                    .collect::<Result<Vec<_>, _>>()?,
                return_type: Box::new(self.instantiate(&func.return_type, type_args)?),
                is_async: func.is_async,
            })),

            // Already concrete
            other => Ok(other.clone()),
        }
    }

    fn build_substitutions(&mut self, params: &[Type], args: &[Type]) -> Result<(), GenericError> {
        if params.len() != args.len() {
            return Err(GenericError::ArityMismatch {
                expected: params.len(),
                found: args.len(),
            });
        }

        for (param, arg) in params.iter().zip(args.iter()) {
            if let Type::TypeVar(tv) = param {
                // Check constraint
                if let Some(constraint) = &tv.constraint {
                    let mut ctx = SubtypingContext::new();
                    if !ctx.is_subtype(arg, constraint) {
                        return Err(GenericError::ConstraintViolation {
                            param: tv.name.clone(),
                            constraint: constraint.to_string(),
                            arg: arg.to_string(),
                        });
                    }
                }

                self.substitutions.insert(tv.name.clone(), arg.clone());
            }
        }

        Ok(())
    }

    fn apply_substitutions(&self, ty: &Type) -> Result<Type, GenericError> {
        match ty {
            Type::TypeVar(tv) => {
                match self.substitutions.get(&tv.name) {
                    Some(concrete) => Ok(concrete.clone()),
                    None => Err(GenericError::UnboundTypeVariable(tv.name.clone())),
                }
            }

            Type::Array(arr) => Ok(Type::Array(ArrayType {
                element_type: Box::new(self.apply_substitutions(&arr.element_type)?),
            })),

            // ... apply to all nested types

            other => Ok(other.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum GenericError {
    UnboundTypeVariable(String),
    ArityMismatch { expected: usize, found: usize },
    ConstraintViolation { param: String, constraint: String, arg: String },
}
```

#### 3.2 Type Variable Unification (`raya-types/src/unify.rs`)

**Unify type variables during type checking:**
```rust
use super::Type;

pub struct Unifier {
    substitutions: FxHashMap<String, Type>,
}

impl Unifier {
    pub fn new() -> Self {
        Self {
            substitutions: FxHashMap::default(),
        }
    }

    /// Unify two types, solving for type variables
    pub fn unify(&mut self, t1: &Type, t2: &Type) -> Result<(), UnifyError> {
        match (t1, t2) {
            // Same type
            (a, b) if a == b => Ok(()),

            // Type variable unification
            (Type::TypeVar(tv), ty) | (ty, Type::TypeVar(tv)) => {
                self.unify_type_var(&tv.name, ty)
            }

            // Structural unification
            (Type::Array(a1), Type::Array(a2)) => {
                self.unify(&a1.element_type, &a2.element_type)
            }

            (Type::Function(f1), Type::Function(f2)) => {
                if f1.params.len() != f2.params.len() {
                    return Err(UnifyError::IncompatibleTypes);
                }
                for (p1, p2) in f1.params.iter().zip(f2.params.iter()) {
                    self.unify(&p1.ty, &p2.ty)?;
                }
                self.unify(&f1.return_type, &f2.return_type)
            }

            _ => Err(UnifyError::IncompatibleTypes),
        }
    }

    fn unify_type_var(&mut self, var: &str, ty: &Type) -> Result<(), UnifyError> {
        // Check for existing binding
        if let Some(bound) = self.substitutions.get(var) {
            return self.unify(bound, ty);
        }

        // Occurs check (prevent infinite types)
        if self.occurs(var, ty) {
            return Err(UnifyError::OccursCheck);
        }

        // Bind type variable
        self.substitutions.insert(var.to_string(), ty.clone());
        Ok(())
    }

    fn occurs(&self, var: &str, ty: &Type) -> bool {
        match ty {
            Type::TypeVar(tv) => tv.name == var,
            Type::Array(arr) => self.occurs(var, &arr.element_type),
            Type::Function(func) => {
                func.params.iter().any(|p| self.occurs(var, &p.ty))
                    || self.occurs(var, &func.return_type)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum UnifyError {
    IncompatibleTypes,
    OccursCheck,
}
```

### Tasks

- [ ] Implement generic type instantiation
- [ ] Implement type variable substitution
- [ ] Implement constraint checking for type parameters
- [ ] Implement type variable unification
- [ ] Add occurs check for infinite types
- [ ] Write 25+ tests for generic instantiation
- [ ] Write 15+ tests for unification

---

## Phase 4: Utilities & Testing (Days 13-15) ✅ Complete

**Completion Date:** 2026-01-24

### Goal
Add utility functions, comprehensive testing, and documentation.

### Deliverables

#### 4.1 Type Utilities (`raya-types/src/utils.rs`)

**Helper functions:**
```rust
use super::Type;

impl Type {
    /// Check if type is a bare union (primitives only)
    pub fn is_bare_union(&self) -> bool {
        match self {
            Type::Union(u) => {
                u.variants.iter().all(|v| matches!(v, Type::Primitive(_)))
            }
            _ => false,
        }
    }

    /// Check if type is a discriminated union
    pub fn is_discriminated_union(&self) -> bool {
        match self {
            Type::Union(u) => u.discriminant.is_some(),
            _ => false,
        }
    }

    /// Check if type is nullable (includes null)
    pub fn is_nullable(&self) -> bool {
        match self {
            Type::Primitive(PrimitiveType::Null) => true,
            Type::Union(u) => {
                u.variants.iter().any(|v| v.is_nullable())
            }
            _ => false,
        }
    }

    /// Remove null from type (for null checking narrowing)
    pub fn remove_null(&self) -> Type {
        match self {
            Type::Union(u) => {
                let variants: Vec<_> = u.variants.iter()
                    .filter(|v| !v.is_nullable())
                    .cloned()
                    .collect();
                if variants.len() == 1 {
                    variants.into_iter().next().unwrap()
                } else {
                    Type::Union(UnionType {
                        variants,
                        discriminant: u.discriminant.clone(),
                    })
                }
            }
            Type::Primitive(PrimitiveType::Null) => Type::Never,
            other => other.clone(),
        }
    }

    /// Get size estimate for type (for memory allocation)
    pub fn size_estimate(&self) -> usize {
        match self {
            Type::Primitive(_) => 8,  // Tagged value
            Type::Array(_) => 24,      // Pointer + length + capacity
            Type::Object(obj) => 16 + obj.properties.len() * 16,
            Type::Class(_) => 16,      // Object header + vtable
            _ => 8,
        }
    }
}
```

#### 4.2 Type Cache (`raya-types/src/cache.rs`)

**Efficient type storage:**
```rust
use std::sync::Arc;
use rustc_hash::FxHashMap;

pub struct TypeCache {
    types: FxHashMap<Type, Arc<Type>>,
}

impl TypeCache {
    pub fn new() -> Self {
        Self {
            types: FxHashMap::default(),
        }
    }

    /// Intern a type (deduplicate storage)
    pub fn intern(&mut self, ty: Type) -> Arc<Type> {
        if let Some(cached) = self.types.get(&ty) {
            Arc::clone(cached)
        } else {
            let arc = Arc::new(ty.clone());
            self.types.insert(ty, Arc::clone(&arc));
            arc
        }
    }

    /// Get cached type count
    pub fn len(&self) -> usize {
        self.types.len()
    }
}
```

### Tasks

- [ ] Implement type utility functions
- [ ] Implement type cache/interning
- [ ] Add comprehensive documentation
- [ ] Write 100+ total unit tests
- [ ] Add integration tests with parser AST types
- [ ] Benchmark type operations
- [ ] Profile memory usage

---

## Success Criteria

### Must Have

- [ ] Complete type representation for all Raya types
- [ ] Subtyping implementation for all type combinations
- [ ] Implicit primitive coercion rules (number → string)
- [ ] Type assignability checking
- [ ] Generic type instantiation with constraints
- [ ] Type normalization and simplification
- [ ] Type equality and hashing
- [ ] Pretty-printing for error messages
- [ ] 100+ comprehensive unit tests
- [ ] All tests passing

### Should Have

- [ ] Type caching for performance
- [ ] Type size estimates
- [ ] Utility functions (is_nullable, remove_null, etc.)
- [ ] Benchmark suite for type operations
- [ ] Memory profiling

### Nice to Have

- [ ] Type serialization for caching
- [ ] Type diffing for better error messages
- [ ] Type inference hints for error messages

---

## Testing Strategy

### Unit Tests

**Test categories (100+ tests total):**

1. **Type Construction (30 tests)**
   - Create all type variants
   - Test type equality
   - Test type hashing
   - Test Display formatting

2. **Subtyping (40 tests)**
   - Primitive types
   - Union types
   - Function types (contravariance/covariance)
   - Array/tuple types
   - Object structural subtyping
   - Class nominal subtyping
   - Type variable constraints

3. **Assignability & Coercion (20 tests)**
   - Primitive coercions (number → string)
   - Union type coercion
   - Structural subtyping
   - Subtype widening

4. **Generics (25 tests)**
   - Generic instantiation
   - Type variable substitution
   - Constraint checking
   - Unification
   - Occurs check

5. **Normalization (15 tests)**
   - Flatten unions
   - Remove duplicates
   - Simplify types

### Integration Tests

**Integration with parser:**
```rust
// tests/integration.rs
use raya_parser::Parser;
use raya_types::TypeConverter;

#[test]
fn test_convert_ast_types_to_type_system() {
    let source = "let x: number | string = 42;";
    let ast = Parser::new(source).unwrap().parse().unwrap();

    let converter = TypeConverter::new();
    let ty = converter.convert_type_annotation(&ast.statements[0].type_annotation);

    assert!(matches!(ty, Type::Union(_)));
}
```

### Test Files

```
crates/raya-types/
├── src/
│   ├── lib.rs
│   ├── types.rs
│   ├── display.rs
│   ├── subtyping.rs
│   ├── assignability.rs
│   ├── normalize.rs
│   ├── generics.rs
│   ├── unify.rs
│   ├── utils.rs
│   └── cache.rs
└── tests/
    ├── types_test.rs           # Type construction tests
    ├── subtyping_test.rs       # Subtyping tests
    ├── coercion_test.rs        # Coercion tests
    ├── generics_test.rs        # Generic tests
    ├── normalize_test.rs       # Normalization tests
    └── integration_test.rs     # Integration with parser
```

---

## References

### Language Specification

- [design/LANG.md](../design/LANG.md) - Complete language specification
  - Section 4: Type System (comprehensive type rules)
  - Section 4.7: Discriminated Unions
  - Section 13: Generics and Monomorphization

### Related Milestones

- [Milestone 2.3](milestone-2.3.md) - Parser (✅ Complete)
- [Milestone 2.5](milestone-2.5.md) - Type Checker (Next)
- [Milestone 2.6](milestone-2.6.md) - Discriminant Inference (Future)

### External References

- **Type Systems:**
  - Types and Programming Languages (TAPL) by Benjamin Pierce
  - Practical Foundations for Programming Languages by Robert Harper

- **Subtyping:**
  - https://en.wikipedia.org/wiki/Subtyping
  - Nominal vs Structural Typing

- **Generics:**
  - Rust's generic system (monomorphization)
  - TypeScript's generic system (for comparison)

---

## Notes

### 1. Type System vs Type Checker

This milestone focuses on the **type system infrastructure**:
- Type representation
- Type operations (subtyping, normalization)
- Generic type handling

The **type checker** (Milestone 2.5) will use this infrastructure to:
- Validate programs
- Infer types
- Perform control flow analysis

### 2. Implicit Coercions

Raya allows implicit primitive coercions:
- `number → string` ✅
- `boolean → string` ✅
- `null → string` ✅
- `string → number` ❌
- `string → boolean` ❌

This is more permissive than TypeScript but safer than JavaScript.

### 3. Discriminated Unions

All object unions must be discriminated:
```typescript
// ❌ ERROR: Non-discriminated
type Bad = { x: number } | { y: string };

// ✅ OK: Discriminated with "kind"
type Good =
  | { kind: "a"; x: number }
  | { kind: "b"; y: string };
```

Bare primitive unions are allowed:
```typescript
// ✅ OK: Bare primitive union
type ID = string | number;
```

### 4. Structural vs Nominal Typing

- **Objects**: Structural typing (compared by shape)
- **Classes**: Nominal for inheritance, structural for interfaces
- **Interfaces**: Structural (deprecated, use type aliases)

### 5. Performance Considerations

- Type interning to reduce memory
- Type caching to avoid recomputation
- Efficient subtyping checks (memoization)
- Fast type equality (hash-based)

---

**End of Milestone 2.4 Specification**
