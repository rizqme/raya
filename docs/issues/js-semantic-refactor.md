# Unified Object Model Refactor

## Core Insight

Raya, TypeScript, and JavaScript are not three different runtime models. They are
one object model with different levels of compiler knowledge:

- **JS mode**: the compiler knows almost nothing — property access is always dynamic.
- **TS mode**: the compiler knows types but they're erasable — most access is dynamic,
  some can be proven safe for direct slot dispatch.
- **Raya strict mode**: the compiler knows exact types — most access can be lowered
  to direct slot reads, enabling fast-path interpretation, JIT, and AOT.

The interpreter does not care which mode compiled the code. There is one property
access path. The compiler decides how much of that path to bypass based on what it
can prove.

**Strict mode is not different semantics. It is more optimization opportunity.**

---

## Root Cause (Revised)

The engine currently treats JS-mode and Raya-mode as different runtime mechanisms:
separate opcode choices in the lowerer, separate handler branches in the interpreter,
and separate metadata models on objects. This causes:

1. **Two property models** that don't agree — `fields[]` for "Raya objects" and
   `dyn_map` + metadata bags for "JS objects".
2. **Compiler choosing semantics** — `expr.rs` decides between `LoadFieldShape`
   (static) and `DynGetKeyed` (dynamic) based on type facts, but these opcodes have
   different runtime semantics, not just different performance.
3. **Duplicated dispatch** — four callable types, three member-access paths, two
   prototype chain implementations.

The fix: one object model, one property access semantic, and compiler-controlled
fast paths.

---

## Architecture

### One Object, Always

Every heap object carries:

```
Object {
    header:    ObjectHeader    // identity, layout, flags
    fields:    Vec<Value>      // fixed-slot values (indexed by layout)
    slot_meta: SlotMetaTable   // descriptor metadata parallel to fields (Arc COW)
    dyn_props: Option<DynProps> // dynamic properties with full descriptors
    prototype: Value           // [[Prototype]] chain link
}
```

This is the same whether the object was created by Raya, TypeScript, or JavaScript
source. A Raya `class Point { x: number; y: number }` and a JS `{ x: 1, y: 2 }`
produce the same `Object` with the same `slot_meta`.

### One Property Access Semantic

`DynGetKeyed` is the **universal property access opcode**. Its handler implements
the same lookup algorithm for all objects regardless of source language:

```
fn get_property(obj, key) -> Value:
    // 1. Shape lookup: key → slot index via the object's layout
    if let Some(slot_idx) = shape_resolve(obj.layout_id, key):
        let meta = obj.slot_meta[slot_idx]
        if meta.accessor.is_some():
            return call_getter(meta.accessor.get, obj)
        return obj.fields[slot_idx]

    // 2. Dynamic fallback: key not in shape → check dyn_props
    if let Some(prop) = obj.dyn_props.get(key):
        if prop.is_accessor:
            return call_getter(prop.get, obj)
        return prop.value

    // 3. Prototype chain: walk obj.prototype, repeat from step 1
    if obj.prototype is not null:
        return get_property(obj.prototype, key)

    // 4. Not found
    return undefined
```

Every object has a shape (layout) that maps known property names to fixed slot
indices. The shape system and shape adapters are retained — they are the
performance backbone. Properties that don't exist in the shape (added dynamically
via `Object.defineProperty`, bracket assignment to new keys, etc.) fall through
to `dyn_props`.

This is the ES [[Get]]/[[Set]] algorithm. The shape lookup at step 1 is the fast
path. When the compiler knows the shape at compile time, it emits `LoadFieldShape`
or `LoadFieldExact` to skip the runtime key→slot resolution entirely.

### Compiler Fast Paths (Not Different Semantics)

When the compiler can prove type safety, it skips the runtime key→slot resolution:

- `LoadFieldExact { slot }` — direct `fields[slot]` read. Emitted when the compiler
  knows the exact class and slot index. Skips shape lookup, descriptor check, and
  prototype walk. **This is what strict Raya mode uses almost everywhere.**
- `LoadFieldShape { shape_id, slot }` — shape-adapter-mediated `fields[actual_slot]`
  read. Emitted when the compiler knows the structural shape but not the exact
  class. The shape adapter maps the expected slot to the actual slot at the first
  access, then caches the mapping.
- `StoreFieldExact` / `StoreFieldShape` — same for writes.
- `CallMethodExact` / `CallMethodShape` — same for method dispatch.

These are **the same semantics as `DynGetKeyed`, just with the key→slot resolution
done at compile time instead of runtime.** A JS file could emit `LoadFieldExact`
if the compiler can prove the access is safe. A Raya file emits `DynGetKeyed`
when the type is `any` or `unknown`.

The shape adapter cache (`StructuralAdapterKey → ShapeAdapter`) is the bridge:
at compile time, `LoadFieldShape` encodes the expected shape and slot; at runtime,
the adapter maps this to the actual object's layout. `DynGetKeyed` does the same
mapping but starts from the string key instead of a pre-resolved slot.

### Strict Mode = AOT/JIT Interpretability

Raya strict mode gives the compiler maximum type information. This means:

- More `LoadFieldExact` / `StoreFieldExact` (direct slot access, zero overhead)
- More `CallMethodExact` (direct vtable dispatch)
- AOT and JIT can assume slot stability — no `Object.defineProperty` can change
  the layout at runtime
- Inline caches are trivially monomorphic

This is not a different object model. It is the same model with more proven fast
paths.

---

## What's Already Done (Phase 1)

The property kernel is implemented:

- `SlotMetaTable` (Arc COW) parallel to `Object.fields`
- `DynProps` replaces `dyn_map` with full descriptor attributes
- `Object.prototype` explicit slot
- Kernel methods: `js_get_own_dyn`, `js_get_own_slot`, `js_define_own_dyn`,
  `js_define_own_slot`, `js_delete_dyn`, `js_own_dyn_keys`
- `Class.slot_meta_template` shared across instances

---

## Phase 2: Wire DynGetKeyed/DynSetKeyed to the Property Kernel

**Goal:** The existing `DynGetKeyed` and `DynSetKeyed` handlers become the canonical
property access path, using the Phase 1 property kernel instead of ad-hoc field
lookups and metadata bags.

### 2.1 DynGetKeyed handler rewrite

Current: classifies value type (JSView::Str, Arr, Struct, other), then does
ad-hoc field lookup, `get_property_value_via_js_semantics_with_context`, and
`builtin_handle_native_method_id` as separate branches.

New: the shape system drives the fast path, `dyn_props` is the fallback:

```
fn exec_dyn_get(obj, key) -> Value:
    // 0. Exotic fast path (Array index, String char, TypedArray element)
    if let Some(v) = exotic_fast_path(obj, key):
        return v

    // 1. Shape lookup: resolve key → slot index via layout field names
    //    This is the same resolution that LoadFieldShape does at runtime,
    //    but starting from the string key instead of a pre-resolved slot.
    if let Some(slot_idx) = shape_resolve_key(obj.header.layout_id, key):
        let meta = obj.slot_meta.get(slot_idx)
        if meta.accessor.is_some():
            return call_getter(meta.accessor.get, obj)
        return obj.fields[slot_idx]

    // 2. Dynamic fallback: key not in shape → check dyn_props
    if let Some(prop) = obj.dyn_props.get(key):
        if prop.is_accessor:
            return call_getter(prop.get, obj)
        return prop.value

    // 3. Prototype chain: walk obj.prototype, repeat from step 1
    if obj.prototype is not null:
        return exec_dyn_get(obj.prototype, key)

    // 4. Not found
    return undefined
```

`shape_resolve_key` uses the existing `RuntimeLayoutRegistry` to map
`(layout_id, key_name) → slot_index`. This is the same data that
`ShapeAdapter` uses, just queried by string key instead of pre-resolved index.

The existing `get_property_value_via_js_semantics_with_context`,
`try_proxy_like_get_property`, `get_field_value_by_name`, and
`builtin_handle_native_method_id` are consolidated into this single path.

### 2.2 DynSetKeyed handler rewrite

Same structure: own property check (writable? accessor setter?), then prototype
chain for inherited setters, then create own property if extensible.

### 2.3 Array/String/TypedArray special cases

For now, the type classification (JSView::Arr, JSView::Str) remains as a fast-path
**before** the property kernel, not instead of it:

```
fn exec_dyn_get(obj, key) -> Value:
    // Fast path: integer index on Array
    if obj is Array and key is integer index:
        return array_get_element(obj, index)
    // Fast path: length on Array
    if obj is Array and key == "length":
        return array_length(obj)
    // Fast path: string character index
    if obj is String and key is integer index:
        return string_char_at(obj, index)
    // General property kernel path
    ...
```

These fast paths will be replaced by exotic backends in Phase 4, but they work
now and don't break the model.

### 2.4 Invariant

After Phase 2: `DynGetKeyed` and `DynSetKeyed` go through the property kernel
for all non-fast-path cases. Descriptor attributes (writable, enumerable,
configurable, accessor) are respected. Prototype chain works via
`Object.prototype`. The handler works identically regardless of whether the code
was compiled from Raya, TypeScript, or JavaScript.

---

## Phase 3: Unified Callable

**Goal:** One heap type for all function values. `name`, `length`, `prototype`,
`bind`, `call`, `apply` are ordinary property accesses through the property kernel.

### 3.1 CallableObject

```rust
pub enum CallableKind {
    UserDefined { func_id: usize, module: Option<Arc<Module>> },
    Bound { target: Value, this_arg: Value, bound_args: Vec<Value> },
    Native { native_id: u32 },
    Class { constructor_func_id: usize, module: Option<Arc<Module>> },
}

pub struct CallableObject {
    pub kind:      CallableKind,
    pub captures:  Vec<Value>,
    pub fields:    Vec<Value>,       // [name, length, prototype] as fixed slots
    pub slot_meta: SlotMetaTable,    // same COW mechanism as Object
    pub dyn_props: Option<Box<DynProps>>,
    pub prototype: Value,            // [[Prototype]] = Function.prototype
}
```

### 3.2 Migration

- `Closure` → `CallableObject { kind: UserDefined }`
- `BoundMethod` → `CallableObject { kind: UserDefined }` with receiver in captures
- `BoundNativeMethod` → `CallableObject { kind: Native }` with receiver in captures
- `BoundFunction` → `CallableObject { kind: Bound }`

### 3.3 Property access on callables

`DynGetKeyed` detects `CallableObject` heap tag and routes to the same property
kernel. `name`, `length`, `prototype` are just `fields[0]`, `fields[1]`,
`fields[2]` with appropriate `slot_meta` descriptors.

---

## Phase 4: Exotic Object Backends

**Goal:** Array, TypedArray, and StringObject exotic behaviors are implemented as
dispatch hooks in the property kernel, not as special cases scattered through
handlers.

### 4.1 ExoticKind tag

```rust
pub enum ExoticKind { None, Array, TypedArray, StringObject }
```

Stored in `ObjectHeader`. The `DynGetKeyed`/`DynSetKeyed` handlers check
`exotic_kind` before falling through to the ordinary property kernel.

### 4.2 Array exotic

`[[DefineOwnProperty]]` for integer indices: write to `Array.elements`, update
length. `"length"` write: truncate elements. All other keys: ordinary kernel.

### 4.3 TypedArray exotic

Integer index: read/write through `TypedArrayRecord` buffer with type coercion.
Non-integer keys: ordinary kernel.

### 4.4 StringObject exotic

Integer index within string length: return character. All other keys: ordinary
kernel.

### 4.5 Out of scope

Proxy — separate plan after Phase 4 is stable.

---

## Phase 5: Runtime Intrinsics for Builtins

**Goal:** `.raya` builtin source calls native intrinsics for internal-slot
operations instead of emulating them as JS objects.

### 5.1 Typed array intrinsics

```
__typed_array_create(kind, length) -> ObjectRef
__typed_array_get_element(obj, index) -> Value
__typed_array_set_element(obj, index, value)
```

### 5.2 Iterator intrinsics

```
__iterator_create(iterable) -> IteratorRecord
__iterator_step(record) -> Option<Value>
__iterator_close(record, abrupt: bool)
```

### 5.3 Descriptor intrinsics

```
__descriptor_from_object(obj) -> DescriptorRecord
__descriptor_to_object(record) -> ObjectRef
__define_own_property(target, key, record) -> bool
```

---

## Phase 6: Clean Builtin Source

Remove witness bags, descriptor emulation objects, and internal-slot emulation
from `.raya` source. Builtins express spec algorithms using Phase 5 intrinsics.

---

## Cleanup: Remove Js* Opcodes

The 9 `Js*` opcodes (`JsGetNamed`, `JsSetNamed`, `JsGet`, `JsSet`, `JsDefineOwn`,
`JsDelete`, `JsHas`, `JsCall`, `JsConstruct`) added in the previous iteration are
**removed**. They were based on a dual-path model that this plan rejects.

The unified approach uses:
- `DynGetKeyed` / `DynSetKeyed` — universal property access (wired to kernel)
- `LoadFieldExact` / `StoreFieldExact` — compiler fast path (strict mode)
- `LoadFieldShape` / `StoreFieldShape` — compiler fast path (structural)
- `CallMethodExact` / `CallMethodShape` — compiler fast path (method dispatch)

No new opcodes are needed.

---

## Execution Order

| Phase | Name | Prerequisites |
|-------|------|---------------|
| 1 | Property kernel on every object | — (DONE) |
| 2 | Wire DynGetKeyed/DynSetKeyed to kernel | Phase 1 |
| 3 | Unified callable | Phase 2 |
| 4 | Exotic backends (Array/TypedArray/String) | Phase 2 |
| 5 | Runtime intrinsics for builtins | Phase 3 |
| 6 | Clean builtin source | Phase 4, Phase 5 |
| cleanup | Remove Js* opcodes | Phase 2 |

Phase 3 and Phase 4 are independent and can proceed in parallel.

---

## Success Criteria

### After Phase 2

- `raya eval --mode js` property access works correctly (no regression).
- `Object.defineProperty` / `Object.getOwnPropertyDescriptor` respect descriptor
  attributes through the property kernel.
- Prototype chain walks work for all object types.
- Raya strict mode still emits `LoadFieldExact` and runs at full speed.

### After Phase 3

- `Function.name`, `Function.length`, `Function.prototype` descriptor tests pass.
- `Function.prototype.bind/call/apply` work through the property kernel.

### After Phase 4

- Array length truncation, integer-index exotic behavior works.
- TypedArray buffer access works through exotic backend.

### Invariant across all phases

- `cargo test -p raya-engine` passes. No regressions in any mode.
- Raya strict mode performance unchanged — `LoadFieldExact` paths untouched.
- JIT/AOT integration tests remain green.
