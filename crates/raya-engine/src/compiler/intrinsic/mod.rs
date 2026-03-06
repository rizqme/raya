//! Compiler Intrinsics
//!
//! This module provides compile-time code generation for special built-in
//! functions that require type-aware handling.
//!
//! Currently supported intrinsics:
//! - `JSON.stringify()` / `JSON.parse()` - Runtime JSON operations
//!
//! Note: Field mapping with @json decorator will be added in a future milestone.

pub mod json;

pub use json::JsonIntrinsic;
