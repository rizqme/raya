# milestone_2_9_test::test_complex_object_with_all_features

- Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/milestone_2_9_test/mod.rs:305`
- Status in full workspace run: hung until the workspace run was aborted.
- What the test checks: a mixed object literal using spread, computed properties, and nested objects should parse successfully.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'let obj = { ...defaults, name: "test", [computedKey]: value, nested: { ...nestedDefaults, x: 1 }, ...overrides }; obj'
```

- Result: timed out after 8 seconds with no output.

## Analysis

- The complex repro hangs on its own, but simpler computed-property tests were not part of the current failure set.
- Simpler object-spread repros also hang, so spread support is the strongest common denominator here.
- Inference: computed keys are probably not the primary blocker. The nested and top-level spread handling is more likely to be trapping the parser or early compiler pipeline.
