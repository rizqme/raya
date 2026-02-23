//! End-to-end tests for std:glob module

use super::harness::*;

#[test]
fn test_glob_matches_pattern() {
    expect_bool_with_builtins(
        r#"
        import glob from "std:glob";
        return glob.matches("src/main.raya", "src/*.raya") && !glob.matches("src/main.ts", "src/*.raya");
    "#,
        true,
    );
}

#[test]
fn test_glob_find_in_dir() {
    expect_bool_with_builtins(
        r#"
        import fs from "std:fs";
        import glob from "std:glob";
        const base = fs.tempDir() + "/raya_glob_test_1";
        fs.mkdir(base);
        fs.writeTextFile(base + "/a.raya", "a");
        fs.writeTextFile(base + "/b.txt", "b");
        const found = glob.findInDir("*.raya", base);
        const ok = found.length == 1;
        fs.remove(base + "/a.raya");
        fs.remove(base + "/b.txt");
        fs.rmdir(base);
        return ok;
    "#,
        true,
    );
}
