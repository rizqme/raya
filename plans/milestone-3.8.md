# Milestone 3.8: Reflection API (std:reflect)

**Status:** In Progress (Phases 1-5 handlers complete, compiler integration pending for --emit-reflection)
**Goal:** Implement comprehensive runtime reflection for introspection, metadata, dynamic invocation, and devtools support

---

## Overview

The Reflect API enables runtime introspection and manipulation of classes, methods, and fields. Key capabilities:
- Metadata storage on any target
- Type introspection (query class structure)
- Dynamic invocation (call methods, get/set fields)
- Object creation (instantiate classes dynamically)
- Object inspection for debugging and devtools
- Proxy objects for interception

**Compiler flag:** `--emit-reflection` includes full type metadata. Without it, only basic features work.

See [design/REFLECTION.md](../design/REFLECTION.md) for full specification.

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
    readonly decorators: DecoratorInfo[];
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
    readonly decorators: DecoratorInfo[];
}

interface TypeInfo {
    readonly kind: "primitive" | "class" | "interface" | "union" | "function" | "array" | "generic";
    readonly name: string;
    readonly classRef: Class<Object> | null;
    readonly unionMembers: TypeInfo[] | null;
    readonly elementType: TypeInfo | null;
    readonly typeArguments: TypeInfo[] | null;
}

interface Modifiers {
    readonly isPublic: boolean;
    readonly isPrivate: boolean;
    readonly isProtected: boolean;
    readonly isStatic: boolean;
    readonly isReadonly: boolean;
    readonly isAbstract: boolean;
}

interface DecoratorInfo {
    readonly name: string;
    readonly decorator: Function;
    readonly args: unknown[];
}

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

Implement basic metadata storage (works without `--emit-reflection`).

**Tasks:**
- [x] Define `Class<T>` interface in builtins
- [ ] Define `FieldInfo`, `MethodInfo`, `ParameterInfo` interfaces
- [ ] Define `TypeInfo`, `Modifiers`, `DecoratorInfo` interfaces
- [x] Implement metadata WeakMap storage in VM
- [x] Implement `Reflect.defineMetadata(key, value, target)`
- [x] Implement `Reflect.defineMetadata(key, value, target, propertyKey)`
- [x] Implement `Reflect.getMetadata<T>(key, target)`
- [x] Implement `Reflect.getMetadata<T>(key, target, propertyKey)`
- [x] Implement `Reflect.hasMetadata(key, target)`
- [x] Implement `Reflect.getMetadataKeys(target)`
- [x] Implement `Reflect.deleteMetadata(key, target)`
- [x] Add native call IDs for Reflect methods
- [x] Add unit tests for metadata storage
- [x] Add MetadataStore to SharedVmState
- [x] Implement native call dispatch in TaskInterpreter

**Function Signatures:**
```typescript
// Metadata operations (work without --emit-reflection)
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

Query class structure at runtime (requires `--emit-reflection` for full metadata).

**Tasks:**
- [x] Implement `Reflect.getClass<T>(obj)` - get class of object (returns class ID)
- [x] Implement `Reflect.getClassByName(name)` - lookup by name
- [x] Implement `Reflect.getAllClasses()` - get all registered classes
- [x] Implement `Reflect.getClassesWithDecorator(decorator)` - filter by decorator (stub - needs decorator metadata)
- [x] Implement `Reflect.isSubclassOf(sub, super)` - check inheritance
- [x] Implement `Reflect.isInstanceOf<T>(obj, cls)` - type guard
- [x] Implement `Reflect.getTypeInfo(target)` - get type info (basic - returns type name)
- [x] Implement `Reflect.getClassHierarchy(obj)` - get inheritance chain
- [x] Add class registry to VM (populated at class definition)
- [x] Add native call IDs and handlers for Phase 2
- [ ] Compiler: emit class metadata when `--emit-reflection`
- [x] Add unit tests for introspection

**Function Signatures:**
```typescript
// Class introspection (requires --emit-reflection for full metadata)
function getClass<T>(obj: T): number;  // Returns class ID
function getClassByName(name: string): number | null;  // Returns class ID
function getAllClasses(): number[];  // Returns array of class IDs
function getClassesWithDecorator<D extends Function>(decorator: D): number[];
function isSubclassOf(subClassId: number, superClassId: number): boolean;
function isInstanceOf(obj: unknown, classId: number): boolean;
function getTypeInfo(target: object): string;  // Returns type name (basic)
function getClassHierarchy(obj: object): number[];  // Returns array of class IDs from obj to root
```

**Compiler Changes:**
- Add `--emit-reflection` flag handling
- Generate `ClassInfo` structures for each class
- Include field types, method signatures, parameter names
- Store decorator applications with arguments

---

### Phase 3: Field Access

Dynamic field get/set operations.

**Tasks:**
- [x] Implement `Reflect.get<T>(target, propertyKey)` - get field value (needs --emit-reflection for name lookup)
- [x] Implement `Reflect.set<T>(target, propertyKey, value)` - set field value (needs --emit-reflection for name lookup)
- [x] Implement `Reflect.has(target, propertyKey)` - check field exists (needs --emit-reflection)
- [x] Implement `Reflect.getFieldNames(target)` - list all fields (needs --emit-reflection)
- [ ] Implement `Reflect.getFieldInfo(target, propertyKey)` - get field metadata
- [ ] Implement `Reflect.getFields(target)` - get all field infos
- [ ] Handle private field access (allowed by default)
- [ ] Handle static fields
- [ ] Add unit tests for field access

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
- [ ] Implement `Reflect.invoke<R>(target, methodName, ...args)` - call method (needs --emit-reflection)
- [ ] Implement `Reflect.invokeAsync<R>(target, methodName, ...args)` - call async (needs --emit-reflection)
- [ ] Implement `Reflect.getMethod<F>(target, methodName)` - get method reference
- [ ] Implement `Reflect.getMethodInfo(target, methodName)` - get metadata
- [ ] Implement `Reflect.getMethods(target)` - list all methods
- [x] Implement `Reflect.hasMethod(target, methodName)` - check exists (needs --emit-reflection)
- [ ] Handle static methods
- [ ] Handle method overloading (if applicable)
- [ ] Add unit tests for invocation

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
- [x] Implement `Reflect.construct<T>(cls, ...args)` - create instance (basic, no constructor call yet)
- [ ] Implement `Reflect.constructWith<T>(cls, params)` - named params (for DI)
- [x] Implement `Reflect.allocate<T>(cls)` - empty instance (uninitialized)
- [x] Implement `Reflect.clone<T>(obj)` - shallow clone
- [ ] Implement `Reflect.deepClone<T>(obj)` - deep clone
- [ ] Implement `Reflect.getConstructorInfo(cls)` - constructor metadata
- [ ] Implement `Reflect.getConstructorParameterTypes(cls)`
- [ ] Implement `Reflect.getConstructorParameterNames(cls)`
- [ ] Add unit tests for object creation

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
- [ ] Implement `Reflect.typeOf(typeName)` - get TypeInfo from string
- [ ] Implement `Reflect.typeOf<T>()` - get TypeInfo from generic
- [ ] Implement `Reflect.isAssignableTo(source, target)` - check compatibility
- [ ] Implement type guards: `isString`, `isNumber`, `isBoolean`, `isNull`
- [ ] Implement type guards: `isArray`, `isFunction`, `isObject`
- [ ] Implement `Reflect.cast<T>(value, cls)` - safe cast (returns null)
- [ ] Implement `Reflect.castOrThrow<T>(value, cls)` - cast or throw
- [ ] Add unit tests for type utilities

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
- [ ] Implement `Reflect.implements(cls, interfaceName)` - check interface
- [ ] Implement `Reflect.getInterfaces(cls)` - list implemented interfaces
- [ ] Implement `Reflect.getImplementors(interfaceName)` - find implementors
- [ ] Implement `Reflect.isStructurallyCompatible(a, b)` - structural check
- [ ] Track interface implementations in class metadata
- [ ] Add unit tests for interface queries

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
- [ ] Implement `Reflect.inspect(obj)` - human-readable object representation
- [ ] Implement `Reflect.snapshot(obj)` - capture object state as `ObjectSnapshot`
- [ ] Implement `Reflect.diff(a, b)` - compare two objects/snapshots
- [ ] Implement `Reflect.getObjectId(obj)` - unique identity for tracking
- [ ] Implement `Reflect.getClassHierarchy(obj)` - inheritance chain from object's class to root
- [ ] Implement `Reflect.describe(cls)` - detailed class description string

*Memory Analysis:*
- [ ] Implement `Reflect.getObjectSize(obj)` - shallow memory footprint
- [ ] Implement `Reflect.getRetainedSize(obj)` - size with retained objects
- [ ] Implement `Reflect.getReferences(obj)` - objects referenced by this object
- [ ] Implement `Reflect.getReferrers(obj)` - objects referencing this (if GC supports)
- [ ] Implement `Reflect.getHeapStats()` - total objects, memory usage by class
- [ ] Implement `Reflect.findInstances<T>(cls)` - all live instances of a class

*Stack/Execution Introspection:*
- [ ] Implement `Reflect.getCallStack()` - current call frames with function names, args
- [ ] Implement `Reflect.getLocals(frame?)` - local variables in current/specified frame
- [ ] Implement `Reflect.getSourceLocation(method)` - file:line:col mapping (requires `--emit-reflection`)

*Serialization Helpers:*
- [ ] Implement `Reflect.toJSON(obj)` - serializable representation
- [ ] Implement `Reflect.getEnumerableKeys(obj)` - keys suitable for iteration
- [ ] Implement `Reflect.isCircular(obj)` - check for circular references

- [ ] Add unit tests for inspection
- [ ] Add unit tests for memory analysis
- [ ] Add unit tests for stack introspection

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
- [ ] Define `ProxyHandler<T>` interface
- [ ] Implement `Reflect.createProxy<T>(target, handler)` - create proxy
- [ ] Implement proxy trap: `get` - intercept property access
- [ ] Implement proxy trap: `set` - intercept property assignment
- [ ] Implement proxy trap: `has` - intercept `in` operator
- [ ] Implement proxy trap: `invoke` - intercept method calls
- [ ] Implement `Reflect.isProxy(obj)` - check if object is a proxy
- [ ] Implement `Reflect.getProxyTarget(proxy)` - get underlying target
- [ ] Add unit tests for proxies

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

**Tasks:**
- [ ] Define `SubclassDefinition<T>` interface
- [ ] Define `FieldDefinition` interface
- [ ] Implement `Reflect.createSubclass<T, S>(superclass, name, definition)`
- [ ] Implement `Reflect.extendWith<T>(cls, fields)` - add fields
- [ ] Generate runtime class structures
- [ ] Register dynamic classes in class registry
- [ ] Add unit tests for dynamic classes

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
- [ ] Add `--emit-reflection` compiler flag
- [ ] Generate class structure metadata
- [ ] Generate field type information
- [ ] Generate method signature metadata
- [ ] Generate parameter name/type information
- [ ] Generate decorator application info
- [ ] Emit compact binary format for metadata
- [ ] Add `--no-reflection-private` flag (block private access)
- [ ] Add integration tests with reflection enabled

**Metadata Format:**
```
ClassMetadata {
    name: String,
    superclass: Option<ClassId>,
    fields: Vec<FieldMetadata>,
    methods: Vec<MethodMetadata>,
    constructors: Vec<ConstructorMetadata>,
    decorators: Vec<DecoratorMetadata>,
}
```

---

### Phase 12: Framework Integration Tests

End-to-end tests with framework patterns.

**Tasks:**
- [ ] Test: Dependency Injection container using Reflect
- [ ] Test: ORM entity mapping with decorators + Reflect
- [ ] Test: HTTP routing framework with controller discovery
- [ ] Test: Validation framework with decorator-based rules
- [ ] Test: Serialization using field introspection
- [ ] Test: Object inspection and diff for state debugging
- [ ] Test: DevTools integration (inspect, snapshot, memory)
- [ ] Performance benchmarks vs direct calls

---

## Files to Create/Modify

```
crates/raya-engine/src/
├── parser/
│   ├── checker/builtins.rs     # Reflect interface definitions
│   └── ast/                     # DecoratorInfo in AST (if needed)
├── compiler/
│   ├── reflection.rs            # NEW: Reflection metadata generation
│   ├── codegen/                 # Emit metadata tables
│   └── flags.rs                 # --emit-reflection flag
├── vm/
│   ├── reflect/                 # NEW: Reflection runtime
│   │   ├── mod.rs
│   │   ├── metadata.rs          # WeakMap metadata storage
│   │   ├── introspection.rs     # Class/field/method queries
│   │   ├── invocation.rs        # Dynamic invoke/construct
│   │   ├── inspection.rs        # Object inspection & devtools
│   │   ├── proxy.rs             # Proxy objects
│   │   └── types.rs             # TypeInfo runtime representation
│   └── vm/interpreter.rs        # Native call handlers
└── builtins/
    └── Reflect.raya             # Reflect class definition
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
| 0x0D50-0x0D5F | Type utilities |
| 0x0D60-0x0D6F | Object inspection (inspect, snapshot, diff) |
| 0x0D70-0x0D7F | Memory analysis (heap, instances, refs) |
| 0x0D80-0x0D8F | Stack introspection (frames, locals, source) |
| 0x0D90-0x0D9F | Proxy objects |
| 0x0DA0-0x0DAF | Dynamic class creation |

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

**Total:** ~116 tasks

---

## Success Criteria

1. Metadata can be stored/retrieved on any object
2. Class structure queryable at runtime (with `--emit-reflection`)
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
