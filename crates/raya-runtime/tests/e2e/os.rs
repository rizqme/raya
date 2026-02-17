//! E2E tests for std:os module

use super::harness::*;

#[test]
fn test_os_platform() {
    // On macOS this should be "darwin", on Linux "linux"
    expect_bool_with_builtins(r#"
        import os from "std:os";
        const p: string = os.platform();
        return p.length > 0;
    "#, true);
}

#[test]
fn test_os_arch() {
    expect_bool_with_builtins(r#"
        import os from "std:os";
        const a: string = os.arch();
        return a.length > 0;
    "#, true);
}

#[test]
fn test_os_hostname() {
    expect_bool_with_builtins(r#"
        import os from "std:os";
        const h: string = os.hostname();
        return h.length > 0;
    "#, true);
}

#[test]
fn test_os_cpus() {
    // Should return at least 1 CPU
    expect_bool_with_builtins(r#"
        import os from "std:os";
        return os.cpus() >= 1;
    "#, true);
}

#[test]
fn test_os_total_memory() {
    // Should be positive (at least 1GB = ~1e9)
    expect_bool_with_builtins(r#"
        import os from "std:os";
        return os.totalMemory() > 0;
    "#, true);
}

#[test]
fn test_os_eol() {
    expect_string_with_builtins(r#"
        import os from "std:os";
        return os.eol();
    "#, "\n");
}

#[test]
fn test_os_tmpdir() {
    expect_bool_with_builtins(r#"
        import os from "std:os";
        const t: string = os.tmpdir();
        return t.length > 0;
    "#, true);
}
