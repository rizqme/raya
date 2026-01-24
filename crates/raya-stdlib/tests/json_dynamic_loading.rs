//! Integration test for dynamically loading the JSON stdlib as a native module
//!
//! This test verifies that raya-stdlib can be built as a dynamic library
//! and loaded using the Library loader.

use std::env;
use std::path::PathBuf;

#[test]
fn test_json_module_dynamic_loading() {
    // Find the compiled dynamic library
    let mut lib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    lib_path.pop(); // Go to workspace root
    lib_path.pop();
    lib_path.push("target");
    lib_path.push("debug");

    #[cfg(target_os = "macos")]
    lib_path.push("libraya_stdlib.dylib");

    #[cfg(target_os = "linux")]
    lib_path.push("libraya_stdlib.so");

    #[cfg(target_os = "windows")]
    lib_path.push("raya_stdlib.dll");

    // Skip test if library doesn't exist (might not be built yet)
    if !lib_path.exists() {
        eprintln!(
            "Skipping test: Library not found at {:?}. Build with 'cargo build -p raya-stdlib'",
            lib_path
        );
        return;
    }

    // Load the library
    let library = raya_ffi::Library::open(&lib_path)
        .expect("Failed to load raya-stdlib dynamic library");

    // Load the module
    let module = library.load_module().expect("Failed to load module");

    // Verify module metadata
    assert_eq!(module.name(), "std:json");
    assert!(!module.version().is_empty());

    // Verify functions are registered
    assert!(module.get_function("parse").is_some());
    assert!(module.get_function("stringify").is_some());
    assert!(module.get_function("isValid").is_some());

    // Verify function count
    assert_eq!(module.functions().len(), 3);
}

#[test]
fn test_module_initialization_export() {
    // This test verifies that the raya_module_init symbol is properly exported
    // by the proc-macro and can be found

    let mut lib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    lib_path.pop();
    lib_path.pop();
    lib_path.push("target");
    lib_path.push("debug");

    #[cfg(target_os = "macos")]
    lib_path.push("libraya_stdlib.dylib");

    #[cfg(target_os = "linux")]
    lib_path.push("libraya_stdlib.so");

    #[cfg(target_os = "windows")]
    lib_path.push("raya_stdlib.dll");

    if !lib_path.exists() {
        eprintln!(
            "Skipping test: Library not found at {:?}",
            lib_path
        );
        return;
    }

    // Try to load the library
    let library = raya_ffi::Library::open(&lib_path)
        .expect("Failed to load library");

    // Try to get the init symbol directly
    unsafe {
        type InitFn = extern "C" fn() -> *mut raya_core::ffi::NativeModule;
        let _init_fn: InitFn = library
            .get("raya_module_init")
            .expect("raya_module_init symbol should be exported");
    }
}
