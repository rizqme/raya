//! Integration tests for module loading
//!
//! Tests the complete module loading pipeline from .ryb bytes to registered modules.

use raya_engine::compiler::{Function, Module};
use raya_engine::compiler::Opcode;
use raya_engine::vm::Vm;

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
    let mut vm = Vm::new();

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
    let mut vm = Vm::new();

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
    let mut vm = Vm::new();

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
    let mut vm = Vm::new();

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
    let mut vm = Vm::new();

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
    use raya_engine::compiler::{Export, SymbolType};

    // Create a VM
    let mut vm = Vm::new();

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
    use raya_engine::compiler::Import;

    // Create a VM
    let mut vm = Vm::new();

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
    let mut vm = Vm::new();

    // Try to load empty bytes
    let result = vm.load_rbin_bytes(&[]);
    assert!(result.is_err(), "Should fail with empty bytes");
}

#[test]
fn test_truncated_rbin_file() {
    // Create a VM
    let mut vm = Vm::new();

    // Create a valid module but only take first few bytes
    let module = create_test_module("test_module");
    let bytes = module.encode();
    let truncated = &bytes[..20.min(bytes.len())];

    // Try to load truncated bytes
    let result = vm.load_rbin_bytes(truncated);
    assert!(result.is_err(), "Should fail with truncated file");
}

// =============================================================================
// E2E: encode → load_rbin_bytes → execute → verify
// =============================================================================

/// Helper to encode a module to .ryb bytes and load it into a fresh VM
fn load_and_execute(module: &Module) -> raya_engine::vm::VmResult<raya_engine::vm::value::Value> {
    let bytes = module.encode();
    let mut vm = Vm::new();
    vm.load_rbin_bytes(&bytes).expect("load_rbin_bytes failed");
    vm.execute(module)
}

#[test]
fn test_e2e_load_then_execute_simple() {
    use raya_engine::vm::value::Value;

    // Build a module that returns 42
    let mut module = Module::new("e2e_simple".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8],
    });

    let result = load_and_execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_e2e_load_then_execute_arithmetic() {
    use raya_engine::vm::value::Value;

    // Build: 7 * 8 = 56
    let mut module = Module::new("e2e_arith".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8, 7, 0, 0, 0,
            Opcode::ConstI32 as u8, 8, 0, 0, 0,
            Opcode::Imul as u8,
            Opcode::Return as u8,
        ],
    });

    let result = load_and_execute(&module).unwrap();
    assert_eq!(result, Value::i32(56));
}

#[test]
fn test_e2e_load_then_execute_with_locals() {
    use raya_engine::vm::value::Value;

    // let x = 100; let y = 23; return x - y  →  77
    let mut module = Module::new("e2e_locals".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 2,
        code: vec![
            Opcode::ConstI32 as u8, 100, 0, 0, 0,
            Opcode::StoreLocal as u8, 0, 0,
            Opcode::ConstI32 as u8, 23, 0, 0, 0,
            Opcode::StoreLocal as u8, 1, 0,
            Opcode::LoadLocal as u8, 0, 0,
            Opcode::LoadLocal as u8, 1, 0,
            Opcode::Isub as u8,
            Opcode::Return as u8,
        ],
    });

    let result = load_and_execute(&module).unwrap();
    assert_eq!(result, Value::i32(77));
}

#[test]
fn test_e2e_load_then_execute_with_string_constant() {
    use raya_engine::vm::value::Value;

    // Module with a string in the constant pool; ConstStr pushes it, then Return
    let mut module = Module::new("e2e_string".to_string());
    let idx = module.constants.add_string("hello world".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstStr as u8,
            (idx & 0xFF) as u8,
            ((idx >> 8) & 0xFF) as u8,
            Opcode::Return as u8,
        ],
    });

    let result = load_and_execute(&module).unwrap();
    // The returned value should be a pointer to a RayaString; check it's non-null and truthy
    assert!(result != Value::null(), "Expected a string value, got null");
}

#[test]
fn test_e2e_load_then_execute_multi_function_call() {
    use raya_engine::vm::value::Value;

    // func add_ten(x) { return x + 10 }
    // func main() { return add_ten(32) }  →  42
    let mut module = Module::new("e2e_call".to_string());

    // Function 0: add_ten  (param_count=1, local_count=1 for the param)
    module.functions.push(Function {
        name: "add_ten".to_string(),
        param_count: 1,
        local_count: 1,
        code: vec![
            Opcode::LoadLocal as u8, 0, 0,        // load param x
            Opcode::ConstI32 as u8, 10, 0, 0, 0,  // push 10
            Opcode::Iadd as u8,                    // x + 10
            Opcode::Return as u8,
        ],
    });

    // Function 1: main
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8, 32, 0, 0, 0,  // push 32 (arg)
            Opcode::Call as u8,
            0, 0, 0, 0,  // func_index = 0 (add_ten), u32 LE
            1, 0,         // arg_count = 1, u16 LE
            Opcode::Return as u8,
        ],
    });

    let result = load_and_execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}

// =============================================================================
// Module Registry verification after loading
// =============================================================================

#[test]
fn test_registry_tracks_loaded_module() {
    let mut module = Module::new("tracked_module".to_string());
    module.functions.push(Function {
        name: "f".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    let bytes = module.encode();
    let mut vm = Vm::new();
    vm.load_rbin_bytes(&bytes).unwrap();

    let registry = vm.shared_state().module_registry.read();
    assert_eq!(registry.module_count(), 1);
    assert!(registry.get_by_name("tracked_module").is_some());
}

#[test]
fn test_registry_tracks_multiple_modules() {
    let mut vm = Vm::new();

    for i in 0..3 {
        let name = format!("mod_{}", i);
        let mut module = Module::new(name.clone());
        module.functions.push(Function {
            name: "f".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8, i as u8, 0, 0, 0,
                Opcode::Return as u8,
            ],
        });
        let bytes = module.encode();
        vm.load_rbin_bytes(&bytes).unwrap();
    }

    let registry = vm.shared_state().module_registry.read();
    assert_eq!(registry.module_count(), 3);
    for i in 0..3 {
        assert!(
            registry.get_by_name(&format!("mod_{}", i)).is_some(),
            "mod_{} not found in registry",
            i
        );
    }
}

#[test]
fn test_registry_deduplicates_same_module() {
    let mut module = Module::new("dedup_mod".to_string());
    module.functions.push(Function {
        name: "f".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    let bytes = module.encode();
    let mut vm = Vm::new();

    // Load the same bytes three times
    vm.load_rbin_bytes(&bytes).unwrap();
    vm.load_rbin_bytes(&bytes).unwrap();
    vm.load_rbin_bytes(&bytes).unwrap();

    let registry = vm.shared_state().module_registry.read();
    assert_eq!(registry.module_count(), 1, "Duplicate modules should be deduplicated");
}

// =============================================================================
// Module with class definitions
// =============================================================================

#[test]
fn test_e2e_load_module_with_class() {
    use raya_engine::compiler::ClassDef;

    let mut module = Module::new("class_mod".to_string());

    // Add a class with 2 fields
    module.classes.push(ClassDef {
        name: "Point".to_string(),
        field_count: 2,
        parent_id: None,
        methods: vec![],
    });

    // A simple main function
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    let bytes = module.encode();
    let mut vm = Vm::new();
    vm.load_rbin_bytes(&bytes).unwrap();

    // Verify the class was registered
    let classes = vm.shared_state().classes.read();
    let point = classes.get_class(0);
    assert!(point.is_some(), "Class 'Point' should be registered at index 0");
    assert_eq!(point.unwrap().name, "Point");
    assert_eq!(point.unwrap().field_count, 2);
}

#[test]
fn test_e2e_load_module_with_class_hierarchy() {
    use raya_engine::compiler::{ClassDef, Method};

    let mut module = Module::new("hierarchy_mod".to_string());

    // Function 0: base_method (for parent)
    module.functions.push(Function {
        name: "base_method".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 1, 0, 0, 0, Opcode::Return as u8],
    });

    // Function 1: override_method (for child)
    module.functions.push(Function {
        name: "override_method".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 2, 0, 0, 0, Opcode::Return as u8],
    });

    // Function 2: main
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    // Class 0: Base (1 field, 1 method)
    module.classes.push(ClassDef {
        name: "Base".to_string(),
        field_count: 1,
        parent_id: None,
        methods: vec![Method {
            name: "greet".to_string(),
            function_id: 0,
            slot: 0,
        }],
    });

    // Class 1: Child extends Base (2 fields, overrides greet)
    module.classes.push(ClassDef {
        name: "Child".to_string(),
        field_count: 2,
        parent_id: Some(0),
        methods: vec![Method {
            name: "greet".to_string(),
            function_id: 1,
            slot: 0, // same vtable slot as parent
        }],
    });

    let bytes = module.encode();
    let mut vm = Vm::new();
    vm.load_rbin_bytes(&bytes).unwrap();

    let classes = vm.shared_state().classes.read();

    // Base class
    let base = classes.get_class(0).expect("Base class should exist");
    assert_eq!(base.name, "Base");
    assert_eq!(base.vtable.methods.len(), 1);
    assert_eq!(base.vtable.methods[0], 0); // points to function 0

    // Child class
    let child = classes.get_class(1).expect("Child class should exist");
    assert_eq!(child.name, "Child");
    assert_eq!(child.vtable.methods.len(), 1);
    assert_eq!(child.vtable.methods[0], 1); // overridden to function 1
}

// =============================================================================
// File-based .ryb loading
// =============================================================================

#[test]
fn test_load_rbin_file() {
    use raya_engine::vm::value::Value;

    let mut module = Module::new("file_mod".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 99, 0, 0, 0, Opcode::Return as u8],
    });

    // Write to temp file
    let dir = std::env::temp_dir();
    let path = dir.join("raya_test_module.ryb");
    let bytes = module.encode();
    std::fs::write(&path, &bytes).expect("Failed to write temp .ryb file");

    // Load from file and execute
    let mut vm = Vm::new();
    vm.load_rbin(&path).unwrap();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(99));

    // Clean up
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_load_rbin_file_nonexistent() {
    let mut vm = Vm::new();
    let result = vm.load_rbin(std::path::Path::new("/tmp/does_not_exist_raya_test.ryb"));
    assert!(result.is_err(), "Loading a non-existent file should fail");
}

// =============================================================================
// Module with exports: round-trip verification
// =============================================================================

#[test]
fn test_e2e_exports_survive_encode_decode() {
    use raya_engine::compiler::{Export, SymbolType};

    let mut module = Module::new("export_mod".to_string());
    module.functions.push(Function {
        name: "add".to_string(),
        param_count: 2,
        local_count: 2,
        code: vec![
            Opcode::LoadLocal as u8, 0, 0,
            Opcode::LoadLocal as u8, 1, 0,
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    });
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    module.exports.push(Export {
        name: "add".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
    });

    // Encode → decode round-trip
    let bytes = module.encode();
    let decoded = Module::decode(&bytes).expect("Decode should succeed");

    assert_eq!(decoded.exports.len(), 1);
    assert_eq!(decoded.exports[0].name, "add");
    assert_eq!(decoded.exports[0].symbol_type, SymbolType::Function);
    assert_eq!(decoded.exports[0].index, 0);
    assert_eq!(decoded.metadata.name, "export_mod");

    // Also verify it can be loaded
    let mut vm = Vm::new();
    vm.load_rbin_bytes(&bytes).unwrap();
}

// =============================================================================
// Module with imports: round-trip verification
// =============================================================================

#[test]
fn test_e2e_imports_survive_encode_decode() {
    use raya_engine::compiler::Import;

    let mut module = Module::new("import_mod".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    module.imports.push(Import {
        module_specifier: "math@2.0.0".to_string(),
        symbol: "sqrt".to_string(),
        alias: Some("squareRoot".to_string()),
        version_constraint: Some("^2.0.0".to_string()),
    });

    let bytes = module.encode();
    let decoded = Module::decode(&bytes).expect("Decode should succeed");

    assert_eq!(decoded.imports.len(), 1);
    assert_eq!(decoded.imports[0].module_specifier, "math@2.0.0");
    assert_eq!(decoded.imports[0].symbol, "sqrt");
    assert_eq!(decoded.imports[0].alias.as_deref(), Some("squareRoot"));
    assert_eq!(decoded.imports[0].version_constraint.as_deref(), Some("^2.0.0"));
}

// =============================================================================
// Constant pool round-trip
// =============================================================================

#[test]
fn test_e2e_constant_pool_survives_encode_decode() {
    let mut module = Module::new("const_pool_mod".to_string());

    // Add various constants
    module.constants.add_string("hello".to_string());
    module.constants.add_string("world".to_string());
    module.constants.add_integer(42);
    module.constants.add_float(3.14);

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    let bytes = module.encode();
    let decoded = Module::decode(&bytes).expect("Decode should succeed");

    assert_eq!(decoded.constants.get_string(0), Some("hello"));
    assert_eq!(decoded.constants.get_string(1), Some("world"));
    assert_eq!(decoded.constants.get_integer(0), Some(42));
    // Float comparison with epsilon
    let f = decoded.constants.get_float(0).unwrap();
    assert!((f - 3.14).abs() < 1e-10, "Expected 3.14, got {}", f);
}

// =============================================================================
// Snapshot round-trip with loaded module
// =============================================================================

#[test]
fn test_e2e_snapshot_with_loaded_module() {
    // Create and encode a module
    let mut module = Module::new("snap_mod".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8],
    });

    // VM1: execute (which registers internally), then snapshot
    let mut vm1 = Vm::new();
    let _result = vm1.execute(&module).unwrap();

    let snap_bytes = vm1.snapshot_to_bytes().unwrap();
    assert!(!snap_bytes.is_empty());

    // VM2: load module via bytes (to have it in registry), then restore snapshot
    let bytes = module.encode();
    let mut vm2 = Vm::new();
    vm2.load_rbin_bytes(&bytes).unwrap();
    vm2.restore_from_bytes(&snap_bytes).unwrap();

    // Verify both VMs have a module registered
    assert!(vm1.shared_state().module_registry.read().module_count() >= 1);
    assert!(vm2.shared_state().module_registry.read().module_count() >= 1);
}

#[test]
fn test_e2e_snapshot_file_round_trip_with_module() {
    // Same as above but using file-based snapshot
    let mut module = Module::new("snap_file_mod".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 7, 0, 0, 0, Opcode::Return as u8],
    });
    let bytes = module.encode();

    let mut vm1 = Vm::new();
    vm1.load_rbin_bytes(&bytes).unwrap();
    let _result = vm1.execute(&module).unwrap();

    let dir = std::env::temp_dir();
    let snap_path = dir.join("raya_e2e_snapshot.snap");
    vm1.snapshot_to_file(&snap_path).unwrap();

    let mut vm2 = Vm::new();
    vm2.load_rbin_bytes(&bytes).unwrap();
    vm2.restore_from_file(&snap_path).unwrap();

    // Clean up
    let _ = std::fs::remove_file(&snap_path);
}

// =============================================================================
// Error cases
// =============================================================================

#[test]
fn test_e2e_execute_module_without_main() {
    let mut module = Module::new("no_main".to_string());
    module.functions.push(Function {
        name: "helper".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module);
    assert!(result.is_err(), "Execute without main should fail");
    assert!(
        result.unwrap_err().to_string().contains("No main function"),
        "Error should mention missing main"
    );
}

#[test]
fn test_e2e_version_preserved_in_encode_decode() {
    let mut module = Module::new("version_test".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });

    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();

    assert_eq!(decoded.magic, *b"RAYA");
    assert_eq!(decoded.version, 1);
    assert_eq!(decoded.metadata.name, "version_test");
}

#[test]
fn test_e2e_checksum_differs_for_different_modules() {
    let mut m1 = Module::new("mod_a".to_string());
    m1.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 1, 0, 0, 0, Opcode::Return as u8],
    });

    let mut m2 = Module::new("mod_b".to_string());
    m2.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 2, 0, 0, 0, Opcode::Return as u8],
    });

    let bytes1 = m1.encode();
    let bytes2 = m2.encode();

    let decoded1 = Module::decode(&bytes1).unwrap();
    let decoded2 = Module::decode(&bytes2).unwrap();

    assert_ne!(
        decoded1.checksum, decoded2.checksum,
        "Different modules should have different checksums"
    );
}
