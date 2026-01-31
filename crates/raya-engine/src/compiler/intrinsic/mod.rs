//! Compiler Intrinsics
//!
//! This module provides compile-time code generation for special built-in
//! functions that require type-aware handling.
//!
//! Currently supported intrinsics:
//! - `JSON.stringify()` / `JSON.parse()` - Runtime JSON operations
//! - `JSON.encode<T>()` - Type-safe JSON encoding with compile-time codegen
//! - `JSON.decode<T>()` - Type-safe JSON decoding with compile-time codegen
//!
//! Note: Field mapping with @json decorator will be added in a future milestone.

pub mod json;

pub use json::JsonIntrinsic;
