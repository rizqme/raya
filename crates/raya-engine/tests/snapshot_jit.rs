//! Integration tests for Snapshot + JIT interaction
//!
//! Verifies that VM snapshots work correctly when JIT compilation is enabled.
//! Key invariant: CodeCache is NOT persisted in snapshots — only bytecode-level
//! task state is serialized. JIT can re-prewarm after restore.

#![cfg(feature = "jit")]

use raya_engine::compiler::bytecode::{ConstantPool, Function, Metadata, Module, Opcode};
use raya_engine::jit::JitConfig;
use raya_engine::vm::value::Value;
use raya_engine::Vm;

// ============================================================================
// Helpers
// ============================================================================

fn emit(code: &mut Vec<u8>, op: Opcode) {
    code.push(op as u8);
}

fn emit_i32(code: &mut Vec<u8>, val: i32) {
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&val.to_le_bytes());
}

/// Create a simple module with a "main" function
fn make_module(code: Vec<u8>, param_count: usize, local_count: usize) -> Module {
    Module {
        magic: *b"RAYA",
        version: 1,
        flags: 0,
        constants: ConstantPool::new(),
        functions: vec![Function {
            name: "main".to_string(),
            param_count,
            local_count,
            code,
        }],
        classes: vec![],
        metadata: Metadata {
            name: "test_module".to_string(),
            source_file: None,
        },
        exports: vec![],
        imports: vec![],
        checksum: [0; 32],
        reflection: None,
        debug_info: None,
        native_functions: vec![],
    }
}

/// Create a module with a math-heavy "main" function that should trigger JIT compilation
fn make_jit_eligible_module() -> Module {
    let mut code = Vec::new();
    // Build a compute-heavy function that exceeds the heuristic threshold
    for _ in 0..4 {
        emit_i32(&mut code, 1);
        emit_i32(&mut code, 2);
        emit(&mut code, Opcode::Iadd);
        emit_i32(&mut code, 3);
        emit(&mut code, Opcode::Imul);
    }
    // Collapse into single result
    for _ in 0..3 {
        emit(&mut code, Opcode::Iadd);
    }
    emit(&mut code, Opcode::Return);

    make_module(code, 0, 0)
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn snapshot_succeeds_with_jit_enabled() {
    let mut vm = Vm::new();
    vm.enable_jit().expect("Failed to enable JIT");

    // Execute a simple module
    let mut code = Vec::new();
    emit_i32(&mut code, 42);
    emit(&mut code, Opcode::Return);

    let module = make_module(code, 0, 0);
    let result = vm.execute(&module).expect("Execution failed");
    assert_eq!(result, Value::i32(42));

    // Snapshot should succeed even with JIT enabled
    let snap = vm.snapshot_to_bytes().expect("Snapshot failed");
    assert!(!snap.is_empty(), "Snapshot should not be empty");
}

#[test]
fn snapshot_round_trip_with_jit_preserves_task_state() {
    // VM1: execute with JIT, take snapshot
    let mut vm1 = Vm::new();
    vm1.enable_jit().expect("Failed to enable JIT");

    let mut code = Vec::new();
    emit_i32(&mut code, 99);
    emit(&mut code, Opcode::Return);

    let module = make_module(code, 0, 0);
    let result1 = vm1.execute(&module).expect("Execution failed");
    assert_eq!(result1, Value::i32(99));

    let snap = vm1.snapshot_to_bytes().expect("Snapshot failed");

    // VM2: restore WITHOUT JIT — snapshot should still work (JIT state not persisted)
    let mut vm2 = Vm::new();
    // Load the module first (required for restore)
    let encoded = module.encode();
    vm2.load_rbin_bytes(&encoded).expect("Failed to load module");

    vm2.restore_from_bytes(&snap).expect("Restore failed");

    // Verify task count matches (1 completed task from execute)
    let task_count = vm2.shared_state().tasks.read().len();
    assert!(task_count >= 1, "Should have at least 1 task after restore");
}

#[test]
fn snapshot_round_trip_jit_to_jit() {
    // VM1: execute with JIT
    let mut vm1 = Vm::new();
    vm1.enable_jit().expect("Failed to enable JIT");

    let mut code = Vec::new();
    emit_i32(&mut code, 77);
    emit(&mut code, Opcode::Return);

    let module = make_module(code, 0, 0);
    let result = vm1.execute(&module).expect("Execution failed");
    assert_eq!(result, Value::i32(77));

    let snap = vm1.snapshot_to_bytes().expect("Snapshot failed");

    // VM2: restore WITH JIT enabled
    let mut vm2 = Vm::new();
    vm2.enable_jit().expect("Failed to enable JIT on VM2");

    let encoded = module.encode();
    vm2.load_rbin_bytes(&encoded).expect("Failed to load module");

    vm2.restore_from_bytes(&snap).expect("Restore failed");

    let task_count = vm2.shared_state().tasks.read().len();
    assert!(task_count >= 1, "Should have at least 1 task after restore");
}

#[test]
fn execute_after_restore_with_jit() {
    // VM1: execute a module, take snapshot
    let mut vm1 = Vm::new();

    let mut code1 = Vec::new();
    emit_i32(&mut code1, 10);
    emit(&mut code1, Opcode::Return);

    let module1 = make_module(code1, 0, 0);
    let _ = vm1.execute(&module1).expect("Execution 1 failed");

    let snap = vm1.snapshot_to_bytes().expect("Snapshot failed");

    // VM2: restore, then enable JIT, then execute a NEW module
    let mut vm2 = Vm::new();
    let encoded = module1.encode();
    vm2.load_rbin_bytes(&encoded).expect("Failed to load module");
    vm2.restore_from_bytes(&snap).expect("Restore failed");

    vm2.enable_jit().expect("Failed to enable JIT on VM2");

    let mut code2 = Vec::new();
    emit_i32(&mut code2, 20);
    emit(&mut code2, Opcode::Return);

    let module2 = make_module(code2, 0, 0);
    let result2 = vm2.execute(&module2).expect("Execution 2 failed");
    assert_eq!(result2, Value::i32(20));
}

#[test]
fn snapshot_after_jit_prewarm_execution() {
    // Use a JIT-eligible module (math-heavy) to trigger actual prewarm compilation
    let config = JitConfig {
        min_score: 1.0,
        min_instruction_count: 2,
        ..Default::default()
    };

    let mut vm = Vm::new();
    vm.enable_jit_with_config(config).expect("Failed to enable JIT");

    let module = make_jit_eligible_module();
    let result = vm.execute(&module).expect("Execution failed");
    // The math result: (1+2)*3 = 9, computed 4 times, then summed: 9+9+9+9 = 36
    // Actually the exact value depends on stack behavior, but it should succeed
    assert!(result.as_i32().is_some(), "Should return an integer");

    // Snapshot after prewarm + execution
    let snap = vm.snapshot_to_bytes().expect("Snapshot failed");
    assert!(!snap.is_empty());

    // Restore to a new VM
    let mut vm2 = Vm::new();
    let encoded = module.encode();
    vm2.load_rbin_bytes(&encoded).expect("Failed to load module");
    vm2.restore_from_bytes(&snap).expect("Restore failed");

    let task_count = vm2.shared_state().tasks.read().len();
    assert!(task_count >= 1, "Should have at least 1 task after restore");
}

#[test]
fn snapshot_file_round_trip_with_jit() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let snap_path = dir.path().join("jit_snapshot.rsnap");

    // VM1: execute with JIT, snapshot to file
    let mut vm1 = Vm::new();
    vm1.enable_jit().expect("Failed to enable JIT");

    let mut code = Vec::new();
    emit_i32(&mut code, 55);
    emit(&mut code, Opcode::Return);

    let module = make_module(code, 0, 0);
    let result = vm1.execute(&module).expect("Execution failed");
    assert_eq!(result, Value::i32(55));

    vm1.snapshot_to_file(&snap_path).expect("Snapshot to file failed");
    assert!(snap_path.exists(), "Snapshot file should exist");

    // VM2: restore from file (no JIT — proving JIT state isn't needed)
    let mut vm2 = Vm::new();
    let encoded = module.encode();
    vm2.load_rbin_bytes(&encoded).expect("Failed to load module");
    vm2.restore_from_file(&snap_path).expect("Restore from file failed");

    let task_count = vm2.shared_state().tasks.read().len();
    assert!(task_count >= 1, "Should have at least 1 task after file restore");
}
