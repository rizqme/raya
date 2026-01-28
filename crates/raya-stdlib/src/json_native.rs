//! Native module wrapper for JSON functionality
//!
//! This module exposes Raya's custom JSON implementation through the FFI interface.

use raya_sdk::NativeModule;
use raya_native::{function, module};

// TODO: Once String marshalling is implemented, we can expose the real JSON functions
// For now, these are placeholders that demonstrate the module structure

/// Placeholder for JSON.parse() - will use crate::json::parse() once String marshalling exists
///
/// # Arguments
/// * `_dummy` - Placeholder until String support is ready
///
/// # Returns
/// * Always returns true for now
#[function]
fn parse(_dummy: i32) -> bool {
    // TODO: Call crate::json::parse() once we have String marshalling
    // let input = RayaString { data: json_string };
    // let result = crate::json::parse(&input, gc)?;
    true
}

/// Placeholder for JSON.stringify() - will use crate::json::stringify() once String marshalling exists
///
/// # Arguments
/// * `_dummy` - Placeholder until String support is ready
///
/// # Returns
/// * Always returns true for now
#[function]
fn stringify(_dummy: i32) -> bool {
    // TODO: Call crate::json::stringify() once we have String marshalling
    // let result = crate::json::stringify(&json_value, gc)?;
    true
}

/// Check if a string is valid JSON (placeholder)
///
/// # Arguments
/// * `_dummy` - Placeholder until String support is ready
///
/// # Returns
/// * true if valid JSON, false otherwise
#[function]
fn is_valid(_dummy: i32) -> bool {
    // TODO: Try to parse and return true/false
    true
}

/// Module initialization
///
/// Exports the JSON functions as a native module.
#[module]
pub fn init() -> NativeModule {
    let mut module = NativeModule::new("std:json", env!("CARGO_PKG_VERSION"));

    // Register placeholder functions
    // Once String marshalling is implemented, these will call the real implementations
    module.register_function("parse", parse_ffi);
    module.register_function("stringify", stringify_ffi);
    module.register_function("isValid", is_valid_ffi);

    module
}
