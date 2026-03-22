# milestone_2_9_test::test_object_spread

- Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/milestone_2_9_test/mod.rs:222`
- Status in full workspace run: hung until the workspace run was aborted.
- What the test checks: parsing `let merged = { ...obj1, ...obj2 };` should produce object properties that include spread entries.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'let merged = { ...obj1, ...obj2 }; merged'
```

- Result: timed out after 8 seconds with no output.

## Analysis

- This is the cleanest object-spread repro in the current failure set, and it hangs by itself under `raya eval`.
- The matching parser test and IR test both stall on object-spread syntax too.
- Inference: object spread parsing is a primary root cause for this cluster, not just a downstream assertion failure in one test file.
