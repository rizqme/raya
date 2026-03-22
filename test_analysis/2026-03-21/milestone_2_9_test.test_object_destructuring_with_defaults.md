# milestone_2_9_test::test_object_destructuring_with_defaults

- Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/milestone_2_9_test/mod.rs:87`
- Status in full workspace run: hung until the workspace run was aborted.
- What the test checks: parsing `let { x = 0, y = 0 } = partial;` should retain default initializers on object pattern properties.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'let { x = 0, y = 0 } = partial; x'
```

- Result: timed out after 8 seconds with no output.

## Analysis

- This hangs on a minimal object destructuring default pattern, which lines up with the stalled Rust test.
- Plain object destructuring without defaults is not in the current failure set, so the `=` default branch is the important difference.
- Inference: object-pattern parsing around property defaults is likely sharing the same front-end progress bug as array-pattern defaults and object spread.
