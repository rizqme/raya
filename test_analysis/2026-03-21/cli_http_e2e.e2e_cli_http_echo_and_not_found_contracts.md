# cli_http_e2e::e2e_cli_http_echo_and_not_found_contracts

- Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:427`
- Status in skip-6 workspace run: failed.
- What the test checks: `POST /echo` should return the echoed JSON contract and `GET /missing` should return `404:not-found`.

## Eval validation

Fresh eval probes for both paths reproduced the same crash pattern:

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.request("POST", "http://127.0.0.1:<port>/echo", "payload-e2e", "X-Trace: e2e-123"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/missing"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

- Client result each time: `fetch.request: Invalid HTTP status line`
- Server result each time: `Type error: Expected Object receiver for shape method call, got UnknownGcType`

## Analysis

- `/echo` exercises the widest `HttpRequest` surface area because it touches `method`, `path`, `query`, `body`, and `header` through `AppCommon.computeEcho(...)`.
- `/missing` should be the simplest handler, but it still calls `req.path()` first.
- Inference: the root cause is likely upstream of route-specific business logic. Any request that reaches `HttpRequest` method dispatch seems able to crash the server.
