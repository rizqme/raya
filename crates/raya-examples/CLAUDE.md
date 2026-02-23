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
  - `src/app/common.raya` uses many std modules (`logger`, `math`, `crypto`,
    `time`, `path`, `stream`, `fs`, `env`, `process`, `os`, `encoding`,
    `semver`, `archive`)

## Tests

- `tests/cli_http_e2e.rs` spawns `raya` through `cargo run -p raya-cli -- run ...`
  and verifies a full HTTP request/response roundtrip plus generated artifacts.
