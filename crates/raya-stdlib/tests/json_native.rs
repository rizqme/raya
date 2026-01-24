//! Integration tests for JSON native module
//!
//! These tests verify that the JSON module can be loaded as a native module
//! and that its functions are correctly registered and callable.

use raya_stdlib::json_module_init;

#[test]
fn test_json_module_initialization() {
    // Initialize the JSON module
    let module = json_module_init();

    // Verify module metadata
    assert_eq!(module.name(), "std:json");
    assert!(!module.version().is_empty());
}

#[test]
fn test_json_module_has_parse_function() {
    let module = json_module_init();

    // Verify parse function is registered
    assert!(
        module.get_function("parse").is_some(),
        "parse function should be registered"
    );
}

#[test]
fn test_json_module_has_stringify_function() {
    let module = json_module_init();

    // Verify stringify function is registered
    assert!(
        module.get_function("stringify").is_some(),
        "stringify function should be registered"
    );
}

#[test]
fn test_json_module_has_is_valid_function() {
    let module = json_module_init();

    // Verify isValid function is registered
    assert!(
        module.get_function("isValid").is_some(),
        "isValid function should be registered"
    );
}

#[test]
fn test_json_module_function_count() {
    let module = json_module_init();

    // Verify we have exactly 3 functions registered
    let functions = module.functions();
    assert_eq!(
        functions.len(),
        3,
        "JSON module should have exactly 3 functions"
    );

    // Verify all expected functions are present
    let function_names: Vec<&str> = functions.iter().map(|s| s.as_str()).collect();
    assert!(function_names.contains(&"parse"));
    assert!(function_names.contains(&"stringify"));
    assert!(function_names.contains(&"isValid"));
}

#[test]
fn test_json_module_can_be_loaded_multiple_times() {
    // Verify that initializing the module multiple times works
    let module1 = json_module_init();
    let module2 = json_module_init();

    assert_eq!(module1.name(), module2.name());
    assert_eq!(module1.version(), module2.version());
    assert_eq!(module1.functions().len(), module2.functions().len());
}

// TODO: Once String marshalling is implemented, add tests that actually call the functions:
// #[test]
// fn test_parse_valid_json() {
//     let module = json_module_init();
//     let parse_fn = module.get_function("parse").unwrap();
//
//     // Call parse with valid JSON string
//     let args = vec![NativeValue::from_string(r#"{"name":"Alice"}"#)];
//     let result = parse_fn(&args);
//
//     assert!(result.is_ok());
// }
//
// #[test]
// fn test_parse_invalid_json() {
//     let module = json_module_init();
//     let parse_fn = module.get_function("parse").unwrap();
//
//     // Call parse with invalid JSON string
//     let args = vec![NativeValue::from_string("{invalid}")];
//     let result = parse_fn(&args);
//
//     assert!(result.is_err());
// }
