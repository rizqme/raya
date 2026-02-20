# checker module

Type checking, inference, and semantic analysis for Raya.

## Module Structure

```
checker/
├── mod.rs           # Entry point, TypeChecker, CheckResult
├── checker.rs       # Main type checking logic (~2500 lines)
├── binder.rs        # Name resolution, symbol binding
├── symbols.rs       # Symbol table management
├── narrowing.rs     # Control flow-based type narrowing
├── type_guards.rs   # Type guard analysis
├── exhaustiveness.rs# Exhaustiveness checking for unions
├── captures.rs      # Closure capture analysis
├── builtins.rs      # Built-in type signatures
└── diagnostic.rs    # Error reporting
```

## Key Types

### TypeChecker
```rust
pub struct TypeChecker<'a> {
    interner: &'a Interner,
    symbols: SymbolTable,
    type_ctx: TypeContext,
    errors: Vec<CheckError>,
}

checker.check(&module) -> CheckResult
```

### CheckResult
```rust
pub struct CheckResult {
    pub type_ctx: TypeContext,
    pub errors: Vec<CheckError>,
    pub expr_types: HashMap<usize, TypeId>, // expr ptr -> type
}

result.has_errors() -> bool
result.type_context() -> TypeContext
```

### SymbolTable
```rust
pub struct SymbolTable {
    scopes: Vec<Scope>,
    // ...
}

symbols.define(name, kind, type_id)
symbols.lookup(name) -> Option<SymbolInfo>
symbols.enter_scope()
symbols.exit_scope()
```

## Type Checking Features

### Type Inference
- Local variable types inferred from initializers
- Generic type argument inference
- Return type inference (when possible)

### Type Narrowing (`narrowing.rs`)
```typescript
let x: string | number = getValue();
if (typeof x === "string") {
    // x is narrowed to string here
    x.toUpperCase();
}
```

### Type Guards (`type_guards.rs`)
```typescript
function isString(x: unknown): x is string {
    return typeof x === "string";
}
```

### Exhaustiveness (`exhaustiveness.rs`)
```typescript
type Status = "ok" | "error";
function handle(s: Status) {
    switch (s) {
        case "ok": return;
        case "error": return;
        // Compiler ensures all cases covered
    }
}
```

### Capture Analysis (`captures.rs`)
Analyzes which variables are captured by closures for proper code generation.

## Error Types

```rust
pub enum CheckError {
    TypeMismatch { expected: TypeId, actual: TypeId, span: Span },
    UndefinedVariable { name: String, span: Span },
    NotCallable { ty: TypeId, span: Span },
    MissingProperty { name: String, ty: TypeId, span: Span },
    ReadonlyAssignment { property: String, span: Span },  // E2018
    // ... many more
}
```

### Readonly Enforcement
The checker rejects assignments to readonly properties (`obj.readonlyField = value`).
`this.field = value` is allowed inside constructors. Readonly is checked via `is_readonly_property()` which inspects `PropertySignature.readonly` on Class, Object, and Interface types.

## Built-in Types (`builtins.rs`)

Pre-defined signatures for:
- Primitives: `number`, `string`, `boolean`, `null`, `void`
- Built-in classes: `Array<T>`, `Map<K,V>`, `Set<T>`, etc.
- Global objects: `JSON`
- Decorator type aliases: `ClassDecorator<T>`, `MethodDecorator<F>`, `ParameterDecorator<T>`, etc.

## Scope Resolution

- `check_new()`, `check_member()` use `self.symbols.resolve_from_scope(&name, self.current_scope)` (NOT `self.symbols.resolve()`) to find classes/functions in nested scopes (e.g., classes inside functions)
- `check_object()` builds a `Type::Class` from object literal properties (not `unknown_type()`)
- Object destructuring defaults: when property not found, uses default expression type

## For AI Assistants

- Type checking is bidirectional (inference + checking)
- `typeof` is only for primitives, `instanceof` for classes
- Discriminated unions use a discriminant field (kind/type/tag)
- No `any` type - system is fully sound
- Errors include span for precise location
- `expr_types` maps expression pointers to their inferred types
- **Scope resolution**: Always use `resolve_from_scope` with `self.current_scope` when resolving names during type checking (not bare `resolve` which uses scope 0)
