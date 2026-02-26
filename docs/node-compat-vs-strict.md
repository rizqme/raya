# Builtins Contract: `RayaStrict` vs `NodeCompat`

This document formalizes which JavaScript/Node-style builtin behaviors are intentionally excluded from `RayaStrict`, and therefore only enabled in `NodeCompat`.

## `NodeCompat`-only surface (by design)

1. `Object.defineProperty`
2. `Object.getOwnPropertyDescriptor`
3. `Object.defineProperties`
4. Descriptor-driven accessor behavior (`get`/`set`) on object field reads/writes
5. Descriptor attribute enforcement for defined properties (`writable`, `configurable`)
6. Typed-array and shared-memory family (`ArrayBuffer`, `DataView`, typed arrays, `SharedArrayBuffer`, `Atomics`)
7. Legacy JS global helpers (`parseInt`, `parseFloat`, `isNaN`, `isFinite`, `escape`, `unescape`)
8. Dynamic/meta-programming globals (`eval`, `Proxy`, `Reflect`, weak/finalization/disposal APIs, `globalThis`, `Intl`)
9. `WeakMap` / `WeakSet` are available in `NodeCompat` with pragmatic subset behavior.
10. `WeakRef` / `FinalizationRegistry` are available in `NodeCompat` as pragmatic subset APIs.
11. `Intl` is available in `NodeCompat` as a pragmatic subset (`Intl.NumberFormat`, `Intl.DateTimeFormat`).
12. Function/generator constructor families are available in `NodeCompat` (`Function`, `AsyncFunction`, `GeneratorFunction`, `AsyncGeneratorFunction`, `Generator`, `AsyncGenerator`, `AsyncIterator`).
13. `DisposableStack` / `AsyncDisposableStack` are available in `NodeCompat` as pragmatic subsets.
14. `SharedArrayBuffer` / `Atomics` are available in `NodeCompat` as pragmatic subsets.

## `RayaStrict` additions

1. `Buffer` remains available in strict mode.
2. `EventEmitter` is available in strict mode with Node-like minimal API:
   - `on`, `off`, `once`, `emit`, `listenerCount`, `removeAllListeners`.
3. `Temporal` is available in strict mode as a pragmatic subset:
   - `Temporal.Instant`, `Temporal.PlainDate`, `Temporal.PlainTime`, `Temporal.ZonedDateTime`.
4. `Iterator` is available in strict mode as a pragmatic subset:
   - `Iterator.fromArray`, `next`, `toArray`.
5. Shared primitive coercion globals are available in strict mode:
   - `Boolean(value)`, `Number(value)`, `String(value)`.

## Why these do not fit `RayaStrict`

1. Descriptor mutation is highly dynamic and reflective; strict mode prioritizes explicit class fields/methods and predictable assignment behavior.
2. Runtime accessor dispatch (`get`/`set`) adds hidden control-flow on property read/write and weakens local reasoning in strict code.
3. Descriptor flags (`writable`, `configurable`, `enumerable`) introduce JavaScript object meta-programming semantics that are intentionally outside Raya strict object model.
4. The strict mode objective is stable, statically legible object behavior; descriptor APIs are retained only for compatibility migration and Node-like interop.

## Guardrail

When adding future builtin APIs, default to `RayaStrict` only if semantics are explicit and non-reflective. If API requires descriptor/meta-object behavior, gate it behind `NodeCompat`.

## Unimplemented behavior signaling

Known edge behaviors that are intentionally not complete yet should throw with code `E_UNIMPLEMENTED_BUILTIN_BEHAVIOR` (for example, node-compat `eval`, DataView big-endian operations).

Current known limit: `Proxy`/`Reflect` currently support pragmatic `get`/`set`/`has` trap behavior; full ECMAScript proxy invariants/trap matrix are not implemented yet.
Current known limit: `WeakRef`/`FinalizationRegistry` do not integrate with GC finalization yet; cleanup is explicit (`cleanupSome`) in the current subset.
Current known limit: `Intl` formatting is currently pragmatic (`NumberFormat` uses numeric string conversion; `DateTimeFormat` falls back to ISO strings).
Current known limit: `Temporal` currently provides a pragmatic constructor/formatting subset, not full ECMA Temporal semantics.
Current known limit: extended typed-array variants (`Float16Array`, `Float32Array`, `BigInt64Array`, `BigUint64Array`) are available with pragmatic storage semantics, not full spec-accurate binary encoding yet.
Current known limit: function/generator constructor families are compile-visible in `NodeCompat`, but constructor execution currently throws `E_UNIMPLEMENTED_BUILTIN_BEHAVIOR`.
Current known limit: disposal stacks are pragmatic and do not yet integrate language-level `using`/`await using` syntax.
Current known limit: `Atomics.wait` / `Atomics.notify` are currently unimplemented and throw `E_UNIMPLEMENTED_BUILTIN_BEHAVIOR`.
