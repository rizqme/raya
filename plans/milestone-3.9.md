# Milestone 3.9: Decorators

**Status:** In Progress (Phase 2 partial)
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

### Phase 2: Type Checking

Type-check decorator applications.

**Tasks:**
- [ ] Add built-in decorator type aliases to builtins
- [ ] Add `Class<T>` type representation
- [ ] Type-check class decorators: `(Class<T>) => Class<T> | void`
- [ ] Type-check method decorators: `(F) => F`
  - [ ] Verify decorated method matches type parameter `F`
  - [ ] Report compile error if signature doesn't match
- [ ] Type-check field decorators: `(T, string) => void`
- [ ] Type-check parameter decorators: `(T, string, number) => void`
- [ ] Handle decorator factories (call factory, check returned decorator)
- [ ] Add type checker tests

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

### Phase 3: Code Generation

Lower decorators to IR and generate bytecode.

**Tasks:**
- [ ] Lower decorator applications to IR
- [ ] Generate decorator calls in correct order:
  1. Field decorators (declaration order)
  2. Parameter decorators (parameter order)
  3. Method decorators (declaration order)
  4. Class decorators (bottom-up for multiple)
- [ ] Handle decorator factories (call factory, then decorator)
- [ ] Method decorators: replace method with wrapped version
- [ ] Add codegen tests

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

Implement runtime types and Reflect API.

**Tasks:**
- [ ] Implement `Class<T>` runtime representation
  - [ ] `name: string` property
  - [ ] `prototype: T` property
  - [ ] Constructor callable
- [ ] Implement `Reflect` built-in object
  - [ ] `Reflect.define<T>(key, value, target)` - store metadata
  - [ ] `Reflect.define<T>(key, value, target, propertyKey)` - store property metadata
  - [ ] `Reflect.get<T>(key, target)` - retrieve metadata
  - [ ] `Reflect.get<T>(key, target, propertyKey)` - retrieve property metadata
  - [ ] `Reflect.has(key, target)` - check metadata exists
  - [ ] `Reflect.keys(target)` - list metadata keys
- [ ] WeakMap-based metadata storage (GC friendly)
- [ ] Add runtime tests

---

### Phase 5: Integration Tests

End-to-end decorator tests.

**Tasks:**
- [ ] Class decorator tests
  - [ ] Simple decorator
  - [ ] Decorator factory
  - [ ] Multiple decorators (order verification)
  - [ ] Decorator returning new class
- [ ] Method decorator tests
  - [ ] Type-constrained decorator (verify compile error on mismatch)
  - [ ] Function wrapping
  - [ ] Decorator factory
  - [ ] Multiple method decorators
- [ ] Field decorator tests
  - [ ] Simple field decorator
  - [ ] Decorator factory
  - [ ] Multiple fields
- [ ] Parameter decorator tests
  - [ ] Constructor parameter
  - [ ] Method parameter
  - [ ] Multiple parameters
- [ ] Framework pattern tests
  - [ ] HTTP routing (@GET, @POST)
  - [ ] Dependency injection (@Injectable, @Inject)
  - [ ] Validation (@Min, @Max, @Email)

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

---

## Dependencies

- Milestone 3.7 (module system) - for importing decorator definitions
- Type checker infrastructure (generics, function types)
- IR lowering (function calls)

---

## Estimated Scope

| Phase | Tasks | Complexity |
|-------|-------|------------|
| Phase 1: Parser | ✅ Complete | - |
| Phase 2: Type Checking | 8 tasks | High |
| Phase 3: Code Generation | 5 tasks | Medium |
| Phase 4: Runtime | 8 tasks | Medium |
| Phase 5: Integration | 12 tasks | Medium |

**Total:** ~33 tasks remaining

---

## Success Criteria

1. All decorator types parse correctly
2. Type mismatches produce compile errors (not runtime)
3. Method decorators enforce signature constraints
4. Decorator evaluation order matches spec
5. Reflect API stores/retrieves metadata
6. Framework patterns work (routing, DI, validation)
