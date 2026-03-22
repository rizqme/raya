# ir_comprehensive::objects::test_object_spread_lowers_to_field_copy

- Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/ir_comprehensive/mod.rs:920`
- Status in full workspace run: hung until the workspace run was aborted.
- What the test checks: lowering `{ a: 0, ...base, y: 3 }` should produce IR that materializes an object and copies fields.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'let base = { x: 1, y: 2 }; let merged = { a: 0, ...base, y: 3 }; merged'
```

- Result: timed out after 8 seconds with no output.

## Analysis

- The eval repro uses the same spread shape as the IR test and hangs before printing, so the compiler never reaches normal execution.
- Although this test asserts on lowered IR, the matching parser-only object-spread test also hangs, which points earlier than IR printing.
- Inference: the failure is probably not specific to `load_field` / `store_field` generation. It is more likely a shared front-end issue in object spread parsing that prevents lowering from finishing.
