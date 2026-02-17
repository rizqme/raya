//! E2E tests for std:io module
//!
//! Note: stdin tests (readLine, readAll) are excluded because they block
//! waiting for input. Only output methods are tested.

use super::harness::*;

#[test]
fn test_io_write() {
    // write() should succeed without error (returns null)
    compile_and_run_with_builtins(r#"
        import io from "std:io";
        io.write("");
        return 1;
    "#).unwrap();
}

#[test]
fn test_io_writeln() {
    compile_and_run_with_builtins(r#"
        import io from "std:io";
        io.writeln("");
        return 1;
    "#).unwrap();
}

#[test]
fn test_io_write_err() {
    compile_and_run_with_builtins(r#"
        import io from "std:io";
        io.writeErr("");
        return 1;
    "#).unwrap();
}

#[test]
fn test_io_flush() {
    compile_and_run_with_builtins(r#"
        import io from "std:io";
        io.flush();
        return 1;
    "#).unwrap();
}
