# Test262 Conformance Root Cause Analysis

## Failure Clusters (from first 200 tests)

1. **`Function.prototype.call target is not callable`** (~70% of failures)
   Root cause: builtin static methods (Object.keys, Object.defineProperty, etc.)
   are not installed as properties on constructor values at runtime. The compiler
   resolves them statically, but `DynGetKeyed` can't find them. Test harness
   functions like `assert.throws` also fail because they try to call functions
   through dynamic dispatch.

2. **`Expected function to throw the requested constructor`** (~15%)
   Root cause: errors thrown by the runtime are plain RuntimeError/TypeError
   strings, not actual `TypeError` constructor instances. `assert.throws(TypeError, fn)`
   checks `instanceof TypeError` which fails.

3. **`in` operator not supported** (~5%)
   Root cause: parser doesn't support `key in obj` expression syntax.

4. **Property accessor/descriptor failures** (~10%)
   Root cause: `hasOwnProperty`, `propertyIsEnumerable`, `toString` etc. not
   found on objects because Object.prototype methods are not in the prototype
   chain (Object.prototype is null on newly created objects).

## Root Causes (Architectural)

### A. Builtin constructors don't have static methods as properties

When JS code does `Object.keys(obj)`, the compiler resolves `Object.keys` as
a static method call and emits a NativeCall directly. But when code does
`var f = Object.keys; f(obj)`, the `Object.keys` access goes through DynGetKeyed
which can't find `keys` because:
- Object (the constructor value) doesn't have `keys` in its dyn_props
- The shape has no slot for `keys`
- The prototype chain walk finds nothing

Fix: at runtime initialization, install static methods as DynProp entries on
builtin constructor values (Object, Array, String, Number, etc.).

### B. Object.prototype not set on new objects

When `{}` is created via ObjectLiteral, `Object.prototype` is `Value::null()`.
So `obj.hasOwnProperty`, `obj.toString`, `obj.valueOf` etc. can't be found
through prototype chain walk. Every JS object needs `[[Prototype]] = Object.prototype`.

Fix: when ObjectLiteral creates an object, set `obj.prototype` to the runtime's
registered `Object.prototype` value.

### C. Errors are not constructor instances

`throw new TypeError(message)` in the engine creates a string error, not a
TypeError instance. `assert.throws(TypeError, fn)` checks `instanceof TypeError`
which fails.

Fix: runtime errors should be actual constructor instances with proper prototype
chain.

### D. `in` operator parsing

Parser doesn't support `key in obj`. This is a parser-level fix.

## Priority Order

1. **B (Object.prototype on new objects)** — highest impact, fixes all inherited
   method resolution. Unblocks hasOwnProperty, toString, valueOf, etc.
2. **A (static methods on constructors)** — fixes Object.keys, Object.freeze,
   Array.isArray, etc. as first-class values.
3. **C (errors as constructor instances)** — fixes assert.throws pattern.
4. **D (in operator)** — parser feature.
