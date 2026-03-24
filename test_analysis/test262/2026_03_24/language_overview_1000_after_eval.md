# Test262 Language State

- selector: `test/language`
- discovered: `1000`
- recorded: `1000`
- passed: `547`
- failed: `313`
- skipped: `140`
- timed out: `0`
- infrastructure errors: `0`
- missing: `0`
- elapsed seconds: `76.82`

## Early Overview

What looks healthy:
- The runner is resilient enough to keep marching through failing chunks instead of aborting on the first red case.
- A non-trivial slice of the selected language corpus already passes unchanged, so the failures are clustered rather than uniformly broken.
- Unsupported async/module/host-hook cases are being classified as skips instead of hanging the run.

What is failing first:
- `test/language/block-scope`: `113` failures
- `test/language/arguments-object`: `85` failures
- `test/language/computed-property-names`: `38` failures
- `test/language/asi`: `37` failures
- `test/language/directive-prologue`: `22` failures
- `test/language/comments`: `14` failures
- `test/language/destructuring`: `4` failures

Dominant failure clusters:
- `parser-unsupported-syntax`: `164` cases
- `runtime-failure`: `84` cases
- `expected compilation to fail`: `27` cases
- `compile-failure`: `26` cases
- `throw-behavior`: `8` cases
- `unexpected compilation error`: `4` cases

## Sample Failed Cases

- `test/language/arguments-object/10.5-1gs.js`: unexpected compilation error: Parse error: /private/var/folders/jr/hqhj_b8x7s13r08cg8wrhn_w0000gn/T/raya-es262-66021-test_language_arguments_object_10_5_1gs_js.js: Parse error at 29:5: Invalid syntax: Assignment to 'arguments' is not allowed in strict mode
- `test/language/arguments-object/10.6-13-a-2.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Restricted function property access
- `test/language/arguments-object/10.6-13-a-3.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: Restricted function property access
- `test/language/arguments-object/S10.6_A4.js`: runtime failed: Runtime error: Runtime error: Main task failed: Runtime error: #1: arguments object doesn't exists
- `test/language/arguments-object/cls-decl-gen-meth-args-trailing-comma-multiple.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-args-trailing-comma-null.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-args-trailing-comma-single-args.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-args-trailing-comma-spread-operator.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-args-trailing-comma-undefined.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-static-args-trailing-comma-multiple.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-static-args-trailing-comma-null.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-static-args-trailing-comma-single-args.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-static-args-trailing-comma-spread-operator.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-gen-meth-static-args-trailing-comma-undefined.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-decl-private-gen-meth-args-trailing-comma-multiple.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-args-trailing-comma-null.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-args-trailing-comma-single-args.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-args-trailing-comma-spread-operator.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-args-trailing-comma-undefined.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-static-args-trailing-comma-multiple.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-static-args-trailing-comma-null.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-static-args-trailing-comma-single-args.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-static-args-trailing-comma-spread-operator.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-gen-meth-static-args-trailing-comma-undefined.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-args-trailing-comma-multiple.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-args-trailing-comma-null.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-args-trailing-comma-single-args.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-args-trailing-comma-spread-operator.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-args-trailing-comma-undefined.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-static-args-trailing-comma-multiple.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-static-args-trailing-comma-null.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-static-args-trailing-comma-single-args.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-static-args-trailing-comma-spread-operator.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-decl-private-meth-static-args-trailing-comma-undefined.js`: compilation failed: Type error: <inline>.raya: Binding error: Duplicate symbol 'method'
- `test/language/arguments-object/cls-expr-gen-meth-args-trailing-comma-multiple.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-args-trailing-comma-null.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-args-trailing-comma-single-args.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-args-trailing-comma-spread-operator.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-args-trailing-comma-undefined.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-static-args-trailing-comma-multiple.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-static-args-trailing-comma-null.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-static-args-trailing-comma-single-args.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-static-args-trailing-comma-spread-operator.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-gen-meth-static-args-trailing-comma-undefined.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
- `test/language/arguments-object/cls-expr-private-gen-meth-args-trailing-comma-multiple.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Function.prototype.call target is not callable
- `test/language/arguments-object/cls-expr-private-gen-meth-args-trailing-comma-null.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Function.prototype.call target is not callable
- `test/language/arguments-object/cls-expr-private-gen-meth-args-trailing-comma-single-args.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Function.prototype.call target is not callable
- `test/language/arguments-object/cls-expr-private-gen-meth-args-trailing-comma-spread-operator.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Function.prototype.call target is not callable
- `test/language/arguments-object/cls-expr-private-gen-meth-args-trailing-comma-undefined.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Function.prototype.call target is not callable
- `test/language/arguments-object/cls-expr-private-gen-meth-static-args-trailing-comma-multiple.js`: runtime failed: Runtime error: Runtime error: Main task failed: TypeError: Cannot read properties of null (reading 'next')
