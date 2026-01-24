//! Integration tests for module loading
//!
//! Tests the complete module loading pipeline from .rbin bytes to registered modules.

use raya_bytecode::module::{Function, Module};
use raya_bytecode::Opcode;
use raya_core::vm::{InnerVm, VmOptions};

/// Helper to create a simple test module
fn create_test_module(name: &str) -> Module {
    let mut module = Module::new(name.to_string());

    // Add a simple function
    let func = Function {
        name: "test_func".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    };
    module.functions.push(func);

    module
}

#[test]
fn test_load_simple_module() {
    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create and encode a module
    let module = create_test_module("test_module");
    let bytes = module.encode();

    // Load the module
    let result = vm.load_rbin_bytes(&bytes);
    assert!(result.is_ok(), "Failed to load module: {:?}", result);
}

#[test]
fn test_module_checksum_verification() {
    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create and encode a module
    let module = create_test_module("test_module");
    let mut bytes = module.encode();

    // Corrupt the checksum by modifying a byte in the payload
    if bytes.len() > 50 {
        bytes[50] ^= 0xFF; // Flip bits
    }

    // Try to load the corrupted module
    let result = vm.load_rbin_bytes(&bytes);
    assert!(result.is_err(), "Should fail with corrupted checksum");

    // Verify it's a checksum error
    match result {
        Err(e) => {
            let error_msg = format!("{}", e);
            assert!(
                error_msg.contains("ChecksumMismatch") || error_msg.contains("Checksum mismatch"),
                "Expected checksum error, got: {}",
                error_msg
            );
        }
        Ok(_) => panic!("Expected checksum error"),
    }
}

#[test]
fn test_duplicate_module_loading() {
    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create and encode a module
    let module = create_test_module("test_module");
    let bytes = module.encode();

    // Load the module twice
    let result1 = vm.load_rbin_bytes(&bytes);
    assert!(result1.is_ok(), "First load failed: {:?}", result1);

    let result2 = vm.load_rbin_bytes(&bytes);
    assert!(
        result2.is_ok(),
        "Second load failed (should be idempotent): {:?}",
        result2
    );
}

#[test]
fn test_load_multiple_modules() {
    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create and load multiple modules
    for i in 0..5 {
        let module = create_test_module(&format!("module_{}", i));
        let bytes = module.encode();

        let result = vm.load_rbin_bytes(&bytes);
        assert!(result.is_ok(), "Failed to load module {}: {:?}", i, result);
    }
}

#[test]
fn test_invalid_magic_number() {
    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create a module with invalid magic
    let mut module = create_test_module("test_module");
    module.magic = *b"XXXX"; // Invalid magic
    let bytes = module.encode();

    // Try to load it
    let result = vm.load_rbin_bytes(&bytes);
    assert!(result.is_err(), "Should fail with invalid magic");

    match result {
        Err(e) => {
            let error_msg = format!("{}", e);
            assert!(
                error_msg.contains("InvalidMagic") || error_msg.contains("Invalid magic"),
                "Expected magic error, got: {}",
                error_msg
            );
        }
        Ok(_) => panic!("Expected invalid magic error"),
    }
}

#[test]
fn test_module_with_exports() {
    use raya_bytecode::module::{Export, SymbolType};

    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create a module with exports
    let mut module = create_test_module("exported_module");

    // Add an export
    module.exports.push(Export {
        name: "test_func".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
    });

    let bytes = module.encode();

    // Load the module
    let result = vm.load_rbin_bytes(&bytes);
    assert!(
        result.is_ok(),
        "Failed to load module with exports: {:?}",
        result
    );
}

#[test]
fn test_module_with_imports() {
    use raya_bytecode::module::Import;

    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create a module with imports
    let mut module = create_test_module("importing_module");

    // Add an import
    module.imports.push(Import {
        module_specifier: "other_module@1.0.0".to_string(),
        symbol: "some_function".to_string(),
        alias: None,
        version_constraint: Some("^1.0.0".to_string()),
    });

    let bytes = module.encode();

    // Load the module
    let result = vm.load_rbin_bytes(&bytes);
    assert!(
        result.is_ok(),
        "Failed to load module with imports: {:?}",
        result
    );
}

#[test]
fn test_empty_rbin_file() {
    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Try to load empty bytes
    let result = vm.load_rbin_bytes(&[]);
    assert!(result.is_err(), "Should fail with empty bytes");
}

#[test]
fn test_truncated_rbin_file() {
    // Create a VM
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create a valid module but only take first few bytes
    let module = create_test_module("test_module");
    let bytes = module.encode();
    let truncated = &bytes[..20.min(bytes.len())];

    // Try to load truncated bytes
    let result = vm.load_rbin_bytes(truncated);
    assert!(result.is_err(), "Should fail with truncated file");
}
