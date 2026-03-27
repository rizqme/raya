# Test262 Language State

- selector: `test/language`
- discovered: `1000`
- recorded: `1000`
- passed: `947`
- failed: `19`
- skipped: `32`
- timed out: `2`
- infrastructure errors: `0`
- missing: `0`
- elapsed seconds: `1080.19`

## Early Overview

What looks healthy:
- The runner is resilient enough to keep marching through failing chunks instead of aborting on the first red case.
- A non-trivial slice of the selected language corpus already passes unchanged, so the failures are clustered rather than uniformly broken.
- Unsupported async/module/host-hook cases are being classified as skips instead of hanging the run.

What is failing first:
- `test/language/arguments-object`: `4` failures
- `test/language/eval-code`: `4` failures
- `test/language/asi`: `3` failures
- `test/language/directive-prologue`: `3` failures
- `test/language/comments`: `3` failures
- `test/language/destructuring`: `2` failures

Dominant failure clusters:
- `runtime-failure`: `14` cases
- `expected compilation to fail`: `3` cases
- `throw-behavior`: `1` cases
- `compile-failure`: `1` cases

## Timed Out Cases

- `test/language/comments/S7.4_A5.js`
- `test/language/comments/S7.4_A6.js`

## Sample Failed Cases

- `test/language/arguments-object/cls-decl-async-gen-func-args-trailing-comma-multiple.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Expected async closure or bound method
- `test/language/arguments-object/cls-decl-async-gen-func-args-trailing-comma-null.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Expected async closure or bound method
- `test/language/arguments-object/cls-decl-async-gen-func-args-trailing-comma-single-args.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Expected async closure or bound method
- `test/language/arguments-object/cls-decl-async-gen-func-args-trailing-comma-undefined.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Expected async closure or bound method
- `test/language/asi/S7.9.2_A1_T1.js`: expected compilation to fail
- `test/language/asi/S7.9_A10_T8.js`: expected compilation to fail
- `test/language/asi/S7.9_A11_T4.js`: expected compilation to fail
- `test/language/comments/S7.4_A1_T1.js`: runtime failed: Runtime error: Runtime error: Main task failed: Error: The value of `y` is expected to equal `undefined`
- `test/language/comments/S7.4_A2_T1.js`: runtime failed: Runtime error: Runtime error: Main task failed: Error: The value of `y` is expected to equal `undefined`
- `test/language/comments/hashbang/function-constructor.js`: runtime failed: Runtime error: Runtime error: Main task failed: Error: AsyncFunction Call argument
- `test/language/destructuring/binding/keyed-destructuring-property-reference-target-evaluation-order-with-bindings.js`: runtime failed: Runtime error: Runtime error: Main task failed: Error: Actual [binding::source, binding::sourceKey, sourceKey, get source, binding::defaultValue] and expected [binding::source, binding::sourceKey, sourceKey, binding::varTarget, get source, binding::defaultValue] should have the same contents. 
- `test/language/destructuring/binding/typedarray-backed-by-resizable-buffer.js`: runtime failed: Runtime error: Runtime error: Main task failed: Error: Array.concat expects 1 argument, got 2
- `test/language/directive-prologue/10.1.1-30-s.js`: runtime failed: Runtime error: Runtime error: Main task failed: Error: Expected function to throw
- `test/language/directive-prologue/10.1.1-31-s.js`: runtime failed: Runtime error: Runtime error: Main task failed: ReferenceError: public is not defined
- `test/language/directive-prologue/10.1.1-32-s.js`: runtime failed: Runtime error: Runtime error: Main task failed: ReferenceError: public is not defined
- `test/language/eval-code/direct/lex-env-distinct-const.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Assignment to constant variable.
- `test/language/eval-code/direct/var-env-func-strict-caller.js`: runtime failed: Runtime error: Runtime error: Main task failed: ReferenceError: fun is not defined
- `test/language/eval-code/direct/var-env-var-init-global-exstng.js`: runtime failed: Runtime error: Runtime error: Main task failed: Error: Expected SameValue(<<undefined>>, <<23>>) to be true
- `test/language/eval-code/indirect/global-env-rec-fun.js`: compilation failed: Compile error: Internal compiler error: Internal compiler error: unresolved call target '_eval': captured/ancestor value is not callable (type id 1)
