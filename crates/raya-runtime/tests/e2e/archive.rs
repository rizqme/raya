//! End-to-end tests for std:archive module

use super::harness::*;

#[test]
fn test_archive_tar_create_list_extract() {
    expect_bool_with_builtins(
        r#"
        import fs from "std:fs";
        import archive from "std:archive";

        const base = fs.tempDir() + "/raya_archive_tar_1";
        fs.mkdir(base);
        const src = base + "/src";
        const out = base + "/out";
        fs.mkdir(src);
        fs.mkdir(out);

        fs.writeTextFile(src + "/a.txt", "hello tar");
        const tarPath = base + "/data.tar";
        archive.tarCreate(tarPath, [src + "/a.txt"]);
        const entries = archive.tarList(tarPath);
        archive.tarExtract(tarPath, out);
        const txt = fs.readTextFile(out + "/a.txt");

        fs.remove(src + "/a.txt");
        fs.rmdir(src);
        fs.remove(out + "/a.txt");
        fs.rmdir(out);
        fs.remove(tarPath);
        fs.rmdir(base);

        return entries.length == 1 && txt == "hello tar";
    "#,
        true,
    );
}

#[test]
fn test_archive_zip_create_list_extract() {
    expect_bool_with_builtins(
        r#"
        import fs from "std:fs";
        import archive from "std:archive";

        const base = fs.tempDir() + "/raya_archive_zip_1";
        fs.mkdir(base);
        const src = base + "/src";
        const out = base + "/out";
        fs.mkdir(src);
        fs.mkdir(out);

        fs.writeTextFile(src + "/b.txt", "hello zip");
        const zipPath = base + "/data.zip";
        archive.zipCreate(zipPath, [src + "/b.txt"]);
        const entries = archive.zipList(zipPath);
        archive.zipExtract(zipPath, out);
        const txt = fs.readTextFile(out + "/b.txt");

        fs.remove(src + "/b.txt");
        fs.rmdir(src);
        fs.remove(out + "/b.txt");
        fs.rmdir(out);
        fs.remove(zipPath);
        fs.rmdir(base);

        return entries.length == 1 && txt == "hello zip";
    "#,
        true,
    );
}
