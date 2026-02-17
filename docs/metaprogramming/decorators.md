---
title: "Decorators"
---

# Decorators in Raya

> **Status:** Implemented (41/41 e2e tests)
> **Milestone:** M3.9
> **Related:** [Reflection API](./reflection.md), [Language Spec](../language/lang.md)

---

## Overview

Decorators in Raya are **runtime functions** that transform or wrap declarations. Key differences from TypeScript:

1. **Statically typed** - No `any` type; decorator signatures are fully type-checked
2. **Method decorators receive the function directly** - Not a descriptor, just `(method: F) => F`
3. **Type-constrained** - `MethodDecorator<F>` only applies to methods matching signature `F`

---

## Design Goals

1. **Familiar Syntax** - Use `@decorator` syntax like TypeScript
2. **Static Typing** - All decorator signatures are type-checked at compile time
3. **Runtime Execution** - Decorators execute at class definition time
4. **Framework Support** - Enable patterns like `@GET("/path")`, `@Injectable`, `@Column`
5. **No Magic** - Decorators are regular functions with known signatures

---

## Decorators vs Annotations

Raya has two distinct metadata systems:

| Feature | Annotations | Decorators |
|---------|-------------|------------|
| Syntax | `//@@tag value` | `@name` or `@name(args)` |
| Timing | Compile-time only | Runtime |
| Purpose | Code generation hints | Registration, wrapping, validation |
| Type-checked | No (comments) | Yes (function signatures) |
| Example | `//@@json user_name` | `@GET("/users")` |

**When to use which:**
- **Annotations**: Compiler directives (JSON field mapping, optimization hints)
- **Decorators**: Framework patterns (routing, DI, ORM, validation)

---

## Decorator Types

### Built-in Decorator Type Aliases

```typescript
// Provided by the runtime/compiler
type ClassDecorator<T> = (target: Class<T>) => Class<T> | void;
type MethodDecorator<F> = (method: F) => F;
type FieldDecorator<T> = (target: T, fieldName: string) => void;
type ParameterDecorator<T> = (target: T, methodName: string, parameterIndex: number) => void;
```

**Key insight:** Method decorators receive the **function directly** (already bound with `this`). The type parameter `F` constrains which methods can be decorated - the compiler verifies the decorated method matches `F`.

### Type-Safe Method Constraints (Key Innovation)

This is Raya's key advantage over TypeScript decorators:

```typescript
// TypeScript: @GET can be applied to ANY method (no type checking)
// Raya: @GET can ONLY be applied to methods matching HttpHandler

type HttpHandler = (req: Request) => Response;

function GET(path: string): MethodDecorator<HttpHandler> {
    return (handler: HttpHandler): HttpHandler => {
        Router.register(path, handler);
        return handler;
    };
}

class Controller {
    // VALID - signature matches HttpHandler
    @GET("/users")
    getUsers(req: Request): Response { ... }

    // COMPILE ERROR - wrong signature!
    // Expected: (req: Request) => Response
    // Got: () => string
    @GET("/invalid")
    invalid(): string { ... }
}
```

The compiler enforces that decorated methods match the expected signature at **compile time**, not runtime.

### The `Class<T>` Type

`Class<T>` represents the constructor/prototype of a class. It provides:

```typescript
interface Class<T> {
    name: string;                    // Class name
    prototype: T;                    // Prototype object
    new(...args: unknown[]): T;      // Constructor signature
}
```

---

## Syntax

### Class Decorators

```typescript
@Injectable
class UserService {
    // ...
}

// Decorator function
function Injectable<T>(target: Class<T>): void {
    Container.register(target);
}
```

### Method Decorators

```typescript
// Handler type constraint
type HttpHandler = (req: Request) => Response;

class UserController {
    @GET("/users")
    getUsers(req: Request): Response {
        return Response.json(this.userService.findAll());
    }
}

// Decorator factory - returns a decorator that only accepts HttpHandler
function GET(path: string): MethodDecorator<HttpHandler> {
    return function(handler: HttpHandler): HttpHandler {
        Router.register("GET", path, handler);
        return handler;
    };
}
```

### Field Decorators

```typescript
class User {
    @Column("varchar", 255)
    name: string;

    @Column("int")
    age: number;
}

// Decorator factory
function Column(type: string, length: number = 0): FieldDecorator<Object> {
    return function(target: Object, fieldName: string): void {
        ORM.registerColumn(target, fieldName, type, length);
    };
}
```

### Parameter Decorators

```typescript
class UserController {
    createUser(@Body user: User, @Query("validate") validate: boolean): User {
        // ...
    }
}

// Decorator factory
function Body<T>(target: T, methodName: string, parameterIndex: number): void {
    Reflect.markBodyParam(target, methodName, parameterIndex);
}

function Query(name: string): ParameterDecorator<Object> {
    return function(target: Object, methodName: string, parameterIndex: number): void {
        Reflect.markQueryParam(target, methodName, parameterIndex, name);
    };
}
```

---

## Decorator Factories

A decorator factory is a function that returns a decorator. This allows decorators to accept arguments.

```typescript
// Without arguments: direct decorator
@Injectable
class Service {}

// With arguments: decorator factory
@Controller("/api")
class ApiController {}

// The factory pattern
function Controller(prefix: string): ClassDecorator<Object> {
    return function(target: Class<Object>): void {
        Router.registerController(target, prefix);
    };
}
```

**Syntax rules:**
- `@name` - Direct decorator (function is called with target)
- `@name()` - Factory with no args (factory is called, returns decorator)
- `@name(args)` - Factory with args (factory is called with args, returns decorator)

---

## Evaluation Order

Decorators are evaluated in a specific order:

### Multiple Decorators (Bottom-Up)

```typescript
@A
@B
@C
class Foo {}

// Evaluation order: C, B, A (bottom to top)
// C(target) -> B(target) -> A(target)
```

### Within a Class (Order of Declaration)

```typescript
@ClassDec
class Example {
    @FieldDec1
    field1: string;

    @FieldDec2
    field2: number;

    @MethodDec
    method(@ParamDec param: string): void {}
}

// Order:
// 1. FieldDec1 (field1)
// 2. FieldDec2 (field2)
// 3. ParamDec (method parameter)
// 4. MethodDec (method)
// 5. ClassDec (class)
```

---

## Framework Patterns

### HTTP Routing (Type-Safe Handlers)

Method decorators receive the function directly (already bound with `this`). The type parameter constrains which methods can be decorated.

```typescript
// === Handler Type Definitions ===

// HTTP handler signature: takes Request, returns Response
type HttpHandler = (req: Request) => Response;

// Async variant
type AsyncHttpHandler = (req: Request) => Task<Response>;

// === Decorator Definitions ===

// GET decorator: only works on functions matching HttpHandler
function GET(path: string): MethodDecorator<HttpHandler> {
    return function(handler: HttpHandler): HttpHandler {
        Router.register("GET", path, handler);
        return handler;
    };
}

// POST decorator: same constraint
function POST(path: string): MethodDecorator<HttpHandler> {
    return function(handler: HttpHandler): HttpHandler {
        Router.register("POST", path, handler);
        return handler;
    };
}

// Async versions
function AsyncGET(path: string): MethodDecorator<AsyncHttpHandler> {
    return function(handler: AsyncHttpHandler): AsyncHttpHandler {
        Router.registerAsync("GET", path, handler);
        return handler;
    };
}

// === Usage ===

@Controller("/users")
class UserController {
    private userService: UserService;

    // OK - matches HttpHandler signature (Request) => Response
    @GET("/")
    findAll(req: Request): Response {
        let users = this.userService.findAll();
        return Response.json(users);
    }

    // OK - matches HttpHandler
    @GET("/:id")
    findOne(req: Request): Response {
        let id = req.param("id");
        let user = this.userService.findById(id);
        return Response.json(user);
    }

    // OK - matches AsyncHttpHandler
    @AsyncGET("/slow")
    async slowQuery(req: Request): Task<Response> {
        let data = await this.userService.slowQuery();
        return Response.json(data);
    }

    // COMPILE ERROR! Wrong signature - missing Request parameter
    // @GET("/invalid")
    // invalid(): Response { ... }

    // COMPILE ERROR! Wrong return type - must return Response
    // @GET("/invalid2")
    // invalid2(req: Request): User { ... }
}
```

**Benefits of this approach:**
1. **Type-safe** - Compiler ensures decorated methods have correct signature
2. **Simple** - Decorator just receives and returns the function
3. **Wrappable** - Decorator can wrap the function for logging, timing, etc.
4. **No reflection** - No need for `target` object or method name strings

### Dependency Injection

```typescript
function Injectable<T>(target: Class<T>): void {
    Container.register(target);
}

function Inject(token: string): ParameterDecorator<Object> {
    return function(target: Object, methodName: string, index: number): void {
        Container.markInjection(target, methodName, index, token);
    };
}

@Injectable
class UserService {
    constructor(@Inject("Database") private db: Database) {}
}
```

### ORM / Database Mapping

```typescript
function Entity(tableName: string): ClassDecorator<Object> {
    return function(target: Class<Object>): void {
        ORM.registerEntity(target, tableName);
    };
}

function Column(type: string): FieldDecorator<Object> {
    return function(target: Object, fieldName: string): void {
        ORM.registerColumn(target, fieldName, type);
    };
}

function PrimaryKey(): FieldDecorator<Object> {
    return function(target: Object, fieldName: string): void {
        ORM.markPrimaryKey(target, fieldName);
    };
}

@Entity("users")
class User {
    @PrimaryKey()
    @Column("uuid")
    id: string;

    @Column("varchar")
    name: string;

    @Column("int")
    age: number;
}
```

### Validation

```typescript
function Min(value: number): FieldDecorator<Object> {
    return function(target: Object, fieldName: string): void {
        Validator.addRule(target, fieldName, "min", value);
    };
}

function Max(value: number): FieldDecorator<Object> {
    return function(target: Object, fieldName: string): void {
        Validator.addRule(target, fieldName, "max", value);
    };
}

function Email(): FieldDecorator<Object> {
    return function(target: Object, fieldName: string): void {
        Validator.addRule(target, fieldName, "email", null);
    };
}

class CreateUserDto {
    @Min(1)
    @Max(100)
    name: string;

    @Min(0)
    @Max(150)
    age: number;

    @Email()
    email: string;
}
```

---

## Static Typing Constraints

### No `any` Type

Raya does not have `any`. Decorator signatures must use concrete types:

```typescript
// TypeScript (uses any)
function Log(target: any, key: string, descriptor: any): any { ... }

// Raya (explicit types) - decorator receives and returns the function
function Log<F>(method: F): F { ... }
```

### Generic Method Decorators

Use generics to create decorators that work with any function type while preserving the signature:

```typescript
// Generic logging decorator - works with any function
function Logged<F>(method: F): F {
    // Wrap and return - exact implementation depends on F
    return method;  // or wrapped version
}

// Constrained decorator - only for specific signatures
type Computation = (x: number) => number;

function Memoized(method: Computation): Computation {
    let cache = new Map<number, number>();
    return function(x: number): number {
        if (cache.has(x)) {
            return cache.get(x)!;
        }
        let result = method(x);
        cache.set(x, result);
        return result;
    };
}

class Math {
    @Memoized
    fibonacci(n: number): number {
        if (n <= 1) return n;
        return this.fibonacci(n - 1) + this.fibonacci(n - 2);
    }
}
```

### Return Type Constraints

- **Class decorators**: Return `Class<T>` or `void`
- **Method decorators**: Return the same function type `F` (can wrap)
- **Field decorators**: Return `void`
- **Parameter decorators**: Return `void`

Method decorators **can** wrap the function - they receive `F` and return `F`.

---

## Reflect API

The built-in `Reflect` API stores metadata on targets (similar to `reflect-metadata` in TypeScript).

**Note:** Method decorators receive only the function, so they typically wrap behavior rather than store metadata. Class and field decorators receive targets and can use `Reflect`:

```typescript
// Built-in Reflect API for metadata storage
class Reflect {
    static define<T>(key: string, value: T, target: Object): void;
    static define<T>(key: string, value: T, target: Object, propertyKey: string): void;

    static get<T>(key: string, target: Object): T | null;
    static get<T>(key: string, target: Object, propertyKey: string): T | null;

    static has(key: string, target: Object): boolean;
    static has(key: string, target: Object, propertyKey: string): boolean;

    static keys(target: Object): string[];
    static keys(target: Object, propertyKey: string): string[];
}

// Method decorator - wraps to log deprecation warning
function Deprecated<F>(message: string): (method: F) => F {
    return function(method: F): F {
        // For method decorators, wrap to add runtime behavior
        // The exact wrapping depends on F's signature
        logger.warn("Method is deprecated: " + message);
        return method;
    };
}

// Class decorator - can use Reflect to store metadata
function Injectable<T>(target: Class<T>): Class<T> {
    Reflect.define("injectable", true, target);
    Container.register(target);
    return target;
}

// Field decorator - receives target and field name
function Column(type: string): FieldDecorator<Object> {
    return function(target: Object, fieldName: string): void {
        Reflect.define("column:" + fieldName, type, target);
    };
}
```

---

## Implementation Details

### Compiler Changes

1. **Type Checking**
   - Validate decorator function signatures match expected types
   - Check decorator factory return types
   - Verify argument types for decorator factories

2. **Code Generation**
   - Emit decorator calls after class definition
   - Pass correct arguments based on decorator position
   - Handle decorator factories (call factory, then call returned decorator)

### Runtime Support

1. **Class<T> Type**
   - Runtime representation of class constructors
   - Provides `name`, `prototype`, constructor access

2. **Reflect API**
   - Built-in metadata storage per class/method/field
   - WeakMap-based storage for GC friendliness

3. **Execution Order**
   - Field decorators first (in declaration order)
   - Parameter decorators (in parameter order)
   - Method decorators (in declaration order)
   - Class decorators last (bottom-up for multiple)

### Bytecode

Decorator calls are emitted as regular function calls:

```typescript
@Injectable
class Service {}

// Compiles to (pseudo-bytecode):
// 1. Define class Service
// 2. LOAD_GLOBAL Injectable
// 3. LOAD_CLASS Service
// 4. CALL 1        // Injectable(Service)
```

For decorator factories:

```typescript
@Controller("/api")
class Api {}

// Compiles to:
// 1. Define class Api
// 2. LOAD_GLOBAL Controller
// 3. PUSH_STRING "/api"
// 4. CALL 1        // Controller("/api") -> returns decorator
// 5. LOAD_CLASS Api
// 6. CALL 1        // decorator(Api)
```

For method decorators (function wrapping):

```typescript
class Api {
    @GET("/users")
    getUsers(req: Request): Response { ... }
}

// Compiles to:
// 1. Define method getUsers
// 2. LOAD_GLOBAL GET
// 3. PUSH_STRING "/users"
// 4. CALL 1              // GET("/users") -> returns decorator
// 5. LOAD_METHOD getUsers
// 6. CALL 1              // decorator(getUsers) -> returns wrapped function
// 7. STORE_METHOD getUsers  // Replace with wrapped version
```

---

## Comparison with TypeScript

| Feature | TypeScript | Raya |
|---------|------------|------|
| Syntax | `@decorator` | `@decorator` |
| Runtime | Yes | Yes |
| Type-checked | Partially | **Fully** |
| `any` type | Used extensively | Not available |
| Method wrapping | Via PropertyDescriptor | **Direct function wrapping** |
| Signature constraint | None (any method) | **Type parameter enforces signature** |
| Reflect metadata | reflect-metadata lib | Built-in Reflect API |
| Experimental | Yes (flag required) | Core feature |

---

## Limitations

1. **Function Signature Must Match**
   - Method decorators enforce a specific function type
   - Cannot decorate methods with incompatible signatures

2. **No Property Descriptor**
   - Raya uses direct function passing, not descriptors
   - Simpler but less flexible than TypeScript

3. **Static Analysis**
   - Decorator effects are runtime-only
   - Compiler cannot see metadata added by decorators

---

## Decorator Composition

Compose multiple decorators using function composition:

```typescript
// Compose decorators for reuse
function ApiEndpoint(path: string): MethodDecorator<HttpHandler> {
    return function(handler: HttpHandler): HttpHandler {
        // Apply multiple behaviors
        let logged = Logged(handler);
        let timed = Timed(logged);
        Router.register("GET", path, timed);
        return timed;
    };
}

class UserController {
    @ApiEndpoint("/users")
    getUsers(req: Request): Response {
        return Response.json(this.users);
    }
}
```

---

## Examples

### Complete HTTP Framework Example

```typescript
// === Type Definitions ===

type HttpHandler = (req: Request) => Response;
type AsyncHttpHandler = (req: Request) => Task<Response>;

// === Decorator Definitions ===

function Controller(prefix: string): ClassDecorator<Object> {
    return function(target: Class<Object>): Class<Object> {
        Reflect.define("controller:prefix", prefix, target);
        Router.controllers.push(target);
        return target;
    };
}

// GET decorator - only accepts HttpHandler signature
function GET(path: string): MethodDecorator<HttpHandler> {
    return function(handler: HttpHandler): HttpHandler {
        Router.register("GET", path, handler);
        return handler;
    };
}

// POST decorator - only accepts HttpHandler signature
function POST(path: string): MethodDecorator<HttpHandler> {
    return function(handler: HttpHandler): HttpHandler {
        Router.register("POST", path, handler);
        return handler;
    };
}

// Async variants for Task-based handlers
function AsyncGET(path: string): MethodDecorator<AsyncHttpHandler> {
    return function(handler: AsyncHttpHandler): AsyncHttpHandler {
        Router.registerAsync("GET", path, handler);
        return handler;
    };
}

// Logging decorator - wraps the handler
function Logged(handler: HttpHandler): HttpHandler {
    return function(req: Request): Response {
        logger.info("Request: " + req.method + " " + req.path);
        let response = handler(req);
        logger.info("Response: " + response.status);
        return response;
    };
}

// === Usage ===

@Controller("/users")
class UserController {
    private userService: UserService;

    constructor(userService: UserService) {
        this.userService = userService;
    }

    @GET("/")
    @Logged
    findAll(req: Request): Response {
        let users = this.userService.findAll();
        return Response.json(users);
    }

    @GET("/:id")
    findOne(req: Request): Response {
        let id = req.param("id");
        let user = this.userService.findById(id);
        return Response.json(user);
    }

    @POST("/")
    create(req: Request): Response {
        let user = req.json<User>();
        let created = this.userService.create(user);
        return Response.json(created, 201);
    }

    @AsyncGET("/slow")
    async slowQuery(req: Request): Task<Response> {
        let data = await this.userService.expensiveQuery();
        return Response.json(data);
    }
}
```

---

## References

- [TypeScript Decorators](https://www.typescriptlang.org/docs/handbook/decorators.html)
- [TC39 Decorators Proposal](https://github.com/tc39/proposal-decorators)
- [NestJS Decorators](https://docs.nestjs.com/custom-decorators)
- [Python Decorators](https://peps.python.org/pep-0318/)

---

**Document History:**
- 2026-01-31: Initial design document
