//! Test runner — compiles and executes test files, collecting results.
//!
//! The test runner:
//! 1. Reads the test file source
//! 2. Prepends std:test source and appends `__runTests();`
//! 3. Compiles using the standard `compile_source` pipeline
//! 4. Creates a VM with stdlib + test result handlers
//! 5. Executes the module and returns collected results

use raya_engine::vm::Vm;
use raya_stdlib::StdNativeHandler;
use raya_stdlib::test::{self, SharedTestResults};
use std::path::Path;
use std::sync::Arc;

// Re-export types for consumers (e.g., raya-cli)
pub use raya_stdlib::test::{TestResult, TestResults};

use crate::compile;
use crate::error::RuntimeError;
use crate::RuntimeOptions;

/// The std:test source, embedded at compile time.
const TEST_SOURCE: &str = include_str!("../../raya-stdlib/raya/test.raya");

/// Run a single test file and return its results.
pub fn run_test_file(
    path: &Path,
    options: &RuntimeOptions,
) -> Result<TestFileResult, RuntimeError> {
    let source = std::fs::read_to_string(path)?;
    run_test_source(&source, path, options)
}

/// Run test source code and return results.
///
/// Prepends std:test, appends `__runTests()`, compiles, then executes
/// on a VM with test result collection handlers.
pub fn run_test_source(
    source: &str,
    file_path: &Path,
    options: &RuntimeOptions,
) -> Result<TestFileResult, RuntimeError> {
    // Build test source: test framework + user code + runner invocation
    let full_test_source = format!("{}\n{}\n__runTests();\n", TEST_SOURCE, source);

    // Compile (compile_source prepends builtins + stdlib)
    let (module, _interner) = compile::compile_source(&full_test_source)?;

    // Create VM with test handlers
    let results = test::new_results();
    let mut vm = create_test_vm(options, results.clone());

    // Execute
    let exec_result = vm.execute(&module);

    // Collect results regardless of VM outcome
    let test_results = results.lock().clone();

    let error = match exec_result {
        Ok(_) => None,
        Err(e) => Some(format!("{}", e)),
    };

    Ok(TestFileResult {
        file: file_path.to_path_buf(),
        results: test_results,
        execution_error: error,
    })
}

/// Result of running a single test file.
#[derive(Debug, Clone)]
pub struct TestFileResult {
    /// Path to the test file.
    pub file: std::path::PathBuf,
    /// Collected test results.
    pub results: TestResults,
    /// Execution-level error (e.g., compilation failure, VM crash).
    /// Individual test failures are in `results`, not here.
    pub execution_error: Option<String>,
}

impl TestFileResult {
    /// Number of passed tests.
    pub fn passed(&self) -> usize {
        self.results.passed()
    }

    /// Number of failed tests.
    pub fn failed(&self) -> usize {
        self.results.failed()
    }

    /// Total number of tests that ran.
    pub fn total(&self) -> usize {
        self.results.results.len()
    }

    /// Whether the file had any failures (test or execution).
    pub fn has_failures(&self) -> bool {
        self.results.failed() > 0 || self.execution_error.is_some()
    }

    /// Total duration in milliseconds.
    pub fn duration_ms(&self) -> f64 {
        self.results.total_duration_ms()
    }
}

// ── Internal ─────────────────────────────────────────────────────────────

/// Create a VM configured for test execution (stdlib + test handlers).
fn create_test_vm(options: &RuntimeOptions, results: SharedTestResults) -> Vm {
    let threads = if options.threads == 0 {
        num_cpus::get()
    } else {
        options.threads
    };

    let vm = Vm::with_native_handler(threads, Arc::new(StdNativeHandler));

    // Register all stdlib + test native functions
    {
        let mut registry = vm.native_registry().write();
        raya_stdlib::register_stdlib(&mut registry);
        raya_stdlib_posix::register_posix(&mut registry);
        test::register_test(&mut registry, results);
    }

    vm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_no_assertions() {
        let options = RuntimeOptions::default();
        let result = run_test_source(
            "test(\"simple\", () => { let x = 1; });\n",
            Path::new("simple.test.raya"),
            &options,
        );
        let r = result.expect("should compile and run");
        assert!(
            r.execution_error.is_none(),
            "no execution error: {:?}",
            r.execution_error
        );
        assert_eq!(r.total(), 1, "should have 1 test");
        assert_eq!(r.passed(), 1, "should have 1 passing test");
        assert_eq!(r.failed(), 0, "should have 0 failing tests");
    }

    #[test]
    fn test_run_single_passing_test() {
        let options = RuntimeOptions::default();
        let result = run_test_source(
            "test(\"simple eq\", () => { expect(2).toBe(2); });\n",
            Path::new("simple.test.raya"),
            &options,
        );
        let r = result.expect("should compile and run");
        assert!(
            r.execution_error.is_none(),
            "no execution error: {:?}",
            r.execution_error
        );
        assert_eq!(r.total(), 1, "should have 1 test");
        assert_eq!(r.passed(), 1, "should have 1 passing test");
        assert_eq!(r.failed(), 0, "should have 0 failing tests");
    }

    #[test]
    fn test_run_failing_test() {
        let options = RuntimeOptions::default();
        let result = run_test_source(
            "test(\"will fail\", () => { expect(1).toBe(2); });\n",
            Path::new("fail.test.raya"),
            &options,
        );
        let r = result.expect("should compile and run");
        assert!(r.execution_error.is_none(), "no execution error: {:?}", r.execution_error);
        assert_eq!(r.total(), 1);
        assert_eq!(r.passed(), 0);
        assert_eq!(r.failed(), 1);
    }

    #[test]
    fn test_run_mixed_results() {
        let source = r#"
test("passes", () => { expect(true).toBeTruthy(); });
test("fails", () => { expect(false).toBeTruthy(); });
test("also passes", () => { expect(42).toBe(42); });
"#;
        let options = RuntimeOptions::default();
        let r = run_test_source(source, Path::new("mixed.test.raya"), &options)
            .expect("should compile and run");
        assert!(r.execution_error.is_none(), "no execution error: {:?}", r.execution_error);
        assert_eq!(r.total(), 3);
        assert_eq!(r.passed(), 2);
        assert_eq!(r.failed(), 1);
    }

    #[test]
    fn test_describe_groups() {
        let source = r#"
describe("Math", () => {
    it("adds numbers", () => { expect(1 + 2).toBe(3); });
    it("subtracts numbers", () => { expect(5 - 3).toBe(2); });
});
"#;
        let options = RuntimeOptions::default();
        let r = run_test_source(source, Path::new("describe.test.raya"), &options)
            .expect("should compile and run");
        assert!(r.execution_error.is_none(), "no execution error: {:?}", r.execution_error);
        assert_eq!(r.total(), 2);
        assert_eq!(r.passed(), 2);
    }

    #[test]
    fn test_expect_not() {
        let source = r#"
test("not.toBe", () => { expect(1).not.toBe(2); });
test("not.toBeNull", () => { expect(42).not.toBeNull(); });
"#;
        let options = RuntimeOptions::default();
        let r = run_test_source(source, Path::new("not.test.raya"), &options)
            .expect("should compile and run");
        assert!(r.execution_error.is_none(), "no execution error: {:?}", r.execution_error);
        assert_eq!(r.total(), 2);
        assert_eq!(r.passed(), 2);
    }

    #[test]
    fn test_expect_to_throw() {
        let source = r#"
test("catches throw", () => {
    expectToThrow(() => { throw new Error("boom"); });
});
test("catches non-throw", () => {
    expectNotToThrow(() => { let x = 1; });
});
"#;
        let options = RuntimeOptions::default();
        let r = run_test_source(source, Path::new("throw.test.raya"), &options)
            .expect("should compile and run");
        assert!(r.execution_error.is_none(), "no execution error: {:?}", r.execution_error);
        assert_eq!(r.total(), 2);
        assert_eq!(r.passed(), 2);
    }

    #[test]
    fn test_hooks() {
        let source = r#"
let count: number = 0;
beforeEach(() => { count = count + 1; });
test("first", () => { expect(count).toBe(1); });
test("second", () => { expect(count).toBe(2); });
"#;
        let options = RuntimeOptions::default();
        let r = run_test_source(source, Path::new("hooks.test.raya"), &options)
            .expect("should compile and run");
        assert!(r.execution_error.is_none(), "no execution error: {:?}", r.execution_error);
        assert_eq!(r.total(), 2);
        assert_eq!(r.passed(), 2);
    }
}
