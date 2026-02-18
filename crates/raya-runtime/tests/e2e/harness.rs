//! Test harness for end-to-end compilation and execution
//!
//! Provides utilities for compiling Raya source code and executing it in the VM.

use raya_engine::compiler::{Compiler, Module};
use raya_engine::vm::{Value, Vm, VmError, RayaString};
use raya_engine::parser::{Interner, Parser, TypeContext};
use raya_engine::parser::checker::{Binder, TypeChecker};
use raya_runtime::StdNativeHandler;
use std::sync::Arc;

/// Get the builtin source files content
///
/// Returns the source code for all builtin classes.
fn get_builtin_sources() -> &'static str {
    concat!(
        // Object class (base class)
        include_str!("../../../raya-engine/builtins/Object.raya"),
        "\n",
        // Error classes (must come before other classes that might throw)
        include_str!("../../../raya-engine/builtins/Error.raya"),
        "\n",
        // Map class
        include_str!("../../../raya-engine/builtins/Map.raya"),
        "\n",
        // Set class
        include_str!("../../../raya-engine/builtins/Set.raya"),
        "\n",
        // Buffer class
        include_str!("../../../raya-engine/builtins/Buffer.raya"),
        "\n",
        // Date class
        include_str!("../../../raya-engine/builtins/Date.raya"),
        "\n",
        // Channel class
        include_str!("../../../raya-engine/builtins/Channel.raya"),
        "\n",
        // Mutex class
        include_str!("../../../raya-engine/builtins/Mutex.raya"),
        "\n",
        // Task class
        include_str!("../../../raya-engine/builtins/Task.raya"),
        "\n",
    )
}

/// Get the standard library module sources
///
/// Returns source code for std: modules (Logger, etc.) that are
/// included as builtins for single-file compilation in tests.
fn get_std_sources() -> &'static str {
    concat!(
        // Logger (std:logger)
        include_str!("../../../raya-stdlib/raya/logger.raya"),
        "\n",
        // Math (std:math)
        include_str!("../../../raya-stdlib/raya/math.raya"),
        "\n",
        // Reflect (std:reflect)
        include_str!("../../../raya-stdlib/raya/reflect.raya"),
        "\n",
        // Runtime (std:runtime)
        include_str!("../../../raya-stdlib/raya/runtime.raya"),
        "\n",
        // Crypto (std:crypto)
        include_str!("../../../raya-stdlib/raya/crypto.raya"),
        "\n",
        // Time (std:time)
        include_str!("../../../raya-stdlib/raya/time.raya"),
        "\n",
        // Path (std:path)
        include_str!("../../../raya-stdlib/raya/path.raya"),
        "\n",
        // Stream (std:stream)
        include_str!("../../../raya-stdlib/raya/stream.raya"),
        "\n",
        // POSIX stdlib modules
        // Fs (std:fs)
        include_str!("../../../raya-stdlib-posix/raya/fs.raya"),
        "\n",
        // Net (std:net)
        include_str!("../../../raya-stdlib-posix/raya/net.raya"),
        "\n",
        // Http (std:http)
        include_str!("../../../raya-stdlib-posix/raya/http.raya"),
        "\n",
        // Fetch (std:fetch)
        include_str!("../../../raya-stdlib-posix/raya/fetch.raya"),
        "\n",
        // Env (std:env)
        include_str!("../../../raya-stdlib-posix/raya/env.raya"),
        "\n",
        // Process (std:process)
        include_str!("../../../raya-stdlib-posix/raya/process.raya"),
        "\n",
        // Os (std:os)
        include_str!("../../../raya-stdlib-posix/raya/os.raya"),
        "\n",
        // Io (std:io)
        include_str!("../../../raya-stdlib-posix/raya/io.raya"),
        "\n",
    )
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
    compile_internal(source, false)
}

/// Compile Raya source code with builtin classes included
///
/// This prepends the builtin .raya source files (Map, Set, Buffer, Date, Channel)
/// so they are compiled together with the user code.
pub fn compile_with_builtins(source: &str) -> E2EResult<(Module, Interner)> {
    compile_internal(source, true)
}

/// Internal compile function
fn compile_internal(source: &str, include_builtins: bool) -> E2EResult<(Module, Interner)> {
    // Optionally prepend builtin and std sources
    let full_source = if include_builtins {
        format!("{}\n{}\n{}", get_builtin_sources(), get_std_sources(), source)
    } else {
        source.to_string()
    };

    // Parse
    let parser = Parser::new(&full_source).map_err(|e| E2EError::Lex(format!("{:?}", e)))?;
    let (ast, interner) = parser.parse().map_err(|e| E2EError::Parse(format!("{:?}", e)))?;

    // Bind (creates symbol table)
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner);

    // Register builtin type signatures only if NOT including builtin sources
    // (to avoid duplicate symbol errors when source files define the same classes)
    if include_builtins {
        // When including builtin sources, just register intrinsics (__NATIVE_CALL, etc.)
        let empty_sigs: Vec<raya_engine::parser::checker::BuiltinSignatures> = vec![];
        binder.register_builtins(&empty_sigs);
    } else {
        // Normal mode: register type signatures from precompiled builtins
        let builtin_sigs = raya_engine::builtins::to_checker_signatures();
        binder.register_builtins(&builtin_sigs);
    }

    let mut symbols = binder
        .bind_module(&ast)
        .map_err(|e| E2EError::TypeCheck(format!("Binding error: {:?}", e)))?;

    // Type check
    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let check_result = checker
        .check_module(&ast)
        .map_err(|e| E2EError::TypeCheck(format!("{:?}", e)))?;

    // Apply inferred types to symbol table
    for ((scope_id, name), ty) in check_result.inferred_types {
        symbols.update_type(raya_engine::parser::checker::ScopeId(scope_id), &name, ty);
    }

    // Note: check_result.captures contains closure capture info for future use

    // Compile via IR pipeline with expression types from type checker
    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(check_result.expr_types);
    let bytecode = compiler
        .compile_via_ir(&ast)
        .map_err(E2EError::Compile)?;

    Ok((bytecode, interner))
}

/// Compile and execute Raya source code, returning the result
///
/// The source code should have a `main` function that returns a value,
/// or use a `return` statement at the top level.
pub fn compile_and_run(source: &str) -> E2EResult<Value> {
    let (module, _interner) = compile(source)?;

    // Use single worker to avoid resource contention during parallel test execution
    let mut vm = Vm::with_worker_count(1);
    vm.execute(&module).map_err(E2EError::Vm)
}

/// Compile and execute with builtins included
///
/// Use this for tests that use Map, Set, Buffer, Date, Channel, Logger, etc.
pub fn compile_and_run_with_builtins(source: &str) -> E2EResult<Value> {
    let (module, _interner) = compile_with_builtins(source)?;

    // Use single worker with StdNativeHandler for stdlib support (logger, etc.)
    let mut vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));

    // Register symbolic native functions for ModuleNativeCall dispatch
    {
        let mut registry = vm.native_registry().write();
        raya_stdlib::register_stdlib(&mut registry);
        raya_stdlib_posix::register_posix(&mut registry);
    }

    vm.execute(&module).map_err(E2EError::Vm)
}

/// Compile and execute with builtins, expecting a specific i32 result
pub fn expect_i32_with_builtins(source: &str, expected: i32) {
    match compile_and_run_with_builtins(source) {
        Ok(value) => {
            let actual = value.as_i32().expect(&format!(
                "Expected i32 result, got {:?}\nSource:\n{}",
                value, source
            ));
            assert_eq!(actual, expected, "Wrong result for:\n{}", source);
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
                expected, actual, source
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
                    expected, actual, source
                );
                return;
            }
            panic!("Expected i32 or f64 result, got {:?}\nSource:\n{}", value, source);
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
                expected, actual, source
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
                value, source
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
                // Extract string from pointer
                // SAFETY: We trust that string values from the VM are RayaString pointers
                let str_ptr = unsafe { value.as_ptr::<RayaString>() };
                if let Some(ptr) = str_ptr {
                    let raya_str = unsafe { &*ptr.as_ptr() };
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
                let str_ptr = unsafe { value.as_ptr::<RayaString>() };
                if let Some(ptr) = str_ptr {
                    let raya_str = unsafe { &*ptr.as_ptr() };
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
                error_pattern, error_msg, source
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
                error_pattern, error_msg, source
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
                error_pattern, error_msg, source
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
    let (module, _interner) = compile(source)?;

    let mut vm = Vm::with_worker_count(worker_count);
    vm.execute(&module).map_err(E2EError::Vm)
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
                    expected, actual, source
                );
                return;
            }
            panic!("Expected i32 or f64 result, got {:?}\nSource:\n{}", value, source);
        }
        Err(e) => {
            panic!("Compilation/execution failed: {}\nSource:\n{}", e, source);
        }
    }
}

/// Compile and execute with multiple worker threads and builtins
///
/// Use this for tests that need to stress-test true parallel execution with Mutex/Channel.
pub fn compile_and_run_multiworker_with_builtins(source: &str, worker_count: usize) -> E2EResult<Value> {
    let (module, _interner) = compile_with_builtins(source)?;

    let mut vm = Vm::with_native_handler(worker_count, Arc::new(StdNativeHandler));

    // Register symbolic native functions for ModuleNativeCall dispatch
    {
        let mut registry = vm.native_registry().write();
        raya_stdlib::register_stdlib(&mut registry);
        raya_stdlib_posix::register_posix(&mut registry);
    }

    vm.execute(&module).map_err(E2EError::Vm)
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
                    expected, actual, source
                );
                return;
            }
            panic!("Expected i32 or f64 result, got {:?}\nSource:\n{}", value, source);
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
    let mut binder = Binder::new(&mut type_ctx, &interner);

    // Register builtin signatures
    let builtin_sigs = raya_engine::builtins::to_checker_signatures();
    binder.register_builtins(&builtin_sigs);

    let symbols = match binder.bind_module(&ast) {
        Ok(s) => s,
        Err(e) => return format!("Binding error: {:?}", e),
    };

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let check_result = match checker.check_module(&ast) {
        Ok(r) => r,
        Err(e) => return format!("Type check error: {:?}", e),
    };

    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(check_result.expr_types);
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
