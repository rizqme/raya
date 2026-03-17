# Node.js Compatibility Scope Specification (Raya)

## Document Status
- Status: Draft for alignment and execution planning
- Audience: Runtime, compiler, stdlib, package manager, CLI, QA, docs teams
- Purpose: Define the complete scope for "full Node.js compatibility" in Raya

## 1. Executive Definition

"Full Node.js compatibility" means a project written for Node.js can run on Raya's Node-compat runtime mode with no source changes (or only standard ecosystem-level transpilation), while preserving runtime behavior, module resolution, core API contracts, package install behavior, tooling expectations, and observable edge-case semantics to a level acceptable for production migration.

This includes:
- JavaScript runtime semantics expected by Node-targeted code
- Node's module systems (ESM + CommonJS) and their interop
- Node core module API and behavior compatibility
- npm ecosystem compatibility (dependency graph, package metadata semantics, node_modules behavior)
- Native addon compatibility strategy (N-API boundary)
- Tooling and protocol compatibility needed by real production workflows
- A measurable conformance bar with pass/fail criteria

This does not mean replacing Raya's native model. It means adding an explicit, isolated compatibility surface.

## 2. Scope Boundary and Operating Modes

### 2.1 Required Runtime Modes
- `raya` (native mode): Existing semantics and stdlib behavior remain unchanged.
- `raya --node-compat` (compat mode): Enables Node-compatible semantics and APIs.

### 2.2 Hard Rule
- Compatibility behavior MUST be opt-in and isolated.
- Native Raya behavior MUST NOT silently drift toward JS semantics outside compat mode.

### 2.3 Compatibility Targets
- Primary compatibility target: one pinned Node major line (for example, Node 22.x semantics baseline).
- Secondary target: compatible behavior for at least one previous major where practical.
- Version pinning must be explicit and documented in this file when finalized.

## 3. Normative Compatibility Contract

A capability is considered "compatible" only if all are true:
- API shape matches (names, signatures, overload behavior)
- Return values and thrown errors match expected behavior
- Timing/scheduling behavior matches where observable
- Platform behavior matches (Linux/macOS/Windows) within documented exceptions
- Edge cases match documented Node behavior
- Interoperability with ecosystem tools/packages is validated

## 4. Compatibility Dimensions (Complete)

## 4.1 Language and Runtime Semantics

### 4.1.1 JavaScript Value Semantics
In scope:
- `undefined`, `null`, booleans, numbers (`NaN`, `Infinity`, `-0`), bigint, strings, symbols
- Objects, arrays, maps, sets, typed arrays, DataView, ArrayBuffer, SharedArrayBuffer (if supported by target baseline)
- Property access semantics and property attributes (writable/enumerable/configurable)
- Prototype chain lookup and mutation semantics
- `instanceof`, `typeof`, `in`, `delete`, optional chaining, nullish coalescing behavior

### 4.1.2 Coercion and Equality
In scope:
- Abstract equality (`==`) semantics
- Strict equality (`===`) semantics
- Relational comparisons and ToPrimitive/ToNumber/ToString conversion edge cases
- Truthiness/falsiness rules

### 4.1.3 Function/Class/This Semantics
In scope:
- Function call `this` binding behavior
- Arrow function lexical `this`
- Class fields, static fields, private fields/methods behavior
- Constructor/new behavior and errors
- `super`, `new.target`, `arguments` semantics

### 4.1.4 Error and Stack Semantics
In scope:
- Standard JS errors and Node-specific operational errors
- Error codes where relevant (e.g. `ERR_*`, `ENOENT`, etc.)
- Stack formatting compatibility sufficient for ecosystem tooling
- Cause chaining (`cause`) and aggregate errors

### 4.1.5 Microtasks, Macrotasks, Event Loop
In scope:
- Promise microtask queue ordering
- `queueMicrotask`, `setTimeout`, `setInterval`, `setImmediate`
- `process.nextTick` ordering relative to Promise jobs and timers
- I/O callback dispatch ordering compatibility where observable

Out of scope unless explicitly committed:
- Bit-for-bit internal scheduling implementation parity
- Undocumented ordering side effects

### 4.1.6 Object Model and Property Descriptor Semantics
In scope:
- `Object.defineProperty`, `Object.getOwnPropertyDescriptor`, `Object.getOwnPropertyDescriptors`
- `Object.create`, `Object.setPrototypeOf`, `Object.getPrototypeOf`, `Object.hasOwn`
- Enumerability/ownership rules for:
  - `for...in`
  - `Object.keys/values/entries`
  - `Reflect.ownKeys`
  - `JSON.stringify` own-enumerable traversal
- Accessor semantics (`get`/`set`) and side-effect timing
- Non-configurable/non-writable behavior and strict mode errors
- `Object.freeze`, `Object.seal`, `Object.preventExtensions` invariants

### 4.1.7 Prototype and Class Interop Semantics
In scope:
- Prototype chain lookup precedence and shadowing behavior
- `class` methods on prototype vs instance fields on object
- `super` dispatch rules for methods and accessors
- Static inheritance for classes and static field initialization order
- Private field brand checks and thrown error behavior
- `instanceof` behavior including `Symbol.hasInstance`

### 4.1.8 Primitive Wrappers and Boxing Behavior
In scope:
- Auto-boxing behavior for string/number/boolean primitive method calls
- Wrapper object differences (`new String`, `new Number`, `new Boolean`) vs primitives
- `valueOf`/`toString` precedence in primitive conversion
- Symbol and BigInt boxing behavior and constraints

### 4.1.9 Coercion Algorithms (Normative)
In scope:
- `ToPrimitive` with hint handling (`string`/`number`/`default`)
- `ToNumber` edge cases: whitespace, empty string, hex/binary/octal literals, invalid numerics
- `ToString` edge cases for arrays, objects, symbols, bigint
- `ToBoolean` exact falsy set (`false`, `0`, `-0`, `0n`, `NaN`, `\"\"`, `null`, `undefined`)
- `ToPropertyKey` behavior for symbol/string keys
- Arithmetic coercion behavior for `+`, `-`, `*`, `/`, `%`, `**`

### 4.1.10 Equality and Comparison Edge Cases
In scope:
- `NaN !== NaN` and `Object.is(NaN, NaN)` behavior
- `Object.is(+0, -0)` distinction
- `==` cross-type coercion behavior (`null == undefined`, number/string/bool comparisons)
- Relational comparison order and coercion timing for side-effectful operands
- BigInt/Number comparison rules and thrown errors for invalid mixed arithmetic

### 4.1.11 Function Invocation and Arguments Semantics
In scope:
- `call`, `apply`, `bind` behavior and bound function metadata constraints
- Default parameters evaluation order
- Rest parameters and spread argument evaluation order
- `arguments` aliasing behavior (sloppy vs strict semantics as applicable)
- Function `name` and `length` behavior compatible enough for ecosystem checks
- Tail behavior for constructor calls without `new` where Node/JS throws

### 4.1.12 Strict Mode, Scope, and Hoisting Behavior
In scope:
- Strict mode parsing/runtime errors (`with`, duplicate params, assignment to non-writable targets)
- `var` function-scope hoisting semantics
- `let`/`const` temporal dead zone behavior
- Block scope capture behavior in loops (`for (let i...)`)
- `eval` scoping behavior policy:
  - If supported: must match strict/non-strict observable rules
  - If unsupported: explicit incompatibility classification and guardrail errors

### 4.1.13 Control Flow and Exception Semantics
In scope:
- `try/catch/finally` completion behavior (return/throw interaction)
- Labeled break/continue behavior
- `switch` strict comparison matching and fallthrough behavior
- Iterator closing behavior on abrupt completion (`break`, `throw`, `return`)

### 4.1.14 Iteration and Protocol Semantics
In scope:
- Sync iterator protocol (`Symbol.iterator`) and async iterator protocol (`Symbol.asyncIterator`)
- `for...of` and `for await...of` semantics including early close (`return()`)
- Generator and async generator behavior:
  - `next`, `throw`, `return`
  - delegation (`yield*`)
  - completion/exception propagation

### 4.1.15 Built-in Collections and Typed Data Semantics
In scope:
- `Map` insertion-order guarantees and key identity rules
- `Set` uniqueness rules (`NaN`, `+0/-0` behavior)
- WeakMap/WeakSet key constraints and error behavior
- `ArrayBuffer`, TypedArray, DataView endianness methods and range-check semantics
- `subarray`, `slice`, copy semantics and detached buffer behavior policy

### 4.1.16 Promise and Async Function Semantics
In scope:
- Promise resolution procedure (thenable assimilation behavior)
- Single-settlement guarantees
- `Promise.all`, `allSettled`, `race`, `any` ordering and error aggregation semantics
- Async function rejection wrapping semantics
- Unhandled rejection policy compatibility:
  - emission timing
  - process warning/error policy mode

### 4.1.17 RegExp and String Semantics
In scope:
- RegExp flags (`g`, `i`, `m`, `s`, `u`, `y`, `d` as baseline dictates) and behavior
- `lastIndex` mutation rules
- String/RegExp method interaction (`match`, `matchAll`, `replace`, `replaceAll`, `search`, `split`)
- Replacement pattern semantics (`$&`, `$1`, `$<name>`, etc.)
- Unicode behavior and normalization policy where Node baseline defines behavior

### 4.1.18 Date, Time, and Intl-Sensitive Behavior
In scope:
- `Date` parse/format compatibility level and timezone behavior contract
- Epoch boundary behavior and invalid date handling
- Timer delay clamping behavior and maximum timeout behavior
- Locale/Intl policy:
  - If full Intl support is claimed: behavior must match Node baseline capabilities
  - If partial: explicitly classify unsupported locales/features

### 4.1.19 JSON Semantics
In scope:
- `JSON.parse` error behavior and reviver order
- `JSON.stringify` replacer and space argument semantics
- Property traversal ordering compatibility for ordinary objects
- BigInt serialization behavior and thrown error compatibility
- `toJSON` hook invocation semantics

### 4.1.20 Reflect and Proxy Semantics
In scope:
- `Reflect` API behavior parity (`get`, `set`, `defineProperty`, `ownKeys`, etc.)
- `Proxy` traps and invariant enforcement
- Error semantics when proxy invariants are violated
- Compatibility for meta-programming-heavy libraries relying on proxy edge cases

### 4.1.21 Symbol and Well-Known Symbol Behavior
In scope:
- Symbol identity and registry behavior (`Symbol.for`, `Symbol.keyFor`)
- Well-known symbol hooks:
  - `Symbol.iterator`
  - `Symbol.asyncIterator`
  - `Symbol.toStringTag`
  - `Symbol.toPrimitive`
  - `Symbol.hasInstance`
  - `Symbol.species`
  - `Symbol.match`, `replace`, `search`, `split`
- Symbol-keyed property enumerability/visibility rules

### 4.1.22 Observable Ordering Guarantees
In scope:
- Property insertion ordering rules for object keys (integer-like, string, symbol ordering)
- Deterministic ordering guarantees for maps/sets/arrays as defined by JS semantics
- Callback ordering in common built-ins (`Array.prototype` methods, Promise handlers)

### 4.1.23 Numeric and Bitwise Semantics
In scope:
- 32-bit conversion behavior for bitwise operators
- Shift operator semantics (`>>`, `>>>`, `<<`) including sign behavior
- `Math` edge cases and IEEE-754 consistency requirements
- BigInt operator coverage and forbidden mixed-operand operations

### 4.1.24 Compatibility Exclusions and Escalation Rule
Any JavaScript behavior not listed above but observable by user code is considered in scope by default if:
- it is required by the pinned Node baseline, and
- it is exercised by the conformance suite or ecosystem corpus.

Exclusions must be explicitly added to the deviation manifest with owner and milestone.

## 4.2 Module System Compatibility

### 4.2.1 ESM Support
In scope:
- Static import/export
- Dynamic import
- Top-level await
- Live bindings semantics
- URL/path module identity behavior consistent with Node rules

### 4.2.2 CommonJS Support
In scope:
- `require`, `module`, `exports`, `module.exports`
- CJS wrapper semantics (`__filename`, `__dirname`, `require.main` behavior)
- CJS cache semantics and singleton loading behavior

### 4.2.3 ESM <-> CJS Interop
In scope:
- Importing CJS from ESM and vice versa with Node-compatible default/named behavior
- Synthetic default export behavior where Node expects it
- Edge cases around `exports` object mutation timing

### 4.2.4 Resolution Algorithm
In scope:
- Relative/absolute specifier resolution
- Bare specifier resolution from `node_modules`
- `package.json` fields: `exports`, `imports`, `main`, `type`, `name`
- Conditional exports conditions (at minimum: `import`, `require`, `default`, `node`)
- Extension resolution behavior where applicable
- Directory index resolution behavior where applicable
- Symlink handling and package boundary resolution behavior

### 4.2.5 Loader Hooks and Policies
In scope (if claiming full tooling parity):
- Custom loader interfaces needed by major ecosystem tools
- Source map support in loader/transpilation paths

## 4.3 Global Objects and Runtime Environment

In scope:
- `globalThis`, `global`
- `process`
- `Buffer`
- `console`
- Timers (`setTimeout`, `clearTimeout`, etc.)
- `URL`, `URLSearchParams`
- `TextEncoder`, `TextDecoder`
- `AbortController`, `AbortSignal`
- `performance`
- `EventTarget` and events model where required by core APIs
- WHATWG streams if baseline Node major expects this in core pathways

## 4.4 Node Core Module Surface

Full compatibility scope includes these modules with behavior-level parity and test coverage:

### 4.4.1 Fundamental
- `node:fs` and `node:fs/promises`
- `node:path` (`posix`, `win32` variants)
- `node:os`
- `node:process` (global + module contract)
- `node:util`
- `node:events`
- `node:buffer`
- `node:stream` and `node:stream/promises`

### 4.4.2 Networking/IPC
- `node:net`
- `node:dgram`
- `node:dns`
- `node:http`
- `node:https`
- `node:http2` (if baseline declares support target)
- `node:tls`
- `node:ws` is not core Node; compatibility handled via ecosystem package interop

### 4.4.3 Process/Execution
- `node:child_process`
- `node:cluster` (if included in baseline target)
- `node:worker_threads`
- `node:readline`
- `node:repl` (optional unless required by compatibility statement)

### 4.4.4 Data/Crypto/Compression
- `node:crypto`
- `node:zlib`
- `node:querystring` (legacy but still used)
- `node:url`
- `node:assert`
- `node:vm`

### 4.4.5 File/Platform Utilities
- `node:fs`, `node:path`, `node:module`
- `node:diagnostics_channel`
- `node:inspector` (required for debugger protocol parity claims)

### 4.4.6 Compatibility Classification Per API
Each API endpoint must be labeled:
- `FULL`: behavior tested and compatible
- `PARTIAL`: works with documented caveats
- `STUB`: available symbol, non-functional / throws unsupported
- `UNSUPPORTED`: absent

Claiming "full Node compatibility" requires no `PARTIAL/STUB/UNSUPPORTED` entries in baseline scope, except explicitly waived in the public compatibility policy.

## 4.5 Filesystem and Path Behavioral Parity

In scope:
- Sync and async APIs parity where Node offers both
- File descriptor semantics
- Permission errors and errno mapping
- Symlink, hardlink, stat/lstat behavior
- UTF-8 and binary path edge cases as supported by OS
- Watch behavior compatibility contract (`fs.watch`, recursive caveats)
- Path normalization quirks across POSIX and Windows

## 4.6 Process and OS Semantics

In scope:
- `process.argv`, `execArgv`, `env`, `cwd`, `chdir`, `exitCode`, `exit`
- Signal behavior (`SIGINT`, `SIGTERM`, etc.)
- `stdin/stdout/stderr` stream characteristics
- TTY detection behavior
- Platform metadata (`process.platform`, `process.arch`)
- High-resolution timers (`hrtime`) and uptime/memory usage semantics

## 4.7 Async, Streams, and Backpressure

In scope:
- Node stream classes (`Readable`, `Writable`, `Duplex`, `Transform`, `PassThrough`)
- Backpressure semantics and event ordering
- Pipeline and compose utilities
- Interop between Buffer/string/object mode
- Stream error propagation and close semantics

## 4.8 Package Ecosystem Compatibility (npm Semantics)

### 4.8.1 package.json Contract
In scope:
- Parsing and honoring `dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies`
- `engines`, `os`, `cpu` constraints behavior
- `bin`, `exports`, `imports`, `files`, `types` handling as needed for runtime/toolchain

### 4.8.2 Install and Resolution Behavior
In scope:
- node_modules graph construction and lookup compatibility
- Hoisting strategy compatibility adequate for ecosystem expectations
- Symlink/bin linking semantics
- Deterministic lockfile behavior with integrity hashes
- Offline cache behavior and reproducibility controls

### 4.8.3 Lifecycle Scripts
In scope:
- `preinstall/install/postinstall`
- `prepare`, `prepack`, etc. where required for package usability
- Security policy controls for script execution

### 4.8.4 Registry/Auth
In scope:
- npm registry protocol compatibility
- Auth token handling for private registries
- Scoped registry config behavior

## 4.9 Native Addon Compatibility

### 4.9.1 N-API Strategy (Mandatory Decision)
To claim full Node compatibility for real-world packages, one of these must be in scope:
- N-API ABI compatibility layer
- Or strict declaration that native addons are unsupported, which downgrades claim from "full" to "high" compatibility

### 4.9.2 Node-API Coverage
If in scope:
- Versioned N-API symbols compatibility map
- Thread-safe function behavior
- Finalizers, external buffers, async workers
- ABI stability guarantees

## 4.10 Tooling and Developer Experience Compatibility

In scope:
- Source map compatibility for stack traces and debugging
- Inspector protocol compatibility for IDE debugging
- Test runners commonly used in Node projects (runtime compatibility, not reimplementation)
- Transpilation toolchain compatibility (TypeScript/Babel/SWC outputs targeting Node)
- Lint/format/tool hooks that assume Node runtime

## 4.11 Security and Sandboxing Compatibility Policy

Define and enforce:
- Whether compat mode mirrors Node's unrestricted I/O model by default
- Optional permission model and its defaults
- Environment variable and subprocess risk boundaries
- Policy flags for CI/hardened runtime use

## 4.12 Platform/Architecture Matrix

Mandatory support matrix with explicit status per target:
- Linux x64, arm64
- macOS x64, arm64
- Windows x64 (minimum)

For each target define:
- CI coverage
- Core module pass rate
- Known deviations

## 4.13 Performance and Resource Expectations

In scope:
- Startup time envelope vs Node baseline (declared target, e.g., <= X% regression for compat workloads)
- Throughput/latency benchmarks for HTTP, fs, stream, crypto
- Memory behavior and leak checks

Out of scope:
- Matching Node internals or implementation language-level architecture

## 4.14 Diagnostics and Error Compatibility

In scope:
- Node-like error classes and codes
- Human-readable error messages sufficiently compatible for ecosystem checks
- Exit codes for CLI/runtime failures
- Warning channels and deprecation behavior policy

## 4.15 Observability

In scope:
- `console` behavior
- `process.emitWarning`
- Diagnostics channel support where required by tooling
- Inspector hooks (if supported)

## 5. Non-Goals (Must Be Explicit)

Unless later promoted to scope, non-goals include:
- Perfect byte-for-byte matching of undocumented internals
- Re-creating Node's internal implementation details not observable by user code
- Supporting deprecated APIs with no ecosystem use (evaluate via telemetry)

## 6. Compatibility Tiers

Define tiers to avoid ambiguous claims:
- Tier 0: Native Raya only
- Tier 1: Core JS runtime + limited Node globals
- Tier 2: Core Node modules + ESM/CJS resolution
- Tier 3: npm ecosystem compatibility for pure JS packages
- Tier 4: Native addon/N-API compatibility
- Tier 5: Full production-grade Node compatibility

Public "full compatibility" claim is allowed only at Tier 5.

## 7. Conformance and Acceptance Criteria

## 7.1 Test Sources
Conformance suite must include:
- Internal API parity tests per module/function
- Ecosystem package corpus tests (real packages)
- Node behavior regression tests for edge cases
- Cross-platform CI matrix runs

## 7.2 Quantitative Exit Gates
Minimum release gates for claiming "full":
- 99%+ pass on defined Node-core compatibility suite for scoped APIs
- 95%+ pass rate on curated real-world npm corpus (excluding explicitly waived packages)
- 100% pass on critical modules: `fs`, `path`, `process`, `buffer`, `stream`, `events`, `http`, `crypto`
- No P0/P1 compatibility bugs open for baseline scope

## 7.3 Waiver Policy
- Every deviation must have:
  - unique ID
  - affected API/package
  - severity
  - workaround
  - target fix milestone
- Deviations must be published in a machine-readable compatibility manifest.

## 8. API Coverage Inventory Requirement

Before implementation, create a complete machine-readable inventory for all Node baseline APIs:
- Module
- Export name
- Signature
- Sync/async model
- Status (`FULL/PARTIAL/STUB/UNSUPPORTED`)
- Test references
- Known deviations

This inventory is a required artifact and release gate.

## 9. Implementation Workstreams (Complete Scope)

## 9.1 Runtime Semantics Workstream
- JS semantics layer
- Event loop and scheduling compatibility
- Error model parity

## 9.2 Module/Loader Workstream
- ESM parser/loader semantics
- CJS runtime wrapper
- Resolution algorithm parity
- Package boundary rules

## 9.3 Core Modules Workstream
- Implement/bridge all Node core modules
- Cross-platform abstractions for OS variance

## 9.4 Package Manager Workstream
- npm protocol + node_modules semantics
- Lockfile/integrity
- Scripts and security controls

## 9.5 Native Addon Workstream
- N-API compatibility layer or explicit downgrade policy

## 9.6 Tooling Workstream
- Inspector, source maps, debugging
- Test-runner and transpilation interoperability

## 9.7 QA and Conformance Workstream
- Compatibility harness
- Corpus automation
- Flake and regression management

## 9.8 Docs and Developer Guidance Workstream
- Compatibility docs and migration guides
- Deviation catalog
- Versioned support matrix

## 10. Deliverables Checklist (Definition of Ready for Claim)

Required deliverables:
- Node compatibility policy document (this scope + finalized baseline)
- Machine-readable API inventory and status
- Published deviations manifest
- Conformance dashboard and CI reports
- Migration guides and known-issues docs
- Runtime flags and stable CLI surface for compat mode
- Security posture and defaults documentation

## 11. Release and Versioning Policy

In scope:
- Compat mode semantic version contract
- Compatibility target updates per Node major
- Deprecation and removal policy with timelines
- Backport strategy for critical compatibility regressions

## 12. Governance and Change Control

Any new claim of compatibility must include:
- API inventory delta
- Conformance result delta
- Performance/security impact assessment
- Documentation updates

No compatibility claim is allowed based on anecdotal package success alone.

## 13. Immediate Next Artifacts to Produce

1. Baseline Selection Record
- Choose exact Node major/minor baseline and freeze it for initial release.

2. API Inventory Generator
- Generate full list of required modules/exports for the baseline.

3. Gap Report Against Current Raya
- For each API mark status and owning team.

4. Milestone Plan
- Convert workstreams into execution milestones with measurable gates.

5. Public Compatibility Matrix
- Publish what works/does not work before the first compat preview.

## 14. Reality Check Against Current Raya Positioning

Current project messaging emphasizes:
- no JavaScript compatibility layer
- no npm ecosystem interop guarantee
- no JavaScript interop positioning

Therefore, this scope represents a major strategic expansion and should be treated as a separate product track (Node-compat edition) with dedicated governance, testing, and release discipline.
