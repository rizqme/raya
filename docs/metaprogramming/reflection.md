---
title: "Reflection API"
---

# Reflection API in Raya

> **Status:** Implemented (Phases 1-17, 149+ handlers)
> **Milestone:** M3.8
> **Related:** [Language Spec](../language/lang.md), [Security](./reflect-security.md), [Decorators](./decorators.md)

---

## Overview

Raya provides a comprehensive `Reflect` API for runtime introspection and manipulation. Unlike TypeScript's limited reflection, Raya's `Reflect` is a first-class feature that enables:

- **Metadata storage** on classes, methods, and fields
- **Type introspection** - query class structure, methods, fields
- **Dynamic invocation** - call methods, get/set fields at runtime
- **Object creation** - instantiate classes, create proxies
- **AOP (Aspect-Oriented Programming)** - wrap/replace methods dynamically

> **Note:** Reflection metadata is always included in compiled modules to support runtime introspection.

---

## Design Goals

1. **Type-Safe Where Possible** - Use generics to preserve type information
2. **Complete Introspection** - Access all class metadata at runtime
3. **Dynamic Capabilities** - Create objects, invoke methods, modify behavior
4. **AOP Support** - Enable cross-cutting concerns (logging, caching, security)
5. **Framework Enablement** - Support DI containers, ORMs, serialization
6. **Always Available** - Reflection metadata always included for full runtime support
7. **Zero Overhead Principle** - Static code pays no cost when dynamic features are unused
8. **Near-Native Dynamic Code** - Dynamic bytecode executes on same interpreter as static code

---

## Core Types

### Class<T>

Runtime representation of a class constructor:

```typescript
interface Class<T> {
    // Identity
    readonly name: string;
    readonly prototype: T;

    // Hierarchy
    readonly superclass: Class<Object> | null;
    readonly interfaces: Class<Object>[];

    // Members
    readonly fields: FieldInfo[];
    readonly methods: MethodInfo[];
    readonly constructors: ConstructorInfo[];

    // Annotations/Decorators
    readonly decorators: DecoratorInfo[];

    // Instantiation
    new(...args: unknown[]): T;
}
```

### FieldInfo

Information about a class field:

```typescript
interface FieldInfo {
    readonly name: string;
    readonly type: TypeInfo;
    readonly declaringClass: Class<Object>;
    readonly modifiers: Modifiers;
    readonly decorators: DecoratorInfo[];
    readonly isStatic: boolean;
    readonly isReadonly: boolean;
}
```

### MethodInfo

Information about a class method:

```typescript
interface MethodInfo {
    readonly name: string;
    readonly returnType: TypeInfo;
    readonly parameters: ParameterInfo[];
    readonly declaringClass: Class<Object>;
    readonly modifiers: Modifiers;
    readonly decorators: DecoratorInfo[];
    readonly isStatic: boolean;
    readonly isAsync: boolean;
}
```

### ParameterInfo

Information about a method parameter:

```typescript
interface ParameterInfo {
    readonly name: string;
    readonly type: TypeInfo;
    readonly index: number;
    readonly isOptional: boolean;
    readonly defaultValue: unknown | null;
    readonly decorators: DecoratorInfo[];
}
```

### TypeInfo

Runtime type information:

```typescript
interface TypeInfo {
    readonly kind: "primitive" | "class" | "interface" | "union" | "function" | "array" | "generic";
    readonly name: string;

    // For class/interface types
    readonly classRef: Class<Object> | null;

    // For union types
    readonly unionMembers: TypeInfo[] | null;

    // For array types
    readonly elementType: TypeInfo | null;

    // For generic types
    readonly typeArguments: TypeInfo[] | null;

    // For function types
    readonly parameterTypes: TypeInfo[] | null;
    readonly returnType: TypeInfo | null;
}
```

### DecoratorInfo

Information about applied decorators:

```typescript
interface DecoratorInfo {
    readonly name: string;
    readonly args: unknown[];
    readonly target: "class" | "method" | "field" | "parameter";
}
```

### Modifiers

Access modifiers and flags:

```typescript
interface Modifiers {
    readonly isPublic: boolean;
    readonly isPrivate: boolean;
    readonly isProtected: boolean;
    readonly isStatic: boolean;
    readonly isReadonly: boolean;
    readonly isAbstract: boolean;
}
```

---

## Reflect API

### Metadata Storage

Store and retrieve arbitrary metadata on targets:

```typescript
class Reflect {
    // Define metadata
    static defineMetadata<T>(key: string, value: T, target: Object): void;
    static defineMetadata<T>(key: string, value: T, target: Object, propertyKey: string): void;

    // Get metadata
    static getMetadata<T>(key: string, target: Object): T | null;
    static getMetadata<T>(key: string, target: Object, propertyKey: string): T | null;

    // Check metadata existence
    static hasMetadata(key: string, target: Object): boolean;
    static hasMetadata(key: string, target: Object, propertyKey: string): boolean;

    // Get all metadata keys
    static getMetadataKeys(target: Object): string[];
    static getMetadataKeys(target: Object, propertyKey: string): string[];

    // Delete metadata
    static deleteMetadata(key: string, target: Object): boolean;
    static deleteMetadata(key: string, target: Object, propertyKey: string): boolean;
}
```

**Example:**
```typescript
@Controller("/api")
class UserController {
    @GET("/users")
    getUsers(req: Request): Response { ... }
}

// Store metadata
Reflect.defineMetadata("route:path", "/api", UserController);
Reflect.defineMetadata("route:method", "GET", UserController.prototype, "getUsers");

// Retrieve metadata
let path = Reflect.getMetadata<string>("route:path", UserController);  // "/api"
let method = Reflect.getMetadata<string>("route:method", UserController.prototype, "getUsers");  // "GET"
```

---

### Class Introspection

Query class structure and type information:

```typescript
class Reflect {
    // Get class of an object
    static getClass<T>(obj: T): Class<T>;

    // Get all registered classes
    static getAllClasses(): Class<Object>[];

    // Get classes by decorator
    static getClassesWithDecorator(decoratorName: string): Class<Object>[];

    // Get class by name
    static getClassByName(name: string): Class<Object> | null;

    // Check class relationships
    static isSubclassOf(subclass: Class<Object>, superclass: Class<Object>): boolean;
    static isInstanceOf<T>(obj: Object, cls: Class<T>): obj is T;

    // Get type info
    static getTypeInfo(target: Object): TypeInfo;
    static getTypeInfo(target: Object, propertyKey: string): TypeInfo;
}
```

**Example:**
```typescript
class Animal { name: string; }
class Dog extends Animal { breed: string; }

let dog = new Dog();

// Get class
let dogClass = Reflect.getClass(dog);  // Class<Dog>
logger.info(dogClass.name);  // "Dog"

// Check relationships
Reflect.isSubclassOf(Dog, Animal);  // true
Reflect.isInstanceOf(dog, Animal);  // true

// Get all classes with @Entity decorator
let entities = Reflect.getClassesWithDecorator("Entity");
for (let entity of entities) {
    logger.info(entity.name);
}
```

---

### Field Access

Get and set field values dynamically:

```typescript
class Reflect {
    // Get field value
    static get<T>(target: Object, propertyKey: string): T;

    // Set field value
    static set<T>(target: Object, propertyKey: string, value: T): void;

    // Check if field exists
    static has(target: Object, propertyKey: string): boolean;

    // Get all field names
    static getFieldNames(target: Object): string[];

    // Get field info
    static getFieldInfo(target: Object, propertyKey: string): FieldInfo | null;

    // Get all fields
    static getFields(target: Object): FieldInfo[];
    static getFields(cls: Class<Object>): FieldInfo[];
}
```

**Example:**
```typescript
class User {
    name: string = "Alice";
    private age: number = 30;
}

let user = new User();

// Get field value
let name = Reflect.get<string>(user, "name");  // "Alice"

// Set field value
Reflect.set(user, "name", "Bob");

// Get all fields
let fields = Reflect.getFields(user);
for (let field of fields) {
    logger.info(field.name + ": " + field.type.name);
}
// Output: "name: string", "age: number"
```

---

### Method Invocation

Invoke methods dynamically:

```typescript
class Reflect {
    // Invoke method
    static invoke<R>(target: Object, methodName: string, ...args: unknown[]): R;

    // Invoke async method
    static invokeAsync<R>(target: Object, methodName: string, ...args: unknown[]): Task<R>;

    // Get method reference
    static getMethod<F>(target: Object, methodName: string): F | null;

    // Get method info
    static getMethodInfo(target: Object, methodName: string): MethodInfo | null;

    // Get all methods
    static getMethods(target: Object): MethodInfo[];
    static getMethods(cls: Class<Object>): MethodInfo[];

    // Check if method exists
    static hasMethod(target: Object, methodName: string): boolean;
}
```

**Example:**
```typescript
class Calculator {
    add(a: number, b: number): number {
        return a + b;
    }

    async fetchData(url: string): Task<string> {
        // ...
    }
}

let calc = new Calculator();

// Invoke synchronously
let result = Reflect.invoke<number>(calc, "add", 2, 3);  // 5

// Invoke asynchronously
let data = await Reflect.invokeAsync<string>(calc, "fetchData", "https://api.example.com");

// Get method info
let methodInfo = Reflect.getMethodInfo(calc, "add");
logger.info(methodInfo.parameters.length);  // 2
logger.info(methodInfo.returnType.name);    // "number"
```

---

### Object Creation

Create instances dynamically:

```typescript
class Reflect {
    // Create instance with constructor args
    static construct<T>(cls: Class<T>, ...args: unknown[]): T;

    // Create instance with named parameters (for DI)
    static constructWith<T>(cls: Class<T>, params: Map<string, unknown>): T;

    // Create empty instance (fields uninitialized)
    static allocate<T>(cls: Class<T>): T;

    // Clone an object (shallow)
    static clone<T>(obj: T): T;

    // Clone an object (deep)
    static deepClone<T>(obj: T): T;
}
```

**Example:**
```typescript
class User {
    constructor(public name: string, public age: number) {}
}

// Create with args
let user1 = Reflect.construct(User, "Alice", 30);

// Create with named params (useful for DI)
let params = new Map<string, unknown>();
params.set("name", "Bob");
params.set("age", 25);
let user2 = Reflect.constructWith(User, params);

// Clone
let user3 = Reflect.clone(user1);
```

---

### Dynamic Subclass Creation

Create subclasses at runtime:

```typescript
class Reflect {
    // Create a subclass
    static createSubclass<T, S extends T>(
        superclass: Class<T>,
        name: string,
        definition: SubclassDefinition<S>
    ): Class<S>;

    // Extend with additional fields
    static extendWith<T>(
        cls: Class<T>,
        fields: Map<string, TypeInfo>
    ): Class<T>;
}

interface SubclassDefinition<T> {
    fields?: Map<string, FieldDefinition>;
    methods?: Map<string, Function>;
    constructor?: (...args: unknown[]) => void;
}

interface FieldDefinition {
    type: TypeInfo;
    defaultValue?: unknown;
    decorators?: DecoratorInfo[];
}
```

**Example:**
```typescript
class Animal {
    name: string;
    speak(): string { return "..."; }
}

// Create Dog subclass dynamically
let DogClass = Reflect.createSubclass(Animal, "Dog", {
    fields: new Map([
        ["breed", { type: Reflect.typeOf("string"), defaultValue: "Unknown" }]
    ]),
    methods: new Map([
        ["speak", function(): string { return "Woof!"; }],
        ["fetch", function(): void { logger.info("Fetching..."); }]
    ])
});

let dog = Reflect.construct(DogClass);
dog.name = "Buddy";
dog.breed = "Labrador";
logger.info(dog.speak());  // "Woof!"
```

---

### Proxy and AOP (Aspect-Oriented Programming)

Create proxies with intercepted behavior:

```typescript
class Reflect {
    // Create a proxy with method interception
    static createProxy<T>(
        target: T,
        handler: ProxyHandler<T>
    ): T;

    // Wrap specific methods
    static wrapMethod<T>(
        target: T,
        methodName: string,
        wrapper: MethodWrapper
    ): T;

    // Wrap all methods matching a pattern
    static wrapMethods<T>(
        target: T,
        pattern: string | RegExp,
        wrapper: MethodWrapper
    ): T;

    // Create AOP-enhanced object
    static createAspect<T>(
        target: T,
        aspects: Aspect[]
    ): T;
}

interface ProxyHandler<T> {
    get?(target: T, property: string): unknown;
    set?(target: T, property: string, value: unknown): boolean;
    invoke?(target: T, method: string, args: unknown[]): unknown;
    construct?(target: Class<T>, args: unknown[]): T;
}

type MethodWrapper = (
    original: Function,
    args: unknown[],
    target: Object,
    methodName: string
) => unknown;

interface Aspect {
    pointcut: string | RegExp;  // Method pattern to match
    before?: (target: Object, method: string, args: unknown[]) => void;
    after?: (target: Object, method: string, args: unknown[], result: unknown) => void;
    around?: MethodWrapper;
    onError?: (target: Object, method: string, error: Error) => void;
}
```

**Example - Simple Proxy:**
```typescript
class UserService {
    getUser(id: number): User { ... }
    saveUser(user: User): void { ... }
}

let service = new UserService();

// Create logging proxy
let proxy = Reflect.createProxy(service, {
    invoke(target, method, args) {
        logger.info("Calling " + method + " with args: " + args);
        let result = Reflect.invoke(target, method, ...args);
        logger.info("Result: " + result);
        return result;
    }
});

proxy.getUser(123);  // Logs: "Calling getUser with args: [123]"
```

**Example - Method Wrapping:**
```typescript
class Calculator {
    add(a: number, b: number): number { return a + b; }
    multiply(a: number, b: number): number { return a * b; }
}

let calc = new Calculator();

// Wrap 'add' method with timing
let wrapped = Reflect.wrapMethod(calc, "add", (original, args, target, name) => {
    let start = Date.now();
    let result = original(...args);
    let elapsed = Date.now() - start;
    logger.info(name + " took " + elapsed + "ms");
    return result;
});

wrapped.add(2, 3);  // Logs timing, returns 5
```

**Example - AOP Aspects:**
```typescript
class OrderService {
    createOrder(order: Order): Order { ... }
    cancelOrder(id: number): void { ... }
    getOrder(id: number): Order { ... }
}

let service = new OrderService();

// Apply multiple aspects
let enhanced = Reflect.createAspect(service, [
    // Logging aspect for all methods
    {
        pointcut: /.*/,
        before(target, method, args) {
            logger.info(">>> " + method);
        },
        after(target, method, args, result) {
            logger.info("<<< " + method + " returned " + result);
        }
    },
    // Transaction aspect for mutating methods
    {
        pointcut: /^(create|cancel|update)/,
        around(original, args, target, method) {
            Transaction.begin();
            try {
                let result = original(...args);
                Transaction.commit();
                return result;
            } catch (e) {
                Transaction.rollback();
                throw e;
            }
        }
    },
    // Error logging aspect
    {
        pointcut: /.*/,
        onError(target, method, error) {
            logger.error("Error in " + method + ": " + error.message);
        }
    }
]);

enhanced.createOrder(order);  // Wrapped with logging + transaction
```

---

### Generic Type Inspection

Inspect generic type origins after monomorphization:

```typescript
class Reflect {
    // Get the original generic type name (e.g., "Box" for Box_number)
    static getGenericOrigin<T>(cls: Class<T>): string | null;

    // Get type parameters from the generic definition
    static getTypeParameters<T>(cls: Class<T>): GenericParameterInfo[];

    // Get actual type arguments (e.g., [number] for Box<number>)
    static getTypeArguments<T>(cls: Class<T>): TypeInfo[];

    // Check if this is a monomorphized generic instance
    static isGenericInstance<T>(cls: Class<T>): boolean;

    // Find all specializations of a generic type
    static findSpecializations(genericName: string): Class<Object>[];

    // Create a new specialization at runtime
    static specialize<T>(genericName: string, typeArgs: TypeInfo[]): Class<T>;
}

interface GenericParameterInfo {
    readonly name: string;           // e.g., "T"
    readonly index: number;          // Position in type parameter list
    readonly constraint: TypeInfo | null;  // e.g., "extends Comparable"
}

interface GenericOrigin {
    readonly name: string;           // e.g., "Box"
    readonly typeParameters: string[];  // e.g., ["T"]
    readonly typeArguments: TypeInfo[];  // e.g., [TypeInfo(number)]
}
```

**Example:**
```typescript
class Box<T> {
    value: T;
    constructor(value: T) { this.value = value; }
}

let box = new Box<number>(42);
let boxClass = Reflect.getClass(box);

// Inspect generic origin
Reflect.getGenericOrigin(boxClass);      // "Box"
Reflect.isGenericInstance(boxClass);     // true
Reflect.getTypeArguments(boxClass);      // [TypeInfo { kind: "primitive", name: "number" }]
Reflect.getTypeParameters(boxClass);     // [{ name: "T", index: 0, constraint: null }]

// Find all Box specializations
let allBoxes = Reflect.findSpecializations("Box");
// [Box_number, Box_string, Box_User, ...]

// Create new specialization at runtime
let Box_boolean = Reflect.specialize<Box<boolean>>("Box", [Reflect.typeOf("boolean")]);
```

**Background:**
Raya uses monomorphization - generics are specialized at compile time. `Box<number>` becomes `Box_number` in the compiled bytecode. The generic origin metadata preserves the connection to the original generic type, enabling runtime inspection and new specializations.

---

### Runtime Type Creation

Create new types dynamically:

```typescript
class Reflect {
    // Create a new class at runtime
    static createClass<T>(name: string, definition: ClassDefinition): Class<T>;

    // Create a function at runtime
    static createFunction<R>(
        name: string,
        params: ParameterDefinition[],
        body: FunctionBody
    ): (...args: unknown[]) => R;

    // Create an async function at runtime
    static createAsyncFunction<R>(
        name: string,
        params: ParameterDefinition[],
        body: FunctionBody
    ): (...args: unknown[]) => Task<R>;
}

interface ClassDefinition {
    fields?: FieldDefinition[];
    methods?: MethodDefinition[];
    constructor?: ConstructorDefinition;
    parent?: Class<Object>;
    interfaces?: string[];
}

interface MethodDefinition {
    name: string;
    params: ParameterDefinition[];
    returnType: TypeInfo;
    body: FunctionBody;
    isStatic?: boolean;
    isAsync?: boolean;
}

interface ParameterDefinition {
    name: string;
    type: TypeInfo;
    isOptional?: boolean;
    defaultValue?: unknown;
}

// Function body can be bytecode, AST, or native callback
type FunctionBody =
    | { kind: "bytecode"; instructions: number[] }
    | { kind: "ast"; statements: Statement[] }
    | { kind: "native"; callback: NativeCallback };
```

**Example:**
```typescript
// Create a simple class at runtime
let PointClass = Reflect.createClass<{ x: number; y: number }>("Point", {
    fields: [
        { name: "x", type: Reflect.typeOf("number"), initialValue: 0 },
        { name: "y", type: Reflect.typeOf("number"), initialValue: 0 },
    ],
    methods: [
        {
            name: "toString",
            params: [],
            returnType: Reflect.typeOf("string"),
            body: { kind: "native", callback: (self) => `(${self.x}, ${self.y})` },
        },
    ],
});

let point = Reflect.construct(PointClass);
point.x = 10;
point.y = 20;
logger.info(point.toString());  // "(10, 20)"

// Create a function at runtime
let addFunc = Reflect.createFunction<number>("add", [
    { name: "a", type: Reflect.typeOf("number") },
    { name: "b", type: Reflect.typeOf("number") },
], {
    kind: "bytecode",
    instructions: [0x10, 0x00, 0x10, 0x01, 0x60, 0xB1]  // LOAD_LOCAL 0, LOAD_LOCAL 1, IADD, RETURN
});

logger.info(addFunc(2, 3));  // 5
```

---

### Dynamic Bytecode Generation

Programmatic bytecode construction for advanced use cases:

```typescript
class BytecodeBuilder {
    constructor(name: string, params: ParameterDefinition[], returnType: TypeInfo);

    // Instruction emission
    emit(opcode: number, ...operands: number[]): void;
    emitPush(value: unknown): void;
    emitPop(): void;

    // Labels and control flow
    defineLabel(): Label;
    markLabel(label: Label): void;
    emitJump(label: Label): void;
    emitJumpIf(label: Label): void;
    emitJumpIfNot(label: Label): void;

    // Locals
    declareLocal(type: TypeInfo): number;
    emitLoadLocal(index: number): void;
    emitStoreLocal(index: number): void;

    // Object operations
    emitNew(classId: number): void;
    emitLoadField(fieldOffset: number): void;
    emitStoreField(fieldOffset: number): void;

    // Calls
    emitCall(functionId: number): void;
    emitVirtualCall(methodIndex: number): void;
    emitNativeCall(nativeId: number): void;

    // Arithmetic (typed opcodes)
    emitIAdd(): void;  // Integer add
    emitFAdd(): void;  // Float add

    // Build final function
    validate(): ValidationResult;
    build(): CompiledFunction;
}
```

**Example - Building a function dynamically:**
```typescript
// Build: function multiply(a: number, b: number): number { return a * b; }
let builder = new BytecodeBuilder("multiply", [
    { name: "a", type: Reflect.typeOf("number") },
    { name: "b", type: Reflect.typeOf("number") },
], Reflect.typeOf("number"));

builder.emitLoadLocal(0);   // Load 'a'
builder.emitLoadLocal(1);   // Load 'b'
builder.emit(0x68);         // IMUL opcode
builder.emitReturn();

let result = builder.validate();
if (!result.isValid) {
    throw new Error("Invalid bytecode: " + result.errors.join(", "));
}

let multiply = builder.build();
logger.info(multiply(6, 7));  // 42
```

---

### Dynamic VM Bootstrap

Create and execute code from an empty VM:

```typescript
class Reflect {
    // Initialize minimal runtime environment
    static bootstrap(): BootstrapContext;

    // Create a new module at runtime
    static createModule(name: string): DynamicModule;

    // Execute dynamically created code
    static execute<R>(func: CompiledFunction | Closure, ...args: unknown[]): R;
    static spawn<R>(func: CompiledFunction | Closure, ...args: unknown[]): Task<R>;
    static eval(bytecode: number[]): unknown;
}

interface DynamicModule {
    readonly name: string;
    readonly isSealed: boolean;

    addFunction(func: CompiledFunction): number;
    addClass(cls: DynamicClass): number;
    addGlobal(name: string, value: unknown): void;
    seal(): void;
}

interface BootstrapContext {
    readonly objectClass: Class<Object>;
    readonly arrayClass: Class<Array<unknown>>;
    readonly stringClass: Class<string>;
    readonly taskClass: Class<Task<unknown>>;

    print(message: string): void;
    createArray<T>(elements: T[]): T[];
    createString(value: string): string;
}
```

**Example - Creating a Program from Scratch:**
```typescript
// Start with nothing - create everything dynamically
const ctx = Reflect.bootstrap();
const module = Reflect.createModule("main");

// Create a simple "add" function using BytecodeBuilder
const builder = new BytecodeBuilder("add", [
    { name: "a", type: Reflect.typeOf("number") },
    { name: "b", type: Reflect.typeOf("number") },
], Reflect.typeOf("number"));

builder.emitLoadLocal(0);   // Load 'a'
builder.emitLoadLocal(1);   // Load 'b'
builder.emitIAdd();         // Integer add
builder.emitReturn();

const addFunc = builder.build();
module.addFunction(addFunc);
module.seal();

// Execute the dynamically created function
const result = Reflect.execute(addFunc, 2, 3);  // Returns 5

// Can also create classes dynamically
const PointClass = Reflect.createClass("Point", {
    fields: [
        { name: "x", type: Reflect.typeOf("number") },
        { name: "y", type: Reflect.typeOf("number") },
    ],
});
module.addClass(PointClass);

const point = Reflect.construct(PointClass, 10, 20);
```

This enables:
- REPL/interpreter implementations
- Hot code reloading
- Plugin systems with runtime-defined types
- Meta-programming and code generation
- Educational tools and sandboxes

---

### Performance Principles

Dynamic code generation is designed with **zero overhead** for static code:

**Key Design Decisions:**

1. **Unified Function Table**: Dynamic functions are stored in the same `Module.functions` array as static functions. No separate lookup path.

2. **Identical Bytecode Format**: `BytecodeBuilder` emits the exact same bytecode format as the compiler. The interpreter cannot distinguish dynamic from static code.

3. **No Runtime Discrimination**: There are no `if (is_dynamic)` checks in the interpreter hot path. All code is treated identically.

4. **Validation at Build Time**: Bytecode is fully validated when `build()` is called. Execution assumes valid bytecode - no per-instruction validation.

5. **Lazy Initialization**: Dynamic infrastructure (module registry, bootstrap context) is only allocated when first used. Programs that don't use dynamic features pay nothing.

**Performance Targets:**
```
Static code with reflect infrastructure:     0% overhead
Dynamic function call vs static:           < 5% overhead
Dynamic class instantiation:              < 10% overhead
Dynamic method dispatch:                  < 10% overhead
```

---

### Type Utilities

Runtime type checking and conversion:

```typescript
class Reflect {
    // Get TypeInfo for a type expression
    static typeOf(typeName: string): TypeInfo;
    static typeOf<T>(): TypeInfo;  // From generic

    // Check type compatibility
    static isAssignableTo(source: TypeInfo, target: TypeInfo): boolean;

    // Type checking
    static isString(value: unknown): value is string;
    static isNumber(value: unknown): value is number;
    static isBoolean(value: unknown): value is boolean;
    static isNull(value: unknown): value is null;
    static isArray(value: unknown): value is unknown[];
    static isFunction(value: unknown): value is Function;
    static isObject(value: unknown): value is Object;

    // Safe casting
    static cast<T>(value: unknown, cls: Class<T>): T | null;
    static castOrThrow<T>(value: unknown, cls: Class<T>): T;
}
```

**Example:**
```typescript
function processValue(value: unknown): void {
    if (Reflect.isString(value)) {
        logger.info(value.toUpperCase());  // Type narrowed to string
    } else if (Reflect.isNumber(value)) {
        logger.info(value.toFixed(2));  // Type narrowed to number
    }
}

// Safe casting
let user = Reflect.cast(obj, User);
if (user != null) {
    logger.info(user.name);
}

// Or throw on failure
let user2 = Reflect.castOrThrow(obj, User);  // Throws if not User
```

---

### Constructor/Parameter Inspection

Inspect constructor parameters (essential for DI):

```typescript
class Reflect {
    // Get constructor info
    static getConstructorInfo(cls: Class<Object>): ConstructorInfo;

    // Get constructor parameter types
    static getConstructorParameterTypes(cls: Class<Object>): TypeInfo[];

    // Get constructor parameter names
    static getConstructorParameterNames(cls: Class<Object>): string[];
}

interface ConstructorInfo {
    readonly parameters: ParameterInfo[];
    readonly decorators: DecoratorInfo[];
}
```

**Example - Dependency Injection:**
```typescript
@Injectable
class UserService {
    constructor(
        private db: Database,
        private logger: Logger,
        @Inject("config") private config: Config
    ) {}
}

// DI Container implementation
class Container {
    resolve<T>(cls: Class<T>): T {
        let ctorInfo = Reflect.getConstructorInfo(cls);
        let args: unknown[] = [];

        for (let param of ctorInfo.parameters) {
            // Check for @Inject decorator
            let injectDecorator = param.decorators.find(d => d.name === "Inject");
            if (injectDecorator != null) {
                let token = injectDecorator.args[0] as string;
                args.push(this.resolveByToken(token));
            } else if (param.type.classRef != null) {
                // Resolve by type
                args.push(this.resolve(param.type.classRef));
            }
        }

        return Reflect.construct(cls, ...args);
    }
}
```

---

### Interface and Type Query

Query types and interfaces:

```typescript
class Reflect {
    // Check if class implements interface
    static implements(cls: Class<Object>, interfaceName: string): boolean;

    // Get all interfaces implemented by class
    static getInterfaces(cls: Class<Object>): string[];

    // Get classes implementing an interface
    static getImplementors(interfaceName: string): Class<Object>[];

    // Check structural compatibility
    static isStructurallyCompatible(a: Class<Object>, b: Class<Object>): boolean;
}
```

**Example:**
```typescript
interface Serializable {
    serialize(): string;
}

@Implements("Serializable")
class User {
    serialize(): string { return JSON.stringify(this); }
}

// Query interfaces
Reflect.implements(User, "Serializable");  // true
Reflect.getInterfaces(User);  // ["Serializable"]

// Find all serializable classes
let serializables = Reflect.getImplementors("Serializable");
```

---

## Framework Integration Examples

### ORM Framework

```typescript
@Entity("users")
class User {
    @PrimaryKey()
    @Column("uuid")
    id: string;

    @Column("varchar", 255)
    name: string;

    @Column("int")
    age: number;

    @OneToMany(() => Post)
    posts: Post[];
}

// ORM implementation using Reflect
class Repository<T> {
    private entityClass: Class<T>;
    private tableName: string;
    private columns: Map<string, ColumnInfo>;

    constructor(entityClass: Class<T>) {
        this.entityClass = entityClass;

        // Read @Entity metadata
        this.tableName = Reflect.getMetadata<string>("entity:table", entityClass) ?? entityClass.name;

        // Read column metadata
        this.columns = new Map();
        for (let field of Reflect.getFields(entityClass)) {
            let columnType = Reflect.getMetadata<string>("column:type", entityClass.prototype, field.name);
            if (columnType != null) {
                this.columns.set(field.name, {
                    name: field.name,
                    type: columnType,
                    isPrimaryKey: Reflect.hasMetadata("column:primaryKey", entityClass.prototype, field.name)
                });
            }
        }
    }

    save(entity: T): void {
        let values = new Map<string, unknown>();
        for (let [fieldName, column] of this.columns) {
            values.set(column.name, Reflect.get(entity, fieldName));
        }
        // Generate INSERT query...
    }

    findById(id: unknown): T | null {
        // Execute SELECT query...
        let row = this.executeQuery(...);
        if (row == null) return null;

        // Map row to entity
        let entity = Reflect.allocate(this.entityClass);
        for (let [fieldName, column] of this.columns) {
            Reflect.set(entity, fieldName, row[column.name]);
        }
        return entity;
    }
}
```

### HTTP Framework

```typescript
@Controller("/api/users")
class UserController {
    constructor(private userService: UserService) {}

    @GET("/")
    findAll(req: Request): Response {
        return Response.json(this.userService.findAll());
    }

    @GET("/:id")
    findOne(req: Request): Response {
        let id = req.param("id");
        return Response.json(this.userService.findById(id));
    }

    @POST("/")
    create(req: Request): Response {
        let user = req.json<User>();
        return Response.json(this.userService.create(user), 201);
    }
}

// Router implementation
class Router {
    private routes: Route[] = [];

    registerController(cls: Class<Object>): void {
        let prefix = Reflect.getMetadata<string>("controller:prefix", cls) ?? "";
        let instance = Container.resolve(cls);

        for (let method of Reflect.getMethods(cls)) {
            // Check for HTTP method decorators
            for (let httpMethod of ["GET", "POST", "PUT", "DELETE"]) {
                let path = Reflect.getMetadata<string>("route:" + httpMethod, cls.prototype, method.name);
                if (path != null) {
                    this.routes.push({
                        method: httpMethod,
                        path: prefix + path,
                        handler: Reflect.getMethod(instance, method.name)
                    });
                }
            }
        }
    }

    handle(req: Request): Response {
        for (let route of this.routes) {
            if (this.matches(route, req)) {
                return route.handler(req);
            }
        }
        return Response.notFound();
    }
}
```

### Validation Framework

```typescript
class CreateUserDto {
    @IsString()
    @MinLength(1)
    @MaxLength(100)
    name: string;

    @IsNumber()
    @Min(0)
    @Max(150)
    age: number;

    @IsEmail()
    email: string;
}

// Validator implementation
class Validator {
    static validate<T>(obj: T): ValidationResult {
        let errors: ValidationError[] = [];
        let cls = Reflect.getClass(obj);

        for (let field of Reflect.getFields(cls)) {
            let value = Reflect.get(obj, field.name);

            // Check each validation decorator
            for (let decorator of field.decorators) {
                let rule = ValidationRules.get(decorator.name);
                if (rule != null) {
                    let error = rule.validate(value, decorator.args);
                    if (error != null) {
                        errors.push({
                            field: field.name,
                            message: error,
                            decorator: decorator.name
                        });
                    }
                }
            }
        }

        return { valid: errors.length === 0, errors };
    }
}

// Usage
let dto = new CreateUserDto();
dto.name = "";
dto.age = -5;
dto.email = "invalid";

let result = Validator.validate(dto);
// result.errors = [
//   { field: "name", message: "must be at least 1 character" },
//   { field: "age", message: "must be >= 0" },
//   { field: "email", message: "must be a valid email" }
// ]
```

---

## Compiler Flag

Reflection metadata is opt-in:

```bash
# Compile program (reflection metadata always included)
rayac program.raya
```

**What reflection metadata includes:**
- Class structure metadata (fields, methods, parameters)
- Decorator information
- Type information for all declarations
- Class registry for `Reflect.getAllClasses()`

**What's always available (no flag needed):**
- `Reflect.defineMetadata` / `Reflect.getMetadata` (user-defined metadata)
- `Reflect.get` / `Reflect.set` (field access)
- `Reflect.invoke` (method invocation)
- `Reflect.construct` (object creation)

---

## Implementation Notes

### Storage

- Metadata stored in WeakMap for GC-friendliness
- Class registry uses global Map (cleared on VM reset)
- Type info encoded in compact binary format

### Performance

- Reflection calls are slower than direct calls
- Metadata lookup is O(1) via hash maps
- Proxy overhead depends on handler complexity

### Security

- Private fields accessible via reflection (by design)
- Use `--no-reflection-private` to block private access
- Proxies cannot intercept private member access

---

## Comparison with Other Languages

| Feature | Raya | TypeScript | Java | C# |
|---------|------|------------|------|-----|
| Metadata API | Built-in | reflect-metadata lib | Annotations | Attributes |
| Type info at runtime | Always | None (erased) | Full | Full |
| Generic type inspection | Yes (via origin tracking) | No (erased) | Yes (reified) | Yes (reified) |
| Dynamic invocation | Yes | Limited | Yes | Yes |
| Dynamic class creation | Yes | No | Yes (bytecode) | Yes (Emit) |
| Dynamic bytecode generation | Yes (BytecodeBuilder) | No | Yes (ASM/Javassist) | Yes (Emit.ILGenerator) |
| AOP/Proxies | Built-in | Limited | Spring AOP | Castle DynamicProxy |
| Constructor params | Yes | No | Yes | Yes |
| Runtime generic specialization | Yes | No | No | No |

---

## References

- [Java Reflection API](https://docs.oracle.com/javase/tutorial/reflect/)
- [C# System.Reflection](https://docs.microsoft.com/en-us/dotnet/framework/reflection-and-codedom/reflection)
- [TypeScript reflect-metadata](https://github.com/rbuckton/reflect-metadata)
- [Spring AOP](https://docs.spring.io/spring-framework/docs/current/reference/html/core.html#aop)

---

**Document History:**
- 2026-02-02: Added generic type inspection, runtime type creation, and dynamic bytecode generation
- 2026-01-31: Initial design document
