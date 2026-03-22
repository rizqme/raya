# Test Failures - 2026-03-21

## Commands run

```sh
cargo test --workspace
```

- This full run stalled in `raya-engine` and never completed.
- Log: `/tmp/raya-full-tests-20260321.log`

```sh
cargo test --workspace -- --skip expression_tests::test_parse_object_literal_with_spread --skip ir_comprehensive::objects::test_object_spread_lowers_to_field_copy --skip milestone_2_9_test::test_array_destructuring_with_defaults --skip milestone_2_9_test::test_complex_object_with_all_features --skip milestone_2_9_test::test_object_destructuring_with_defaults --skip milestone_2_9_test::test_object_spread
```

- This second run completed.
- Log: `/tmp/raya-full-tests-20260321-skip6.log`
- Result: all remaining tests passed except 7 `cli_http_e2e` failures.

## Current failing set

### Stuck / hanging tests (6)

1. `expression_tests::test_parse_object_literal_with_spread`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/expression_tests/mod.rs:282`
2. `ir_comprehensive::objects::test_object_spread_lowers_to_field_copy`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/ir_comprehensive/mod.rs:920`
3. `milestone_2_9_test::test_array_destructuring_with_defaults`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/milestone_2_9_test/mod.rs:41`
4. `milestone_2_9_test::test_object_destructuring_with_defaults`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/milestone_2_9_test/mod.rs:87`
5. `milestone_2_9_test::test_object_spread`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/milestone_2_9_test/mod.rs:222`
6. `milestone_2_9_test::test_complex_object_with_all_features`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-engine/tests/milestone_2_9_test/mod.rs:305`

### Failing HTTP E2E tests (7)

1. `cli_http_e2e::e2e_cli_http_diag_contract`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:393`
2. `cli_http_e2e::e2e_cli_http_echo_and_not_found_contracts`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:427`
3. `cli_http_e2e::e2e_cli_http_echo_method_not_allowed_contract`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:515`
4. `cli_http_e2e::e2e_cli_http_health_contract_and_artifacts`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:488`
5. `cli_http_e2e::e2e_cli_http_route_sequence_contracts`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:332`
6. `cli_http_e2e::e2e_cli_http_server_readiness_smoke`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:319`
7. `cli_http_e2e::e2e_cli_http_stress_workflow`
   Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:231`

## Cluster summary

### Cluster 1: spread / destructuring parser-or-compiler hangs

- `raya eval` reproduces timeouts for:
  - `let base = { x: 1, y: 2 }; let merged = { a: 0, ...base, y: 3 }; merged`
  - `let [x = 10, y = 20] = arr; x`
  - `let { x = 0, y = 0 } = partial; x`
  - `let obj = { ...defaults, name: "test", [computedKey]: value, nested: { ...nestedDefaults, x: 1 }, ...overrides }; obj`
- All of those timed out after 8 seconds with no output.
- Because parser-only tests and IR-lowering tests are both affected, the issue is probably in the front end around spread / destructuring-default parsing, with the compiler never reaching assertion time.

### Cluster 2: HTTP fixture server crashes on first request

- Fresh `raya eval` fetches against `/health`, `/diag?mode=contract`, `/echo` (GET and POST), `/missing`, and `/shutdown` all fail from the client side with:
  - `fetch.request: Invalid HTTP status line`
- The matching server process exits immediately after the first request with:
  - `Type error: Expected Object receiver for shape method call, got UnknownGcType`
- The shared server-side hot path is:
  - `/Users/rizqme/Workspace/raya/crates/raya-examples/fixtures/webapp/src/server.raya`
  - `/Users/rizqme/Workspace/raya/crates/raya-examples/fixtures/webapp/src/app/common.raya`
  - `/Users/rizqme/Workspace/raya/crates/raya-stdlib-posix/raya/http.raya`
- Inference: `HttpRequest` method dispatch (`req.path()`, `req.method()`, `req.query()`, `req.body()`, `req.header()`) is producing a runtime shape mismatch, so the server crashes before it can send a valid HTTP status line.

## Detailed analysis files

- Index: `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/README.md`
- One file per failing or stuck test lives under `/Users/rizqme/Workspace/raya/test_analysis/2026-03-21/`
