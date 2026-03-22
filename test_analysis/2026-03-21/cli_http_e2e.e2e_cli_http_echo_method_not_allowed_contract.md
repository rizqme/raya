# cli_http_e2e::e2e_cli_http_echo_method_not_allowed_contract

- Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:515`
- Status in skip-6 workspace run: failed.
- What the test checks: `GET /echo` should return `405:method-not-allowed`.

## Eval validation

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/echo"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

- Client result: `fetch.request: Invalid HTTP status line`
- Server result: `Type error: Expected Object receiver for shape method call, got UnknownGcType`

## Analysis

- The route needs only `req.path()` and `req.method()` to decide between 201 and 405.
- Inference: the server is crashing before it can even reach the simple method gate, which again points at `HttpRequest` method dispatch rather than the 405 branch itself.
