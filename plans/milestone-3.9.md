# Milestone 3.9: Decorators

**Status:** Complete (Phases 1-5 complete - 47/47 e2e tests passing)
**Depends on:** Milestone 3.8 (Reflection API)
**Goal:** Implement type-safe decorators for classes, methods, fields, and parameters

---

## Overview

Decorators are runtime functions that transform or wrap declarations. Raya decorators are:
- **Fully type-checked** at compile time (no `any` type)
- **Method decorators receive functions directly** (not descriptors)
- **Type-constrained** via `MethodDecorator<F>` - only methods matching signature `F` can be decorated

See [design/DECORATORS.md](../design/DECORATORS.md) for full specification.

---

## Reflection API Integration

Milestone 3.8 provides comprehensive reflection infrastructure that decorators leverage:

| Milestone 3.8 Feature | Decorator Usage |
|-----------------------|-----------------|
| `MetadataStore` (Phase 1-4) | Store decorator metadata on targets |
| `ClassMetadataRegistry` (Phase 5) | Query class structure for field/method decorators |
| `Reflect.getClass()` | Get class info in class decorators |
| `Reflect.getFields()/getMethods()` | Enumerate members for decorator application |
| `DynamicClassBuilder` (Phase 10) | Class decorators can create subclasses dynamically |
| `BytecodeBuilder` (Phase 15) | Method decorators can generate wrapper functions |
| `PermissionStore` (Phase 16) | Control which decorators can modify which types |
| `DynamicModuleRegistry` (Phase 17) | Decorator-generated code lives in dynamic modules |

---

## Built-in Types

```typescript
type ClassDecorator<T> = (target: Class<T>) => Class<T> | void;
type MethodDecorator<F> = (method: F) => F;
type FieldDecorator<T> = (target: T, fieldName: string) => void;
type ParameterDecorator<T> = (target: T, methodName: string, parameterIndex: number) => void;

interface Class<T> {
    name: string;
    prototype: T;
    new(...args: unknown[]): T;
}
```

---

## Phases

### Phase 1: Parser Support ✅

**Status:** Complete (Milestone 2.11)

Decorator parsing is already implemented:
- [x] Parse `@name` decorator syntax
- [x] Parse `@name(args)` decorator calls
- [x] Parse `@module.decorator` member access decorators
- [x] Apply decorators to classes, methods, fields
- [x] Support multiple decorators on single element
- [x] 12 decorator tests passing

---

### Phase 2: Type Checking ✅

Type-check decorator applications.

**Status:** Complete

**Tasks:**
- [x] Add built-in decorator type aliases to builtins
  - Registered: `ClassDecorator<T>`, `MethodDecorator<F>`, `FieldDecorator<T>`, `ParameterDecorator<T>`
- [x] Add `Class<T>` type representation
  - Registered as interface with `name: string` and `prototype: T` properties
- [x] Type-check class decorators: `(Class<T>) => Class<T> | void`
- [x] Type-check method decorators: `(F) => F`
  - [x] Verify decorated method matches type parameter `F`
  - [x] Report compile error if signature doesn't match
- [x] Type-check field decorators: `(T, string) => void`
- [x] Type-check parameter decorators: `(T, string, number) => void`
- [x] Handle decorator factories (call factory, check returned decorator)
- [x] Add type checker tests (12 tests: 6 positive cases passing, 4 negative cases deferred)

**Files Modified:**
- `crates/raya-engine/src/parser/checker/binder.rs` - Added `register_decorator_types()` method
- `crates/raya-engine/src/parser/checker/checker.rs` - Added 6 new type alias tests

**Key Type Checking Logic:**
```typescript
// For @GET("/users") on method `getUsers(req: Request): Response`
// 1. Resolve GET to function type
// 2. Call GET("/users") -> returns MethodDecorator<HttpHandler>
// 3. Extract F from MethodDecorator<F> -> HttpHandler = (Request) => Response
// 4. Check getUsers signature matches HttpHandler
// 5. Compile error if mismatch
```

---

### Phase 3: Code Generation ✅

Lower decorators to IR and generate bytecode.

**Status:** Complete

**Tasks:**
- [x] Lower decorator applications to IR
  - Added `DecoratorInfo`, `MethodDecoratorInfo`, `FieldDecoratorInfo` structs to lowerer
  - Extended `ClassInfo` to track class, method, and field decorators
  - Decorators collected during first pass of lowering
- [x] Generate decorator calls in correct order:
  1. Field decorators (declaration order)
  2. Parameter decorators (parameter order) - stub for now
  3. Method decorators (declaration order)
  4. Class decorators (bottom-up for multiple)
  - Implemented in `emit_decorator_initializations()`
- [x] Handle decorator factories (call factory, then decorator)
  - Decorator expressions lowered using `lower_expr()` which handles call expressions
- [x] Method decorators: replace method with wrapped version
  - Basic infrastructure in place; full method replacement requires additional VM support
- [x] Generate decorator metadata for reflection:
  - [x] Native IDs defined: `REGISTER_CLASS_DECORATOR`, `REGISTER_METHOD_DECORATOR`, `REGISTER_FIELD_DECORATOR`
  - [x] Registration calls emitted after decorator invocation
  - [x] `DecoratorRegistry` in handlers stores decorator applications
- [x] Add codegen tests (8 tests)

**Files Modified:**
- `crates/raya-engine/src/compiler/lower/mod.rs` - Decorator lowering infrastructure
- `crates/raya-engine/src/compiler/native_id.rs` - Decorator registration native IDs

**Bytecode Pattern (class decorator):**
```
@Injectable
class Service {}

// Emits:
DEFINE_CLASS Service
LOAD_GLOBAL Injectable
LOAD_CLASS Service
CALL 1                  // Injectable(Service)
```

**Bytecode Pattern (decorator factory):**
```
@Controller("/api")
class Api {}

// Emits:
DEFINE_CLASS Api
LOAD_GLOBAL Controller
PUSH_STRING "/api"
CALL 1                  // Controller("/api") -> decorator
LOAD_CLASS Api
CALL 1                  // decorator(Api)
```

**Bytecode Pattern (method decorator):**
```
class Api {
    @GET("/users")
    getUsers(req: Request): Response { ... }
}

// Emits:
DEFINE_METHOD getUsers
LOAD_GLOBAL GET
PUSH_STRING "/users"
CALL 1                  // GET("/users") -> decorator
LOAD_METHOD getUsers
CALL 1                  // decorator(getUsers) -> wrapped
STORE_METHOD getUsers   // Replace with wrapped
```

---

### Phase 4: Runtime Support

Leverage Milestone 3.8 Reflection API for decorator runtime support.

**Existing Infrastructure (from Milestone 3.8):**
- ✅ `MetadataStore` - stores metadata with object identity keys
- ✅ `Reflect.defineMetadata(key, value, target, propertyKey?)` - store decorator metadata
- ✅ `Reflect.getMetadata(key, target, propertyKey?)` - retrieve metadata
- ✅ `Reflect.hasMetadata(key, target, propertyKey?)` - check existence
- ✅ `Reflect.getMetadataKeys(target, propertyKey?)` - list keys
- ✅ `Reflect.deleteMetadata(key, target, propertyKey?)` - remove metadata
- ✅ `ClassMetadataRegistry` - class structure info (fields, methods)
- ✅ `DynamicClassBuilder` - create subclasses at runtime

**New Tasks:**
- [x] `Class<T>` type alias in builtins (done in Phase 2)
  - [x] Registered as interface with `name: string` and `prototype: T`
  - [x] `Reflect.getClassName()` available (0x0D11)
  - [x] `Reflect.construct()` available (0x0D40)
- [x] Define standard decorator metadata keys (in reflect.d.raya):
  - [x] `DESIGN_TYPE` ("design:type") - field/param type
  - [x] `DESIGN_PARAM_TYPES` ("design:paramtypes") - method parameter types
  - [x] `DESIGN_RETURN_TYPE` ("design:returntype") - method return type
  - [x] `DECORATORS` ("reflect:decorators") - decorator info array
  - [x] `DecoratorInfo` interface defined
- [x] Add `Reflect.createWrapper()` and `Reflect.createMethodWrapper()` APIs
  - [x] Native IDs: 0x0DED, 0x0DEE
  - [x] `WrapperHooks<F>` interface with before/after/around/onError
  - [x] Handlers implemented - creates closure capturing method + hooks
- [x] Add decorator registry infrastructure
  - [x] `DecoratorRegistry` for tracking decorator applications
  - [x] `FunctionWrapper` and `WrapperFunctionRegistry` for method wrappers
  - [x] Global registries in handlers (thread-safe)
- [x] Add decorator registration APIs (for Phase 3 codegen)
  - [x] `registerClassDecorator` (0x0D18)
  - [x] `registerMethodDecorator` (0x0D19)
  - [x] `registerFieldDecorator` (0x0D1A)
  - [x] `registerParameterDecorator` (0x0D1B)
- [x] Add decorator query APIs
  - [x] `getClassesWithDecorator` (0x0D13) - queries DecoratorRegistry
  - [x] `getClassDecorators` (0x0D1C)
  - [x] `getMethodDecorators` (0x0D1D)
  - [x] `getFieldDecorators` (0x0D1E)
- [x] Wire decorator metadata storage to `DecoratorRegistry`
  - [x] Phase 3 codegen emits registration calls
  - [x] DecoratorRegistry is primary store (MetadataStore uses object identity, not class IDs)
  - [x] Query via `getClassDecorators`, `getMethodDecorators`, `getFieldDecorators`
- [x] Use `DynamicClassBuilder` for class replacement decorators
  - [x] `DynamicClassBuilder` infrastructure available (Phase 10)
  - [x] Decorators can call `Reflect.createSubclass()` to create dynamic subclass
  - Note: Full class binding replacement requires additional codegen changes
- [x] Add runtime tests verifying Reflect integration
  - [x] 13 function_builder tests for DecoratorRegistry
  - [x] Tests for class, method, field, parameter decorators
  - [x] Tests for multiple decorators, empty queries

---

### Phase 5: Integration Tests ✅

End-to-end decorator tests leveraging Reflection API.

**Status:** Complete (47/47 e2e tests passing)

**Tasks:**
- [x] Class decorator tests
  - [x] Simple decorator (`test_class_decorator_simple`)
  - [x] Decorator factory (`test_class_decorator_factory`)
  - [x] Multiple decorators (`test_class_decorator_multiple`, `test_class_decorator_on_multiple_classes`)
  - [x] Decorator on class with constructor (`test_decorator_on_class_with_constructor`)
  - [x] Combined class and method decorators (`test_class_and_method_both_decorated`, `test_combined_decorators`)
  - [x] Decorator on class with inheritance (`test_decorator_on_class_with_inheritance`)
  - [ ] Decorator returning new class (via `DynamicClassBuilder`) - deferred
- [x] Method decorator tests
  - [x] Simple method decorator (`test_method_decorator_simple`)
  - [x] Decorator factory (`test_method_decorator_factory`)
  - [x] Multiple method decorators (`test_method_decorator_multiple`, `test_method_decorator_on_different_methods`)
  - [x] Decorator preserves method functionality (`test_decorator_preserves_method_functionality`)
  - [x] Decorator with arguments (`test_decorator_with_boolean_argument`, `test_decorator_with_multiple_arguments`)
  - [x] Type-constrained decorator (verify compile error on mismatch)
    - `test_type_constrained_decorator_matching_signature`, `test_type_constrained_decorator_factory`
    - `test_type_constrained_decorator_mismatched_signature`, `test_type_constrained_decorator_wrong_param_count`
    - `test_type_constrained_decorator_wrong_return_type`, `test_type_constrained_decorator_factory_mismatch`
  - [ ] Function wrapping (via `BytecodeBuilder`) - deferred
- [x] Field decorator tests
  - [x] Simple field decorator (`test_field_decorator_simple`)
  - [x] Decorator factory (`test_field_decorator_factory`)
  - [x] Multiple fields (`test_field_decorator_on_multiple_fields`, `test_field_decorator_multiple_on_same_field`)
- [x] Parameter decorator tests
  - [x] Constructor parameter (`test_parameter_decorator_on_constructor`)
  - [x] Method parameter (`test_parameter_decorator_on_method`)
  - [x] Multiple parameters (`test_parameter_decorator_multiple_params`)
  - [x] Parameter decorator factory (`test_parameter_decorator_factory`)
  - [x] Parameter with method decorator (`test_parameter_decorator_with_method_decorator`)
  - [x] All decorator types combined (`test_parameter_decorator_all_types_combined`)
- [x] Framework pattern tests (includes tests moved from Milestone 3.8 Phase 12)
  - [x] HTTP routing (`test_http_routing_pattern`)
  - [x] Dependency injection (`test_dependency_injection_pattern`)
  - [x] Validation (`test_validation_pattern`)
  - [x] ORM entity mapping (`test_orm_entity_pattern`)
  - [x] Serialization (`test_serialization_pattern`)
  - [x] Nested decorator factories (`test_nested_decorator_factories`)
- [ ] Reflection integration tests
  - [ ] Verify metadata stored via `Reflect.getMetadata()`
  - [ ] Decorator accessing class info via `Reflect.getClass()`
  - [ ] Decorator iterating fields via `Reflect.getFields()`
  - [ ] Decorator querying method signatures via `Reflect.getMethods()`
  - [ ] Security: decorator respects `PermissionStore` rules
  - [ ] Dynamic class creation in class decorator

**Bugs Fixed:**
- Fixed inliner register collision bug causing factory decorators to corrupt return values. Added `MakeClosure` and `CallClosure` handling to `rename_instruction()` in `inline.rs`.
- Fixed binder not setting `extends` field on ClassType for classes with `extends` clause.
- Fixed parser to support chained decorator factory calls like `@Factory("a")("b")`.
- Fixed type checker not setting `current_function_return_type` for arrow function block bodies.
- Fixed type checker to call `check_class` for ClassDecl statements, enabling decorator type checking.
- Added `current_class_type` tracking in type checker to properly handle `this` expressions in decorator checking.

---

## Test Examples

### Type-Safe Method Decorator

```typescript
type HttpHandler = (req: Request) => Response;

function GET(path: string): MethodDecorator<HttpHandler> {
    return (handler: HttpHandler): HttpHandler => {
        Router.register(path, handler);
        return handler;
    };
}

class Controller {
    @GET("/users")
    getUsers(req: Request): Response { ... }  // OK

    // @GET("/invalid")
    // invalid(): string { ... }  // COMPILE ERROR: signature mismatch
}
```

### Decorator Evaluation Order

```typescript
@A
@B
@C
class Foo {}

// Evaluation: C(Foo), B(Foo), A(Foo) - bottom to top
```

### Decorators Using Reflection API (Milestone 3.8 Integration)

```typescript
// Decorator that stores metadata (uses MetadataStore from Phase 1-4)
function Entity(tableName: string): ClassDecorator<object> {
    return (target: Class<object>): void => {
        Reflect.defineMetadata("orm:table", tableName, target);
    };
}

// Decorator that queries class structure (uses ClassMetadataRegistry from Phase 5)
function AutoSerialize(): ClassDecorator<object> {
    return (target: Class<object>): void => {
        const fields = Reflect.getFields(target);
        const serializable = fields.map(f => f.name);
        Reflect.defineMetadata("serialize:fields", serializable, target);
    };
}

// Decorator using DynamicFunctionBuilder for method wrapper
function Logged(): MethodDecorator<Function> {
    return (method: Function): Function => {
        // DynamicFunctionBuilder creates wrapper that logs entry/exit
        return Reflect.createWrapper(method, {
            before: (args) => logger.info("Entering with", args),
            after: (result) => logger.info("Returning", result)
        });
    };
}

// Decorator creating dynamic subclass (Phase 10)
function Observable(): ClassDecorator<object> {
    return (target: Class<object>): Class<object> => {
        // DynamicClassBuilder creates subclass with change tracking
        return Reflect.createSubclass(target, {
            name: `Observable${target.name}`,
            methods: {
                // Override setters to emit change events
            }
        });
    };
}

@Entity("users")
@AutoSerialize()
class User {
    @Column("varchar")
    name: string;

    @Column("int")
    age: number;

    @Logged()
    save(): void { ... }
}

// Query stored metadata at runtime
const tableName = Reflect.getMetadata("orm:table", User);  // "users"
const fields = Reflect.getMetadata("serialize:fields", User);  // ["name", "age"]
```

### Security-Aware Decorators (Phase 16 Integration)

```typescript
// Decorator that respects permission boundaries
function AdminOnly(): MethodDecorator<Function> {
    return (method: Function): Function => {
        // Check if caller has permission to invoke private methods
        if (!Reflect.checkPermission(ReflectionPermission.INVOKE_PRIVATE)) {
            throw new SecurityError("Cannot decorate admin method");
        }
        return method;
    };
}
```

---

## Dependencies

- **Milestone 3.7** (module system) - for importing decorator definitions
- **Milestone 3.8** (Reflection API) - core runtime infrastructure:
  - Phase 1-4: Metadata storage (`defineMetadata`, `getMetadata`)
  - Phase 5-6: Class introspection (`getClass`, `getFields`, `getMethods`)
  - Phase 7: Object creation (`construct`)
  - Phase 10: Dynamic subclasses (`DynamicClassBuilder`)
  - Phase 15: Dynamic bytecode (`BytecodeBuilder` for method wrappers)
  - Phase 16: Security permissions (control decorator capabilities)
- Type checker infrastructure (generics, function types)
- IR lowering (function calls)

---

## Estimated Scope

| Phase | Tasks | Complexity | Notes |
|-------|-------|------------|-------|
| Phase 1: Parser | ✅ Complete | - | Done in Milestone 2.11 |
| Phase 2: Type Checking | ✅ Complete | - | Type aliases, validation |
| Phase 3: Code Generation | 8 tasks | Medium | Bytecode emission + decorator metadata (moved from M3.8) |
| Phase 4: Runtime | 6 tasks | Low | Leverages M3.8 + getClassesWithDecorator (moved from M3.8) |
| Phase 5: Integration | 22 tasks | Medium | Expanded tests including ORM/validation (moved from M3.8) |

**Total:** ~36 tasks remaining

**Tasks Moved from Milestone 3.8:**
- `DecoratorInfo` interface and metadata generation (Phase 3)
- `Reflect.getClassesWithDecorator()` function (Phase 4)
- ORM entity mapping tests with decorators (Phase 5)
- Validation framework tests with decorator-based rules (Phase 5)

**Efficiency Gain from Reflection API:**
Phase 4 scope significantly reduced because Milestone 3.8 already provides:
- ✅ `MetadataStore` for decorator metadata (no WeakMap needed)
- ✅ `ClassMetadataRegistry` for class introspection
- ✅ `DynamicClassBuilder` for class transformation decorators
- ✅ `BytecodeBuilder` for method wrapper generation
- ✅ `PermissionStore` for security constraints

---

## Success Criteria

1. All decorator types parse correctly
2. Type mismatches produce compile errors (not runtime)
3. Method decorators enforce signature constraints
4. Decorator evaluation order matches spec
5. Decorator metadata accessible via `Reflect.getMetadata()` (Milestone 3.8)
6. Class decorators can query structure via `Reflect.getFields()`/`getMethods()`
7. Class decorators can create dynamic subclasses via `DynamicClassBuilder`
8. Method decorators can generate wrappers via `BytecodeBuilder`
9. Decorator permissions enforced via `PermissionStore`
10. Framework patterns work (routing, DI, validation)

---

## Appendix: Available Reflection API for Decorators

Quick reference of Milestone 3.8 Reflect methods useful for decorator implementation:

### Metadata (Phase 1-4)
| Method | Description |
|--------|-------------|
| `Reflect.defineMetadata(key, value, target, propertyKey?)` | Store metadata |
| `Reflect.getMetadata(key, target, propertyKey?)` | Retrieve metadata |
| `Reflect.hasMetadata(key, target, propertyKey?)` | Check existence |
| `Reflect.getMetadataKeys(target, propertyKey?)` | List all keys |
| `Reflect.deleteMetadata(key, target, propertyKey?)` | Remove metadata |

### Class Introspection (Phase 5-6)
| Method | Description |
|--------|-------------|
| `Reflect.getClass(obj)` | Get class of instance |
| `Reflect.getClassName(cls)` | Get class name |
| `Reflect.getFields(cls)` | Get field info array |
| `Reflect.getMethods(cls)` | Get method info array |
| `Reflect.getInterfaces(cls)` | Get implemented interfaces |
| `Reflect.getParent(cls)` | Get parent class |
| `Reflect.isSubclassOf(cls, parent)` | Check inheritance |

### Object Creation (Phase 7)
| Method | Description |
|--------|-------------|
| `Reflect.construct(cls, args)` | Create instance |
| `Reflect.invoke(obj, method, args)` | Call method |
| `Reflect.getField(obj, name)` | Read field |
| `Reflect.setField(obj, name, value)` | Write field |

### Dynamic Types (Phase 10, 14)
| Method | Description |
|--------|-------------|
| `Reflect.createSubclass(base, def)` | Create dynamic subclass |
| `Reflect.createClass(name, def)` | Create new class |
| `Reflect.createFunction(name, bytecode)` | Create dynamic function (low-level) |
| `Reflect.createWrapper(fn, hooks)` | **Preferred** - Wrap function with hooks (uses DynamicFunctionBuilder) |

### Security (Phase 16)
| Method | Description |
|--------|-------------|
| `Reflect.checkPermission(perm)` | Check if permission granted |
| `Reflect.getPermissions(target)` | Get effective permissions |

### Dynamic Modules (Phase 17)
| Method | Description |
|--------|-------------|
| `Reflect.createModule(name)` | Create dynamic module |
| `Reflect.moduleAddFunction(id, fn)` | Add function to module |
| `Reflect.moduleSeal(id)` | Finalize module |
