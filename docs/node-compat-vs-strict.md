# Builtins Contract: `RayaStrict` vs `NodeCompat`

This document formalizes which JavaScript/Node-style builtin behaviors are intentionally excluded from `RayaStrict`, and therefore only enabled in `NodeCompat`.

## Type-System Contract

1. `RayaStrict`
2. `any` is forbidden.
3. Bare `let x;` is forbidden.
4. Inference fallback is `unknown` (not `JSObject`).
5. `unknown` cannot be used for actionable operations until narrowed/casted.
6. Extracted methods are unbound and must be explicitly bound before direct calls.

1. `NodeCompat`
2. `any` is allowed.
3. Bare `let x;` is allowed.
4. Inference fallback can use `JSObject` for unpredictable dynamic object flows.
5. Extracted methods are also unbound (JS-like), with explicit `.bind(...)` for stable `this`.

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

## `node:` stdlib shim modules (v1)

`node:` module imports are supported as a compatibility shim over existing Raya stdlib modules and builtins. This is additive to `std:` imports (both remain valid).

Supported `node:` imports:

1. `node:fs` -> `std:fs`
2. `node:fs/promises` -> `raya-stdlib-node` fs promises shim
3. `node:path` -> `std:path`
4. `node:os` -> `std:os`
5. `node:process` -> `std:process`
6. `node:dns` -> `std:dns`
7. `node:net` -> `std:net`
8. `node:http` -> `std:http`
9. `node:https` -> `raya-stdlib-node` https shim
10. `node:crypto` -> `std:crypto`
11. `node:url` -> `std:url`
12. `node:stream` -> `std:stream`
13. `node:events` -> shim module exposing `EventEmitter` via builtins
14. `node:assert` -> `raya-stdlib-node` assert shim
15. `node:assert/strict` -> `raya-stdlib-node` assert shim
16. `node:util` -> `raya-stdlib-node` util shim
17. `node:module` -> `raya-stdlib-node` module shim
18. `node:child_process` -> `raya-stdlib-node` child process shim
19. `node:test` -> `raya-stdlib-node` test shim
20. `node:test/reporters` -> `raya-stdlib-node` test reporters shim
21. `node:timers` -> `raya-stdlib-node` timers shim
22. `node:timers/promises` -> `raya-stdlib-node` timers promises shim
23. `node:buffer` -> `raya-stdlib-node` buffer shim
24. `node:string_decoder` -> `raya-stdlib-node` string decoder shim
25. `node:stream/promises` -> `raya-stdlib-node` stream promises shim
26. `node:stream/web` -> `raya-stdlib-node` stream web shim

Unsupported `node:*` imports fail with an explicit diagnostic that lists supported modules.

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
