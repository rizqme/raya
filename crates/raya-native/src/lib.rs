// raya-native: Rust ergonomic API for writing Raya native modules
//
// Provides proc-macros for automatic FFI wrapping:
// - #[function] - Wraps a Rust function for native module use
// - #[module] - Defines a native module
//
// Example:
// ```
// use raya_native::*;
//
// #[function]
// fn add(a: i32, b: i32) -> i32 {
//     a + b
// }
//
// #[module]
// mod math {
//     fn add(a: i32, b: i32) -> i32;
//     fn subtract(a: i32, b: i32) -> i32;
// }
// ```

use proc_macro::TokenStream;
use syn::{parse_macro_input, ItemFn};

mod function;
mod module;
#[allow(dead_code)]
mod traits;

/// Marks a Rust function as a native module function.
///
/// Automatically generates FFI wrapper code that:
/// - Converts RayaValue arguments to Rust types (FromRaya)
/// - Converts the return value to RayaValue (ToRaya)
/// - Catches panics and converts them to Raya errors
/// - Handles GC pinning/unpinning
///
/// # Example
///
/// ```ignore
/// #[function]
/// fn greet(name: String) -> String {
///     format!("Hello, {}!", name)
/// }
/// ```
///
/// This generates a wrapper function `greet_ffi` that can be registered
/// with the VM's native module system.
#[proc_macro_attribute]
pub fn function(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    function::expand_function(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Defines a native module initialization function.
///
/// Must be applied to a function named `init()` that returns `NativeModule`.
///
/// Automatically generates:
/// - FFI entry point `raya_module_init()` for dynamic library loading
/// - Cleanup function `raya_module_cleanup()` for resource management
///
/// # Example
///
/// ```ignore
/// use raya_sdk::NativeModule;
/// use raya_native::{function, module};
///
/// #[function]
/// fn add(a: i32, b: i32) -> i32 {
///     a + b
/// }
///
/// #[module]
/// fn init() -> NativeModule {
///     let mut module = NativeModule::new("math", "1.0.0");
///     module.register_function("add", add_ffi);
///     module
/// }
/// ```
///
/// This generates `raya_module_init()` which is called by the VM when
/// loading the dynamic library.
#[proc_macro_attribute]
pub fn module(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    module::expand_module(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
