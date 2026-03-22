# Test262 Language Clusters And Engine Rework Plan

Basis:
- corpus selector: `test/language`
- discovered cases: `23,887`
- stable overview sample: first `500` cases via the timeout-safe driver
- overview artifacts:
  - `test_analysis/test262/2026_03_22/language_overview_500.json`
  - `test_analysis/test262/2026_03_22/language_overview_500.md`
- runner used:
  - `scripts/test262_language_state.py`

This is not a “fix tests one by one” plan. The failures already cluster around a
small number of missing JS semantics. The right move is to rework the engine by
semantic layer and use Test262 only as the acceptance harness.

## Current Failure Clusters

Sample overview counts from the first 500 language cases:
- passed: `135`
- failed: `305`
- skipped: `60`
- timed out: `0`

Top failing sections:
- `test/language/arguments-object`: `148`
- `test/language/block-scope`: `106`
- `test/language/asi`: `51`

Engine-level clusters inferred from the sample:
- `71` wrong early-error classification
- `60` missing early errors
- `45` parser gaps around generator syntax
- `13` parser gaps around spread/rest in call and class-element positions
- `20` runtime property-descriptor mismatches
- `11` runtime throw-semantics mismatches
- `8` binder/redeclaration model mismatches
- `8` iterator protocol/runtime object-shape mismatches
- `23` general runtime semantic mismatches

## What These Failures Mean

### 1. Parser Is Not ECMAScript grammar-complete

Evidence:
- generator methods fail with `Unexpected token Star`
- spread/rest positions fail with `Unexpected token DotDotDot`
- ASI cases are parsed as hard syntax errors instead of newline-sensitive forms

Implication:
- the parser is still using a simplified statement/expression grammar where
  ECMAScript needs context-sensitive productions and line-terminator awareness

Likely files:
- `crates/raya-engine/src/parser/parser/expr.rs`
- `crates/raya-engine/src/parser/parser/stmt.rs`
- `crates/raya-engine/src/parser/lexer.rs`
- `crates/raya-engine/src/parser/parser/pattern.rs`

### 2. Binder And Early-Error Rules Are Too Simplified

Evidence:
- many `expected compilation to fail` cases currently compile
- other cases fail, but with the wrong error kind
- redeclaration cases collapse into generic duplicate-symbol failures
- some shadowing cases surface as const reassignment instead of scope legality

Implication:
- name binding, redeclaration legality, and ECMAScript early errors are being
  approximated by a single “symbol exists / assignment invalid” model

Likely files:
- `crates/raya-engine/src/parser/checker/binder.rs`
- `crates/raya-engine/src/parser/checker/checker.rs`
- `crates/raya-engine/src/parser/checker/symbols.rs`

### 3. Function Activation And `arguments` Semantics Are Not Real JS Yet

Evidence:
- mapped/unmapped `arguments` behavior is wrong
- `arguments.callee` and `caller` shape/strict-mode behavior is wrong
- `length` and indexed properties have wrong descriptor flags
- strict-mode write and throw behavior is inconsistent

Implication:
- `arguments` is currently synthesized as a plain dynamic object, not an
  arguments exotic object tied to a call frame and parameter mapping rules

Current hot path:
- `crates/raya-engine/src/compiler/lower/expr.rs`
  - `lower_js_arguments_object`

### 4. Object Descriptor Kernel Is Still Too Shallow For Conformance

Evidence:
- many failures are “should have own property” or “descriptor should not be
  enumerable/writable/configurable”
- some runtime checks read descriptor fields and receive `undefined`

Implication:
- the engine’s object model is good enough for ordinary app behavior but not yet
  strict enough for spec-accurate reflection and exotic-object surfaces

Likely files:
- `crates/raya-engine/src/vm/interpreter/handlers/reflect.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/objects.rs`
- `crates/raya-engine/src/vm/builtins/mod.rs`

### 5. Runtime Error Values Are Not Spec-Shaped Enough

Evidence:
- `Expected function to throw`
- `Expected function to throw the requested constructor`
- wrong negative-phase outcomes

Implication:
- thrown errors and compile/runtime phase boundaries still do not line up with
  Test262’s constructor-based assertions and negative metadata

Likely files:
- runtime error construction paths in the VM/interpreter
- compile/check entrypoints used by `raya-es262-conformance`

### 6. Iterator Protocol Surfaces Need To Be First-Class

Evidence:
- failures reading `.next` on `null`
- method invocation count assertions around iterator-driven cases

Implication:
- several places still treat iteration as a convenient engine helper instead of
  a fully shaped JS protocol with iterator/result objects and exact control flow

## Fundamental Rework Plan

### Phase 0. Keep The Conformance Loop Stable

Goal:
- never lose global visibility while changing the engine

Work:
- keep using `scripts/test262_language_state.py` for chunked, timeout-safe runs
- always write JSON and Markdown snapshots under `test_analysis/test262/...`
- expand from the 500-case overview to larger slices only after each phase lands

Success signal:
- zero timed-out cases in the driver
- deterministic snapshot files between runs

### Phase 1. Rebuild The Parser Around ECMAScript Context Flags

Goal:
- stop treating generator, spread/rest, and ASI cases as isolated parser bugs

Required rework:
- introduce explicit parse context flags for:
  - line terminator sensitivity
  - allow/disallow `yield`
  - allow/disallow `await`
  - `in`-operator suppression
  - class-element vs ordinary method heads
- make statement parsing newline-aware for:
  - `return`
  - `break`
  - `continue`
  - postfix `++` and `--`
- unify call/member/class-element parsing so generator methods and spread/rest
  forms reuse the same argument and parameter grammar

Why this is fundamental:
- ASI and generator bugs are symptoms of the parser not carrying the same
  context the ECMAScript grammar does

Acceptance slice:
- `test/language/asi`
- generator-heavy subsets under `test/language/arguments-object`

### Phase 2. Separate Binding, Early Errors, And Type Checking

Goal:
- make ECMAScript legality checks happen before type-level Raya rules distort them

Required rework:
- create a dedicated early-error pass that runs after AST construction but
  before regular type checking
- represent lexical environments explicitly:
  - module/global env
  - function env
  - parameter env
  - block env
  - catch env
  - class env
- encode redeclaration rules per environment type instead of a single duplicate
  symbol path
- classify negative failures by phase:
  - parse
  - early/binding
  - runtime

Why this is fundamental:
- current failures show the binder is carrying both JS legality and Raya typing
  concerns at once, which misclassifies many block-scope tests

Acceptance slice:
- `test/language/block-scope`
- strict negative tests expecting compile-time failure

### Phase 3. Introduce A Real JS Activation Record And Arguments Exotic Object

Goal:
- stop lowering `arguments` as a generic object literal

Required rework:
- add a runtime `ArgumentsObject` representation with two modes:
  - mapped arguments for non-strict functions
  - unmapped arguments for strict functions
- link mapped arguments slots to the function’s formal parameter environment
- materialize exact own properties:
  - integer indices
  - `length`
  - `callee`
  - `caller` behavior as required by strict/sloppy mode
- expose exact descriptor flags for those properties
- make iterator behavior for arguments explicit, not incidental

Current anti-pattern to remove:
- `lower_js_arguments_object` in `crates/raya-engine/src/compiler/lower/expr.rs`
  currently synthesizes a plain object and sets `length`, which cannot satisfy
  JS exotic semantics

Why this is fundamental:
- `arguments-object` is currently the biggest cluster and cannot be solved with
  local descriptor tweaks

Acceptance slice:
- all of `test/language/arguments-object`

### Phase 4. Finish The Descriptor/Object Kernel For Spec Surfaces

Goal:
- make reflection and property assertions trustworthy

Required rework:
- formalize a descriptor model with exact:
  - data vs accessor distinction
  - enumerable
  - writable
  - configurable
- ensure builtins and exotic objects participate in the same descriptor path
- make `getOwnPropertyDescriptor`, own-key enumeration, and property existence
  share one kernel instead of parallel ad hoc logic

Why this is fundamental:
- many runtime failures are not about computation; they are about object shape
  truthfulness and reflection fidelity

Acceptance slice:
- descriptor-related arguments-object tests
- reflection-heavy language cases that inspect builtins and exotics

### Phase 5. Normalize Thrown Errors And Negative Test Matching

Goal:
- align runtime and compile failures with what Test262 actually asserts

Required rework:
- ensure thrown values are proper error instances with correct constructors
- map engine parse/bind/runtime failures to the conformance runner’s negative
  expectations without stringly heuristics
- preserve exact error phase when surfacing failures from compile and execute

Why this is fundamental:
- “expected function to throw” and “expected requested constructor” are not
  isolated bugs; they show the engine still lacks a consistent error surface

Acceptance slice:
- strict-mode throw tests
- negative parse/runtime cases across `arguments-object`, `block-scope`, and `asi`

### Phase 6. Make Iterator Protocol A Shared Runtime Primitive

Goal:
- eliminate protocol drift between arrays, arguments, and helper-built iterables

Required rework:
- centralize iterator acquisition and `next()` stepping in one runtime path
- ensure iterator result objects and termination checks are spec-shaped
- stop relying on null-ish sentinel shortcuts that leak into user-visible errors

Why this is fundamental:
- iterator protocol bugs are showing up in sections that should not have custom
  per-feature iterator behavior

Acceptance slice:
- iterator-related `arguments-object` cases
- any `for-of` and protocol-driven language subsets that currently throw on `.next`

## Suggested Execution Order

1. Parser context and ASI
2. Early-error and scope legality pass
3. Arguments exotic object and function activation model
4. Descriptor kernel completion
5. Error surface normalization
6. Iterator protocol unification

This order matters:
- parser and early-error work unblock honest classification
- `arguments` and descriptors are the largest concrete semantic gap
- error normalization is only reliable once the preceding phases expose the
  right internal failure boundaries

## Non-Goals During The Rework

Avoid these traps:
- do not fix individual Test262 files by adding runner-specific hacks
- do not special-case `arguments` in the conformance harness beyond harness code
- do not keep layering extra binder exceptions on top of the current scope model
- do not patch ASI with token-by-token heuristics without line-terminator context

## Immediate Next Measurements

After each phase:
- rerun the same 500-case overview first
- regenerate the JSON/Markdown snapshot
- confirm the failing section mix changes in the expected direction

When phases 1 through 3 land:
- expand to a 2,000-case overview
- then run the full `test/language` sweep with the chunked driver

