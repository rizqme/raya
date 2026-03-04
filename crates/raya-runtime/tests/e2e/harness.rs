//! Test harness for end-to-end compilation and execution
//!
//! Provides utilities for compiling Raya source code and executing it in the VM.

use raya_engine::compiler::{Compiler, Module};
use raya_engine::parser::checker::{Binder, TypeChecker, TypeSystemMode};
use raya_engine::parser::{Interner, Parser, TypeContext};
use raya_engine::vm::gc::GcHeader;
use raya_engine::vm::scheduler::SchedulerLimits;
use raya_engine::vm::{Array, Object, RayaString, Value, Vm, VmError};
use raya_runtime::{BuiltinMode, StdNativeHandler};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

thread_local! {
    /// Keeps a small ring of recently used VMs alive on each test thread so
    /// pointer values returned from `compile_and_run*` remain valid across
    /// back-to-back invocations within the same test.
    static KEPT_VMS: RefCell<VecDeque<Vm>> = RefCell::new(VecDeque::new());
}

/// Retain a small number of recent VMs to keep returned pointer Values valid.
fn keep_vm_alive(vm: Vm) {
    const MAX_KEPT_VMS: usize = 2;
    KEPT_VMS.with(|slot| {
        let mut kept = slot.borrow_mut();
        kept.push_back(vm);
        while kept.len() > MAX_KEPT_VMS {
            kept.pop_front();
        }
    });
}

/// Error type for e2e tests
#[derive(Debug)]
pub enum E2EError {
    /// Lexer error
    Lex(String),
    /// Parse error
    Parse(String),
    /// Type check error
    TypeCheck(String),
    /// Compilation error
    Compile(raya_engine::compiler::CompileError),
    /// VM execution error
    Vm(VmError),
}

impl std::fmt::Display for E2EError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            E2EError::Lex(e) => write!(f, "Lexer error: {}", e),
            E2EError::Parse(e) => write!(f, "Parse error: {}", e),
            E2EError::TypeCheck(e) => write!(f, "Type check error: {}", e),
            E2EError::Compile(e) => write!(f, "Compile error: {}", e),
            E2EError::Vm(e) => write!(f, "VM error: {}", e),
        }
    }
}

impl std::error::Error for E2EError {}

/// Result type for e2e tests
pub type E2EResult<T> = Result<T, E2EError>;

/// Compile Raya source code to bytecode
pub fn compile(source: &str) -> E2EResult<(Module, Interner)> {
    compile_via_binary_linker(source, BuiltinMode::RayaStrict)
}

/// Compile Raya source code with builtin classes included
///
/// Uses the production runtime module pipeline (Module System V2), with no
/// std-prelude/source-rewrite fallback path.
pub fn compile_with_builtins(source: &str) -> E2EResult<(Module, Interner)> {
    compile_via_binary_linker(source, BuiltinMode::RayaStrict)
}

fn compile_via_binary_linker(source: &str, mode: BuiltinMode) -> E2EResult<(Module, Interner)> {
    let runtime = raya_runtime::Runtime::with_options(raya_runtime::RuntimeOptions {
        builtin_mode: mode,
        no_jit: true,
        ..Default::default()
    });
    let compiled = runtime
        .compile(source)
        .map_err(|e| E2EError::TypeCheck(e.to_string()))?;
    let interner = parse_interner(source)?;
    Ok((compiled.module().clone(), interner))
}

fn parse_interner(source: &str) -> E2EResult<Interner> {
    let parser = Parser::new(source).map_err(|e| E2EError::Lex(format!("{:?}", e)))?;
    let (_ast, interner) = parser
        .parse()
        .map_err(|e| E2EError::Parse(format!("{:?}", e)))?;
    Ok(interner)
}

fn compile_program_with_mode(
    source: &str,
    mode: BuiltinMode,
) -> E2EResult<(raya_runtime::Runtime, raya_runtime::CompiledProgram)> {
    let runtime = raya_runtime::Runtime::with_options(raya_runtime::RuntimeOptions {
        builtin_mode: mode,
        no_jit: true,
        ..Default::default()
    });
    let program = runtime
        .compile_program_source(source)
        .map_err(|e| E2EError::TypeCheck(e.to_string()))?;
    Ok((runtime, program))
}

fn map_runtime_error(error: raya_runtime::RuntimeError) -> E2EError {
    match error {
        raya_runtime::RuntimeError::Vm(vm_error) => E2EError::Vm(vm_error),
        other => E2EError::TypeCheck(other.to_string()),
    }
}

/// Compile and execute Raya source code, returning the result
///
/// The source code should have a `main` function that returns a value,
/// or use a `return` statement at the top level.
pub fn compile_and_run(source: &str) -> E2EResult<Value> {
    let (runtime, program) = compile_program_with_mode(source, BuiltinMode::RayaStrict)?;
    // Use single worker to avoid resource contention during parallel test execution.
    let mut vm = Vm::with_worker_count(1);
    let value = runtime
        .execute_program_with_vm(&program, &mut vm)
        .map_err(map_runtime_error)?;
    keep_vm_alive(vm);
    Ok(value)
}

/// Compile and execute with builtins included
///
/// Use this for tests that use Map, Set, Buffer, Date, Channel, Logger, etc.
pub fn compile_and_run_with_builtins(source: &str) -> E2EResult<Value> {
    let (runtime, program) = compile_program_with_mode(source, BuiltinMode::RayaStrict)?;

    // Use single worker with StdNativeHandler for stdlib support (logger, etc.).
    let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));

    // Register symbolic native functions for ModuleNativeCall dispatch.
    {
        let mut registry = vm.native_registry().write();
        raya_stdlib::register_stdlib(&mut registry);
        raya_stdlib_posix::register_posix(&mut registry);
    }

    let value = runtime
        .execute_program_with_vm(&program, &mut vm)
        .map_err(map_runtime_error)?;
    keep_vm_alive(vm);
    Ok(value)
}

/// Compile and execute using the production runtime compile pipeline.
///
/// This path mirrors `raya run/eval` behavior through the module-system pipeline.
pub fn compile_and_run_runtime(source: &str) -> E2EResult<Value> {
    compile_and_run_runtime_with_mode(source, BuiltinMode::RayaStrict)
}

/// Compile and execute using the production runtime compile pipeline with an
/// explicit builtin compatibility mode.
pub fn compile_and_run_runtime_with_mode(source: &str, mode: BuiltinMode) -> E2EResult<Value> {
    let (runtime, program) = compile_program_with_mode(source, mode)?;

    let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
    {
        let mut registry = vm.native_registry().write();
        raya_stdlib::register_stdlib(&mut registry);
        raya_stdlib_posix::register_posix(&mut registry);
    }

    let value = runtime
        .execute_program_with_vm(&program, &mut vm)
        .map_err(map_runtime_error)?;
    keep_vm_alive(vm);
    Ok(value)
}

pub fn compile_and_run_runtime_node_compat(source: &str) -> E2EResult<Value> {
    compile_and_run_runtime_with_mode(source, BuiltinMode::NodeCompat)
}

pub fn expect_i32_runtime_node_compat(source: &str, expected: i32) {
    match compile_and_run_runtime_node_compat(source) {
        Ok(value) => {
            if let Some(actual) = value.as_i32() {
                assert_eq!(actual, expected, "Wrong result for:\n{}", source);
                return;
            }
            if let Some(actual) = value.as_f64() {
                let expected_f64 = expected as f64;
                assert!(
                    (actual - expected_f64).abs() < 1e-10 && actual.fract() == 0.0,
                    "Expected {} (i32), got {} (f64) for:\n{}",
                    expected,
                    actual,
                    source
                );
                return;
            }
            panic!(
                "Expected i32 or f64 result, got {:?}\nSource:\n{}",
                value, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

pub fn expect_string_runtime_node_compat(source: &str, expected: &str) {
    match compile_and_run_runtime_node_compat(source) {
        Ok(value) => {
            if value.is_ptr() {
                let raw_ptr = unsafe { value.as_ptr::<u8>() };
                if let Some(ptr) = raw_ptr {
                    let header = unsafe {
                        let hp = ptr.as_ptr().sub(std::mem::size_of::<GcHeader>());
                        &*(hp as *const GcHeader)
                    };
                    if header.type_id() != std::any::TypeId::of::<RayaString>() {
                        let detected = if header.type_id() == std::any::TypeId::of::<Object>() {
                            "Object"
                        } else if header.type_id() == std::any::TypeId::of::<Array>() {
                            "Array"
                        } else if header.type_id() == std::any::TypeId::of::<RayaString>() {
                            "RayaString"
                        } else {
                            "Unknown"
                        };
                        panic!(
                            "Expected RayaString pointer, got GC object type={} (value={:?})\nSource:\n{}",
                            detected, value, source
                        );
                    }
                    let raya_str = unsafe { &*value.as_ptr::<RayaString>().unwrap().as_ptr() };
                    assert_eq!(
                        raya_str.data, expected,
                        "String mismatch.\nExpected: '{}'\nGot: '{}'\nSource:\n{}",
                        expected, raya_str.data, source
                    );
                } else {
                    panic!(
                        "Failed to extract string pointer from value {:?}\nSource:\n{}",
                        value, source
                    );
                }
            } else {
                panic!(
                    "Expected string (pointer), got {:?}\nSource:\n{}",
                    value, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

pub fn expect_bool_runtime_node_compat(source: &str, expected: bool) {
    match compile_and_run_runtime_node_compat(source) {
        Ok(value) => {
            let actual = value.as_bool().expect(&format!(
                "Expected bool result, got {:?}\nSource:\n{}",
                value, source
            ));
            assert_eq!(actual, expected, "Wrong result for:\n{}", source);
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

pub fn expect_i32_runtime(source: &str, expected: i32) {
    match compile_and_run_runtime(source) {
        Ok(value) => {
            if let Some(actual) = value.as_i32() {
                assert_eq!(actual, expected, "Wrong result for:\n{}", source);
                return;
            }
            if let Some(actual) = value.as_f64() {
                let expected_f64 = expected as f64;
                assert!(
                    (actual - expected_f64).abs() < 1e-10 && actual.fract() == 0.0,
                    "Expected {} (i32), got {} (f64) for:\n{}",
                    expected,
                    actual,
                    source
                );
                return;
            }
            panic!(
                "Expected i32 or f64 result, got {:?}\nSource:\n{}",
                value, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

pub fn expect_bool_runtime(source: &str, expected: bool) {
    match compile_and_run_runtime(source) {
        Ok(value) => {
            let actual = value.as_bool().expect(&format!(
                "Expected bool result, got {:?}\nSource:\n{}",
                value, source
            ));
            assert_eq!(actual, expected, "Wrong result for:\n{}", source);
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute using the production runtime compile pipeline.
///
/// This path mirrors `raya run/eval` behavior (builtin + std prelude injection)
/// and is useful for validating builtins that depend on runtime compile semantics.
pub fn compile_and_run_runtime_legacy(source: &str) -> E2EResult<Value> {
    let (module, _interner) = raya_runtime::compile::compile_source(source)
        .map_err(|e| E2EError::TypeCheck(e.to_string()))?;

    let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
    {
        let mut registry = vm.native_registry().write();
        raya_stdlib::register_stdlib(&mut registry);
        raya_stdlib_posix::register_posix(&mut registry);
    }

    let value = vm.execute(&module).map_err(E2EError::Vm)?;
    keep_vm_alive(vm);
    Ok(value)
}

/// Compile and execute with builtins, expecting a specific i32 result
/// Also accepts f64 values that represent whole numbers (since Number type is f64)
pub fn expect_i32_with_builtins(source: &str, expected: i32) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            // Try i32 first
            if let Some(actual) = value.as_i32() {
                assert_eq!(actual, expected, "Wrong result for:\n{}", source);
                return;
            }
            // Also accept f64 that represents a whole number
            if let Some(actual) = value.as_f64() {
                let expected_f64 = expected as f64;
                assert!(
                    (actual - expected_f64).abs() < 1e-10 && actual.fract() == 0.0,
                    "Expected {} (i32), got {} (f64) for:\n{}",
                    expected,
                    actual,
                    source
                );
                return;
            }
            panic!(
                "Expected i32 or f64 result, got {:?}\nSource:\n{}",
                value, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with builtins, expecting a specific f64 result (within epsilon)
pub fn expect_f64_with_builtins(source: &str, expected: f64) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            let actual = value.as_f64().expect(&format!(
                "Expected f64 result, got {:?}\nSource:\n{}",
                value, source
            ));
            assert!(
                (actual - expected).abs() < 1e-10,
                "Expected {}, got {} for:\n{}",
                expected,
                actual,
                source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with builtins, expecting a specific boolean result
pub fn expect_bool_with_builtins(source: &str, expected: bool) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            let actual = value.as_bool().expect(&format!(
                "Expected bool result, got {:?}\nSource:\n{}",
                value, source
            ));
            assert_eq!(actual, expected, "Wrong result for:\n{}", source);
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute, expecting a specific i32 result
/// Also accepts f64 values that represent whole numbers (since Number type is f64)
pub fn expect_i32(source: &str, expected: i32) {
    match compile_and_run(source) {
        Ok(value) => {
            // Try i32 first
            if let Some(actual) = value.as_i32() {
                assert_eq!(actual, expected, "Wrong result for:\n{}", source);
                return;
            }
            // Also accept f64 that represents a whole number
            if let Some(actual) = value.as_f64() {
                let expected_f64 = expected as f64;
                assert!(
                    (actual - expected_f64).abs() < 1e-10 && actual.fract() == 0.0,
                    "Expected {} (i32), got {} (f64) for:\n{}",
                    expected,
                    actual,
                    source
                );
                return;
            }
            panic!(
                "Expected i32 or f64 result, got {:?}\nSource:\n{}",
                value, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute, expecting a specific f64 result
pub fn expect_f64(source: &str, expected: f64) {
    match compile_and_run(source) {
        Ok(value) => {
            let actual = value.as_f64().expect(&format!(
                "Expected f64 result, got {:?}\nSource:\n{}",
                value, source
            ));
            assert!(
                (actual - expected).abs() < 1e-10,
                "Expected {}, got {} for:\n{}",
                expected,
                actual,
                source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute, expecting a specific boolean result
pub fn expect_bool(source: &str, expected: bool) {
    match compile_and_run(source) {
        Ok(value) => {
            let actual = value.as_bool().expect(&format!(
                "Expected bool result, got {:?}\nSource:\n{}",
                value, source
            ));
            assert_eq!(actual, expected, "Wrong result for:\n{}", source);
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute, expecting null result
pub fn expect_null(source: &str) {
    match compile_and_run(source) {
        Ok(value) => {
            assert!(
                value.is_null(),
                "Expected null, got {:?}\nSource:\n{}",
                value,
                source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute, expecting a specific string result
#[allow(dead_code)]
pub fn expect_string(source: &str, expected: &str) {
    match compile_and_run(source) {
        Ok(value) => {
            if value.is_ptr() {
                // Extract string from pointer with runtime type check via GC header
                // to avoid UB when a non-string pointer is returned.
                let raw_ptr = unsafe { value.as_ptr::<u8>() };
                if let Some(ptr) = raw_ptr {
                    let header = unsafe {
                        let hp = ptr.as_ptr().sub(std::mem::size_of::<GcHeader>());
                        &*(hp as *const GcHeader)
                    };
                    if header.type_id() != std::any::TypeId::of::<RayaString>() {
                        let detected = if header.type_id() == std::any::TypeId::of::<Object>() {
                            "Object"
                        } else if header.type_id() == std::any::TypeId::of::<Array>() {
                            "Array"
                        } else if header.type_id() == std::any::TypeId::of::<RayaString>() {
                            "RayaString"
                        } else {
                            "Unknown"
                        };
                        panic!(
                            "Expected RayaString pointer, got GC object type={} (value={:?})\nSource:\n{}",
                            detected, value, source
                        );
                    }
                    let raya_str = unsafe { &*value.as_ptr::<RayaString>().unwrap().as_ptr() };
                    assert_eq!(
                        raya_str.data, expected,
                        "String mismatch.\nExpected: '{}'\nGot: '{}'\nSource:\n{}",
                        expected, raya_str.data, source
                    );
                } else {
                    panic!(
                        "Failed to extract string pointer from value {:?}\nSource:\n{}",
                        value, source
                    );
                }
            } else {
                panic!(
                    "Expected string (pointer), got {:?}\nSource:\n{}",
                    value, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with builtins, expecting a specific string result
pub fn expect_string_with_builtins(source: &str, expected: &str) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            if value.is_ptr() {
                let raw_ptr = unsafe { value.as_ptr::<u8>() };
                if let Some(ptr) = raw_ptr {
                    let header = unsafe {
                        let hp = ptr.as_ptr().sub(std::mem::size_of::<GcHeader>());
                        &*(hp as *const GcHeader)
                    };
                    if header.type_id() != std::any::TypeId::of::<RayaString>() {
                        let detected = if header.type_id() == std::any::TypeId::of::<Object>() {
                            "Object"
                        } else if header.type_id() == std::any::TypeId::of::<Array>() {
                            "Array"
                        } else if header.type_id() == std::any::TypeId::of::<RayaString>() {
                            "RayaString"
                        } else {
                            "Unknown"
                        };
                        panic!(
                            "Expected RayaString pointer, got GC object type={} (value={:?})\nSource:\n{}",
                            detected, value, source
                        );
                    }
                    let raya_str = unsafe { &*value.as_ptr::<RayaString>().unwrap().as_ptr() };
                    assert_eq!(
                        raya_str.data, expected,
                        "String mismatch.\nExpected: '{}'\nGot: '{}'\nSource:\n{}",
                        expected, raya_str.data, source
                    );
                } else {
                    panic!(
                        "Failed to extract string pointer from value {:?}\nSource:\n{}",
                        value, source
                    );
                }
            } else {
                panic!(
                    "Expected string (pointer), got {:?}\nSource:\n{}",
                    value, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

pub fn expect_string_runtime(source: &str, expected: &str) {
    match compile_and_run_runtime(source) {
        Ok(value) => {
            if value.is_ptr() {
                let raw_ptr = unsafe { value.as_ptr::<u8>() };
                if let Some(ptr) = raw_ptr {
                    let header = unsafe {
                        let hp = ptr.as_ptr().sub(std::mem::size_of::<GcHeader>());
                        &*(hp as *const GcHeader)
                    };
                    if header.type_id() != std::any::TypeId::of::<RayaString>() {
                        let detected = if header.type_id() == std::any::TypeId::of::<Object>() {
                            "Object"
                        } else if header.type_id() == std::any::TypeId::of::<Array>() {
                            "Array"
                        } else if header.type_id() == std::any::TypeId::of::<RayaString>() {
                            "RayaString"
                        } else {
                            "Unknown"
                        };
                        panic!(
                            "Expected RayaString pointer, got GC object type={} (value={:?})\nSource:\n{}",
                            detected, value, source
                        );
                    }
                    let raya_str = unsafe { &*value.as_ptr::<RayaString>().unwrap().as_ptr() };
                    assert_eq!(
                        raya_str.data, expected,
                        "String mismatch.\nExpected: '{}'\nGot: '{}'\nSource:\n{}",
                        expected, raya_str.data, source
                    );
                } else {
                    panic!(
                        "Failed to extract string pointer from value {:?}\nSource:\n{}",
                        value, source
                    );
                }
            } else {
                panic!(
                    "Expected string (pointer), got {:?}\nSource:\n{}",
                    value, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with builtins, expecting a string result containing a pattern
pub fn expect_string_contains_with_builtins(source: &str, pattern: &str) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            if value.is_ptr() {
                let str_ptr = unsafe { value.as_ptr::<RayaString>() };
                if let Some(ptr) = str_ptr {
                    let raya_str = unsafe { &*ptr.as_ptr() };
                    assert!(
                        raya_str.data.contains(pattern),
                        "String does not contain pattern.\nExpected to contain: '{}'\nGot: '{}'\nSource:\n{}",
                        pattern, raya_str.data, source
                    );
                } else {
                    panic!(
                        "Failed to extract string pointer from value {:?}\nSource:\n{}",
                        value, source
                    );
                }
            } else {
                panic!(
                    "Expected string (pointer), got {:?}\nSource:\n{}",
                    value, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute, expecting a compilation or type check error
pub fn expect_compile_error(source: &str, error_pattern: &str) {
    match compile(source) {
        Ok(_) => {
            panic!(
                "Expected compile error containing '{}', but compilation succeeded\nSource:\n{}",
                error_pattern, source
            );
        }
        Err(e) => {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains(error_pattern),
                "Expected error containing '{}', got: {}\nSource:\n{}",
                error_pattern,
                error_msg,
                source
            );
        }
    }
}

/// Compile and execute, expecting a runtime error
pub fn expect_runtime_error(source: &str, error_pattern: &str) {
    match compile_and_run(source) {
        Ok(value) => {
            panic!(
                "Expected runtime error containing '{}', but got {:?}\nSource:\n{}",
                error_pattern, value, source
            );
        }
        Err(E2EError::Vm(e)) => {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains(error_pattern),
                "Expected runtime error containing '{}', got: {}\nSource:\n{}",
                error_pattern,
                error_msg,
                source
            );
        }
        Err(e) => {
            panic!(
                "Expected runtime error, got compile error: {}\nSource:\n{}",
                e, source
            );
        }
    }
}

/// Compile and execute with a low preemption limit, expecting a runtime error.
/// Used for infinite loop detection tests that need fast failure (100 preemptions instead of 1000).
pub fn expect_runtime_error_fast_preempt(source: &str, error_pattern: &str) {
    let (module, _interner) = compile(source).expect("compile failed");
    let limits = SchedulerLimits {
        max_preemptions: 100,
        ..Default::default()
    };
    let mut vm = Vm::with_scheduler_limits(1, limits);
    match vm.execute(&module) {
        Ok(value) => {
            panic!(
                "Expected runtime error containing '{}', but got {:?}\nSource:\n{}",
                error_pattern, value, source
            );
        }
        Err(e) => {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains(error_pattern),
                "Expected runtime error containing '{}', got: {}\nSource:\n{}",
                error_pattern,
                error_msg,
                source
            );
        }
    }
}

/// Compile and execute with builtins, expecting a runtime error
pub fn expect_runtime_error_with_builtins(source: &str, error_pattern: &str) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            panic!(
                "Expected runtime error containing '{}', but got {:?}\nSource:\n{}",
                error_pattern, value, source
            );
        }
        Err(E2EError::Vm(e)) => {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains(error_pattern),
                "Expected runtime error containing '{}', got: {}\nSource:\n{}",
                error_pattern,
                error_msg,
                source
            );
        }
        Err(e) => {
            panic!(
                "Expected runtime error, got compile error: {}\nSource:\n{}",
                e, source
            );
        }
    }
}

/// Compile and execute with multiple worker threads
///
/// Use this for tests that need to stress-test true parallel execution.
/// Note: This should be used sparingly as it creates more threads.
pub fn compile_and_run_multiworker(source: &str, worker_count: usize) -> E2EResult<Value> {
    compile_and_run_multiworker_with_timeout(source, worker_count, Duration::from_secs(30))
}

/// Compile and execute with multiple workers, expecting a specific i32 result
#[allow(dead_code)]
pub fn expect_i32_multiworker(source: &str, expected: i32, worker_count: usize) {
    match compile_and_run_multiworker(source, worker_count) {
        Ok(value) => {
            if let Some(actual) = value.as_i32() {
                assert_eq!(actual, expected, "Wrong result for:\n{}", source);
                return;
            }
            if let Some(actual) = value.as_f64() {
                let expected_f64 = expected as f64;
                assert!(
                    (actual - expected_f64).abs() < 1e-10 && actual.fract() == 0.0,
                    "Expected {} (i32), got {} (f64) for:\n{}",
                    expected,
                    actual,
                    source
                );
                return;
            }
            panic!(
                "Expected i32 or f64 result, got {:?}\nSource:\n{}",
                value, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with multiple worker threads and builtins
///
/// Use this for tests that need to stress-test true parallel execution with Mutex/Channel.
pub fn compile_and_run_multiworker_with_builtins(
    source: &str,
    worker_count: usize,
) -> E2EResult<Value> {
    compile_and_run_multiworker_with_builtins_timeout(source, worker_count, Duration::from_secs(30))
}

/// Compile and execute with multiple worker threads and builtins using a custom timeout.
///
/// Use this helper for intentionally heavy multiworker e2e cases that can exceed the
/// default 30s timeout on contended CI runners.
pub fn compile_and_run_multiworker_with_builtins_with_timeout(
    source: &str,
    worker_count: usize,
    timeout: Duration,
) -> E2EResult<Value> {
    compile_and_run_multiworker_with_builtins_timeout(source, worker_count, timeout)
}

fn compile_and_run_multiworker_with_timeout(
    source: &str,
    worker_count: usize,
    timeout: Duration,
) -> E2EResult<Value> {
    let (tx, rx) = mpsc::channel();
    let src = source.to_string();

    std::thread::spawn(move || {
        let result: E2EResult<Value> = (|| {
            let (runtime, program) = compile_program_with_mode(&src, BuiltinMode::RayaStrict)?;
            let mut vm = Vm::with_worker_count(worker_count);
            let value = runtime
                .execute_program_with_vm(&program, &mut vm)
                .map_err(map_runtime_error)?;
            keep_vm_alive(vm);
            Ok(value)
        })();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(E2EError::Vm(VmError::RuntimeError(format!(
            "Multiworker execution timed out after {:?} (workers={})",
            timeout, worker_count
        )))),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(E2EError::Vm(VmError::RuntimeError(
            "Multiworker execution thread disconnected unexpectedly".to_string(),
        ))),
    }
}

fn compile_and_run_multiworker_with_builtins_timeout(
    source: &str,
    worker_count: usize,
    timeout: Duration,
) -> E2EResult<Value> {
    let (tx, rx) = mpsc::channel();
    let src = source.to_string();

    std::thread::spawn(move || {
        let result: E2EResult<Value> = (|| {
            let (runtime, program) = compile_program_with_mode(&src, BuiltinMode::RayaStrict)?;

            let mut vm = Vm::with_native_handler(worker_count, Arc::new(StdNativeHandler));

            // Register symbolic native functions for ModuleNativeCall dispatch.
            {
                let mut registry = vm.native_registry().write();
                raya_stdlib::register_stdlib(&mut registry);
                raya_stdlib_posix::register_posix(&mut registry);
            }

            let value = runtime
                .execute_program_with_vm(&program, &mut vm)
                .map_err(map_runtime_error)?;
            keep_vm_alive(vm);
            Ok(value)
        })();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(E2EError::Vm(VmError::RuntimeError(format!(
            "Multiworker+builtins execution timed out after {:?} (workers={})",
            timeout, worker_count
        )))),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(E2EError::Vm(VmError::RuntimeError(
            "Multiworker+builtins execution thread disconnected unexpectedly".to_string(),
        ))),
    }
}

/// Compile and execute with multiple workers and builtins, expecting a specific i32 result
#[allow(dead_code)]
pub fn expect_i32_multiworker_with_builtins(source: &str, expected: i32, worker_count: usize) {
    match compile_and_run_multiworker_with_builtins(source, worker_count) {
        Ok(value) => {
            if let Some(actual) = value.as_i32() {
                assert_eq!(actual, expected, "Wrong result for:\n{}", source);
                return;
            }
            if let Some(actual) = value.as_f64() {
                let expected_f64 = expected as f64;
                assert!(
                    (actual - expected_f64).abs() < 1e-10 && actual.fract() == 0.0,
                    "Expected {} (i32), got {} (f64) for:\n{}",
                    expected,
                    actual,
                    source
                );
                return;
            }
            panic!(
                "Expected i32 or f64 result, got {:?}\nSource:\n{}",
                value, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with multiple workers and builtins, expecting a specific bool result
#[allow(dead_code)]
pub fn expect_bool_multiworker_with_builtins(source: &str, expected: bool, worker_count: usize) {
    match compile_and_run_multiworker_with_builtins(source, worker_count) {
        Ok(value) => {
            let actual = value.as_bool().expect(&format!(
                "Expected bool result, got {:?}\nSource:\n{}",
                value, source
            ));
            assert_eq!(actual, expected, "Wrong result for:\n{}", source);
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Extract array of i32 values from a VM Value
fn extract_array_i32(value: &Value, source: &str) -> Vec<i32> {
    assert!(
        value.is_ptr(),
        "Expected array (pointer), got {:?}\nSource:\n{}",
        value,
        source
    );
    let arr_ptr = unsafe { value.as_ptr::<Array>() };
    let ptr = arr_ptr.unwrap_or_else(|| {
        panic!(
            "Failed to extract array pointer from {:?}\nSource:\n{}",
            value, source
        )
    });
    let array = unsafe { &*ptr.as_ptr() };
    let mut result = Vec::with_capacity(array.len());
    for i in 0..array.len() {
        let elem = array
            .get(i)
            .unwrap_or_else(|| panic!("Missing array element at index {}\nSource:\n{}", i, source));
        // Try i32 first, then f64 whole number
        if let Some(v) = elem.as_i32() {
            result.push(v);
        } else if let Some(v) = elem.as_f64() {
            assert!(
                v.fract() == 0.0,
                "Array element {} is f64 {} (not whole number)\nSource:\n{}",
                i,
                v,
                source
            );
            result.push(v as i32);
        } else {
            panic!(
                "Array element {} is not numeric: {:?}\nSource:\n{}",
                i, elem, source
            );
        }
    }
    result
}

/// Compile and execute, expecting an array of i32 results
#[allow(dead_code)]
pub fn expect_array_i32(source: &str, expected: &[i32]) {
    match compile_and_run(source) {
        Ok(value) => {
            let actual = extract_array_i32(&value, source);
            assert_eq!(
                actual.len(),
                expected.len(),
                "Array length mismatch: expected {}, got {}\nSource:\n{}",
                expected.len(),
                actual.len(),
                source
            );
            for (i, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    a, e,
                    "Array element {} mismatch: expected {}, got {}\nSource:\n{}",
                    i, e, a, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with builtins, expecting an array of i32 results
#[allow(dead_code)]
pub fn expect_array_i32_with_builtins(source: &str, expected: &[i32]) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            let actual = extract_array_i32(&value, source);
            assert_eq!(
                actual.len(),
                expected.len(),
                "Array length mismatch: expected {}, got {}\nSource:\n{}",
                expected.len(),
                actual.len(),
                source
            );
            for (i, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    a, e,
                    "Array element {} mismatch: expected {}, got {}\nSource:\n{}",
                    i, e, a, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with multiple workers and builtins, expecting an array of i32 results
#[allow(dead_code)]
pub fn expect_array_i32_multiworker_with_builtins(
    source: &str,
    expected: &[i32],
    worker_count: usize,
) {
    match compile_and_run_multiworker_with_builtins(source, worker_count) {
        Ok(value) => {
            let actual = extract_array_i32(&value, source);
            assert_eq!(
                actual.len(),
                expected.len(),
                "Array length mismatch: expected {}, got {}\nSource:\n{}",
                expected.len(),
                actual.len(),
                source
            );
            for (i, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    a, e,
                    "Array element {} mismatch: expected {}, got {}\nSource:\n{}",
                    i, e, a, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Extract object fields as i32 values from a VM Value
fn extract_object_i32_fields(value: &Value, source: &str) -> Vec<i32> {
    assert!(
        value.is_ptr(),
        "Expected object (pointer), got {:?}\nSource:\n{}",
        value,
        source
    );
    let obj_ptr = unsafe { value.as_ptr::<Object>() };
    let ptr = obj_ptr.unwrap_or_else(|| {
        panic!(
            "Failed to extract object pointer from {:?}\nSource:\n{}",
            value, source
        )
    });
    let object = unsafe { &*ptr.as_ptr() };
    let mut result = Vec::with_capacity(object.field_count());
    for i in 0..object.field_count() {
        let field = object
            .get_field(i)
            .unwrap_or_else(|| panic!("Missing object field at index {}\nSource:\n{}", i, source));
        if let Some(v) = field.as_i32() {
            result.push(v);
        } else if let Some(v) = field.as_f64() {
            assert!(
                v.fract() == 0.0,
                "Object field {} is f64 {} (not whole number)\nSource:\n{}",
                i,
                v,
                source
            );
            result.push(v as i32);
        } else {
            panic!(
                "Object field {} is not numeric: {:?}\nSource:\n{}",
                i, field, source
            );
        }
    }
    result
}

/// Compile and execute, expecting an object whose numeric fields match expected values (by index order)
#[allow(dead_code)]
pub fn expect_object_i32_fields(source: &str, expected: &[i32]) {
    match compile_and_run(source) {
        Ok(value) => {
            let actual = extract_object_i32_fields(&value, source);
            assert_eq!(
                actual.len(),
                expected.len(),
                "Object field count mismatch: expected {}, got {}\nSource:\n{}",
                expected.len(),
                actual.len(),
                source
            );
            for (i, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    a, e,
                    "Object field {} mismatch: expected {}, got {}\nSource:\n{}",
                    i, e, a, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with builtins, expecting an object whose numeric fields match expected values
#[allow(dead_code)]
pub fn expect_object_i32_fields_with_builtins(source: &str, expected: &[i32]) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            let actual = extract_object_i32_fields(&value, source);
            assert_eq!(
                actual.len(),
                expected.len(),
                "Object field count mismatch: expected {}, got {}\nSource:\n{}",
                expected.len(),
                actual.len(),
                source
            );
            for (i, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    a, e,
                    "Object field {} mismatch: expected {}, got {}\nSource:\n{}",
                    i, e, a, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with multiple workers and builtins, expecting an object whose numeric fields match
#[allow(dead_code)]
pub fn expect_object_i32_fields_multiworker_with_builtins(
    source: &str,
    expected: &[i32],
    worker_count: usize,
) {
    match compile_and_run_multiworker_with_builtins(source, worker_count) {
        Ok(value) => {
            let actual = extract_object_i32_fields(&value, source);
            assert_eq!(
                actual.len(),
                expected.len(),
                "Object field count mismatch: expected {}, got {}\nSource:\n{}",
                expected.len(),
                actual.len(),
                source
            );
            for (i, (&a, &e)) in actual.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    a, e,
                    "Object field {} mismatch: expected {}, got {}\nSource:\n{}",
                    i, e, a, source
                );
            }
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile source and dump debug info (useful for debugging failed tests)
pub fn debug_compile(source: &str) -> String {
    let parser = match Parser::new(source) {
        Ok(p) => p,
        Err(e) => return format!("Lexer error: {:?}", e),
    };

    let (ast, interner) = match parser.parse() {
        Ok(r) => r,
        Err(e) => return format!("Parse error: {:?}", e),
    };

    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner).with_mode(TypeSystemMode::Raya);

    // Register builtin signatures
    let builtin_sigs = raya_engine::builtins::to_checker_signatures();
    binder.register_builtins(&builtin_sigs);

    let symbols = match binder.bind_module(&ast) {
        Ok(s) => s,
        Err(e) => return format!("Binding error: {:?}", e),
    };

    let checker =
        TypeChecker::new(&mut type_ctx, &symbols, &interner).with_mode(TypeSystemMode::Raya);
    let check_result = match checker.check_module(&ast) {
        Ok(r) => r,
        Err(e) => return format!("Type check error: {:?}", e),
    };

    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(check_result.expr_types)
        .with_js_this_binding_compat(true);
    match compiler.compile_with_debug(&ast) {
        Ok((_, debug_output)) => debug_output,
        Err(e) => format!("Compile error: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_harness_works() {
        // Simple sanity check that the harness compiles
        let result = compile("return 42;");
        assert!(result.is_ok(), "Basic compilation should work");
    }
}
