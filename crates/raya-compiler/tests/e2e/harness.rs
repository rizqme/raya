//! Test harness for end-to-end compilation and execution
//!
//! Provides utilities for compiling Raya source code and executing it in the VM.

use raya_compiler::{Compiler, Module};
use raya_core::{Value, Vm, VmError, RayaString};
use raya_parser::{Interner, Parser, TypeContext};
use raya_parser::checker::{Binder, TypeChecker};

/// Get the builtin source files content
///
/// Returns the source code for Map, Set, Buffer, Date, Channel classes.
fn get_builtin_sources() -> &'static str {
    concat!(
        // Map class
        include_str!("../../../raya-builtins/builtins/Map.raya"),
        "\n",
        // Set class
        include_str!("../../../raya-builtins/builtins/Set.raya"),
        "\n",
        // Buffer class
        include_str!("../../../raya-builtins/builtins/Buffer.raya"),
        "\n",
        // Date class
        include_str!("../../../raya-builtins/builtins/Date.raya"),
        "\n",
        // Channel class
        include_str!("../../../raya-builtins/builtins/Channel.raya"),
        "\n",
        // Mutex class (simplified - lock/unlock only)
        "class Mutex {
            private handle: number;

            constructor() {
                this.handle = __OPCODE_MUTEX_NEW();
            }

            lock(): void {
                __OPCODE_MUTEX_LOCK(this.handle);
            }

            unlock(): void {
                __OPCODE_MUTEX_UNLOCK(this.handle);
            }
        }
        ",
        "\n",
        // Task class (simplified - for runtime Task objects)
        "class Task<T> {
            private handle: number;

            private constructor(handle: number) {
                this.handle = handle;
            }

            cancel(): void {
                __OPCODE_TASK_CANCEL(this.handle);
            }
        }
        ",
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
    Compile(raya_compiler::CompileError),
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
    // Optionally prepend builtin sources
    let full_source = if include_builtins {
        format!("{}\n{}", get_builtin_sources(), source)
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
        let empty_sigs: Vec<raya_parser::checker::BuiltinSignatures> = vec![];
        binder.register_builtins(&empty_sigs);
    } else {
        // Normal mode: register type signatures from precompiled builtins
        let builtin_sigs = raya_builtins::to_checker_signatures();
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
        symbols.update_type(raya_parser::checker::ScopeId(scope_id), &name, ty);
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

    let mut vm = Vm::new();
    vm.execute(&module).map_err(E2EError::Vm)
}

/// Compile and execute with builtins included
///
/// Use this for tests that use Map, Set, Buffer, Date, Channel, etc.
pub fn compile_and_run_with_builtins(source: &str) -> E2EResult<Value> {
    let (module, _interner) = compile_with_builtins(source)?;

    let mut vm = Vm::new();
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
    let builtin_sigs = raya_builtins::to_checker_signatures();
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
