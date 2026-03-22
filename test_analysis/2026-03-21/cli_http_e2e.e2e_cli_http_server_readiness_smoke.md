# cli_http_e2e::e2e_cli_http_server_readiness_smoke

- Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:319`
- Status in skip-6 workspace run: failed.
- What the test checks: the server should boot, write `server.ready`, then shut down cleanly through the helper.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/shutdown"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

- Client result: `fetch.request: Invalid HTTP status line`
- Server result: `Type error: Expected Object receiver for shape method call, got UnknownGcType`

## Analysis

- The readiness file is created before requests start, so boot itself is probably fine.
- This test most likely fails during `shutdown_server(...)`, which sends the `/shutdown` request after the ready-file assertion.
- Inference: the regression is not in readiness bookkeeping. It is in request handling after `server.accept()`, where `HttpRequest` methods are used.
