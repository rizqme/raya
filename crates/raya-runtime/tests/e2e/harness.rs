//! Test harness for end-to-end compilation and execution
//!
//! Provides utilities for compiling Raya source code and executing it in the VM.

use raya_engine::compiler::{Compiler, Module};
use raya_engine::parser::checker::{Binder, TypeChecker, TypeSystemMode};
use raya_engine::parser::{Interner, Parser, TypeContext};
use raya_engine::vm::gc::header_ptr_from_value_ptr;
use raya_engine::vm::scheduler::SchedulerLimits;
use raya_engine::vm::{Array, Object, RayaString, Value, Vm, VmError};
use raya_runtime::{BuiltinMode, StdNativeHandler};
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

#[cfg(feature = "jit")]
use raya_engine::vm::interpreter::JitTelemetrySnapshot;

fn finalize_vm_after_result(value: &Value, mut vm: Vm) {
    // Drain terminal task/microtask completion work before shutdown so scheduler
    // teardown does not race pending reactor-side continuations across test boundaries.
    let _ = vm.wait_quiescent(Duration::from_millis(250));
    let _ = vm.wait_all(Duration::from_millis(250));
    vm.terminate();
    let _ = value;
}

fn finalize_vm_after_error(mut vm: Vm) {
    let _ = vm.wait_quiescent(Duration::from_millis(250));
    let _ = vm.wait_all(Duration::from_millis(250));
    vm.terminate();
}

fn extract_live_string(value: &Value, source: &str) -> String {
    if value.is_ptr() {
        let raw_ptr = unsafe { value.as_ptr::<u8>() };
        if let Some(ptr) = raw_ptr {
            let header = unsafe {
                &*header_ptr_from_value_ptr(ptr.as_ptr())
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
            raya_str.data.clone()
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
/// Uses the production runtime module pipeline (Module System V2) only.
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

fn run_joined<T, F>(thread_name: &str, f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let _slot_guard = acquire_global_harness_slot();
    let _ = thread_name;
    f()
}

struct GlobalHarnessSlotGuard {
    path: PathBuf,
    _file: File,
}

impl Drop for GlobalHarnessSlotGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn global_harness_slot_limit() -> Option<usize> {
    if let Ok(raw) = std::env::var("RAYA_E2E_MAX_PARALLEL") {
        let parsed = raw
            .parse::<usize>()
            .expect("RAYA_E2E_MAX_PARALLEL must be a positive integer");
        return Some(parsed.max(1));
    }
    if std::env::var_os("CI").is_some() {
        return Some(2);
    }
    None
}

fn global_harness_slot_dir() -> PathBuf {
    std::env::temp_dir().join("raya-e2e-harness-slots")
}

fn acquire_global_harness_slot() -> Option<GlobalHarnessSlotGuard> {
    let limit = global_harness_slot_limit()?;
    let dir = global_harness_slot_dir();
    fs::create_dir_all(&dir).expect("failed to create e2e slot directory");

    loop {
        for slot in 0..limit {
            let path = dir.join(format!("slot-{}.lock", slot));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Some(GlobalHarnessSlotGuard {
                        path,
                        _file: file,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("failed to acquire e2e harness slot: {}", error),
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn harness_vm_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_harness_vm_lock<T, F>(f: F) -> T
where
    F: FnOnce() -> T,
{
    let _guard = harness_vm_lock()
        .lock()
        .expect("test harness VM lock poisoned");
    f()
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

#[cfg(feature = "jit")]
fn compile_program_with_mode_jit(
    source: &str,
    mode: BuiltinMode,
) -> E2EResult<(raya_runtime::Runtime, raya_runtime::CompiledProgram)> {
    let runtime = raya_runtime::Runtime::with_options(raya_runtime::RuntimeOptions {
        builtin_mode: mode,
        no_jit: false,
        jit_threshold: 1,
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
    with_harness_vm_lock(|| {
        let (runtime, program) = compile_program_with_mode(source, BuiltinMode::RayaStrict)?;
        // Use single worker to avoid resource contention during parallel test execution.
        let mut vm = Vm::with_worker_count(1);
        match runtime.execute_program_with_vm(&program, &mut vm) {
            Ok(value) => {
                finalize_vm_after_result(&value, vm);
                Ok(value)
            }
            Err(error) => {
                finalize_vm_after_error(vm);
                Err(map_runtime_error(error))
            }
        }
    })
}

pub(crate) fn compile_and_run_isolated(source: &str) -> E2EResult<Value> {
    let owned = source.to_string();
    run_joined("raya-e2e-scalar", move || compile_and_run(&owned))
}

/// Compile and execute with builtins included
///
/// Use this for tests that use Map, Set, Buffer, Date, Channel, Logger, etc.
pub fn compile_and_run_with_builtins(source: &str) -> E2EResult<Value> {
    let owned = source.to_string();
    run_joined("raya-e2e-builtins", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;

            // Use single worker with StdNativeHandler for stdlib support (logger, etc.).
            let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));

            // Register symbolic native functions for ModuleNativeCall dispatch.
            {
                let mut registry = vm.native_registry().write();
                raya_stdlib::register_stdlib(&mut registry);
                raya_stdlib_posix::register_posix(&mut registry);
            }

            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    finalize_vm_after_result(&value, vm);
                    Ok(value)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
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
    let owned = source.to_string();
    run_joined("raya-e2e-runtime", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, mode)?;

            let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
            {
                let mut registry = vm.native_registry().write();
                raya_stdlib::register_stdlib(&mut registry);
                raya_stdlib_posix::register_posix(&mut registry);
            }

            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    finalize_vm_after_result(&value, vm);
                    Ok(value)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

pub fn compile_and_run_runtime_node_compat(source: &str) -> E2EResult<Value> {
    compile_and_run_runtime_with_mode(source, BuiltinMode::NodeCompat)
}

fn compile_and_run_string(source: &str) -> E2EResult<String> {
    let owned = source.to_string();
    run_joined("raya-e2e-string", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;
            let mut vm = Vm::with_worker_count(1);
            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    let result = extract_live_string(&value, &owned);
                    finalize_vm_after_result(&value, vm);
                    Ok(result)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

pub fn compile_and_run_string_with_builtins(source: &str) -> E2EResult<String> {
    let owned = source.to_string();
    run_joined("raya-e2e-string-builtins", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;

            let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
            {
                let mut registry = vm.native_registry().write();
                raya_stdlib::register_stdlib(&mut registry);
                raya_stdlib_posix::register_posix(&mut registry);
            }

            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    let result = extract_live_string(&value, &owned);
                    finalize_vm_after_result(&value, vm);
                    Ok(result)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

fn compile_and_run_string_runtime(source: &str) -> E2EResult<String> {
    compile_and_run_string_runtime_with_mode(source, BuiltinMode::RayaStrict)
}

fn compile_and_run_string_runtime_with_mode(source: &str, mode: BuiltinMode) -> E2EResult<String> {
    let owned = source.to_string();
    run_joined("raya-e2e-string-runtime", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, mode)?;

            let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
            {
                let mut registry = vm.native_registry().write();
                raya_stdlib::register_stdlib(&mut registry);
                raya_stdlib_posix::register_posix(&mut registry);
            }

            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    let result = extract_live_string(&value, &owned);
                    finalize_vm_after_result(&value, vm);
                    Ok(result)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

fn compile_and_run_string_runtime_node_compat(source: &str) -> E2EResult<String> {
    compile_and_run_string_runtime_with_mode(source, BuiltinMode::NodeCompat)
}

fn compile_and_run_array_i32(source: &str) -> E2EResult<Vec<i32>> {
    let owned = source.to_string();
    run_joined("raya-e2e-array", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;
            let mut vm = Vm::with_worker_count(1);
            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    let result = extract_array_i32(&value, &owned);
                    finalize_vm_after_result(&value, vm);
                    Ok(result)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

fn compile_and_run_array_i32_with_builtins(source: &str) -> E2EResult<Vec<i32>> {
    let owned = source.to_string();
    run_joined("raya-e2e-array-builtins", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;

            let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
            {
                let mut registry = vm.native_registry().write();
                raya_stdlib::register_stdlib(&mut registry);
                raya_stdlib_posix::register_posix(&mut registry);
            }

            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    let result = extract_array_i32(&value, &owned);
                    finalize_vm_after_result(&value, vm);
                    Ok(result)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

fn compile_and_run_array_i32_multiworker_with_builtins(
    source: &str,
    worker_count: usize,
) -> E2EResult<Vec<i32>> {
    with_harness_vm_lock(|| {
        let (runtime, program) = compile_program_with_mode(source, BuiltinMode::RayaStrict)?;
        let mut vm = Vm::with_native_handler(worker_count, Arc::new(StdNativeHandler));
        {
            let mut registry = vm.native_registry().write();
            raya_stdlib::register_stdlib(&mut registry);
            raya_stdlib_posix::register_posix(&mut registry);
        }

        match runtime.execute_program_with_vm(&program, &mut vm) {
            Ok(value) => {
                let result = extract_array_i32(&value, source);
                finalize_vm_after_result(&value, vm);
                Ok(result)
            }
            Err(error) => {
                finalize_vm_after_error(vm);
                Err(map_runtime_error(error))
            }
        }
    })
}

fn compile_and_run_object_i32_fields(source: &str) -> E2EResult<Vec<i32>> {
    let owned = source.to_string();
    run_joined("raya-e2e-object", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;
            let mut vm = Vm::with_worker_count(1);
            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    let result = extract_object_i32_fields(&value, &owned);
                    finalize_vm_after_result(&value, vm);
                    Ok(result)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

fn compile_and_run_object_i32_fields_with_builtins(source: &str) -> E2EResult<Vec<i32>> {
    let owned = source.to_string();
    run_joined("raya-e2e-object-builtins", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;

            let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
            {
                let mut registry = vm.native_registry().write();
                raya_stdlib::register_stdlib(&mut registry);
                raya_stdlib_posix::register_posix(&mut registry);
            }

            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    let result = extract_object_i32_fields(&value, &owned);
                    finalize_vm_after_result(&value, vm);
                    Ok(result)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

fn compile_and_run_object_i32_fields_multiworker_with_builtins(
    source: &str,
    worker_count: usize,
) -> E2EResult<Vec<i32>> {
    with_harness_vm_lock(|| {
        let (runtime, program) = compile_program_with_mode(source, BuiltinMode::RayaStrict)?;
        let mut vm = Vm::with_native_handler(worker_count, Arc::new(StdNativeHandler));
        {
            let mut registry = vm.native_registry().write();
            raya_stdlib::register_stdlib(&mut registry);
            raya_stdlib_posix::register_posix(&mut registry);
        }

        match runtime.execute_program_with_vm(&program, &mut vm) {
            Ok(value) => {
                let result = extract_object_i32_fields(&value, source);
                finalize_vm_after_result(&value, vm);
                Ok(result)
            }
            Err(error) => {
                finalize_vm_after_error(vm);
                Err(map_runtime_error(error))
            }
        }
    })
}

#[cfg(feature = "jit")]
pub fn compile_and_run_runtime_with_mode_jit(
    source: &str,
    mode: BuiltinMode,
) -> E2EResult<(Value, JitTelemetrySnapshot)> {
    with_harness_vm_lock(|| {
        let (runtime, program) = compile_program_with_mode_jit(source, mode)?;

        let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
        {
            let mut registry = vm.native_registry().write();
            raya_stdlib::register_stdlib(&mut registry);
            raya_stdlib_posix::register_posix(&mut registry);
        }

        match runtime.execute_program_with_vm(&program, &mut vm) {
            Ok(value) => {
                let telemetry = vm.get_jit_telemetry();
                finalize_vm_after_result(&value, vm);
                Ok((value, telemetry))
            }
            Err(error) => {
                finalize_vm_after_error(vm);
                Err(map_runtime_error(error))
            }
        }
    })
}

#[cfg(feature = "jit")]
pub fn compile_and_run_runtime_jit(source: &str) -> E2EResult<(Value, JitTelemetrySnapshot)> {
    compile_and_run_runtime_with_mode_jit(source, BuiltinMode::RayaStrict)
}

#[cfg(feature = "jit")]
pub fn expect_i32_runtime_jit(source: &str, expected: i32) -> JitTelemetrySnapshot {
    match compile_and_run_runtime_jit(source) {
        Ok((value, telemetry)) => {
            if let Some(actual) = value.as_i32() {
                assert_eq!(actual, expected, "Wrong result for:\n{}", source);
                return telemetry;
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
                return telemetry;
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
    match compile_and_run_string_runtime_node_compat(source) {
        Ok(actual) => {
            assert_eq!(
                actual, expected,
                "String mismatch.\nExpected: '{}'\nGot: '{}'\nSource:\n{}",
                expected, actual, source
            );
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
    match compile_and_run_isolated(source) {
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
    match compile_and_run_isolated(source) {
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
    match compile_and_run_isolated(source) {
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
    match compile_and_run_isolated(source) {
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
    match compile_and_run_string(source) {
        Ok(actual) => {
            assert_eq!(
                actual, expected,
                "String mismatch.\nExpected: '{}'\nGot: '{}'\nSource:\n{}",
                expected, actual, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with builtins, expecting a specific string result
pub fn expect_string_with_builtins(source: &str, expected: &str) {
    match compile_and_run_string_with_builtins(source) {
        Ok(actual) => {
            assert_eq!(
                actual, expected,
                "String mismatch.\nExpected: '{}'\nGot: '{}'\nSource:\n{}",
                expected, actual, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

pub fn expect_string_runtime(source: &str, expected: &str) {
    match compile_and_run_string_runtime(source) {
        Ok(actual) => {
            assert_eq!(
                actual, expected,
                "String mismatch.\nExpected: '{}'\nGot: '{}'\nSource:\n{}",
                expected, actual, source
            );
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with builtins, expecting a string result containing a pattern
pub fn expect_string_contains_with_builtins(source: &str, pattern: &str) {
    match compile_and_run_string_with_builtins(source) {
        Ok(actual) => {
            assert!(
                actual.contains(pattern),
                "String does not contain pattern.\nExpected to contain: '{}'\nGot: '{}'\nSource:\n{}",
                pattern, actual, source
            );
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
            let error_msg_lower = error_msg.to_lowercase();
            let pattern_lower = error_pattern.to_lowercase();
            assert!(
                error_msg_lower.contains(&pattern_lower),
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
    match compile_and_run_isolated(source) {
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
    _timeout: Duration,
) -> E2EResult<Value> {
    let owned = source.to_string();
    run_joined("raya-e2e-multiworker", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;
            let mut vm = Vm::with_worker_count(worker_count);
            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    finalize_vm_after_result(&value, vm);
                    Ok(value)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
}

fn compile_and_run_multiworker_with_builtins_timeout(
    source: &str,
    worker_count: usize,
    _timeout: Duration,
) -> E2EResult<Value> {
    let owned = source.to_string();
    run_joined("raya-e2e-multiworker-builtins", move || {
        with_harness_vm_lock(|| {
            let (runtime, program) = compile_program_with_mode(&owned, BuiltinMode::RayaStrict)?;

            let mut vm = Vm::with_native_handler(worker_count, Arc::new(StdNativeHandler));

            // Register symbolic native functions for ModuleNativeCall dispatch.
            {
                let mut registry = vm.native_registry().write();
                raya_stdlib::register_stdlib(&mut registry);
                raya_stdlib_posix::register_posix(&mut registry);
            }

            match runtime.execute_program_with_vm(&program, &mut vm) {
                Ok(value) => {
                    finalize_vm_after_result(&value, vm);
                    Ok(value)
                }
                Err(error) => {
                    finalize_vm_after_error(vm);
                    Err(map_runtime_error(error))
                }
            }
        })
    })
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
    match compile_and_run_array_i32(source) {
        Ok(actual) => {
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
    match compile_and_run_array_i32_with_builtins(source) {
        Ok(actual) => {
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
    match compile_and_run_array_i32_multiworker_with_builtins(source, worker_count) {
        Ok(actual) => {
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
    match compile_and_run_object_i32_fields(source) {
        Ok(actual) => {
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
    match compile_and_run_object_i32_fields_with_builtins(source) {
        Ok(actual) => {
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
    match compile_and_run_object_i32_fields_multiworker_with_builtins(source, worker_count) {
        Ok(actual) => {
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
        .with_type_annotation_types(check_result.type_annotation_types)
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
