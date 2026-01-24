//! VM integration tests for JSON native module
//!
//! These tests verify that the JSON module can be registered with the VM
//! and that its functions can be invoked with actual Value manipulation.

use raya_core::ffi::NativeValue;
use raya_core::value::Value;
use raya_core::vm::VmContext;
use raya_stdlib::json_module_init;
use std::sync::Arc;

#[test]
fn test_register_json_module_with_vm() {
    // Create VM context
    let mut context = VmContext::new();

    // Initialize JSON module
    let module = Arc::new(json_module_init());

    // Register module with VM
    let result = context.register_native_module(module.clone());
    assert!(result.is_ok(), "Failed to register JSON module: {:?}", result);

    // Verify module is registered
    assert!(context.native_module_registry().is_loaded("std:json"));
}

#[test]
fn test_register_json_module_with_custom_name() {
    let mut context = VmContext::new();
    let module = Arc::new(json_module_init());

    // Register with custom name
    let result = context.register_native_module_as("json", module.clone());
    assert!(result.is_ok());

    // Verify registered under custom name
    assert!(context.native_module_registry().is_loaded("json"));
}

#[test]
fn test_get_registered_json_module() {
    let mut context = VmContext::new();
    let module = Arc::new(json_module_init());

    context.register_native_module(module.clone()).unwrap();

    // Get the registered module
    let retrieved = context.native_module_registry().get("std:json");
    assert!(retrieved.is_some());

    let retrieved_module = retrieved.unwrap();
    assert_eq!(retrieved_module.name(), "std:json");
    assert_eq!(retrieved_module.functions().len(), 3);
}

#[test]
fn test_call_json_parse_placeholder_with_value() {
    let module = json_module_init();

    // Get the parse function
    let parse_fn = module
        .get_function("parse")
        .expect("parse function should be registered");

    // Create a placeholder i32 value (since String marshalling not implemented)
    let dummy_value = Value::i32(42);
    let dummy_arg = NativeValue::from_value(dummy_value);
    let args = [dummy_arg];

    // Call the function
    let result = unsafe { parse_fn(args.as_ptr(), args.len()) };

    // Verify result is a boolean true (placeholder behavior)
    let result_value = unsafe { result.as_value() };
    assert!(result_value.is_bool(), "Expected boolean result");
    assert_eq!(result_value.as_bool(), Some(true), "Expected true from placeholder");
}

#[test]
fn test_call_json_stringify_placeholder_with_value() {
    let module = json_module_init();

    let stringify_fn = module
        .get_function("stringify")
        .expect("stringify function should be registered");

    let dummy_value = Value::i32(123);
    let dummy_arg = NativeValue::from_value(dummy_value);
    let args = [dummy_arg];

    let result = unsafe { stringify_fn(args.as_ptr(), args.len()) };

    // Verify result
    let result_value = unsafe { result.as_value() };
    assert!(result_value.is_bool());
    assert_eq!(result_value.as_bool(), Some(true));
}

#[test]
fn test_call_json_is_valid_placeholder_with_value() {
    let module = json_module_init();

    let is_valid_fn = module
        .get_function("isValid")
        .expect("isValid function should be registered");

    let dummy_value = Value::i32(0);
    let dummy_arg = NativeValue::from_value(dummy_value);
    let args = [dummy_arg];

    let result = unsafe { is_valid_fn(args.as_ptr(), args.len()) };

    // Verify result
    let result_value = unsafe { result.as_value() };
    assert!(result_value.is_bool());
    assert_eq!(result_value.as_bool(), Some(true));
}

#[test]
fn test_multiple_function_calls_with_different_values() {
    let module = json_module_init();

    // Call parse with different i32 values
    let parse_fn = module.get_function("parse").unwrap();
    for i in 0..10 {
        let value = Value::i32(i);
        let arg = NativeValue::from_value(value);
        let args = [arg];
        let result = unsafe { parse_fn(args.as_ptr(), args.len()) };

        let result_value = unsafe { result.as_value() };
        assert_eq!(result_value.as_bool(), Some(true));
    }

    // Call stringify with different i32 values
    let stringify_fn = module.get_function("stringify").unwrap();
    for i in 0..10 {
        let value = Value::i32(i * 10);
        let arg = NativeValue::from_value(value);
        let args = [arg];
        let result = unsafe { stringify_fn(args.as_ptr(), args.len()) };

        let result_value = unsafe { result.as_value() };
        assert_eq!(result_value.as_bool(), Some(true));
    }
}

#[test]
fn test_call_with_correct_value_types() {
    let module = json_module_init();

    // Test parse with i32 (correct type)
    let parse_fn = module.get_function("parse").unwrap();
    let i32_val = Value::i32(42);
    let arg = NativeValue::from_value(i32_val);
    let result = unsafe { parse_fn([arg].as_ptr(), 1) };
    let result_value = unsafe { result.as_value() };
    assert!(result_value.is_bool());
    assert_eq!(result_value.as_bool(), Some(true));

    // Test stringify with i32 (correct type)
    let stringify_fn = module.get_function("stringify").unwrap();
    let i32_val = Value::i32(123);
    let arg = NativeValue::from_value(i32_val);
    let result = unsafe { stringify_fn([arg].as_ptr(), 1) };
    let result_value = unsafe { result.as_value() };
    assert!(result_value.is_bool());
    assert_eq!(result_value.as_bool(), Some(true));

    // Test isValid with i32 (correct type)
    let is_valid_fn = module.get_function("isValid").unwrap();
    let i32_val = Value::i32(0);
    let arg = NativeValue::from_value(i32_val);
    let result = unsafe { is_valid_fn([arg].as_ptr(), 1) };
    let result_value = unsafe { result.as_value() };
    assert!(result_value.is_bool());
    assert_eq!(result_value.as_bool(), Some(true));
}

#[test]
fn test_type_mismatch_returns_error() {
    let module = json_module_init();
    let parse_fn = module.get_function("parse").unwrap();

    // Test with f64 (wrong type - function expects i32)
    // Should return error (null for now)
    let f64_val = Value::f64(3.14);
    let arg = NativeValue::from_value(f64_val);
    let result = unsafe { parse_fn([arg].as_ptr(), 1) };
    let result_value = unsafe { result.as_value() };
    // Error returns null (since NativeValue::error() returns null for now)
    assert!(result_value.is_null());

    // Test with bool (wrong type)
    let bool_val = Value::bool(true);
    let arg = NativeValue::from_value(bool_val);
    let result = unsafe { parse_fn([arg].as_ptr(), 1) };
    let result_value = unsafe { result.as_value() };
    assert!(result_value.is_null());

    // Test with null (wrong type)
    let null_val = Value::null();
    let arg = NativeValue::from_value(null_val);
    let result = unsafe { parse_fn([arg].as_ptr(), 1) };
    let result_value = unsafe { result.as_value() };
    assert!(result_value.is_null());
}

#[test]
fn test_concurrent_module_access() {
    use std::thread;

    let module = Arc::new(json_module_init());
    let mut handles = vec![];

    // Spawn multiple threads calling functions concurrently
    for thread_id in 0..4 {
        let module_clone = Arc::clone(&module);
        let handle = thread::spawn(move || {
            let parse_fn = module_clone.get_function("parse").unwrap();

            for i in 0..100 {
                let value = Value::i32(thread_id * 100 + i);
                let arg = NativeValue::from_value(value);
                let args = [arg];
                let result = unsafe { parse_fn(args.as_ptr(), args.len()) };

                let result_value = unsafe { result.as_value() };
                assert_eq!(result_value.as_bool(), Some(true));
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }
}

// ============================================================================
// TODO: Once String marshalling is implemented, add these real JSON tests:
// ============================================================================

// #[test]
// fn test_parse_valid_json_object() {
//     let module = json_module_init();
//     let parse_fn = module.get_function("parse").unwrap();
//
//     // Create JSON string value
//     let json_str = Value::string(r#"{"name":"Alice","age":30}"#);
//     let arg = NativeValue::from_value(json_str);
//     let args = [arg];
//
//     // Call parse
//     let result = unsafe { parse_fn(args.as_ptr(), args.len()) };
//     let result_value = unsafe { result.as_value() };
//
//     // Verify result is an object
//     assert!(result_value.is_object());
// }

// #[test]
// fn test_stringify_object() {
//     let module = json_module_init();
//     let stringify_fn = module.get_function("stringify").unwrap();
//
//     // Create an object value
//     let mut obj = Object::new();
//     obj.set_property("name", Value::string("Alice"));
//     obj.set_property("age", Value::i32(30));
//     let obj_value = Value::object(obj);
//
//     let arg = NativeValue::from_value(obj_value);
//     let args = [arg];
//
//     let result = unsafe { stringify_fn(args.as_ptr(), args.len()) };
//     let result_value = unsafe { result.as_value() };
//
//     assert!(result_value.is_string());
//     let json = result_value.as_string().unwrap();
//     assert_eq!(json, r#"{"name":"Alice","age":30}"#);
// }

// #[test]
// fn test_round_trip_parse_and_stringify() {
//     let module = json_module_init();
//     let parse_fn = module.get_function("parse").unwrap();
//     let stringify_fn = module.get_function("stringify").unwrap();
//
//     let original_json = r#"{"name":"Alice","age":30,"active":true}"#;
//     let json_value = Value::string(original_json);
//     let arg = NativeValue::from_value(json_value);
//
//     // Parse
//     let parsed = unsafe { parse_fn([arg].as_ptr(), 1) };
//     let parsed_value = unsafe { parsed.as_value() };
//     assert!(parsed_value.is_object());
//
//     // Stringify
//     let arg = NativeValue::from_value(*parsed_value);
//     let stringified = unsafe { stringify_fn([arg].as_ptr(), 1) };
//     let stringified_value = unsafe { stringified.as_value() };
//
//     assert!(stringified_value.is_string());
// }
