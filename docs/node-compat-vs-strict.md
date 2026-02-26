# Builtins Contract: `RayaStrict` vs `NodeCompat`

This document formalizes which JavaScript/Node-style builtin behaviors are intentionally excluded from `RayaStrict`, and therefore only enabled in `NodeCompat`.

## `NodeCompat`-only surface (by design)

1. `Object.defineProperty`
2. `Object.getOwnPropertyDescriptor`
3. `Object.defineProperties`
4. Descriptor-driven accessor behavior (`get`/`set`) on object field reads/writes
5. Descriptor attribute enforcement for defined properties (`writable`, `configurable`)

## Why these do not fit `RayaStrict`

1. Descriptor mutation is highly dynamic and reflective; strict mode prioritizes explicit class fields/methods and predictable assignment behavior.
2. Runtime accessor dispatch (`get`/`set`) adds hidden control-flow on property read/write and weakens local reasoning in strict code.
3. Descriptor flags (`writable`, `configurable`, `enumerable`) introduce JavaScript object meta-programming semantics that are intentionally outside Raya strict object model.
4. The strict mode objective is stable, statically legible object behavior; descriptor APIs are retained only for compatibility migration and Node-like interop.

## Guardrail

When adding future builtin APIs, default to `RayaStrict` only if semantics are explicit and non-reflective. If API requires descriptor/meta-object behavior, gate it behind `NodeCompat`.
