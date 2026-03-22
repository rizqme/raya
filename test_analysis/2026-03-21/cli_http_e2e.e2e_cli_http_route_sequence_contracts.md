# cli_http_e2e::e2e_cli_http_route_sequence_contracts

- Source: `/Users/rizqme/Workspace/raya/crates/raya-examples/tests/cli_http_e2e/mod.rs:332`
- Status in skip-6 workspace run: failed.
- What the test checks: `/health`, `/diag`, `/missing`, `/echo`, and `/shutdown` should all return the expected contract outputs in sequence.

## Eval validation

Fresh eval probes for representative routes reproduced the failure:

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/health"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.get("http://127.0.0.1:<port>/missing"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

```sh
target/debug/raya eval --node-compat --mode ts --print 'import fetch from "std:fetch"; let res = fetch.request("POST", "http://127.0.0.1:<port>/echo", "payload-e2e", "X-Trace: seq-1"); let out = `${res.status()}:${res.statusText()}:${res.text()}`; res.release(); out'
```

- Client result each time: `fetch.request: Invalid HTTP status line`
- Server result each time: `Type error: Expected Object receiver for shape method call, got UnknownGcType`

## Analysis

- Because the first route already crashes the server, the route-sequence contract cannot progress to a meaningful multi-route check.
- Inference: this is one shared server crash, not separate regressions in the `/health`, `/diag`, `/missing`, and `/echo` handlers.
