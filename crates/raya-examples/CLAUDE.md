# raya-examples

This crate collects realistic example applications and test fixtures. It exists to exercise the toolchain the way users will, not just at the unit-test level.

## What This Crate Owns

- Fixture projects used by integration tests.
- Helper functions that expose fixture paths to tests.
- Scenario-driven tests that exercise runtime, CLI, stdlib, and package flows together.

## Layout

- `fixtures/`: example apps and system scenarios.
- `src/lib.rs`: fixture path helpers and shared constants such as well-known test ports.
- `tests/`: integration suites organized by fixture or scenario.

## Start Here When

- You need a realistic reproduction of a user workflow.
- A bug only appears when multiple crates interact together.
- You want to add a new end-to-end scenario rather than a narrow unit test.
