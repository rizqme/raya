//! E2E tests for std:process module

use super::harness::*;

#[test]
fn test_process_pid() {
    // pid should be a positive number
    expect_bool_with_builtins(r#"
        import process from "std:process";
        return process.pid() > 0;
    "#, true);
}

#[test]
fn test_process_exec_path() {
    expect_bool_with_builtins(r#"
        import process from "std:process";
        const p: string = process.execPath();
        return p.length > 0;
    "#, true);
}

#[test]
fn test_process_exec_echo() {
    expect_string_with_builtins(r#"
        import process from "std:process";
        const h: number = process.exec("echo hello_raya");
        const stdout: string = process.execGetStdout(h);
        process.execRelease(h);
        return stdout;
    "#, "hello_raya\n");
}

#[test]
fn test_process_exec_code() {
    expect_bool_with_builtins(r#"
        import process from "std:process";
        const h: number = process.exec("true");
        const code: number = process.execGetCode(h);
        process.execRelease(h);
        return code == 0;
    "#, true);
}

#[test]
fn test_process_run() {
    expect_bool_with_builtins(r#"
        import process from "std:process";
        const code: number = process.run("true");
        return code == 0;
    "#, true);
}

#[test]
fn test_process_argv() {
    // argv should return an array (possibly empty in test context)
    compile_and_run_with_builtins(r#"
        import process from "std:process";
        const args: string[] = process.argv();
        return 1;
    "#).unwrap();
}
