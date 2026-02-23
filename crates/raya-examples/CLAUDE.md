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
  - `tcp_protocol.raya` (TCP protocol handshake flow)
  - `template_archive.raya` (template + archive/compress pipeline)
  - `fault_injection.raya` (resilience via invalid-input handling)

## Tests

- `tests/cli_http_e2e.rs` spawns `raya` through `cargo run -p raya-cli -- run ...`
  and verifies end-to-end HTTP contracts plus generated artifacts.
- Current scenarios include stress workflow, diagnostics contract, echo/not-found
  contract, health+artifact contract, and echo method-not-allowed contract.
- `tests/cli_second_suite_e2e.rs` runs the second test suite covering stateful,
  concurrency, TCP protocol, template/archive pipeline, package script workflow,
  and fault-injection flows.
