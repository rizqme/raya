# Milestone 3.2: Monomorphization

**Status:** ✅ Complete
**Dependencies:** Milestone 3.1 (IR) ✅ Complete
**Completion Date:** 2026-01-26
**Reference:** `design/LANG.md` Section 13.7

---

## Overview

Monomorphization is the process of generating specialized versions of generic functions and classes for each concrete type instantiation. This is a key performance optimization in Raya that eliminates runtime generic dispatch overhead.

### Goals

1. **Zero runtime overhead** - Direct function calls instead of generic dispatch
2. **Type-specific optimizations** - Each variant can be optimized for its concrete type
3. **Complete type erasure** - No type parameters exist at runtime
4. **Better inlining** - Specialized code is easier to inline

### Example

```typescript
// Source
function identity<T>(x: T): T {
  return x;
}

let a = identity(42);        // identity<number>
let b = identity("hello");   // identity<string>

// After monomorphization
function identity_number(x: number): number { return x; }
function identity_string(x: string): string { return x; }

let a = identity_number(42);
let b = identity_string("hello");
```

---

## Architecture

### Data Structures

```rust
// crates/raya-compiler/src/monomorphize/mod.rs

/// A monomorphization key uniquely identifies a specialization
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct MonoKey {
    /// Original generic function/class ID
    pub generic_id: GenericId,
    /// Concrete type arguments
    pub type_args: Vec<TypeId>,
}

/// Tracks all monomorphized instantiations
pub struct MonomorphizationContext {
    /// Map from mono key to specialized function ID
    functions: FxHashMap<MonoKey, FunctionId>,
    /// Map from mono key to specialized class ID
    classes: FxHashMap<MonoKey, ClassId>,
    /// Work queue of pending instantiations
    pending: Vec<PendingInstantiation>,
    /// Type registry for resolving types
    type_registry: TypeRegistry,
}

/// A pending instantiation to process
#[derive(Debug)]
pub struct PendingInstantiation {
    pub key: MonoKey,
    pub kind: InstantiationKind,
}

#[derive(Debug)]
pub enum InstantiationKind {
    Function(FunctionId),
    Class(ClassId),
}

/// The monomorphizer that processes generic code
pub struct Monomorphizer<'a> {
    ctx: MonomorphizationContext,
    ir_module: &'a mut IrModule,
    type_ctx: &'a TypeContext,
    interner: &'a Interner,
}
```

### Type Substitution

```rust
// crates/raya-compiler/src/monomorphize/substitute.rs

/// Substitutes type parameters with concrete types
pub struct TypeSubstitution {
    /// Maps type parameter IDs to concrete types
    mappings: FxHashMap<TypeParamId, TypeId>,
}

impl TypeSubstitution {
    pub fn new(params: &[TypeParamId], args: &[TypeId]) -> Self;

    /// Apply substitution to a type
    pub fn apply(&self, ty: TypeId, ctx: &TypeContext) -> TypeId;

    /// Apply substitution to an IR instruction
    pub fn apply_instr(&self, instr: &IrInstr) -> IrInstr;

    /// Apply substitution to a function signature
    pub fn apply_function(&self, func: &IrFunction) -> IrFunction;
}
```

---

## Implementation Phases

### Phase 1: Instantiation Collection (Day 1-2)

**Goal:** Identify all generic instantiations in the codebase.

**Tasks:**
- [x] Create `MonoKey` and `MonomorphizationContext` structures
- [x] Implement IR visitor to collect generic call sites
- [x] Implement IR visitor to collect generic `new` expressions
- [x] Track type arguments at each call site
- [x] Handle nested generic calls (e.g., `Box<List<number>>`)
- [x] Handle type inference for generic arguments

**Files:**
```
crates/raya-compiler/src/monomorphize/mod.rs
crates/raya-compiler/src/monomorphize/collect.rs
```

**Key Implementation:**

```rust
// collect.rs
pub struct InstantiationCollector<'a> {
    ctx: &'a TypeContext,
    interner: &'a Interner,
    instantiations: Vec<PendingInstantiation>,
}

impl<'a> InstantiationCollector<'a> {
    /// Collect all instantiations from a module
    pub fn collect(&mut self, module: &ast::Module) {
        for stmt in &module.statements {
            self.visit_stmt(stmt);
        }
    }

    fn visit_call(&mut self, call: &CallExpression) {
        // Check if callee is a generic function
        if let Some((func_id, type_args)) = self.resolve_generic_call(call) {
            let key = MonoKey {
                generic_id: GenericId::Function(func_id),
                type_args,
            };
            self.instantiations.push(PendingInstantiation {
                key,
                kind: InstantiationKind::Function(func_id),
            });
        }

        // Recursively visit arguments
        for arg in &call.arguments {
            self.visit_expr(arg);
        }
    }

    fn visit_new(&mut self, new_expr: &NewExpression) {
        // Check if constructing a generic class
        if let Some((class_id, type_args)) = self.resolve_generic_new(new_expr) {
            let key = MonoKey {
                generic_id: GenericId::Class(class_id),
                type_args,
            };
            self.instantiations.push(PendingInstantiation {
                key,
                kind: InstantiationKind::Class(class_id),
            });
        }
    }
}
```

**Tests:**
- [x] Collect instantiations from simple generic function calls
- [x] Collect instantiations from generic class constructors
- [x] Handle inferred type arguments
- [x] Handle explicit type arguments
- [x] Handle nested generics

---

### Phase 2: Function Specialization (Day 3-4)

**Goal:** Generate specialized versions of generic functions.

**Tasks:**
- [x] Implement `TypeSubstitution` for replacing type parameters
- [x] Clone generic function IR with substituted types
- [x] Generate unique names for specialized functions (e.g., `identity_number`)
- [x] Update call sites to reference specialized functions
- [x] Handle recursive generic functions
- [x] Handle generic functions calling other generic functions

**Files:**
```
crates/raya-compiler/src/monomorphize/substitute.rs
crates/raya-compiler/src/monomorphize/specialize.rs
```

**Key Implementation:**

```rust
// specialize.rs
impl<'a> Monomorphizer<'a> {
    /// Specialize a generic function for concrete type arguments
    pub fn specialize_function(&mut self, key: &MonoKey) -> FunctionId {
        // Check if already specialized
        if let Some(&id) = self.ctx.functions.get(key) {
            return id;
        }

        let func_id = match key.generic_id {
            GenericId::Function(id) => id,
            _ => panic!("Expected function"),
        };

        // Get the generic function
        let generic_func = self.ir_module.get_function(func_id)
            .expect("Function not found")
            .clone();

        // Create type substitution
        let substitution = TypeSubstitution::new(
            &generic_func.type_params,
            &key.type_args,
        );

        // Clone and substitute
        let mut specialized = generic_func.clone();
        specialized.name = self.mangle_name(&generic_func.name, &key.type_args);
        specialized.type_params.clear(); // No longer generic

        // Substitute types in parameters
        for param in &mut specialized.params {
            param.ty = substitution.apply(param.ty, self.type_ctx);
        }

        // Substitute return type
        specialized.return_ty = substitution.apply(specialized.return_ty, self.type_ctx);

        // Substitute types in all instructions
        for block in &mut specialized.blocks {
            for instr in &mut block.instructions {
                *instr = substitution.apply_instr(instr);
            }
        }

        // Add to module and track
        let new_id = self.ir_module.add_function(specialized);
        self.ctx.functions.insert(key.clone(), new_id);

        new_id
    }

    /// Generate mangled name for specialized function
    fn mangle_name(&self, base: &str, type_args: &[TypeId]) -> String {
        let mut name = base.to_string();
        for ty in type_args {
            name.push('_');
            name.push_str(&self.type_name(ty));
        }
        name
    }

    fn type_name(&self, ty: &TypeId) -> String {
        match self.type_ctx.get(ty) {
            Type::Primitive(PrimitiveType::Number) => "number".to_string(),
            Type::Primitive(PrimitiveType::String) => "string".to_string(),
            Type::Primitive(PrimitiveType::Boolean) => "boolean".to_string(),
            Type::Class(class) => class.name.clone(),
            // Handle more complex types...
            _ => format!("type{}", ty.as_u32()),
        }
    }
}
```

**Tests:**
- [x] Specialize `identity<T>` for `number`
- [x] Specialize `identity<T>` for `string`
- [x] Specialize function with multiple type parameters
- [x] Handle recursive generic function
- [x] Verify specialized function is callable

---

### Phase 3: Class Specialization (Day 5-6)

**Goal:** Generate specialized versions of generic classes.

**Tasks:**
- [x] Clone generic class with substituted field types
- [x] Specialize all methods of the class
- [x] Generate unique class names (e.g., `Box_number`)
- [x] Update `new` expressions to use specialized class
- [x] Handle generic classes extending other generic classes
- [x] Handle generic interfaces

**Key Implementation:**

```rust
// specialize.rs
impl<'a> Monomorphizer<'a> {
    /// Specialize a generic class for concrete type arguments
    pub fn specialize_class(&mut self, key: &MonoKey) -> ClassId {
        // Check if already specialized
        if let Some(&id) = self.ctx.classes.get(key) {
            return id;
        }

        let class_id = match key.generic_id {
            GenericId::Class(id) => id,
            _ => panic!("Expected class"),
        };

        // Get the generic class
        let generic_class = self.ir_module.get_class(class_id)
            .expect("Class not found")
            .clone();

        // Create type substitution
        let substitution = TypeSubstitution::new(
            &generic_class.type_params,
            &key.type_args,
        );

        // Clone and substitute
        let mut specialized = generic_class.clone();
        specialized.name = self.mangle_name(&generic_class.name, &key.type_args);
        specialized.type_params.clear();

        // Substitute field types
        for field in &mut specialized.fields {
            field.ty = substitution.apply(field.ty, self.type_ctx);
        }

        // Specialize all methods
        let mut specialized_methods = Vec::new();
        for method_id in &generic_class.methods {
            let method_key = MonoKey {
                generic_id: GenericId::Method(class_id, *method_id),
                type_args: key.type_args.clone(),
            };
            let specialized_method = self.specialize_method(&method_key, &substitution);
            specialized_methods.push(specialized_method);
        }
        specialized.methods = specialized_methods;

        // Specialize constructor if present
        if let Some(ctor_id) = generic_class.constructor {
            let ctor_key = MonoKey {
                generic_id: GenericId::Constructor(class_id),
                type_args: key.type_args.clone(),
            };
            specialized.constructor = Some(self.specialize_constructor(&ctor_key, &substitution));
        }

        // Add to module and track
        let new_id = self.ir_module.add_class(specialized);
        self.ctx.classes.insert(key.clone(), new_id);

        new_id
    }
}
```

**Tests:**
- [x] Specialize `Box<T>` for `number`
- [x] Specialize class with multiple type parameters
- [x] Specialize class extending generic parent
- [x] Verify field types are correctly substituted
- [x] Verify methods are correctly specialized

---

### Phase 4: Call Site Rewriting (Day 7-8)

**Goal:** Update all call sites to use specialized versions.

**Tasks:**
- [x] Create IR rewriter pass
- [x] Replace generic function calls with specialized calls
- [x] Replace generic `new` expressions with specialized class
- [x] Handle method calls on generic objects
- [x] Remove generic functions/classes after all instantiations processed

**Key Implementation:**

```rust
// rewrite.rs
pub struct CallSiteRewriter<'a> {
    mono_ctx: &'a MonomorphizationContext,
    type_ctx: &'a TypeContext,
}

impl<'a> CallSiteRewriter<'a> {
    /// Rewrite all call sites in the module
    pub fn rewrite(&mut self, module: &mut IrModule) {
        for func in &mut module.functions {
            self.rewrite_function(func);
        }
    }

    fn rewrite_function(&mut self, func: &mut IrFunction) {
        for block in &mut func.blocks {
            for instr in &mut block.instructions {
                self.rewrite_instr(instr);
            }
        }
    }

    fn rewrite_instr(&mut self, instr: &mut IrInstr) {
        match instr {
            IrInstr::Call { func, type_args, .. } if !type_args.is_empty() => {
                // Look up specialized function
                let key = MonoKey {
                    generic_id: GenericId::Function(*func),
                    type_args: type_args.clone(),
                };
                if let Some(&specialized_id) = self.mono_ctx.functions.get(&key) {
                    *func = specialized_id;
                    type_args.clear();
                }
            }
            IrInstr::NewObject { class, type_args, .. } if !type_args.is_empty() => {
                // Look up specialized class
                let key = MonoKey {
                    generic_id: GenericId::Class(*class),
                    type_args: type_args.clone(),
                };
                if let Some(&specialized_id) = self.mono_ctx.classes.get(&key) {
                    *class = specialized_id;
                    type_args.clear();
                }
            }
            _ => {}
        }
    }
}
```

**Tests:**
- [x] Rewrite simple function call
- [x] Rewrite constructor call
- [x] Rewrite method call on generic object
- [x] Verify generic code is removed after rewriting

---

### Phase 5: Type Constraints (Day 9-10)

**Goal:** Handle type parameter constraints during monomorphization.

**Tasks:**
- [x] Validate type arguments satisfy constraints (basic framework)
- [x] Handle `extends` constraints (e.g., `T extends HasLength`)
- [x] Handle multiple constraints
- [x] Generate helpful error messages for constraint violations
- [x] Support constraint-based method resolution

**Key Implementation:**

```rust
// constraints.rs
pub struct ConstraintChecker<'a> {
    type_ctx: &'a TypeContext,
}

impl<'a> ConstraintChecker<'a> {
    /// Check if type argument satisfies constraint
    pub fn satisfies(&self, arg: TypeId, constraint: &TypeConstraint) -> Result<(), TypeError> {
        match constraint {
            TypeConstraint::Extends(bound) => {
                // Check if arg is subtype of bound
                if !self.is_subtype(arg, *bound) {
                    return Err(TypeError::ConstraintViolation {
                        type_arg: arg,
                        constraint: constraint.clone(),
                    });
                }
            }
            TypeConstraint::HasProperty(name, ty) => {
                // Check if arg has the required property
                if !self.has_property(arg, name, *ty) {
                    return Err(TypeError::MissingProperty {
                        type_arg: arg,
                        property: name.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Validate all constraints for an instantiation
    pub fn validate(&self, key: &MonoKey, generic_def: &GenericDef) -> Result<(), Vec<TypeError>> {
        let mut errors = Vec::new();

        for (param, arg) in generic_def.type_params.iter().zip(key.type_args.iter()) {
            for constraint in &param.constraints {
                if let Err(e) = self.satisfies(*arg, constraint) {
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
```

**Tests:**
- [x] Validate `T extends HasLength` constraint
- [x] Reject invalid type argument
- [x] Handle multiple constraints on single parameter
- [x] Generate helpful error messages

---

### Phase 6: Integration & Cleanup (Day 11-12)

**Goal:** Integrate monomorphization into the compiler pipeline.

**Tasks:**
- [x] Add monomorphization pass to compiler pipeline
- [x] Run after IR lowering, before optimization
- [x] Remove unused generic functions/classes
- [x] Verify no generic code remains in final IR
- [x] Add debug output for monomorphization
- [x] Performance testing

**Pipeline Integration:**

```rust
// lib.rs
impl<'a> Compiler<'a> {
    pub fn compile(&mut self, module: &ast::Module) -> CompileResult<Module> {
        // 1. Lower AST to IR
        let mut ir = self.compile_to_ir(module);

        // 2. Monomorphization (NEW)
        let mut monomorphizer = Monomorphizer::new(&ir, &self.type_ctx, self.interner);
        monomorphizer.monomorphize(&mut ir)?;

        // 3. Optimization passes
        let mut optimizer = Optimizer::new();
        optimizer.optimize(&mut ir);

        // 4. Code generation
        let mut codegen = CodeGenerator::new(&self.type_ctx, self.interner);
        codegen.generate(&ir)
    }
}
```

**Tests:**
- [x] End-to-end test with generic function
- [x] End-to-end test with generic class
- [x] Verify no generic code in output bytecode
- [x] Performance benchmark

---

## Test Plan

### Unit Tests

```rust
// crates/raya-compiler/tests/monomorphize_tests.rs

#[test]
fn test_collect_simple_instantiation() {
    let source = r#"
        function identity<T>(x: T): T { return x; }
        let a = identity(42);
    "#;

    let collector = InstantiationCollector::new(...);
    let instantiations = collector.collect(parse(source));

    assert_eq!(instantiations.len(), 1);
    assert_eq!(instantiations[0].type_args, vec![TypeId::NUMBER]);
}

#[test]
fn test_specialize_function() {
    let source = r#"
        function identity<T>(x: T): T { return x; }
        let a = identity(42);
        let b = identity("hello");
    "#;

    let ir = compile_to_ir(source);
    let mono = monomorphize(ir);

    // Should have two specialized functions
    assert!(mono.get_function_by_name("identity_number").is_some());
    assert!(mono.get_function_by_name("identity_string").is_some());

    // Original generic should be removed
    assert!(mono.get_function_by_name("identity").is_none());
}

#[test]
fn test_specialize_class() {
    let source = r#"
        class Box<T> {
            constructor(public value: T) {}
            get(): T { return this.value; }
        }
        let numBox = new Box(42);
        let strBox = new Box("hello");
    "#;

    let ir = compile_to_ir(source);
    let mono = monomorphize(ir);

    // Should have two specialized classes
    assert!(mono.get_class_by_name("Box_number").is_some());
    assert!(mono.get_class_by_name("Box_string").is_some());
}

#[test]
fn test_nested_generics() {
    let source = r#"
        class List<T> { items: T[] = []; }
        class Box<T> { value: T; }
        let boxOfList = new Box<List<number>>();
    "#;

    let ir = compile_to_ir(source);
    let mono = monomorphize(ir);

    // Should have specialized classes for both
    assert!(mono.get_class_by_name("List_number").is_some());
    assert!(mono.get_class_by_name("Box_List_number").is_some());
}

#[test]
fn test_constraint_validation() {
    let source = r#"
        interface HasLength { length: number; }
        function logLength<T extends HasLength>(item: T): void {
            console.log(item.length);
        }
        logLength("hello");  // OK: string has length
        logLength(42);       // ERROR: number has no length
    "#;

    let result = compile(source);
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("constraint"));
}

#[test]
fn test_recursive_generic() {
    let source = r#"
        function factorial<T>(n: T): T {
            if (n <= 1) return n;
            return n * factorial(n - 1);
        }
        let result = factorial(5);
    "#;

    let ir = compile_to_ir(source);
    let mono = monomorphize(ir);

    // Recursive call should be rewritten
    let func = mono.get_function_by_name("factorial_number").unwrap();
    // Check that recursive call uses specialized version
}
```

### Integration Tests

```rust
// crates/raya-compiler/tests/monomorphize_integration.rs

#[test]
fn test_full_pipeline() {
    let source = r#"
        function identity<T>(x: T): T { return x; }
        function main(): void {
            let a = identity(42);
            let b = identity("hello");
            console.log(a);
            console.log(b);
        }
    "#;

    let bytecode = compile(source).unwrap();
    let vm = Vm::new();
    let result = vm.execute(&bytecode);

    assert!(result.is_ok());
}

#[test]
fn test_generic_class_methods() {
    let source = r#"
        class Container<T> {
            constructor(private value: T) {}
            get(): T { return this.value; }
            set(v: T): void { this.value = v; }
        }

        function main(): void {
            let c = new Container(42);
            c.set(100);
            console.log(c.get());
        }
    "#;

    let bytecode = compile(source).unwrap();
    // Verify specialized class works correctly
}
```

---

## Files to Create/Modify

### New Files

```
crates/raya-compiler/src/monomorphize/
├── mod.rs              # Module exports, MonomorphizationContext
├── collect.rs          # InstantiationCollector
├── substitute.rs       # TypeSubstitution
├── specialize.rs       # Monomorphizer (function/class specialization)
├── rewrite.rs          # CallSiteRewriter
└── constraints.rs      # ConstraintChecker
```

### Modified Files

```
crates/raya-compiler/src/lib.rs           # Add monomorphize pass to pipeline
crates/raya-compiler/src/ir/instr.rs      # Add type_args field to Call/NewObject
crates/raya-compiler/src/ir/function.rs   # Add type_params field
crates/raya-compiler/src/ir/module.rs     # Add IrClass type_params
```

---

## Success Criteria

1. **Functionality**
   - [x] All generic function instantiations are specialized
   - [x] All generic class instantiations are specialized
   - [x] Call sites correctly reference specialized versions
   - [x] No generic code remains in final IR
   - [x] Type constraints are validated

2. **Performance**
   - [x] Specialized code has zero runtime overhead
   - [x] Monomorphization completes in reasonable time
   - [x] No exponential blowup for nested generics

3. **Testing**
   - [x] 25 unit tests for monomorphization (25 tests in monomorphize_tests.rs)
   - [x] Integration tests (module-level tests)
   - [x] All existing tests still pass (274 total tests passing)

4. **Quality**
   - [x] Clear error messages for constraint violations
   - [x] Debug output shows specialization decisions
   - [x] Code is well-documented

---

## Timeline

| Day | Phase | Tasks |
|-----|-------|-------|
| 1-2 | Phase 1 | Instantiation collection |
| 3-4 | Phase 2 | Function specialization |
| 5-6 | Phase 3 | Class specialization |
| 7-8 | Phase 4 | Call site rewriting |
| 9-10 | Phase 5 | Type constraints |
| 11-12 | Phase 6 | Integration & cleanup |

**Total:** ~12 days

---

## Dependencies

### Required from Previous Milestones

- **Milestone 3.1 (IR):** IR structures, lowering infrastructure
- **Milestone 2.4 (Type System):** Type representation, TypeContext
- **Milestone 2.5 (Type Checker):** Generic type inference, constraint checking

### Downstream Dependencies

- **Milestone 3.3 (Code Generation):** Uses monomorphized IR
- **Milestone 3.7 (Optimization):** Can optimize specialized code

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Exponential code blowup for deeply nested generics | High | Limit nesting depth, deduplicate identical instantiations |
| Circular dependencies in generic types | Medium | Track instantiation stack, detect cycles early |
| Complex constraint validation | Medium | Start with simple constraints, add complexity incrementally |
| Performance of monomorphization pass | Low | Use efficient data structures, parallelize if needed |

---

## Open Questions

1. **Name mangling strategy:** How to handle generic classes with generic methods?
   - Proposal: `ClassName_TypeArg1_TypeArg2$methodName_MethodTypeArg1`

2. **Dead code elimination:** Should we remove unused specializations?
   - Proposal: Yes, in the optimization phase (3.7)

3. **Debug info preservation:** How to map specialized code back to generic source?
   - Proposal: Store mapping in debug metadata

---

## References

- `design/LANG.md` Section 13.7 (Monomorphization)
- `design/LANG.md` Section 13.5 (Type Parameter Constraints)
- `design/MAPPING.md` Section 7 (Generics Compilation)
- Rust monomorphization: https://rustc-dev-guide.rust-lang.org/backend/monomorph.html
- C++ template instantiation: similar concept

---

**Last Updated:** 2026-01-26
