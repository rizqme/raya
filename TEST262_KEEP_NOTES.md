# Test262 Sweep Split Notes

This branch is the non-legacy extraction point from `codex/test262-sweep`.

Goals:
- keep standard ES/runtime/parser fixes that improve Raya generally
- omit Annex B and legacy web-compat behavior
- keep `main` free from the broad Test262 sweep history

Reference points:
- legacy-heavy branch: `codex/test262-sweep`
- snapshot on that branch: `83ddd2d` (`Snapshot Test262 sweep split point`)
- clean base for this branch: `main` at `b4bf202`

## Already Ported Here

These are the changes intentionally carried onto this branch now.

### Parser correctness

Files:
- `crates/raya-engine/src/parser/parser.rs`
- `crates/raya-engine/src/parser/parser/precedence.rs`
- `crates/raya-engine/src/parser/parser/expr.rs`
- `crates/raya-engine/src/parser/parser/stmt.rs`

Kept behavior:
- real comma-operator precedence
- `AssignmentExpression` parsing in separator positions
  - call arguments
  - array elements
  - object property values
  - parameter default values
  - variable initializers in `for (...)` heads
- `disallow_in` parser context for `for` initializers
- better `(x) => ...` lookahead so parenthesized single-parameter arrows are recognized reliably

Why keep it:
- these are core JS grammar fixes
- they are not Annex B specific
- they reduce parser ambiguity and misparsing outside Test262

## Keep From The Sweep

These should be ported later, but only after re-reviewing each patch to avoid dragging in legacy behavior.

### Standard JS runtime and binding behavior

Candidate commits:
- `c0891b2` `Fix JS global binding descriptor semantics`
- `bb9aecb` `Persist indirect eval lexical globals in runtime env`
- `eae0fa2` `Fix JS eval binding and script global coherence`

Files typically involved:
- `crates/raya-engine/src/compiler/lower/mod.rs`
- `crates/raya-engine/src/compiler/lower/stmt.rs`
- `crates/raya-engine/src/compiler/mod.rs`
- `crates/raya-engine/src/compiler/native_id.rs`
- `crates/raya-engine/src/vm/builtins/handlers/runtime.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/native.rs`
- `crates/raya-engine/src/vm/interpreter/shared_state.rs`

Keep rationale:
- global lexical and global object coherence is standard JS behavior
- direct and indirect `eval` should execute against the current VM/shared state, not a detached one
- descriptor-correct global bindings matter outside Test262

### General JS typing and runtime fixes

Candidate commit:
- `2bb62cc` `Reduce Test262 skips and fix JS dynamic typing`

Keep only the non-harness pieces:
- JS empty array typing fixes
- non-poisoning class-value member reads
- unannotated JS function expression and arrow param and return typing defaults
- runtime global lookup fallback to `globalThis` where appropriate

Do not carry from that commit:
- Test262 harness widenings
- `$262`-specific host shims unless explicitly desired for the test runner

### Dynamic class and runtime correctness

Areas to keep:
- dynamic `class extends` validation
- runtime-aware `instanceof`
- runtime parent-name tracking used for real class inheritance, not legacy cases

Files:
- `crates/raya-engine/src/compiler/lower/expr.rs`
- `crates/raya-engine/src/compiler/lower/mod.rs`
- `crates/raya-engine/src/compiler/native_id.rs`
- `crates/raya-engine/src/parser/checker/binder.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/native.rs`
- `crates/raya-engine/src/vm/interpreter/shared_state.rs`
- `crates/raya-engine/src/vm/object.rs`

Why keep it:
- this is standard class semantics, not Annex B

### Generator foundation

Candidate commits:
- `c622eee` `Add task-backed generator runtime foundation`
- `6bf0774` `Add synchronous callable invocation helper`

Files:
- `crates/raya-engine/src/compiler/ir/function.rs`
- `crates/raya-engine/src/compiler/ir/instr.rs`
- `crates/raya-engine/src/compiler/bytecode/module.rs`
- `crates/raya-engine/src/compiler/codegen/context.rs`
- `crates/raya-engine/src/compiler/lower/stmt.rs`
- `crates/raya-engine/src/compiler/module_builder.rs`
- `crates/raya-engine/src/vm/interpreter/core.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/concurrency.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/native.rs`
- `crates/raya-engine/src/vm/scheduler/task.rs`

Why keep it:
- generators are standard ES
- the task-backed design is a core runtime improvement, not legacy compatibility

Note:
- this still needs follow-up for delegated `yield*`
- keep the foundation, not any Annex B function-scoping work that happened around it

### Function, object, and descriptor correctness

Keep these when isolated cleanly:
- proper function-object `name` and `length` descriptors
- accessor and data descriptor application fixes
- stable constructor `prototype` behavior
- delete/property semantics fixes
- callable-property dispatch fixes that are general runtime correctness

Why keep it:
- these are standard object model fixes
- they support normal JS interop and builtins, not just Test262

### `undefined` and nullish semantics

Keep these when isolated cleanly:
- distinct runtime `undefined`
- missing parameters materialize as `undefined`
- loose nullish equality behaves correctly

Why keep it:
- this is baseline JS semantics

### Symbol, coercion, and base constructor semantics

Keep these when isolated cleanly:
- callable `Symbol(...)`
- `Number(Symbol(...))` throwing `TypeError`
- non-legacy `Function(...)` constructor support itself
- `ToPrimitive` / `valueOf` / `toString` fixes that are standard coercion behavior

## Omit From This Branch

These are intentionally out because they are legacy-specific, Annex B-specific, or web-compat-specific.

### Legacy Date APIs

Omit:
- `Date.prototype.getYear`
- `Date.prototype.setYear`

Files:
- `crates/raya-engine/builtins/node_compat/date.raya`

### String legacy HTML methods

Omit:
- `anchor`
- `big`
- `blink`
- `bold`
- `fixed`
- `fontcolor`
- `fontsize`
- `italics`
- `link`
- `small`
- `strike`
- `sub`
- `sup`

Files:
- `crates/raya-engine/builtins/node_compat/globals.raya`
- `crates/raya-engine/builtins/strict/string.raya`
- `crates/raya-engine/src/compiler/lower/class_methods.rs`

### Legacy string aliases and globals

Omit:
- `trimLeft`
- `trimRight`
- `escape`
- `unescape`

Files:
- `crates/raya-engine/builtins/node_compat/globals.raya`

### RegExp legacy statics, accessors, and `compile`

Omit:
- `RegExp.input`
- `RegExp.$1` through `RegExp.$9`
- related legacy descriptor plumbing
- `RegExp.prototype.compile`

Files:
- `crates/raya-engine/builtins/strict/regexp.raya`
- `crates/raya-engine/src/vm/builtin.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/native.rs`
- `crates/raya-engine/src/vm/interpreter/handlers/reflect.rs`

### Annex B parser and runtime behavior

Omit:
- HTML-comment handling only needed for Annex B dynamic `Function(...)` cases
- sloppy direct-eval block-function Annex B rules
- sloppy `for-in` initializer behavior
- legacy octal escape acceptance done only for Annex B compatibility

### Web-compat host quirks

Omit:
- `IsHTMLDDA`
- `$262.IsHTMLDDA`

Files:
- `crates/raya-engine/src/vm/interpreter/opcodes/comparison.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/control_flow.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/native.rs`
- `crates/raya-engine/src/vm/interpreter/opcodes/types.rs`
- `crates/raya-test262/src/lib.rs`

## Runner Notes

If the goal later is only engine quality, do not pull over broad Test262 runner changes by default.

Keep only obviously useful runner improvements if desired:
- better failure diagnostics
- `--from` / `--to` selection

Do not port runner changes that exist only to unskip legacy or host-hook-heavy tests.

## Main Branch Safety

`main` was checked before creating this branch and remains at:
- `b4bf202` `Rewrite CLAUDE hierarchy for navigation`

The Test262 sweep history stays isolated on:
- `codex/test262-sweep`
