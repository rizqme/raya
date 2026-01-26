# Milestone 2.7: Bare Union Transformation

**Status:** Not Started
**Depends On:** Milestone 2.6 (Discriminant Inference)
**Duration:** 2-3 weeks
**Completion Date:** TBD

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Non-Goals](#non-goals)
4. [Design](#design)
5. [Phase 1: Detection & Validation](#phase-1-detection--validation-week-1)
6. [Phase 2: Type Transformation](#phase-2-type-transformation-week-2)
7. [Phase 3: typeof Integration](#phase-3-typeof-integration-week-3)
8. [Testing Strategy](#testing-strategy)
9. [Success Criteria](#success-criteria)

---

## Overview

Implement automatic transformation of bare primitive unions (`string | number`) into internal discriminated union representations while preserving user-friendly syntax. This allows Raya to support TypeScript-style bare unions for primitives while maintaining the zero-runtime-overhead discriminated union approach for complex types.

### What is a Bare Union?

A **bare union** is a union type containing only primitive types, written without explicit discriminant fields:

```typescript
// Bare union (primitives only)
type ID = string | number;

// NOT a bare union (contains object type)
type Value = string | { x: number };
```

### Design Philosophy

Raya provides two patterns for union types:

1. **Bare Primitive Unions** - Simple syntax with `typeof` narrowing
   - Only for primitives: `int`, `float`, `number`, `string`, `boolean`, `null`
   - Compiler automatically transforms to internal discriminated unions
   - User uses `typeof` for type narrowing
   - Zero runtime overhead

2. **Discriminated Unions** - Explicit discriminant fields for complex types
   - Required for objects, arrays, classes
   - Explicit `kind`/`type`/`tag` fields
   - Pattern matching on discriminant values
   - Compile-time exhaustiveness checking

### How It Works

```typescript
// User writes:
type ID = string | number;

// Compiler internally transforms to:
type ID =
  | { $type: "string"; $value: string }
  | { $type: "number"; $value: number };

// User code:
let id: ID = "abc";
if (typeof id === "string") {
  console.log(id.toUpperCase());  // Works transparently
}

// Compiler generates:
let id: ID = { $type: "string", $value: "abc" };
if (id.$type === "string") {
  console.log(id.$value.toUpperCase());
}
```

**Key Insight:** The `$type` and `$value` fields are:
- ✅ Generated automatically by the compiler
- ✅ Hidden from user code (cannot be accessed directly)
- ✅ Used internally for type narrowing and runtime checks
- ✅ Enable exhaustiveness checking at compile time
- ✅ Provide zero-cost abstraction

---

## Goals

1. **Detect bare primitive unions** in type expressions
2. **Transform bare unions** to internal `{ $type, $value }` representation
3. **Insert boxing/unboxing code** automatically at assignment boundaries
4. **Support typeof operator** for type narrowing on bare unions
5. **Prevent user access** to `$type` and `$value` fields
6. **Maintain exhaustiveness checking** for typeof-based switches
7. **Preserve zero runtime overhead** (no actual boxing, use Value enum directly)

---

## Non-Goals

1. ❌ Support bare unions for non-primitive types (objects, arrays, classes)
2. ❌ Allow user-defined types with `$type` or `$value` fields
3. ❌ Runtime reflection on bare union types
4. ❌ Structural compatibility between bare and discriminated unions
5. ❌ Automatic transformation of discriminated unions to bare unions

---

## Design

### Architecture

```
Source Code (bare union)
    ↓
Parser (AST with UnionType)
    ↓
Bare Union Detector (is_bare_primitive_union)
    ↓
Type Transformer (create internal discriminated union)
    ↓
Type Checker (validates typeof usage)
    ↓
Code Generator (insert boxing/unboxing)
    ↓
Bytecode (efficient Value enum operations)
```

### Data Structures

```rust
// Type representation
pub enum Type {
    Union(UnionType),
    BareUnion(BareUnionType),  // NEW: Special marker for bare unions
    // ... other types ...
}

pub struct BareUnionType {
    /// Original primitive members (before transformation)
    pub primitives: Vec<PrimitiveType>,

    /// Internal discriminated union representation
    /// (generated automatically, not visible to user)
    pub internal_union: TypeId,
}

pub struct BareUnionInfo {
    /// The field name for type discrimination (always "$type")
    pub discriminant_field: String,

    /// The field name for value storage (always "$value")
    pub value_field: String,

    /// Mapping from primitive type to discriminant value
    /// e.g., PrimitiveType::String -> "string"
    pub type_map: FxHashMap<PrimitiveType, String>,
}
```

### Transformation Algorithm

**Input:** UnionType with members `[string, number, boolean]`

**Output:** BareUnionType with internal structure:
```rust
{
  primitives: [String, Number, Boolean],
  internal_union: TypeId -> Union([
    Object({ $type: "string", $value: string }),
    Object({ $type: "number", $value: number }),
    Object({ $type: "boolean", $value: boolean }),
  ])
}
```

**Steps:**

1. **Detect** if union contains only primitives
2. **Validate** no duplicate types (e.g., `int | int` is invalid)
3. **Create internal variants** for each primitive:
   ```rust
   for prim in primitives {
       variant = Object {
           properties: [
               { name: "$type", ty: StringLiteral(type_name(prim)) },
               { name: "$value", ty: prim },
           ]
       }
       variants.push(variant)
   }
   ```
4. **Create internal union** from variants
5. **Store mapping** from primitive type to variant index

### Boxing/Unboxing Strategy

**Key Optimization:** Don't actually box values at runtime!

Instead of creating wrapper objects, use the VM's `Value` enum directly:

```rust
// VM Value enum (from raya-core)
pub enum Value {
    I32(i32),
    F64(f64),
    String(GcPtr<String>),
    Boolean(bool),
    Null,
    // ... other variants ...
}
```

**Approach:**

1. **Type System Level:** Bare unions have internal discriminated representation
2. **Bytecode Level:** Use typed opcodes that work directly with Value enum
3. **No Runtime Boxing:** Values stored as-is in Value enum
4. **typeof Implementation:** Use Value enum discriminant directly

**Example:**

```typescript
// Source:
let id: string | number = 42;

// Type System (internal):
// id: { $type: "number", $value: number }

// Bytecode (actual):
CONST_I32 42
STORE_LOCAL 0  // Stores Value::I32(42) directly

// typeof check:
LOAD_LOCAL 0
TYPEOF         // Returns "number" by checking Value enum tag
PUSH_STRING "number"
EQ
```

**Benefits:**
- ✅ Zero allocation overhead
- ✅ No wrapper objects created
- ✅ Direct value storage
- ✅ Fast typeof checks (single enum tag check)

---

## Phase 1: Detection & Validation (Week 1)

**Duration:** 5-7 days
**Goal:** Detect bare primitive unions and validate constraints

### Task 1.1: Create bare_union Module

**New file:** `crates/raya-types/src/bare_union.rs`

```rust
use crate::{Type, TypeContext, TypeId, PrimitiveType};
use rustc_hash::FxHashSet;

/// Detector for bare primitive unions
pub struct BareUnionDetector<'a> {
    type_ctx: &'a TypeContext,
}

impl<'a> BareUnionDetector<'a> {
    pub fn new(type_ctx: &'a TypeContext) -> Self {
        Self { type_ctx }
    }

    /// Check if a union type is a bare primitive union
    pub fn is_bare_primitive_union(&self, members: &[TypeId]) -> bool {
        if members.is_empty() {
            return false;
        }

        // All members must be primitives
        members.iter().all(|&member_id| {
            if let Some(ty) = self.type_ctx.get(member_id) {
                matches!(ty,
                    Type::Primitive(PrimitiveType::Int) |
                    Type::Primitive(PrimitiveType::Float) |
                    Type::Primitive(PrimitiveType::Number) |
                    Type::Primitive(PrimitiveType::String) |
                    Type::Primitive(PrimitiveType::Boolean) |
                    Type::Primitive(PrimitiveType::Null)
                )
            } else {
                false
            }
        })
    }

    /// Extract primitive types from union members
    pub fn extract_primitives(&self, members: &[TypeId]) -> Vec<PrimitiveType> {
        members.iter().filter_map(|&member_id| {
            if let Some(Type::Primitive(prim)) = self.type_ctx.get(member_id) {
                Some(*prim)
            } else {
                None
            }
        }).collect()
    }

    /// Validate no duplicate primitive types
    pub fn validate_no_duplicates(&self, primitives: &[PrimitiveType]) -> Result<(), BareUnionError> {
        let mut seen = FxHashSet::default();
        for &prim in primitives {
            if !seen.insert(prim) {
                return Err(BareUnionError::DuplicatePrimitive {
                    primitive: prim
                });
            }
        }
        Ok(())
    }
}

/// Errors during bare union processing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BareUnionError {
    /// Union contains duplicate primitive types
    DuplicatePrimitive {
        primitive: PrimitiveType,
    },

    /// Union contains non-primitive types (cannot be bare union)
    NonPrimitiveMembers {
        union_members: Vec<TypeId>,
    },

    /// User attempted to access $type or $value fields
    ForbiddenFieldAccess {
        field_name: String,
    },
}
```

### Task 1.2: Add Primitive Type Names

**Modify:** `crates/raya-types/src/ty.rs`

```rust
impl PrimitiveType {
    /// Get the string representation for typeof
    pub fn type_name(&self) -> &'static str {
        match self {
            PrimitiveType::Int => "int",
            PrimitiveType::Float => "float",
            PrimitiveType::Number => "number",
            PrimitiveType::String => "string",
            PrimitiveType::Boolean => "boolean",
            PrimitiveType::Null => "null",
        }
    }

    /// Check if this is a valid bare union primitive
    pub fn is_bare_union_primitive(&self) -> bool {
        matches!(self,
            PrimitiveType::Int |
            PrimitiveType::Float |
            PrimitiveType::Number |
            PrimitiveType::String |
            PrimitiveType::Boolean |
            PrimitiveType::Null
        )
    }
}
```

### Task 1.3: Update UnionType Structure

**Modify:** `crates/raya-types/src/ty.rs`

```rust
pub struct UnionType {
    pub members: Vec<TypeId>,
    pub discriminant: Option<Discriminant>,
    pub is_bare: bool,  // NEW: Flag indicating bare primitive union
}
```

### Verification (Phase 1)

**Tests:** `crates/raya-types/tests/bare_union_test.rs`

```rust
#[test]
fn test_detect_bare_primitive_union() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let number = ctx.number_type();

    let detector = BareUnionDetector::new(&ctx);
    assert!(detector.is_bare_primitive_union(&[string, number]));
}

#[test]
fn test_reject_object_in_bare_union() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let obj = ctx.intern(Type::Object(ObjectType {
        properties: vec![],
        index_signature: None
    }));

    let detector = BareUnionDetector::new(&ctx);
    assert!(!detector.is_bare_primitive_union(&[string, obj]));
}

#[test]
fn test_reject_duplicate_primitives() {
    let detector = BareUnionDetector::new(&TypeContext::new());
    let prims = vec![PrimitiveType::String, PrimitiveType::String];

    let result = detector.validate_no_duplicates(&prims);
    assert!(result.is_err());
}

#[test]
fn test_primitive_type_names() {
    assert_eq!(PrimitiveType::String.type_name(), "string");
    assert_eq!(PrimitiveType::Number.type_name(), "number");
    assert_eq!(PrimitiveType::Boolean.type_name(), "boolean");
}
```

**Success Criteria:**
- ✅ BareUnionDetector correctly identifies bare primitive unions
- ✅ Rejects unions with non-primitive types
- ✅ Validates no duplicate primitives
- ✅ 4+ unit tests passing

---

## Phase 2: Type Transformation (Week 2)

**Duration:** 5-7 days
**Goal:** Transform bare unions to internal discriminated unions

### Task 2.1: Implement Transformation Logic

**Add to:** `crates/raya-types/src/bare_union.rs`

```rust
use crate::ty::{ObjectType, PropertySignature};

/// Transforms bare primitive unions to internal discriminated unions
pub struct BareUnionTransform<'a> {
    type_ctx: &'a mut TypeContext,
}

impl<'a> BareUnionTransform<'a> {
    pub fn new(type_ctx: &'a mut TypeContext) -> Self {
        Self { type_ctx }
    }

    /// Transform a bare primitive union to internal representation
    ///
    /// Transforms `string | number` into:
    /// ```
    /// { $type: "string", $value: string } | { $type: "number", $value: number }
    /// ```
    pub fn transform(&mut self, primitives: &[PrimitiveType]) -> TypeId {
        let variants: Vec<TypeId> = primitives.iter()
            .map(|&prim| self.create_variant(prim))
            .collect();

        // Create internal union with automatic discriminant inference
        self.type_ctx.union_type(variants)
    }

    /// Create a variant object for a primitive type
    ///
    /// For PrimitiveType::String, creates:
    /// ```
    /// { $type: "string", $value: string }
    /// ```
    fn create_variant(&mut self, prim: PrimitiveType) -> TypeId {
        // Create literal type for $type field
        let type_literal = self.type_ctx.string_literal(prim.type_name());

        // Create primitive type for $value field
        let value_type = match prim {
            PrimitiveType::Int => self.type_ctx.int_type(),
            PrimitiveType::Float => self.type_ctx.float_type(),
            PrimitiveType::Number => self.type_ctx.number_type(),
            PrimitiveType::String => self.type_ctx.string_type(),
            PrimitiveType::Boolean => self.type_ctx.boolean_type(),
            PrimitiveType::Null => self.type_ctx.null_type(),
        };

        // Create object type with $type and $value fields
        self.type_ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "$type".to_string(),
                    ty: type_literal,
                    optional: false,
                    readonly: true,  // $type is immutable
                },
                PropertySignature {
                    name: "$value".to_string(),
                    ty: value_type,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }))
    }
}

/// Information about a bare union's internal representation
#[derive(Debug, Clone)]
pub struct BareUnionInfo {
    /// Mapping from primitive type to variant index
    pub variant_map: FxHashMap<PrimitiveType, usize>,

    /// The internal discriminated union TypeId
    pub internal_union: TypeId,
}
```

### Task 2.2: Integrate with TypeContext

**Modify:** `crates/raya-types/src/context.rs`

```rust
use crate::bare_union::{BareUnionDetector, BareUnionTransform};

impl TypeContext {
    /// Create a union type, automatically detecting bare primitive unions
    pub fn union_type(&mut self, members: Vec<TypeId>) -> TypeId {
        // Check if this is a bare primitive union
        let detector = BareUnionDetector::new(self);
        let is_bare = detector.is_bare_primitive_union(&members);

        if is_bare {
            // Extract primitives and transform
            let primitives = detector.extract_primitives(&members);
            let mut transform = BareUnionTransform::new(self);
            let internal_union = transform.transform(&primitives);

            // Create bare union type
            let bare_union = Type::Union(UnionType {
                members: members.clone(),
                discriminant: None,  // Will be set by discriminant inference
                is_bare: true,
            });

            self.intern(bare_union)
        } else {
            // Regular union - use existing logic
            // ... (existing union_type implementation)
        }
    }

    /// Get the internal representation of a bare union
    pub fn get_bare_union_internal(&self, union_id: TypeId) -> Option<TypeId> {
        if let Some(Type::Union(union)) = self.get(union_id) {
            if union.is_bare {
                // Return the internal discriminated union
                // (This requires storing the internal union ID)
                // TODO: Add internal_union field to UnionType
                None
            } else {
                None
            }
        } else {
            None
        }
    }
}
```

### Task 2.3: Update UnionType to Store Internal Representation

**Modify:** `crates/raya-types/src/ty.rs`

```rust
pub struct UnionType {
    pub members: Vec<TypeId>,
    pub discriminant: Option<Discriminant>,
    pub is_bare: bool,
    pub internal_union: Option<TypeId>,  // NEW: Internal representation for bare unions
}
```

### Verification (Phase 2)

**Tests:** Add to `crates/raya-types/tests/bare_union_test.rs`

```rust
#[test]
fn test_transform_string_number_union() {
    let mut ctx = TypeContext::new();

    let primitives = vec![PrimitiveType::String, PrimitiveType::Number];
    let mut transform = BareUnionTransform::new(&mut ctx);
    let internal = transform.transform(&primitives);

    // Verify internal union has correct structure
    if let Some(Type::Union(union)) = ctx.get(internal) {
        assert_eq!(union.members.len(), 2);

        // Check discriminant was inferred as "$type"
        assert!(union.discriminant.is_some());
        let disc = union.discriminant.as_ref().unwrap();
        assert_eq!(disc.field_name, "$type");
    } else {
        panic!("Expected union type");
    }
}

#[test]
fn test_variant_structure() {
    let mut ctx = TypeContext::new();
    let mut transform = BareUnionTransform::new(&mut ctx);

    let variant = transform.create_variant(PrimitiveType::String);

    if let Some(Type::Object(obj)) = ctx.get(variant) {
        assert_eq!(obj.properties.len(), 2);

        // Check $type field
        let type_prop = &obj.properties[0];
        assert_eq!(type_prop.name, "$type");
        assert!(type_prop.readonly);

        // Check $value field
        let value_prop = &obj.properties[1];
        assert_eq!(value_prop.name, "$value");
    } else {
        panic!("Expected object type");
    }
}

#[test]
fn test_union_type_auto_detection() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let number = ctx.number_type();

    let union = ctx.union_type(vec![string, number]);

    if let Some(Type::Union(u)) = ctx.get(union) {
        assert!(u.is_bare);
        assert!(u.internal_union.is_some());
    } else {
        panic!("Expected bare union");
    }
}
```

**Success Criteria:**
- ✅ Bare unions automatically transformed to internal representation
- ✅ Internal unions have correct `{ $type, $value }` structure
- ✅ Discriminant automatically inferred as "$type"
- ✅ 3+ integration tests passing

---

## Phase 3: typeof Integration (Week 3)

**Duration:** 5-7 days
**Goal:** Support typeof operator for type narrowing on bare unions

### Task 3.1: typeof Operator Support

**Modify:** `crates/raya-checker/src/type_guards.rs`

```rust
/// Type guard patterns
pub enum TypeGuard {
    /// typeof x === "type"
    TypeOf {
        var: String,
        type_name: String,
        negated: bool
    },

    /// x.discriminant === "variant"
    Discriminant {
        var: String,
        field: String,
        variant: String,
        negated: bool
    },

    /// x !== null
    Nullish {
        var: String,
        negated: bool
    },
}

impl TypeGuard {
    /// Extract type guard from typeof expression
    pub fn from_typeof_check(binary_expr: &BinaryExpr) -> Option<Self> {
        // Parse: typeof x === "string"
        // Left: UnaryExpr { op: typeof, argument: x }
        // Right: StringLiteral("string")

        if !matches!(binary_expr.op, BinOp::Eq | BinOp::StrictEq) {
            return None;
        }

        // Check if left side is typeof
        if let Expression::Unary(unary) = &binary_expr.left {
            if matches!(unary.op, UnaryOp::TypeOf) {
                // Extract variable name
                if let Expression::Identifier(ident) = &unary.argument {
                    // Extract type name from right side
                    if let Expression::StringLiteral(lit) = &binary_expr.right {
                        return Some(TypeGuard::TypeOf {
                            var: ident.name.clone(),
                            type_name: lit.value.clone(),
                            negated: false,
                        });
                    }
                }
            }
        }

        None
    }
}
```

### Task 3.2: Type Narrowing for Bare Unions

**Modify:** `crates/raya-checker/src/narrowing.rs`

```rust
use crate::bare_union::BareUnionDetector;

impl TypeNarrower<'_> {
    /// Apply typeof guard to narrow bare union
    pub fn apply_typeof_guard(
        &mut self,
        original_ty: TypeId,
        type_name: &str,
        negated: bool,
    ) -> TypeId {
        // Check if original type is a bare union
        if let Some(Type::Union(union)) = self.type_ctx.get(original_ty) {
            if !union.is_bare {
                return original_ty;  // Not a bare union
            }

            // Find the primitive type matching type_name
            let target_prim = match type_name {
                "int" => Some(PrimitiveType::Int),
                "float" => Some(PrimitiveType::Float),
                "number" => Some(PrimitiveType::Number),
                "string" => Some(PrimitiveType::String),
                "boolean" => Some(PrimitiveType::Boolean),
                "null" => Some(PrimitiveType::Null),
                _ => None,
            };

            let Some(target_prim) = target_prim else {
                return original_ty;
            };

            if negated {
                // typeof x !== "string" - remove string from union
                self.remove_primitive_from_union(original_ty, target_prim)
            } else {
                // typeof x === "string" - narrow to string
                self.type_ctx.intern(Type::Primitive(target_prim))
            }
        } else {
            original_ty
        }
    }

    /// Remove a primitive type from a bare union
    fn remove_primitive_from_union(
        &mut self,
        union_id: TypeId,
        to_remove: PrimitiveType,
    ) -> TypeId {
        if let Some(Type::Union(union)) = self.type_ctx.get(union_id) {
            // Filter out the primitive to remove
            let remaining: Vec<TypeId> = union.members.iter()
                .filter(|&&member| {
                    if let Some(Type::Primitive(prim)) = self.type_ctx.get(member) {
                        *prim != to_remove
                    } else {
                        true
                    }
                })
                .copied()
                .collect();

            if remaining.len() == 1 {
                // Only one member left, unwrap union
                remaining[0]
            } else if remaining.is_empty() {
                // No members left, return never type
                self.type_ctx.never_type()
            } else {
                // Create new union with remaining members
                self.type_ctx.union_type(remaining)
            }
        } else {
            union_id
        }
    }
}
```

### Task 3.3: Exhaustiveness Checking for typeof

**Modify:** `crates/raya-checker/src/exhaustiveness.rs`

```rust
/// Check exhaustiveness for typeof-based switch on bare union
pub fn check_typeof_exhaustiveness(
    ctx: &TypeContext,
    bare_union: TypeId,
    tested_types: &HashSet<String>,
) -> Result<(), Vec<String>> {
    if let Some(Type::Union(union)) = ctx.get(bare_union) {
        if !union.is_bare {
            return Ok(());  // Not a bare union
        }

        // Extract all primitive types
        let all_types: HashSet<String> = union.members.iter()
            .filter_map(|&member| {
                if let Some(Type::Primitive(prim)) = ctx.get(member) {
                    Some(prim.type_name().to_string())
                } else {
                    None
                }
            })
            .collect();

        // Find missing types
        let missing: Vec<String> = all_types.iter()
            .filter(|t| !tested_types.contains(*t))
            .cloned()
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    } else {
        Ok(())
    }
}
```

### Task 3.4: Forbid Access to $type/$value Fields

**Modify:** `crates/raya-checker/src/checker.rs`

```rust
impl TypeChecker<'_> {
    /// Check member access expression
    fn check_member_access(&mut self, member: &MemberExpr) -> TypeId {
        let object_ty = self.check_expr(&member.object);

        // Forbid access to $type and $value on bare unions
        if member.property == "$type" || member.property == "$value" {
            if let Some(Type::Union(union)) = self.type_ctx.get(object_ty) {
                if union.is_bare {
                    self.errors.push(CheckError::ForbiddenFieldAccess {
                        field: member.property.clone(),
                        span: member.span,
                        hint: "Bare union internal fields cannot be accessed directly. Use typeof for type narrowing.".to_string(),
                    });
                    return self.type_ctx.error_type();
                }
            }
        }

        // Normal property access
        self.check_property_access(object_ty, &member.property)
    }
}
```

### Verification (Phase 3)

**Tests:** `crates/raya-checker/tests/bare_union_narrowing_test.rs`

```rust
#[test]
fn test_typeof_narrowing_string_number() {
    let source = r#"
        type ID = string | number;

        function process(id: ID): string {
            if (typeof id === "string") {
                return id.toUpperCase();  // id is string
            } else {
                return id.toString();  // id is number
            }
        }
    "#;

    let result = parse_and_check(source);
    assert!(result.is_ok());
}

#[test]
fn test_typeof_switch_exhaustiveness() {
    let source = r#"
        type Value = string | number | boolean;

        function describe(v: Value): string {
            switch (typeof v) {
                case "string": return "str";
                case "number": return "num";
                case "boolean": return "bool";
            }
        }
    "#;

    let result = parse_and_check(source);
    assert!(result.is_ok());
}

#[test]
fn test_non_exhaustive_typeof_switch() {
    let source = r#"
        type Value = string | number | boolean;

        function describe(v: Value): string {
            switch (typeof v) {
                case "string": return "str";
                case "number": return "num";
                // Missing "boolean" case
            }
        }
    "#;

    let result = parse_and_check(source);
    assert!(result.is_err());
    // Should report missing "boolean" case
}

#[test]
fn test_forbid_dollar_type_access() {
    let source = r#"
        type ID = string | number;
        let id: ID = "abc";
        let t = id.$type;  // ERROR: cannot access $type
    "#;

    let result = parse_and_check(source);
    assert!(result.is_err());
}

#[test]
fn test_typeof_negation() {
    let source = r#"
        type ID = string | number;

        function process(id: ID): number {
            if (typeof id !== "string") {
                return id;  // id is number
            } else {
                return id.length;  // id is string
            }
        }
    "#;

    let result = parse_and_check(source);
    assert!(result.is_ok());
}
```

**Success Criteria:**
- ✅ typeof operator narrows bare unions correctly
- ✅ Exhaustiveness checking for typeof switches
- ✅ Access to $type/$value forbidden with clear error
- ✅ 5+ integration tests passing

---

## Testing Strategy

### Unit Tests

**Coverage:**
- Bare union detection algorithm
- Primitive type validation
- Transformation to internal representation
- typeof guard extraction
- Type narrowing logic

**Files:**
- `crates/raya-types/tests/bare_union_test.rs`
- `crates/raya-checker/tests/bare_union_narrowing_test.rs`

### Integration Tests

**Test complete programs:**

```rust
#[test]
fn test_complete_bare_union_program() {
    let source = r#"
        type Response = string | { code: number; message: string };

        function handle(resp: Response): void {
            if (typeof resp === "string") {
                console.log(resp);
            } else {
                console.log(`Error ${resp.code}: ${resp.message}`);
            }
        }
    "#;
    // Parse → Bind → Check
}
```

### Error Message Tests

Verify helpful error messages:

```rust
#[test]
fn test_bare_union_error_messages() {
    // Test: Non-primitive in bare union
    // Test: Duplicate primitives
    // Test: Access to $type/$value
    // Test: Non-exhaustive typeof switch
}
```

---

## Success Criteria

**Phase 1:**
- ✅ Bare primitive union detection working
- ✅ Validation of primitive-only constraint
- ✅ Rejection of duplicate primitives
- ✅ 4+ unit tests passing

**Phase 2:**
- ✅ Automatic transformation to internal representation
- ✅ Correct `{ $type, $value }` structure
- ✅ Discriminant inference on internal union
- ✅ 3+ transformation tests passing

**Phase 3:**
- ✅ typeof operator for type narrowing
- ✅ Exhaustiveness checking for typeof switches
- ✅ Forbidden field access enforcement
- ✅ 5+ integration tests passing

**Overall:**
- ✅ All primitive combinations supported
- ✅ Zero runtime overhead (direct Value enum usage)
- ✅ Seamless user experience (no manual boxing)
- ✅ 12+ tests passing across all phases
- ✅ Clear error messages for violations

---

## Files to Create

**New Files:**
- `crates/raya-types/src/bare_union.rs` - Detection and transformation
- `crates/raya-types/tests/bare_union_test.rs` - Unit tests
- `crates/raya-checker/tests/bare_union_narrowing_test.rs` - Integration tests

---

## Files to Modify

**Update:**
- `crates/raya-types/src/lib.rs` - Export bare_union module
- `crates/raya-types/src/ty.rs` - Add is_bare and internal_union fields to UnionType
- `crates/raya-types/src/context.rs` - Integrate bare union detection in union_type()
- `crates/raya-checker/src/type_guards.rs` - Add typeof guard support
- `crates/raya-checker/src/narrowing.rs` - Add typeof narrowing
- `crates/raya-checker/src/exhaustiveness.rs` - Add typeof exhaustiveness
- `crates/raya-checker/src/checker.rs` - Forbid $type/$value access

---

## Dependencies

**Requires from Previous Milestones:**
- ✅ Milestone 2.6: Discriminant inference (for internal union)
- ✅ Milestone 2.5: Type checker (for typeof integration)
- ✅ Milestone 2.4: Type system foundation

**Provides for Future Milestones:**
- Bare union support for code generation (Milestone 3.x)
- typeof opcode implementation (VM integration)
- Value enum discriminant checking

---

## Performance Considerations

### No Runtime Overhead

**Key Optimization:** Don't create wrapper objects!

```typescript
// Source:
let id: string | number = 42;

// Type system sees:
// { $type: "number", $value: number }

// Bytecode actually stores:
// Value::I32(42) - no wrapper object

// typeof check:
// TYPEOF opcode checks Value enum tag directly
```

### Value Enum Strategy

The VM's `Value` enum already has type discrimination:

```rust
pub enum Value {
    I32(i32),       // Tag = 0
    F64(f64),       // Tag = 1
    String(...),    // Tag = 2
    Boolean(bool),  // Tag = 3
    Null,           // Tag = 4
}
```

**typeof implementation:**
```rust
// TYPEOF opcode (pseudocode)
match value {
    Value::I32(_) => push_string("int"),
    Value::F64(_) => push_string("float"),
    Value::String(_) => push_string("string"),
    Value::Boolean(_) => push_string("boolean"),
    Value::Null => push_string("null"),
}
```

**Benefits:**
- ✅ Zero allocation
- ✅ Single instruction for type check
- ✅ No indirection
- ✅ Cache-friendly

---

## Example Usage

### Simple Bare Union

```typescript
type ID = string | number;

function processID(id: ID): string {
    if (typeof id === "string") {
        return id.toUpperCase();
    } else {
        return id.toString();
    }
}

// Compiler generates efficient code:
// - No boxing/unboxing
// - Direct Value enum checks
// - Type-safe narrowing
```

### Complex Example

```typescript
type Value = int | float | string | boolean | null;

function describe(v: Value): string {
    switch (typeof v) {
        case "int":
            return `Integer: ${v}`;
        case "float":
            return `Float: ${v.toFixed(2)}`;
        case "string":
            return `String: "${v}"`;
        case "boolean":
            return `Boolean: ${v}`;
        case "null":
            return "Null";
    }
}

// Exhaustiveness checked at compile time
// Efficient switch via Value enum tag
```

### Mixed with Discriminated Unions

```typescript
// Bare union (primitives)
type ID = string | number;

// Discriminated union (complex types)
type Result<T> =
    | { status: "ok"; value: T }
    | { status: "error"; error: string };

function process(id: ID): Result<string> {
    if (typeof id === "string") {
        return { status: "ok", value: id };
    } else {
        return { status: "ok", value: id.toString() };
    }
}
```

---

## Next Steps

This milestone provides the foundation for:
- **Milestone 3.x**: Code generation for bare unions
- **VM Implementation**: typeof opcode
- **Standard Library**: match() utility for bare unions

---

**End of Milestone 2.7 Specification**
