# raya-stdlib-node

Node compatibility standard library sources for Raya.

This crate intentionally contains `.raya` module implementations and shims only.
It does not add new native Rust bindings.

Current modules:
- fs
- fs/promises
- path
- os
- process
- dns
- net
- http
- https
- crypto
- url
- stream
- events (EventEmitter shim)
- assert
- assert/strict
- util
- module
- child_process
- test
- test/reporters
- timers
- timers/promises
- buffer
- string_decoder
- stream/promises
- stream/web
- worker_threads
- vm
- http2
- inspector
- inspector/promises
- async_hooks
- diagnostics_channel
- v8
- dgram
- cluster
- repl
- perf_hooks
- sqlite
