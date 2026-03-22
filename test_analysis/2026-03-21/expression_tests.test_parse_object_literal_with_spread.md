# expression_tests::test_parse_object_literal_with_spread

- Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/expression_tests/mod.rs:282`
- Status in full workspace run: hung until the workspace run was aborted.
- What the test checks: parsing `{ a: 1, ...obj, b: 2 }` should produce an object expression with a spread property in the middle.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'let base = { x: 1 }; let obj = { a: 1, ...base, b: 2 }; obj'
```

- Result: timed out after 8 seconds with no output.

## Analysis

- This is a parser-facing test, and the minimal eval repro hangs on the same object-spread feature even outside the Rust test harness.
- Because the hang happens before any value is printed, the front end is likely wedged before runtime execution.
- Inference: object literal parsing around `...spread` inside `{ ... }` is entering a non-progress loop or similar compiler stall.
- This failure matches the same object-spread cluster seen in the IR test and the milestone spread tests, so one parser fix may clear several tests.
