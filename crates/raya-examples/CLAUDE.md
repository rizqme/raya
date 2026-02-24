# raya-examples

Example Raya applications plus end-to-end tests.

## Purpose

- Keep realistic Raya sample apps in one place.
- Validate them through CLI-driven integration tests.
- Exercise broad stdlib coverage in real app flows.

## Current Fixture

- `fixtures/webapp/` — local loopback web app:
  - `src/server.raya` runs an HTTP server via `std:http`
  - `src/client.raya` calls it via `std:fetch`
  - Entry scripts execute top-level statements directly (no implicit `main()` call)
  - `src/app/common.raya` uses many std modules (`logger`, `math`, `crypto`,
    `time`, `path`, `stream`, `fs`, `env`, `process`, `os`, `encoding`,
    `semver`, `archive`)
- `fixtures/systems/` — second-suite system scenarios:
  - `stateful_data.raya` (stateful/persistence style flow)
  - `concurrency_cancel.raya` (channel/task-oriented flow; cancel validated via `cancel()` + `isCancelled()`)
  - `tcp_protocol.raya` (TCP protocol handshake flow with multi-round line framing checks)
  - `template_archive.raya` (template + archive/compress pipeline)
  - `fault_injection.raya` (resilience via invalid-input handling)

## Tests

- `tests/cli_http_e2e.rs` spawns `raya` through `cargo run -p raya-cli -- run ...`
  and verifies end-to-end HTTP contracts plus generated artifacts.
  It now waits for a valid socket address in `server.ready` (not just file
  existence) before running client requests, and retries transient
  startup failures (`fetch.request: Connection refused`, occasional
  `Stack underflow` on first client call) to avoid startup races in CI.
  Tests are serialized via a suite lock and always clean up spawned servers on
  panic through a `Drop`-based server handle.
- Current scenarios include stress workflow, diagnostics contract, echo/not-found
  contract, health+artifact contract, and echo method-not-allowed contract.
- Fixture suites are now one test file per fixture (full suite style):
  - `tests/fixture_todo_kv_e2e.rs`
  - `tests/fixture_worker_queue_e2e.rs`
  - `tests/fixture_tcp_chat_e2e.rs`
  - `tests/fixture_template_pipeline_e2e.rs`
  - `tests/fixture_pkg_workflow_e2e.rs`
  - `tests/fixture_fault_injection_e2e.rs`
  - Shared helpers in `tests/common/mod.rs` provide CLI timeout guard (20s) and
    summary parsing.
