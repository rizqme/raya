//! E2E tests for std:fs module

use super::harness::*;

#[test]
fn test_fs_temp_dir() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const td: string = fs.tempDir();
        return td.length > 0;
    "#, true);
}

#[test]
fn test_fs_write_and_read_text() {
    expect_string_with_builtins(r#"
        import fs from "std:fs";
        const fp: string = fs.tempFile("raya_test_rw_");
        fs.writeTextFile(fp, "hello raya");
        const txt: string = fs.readTextFile(fp);
        fs.remove(fp);
        return txt;
    "#, "hello raya");
}

#[test]
fn test_fs_exists() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const fp: string = fs.tempFile("raya_test_exists_");
        const ex: boolean = fs.exists(fp);
        fs.remove(fp);
        return ex;
    "#, true);
}

#[test]
fn test_fs_exists_false() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        return fs.exists("/tmp/raya_nonexistent_file_xyz_123");
    "#, false);
}

#[test]
fn test_fs_is_file() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const fp: string = fs.tempFile("raya_test_isfile_");
        const res: boolean = fs.isFile(fp);
        fs.remove(fp);
        return res;
    "#, true);
}

#[test]
fn test_fs_is_dir() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        return fs.isDir(fs.tempDir());
    "#, true);
}

#[test]
fn test_fs_append_file() {
    expect_string_with_builtins(r#"
        import fs from "std:fs";
        const fp: string = fs.tempFile("raya_test_append_");
        fs.writeTextFile(fp, "hello");
        fs.appendFile(fp, " world");
        const txt: string = fs.readTextFile(fp);
        fs.remove(fp);
        return txt;
    "#, "hello world");
}

#[test]
fn test_fs_file_size() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const fp: string = fs.tempFile("raya_test_size_");
        fs.writeTextFile(fp, "12345");
        const sz: number = fs.fileSize(fp);
        fs.remove(fp);
        return sz == 5;
    "#, true);
}

#[test]
fn test_fs_mkdir_and_rmdir() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const d: string = fs.tempDir() + "/raya_test_mkdir_" + "1";
        fs.mkdir(d);
        const ex: boolean = fs.isDir(d);
        fs.rmdir(d);
        return ex;
    "#, true);
}

#[test]
fn test_fs_mkdir_recursive() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const base: string = fs.tempDir() + "/raya_test_mkdirr_" + "1";
        const deep: string = base + "/a/b/c";
        fs.mkdirRecursive(deep);
        const ex: boolean = fs.isDir(deep);
        fs.rmdir(deep);
        fs.rmdir(base + "/a/b");
        fs.rmdir(base + "/a");
        fs.rmdir(base);
        return ex;
    "#, true);
}

#[test]
fn test_fs_rename() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const src: string = fs.tempFile("raya_test_ren_src_");
        const dst: string = fs.tempDir() + "/raya_test_ren_dst_1";
        fs.writeTextFile(src, "data");
        fs.rename(src, dst);
        const moved: boolean = fs.exists(dst) && !fs.exists(src);
        fs.remove(dst);
        return moved;
    "#, true);
}

#[test]
fn test_fs_copy() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const src: string = fs.tempFile("raya_test_cp_src_");
        const dst: string = fs.tempDir() + "/raya_test_cp_dst_1";
        fs.writeTextFile(src, "copy me");
        fs.copy(src, dst);
        const txt: string = fs.readTextFile(dst);
        fs.remove(src);
        fs.remove(dst);
        return txt == "copy me";
    "#, true);
}

#[test]
fn test_fs_read_dir() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const d: string = fs.tempDir() + "/raya_test_readdir_" + "1";
        fs.mkdir(d);
        fs.writeTextFile(d + "/a.txt", "a");
        fs.writeTextFile(d + "/b.txt", "b");
        const entries: string[] = fs.readDir(d);
        fs.remove(d + "/a.txt");
        fs.remove(d + "/b.txt");
        fs.rmdir(d);
        return entries.length == 2;
    "#, true);
}

#[test]
fn test_fs_stat() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const fp: string = fs.tempFile("raya_test_stat_");
        fs.writeTextFile(fp, "hello");
        const s: number[] = fs.stat(fp);
        fs.remove(fp);
        return s[0] == 5 && s[1] == 1 && s[2] == 0;
    "#, true);
}

#[test]
fn test_fs_last_modified() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const fp: string = fs.tempFile("raya_test_mtime_");
        const mtime: number = fs.lastModified(fp);
        fs.remove(fp);
        return mtime > 946684800000;
    "#, true);
}

#[test]
fn test_fs_realpath() {
    expect_bool_with_builtins(r#"
        import fs from "std:fs";
        const fp: string = fs.tempFile("raya_test_rp_");
        const real: string = fs.realpath(fp);
        fs.remove(fp);
        return real.length > 0;
    "#, true);
}
