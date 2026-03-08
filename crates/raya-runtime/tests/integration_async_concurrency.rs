//! raya-runtime integration tests — async, concurrency, and streaming.

#[path = "e2e/harness.rs"]
mod harness;
pub use harness::*;
#[path = "e2e/async_await.rs"]
mod async_await;
#[path = "e2e/compress.rs"]
mod compress;
#[path = "e2e/concurrency.rs"]
mod concurrency;
#[path = "e2e/concurrency_edge_cases.rs"]
mod concurrency_edge_cases;
#[path = "e2e/stream.rs"]
mod stream;
