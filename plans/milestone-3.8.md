# Milestone 3.8: Reflection API (std:reflect)

**Status:** Core Implementation Complete (Phases 1-17 handlers done; Phase 12, 18 blocked pending compiler std: import support)
**Goal:** Implement comprehensive runtime reflection for introspection, metadata, dynamic invocation, devtools support, and dynamic code generation

---

## Overview

The Reflect API enables runtime introspection and manipulation of classes, methods, and fields. Key capabilities:
- Metadata storage on any target
- Type introspection (query class structure)
- Generic type inspection (track monomorphized generic origins)
- Dynamic invocation (call methods, get/set fields)
- Object creation (instantiate classes dynamically)
- Object inspection for debugging and devtools
- Proxy objects for interception
- Runtime type creation (classes, functions, generic specializations)
- Dynamic bytecode generation
- **Full VM bootstrap** - create and execute code from an empty VM

**Note:** Reflection metadata is always emitted. All Reflect API features are available at runtime.

See [design/REFLECTION.md](../design/REFLECTION.md) for full specification.

---

## Module Architecture

**Important:** Reflect is NOT a built-in global. It is a native module in `raya-stdlib` that must be imported.

### Import Pattern

```typescript
// Import all Reflect functions as a namespace
import * as Reflect from "std:reflect";

// Usage
const classId = Reflect.getClass(myObject);
Reflect.defineMetadata("key", value, target);

// Or import specific functions
import { getClass, defineMetadata } from "std:reflect";
```

### Implementation Location

```
crates/
├── raya-stdlib/           # Native stdlib implementations
│   └── src/
│       └── reflect.rs     # std:reflect native module
└── raya-engine/
    └── src/vm/reflect/    # Core reflection runtime (used by stdlib)
        ├── mod.rs
        ├── metadata.rs    # WeakMap-style metadata storage
        ├── introspection.rs # Class/field/method queries
        └── class_metadata.rs # ClassMetadataRegistry
```

### Native Module Registration

The `std:reflect` module is registered as a native module in the VM context:

```rust
// In raya-stdlib or VM initialization
let reflect_module = Arc::new(NativeModule::new("reflect", "1.0.0"));
// Add native functions...
vm_context.register_native_module_as("std:reflect", reflect_module)?;
```

### Why Not a Built-in?

1. **Consistency** - All stdlib modules use the same import pattern (`std:*`)
2. **Tree-shaking** - Apps that don't use reflection don't pay for it
3. **Explicit dependencies** - Clear what capabilities a module needs
4. **Future-proof** - Can add more std modules without polluting global namespace

---

## Core Types

```typescript
interface Class<T> {
    readonly id: number;
    readonly name: string;
    readonly parent: Class<Object> | null;
    readonly fieldCount: number;
    readonly fields: FieldInfo[];
    readonly methods: MethodInfo[];
    readonly constructor: ConstructorInfo | null;
    // decorators: DecoratorInfo[] - moved to Milestone 3.9
}

interface FieldInfo {
    readonly name: string;
    readonly type: TypeInfo;
    readonly declaringClass: Class<Object>;
    readonly modifiers: Modifiers;
    readonly isStatic: boolean;
    readonly isReadonly: boolean;
}

interface MethodInfo {
    readonly name: string;
    readonly returnType: TypeInfo;
    readonly parameters: ParameterInfo[];
    readonly declaringClass: Class<Object>;
    readonly modifiers: Modifiers;
    readonly isStatic: boolean;
    readonly isAsync: boolean;
}

interface ConstructorInfo {
    readonly parameters: ParameterInfo[];
    readonly declaringClass: Class<Object>;
}

interface ParameterInfo {
    readonly name: string;
    readonly type: TypeInfo;
    readonly index: number;
    readonly isOptional: boolean;
    readonly defaultValue: unknown | null;
    // decorators: DecoratorInfo[] - moved to Milestone 3.9
}

interface TypeInfo {
    readonly kind: "primitive" | "class" | "interface" | "union" | "function" | "array" | "generic" | "typeParameter";
    readonly name: string;
    readonly classRef: Class<Object> | null;
    readonly unionMembers: TypeInfo[] | null;
    readonly elementType: TypeInfo | null;
    readonly typeArguments: TypeInfo[] | null;
    readonly genericOrigin: GenericOrigin | null;  // For monomorphized types
    readonly typeParameter: GenericParameterInfo | null;  // For type parameters
}

interface Modifiers {
    readonly isPublic: boolean;
    readonly isPrivate: boolean;
    readonly isProtected: boolean;
    readonly isStatic: boolean;
    readonly isReadonly: boolean;
    readonly isAbstract: boolean;
}

// DecoratorInfo moved to Milestone 3.9 (Decorators)
// interface DecoratorInfo {
//     readonly name: string;
//     readonly decorator: Function;
//     readonly args: unknown[];
// }

interface SourceLocation {
    readonly file: string;
    readonly line: number;
    readonly column: number;
}

interface ObjectSnapshot {
    readonly className: string;
    readonly fields: Map<string, FieldSnapshot>;
    readonly identity: number;
    readonly timestamp: number;
}

interface FieldSnapshot {
    readonly name: string;
    readonly value: unknown;
    readonly type: string;
}

interface ObjectDiff {
    readonly added: string[];
    readonly removed: string[];
    readonly changed: Map<string, { old: unknown; new: unknown }>;
}

interface CallFrame {
    readonly functionName: string;
    readonly className: string | null;
    readonly args: unknown[];
    readonly sourceFile: string | null;
    readonly line: number | null;
    readonly column: number | null;
}

interface HeapStats {
    readonly totalObjects: number;
    readonly totalBytes: number;
    readonly byClass: Map<string, { count: number; bytes: number }>;
}
```

---

## Phases

### Phase 1: Core Types and Metadata Storage

Implement basic metadata storage.

**Tasks:**
- [x] Define `Class<T>` interface in std:reflect module types
- [x] Define `FieldInfo`, `MethodInfo`, `ParameterInfo` interfaces in std:reflect
- [x] Define `TypeInfo`, `Modifiers` interfaces in std:reflect
- Note: `DecoratorInfo` moved to Milestone 3.9 (Decorators)
- [x] Implement metadata WeakMap storage in VM (raya-engine)
- [x] Implement `defineMetadata(key, value, target)`
- [x] Implement `defineMetadata(key, value, target, propertyKey)`
- [x] Implement `getMetadata<T>(key, target)`
- [x] Implement `getMetadata<T>(key, target, propertyKey)`
- [x] Implement `hasMetadata(key, target)`
- [x] Implement `getMetadataKeys(target)`
- [x] Implement `deleteMetadata(key, target)`
- [x] Add native call IDs for Reflect methods (0x0Dxx range)
- [x] Add unit tests for metadata storage
- [x] Add MetadataStore to SharedVmState
- [x] Implement native call dispatch in TaskInterpreter
- [ ] Create std:reflect native module in raya-stdlib (blocked: compiler support for std: imports)
- [ ] Register std:reflect module in VM initialization (blocked: compiler support for std: imports)

**Function Signatures:**
```typescript
// Metadata operations
function defineMetadata<T>(key: string, value: T, target: object): void;
function defineMetadata<T>(key: string, value: T, target: object, propertyKey: string): void;
function getMetadata<T>(key: string, target: object): T | null;
function getMetadata<T>(key: string, target: object, propertyKey: string): T | null;
function hasMetadata(key: string, target: object): boolean;
function hasMetadata(key: string, target: object, propertyKey: string): boolean;
function getMetadataKeys(target: object): string[];
function getMetadataKeys(target: object, propertyKey: string): string[];
function deleteMetadata(key: string, target: object): boolean;
function deleteMetadata(key: string, target: object, propertyKey: string): boolean;
```

**Native Call IDs:** `0x0Dxx` (Reflect - `0x0Cxx` is used by JSON)

---

### Phase 2: Class Introspection

Query class structure at runtime.

**Tasks:**
- [x] Implement `getClass<T>(obj)` - get class of object (returns class ID)
- [x] Implement `getClassByName(name)` - lookup by name
- [x] Implement `getAllClasses()` - get all registered classes
- Moved to Milestone 3.9: `getClassesWithDecorator(decorator)` (needs decorator metadata from codegen)
- [x] Implement `isSubclassOf(sub, super)` - check inheritance
- [x] Implement `isInstanceOf<T>(obj, cls)` - type guard
- [x] Implement `getTypeInfo(target)` - get type info (basic - returns type name)
- [x] Implement `getClassHierarchy(obj)` - get inheritance chain
- [x] Add class registry to VM (populated at class definition)
- [x] Add native call IDs and handlers for Phase 2
- [x] Compiler: always emit class metadata
- [x] Add unit tests for introspection

**Function Signatures:**
```typescript
// Class introspection
function getClass<T>(obj: T): number;  // Returns class ID
function getClassByName(name: string): number | null;  // Returns class ID
function getAllClasses(): number[];  // Returns array of class IDs
// getClassesWithDecorator moved to Milestone 3.9
// function getClassesWithDecorator<D extends Function>(decorator: D): number[];
function isSubclassOf(subClassId: number, superClassId: number): boolean;
function isInstanceOf(obj: unknown, classId: number): boolean;
function getTypeInfo(target: object): string;  // Returns type name (basic)
function getClassHierarchy(obj: object): number[];  // Returns array of class IDs from obj to root
```

**Compiler Changes:**
- Reflection is always enabled (no flag needed)
- Generate `ClassInfo` structures for each class
- Include field types, method signatures, parameter names
- Note: Decorator applications stored in Milestone 3.9 (Phase 3 Code Generation)

---

### Phase 3: Field Access

Dynamic field get/set operations.

**Tasks:**
- [x] Implement `get<T>(target, propertyKey)` - get field value
- [x] Implement `set<T>(target, propertyKey, value)` - set field value
- [x] Implement `has(target, propertyKey)` - check field exists
- [x] Implement `getFieldNames(target)` - list all fields
- [x] Implement `getFieldInfo(target, propertyKey)` - get field metadata (returns Map)
- [x] Implement `getFields(target)` - get all field infos (returns array of Maps)
- [x] Handle private field access (allowed by default - no distinction at runtime)
- [x] Handle static fields (`getStaticFieldNames`, `getStaticFields`)
- [x] Add unit tests for field access

**Function Signatures:**
```typescript
// Field access
function get<T>(target: object, propertyKey: string): T | null;
function set<T>(target: object, propertyKey: string, value: T): boolean;
function has(target: object, propertyKey: string): boolean;
function getFieldNames(target: object): string[];
function getFieldInfo(target: object, propertyKey: string): FieldInfo | null;
function getFields(target: object): FieldInfo[];
function getStaticFieldNames<T>(cls: Class<T>): string[];
function getStaticFields<T>(cls: Class<T>): FieldInfo[];
```

---

### Phase 4: Method Invocation

Dynamic method calling.

**Tasks:**
- [x] Implement `invoke<R>(target, methodName, ...args)` - stub with error (needs VM execution context)
- [x] Implement `invokeAsync<R>(target, methodName, ...args)` - stub with error (needs VM execution context)
- [x] Implement `getMethod<F>(target, methodName)` - returns vtable index
- [x] Implement `getMethodInfo(target, methodName)` - get metadata as Map
- [x] Implement `getMethods(target)` - list all methods as array of Maps
- [x] Implement `hasMethod(target, methodName)` - check exists
- [x] Handle static methods (`getStaticMethods`, `invokeStatic` stub)
- N/A: Handle method overloading (function overloading is banned in Raya - see design/LANG.md §8.6)
- [x] Add unit tests for method info

**Function Signatures:**
```typescript
// Method invocation
function invoke<R>(target: object, methodName: string, ...args: unknown[]): R;
function invokeAsync<R>(target: object, methodName: string, ...args: unknown[]): Task<R>;
function getMethod<F extends Function>(target: object, methodName: string): F | null;
function getMethodInfo(target: object, methodName: string): MethodInfo | null;
function getMethods(target: object): MethodInfo[];
function hasMethod(target: object, methodName: string): boolean;
function invokeStatic<T, R>(cls: Class<T>, methodName: string, ...args: unknown[]): R;
function getStaticMethods<T>(cls: Class<T>): MethodInfo[];
```

---

### Phase 5: Object Creation

Dynamic instantiation.

**Tasks:**
- [x] Implement `construct<T>(cls, ...args)` - create instance (basic, no constructor call yet)
- [x] Implement `constructWith<T>(cls, params)` - stub with error (needs named parameter mapping)
- [x] Implement `allocate<T>(cls)` - empty instance (uninitialized)
- [x] Implement `clone<T>(obj)` - shallow clone
- [x] Implement `deepClone<T>(obj)` - deep clone (recursive)
- [x] Implement `getConstructorInfo(cls)` - constructor metadata as Map
- [x] Implement `getConstructorParameterTypes(cls)` - included in getConstructorInfo
- [x] Implement `getConstructorParameterNames(cls)` - included in getConstructorInfo
- [x] Add unit tests for object creation

**Function Signatures:**
```typescript
// Object creation
function construct<T>(cls: Class<T>, ...args: unknown[]): T;
function constructWith<T>(cls: Class<T>, params: Map<string, unknown>): T;
function allocate<T>(cls: Class<T>): T;  // Uninitialized instance
function clone<T>(obj: T): T;  // Shallow clone
function deepClone<T>(obj: T): T;  // Deep clone
function getConstructorInfo<T>(cls: Class<T>): ConstructorInfo;
function getConstructorParameterTypes<T>(cls: Class<T>): TypeInfo[];
function getConstructorParameterNames<T>(cls: Class<T>): string[];
```

---

### Phase 6: Type Utilities

Runtime type checking and conversion.

**Tasks:**
- [x] Implement `Reflect.typeOf(typeName)` - get TypeInfo from string
- Deferred: Implement `Reflect.typeOf<T>()` - get TypeInfo from generic (requires compiler support for type introspection)
- [x] Implement `Reflect.isAssignableTo(source, target)` - check compatibility
- [x] Implement type guards: `isString`, `isNumber`, `isBoolean`, `isNull`
- [x] Implement type guards: `isArray`, `isFunction`, `isObject`
- [x] Implement `Reflect.cast<T>(value, cls)` - safe cast (returns null)
- [x] Implement `Reflect.castOrThrow<T>(value, cls)` - cast or throw
- [x] Add unit tests for type utilities

**Function Signatures:**
```typescript
// Type utilities
function typeOf(typeName: string): TypeInfo | null;
function typeOf<T>(): TypeInfo;  // Compile-time resolved
function isAssignableTo(source: TypeInfo, target: TypeInfo): boolean;
function isString(value: unknown): value is string;
function isNumber(value: unknown): value is number;
function isBoolean(value: unknown): value is boolean;
function isNull(value: unknown): value is null;
function isArray<T>(value: unknown): value is T[];
function isFunction(value: unknown): value is Function;
function isObject(value: unknown): value is object;
function cast<T>(value: unknown, cls: Class<T>): T | null;
function castOrThrow<T>(value: unknown, cls: Class<T>): T;
```

---

### Phase 7: Interface and Hierarchy Query

Query type relationships.

**Tasks:**
- [x] Implement `Reflect.implements(cls, interfaceName)` - check interface implementation
- [x] Implement `Reflect.getInterfaces(cls)` - get implemented interfaces
- [x] Implement `Reflect.getImplementors(interfaceName)` - find implementors
- [x] Implement `Reflect.isStructurallyCompatible(a, b)` - structural check
- [x] Implement `Reflect.getSuperclass(cls)` - get parent class
- [x] Implement `Reflect.getSubclasses(cls)` - get direct subclasses
- [x] Track interface implementations in class metadata
- [x] Add unit tests for interface queries

**Function Signatures:**
```typescript
// Interface and hierarchy queries
function implements<T>(cls: Class<T>, interfaceName: string): boolean;
function getInterfaces<T>(cls: Class<T>): string[];
function getImplementors(interfaceName: string): Class<Object>[];
function isStructurallyCompatible(a: TypeInfo, b: TypeInfo): boolean;
function getSuperclass<T>(cls: Class<T>): Class<Object> | null;
function getSubclasses<T>(cls: Class<T>): Class<Object>[];
```

---

### Phase 8: Object Inspection & DevTools

Comprehensive object inspection for debugging and development tools.

**Tasks:**

*Object Inspection:*
- [x] Implement `Reflect.inspect(obj)` - human-readable object representation
- [x] Implement `Reflect.snapshot(obj)` - capture object state as `ObjectSnapshot`
- [x] Implement `Reflect.diff(a, b)` - compare two objects/snapshots
- [x] Implement `Reflect.getObjectId(obj)` - unique identity for tracking
- [x] Implement `Reflect.getClassHierarchy(obj)` - inheritance chain from object's class to root (already in Phase 2)
- [x] Implement `Reflect.describe(cls)` - detailed class description string

*Memory Analysis:*
- [x] Implement `Reflect.getObjectSize(obj)` - shallow memory footprint
- [x] Implement `Reflect.getRetainedSize(obj)` - size with retained objects (traverses object graph)
- [x] Implement `Reflect.getReferences(obj)` - objects referenced by this object
- [x] Implement `Reflect.getReferrers(obj)` - objects referencing this (scans heap allocations)
- [x] Implement `Reflect.getHeapStats()` - total objects, memory usage by class
- [x] Implement `Reflect.findInstances<T>(cls)` - all live instances of a class

*Stack/Execution Introspection:*
- [x] Implement `Reflect.getCallStack()` - current call frames with function names
- [x] Implement `Reflect.getLocals(frame?)` - local variables by index
- [x] Implement `Reflect.getSourceLocation(method)` - file:line:col mapping (requires debug info in bytecode)

*Serialization Helpers:*
- [x] Implement `Reflect.toJSON(obj)` - serializable representation
- [x] Implement `Reflect.getEnumerableKeys(obj)` - keys suitable for iteration
- [x] Implement `Reflect.isCircular(obj)` - check for circular references

- [x] Add unit tests for inspection (basic tests via existing test infrastructure)
- [x] Add unit tests for snapshot/diff (in reflect/snapshot.rs)
- [x] Add integration tests for memory analysis (in tests/reflect_phase8_tests.rs)
- [x] Add integration tests for stack introspection (in tests/reflect_phase8_tests.rs)
- [x] Add Phase 8 function declarations to reflect.d.raya

**Function Signatures:**
```typescript
// Object inspection
function inspect(obj: object): string;
function snapshot(obj: object): ObjectSnapshot;
function diff(a: object | ObjectSnapshot, b: object | ObjectSnapshot): ObjectDiff;
function getObjectId(obj: object): number;
function getClassHierarchy(obj: object): Class<Object>[];
function describe<T>(cls: Class<T>): string;

// Memory analysis
function getObjectSize(obj: object): number;  // Shallow size in bytes
function getRetainedSize(obj: object): number;  // Size with retained objects
function getReferences(obj: object): object[];  // Objects this references
function getReferrers(obj: object): object[];  // Objects referencing this
function getHeapStats(): HeapStats;
function findInstances<T>(cls: Class<T>): T[];

// Stack/execution introspection
function getCallStack(): CallFrame[];
function getLocals(frameIndex?: number): Map<string, unknown>;
function getSourceLocation(method: MethodInfo): SourceLocation | null;

// Serialization helpers
function toJSON(obj: object): string;
function getEnumerableKeys(obj: object): string[];
function isCircular(obj: object): boolean;
```

---

### Phase 9: Proxy Objects

Basic proxy support for interception (no AOP - that's a separate feature).

**Tasks:**
- [x] Define `Proxy` struct in vm/object.rs
- [x] Add native call IDs for proxy operations (0x0DB0-0x0DBF range)
- [x] Implement `Reflect.createProxy<T>(target, handler)` - create proxy
- [x] Implement `Reflect.isProxy(obj)` - check if object is a proxy
- [x] Implement `Reflect.getProxyTarget(proxy)` - get underlying target
- [x] Implement `Reflect.getProxyHandler(proxy)` - get handler object
- [x] Add unit tests for proxies (in tests/reflect_phase8_tests.rs)
- [x] Implement proxy trap: `get` - intercept property access (passthrough to target; full trap invocation deferred)
- [x] Implement proxy trap: `set` - intercept property assignment (passthrough to target; full trap invocation deferred)
- [x] Implement proxy trap: `has` - intercept `in` operator (helper functions in proxy.rs)
- [x] Implement proxy trap: `invoke` - intercept method calls (helper functions in proxy.rs)
- [x] Define `ProxyHandler<T>` interface in reflect.d.raya

**Function Signatures:**
```typescript
// Proxy objects
function createProxy<T extends object>(target: T, handler: ProxyHandler<T>): T;
function isProxy(obj: object): boolean;
function getProxyTarget<T extends object>(proxy: T): T | null;
```

**ProxyHandler Interface:**
```typescript
interface ProxyHandler<T> {
    get?(target: T, property: string): unknown;
    set?(target: T, property: string, value: unknown): boolean;
    has?(target: T, property: string): boolean;
    invoke?(target: T, method: string, args: unknown[]): unknown;
}
```

---

### Phase 10: Dynamic Subclass Creation

Create classes at runtime.

**Status:** ✅ Complete

**Tasks:**
- [x] Define `SubclassDefinition<T>` interface (type_builder.rs)
- [x] Define `FieldDefinition` interface (type_builder.rs)
- [x] Implement `Reflect.createSubclass<T, S>(superclass, name, definition)` (native ID 0x0DC0)
- [x] Implement `Reflect.extendWith<T>(cls, fields)` - add fields (native ID 0x0DC1)
- [x] Implement `Reflect.defineClass(name, definition)` - create root class (native ID 0x0DC2)
- [x] Implement `Reflect.addMethod(classId, name, functionId)` (native ID 0x0DC3)
- [x] Implement `Reflect.setConstructor(classId, functionId)` (native ID 0x0DC4)
- [x] Generate runtime class structures (DynamicClassBuilder)
- [x] Register dynamic classes in class registry
- [x] Add unit tests for dynamic classes (7 tests)

**Function Signatures:**
```typescript
// Dynamic class creation
function createSubclass<T, S extends T>(
    superclass: Class<T>,
    name: string,
    definition: SubclassDefinition<S>
): Class<S>;
function extendWith<T>(cls: Class<T>, fields: FieldDefinition[]): Class<T>;
```

**Supporting Interfaces:**
```typescript
interface SubclassDefinition<T> {
    fields?: FieldDefinition[];
    methods?: MethodDefinition[];
    constructor?: (...args: unknown[]) => void;
}

interface FieldDefinition {
    name: string;
    type: TypeInfo;
    initialValue?: unknown;
    isStatic?: boolean;
    isReadonly?: boolean;
}

interface MethodDefinition {
    name: string;
    implementation: Function;
    isStatic?: boolean;
    isAsync?: boolean;
}
```

---

### Phase 11: Compiler Integration

Emit reflection metadata.

**Tasks:**
- [x] Reflection always enabled (no flag needed)
- [x] Generate class structure metadata (ReflectionData in bytecode module)
- [x] Generate field type information (FieldReflectionData with name, type_name, is_readonly, is_static)
- [x] Generate method signature metadata (method_names in ClassReflectionData)
- Deferred: Generate parameter name/type information (requires AST parameter info preservation through lowering)
- Note: Decorator application info generation moved to Milestone 3.9 Phase 3
- [x] Emit compact binary format for metadata (encode/decode in Module)
- Blocked: Add integration tests with reflection enabled (requires compiler support for `std:` imports)

**Metadata Format:**
```
ClassMetadata {
    name: String,
    superclass: Option<ClassId>,
    fields: Vec<FieldMetadata>,
    methods: Vec<MethodMetadata>,
    constructors: Vec<ConstructorMetadata>,
    // decorators: Vec<DecoratorMetadata> - moved to Milestone 3.9
}
```

---

### Phase 12: Framework Integration Tests

End-to-end tests with framework patterns.

**Status:** Blocked - requires compiler support for `std:` module imports

**Tasks:**
- Blocked: Test: Dependency Injection container using Reflect
- Note: ORM entity mapping with decorators - moved to Milestone 3.9 Phase 5
- Blocked: Test: HTTP routing framework with controller discovery
- Note: Validation framework with decorator-based rules - moved to Milestone 3.9 Phase 5
- Blocked: Test: Serialization using field introspection
- Blocked: Test: Object inspection and diff for state debugging
- Blocked: Test: DevTools integration (inspect, snapshot, memory)
- Blocked: Performance benchmarks vs direct calls

**Dependency:** These tests require the compiler to support `std:reflect` imports and emit native calls. Until then, reflection functionality is validated through Rust unit tests in `vm/reflect/` modules.

---

### Phase 13: Generic Type Metadata

Track generic type origins through monomorphization for runtime inspection.

**Status:** ✅ Complete (runtime infrastructure, compiler integration deferred)

**Background:**
Raya uses monomorphization - generic types like `Box<T>` become concrete types like `Box_number`, `Box_string` at compile time. To enable generic inspection at runtime, we must preserve the generic origin information.

**Tasks:**

*Data Structures (generic_metadata.rs):*
- [x] Define `GenericTypeInfo` struct to store generic origin
- [x] Define `GenericParameterInfo` for type parameters (name, constraints)
- [x] Define `SpecializedTypeInfo` for monomorphized class info
- [x] Define `GenericTypeRegistry` to track generic type relationships

*Runtime Handlers (handlers/reflect.rs):*
- [x] Implement `Reflect.getGenericOrigin(cls)` - get generic class name (0x0DD0)
- [x] Implement `Reflect.getTypeParameters(cls)` - get type parameter info (0x0DD1)
- [x] Implement `Reflect.getTypeArguments(cls)` - get actual type arguments (0x0DD2)
- [x] Implement `Reflect.isGenericInstance(cls)` - check if monomorphized (0x0DD3)
- [x] Implement `Reflect.getGenericBase(cls)` - get base generic class ID (0x0DD4)
- [x] Implement `Reflect.findSpecializations(genericName)` - find all monomorphized versions (0x0DD5)
- [x] Add native call IDs (0x0DD0-0x0DDF range)
- [x] Add unit tests for generic inspection (17 tests in generic_metadata.rs)

**Deferred:** Compiler integration to record generic origins during monomorphization (requires compiler/mono.rs changes)

**Function Signatures:**
```typescript
// Generic type inspection
function getGenericOrigin<T>(cls: Class<T>): string | null;  // e.g., "Box" for Box_number
function getTypeParameters<T>(cls: Class<T>): GenericParameterInfo[];
function getTypeArguments<T>(cls: Class<T>): TypeInfo[];  // e.g., [TypeInfo(number)] for Box_number
function isGenericInstance<T>(cls: Class<T>): boolean;
function getGenericBase(genericName: string): number | null;  // Base generic class ID
function findSpecializations(genericName: string): Class<Object>[];  // All Box_* classes

interface GenericParameterInfo {
    readonly name: string;           // e.g., "T"
    readonly index: number;          // Position in type parameter list
    readonly constraint: TypeInfo | null;  // e.g., constraint "extends Comparable"
}

interface GenericOrigin {
    readonly name: string;           // e.g., "Box"
    readonly typeParameters: string[];  // e.g., ["T"]
    readonly typeArguments: TypeInfo[];  // e.g., [TypeInfo(number)]
}
```

**Compiler Changes:**
- During monomorphization in `compiler/mono.rs`, record mapping from `Box_number` → `Box<T>` with `T=number`
- Store in `GenericOriginTable` embedded in `ReflectionData`
- Format: `Map<SpecializedClassId, GenericOrigin>`

---

### Phase 14: Runtime Type Creation

Create new types (classes, functions) at runtime.

**Status:** ✅ Complete

**Tasks:**

*Class Creation:*
- [x] Define `ClassBuilder` interface for incremental class construction (runtime_builder.rs)
- [x] Implement `Reflect.newClassBuilder(name)` - create ClassBuilder (0x0DE0)
- [x] Implement `ClassBuilder.addField(builderId, name, type, options)` - add field (0x0DE1)
- [x] Implement `ClassBuilder.addMethod(builderId, name, functionId, options)` - add method (0x0DE2)
- [x] Implement `ClassBuilder.setConstructor(builderId, functionId)` - set constructor (0x0DE3)
- [x] Implement `ClassBuilder.setParent(builderId, parentClassId)` - set parent (0x0DE4)
- [x] Implement `ClassBuilder.addInterface(builderId, interfaceName)` - add interface (0x0DE5)
- [x] Implement `ClassBuilder.build(builderId)` - finalize and register class (0x0DE6)
- [x] Generate runtime VTable for dynamic classes (via DynamicClassBuilder)
- [x] Register dynamic classes in ClassRegistry with unique IDs

*Function Creation:*
- [x] Implement `Reflect.createFunction(name, paramCount, bytecode)` - create function (0x0DE7)
- [x] Implement `Reflect.createAsyncFunction(name, paramCount, bytecode)` - create async function (0x0DE8)
- [x] Implement `Reflect.createClosure(functionId, captures)` - create closure with captures (0x0DE9)
- [x] Implement `Reflect.createNativeCallback(callbackId)` - register native callback (0x0DEA)
- [x] Support function body as bytecode array (DynamicFunction)

*Generic Specialization:*
- [x] Implement `Reflect.specialize(genericName, typeArgs)` - lookup/create specialization (0x0DEB)
- [x] Implement `Reflect.getSpecializationCache()` - get cached specializations (0x0DEC)
- [x] Cache specializations to avoid duplicate generation (SpecializationCache)
- **Deferred:** Generate specialized bytecode from generic template (requires compiler/mono.rs integration)

*Native Call IDs:* 0x0DE0-0x0DEF (13 handlers implemented)

**Function Signatures:**
```typescript
// Class creation
function createClass<T>(name: string, definition: ClassDefinition): Class<T>;
function createSubclass<T, S extends T>(parent: Class<T>, name: string, definition: ClassDefinition): Class<S>;

// Function creation
function createFunction<R>(name: string, params: ParameterDefinition[], body: FunctionBody): (...args: unknown[]) => R;
function createAsyncFunction<R>(name: string, params: ParameterDefinition[], body: FunctionBody): (...args: unknown[]) => Task<R>;
function createClosure<R>(params: ParameterDefinition[], body: FunctionBody, captures: Map<string, unknown>): (...args: unknown[]) => R;

// Generic specialization
function specialize<T>(genericName: string, typeArgs: TypeInfo[]): Class<T>;

interface ClassDefinition {
    fields?: FieldDefinition[];
    methods?: MethodDefinition[];
    constructor?: ConstructorDefinition;
    interfaces?: string[];
}

interface FieldDefinition {
    name: string;
    type: TypeInfo;
    initialValue?: unknown;
    isStatic?: boolean;
    isReadonly?: boolean;
}

interface MethodDefinition {
    name: string;
    params: ParameterDefinition[];
    returnType: TypeInfo;
    body: FunctionBody;
    isStatic?: boolean;
    isAsync?: boolean;
}

interface ConstructorDefinition {
    params: ParameterDefinition[];
    body: FunctionBody;
}

interface ParameterDefinition {
    name: string;
    type: TypeInfo;
    isOptional?: boolean;
    defaultValue?: unknown;
}

// Function body can be bytecode, AST, or native
type FunctionBody =
    | { kind: "bytecode"; instructions: number[] }
    | { kind: "ast"; statements: Statement[] }
    | { kind: "native"; callback: NativeCallback };
```

---

### Phase 15: Dynamic Bytecode Generation

JIT-style bytecode emission for runtime-created types.

**Status:** ✅ Complete

**Tasks:**

*Bytecode Builder API (bytecode_builder.rs):*
- [x] Define `BytecodeBuilder` class for programmatic bytecode construction
- [x] Implement instruction emission: `emit(opcode, ...operands)` (0x0DF1)
- [x] Implement label system: `defineLabel()`, `markLabel()`, `emitJump()` (0x0DF3-0x0DF6)
- [x] Implement local variable management: `declareLocal(type)`, `emitLoadLocal()`, `emitStoreLocal()` (0x0DF7-0x0DF9)
- [x] Implement constant pushing: `emitPush(value)` with type detection (0x0DF2)
- [x] Implement control flow: `emitJumpIf()`, `emitReturn()` (0x0DF6, 0x0DFB)
- [x] Implement object operations: `emitNew()`, `emitLoadField()`, `emitStoreField()`
- [x] Implement method calls: `emitCall()`, `emitNativeCall()` (0x0DFA)
- [x] Implement validation: verify stack balance, label resolution (0x0DFC)
- [x] Arithmetic operations: `emit_iadd()`, `emit_isub()`, `emit_fadd()`, etc.
- [x] Comparison operations: `emit_ieq()`, `emit_ilt()`, `emit_eq()`, etc.
- [x] Stack type tracking for validation

*Function Compilation:*
- [x] Implement `BytecodeBuilder.build()` - returns CompiledFunction (0x0DFD)
- [x] Generate proper function metadata (locals, stack depth, constants)
- [x] Register compiled functions via BytecodeBuilderRegistry
- [x] Label resolution in single pass at build() time

*Module Extension:*
- [x] Stub for `Reflect.extendModule(module, additions)` (0x0DFE)
- [x] Dynamic function registration via BytecodeBuilderRegistry

*Native Call IDs:* 0x0DF0-0x0DFE (15 handlers implemented)

**Deferred:** Hot-swapping function implementations, cross-module references for dynamic code

**Function Signatures:**
```typescript
// Bytecode builder
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

    // Arithmetic (typed)
    emitIAdd(): void;  // Integer add
    emitFAdd(): void;  // Float add
    emitNAdd(): void;  // Number add (dynamic)
    // ... other arithmetic ops

    // Comparison
    emitICmpEq(): void;
    emitICmpLt(): void;
    // ... other comparison ops

    // Return
    emitReturn(): void;
    emitReturnValue(): void;

    // Build
    validate(): ValidationResult;
    build(): CompiledFunction;
}

interface Label {
    readonly id: number;
}

interface ValidationResult {
    readonly isValid: boolean;
    readonly errors: string[];
}

interface CompiledFunction {
    readonly functionId: number;
    readonly name: string;
    readonly bytecode: number[];
}

// Module extension
function extendModule(moduleName: string, additions: ModuleAdditions): void;

interface ModuleAdditions {
    functions?: CompiledFunction[];
    classes?: Class<Object>[];
}
```

**Implementation Notes:**

1. **Stack Type Tracking**: The builder must track value types on the operand stack to emit correct typed opcodes (IADD vs FADD vs NADD).

2. **Verification**: Before `build()`, validate:
   - Stack is balanced (push/pop match)
   - All labels are defined and marked
   - Local variable indices are valid
   - Type compatibility for operations

3. **Integration with VM**:
   - Dynamic functions stored in `Module.functions` with generated IDs (0x80000000+)
   - VTable entries for dynamic methods point to dynamic function IDs
   - GC must trace closures created from dynamic functions

4. **Security Considerations**:
   - Optional sandboxing for dynamically generated code
   - Bytecode verification to prevent malformed instructions
   - Memory limits on dynamic code generation

5. **Performance Design** (Critical):
   - BytecodeBuilder uses pre-allocated Vec with capacity hint
   - Labels resolved in single pass at build() time
   - No heap allocations in emit* methods (append to pre-allocated buffer)
   - Built bytecode is identical to compiler-generated bytecode
   - Same interpreter loop executes both static and dynamic code
   - **No runtime "is_dynamic" checks** - treat all bytecode identically

---

### Phase 16: Reflection Security & Permissions

Control access to reflection capabilities.

**Status:** ✅ Complete

**Tasks:**
- [x] Define `ReflectionPermission` flags (bitflags: READ_PUBLIC/PRIVATE, WRITE_PUBLIC/PRIVATE, INVOKE_PUBLIC/PRIVATE, CREATE_TYPES, GENERATE_CODE)
- [x] Implement `Reflect.setPermissions(target, permissions)` - set object-level permissions (0x0E00)
- [x] Implement `Reflect.getPermissions(target)` - get resolved permissions (0x0E01)
- [x] Implement `Reflect.hasPermission(target, permission)` - check specific flag (0x0E02)
- [x] Implement `Reflect.clearPermissions(target)` - clear object permissions (0x0E03)
- [x] Implement `Reflect.setClassPermissions(classId, permissions)` - class-level (0x0E04)
- [x] Implement `Reflect.getClassPermissions(classId)` - get class permissions (0x0E05)
- [x] Implement `Reflect.clearClassPermissions(classId)` - clear class permissions (0x0E06)
- [x] Implement `Reflect.setModulePermissions(moduleName, permissions)` - module-level (0x0E07)
- [x] Implement `Reflect.getModulePermissions(moduleName)` - get module permissions (0x0E08)
- [x] Implement `Reflect.clearModulePermissions(moduleName)` - clear module permissions (0x0E09)
- [x] Implement `Reflect.setGlobalPermissions(permissions)` - global default (0x0E0A)
- [x] Implement `Reflect.getGlobalPermissions()` - get global permissions (0x0E0B)
- [x] Implement `Reflect.sealPermissions(target)` - make immutable (0x0E0C)
- [x] Implement `Reflect.isPermissionsSealed(target)` - check sealed (0x0E0D)
- [x] Add TOML config support for module permissions
- [x] Add unit tests for permission system (15+ tests in permissions.rs)

**Native Call IDs:** 0x0E00-0x0E0D (14 handlers implemented)

**Files:**
- `crates/raya-engine/src/vm/reflect/permissions.rs` - ReflectionPermission, PermissionStore, TOML loading
- `crates/raya-engine/src/vm/vm/handlers/reflect.rs` - Phase 16 handlers
- `design/REFLECT_SECURITY.md` - Security model design document

**Function Signatures:**
```typescript
enum ReflectionPermission {
    NONE = 0,
    READ_PUBLIC = 1,
    READ_PRIVATE = 2,
    WRITE_PUBLIC = 4,
    WRITE_PRIVATE = 8,
    INVOKE_PUBLIC = 16,
    INVOKE_PRIVATE = 32,
    CREATE_TYPES = 64,
    GENERATE_BYTECODE = 128,
    ALL = 255,
}

function setPermissions(target: object | Class<Object>, permissions: ReflectionPermission): void;
function getPermissions(target: object | Class<Object>): ReflectionPermission;
```

---

### Phase 17: Dynamic VM Bootstrap (Create Running Code from Empty VM)

Enable creating and executing code entirely at runtime without any pre-compiled modules.

**Status:** ✅ Complete (Infrastructure and handlers implemented; full execution deferred pending VM context threading)

**Goal:** Start with an empty VM and use Reflect API to create modules, classes, functions, and execute them - full bootstrap capability.

**Performance Requirements:**
- Zero overhead for statically compiled code (no runtime checks for "is this dynamic?")
- Dynamic code uses same interpreter loop as static code (no separate slow path)
- Bytecode validation happens once at creation time, not at execution time
- Dynamic function lookup uses same O(1) table as static functions
- No boxing/unboxing overhead for dynamic values

**Tasks:**

*Dynamic Module Creation (0x0E10-0x0E17):*
- [x] Implement `Reflect.createModule(name)` - create empty module at runtime (0x0E10)
- [x] Implement `Reflect.moduleAddFunction(moduleId, funcId)` - add function to module (0x0E11)
- [x] Implement `Reflect.moduleAddClass(moduleId, classId, name)` - add class to module (0x0E12)
- [x] Implement `Reflect.moduleAddGlobal(moduleId, name, value)` - add global variable (0x0E13)
- [x] Implement `Reflect.moduleSeal(moduleId)` - finalize module for execution (0x0E14)
- [x] Implement `Reflect.moduleLink(moduleId, imports)` - resolve imports (stub) (0x0E15)
- [x] Implement `Reflect.getModule(moduleId)` - get module info by ID (0x0E16)
- [x] Implement `Reflect.getModuleByName(name)` - get module by name (0x0E17)

*Entry Point & Execution (0x0E18-0x0E1F):*
- [x] Implement `Reflect.execute(functionId, args)` - execute function (stub - needs VM context) (0x0E18)
- [x] Implement `Reflect.spawn(functionId, args)` - spawn as Task (stub) (0x0E19)
- [x] Implement `Reflect.eval(bytecode)` - execute raw bytecode (stub) (0x0E1A)
- [x] Implement `Reflect.callDynamic(functionId, args)` - call dynamic function (stub) (0x0E1B)
- [x] Implement `Reflect.invokeDynamicMethod(target, methodIndex, args)` - invoke method (stub) (0x0E1C)
- **Deferred:** Full execution context threading (requires passing VM/Task to handlers)

*Runtime Class System:*
- [x] Dynamic VTable generation for runtime classes (via Phase 10/14 DynamicClassBuilder)
- [x] Runtime method dispatch for dynamically added methods (via addMethod)
- [x] Dynamic field layout calculation (via FieldDefinition)
- [x] Support inheritance between runtime-created classes (via createSubclass)
- [x] Constructor invocation for runtime classes (via setConstructor)

*Standard Library Bootstrap (0x0E20-0x0E28):*
- [x] Implement `Reflect.bootstrap()` - initialize minimal runtime environment (0x0E20)
- [x] Implement `Reflect.getObjectClass()` - get core Object class ID (0x0E21)
- [x] Implement `Reflect.getArrayClass()` - get core Array class ID (0x0E22)
- [x] Implement `Reflect.getStringClass()` - get core String class ID (0x0E23)
- [x] Implement `Reflect.getTaskClass()` - get core Task class ID (0x0E24)
- [x] Implement `Reflect.dynamicPrint(message)` - print to console (0x0E25)
- [x] Implement `Reflect.createDynamicArray(elements)` - create array (0x0E26)
- [x] Implement `Reflect.createDynamicString(value)` - create string (0x0E27)
- [x] Implement `Reflect.isBootstrapped()` - check if context exists (0x0E28)
- [x] Add unit tests (17 tests in dynamic_module.rs, 7 tests in bootstrap.rs)

**Native Call IDs:** 0x0E10-0x0E28 (25 handlers implemented)

**Files:**
- `crates/raya-engine/src/vm/reflect/dynamic_module.rs` - DynamicModule, DynamicModuleRegistry
- `crates/raya-engine/src/vm/reflect/bootstrap.rs` - BootstrapContext, ExecutionOptions
- `crates/raya-engine/src/vm/vm/handlers/reflect.rs` - Phase 17 handlers
- `design/DYNAMIC_VM_BOOTSTRAP.md` - Design document

**Function Signatures:**
```typescript
// Module creation
function createModule(name: string): DynamicModule;

interface DynamicModule {
    readonly name: string;
    readonly isSealed: boolean;

    addFunction(func: CompiledFunction): number;  // Returns function ID
    addClass(cls: DynamicClass): number;  // Returns class ID
    addGlobal(name: string, value: unknown): void;
    seal(): void;  // Finalize for execution
}

// Execution
function execute<R>(func: CompiledFunction | Closure, ...args: unknown[]): R;
function spawn<R>(func: CompiledFunction | Closure, ...args: unknown[]): Task<R>;
function eval(bytecode: number[]): unknown;

// Bootstrap
function bootstrap(): BootstrapContext;

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

**Example - Hello World from Empty VM:**
```typescript
// Start with nothing - create everything dynamically
const ctx = Reflect.bootstrap();
const module = Reflect.createModule("main");

// Build a "hello" function using BytecodeBuilder
const builder = new BytecodeBuilder("hello", [], Reflect.typeOf("void"));
builder.emitPush("Hello from dynamic code!");
builder.emitNativeCall(0x0100);  // print native call
builder.emitReturnVoid();

const helloFunc = builder.build();
module.addFunction(helloFunc);
module.seal();

// Execute it
Reflect.execute(helloFunc);  // Prints: "Hello from dynamic code!"
```

**Example - Dynamic Class with Methods:**
```typescript
const ctx = Reflect.bootstrap();
const module = Reflect.createModule("shapes");

// Create Point class
const pointClass = Reflect.createClass<{x: number, y: number}>("Point", {
    fields: [
        { name: "x", type: Reflect.typeOf("number") },
        { name: "y", type: Reflect.typeOf("number") },
    ],
});

// Add toString method dynamically
const toStringBuilder = new BytecodeBuilder("toString", [], Reflect.typeOf("string"));
toStringBuilder.emitLoadLocal(0);  // this
toStringBuilder.emitLoadField(0);  // this.x
toStringBuilder.emitToString();
// ... build full "(x, y)" string
toStringBuilder.emitReturn();

pointClass.addMethod("toString", toStringBuilder.build());
module.addClass(pointClass);
module.seal();

// Use the dynamic class
const point = Reflect.construct(pointClass, 10, 20);
logger.info(Reflect.invoke(point, "toString"));  // "(10, 20)"
```

**Implementation Notes:**

1. **VM State Management**:
   - Dynamic modules stored in `SharedVmState.dynamic_modules`
   - Function IDs for dynamic functions use high bit (0x80000000+)
   - Class IDs for dynamic classes use range 0x10000000+

2. **Execution Context**:
   - Dynamic code runs in a special "dynamic context"
   - Stack and locals work the same as compiled code
   - GC tracks dynamic closures and objects

3. **Bootstrapping Order**:
   - `bootstrap()` initializes core type registry
   - Core classes (Object, Array, String) registered first
   - Then user can create modules/classes/functions

4. **Error Handling**:
   - Invalid bytecode throws `BytecodeError`
   - Type mismatches throw `TypeError`
   - Stack overflow/underflow throws `RuntimeError`

5. **Performance Design**:
   - Dynamic functions stored in same `Module.functions` array (extended, not separate)
   - Dynamic class VTables identical in structure to static VTables
   - No runtime "is_dynamic" checks in hot paths
   - Bytecode fully validated at `build()` time - execution assumes valid
   - Use inline caching for dynamic method dispatch (same as static)

---

### Phase 18: Performance Validation

Ensure dynamic code generation has no impact on static code performance.

**Status:** Blocked - requires end-to-end tests from Phase 12

**Tasks:**

*Zero-Overhead Verification:*
- Blocked: Benchmark: Static code performance unchanged with dynamic code infrastructure present
- [x] Verify: No additional branches in interpreter hot path for dynamic code (design ensures same code path)
- [x] Verify: Module.functions lookup remains O(1) with dynamic functions (implemented)
- [x] Verify: VTable dispatch unchanged for dynamic classes (same VTable structure)
- Blocked: Profile: Memory overhead of reflection infrastructure when unused

*Dynamic Code Performance:*
- Blocked: Benchmark: Dynamic function execution vs equivalent static function (<5% overhead)
- Blocked: Benchmark: Dynamic class instantiation vs static class (<10% overhead)
- Blocked: Benchmark: Dynamic method dispatch vs static dispatch (<10% overhead)
- [x] Optimize: BytecodeBuilder should pre-allocate to minimize allocations (implemented with capacity hints)
- [x] Optimize: Avoid string operations in hot paths (use interned strings where possible)

*Lazy Initialization:*
- [x] Dynamic module registry only allocated on first `createModule()` call (implemented)
- [x] Bootstrap context only created on `bootstrap()` call (implemented)
- [x] No global state initialized unless dynamic features used (design implemented)

**Note:** Performance benchmarks require compiler support for `std:reflect`. Core implementation verifications are complete based on code review.

**Performance Targets:**
| Operation | Target |
|-----------|--------|
| Static code with reflect infrastructure | 0% overhead |
| Dynamic function call vs static | < 5% overhead |
| Dynamic class instantiation | < 10% overhead |
| Dynamic method dispatch | < 10% overhead |
| BytecodeBuilder.build() | < 1ms for 1000 instructions |
| Module.seal() | < 1ms for 100 functions |

---

## Files to Create/Modify

```
crates/raya-stdlib/src/          # Native stdlib implementations
├── lib.rs                       # Module exports
└── reflect.rs                   # NEW: std:reflect native module
                                 # - Registers native functions
                                 # - Exports: defineMetadata, getMetadata, getClass, etc.

crates/raya-engine/src/
├── parser/
│   └── ast/                     # Note: DecoratorInfo in AST handled by parser (Milestone 2.11)
├── compiler/
│   ├── reflection.rs            # NEW: Reflection metadata generation
│   ├── codegen/                 # Emit metadata tables
│   ├── mono.rs                  # MODIFY: Track generic origins during monomorphization
│   └── flags.rs                 # Module flags (reflection always on)
├── vm/
│   ├── reflect/                 # Reflection runtime (used by stdlib)
│   │   ├── mod.rs               # DONE: Module exports
│   │   ├── metadata.rs          # DONE: WeakMap metadata storage
│   │   ├── introspection.rs     # DONE: Class/field/method queries
│   │   ├── class_metadata.rs    # DONE: ClassMetadataRegistry
│   │   ├── invocation.rs        # Dynamic invoke/construct
│   │   ├── inspection.rs        # Object inspection & devtools
│   │   ├── proxy.rs             # DONE: Proxy objects
│   │   ├── generic.rs           # NEW: Generic type metadata (Phase 13)
│   │   ├── type_builder.rs      # NEW: Runtime type creation (Phase 14)
│   │   ├── bytecode_builder.rs  # NEW: Dynamic bytecode generation (Phase 15)
│   │   ├── permissions.rs       # NEW: Reflection permissions (Phase 16)
│   │   ├── dynamic_module.rs    # NEW: Runtime module creation (Phase 17)
│   │   └── bootstrap.rs         # NEW: VM bootstrap context (Phase 17)
│   ├── bytecode/
│   │   └── builder.rs           # NEW: BytecodeBuilder for dynamic generation
│   └── vm/
│       ├── task_interpreter.rs  # DONE: Native call handlers (0x0Dxx)
│       ├── shared_state.rs      # DONE: MetadataStore, ClassMetadataRegistry
│       └── handlers/
│           └── reflect.rs       # MODIFY: Add handlers for Phases 13-16
```

---

## Native Call IDs

**Note:** `0x0Cxx` is used by JSON intrinsics, so Reflect uses `0x0Dxx`.

| Range | Category |
|-------|----------|
| 0x0D00-0x0D0F | Metadata (define/get/has/delete) |
| 0x0D10-0x0D1F | Class introspection |
| 0x0D20-0x0D2F | Field access |
| 0x0D30-0x0D3F | Method invocation |
| 0x0D40-0x0D4F | Object creation |
| 0x0D50-0x0D5F | Type utilities (type checks) |
| 0x0D57-0x0D5A | Type utilities (typeOf, isAssignableTo, cast) |
| 0x0D60-0x0D6F | Interface/hierarchy query |
| 0x0D70-0x0D7F | Object inspection (inspect, getObjectId, describe, snapshot, diff) |
| 0x0D80-0x0D8F | Memory analysis (getObjectSize, getRetainedSize, getReferences, etc.) |
| 0x0D90-0x0D9F | Stack introspection (getCallStack, getLocals, getSourceLocation) |
| 0x0DA0-0x0DAF | Serialization helpers (toJSON, getEnumerableKeys, isCircular) |
| 0x0DB0-0x0DBF | Proxy objects |
| 0x0DC0-0x0DCF | Dynamic subclass creation (Phase 10) |
| 0x0DD0-0x0DDF | Generic type metadata (Phase 13) |
| 0x0DE0-0x0DEF | Runtime type creation (Phase 14) |
| 0x0DF0-0x0DFF | Dynamic bytecode generation (Phase 15) |
| 0x0E00-0x0E0F | Reflection permissions (Phase 16) |
| 0x0E10-0x0E2F | Dynamic VM bootstrap (Phase 17) |

---

## Dependencies

- Milestone 3.7 (module system) - for `std:reflect` module
- Class/method/field AST structures (already exist)
- Native call infrastructure (already exists)

---

## Estimated Scope

| Phase | Tasks | Complexity |
|-------|-------|------------|
| Phase 1: Core Types & Metadata | 13 tasks | Medium |
| Phase 2: Class Introspection | 10 tasks | High |
| Phase 3: Field Access | 9 tasks | Medium |
| Phase 4: Method Invocation | 9 tasks | Medium |
| Phase 5: Object Creation | 9 tasks | Medium |
| Phase 6: Type Utilities | 8 tasks | Medium |
| Phase 7: Interface Query | 5 tasks | Medium |
| Phase 8: Object Inspection & DevTools | 21 tasks | High |
| Phase 9: Proxy Objects | 9 tasks | Medium |
| Phase 10: Dynamic Classes | 6 tasks | High |
| Phase 11: Compiler Integration | 9 tasks | High |
| Phase 12: Integration Tests | 8 tasks | Medium |
| Phase 13: Generic Type Metadata | 12 tasks | High |
| Phase 14: Runtime Type Creation | 16 tasks | Very High |
| Phase 15: Dynamic Bytecode Generation | 17 tasks | Very High |
| Phase 16: Reflection Security | 7 tasks | Medium |
| Phase 17: Dynamic VM Bootstrap | 18 tasks | Very High |
| Phase 18: Performance Validation | 13 tasks | High |

**Total:** ~199 tasks

---

## Success Criteria

1. Metadata can be stored/retrieved on any object
2. Class structure queryable at runtime
3. Fields can be read/written dynamically
4. Methods can be invoked dynamically
5. Objects can be created from Class<T> references
6. Object inspection provides comprehensive debugging info
7. Object snapshots enable state tracking and diffing
8. Memory analysis works (heap stats, instance finding, retained size)
9. Stack introspection returns call frames with source locations
10. Proxies intercept field/method access
11. DI container can resolve dependencies using Reflect
12. ORM can map entities using field introspection
13. Performance overhead < 10x for reflection vs direct calls
14. Generic type origins are preserved and queryable after monomorphization
15. New classes can be created at runtime with fields and methods
16. New functions can be created at runtime with bytecode or AST
17. Generic types can be specialized with new type arguments at runtime
18. BytecodeBuilder can construct valid bytecode programmatically
19. Dynamic code integrates with existing module system
20. Reflection permissions can restrict access to sensitive operations
21. Modules can be created entirely at runtime without pre-compiled code
22. Classes and functions can be dynamically added to runtime modules
23. Code can execute from an empty VM using only Reflect API
24. Dynamic classes support inheritance and method dispatch
25. Bootstrap context provides minimal runtime for dynamic execution
26. **Zero overhead**: Static code performance unchanged when dynamic infrastructure present
27. **Near-native dynamic**: Dynamic function calls within 5% of static performance
28. **Lazy loading**: No memory/CPU cost when dynamic features are unused
