# cli_http_e2e::e2e_cli_http_diag_contract

- Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:393`
- Status in skip-6 workspace run: failed.
- What the test checks: `/diag?mode=contract` should return a 200 JSON body with `allPass: true` and preserve the query string.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/diag?mode=contract"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

- Client result: `fetch.request: Invalid HTTP status line`
- Server result: `Type error: Expected Object receiver for shape method call, got UnknownGcType`

## Analysis

- The `/diag` handler calls `req.path()` and `req.query()` in `/Users/rizqme/Workspace/raya/crates/raya-examples/fixtures/webapp/src/server.raya:46-47`.
- Inference: one of those `HttpRequest` shape-method calls is crashing the server before any JSON body is emitted, so the client never gets a valid status line to parse.
