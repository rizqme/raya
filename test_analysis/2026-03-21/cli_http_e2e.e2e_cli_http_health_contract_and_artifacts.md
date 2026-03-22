# cli_http_e2e::e2e_cli_http_health_contract_and_artifacts

- Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:488`
- Status in skip-6 workspace run: failed.
- What the test checks: `/health` should return `200:OK:OK` and write `health.csv`, `health.tar`, and `health.ok`.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/health"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

- Client result: `fetch.request: Invalid HTTP status line`
- Server result: `Type error: Expected Object receiver for shape method call, got UnknownGcType`

## Analysis

- The `/health` path is the simplest successful contract in the suite, so its failure is a strong signal that the regression happens before route-specific artifact work.
- Inference: the handler probably never gets far enough to call `AppCommon.computeHealth(...)` reliably because request inspection crashes first.
