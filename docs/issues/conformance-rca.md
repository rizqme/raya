# Test262 Conformance — Root Cause Analysis & Fix Plan

## Current State (after unified object model)

- Prototype methods work: `typeof Object.prototype.hasOwnProperty` → "function"
- One heap type: Object + optional callable data
- Property kernel: shape → dyn_props → prototype chain
- Descriptors: writable/enumerable/configurable in SlotMeta/DynProp

## The Three Foundational Flaws

### Flaw 1: Missing builtins (~40% of failures)

Many ES standard builtins are simply not implemented:

**Object:** `keys`, `assign`, `freeze`, `seal`, `create`, `entries`, `values`,
`fromEntries`, `isFrozen`, `isSealed`, `hasOwn`

**Array:** `from`, `of`, `isArray` (static); `find`, `findIndex`, `flat`,
`flatMap`, `every`, `some`, `reduce`, `reduceRight`, `fill`, `copyWithin`,
`entries`, `keys`, `values`, `at` (prototype)

**String:** `raw`, `fromCodePoint` (static); `padStart`, `padEnd`, `repeat`,
`startsWith`, `endsWith`, `trimStart`, `trimEnd`, `matchAll`, `replaceAll`,
`at` (prototype)

**Number:** `isInteger`, `isSafeInteger`, `isFinite`, `isNaN`, `parseFloat`,
`parseInt` (static)

**Math:** Many methods may be present but return wrong values for edge cases
(NaN, -0, Infinity).

These are the highest-impact gap. Each method is straightforward to implement
as either:
- A native handler (Rust function with native_id)
- A .raya builtin source method

### Flaw 2: Thrown errors are strings, not constructor instances (~30%)

`VmError::TypeError("msg")` is a Rust string. Test harness does:
```js
assert.throws(TypeError, fn);
```
Which checks `error instanceof TypeError`. Fails because error is a string.

**Fix:** New `VmError::ThrownValue(Value)` variant. When throwing TypeError,
allocate an error object via `alloc_builtin_error_value("TypeError", msg)`.

### Flaw 3: Static method access as values (~20%)

Even for methods that DO exist (`Object.defineProperty`, `Object.getOwnPropertyNames`
etc.), `typeof Object.defineProperty` → "undefined" because the method isn't stored
as a runtime property on the constructor.

`materialize_constructor_static_method` finds them but the result isn't cached
in dyn_props, so DynGetKeyed doesn't see them.

**Fix:** In `materialize_constructor_static_method`, after creating the closure,
store it in the constructor's `dyn_props`. Then subsequent accesses find it.

## Priority Execution Order

### Phase A: Cache materialized static methods in dyn_props

**Impact:** Makes existing static methods (defineProperty, getOwnPropertyNames,
getPrototypeOf, setPrototypeOf, isExtensible, preventExtensions, is) accessible
as first-class values.

**Where:** `materialize_constructor_static_method` in native.rs.
**How:** After creating the bound closure, store it in the constructor's dyn_props.

### Phase B: Implement missing Object builtins

Add to `builtins/node_compat/object.raya`:

```raya
static keys(obj: any): string[] {
    var names = Object.getOwnPropertyNames(obj);
    var result: string[] = [];
    for (var i = 0; i < names.length; i = i + 1) {
        var desc = Object.getOwnPropertyDescriptor(obj, names[i]);
        if (desc != null && desc["enumerable"] == true) {
            result.push(names[i]);
        }
    }
    return result;
}

static assign(target: any, ...sources: any[]): any {
    for (var i = 0; i < sources.length; i = i + 1) {
        var source = sources[i];
        if (source != null) {
            var keys = Object.keys(source);
            for (var j = 0; j < keys.length; j = j + 1) {
                target[keys[j]] = source[keys[j]];
            }
        }
    }
    return target;
}

static freeze(obj: any): any { ... }
static seal(obj: any): any { ... }
static isFrozen(obj: any): boolean { ... }
static isSealed(obj: any): boolean { ... }
static create(proto: any, props: any): any { ... }
static entries(obj: any): any[][] { ... }
static values(obj: any): any[] { ... }
static fromEntries(iterable: any): any { ... }
static hasOwn(obj: any, key: any): boolean { ... }
```

### Phase C: Error objects as constructor instances

Add `VmError::ThrownValue(Value)` variant. Modify error-throwing paths to
allocate actual error objects. Modify catch blocks to extract Value.

### Phase D: ToString coercion for special values

Fix `Infinity`, `-Infinity`, `NaN`, `undefined`, `null` string representations.

### Phase E: Implement missing Array/String/Number builtins

Add methods as needed based on test262 failure clusters.

## Success Criteria

After Phase A: `typeof Object.defineProperty === "function"` ✓
After Phase B: `Object.keys({a:1})` returns `["a"]` ✓
After Phase A+B: Object category improves from 0% to ~30%
After Phase C: assert.throws works → all categories improve ~20%
After all phases: Overall test262 pass rate ~40-50%

## Foundational Engine Architecture (Done)

The following architectural work is COMPLETE and should not be revisited:

- One heap type (Object with optional callable)
- Property kernel (SlotMeta + DynProps + prototype)
- Shape system retained for fast-path slot access
- Prototype chain with nominal_type_id for vtable method lookup
- Prototype layouts don't shadow vtable methods
- defineProperty/getOwnPropertyDescriptor through kernel
- Extensibility via OBJECT_FLAG_NOT_EXTENSIBLE
