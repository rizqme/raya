//! End-to-end tests for node: stdlib shim imports.

use super::harness::*;
use raya_runtime::Runtime;

fn expect_runtime_eval_bool(source: &str, expected: bool) {
    let runtime = Runtime::new();
    let value = runtime
        .eval(source)
        .expect("runtime.eval should succeed for compatibility test");
    let actual = value.as_bool().unwrap_or(false);
    assert_eq!(actual, expected, "expected {expected}, got {:?}", value);
}

// ── node:path ──

#[test]
fn test_node_path_join() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.join("a", "b") == "a/b";
    "#,
        true,
    );
}

#[test]
fn test_node_path_parse() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        const parsed = path.parse("/home/user/file.txt");
        return parsed.base == "file.txt" && parsed.ext == ".txt" && parsed.name == "file";
    "#,
        true,
    );
}

// ── node:fs ──

#[test]
fn test_node_fs_write_and_read_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const fp: string = fs.tempFile("raya_test_node_rw_");
        fs.writeFileSync(fp, "hello node shim");
        const txt: string = fs.readFileSync(fp);
        fs.unlinkSync(fp);
        return txt == "hello node shim";
    "#,
        true,
    );
}

#[test]
fn test_node_fs_exists_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const fp: string = fs.tempFile("raya_test_exists_");
        fs.writeFileSync(fp, "test");
        const exists = fs.existsSync(fp);
        fs.unlinkSync(fp);
        return exists == true;
    "#,
        true,
    );
}

#[test]
fn test_node_fs_stat_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const fp: string = fs.tempFile("raya_test_stat_");
        fs.writeFileSync(fp, "stat test data");
        const stats = fs.statSync(fp);
        const isFile = stats.isFile();
        const isDir = stats.isDirectory();
        fs.unlinkSync(fp);
        return isFile == true && isDir == false;
    "#,
        true,
    );
}

// ── node:crypto ──

#[test]
fn test_node_crypto_create_hash_chain() {
    expect_runtime_eval_bool(
        r#"
        import crypto from "node:crypto";
        const hash = crypto.createHash("sha256").update("hello").digest();
        return hash.length > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_crypto_random_uuid() {
    expect_runtime_eval_bool(
        r#"
        import crypto from "node:crypto";
        const uuid = crypto.randomUUID();
        return uuid.length == 36;
    "#,
        true,
    );
}

// ── node:os ──

#[test]
fn test_node_os_type_and_homedir() {
    expect_runtime_eval_bool(
        r#"
        import os from "node:os";
        const t = os.type();
        const home = os.homedir();
        return t.length > 0 && home.length > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_os_platform_and_arch() {
    expect_runtime_eval_bool(
        r#"
        import os from "node:os";
        return os.platform().length > 0 && os.arch().length > 0;
    "#,
        true,
    );
}

// ── node:process ──

#[test]
fn test_node_process_cwd_and_pid() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        return process.cwd().length > 0 && process.pid() > 0;
    "#,
        true,
    );
}

// ── node:url ──

#[test]
fn test_node_url_constructor() {
    expect_runtime_eval_bool(
        r##"
        import { URL } from "node:url";
        const u = new URL("https://example.com/path?x=1#a");
        return u.hostname() == "example.com" && u.pathname() == "/path";
    "##,
        true,
    );
}

// ── node:events ──

#[test]
fn test_node_events_default_export_event_emitter() {
    expect_runtime_eval_bool(
        r#"
        import EventEmitter from "node:events";
        const emitter = new EventEmitter<{ tick: [number] }>();
        emitter.on("tick", (_: number): void => {});
        emitter.emit("tick", 1);
        return emitter.listenerCount("tick") == 1;
    "#,
        true,
    );
}

// ── node:assert ──

#[test]
fn test_node_assert_equal() {
    expect_runtime_eval_bool(
        r#"
        import assert from "node:assert";
        assert.equal(1, 1);
        assert.strictEqual("a", "a");
        return true;
    "#,
        true,
    );
}

#[test]
fn test_node_assert_deep_equal() {
    expect_runtime_eval_bool(
        r#"
        import assert from "node:assert";
        assert.deepEqual([1, 2, 3], [1, 2, 3]);
        return true;
    "#,
        true,
    );
}

#[test]
fn test_node_assert_throws() {
    expect_runtime_eval_bool(
        r#"
        import assert from "node:assert";
        assert.throws((): void => { throw new Error("boom"); });
        return true;
    "#,
        true,
    );
}

// ── node:worker_threads / node:vm / node:cluster ──

#[test]
fn test_node_worker_threads_is_main_thread() {
    expect_runtime_eval_bool(
        r#"
        import workerThreads from "node:worker_threads";
        return workerThreads.isMainThread();
    "#,
        true,
    );
}

#[test]
fn test_node_vm_run_in_new_context() {
    expect_runtime_eval_bool(
        r#"
        import vm from "node:vm";
        const result = vm.runInNewContext("1 + 2");
        return result == 3;
    "#,
        true,
    );
}

#[test]
fn test_node_cluster_is_primary() {
    expect_runtime_eval_bool(
        r#"
        import cluster from "node:cluster";
        return cluster.isPrimary();
    "#,
        true,
    );
}

// ── std:http2 / std:sqlite (unchanged) ──

#[test]
fn test_std_http2_server_lifecycle() {
    expect_bool_with_builtins(
        r#"
        import http2 from "std:http2";
        const server = http2.createServer("127.0.0.1", 0);
        const ok = server.localPort() > 0 && server.localAddr() != "";
        server.close();
        return ok;
    "#,
        true,
    );
}

#[test]
fn test_std_sqlite_basic_query() {
    expect_bool_with_builtins(
        r#"
        import sqlite from "std:sqlite";
        const db = sqlite.open(":memory:");
        db.exec("create table t(v integer);");
        db.exec("insert into t(v) values (7);");
        const rows = db.query("select v from t;");
        db.close();
        return rows[0] == "7";
    "#,
        true,
    );
}

// ── node:http2 / node:sqlite smoke tests ──

#[test]
fn test_node_http2_import_smoke() {
    expect_runtime_eval_bool(
        r#"
        import http2 from "node:http2";
        return http2 != null;
    "#,
        true,
    );
}

#[test]
fn test_node_sqlite_import_smoke() {
    expect_runtime_eval_bool(
        r#"
        import sqlite from "node:sqlite";
        return sqlite != null;
    "#,
        true,
    );
}

// ── Error handling ──

#[test]
fn test_unsupported_node_module_import_fails() {
    let runtime = Runtime::new();
    let result = runtime.eval(
        r#"
        import nope from "node:not_a_core_module";
        return 1;
    "#,
    );
    assert!(
        result.is_err(),
        "unsupported node module import should fail"
    );
    let msg = format!("{:?}", result.err());
    assert!(
        msg.contains("Unsupported node module import 'node:not_a_core_module'"),
        "expected explicit unsupported-module error, got: {msg}"
    );
}
