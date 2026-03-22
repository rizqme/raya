# cli_http_e2e::e2e_cli_http_stress_workflow

- Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:231`
- Status in skip-6 workspace run: failed.
- Reported symptom in the completed test run: client-side HTTP calls fail with `fetch.request: Invalid HTTP status line`.
- What the test checks: the full health/diag/echo/missing/shutdown workflow should succeed and produce all expected artifacts.

## Eval validation

Fresh route-specific evals all reproduced the same failure pattern:

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/health"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/diag?mode=contract"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.request("POST", "http://127.0.0.1:<port>/echo", "payload-e2e", "X-Trace: e2e-123"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

- Client result each time: `fetch.request: Invalid HTTP status line`
- Server result each time: `Type error: Expected Object receiver for shape method call, got UnknownGcType`

## Analysis

- The stress test depends on every route working. The server currently dies on the first request, so the workflow never has a chance to create consistent artifacts.
- Inference: the real failure is server-side, not an HTTP parser bug in the client. The client is only reporting a broken status line because the server crashes before it can finish a response.
- Likely hot path: `req.path()`, `req.method()`, `req.query()`, `req.body()`, and `req.header()` calls in `/Users/rizqme/Workspace/raya/crates/raya-examples/fixtures/webapp/src/server.raya` and `/Users/rizqme/Workspace/raya/crates/raya-examples/fixtures/webapp/src/app/common.raya`.
