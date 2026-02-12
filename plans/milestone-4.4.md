# Milestone 4.4: std:reflect Module

**Status:** Not Started
**Depends on:** Milestone 3.8 (Reflection API), Milestone 4.2 (stdlib pattern)
**Goal:** Expose the existing Reflect API as `std:reflect` with a Raya source file, enabling `import reflect from "std:reflect"` and end-to-end testing

---

## Overview

The Reflect API is already fully implemented in raya-engine (141+ handlers, Phases 1-17). However, it's currently only accessible via compiler-generated `__NATIVE_CALL` instructions (e.g., from decorator lowering). There is no user-facing `Reflect.raya` source file, no `std:reflect` module registration, and no e2e tests.

This milestone creates the Raya-side interface (`Reflect.raya`) so users can `import reflect from "std:reflect"` and call reflection methods directly, following the same stdlib pattern as `std:logger` and `std:math`.

### Key Architectural Difference

Unlike `std:logger` and `std:math`, reflect handlers **stay in raya-engine** (not raya-stdlib) because they need VM internals:
- `MetadataStore` (WeakMap-style metadata)
- `ClassMetadataRegistry` (field/method info)
- `ClassRegistry` (class inheritance)
- GC heap (object inspection, proxies)
- `Stack` (call stack introspection)
- `DecoratorRegistry` (decorator queries)

```
Reflect.raya (raya-stdlib)  →  __NATIVE_CALL(ID, args)
                                      ↓
                           NativeCall opcode (VM)
                                      ↓
                           is_reflect_method() check (builtin.rs)
                                      ↓
                           call_reflect_method() (handlers/reflect.rs)
                                      ↓
                           vm/reflect/* (engine internals)
```

No `StdNativeHandler` routing needed — the engine handles reflect dispatch directly.

### Usage

```typescript
import reflect from "std:reflect";

// Metadata
reflect.defineMetadata("role", "admin", user);
let role: string = reflect.getMetadata("role", user);

// Introspection
let classId: number = reflect.getClass(user);
let fields: Array<string> = reflect.getFieldNames(user);
let hasName: boolean = reflect.has(user, "name");

// Dynamic access
let name: string = reflect.get(user, "name");
reflect.set(user, "name", "Alice");

// Type guards
let isStr: boolean = reflect.isString(value);
let isNum: boolean = reflect.isNumber(value);
```

---

## API Surface

### Tier 1: Core Reflection (Phase 1 scope)

The most commonly used methods — metadata, introspection, field access, type guards.

| Method | Signature | Native ID |
|--------|-----------|-----------|
| `defineMetadata` | `<T>(key: string, value: T, target: Object): void` | 0x0D00 |
| `defineMetadataProp` | `<T>(key: string, value: T, target: Object, prop: string): void` | 0x0D01 |
| `getMetadata` | `<T>(key: string, target: Object): T` | 0x0D02 |
| `getMetadataProp` | `<T>(key: string, target: Object, prop: string): T` | 0x0D03 |
| `hasMetadata` | `(key: string, target: Object): boolean` | 0x0D04 |
| `hasMetadataProp` | `(key: string, target: Object, prop: string): boolean` | 0x0D05 |
| `deleteMetadata` | `(key: string, target: Object): boolean` | 0x0D08 |
| `getClass` | `<T>(obj: T): Class<T>` | 0x0D10 |
| `getClassByName` | `(name: string): Class<Object>` | 0x0D11 |
| `getAllClasses` | `(): Array<Class<Object>>` | 0x0D12 |
| `isSubclassOf` | `(sub: Class<Object>, sup: Class<Object>): boolean` | 0x0D14 |
| `isInstanceOf` | `<T>(obj: Object, cls: Class<T>): boolean` | 0x0D15 |
| `get` | `<T>(target: Object, key: string): T` | 0x0D20 |
| `set` | `<T>(target: Object, key: string, value: T): void` | 0x0D21 |
| `has` | `(target: Object, key: string): boolean` | 0x0D22 |
| `getFieldNames` | `(target: Object): Array<string>` | 0x0D23 |
| `isString` | `(value: Object): boolean` | 0x0D50 |
| `isNumber` | `(value: Object): boolean` | 0x0D51 |
| `isBoolean` | `(value: Object): boolean` | 0x0D52 |
| `isNull` | `(value: Object): boolean` | 0x0D53 |
| `isArray` | `(value: Object): boolean` | 0x0D54 |
| `isObject` | `(value: Object): boolean` | 0x0D56 |

### Tier 2: Method & Object Operations (Phase 2 scope)

Dynamic method invocation, object creation, inspection.

| Method | Signature | Native ID |
|--------|-----------|-----------|
| `invoke` | `<R>(target: Object, method: string, ...args: Array<Object>): R` | 0x0D30 |
| `getMethodInfo` | `(target: Object, method: string): MethodInfo` | 0x0D33 |
| `getMethods` | `(target: Object): Array<MethodInfo>` | 0x0D34 |
| `hasMethod` | `(target: Object, method: string): boolean` | 0x0D35 |
| `construct` | `<T>(cls: Class<T>, ...args: Array<Object>): T` | 0x0D40 |
| `allocate` | `<T>(cls: Class<T>): T` | 0x0D42 |
| `clone` | `<T>(obj: T): T` | 0x0D43 |
| `inspect` | `(obj: Object): string` | 0x0D70 |
| `getObjectId` | `(obj: Object): number` | 0x0D71 |
| `describe` | `<T>(cls: Class<T>): string` | 0x0D72 |

### Tier 3: Advanced (Phase 3 scope)

Proxies, dynamic classes, permissions, decorator queries, hierarchy.

| Method | Signature | Native ID |
|--------|-----------|-----------|
| `createProxy` | `<T>(target: T, handler: ProxyHandler<T>): T` | 0x0DB0 |
| `isProxy` | `(obj: Object): boolean` | 0x0DB1 |
| `getProxyTarget` | `<T>(proxy: T): T` | 0x0DB2 |
| `createSubclass` | `<T>(parent: Class<T>, name: string, def: SubclassDefinition<T>): Class<T>` | 0x0DC0 |
| `implements` | `<T>(cls: Class<T>, iface: string): boolean` | 0x0D60 |
| `getInterfaces` | `<T>(cls: Class<T>): Array<string>` | 0x0D61 |
| `getSuperclass` | `<T>(cls: Class<T>): Class<Object>` | 0x0D62 |
| `getClassDecorators` | `(cls: Class<Object>): Array<string>` | 0x0D1C |
| `getMethodDecorators` | `(cls: Class<Object>, method: string): Array<string>` | 0x0D1D |
| `setPermissions` | `(target: Object, perms: ReflectionPermission): void` | 0x0E00 |
| `getPermissions` | `(target: Object): ReflectionPermission` | 0x0E01 |
| `hasPermission` | `(target: Object, perm: ReflectionPermission): boolean` | 0x0E02 |

---

## Phases

### Phase 1: Reflect.raya & Registration ⬜

**Status:** Not Started

**Tasks:**
- [ ] Create `crates/raya-stdlib/Reflect.raya`
  - [ ] Define native call ID constants for all reflect methods (0x0D00-0x0E28)
  - [ ] Define `Reflect` class with methods wrapping `__NATIVE_CALL`
  - [ ] Start with Tier 1 methods (metadata, introspection, field access, type guards)
  - [ ] Add Tier 2 methods (method invocation, object creation, inspection)
  - [ ] Add Tier 3 methods (proxies, hierarchy, permissions, decorators)
  - [ ] `const reflect = new Reflect(); export default reflect;` (lowercase singleton)
- [ ] Register `Reflect.raya` in `std_modules.rs`
- [ ] Fix `is_reflect_method()` in `builtin.rs` to cover full range `0x0D00..=0x0E2F`
  - Currently only covers `0x0D00..=0x0DFF`, missing Phase 16-17 handlers (0x0E00-0x0E28)

**Type Signature Notes:**
- Methods use proper generic signatures: `defineMetadata<T>(key: string, value: T, target: Object): void`
- `Object` is the base type for arbitrary object references
- Generics are monomorphized at compile time — `__NATIVE_CALL` handles raw Values at runtime
- `Class<T>`, `MethodInfo`, `FieldInfo`, `ProxyHandler<T>` etc. are type interfaces defined in the Reflect module

**Files:**
- `crates/raya-stdlib/Reflect.raya` (new)
- `crates/raya-engine/src/compiler/module/std_modules.rs`
- `crates/raya-engine/src/vm/builtin.rs` (fix `is_reflect_method` range)

---

### Phase 2: Test Harness Updates ⬜

**Status:** Not Started

**Tasks:**
- [ ] Update `get_std_sources()` in test harness to include `Reflect.raya`
- [ ] Verify reflect dispatch works through the `__NATIVE_CALL` → `is_reflect_method` → `call_reflect_method` path
- [ ] Ensure `import reflect from "std:reflect"` syntax works in e2e tests

**Files:**
- `crates/raya-runtime/tests/e2e/harness.rs`

---

### Phase 3: Tier 1 E2E Tests (Metadata & Introspection) ⬜

**Status:** Not Started

**Tasks:**
- [ ] Create `crates/raya-runtime/tests/e2e/reflect.rs`
- [ ] Register `mod reflect;` in `crates/raya-runtime/tests/e2e/mod.rs`
- [ ] Metadata tests:
  - [ ] `test_reflect_define_and_get_metadata` — define metadata, retrieve it
  - [ ] `test_reflect_has_metadata` — check metadata existence
  - [ ] `test_reflect_delete_metadata` — delete metadata
  - [ ] `test_reflect_metadata_on_property` — property-level metadata
- [ ] Type guard tests:
  - [ ] `test_reflect_is_string` — string type check
  - [ ] `test_reflect_is_number` — number type check
  - [ ] `test_reflect_is_boolean` — boolean type check
  - [ ] `test_reflect_is_null` — null type check
  - [ ] `test_reflect_is_array` — array type check
  - [ ] `test_reflect_is_object` — object type check
- [ ] Field access tests:
  - [ ] `test_reflect_get_set_field` — dynamic field get/set
  - [ ] `test_reflect_has_field` — field existence check
  - [ ] `test_reflect_get_field_names` — list fields
- [ ] Class introspection tests:
  - [ ] `test_reflect_get_class` — get class of object
  - [ ] `test_reflect_is_instance_of` — type guard
  - [ ] `test_reflect_import` — verify import syntax works

**Files:**
- `crates/raya-runtime/tests/e2e/reflect.rs` (new)
- `crates/raya-runtime/tests/e2e/mod.rs`

---

### Phase 4: Tier 2 E2E Tests (Methods & Objects) ⬜

**Status:** Not Started

**Tasks:**
- [ ] Method invocation tests:
  - [ ] `test_reflect_invoke` — call method by name
  - [ ] `test_reflect_has_method` — method existence
  - [ ] `test_reflect_get_methods` — list methods
- [ ] Object creation tests:
  - [ ] `test_reflect_construct` — dynamic instantiation
  - [ ] `test_reflect_clone` — shallow clone
  - [ ] `test_reflect_allocate` — uninitialized allocation
- [ ] Inspection tests:
  - [ ] `test_reflect_inspect` — human-readable object string
  - [ ] `test_reflect_get_object_id` — unique identity
  - [ ] `test_reflect_describe` — class description

**Files:**
- `crates/raya-runtime/tests/e2e/reflect.rs`

---

### Phase 5: Tier 3 E2E Tests (Proxies, Hierarchy, Permissions) ⬜

**Status:** Not Started

**Tasks:**
- [ ] Proxy tests:
  - [ ] `test_reflect_create_proxy` — create proxy with traps
  - [ ] `test_reflect_is_proxy` — proxy detection
  - [ ] `test_reflect_proxy_get_trap` — intercept property read
- [ ] Hierarchy tests:
  - [ ] `test_reflect_get_superclass` — parent class
  - [ ] `test_reflect_is_subclass_of` — inheritance check
  - [ ] `test_reflect_implements` — interface check
- [ ] Permission tests:
  - [ ] `test_reflect_set_get_permissions` — permission management
  - [ ] `test_reflect_has_permission` — permission check
- [ ] Decorator query tests:
  - [ ] `test_reflect_get_class_decorators` — list decorators on class
  - [ ] `test_reflect_get_method_decorators` — list decorators on method

**Files:**
- `crates/raya-runtime/tests/e2e/reflect.rs`

---

### Phase 6: Documentation ⬜

**Status:** Not Started

**Tasks:**
- [ ] Update `design/STDLIB.md` with `std:reflect` module documentation
- [ ] Update `CLAUDE.md` with milestone 4.4 status
- [ ] Update `plans/PLAN.md` with 4.4 status

**Files:**
- `design/STDLIB.md`
- `CLAUDE.md`
- `plans/PLAN.md`

---

## Native ID Ranges

| Range | Category | Phase |
|-------|----------|-------|
| 0x0D00-0x0D09 | Metadata operations | 1 |
| 0x0D10-0x0D1F | Class introspection + decorators | 2-3 |
| 0x0D20-0x0D27 | Field access | 3 |
| 0x0D30-0x0D37 | Method invocation | 4 |
| 0x0D40-0x0D45 | Object creation | 5 |
| 0x0D50-0x0D5A | Type utilities | 6 |
| 0x0D60-0x0D65 | Interface/hierarchy | 7 |
| 0x0D70-0x0D74 | Object inspection | 8 |
| 0x0D80-0x0D85 | Memory analysis | 8 |
| 0x0D90-0x0D92 | Stack introspection | 8 |
| 0x0DA0-0x0DA2 | Serialization | 8 |
| 0x0DB0-0x0DB3 | Proxy objects | 9 |
| 0x0DC0-0x0DC4 | Dynamic subclass | 10 |
| 0x0DD0-0x0DD5 | Generic metadata | 13 |
| 0x0DE0-0x0DEE | Runtime type creation | 14 |
| 0x0DF0-0x0DFE | Bytecode builder | 15 |
| 0x0E00-0x0E0D | Permissions | 16 |
| 0x0E10-0x0E28 | Bootstrap/execution | 17 |

**Bug:** `is_reflect_method()` currently checks `0x0D00..=0x0DFF` but reflect IDs extend to `0x0E28`. Phase 1 must fix this to `0x0D00..=0x0E2F`.

---

## Key Differences from std:logger / std:math

| Aspect | std:logger / std:math | std:reflect |
|--------|----------------------|-------------|
| Handler location | raya-stdlib (Rust) | raya-engine (handlers/reflect.rs) |
| NativeHandler trait | Yes (StdNativeHandler) | No (direct VM dispatch) |
| Args format | `&[String]` | `Vec<Value>` (raw NaN-boxed values) |
| VM state needed | No | Yes (metadata, classes, GC, stack) |
| raya-stdlib module | `pub mod logger/math` | Not needed |
| StdNativeHandler arms | Yes | Not needed |
| Raya source file | Logger.raya / Math.raya | Reflect.raya (same pattern) |
| std_modules registry | Yes | Yes |

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/raya-stdlib/Reflect.raya` | Raya source: Reflect class + `__NATIVE_CALL` + `export default reflect` |
| `crates/raya-engine/src/compiler/module/std_modules.rs` | Register `Reflect.raya` in std module registry |
| `crates/raya-engine/src/vm/builtin.rs` | Fix `is_reflect_method()` range, reflect ID constants |
| `crates/raya-engine/src/vm/vm/handlers/reflect.rs` | All reflect handler implementations (4,730 lines) |
| `crates/raya-engine/src/vm/reflect/` | Reflect runtime modules (metadata, proxy, bytecode, etc.) |
| `crates/raya-runtime/tests/e2e/harness.rs` | `get_std_sources()` includes `Reflect.raya` |
| `crates/raya-runtime/tests/e2e/reflect.rs` | E2E tests |
| `design/STDLIB.md` | API specification |
