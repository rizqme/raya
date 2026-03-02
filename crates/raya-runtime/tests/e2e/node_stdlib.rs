//! End-to-end tests for node: stdlib shim imports.

use super::harness::*;
use raya_runtime::{BuiltinMode, Runtime, RuntimeOptions};

fn expect_runtime_eval_bool(source: &str, expected: bool) {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });
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

// ── std:http2 (unchanged) ──

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

// ── node:http2 smoke tests ──

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

// ── node:path (expanded) ──

#[test]
fn test_node_path_variadic_join() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.join("a", "b", "c") == "a/b/c";
    "#,
        true,
    );
}

#[test]
fn test_node_path_dirname() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.dirname("/home/user/file.txt") == "/home/user";
    "#,
        true,
    );
}

#[test]
fn test_node_path_basename() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.basename("/home/user/file.txt") == "file.txt";
    "#,
        true,
    );
}

#[test]
fn test_node_path_basename_strip_ext() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.basename("/home/user/file.txt", ".txt") == "file";
    "#,
        true,
    );
}

#[test]
fn test_node_path_extname() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.extname("index.html") == ".html";
    "#,
        true,
    );
}

#[test]
fn test_node_path_is_absolute() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.isAbsolute("/foo/bar") == true && path.isAbsolute("foo/bar") == false;
    "#,
        true,
    );
}

#[test]
fn test_node_path_normalize() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        const n = path.normalize("/foo/bar//baz/asdf/quux/..");
        return n == "/foo/bar/baz/asdf";
    "#,
        true,
    );
}

#[test]
fn test_node_path_sep_and_delimiter() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.sep == "/" && path.delimiter == ":";
    "#,
        true,
    );
}

#[test]
fn test_node_path_resolve_absolute() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        const resolved = path.resolve("/foo", "bar");
        return path.isAbsolute(resolved);
    "#,
        true,
    );
}

#[test]
fn test_node_path_format_roundtrip() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        const parsed = path.parse("/home/user/file.txt");
        const formatted = path.format(parsed);
        return formatted == "/home/user/file.txt";
    "#,
        true,
    );
}

#[test]
fn test_node_path_to_namespaced_path() {
    expect_runtime_eval_bool(
        r#"
        import path from "node:path";
        return path.toNamespacedPath("/foo/bar") == "/foo/bar";
    "#,
        true,
    );
}

// ── node:fs (expanded) ──

#[test]
fn test_node_fs_append_file_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const fp: string = fs.tempFile("raya_test_append_");
        fs.writeFileSync(fp, "hello");
        fs.appendFileSync(fp, " world");
        const txt: string = fs.readFileSync(fp);
        fs.unlinkSync(fp);
        return txt == "hello world";
    "#,
        true,
    );
}

#[test]
fn test_node_fs_mkdir_and_rmdir_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const base: string = fs.tempDir();
        const dir: string = base + "/raya_test_mkdir";
        fs.mkdirSync(dir);
        const exists: boolean = fs.existsSync(dir);
        fs.rmdirSync(dir);
        const gone: boolean = fs.existsSync(dir);
        return exists == true && gone == false;
    "#,
        true,
    );
}

#[test]
fn test_node_fs_copy_file_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const src: string = fs.tempFile("raya_test_copy_src_");
        const dst: string = fs.tempFile("raya_test_copy_dst_");
        fs.writeFileSync(src, "copy me");
        fs.copyFileSync(src, dst);
        const txt: string = fs.readFileSync(dst);
        fs.unlinkSync(src);
        fs.unlinkSync(dst);
        return txt == "copy me";
    "#,
        true,
    );
}

#[test]
fn test_node_fs_rename_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const src: string = fs.tempFile("raya_test_rename_src_");
        const dst: string = src + "_renamed";
        fs.writeFileSync(src, "rename me");
        fs.renameSync(src, dst);
        const exists_src: boolean = fs.existsSync(src);
        const txt: string = fs.readFileSync(dst);
        fs.unlinkSync(dst);
        return exists_src == false && txt == "rename me";
    "#,
        true,
    );
}

#[test]
fn test_node_fs_stat_size() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const fp: string = fs.tempFile("raya_test_stat_size_");
        fs.writeFileSync(fp, "12345");
        const stats = fs.statSync(fp);
        const sz: number = stats.size();
        fs.unlinkSync(fp);
        return sz == 5;
    "#,
        true,
    );
}

#[test]
fn test_node_fs_stat_mtime() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const fp: string = fs.tempFile("raya_test_stat_mtime_");
        fs.writeFileSync(fp, "test");
        const stats = fs.statSync(fp);
        const mtime: number = stats.mtimeMs();
        fs.unlinkSync(fp);
        return mtime > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_fs_truncate_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const fp: string = fs.tempFile("raya_test_trunc_");
        fs.writeFileSync(fp, "hello world");
        fs.truncateSync(fp, 5);
        const txt: string = fs.readFileSync(fp);
        fs.unlinkSync(fp);
        return txt == "hello";
    "#,
        true,
    );
}

#[test]
fn test_node_fs_access_sync_throws_on_missing() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        let threw = false;
        try {
            fs.accessSync("/nonexistent/path/definitely/missing");
        } catch (e) {
            threw = true;
        }
        return threw;
    "#,
        true,
    );
}

#[test]
fn test_node_fs_lstat_sync() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        const fp: string = fs.tempFile("raya_test_lstat_");
        fs.writeFileSync(fp, "lstat test");
        const stats = fs.lstatSync(fp);
        const ok = stats.isFile() && !stats.isDirectory();
        fs.unlinkSync(fp);
        return ok;
    "#,
        true,
    );
}

// ── node:crypto (expanded) ──

#[test]
fn test_node_crypto_sha256_deterministic() {
    expect_runtime_eval_bool(
        r#"
        import crypto from "node:crypto";
        const h1 = crypto.createHash("sha256").update("test").digest();
        const h2 = crypto.createHash("sha256").update("test").digest();
        return h1 == h2 && h1.length == 64;
    "#,
        true,
    );
}

#[test]
fn test_node_crypto_create_hmac_chain() {
    expect_runtime_eval_bool(
        r#"
        import crypto from "node:crypto";
        const h = crypto.createHmac("sha256", "secret").update("message").digest();
        return h.length > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_crypto_random_bytes() {
    expect_runtime_eval_bool(
        r#"
        import crypto from "node:crypto";
        const buf: Buffer = crypto.randomBytes(16);
        return buf.length == 16;
    "#,
        true,
    );
}

#[test]
fn test_node_crypto_random_int() {
    expect_runtime_eval_bool(
        r#"
        import crypto from "node:crypto";
        const n: number = crypto.randomInt(10, 100);
        return n >= 10 && n < 100;
    "#,
        true,
    );
}

#[test]
fn test_node_crypto_get_hashes() {
    expect_runtime_eval_bool(
        r#"
        import crypto from "node:crypto";
        const hashes: string[] = crypto.getHashes();
        return hashes.length > 0 && hashes.includes("sha256");
    "#,
        true,
    );
}

#[test]
fn test_node_crypto_digest_base64() {
    expect_runtime_eval_bool(
        r#"
        import crypto from "node:crypto";
        const hex = crypto.createHash("sha256").update("hello").digest("hex");
        const b64 = crypto.createHash("sha256").update("hello").digest("base64");
        return hex.length == 64 && b64.length > 0 && b64 != hex;
    "#,
        true,
    );
}

// ── node:process (expanded) ──

#[test]
fn test_node_process_env_set_get() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        process.env.set("RAYA_TEST_VAR_123", "hello");
        const val: string = process.env.get("RAYA_TEST_VAR_123");
        process.env.delete("RAYA_TEST_VAR_123");
        return val == "hello";
    "#,
        true,
    );
}

#[test]
fn test_node_process_env_has() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        process.env.set("RAYA_TEST_HAS_123", "1");
        const has: boolean = process.env.has("RAYA_TEST_HAS_123");
        process.env.delete("RAYA_TEST_HAS_123");
        const hasAfter: boolean = process.env.has("RAYA_TEST_HAS_123");
        return has == true && hasAfter == false;
    "#,
        true,
    );
}

#[test]
fn test_node_process_hrtime_format() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        const t: number[] = process.hrtime();
        return t.length == 2 && t[0] >= 0 && t[1] >= 0;
    "#,
        true,
    );
}

#[test]
fn test_node_process_memory_usage() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        const mem = process.memoryUsage();
        return mem.heapUsed >= 0 && mem.heapTotal >= 0;
    "#,
        true,
    );
}

#[test]
fn test_node_process_platform_arch() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        return process.platform().length > 0 && process.arch().length > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_process_version() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        return process.version().length > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_process_uptime() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        return process.uptime() >= 0;
    "#,
        true,
    );
}

#[test]
fn test_node_process_exit_code() {
    expect_runtime_eval_bool(
        r#"
        import process from "node:process";
        const initial: number = process.exitCode;
        process.exitCode = 42;
        const updated: number = process.exitCode;
        process.exitCode = 0;
        return initial == 0 && updated == 42;
    "#,
        true,
    );
}

// ── node:url (expanded) ──

#[test]
fn test_node_url_protocol_and_host() {
    expect_runtime_eval_bool(
        r##"
        import { URL } from "node:url";
        const u = new URL("https://example.com:8080/path");
        return u.protocol() == "https:" && u.host() == "example.com:8080";
    "##,
        true,
    );
}

#[test]
fn test_node_url_search_params() {
    expect_runtime_eval_bool(
        r##"
        import { URL } from "node:url";
        const u = new URL("https://example.com/path?foo=bar&x=1");
        return u.searchParams.get("foo") == "bar" && u.searchParams.get("x") == "1";
    "##,
        true,
    );
}

#[test]
fn test_node_url_search_params_operations() {
    expect_runtime_eval_bool(
        r##"
        import { URLSearchParams } from "node:url";
        const p = new URLSearchParams("a=1&b=2");
        p.set("c", "3");
        p.delete("a");
        return p.has("b") && p.has("c") && !p.has("a");
    "##,
        true,
    );
}

#[test]
fn test_node_url_to_json() {
    expect_runtime_eval_bool(
        r##"
        import { URL } from "node:url";
        const u = new URL("https://example.com/path");
        const json = u.toJSON();
        return json == u.toString();
    "##,
        true,
    );
}

#[test]
fn test_node_url_origin() {
    expect_runtime_eval_bool(
        r##"
        import { URL } from "node:url";
        const u = new URL("https://example.com/path");
        return u.origin() == "https://example.com";
    "##,
        true,
    );
}

// ── node:events (expanded) ──

#[test]
fn test_node_events_named_import() {
    expect_runtime_eval_bool(
        r#"
        import { EventEmitter } from "node:events";
        const emitter = new EventEmitter<{ test: [string] }>();
        let received = "";
        emitter.on("test", (val: string): void => { received = val; });
        emitter.emit("test", "hello");
        return received == "hello";
    "#,
        true,
    );
}

#[test]
fn test_node_events_remove_listener() {
    expect_runtime_eval_bool(
        r#"
        import EventEmitter from "node:events";
        const emitter = new EventEmitter<{ data: [number] }>();
        let count = 0;
        const handler = (_n: number): void => { count = count + 1; };
        emitter.on("data", handler);
        emitter.emit("data", 1);
        emitter.off("data", handler);
        emitter.emit("data", 2);
        return count == 1;
    "#,
        true,
    );
}

#[test]
fn test_node_events_once() {
    expect_runtime_eval_bool(
        r#"
        import EventEmitter from "node:events";
        const emitter = new EventEmitter<{ ping: [number] }>();
        let count = 0;
        emitter.once("ping", (_n: number): void => { count = count + 1; });
        emitter.emit("ping", 1);
        emitter.emit("ping", 2);
        return count == 1;
    "#,
        true,
    );
}

#[test]
fn test_node_events_event_names() {
    expect_runtime_eval_bool(
        r#"
        import EventEmitter from "node:events";
        const emitter = new EventEmitter<{ a: [number], b: [number] }>();
        emitter.on("a", (_n: number): void => {});
        emitter.on("b", (_n: number): void => {});
        const names: string[] = emitter.eventNames();
        return names.length == 2;
    "#,
        true,
    );
}

// ── node:util ──

#[test]
fn test_node_util_format() {
    expect_runtime_eval_bool(
        r#"
        import util from "node:util";
        const result: string = util.format("hello %s, you are %d", "world", "42");
        return result == "hello world, you are 42";
    "#,
        true,
    );
}

#[test]
fn test_node_util_format_percent_escape() {
    expect_runtime_eval_bool(
        r#"
        import util from "node:util";
        const result: string = util.format("100%%");
        return result == "100%";
    "#,
        true,
    );
}

#[test]
fn test_node_util_inspect() {
    expect_runtime_eval_bool(
        r#"
        import util from "node:util";
        const result: string = util.inspect([1, 2, 3]);
        return result.length > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_util_is_deep_strict_equal() {
    expect_runtime_eval_bool(
        r#"
        import util from "node:util";
        return util.isDeepStrictEqual([1, 2, 3], [1, 2, 3]) == true
            && util.isDeepStrictEqual([1, 2], [1, 3]) == false;
    "#,
        true,
    );
}

#[test]
fn test_node_util_types_is_native_error() {
    expect_runtime_eval_bool(
        r#"
        import util from "node:util";
        return util.types.isNativeError(new Error("test")) == true;
    "#,
        true,
    );
}

// ── node:child_process ──

#[test]
fn test_node_child_process_exec_sync() {
    expect_runtime_eval_bool(
        r#"
        import childProcess from "node:child_process";
        const output: string = childProcess.execSync("echo hello");
        return output.includes("hello");
    "#,
        true,
    );
}

#[test]
fn test_node_child_process_spawn_sync() {
    expect_runtime_eval_bool(
        r#"
        import childProcess from "node:child_process";
        const result = childProcess.spawnSync("/bin/echo");
        return result.status == 0;
    "#,
        true,
    );
}

#[test]
fn test_node_child_process_exec_sync_throws_on_failure() {
    expect_runtime_eval_bool(
        r#"
        import childProcess from "node:child_process";
        let threw = false;
        try {
            childProcess.execSync("false");
        } catch (e) {
            threw = true;
        }
        return threw;
    "#,
        true,
    );
}

// ── node:timers ──

#[test]
fn test_node_timers_set_timeout_returns_id() {
    expect_runtime_eval_bool(
        r#"
        import timers from "node:timers";
        const id: number = timers.setTimeout((): void => {}, 1000);
        timers.clearTimeout(id);
        return id > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_timers_set_immediate() {
    expect_runtime_eval_bool(
        r#"
        import timers from "node:timers";
        const id: number = timers.setImmediate((): void => {});
        return id > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_timers_set_interval_and_clear() {
    expect_runtime_eval_bool(
        r#"
        import timers from "node:timers";
        const id: number = timers.setInterval((): void => {}, 5000);
        timers.clearInterval(id);
        return id > 0;
    "#,
        true,
    );
}

// ── node:string_decoder ──

#[test]
fn test_node_string_decoder_constructor() {
    expect_runtime_eval_bool(
        r#"
        import { StringDecoder } from "node:string_decoder";
        const decoder = new StringDecoder();
        return decoder.encoding == "utf8";
    "#,
        true,
    );
}

#[test]
fn test_node_string_decoder_encoding() {
    expect_runtime_eval_bool(
        r#"
        import { StringDecoder } from "node:string_decoder";
        const decoder = new StringDecoder("ascii");
        return decoder.encoding == "ascii";
    "#,
        true,
    );
}

// ── node:diagnostics_channel ──

#[test]
fn test_node_diagnostics_channel_pub_sub() {
    expect_runtime_eval_bool(
        r#"
        import diagnosticsChannel from "node:diagnostics_channel";
        const ch = diagnosticsChannel.channel("test.ch");
        let received = false;
        ch.subscribe((msg: unknown): void => { received = true; });
        ch.publish("hello");
        return received == true && ch.hasSubscribers() == true;
    "#,
        true,
    );
}

#[test]
fn test_node_diagnostics_channel_unsubscribe() {
    expect_runtime_eval_bool(
        r#"
        import diagnosticsChannel from "node:diagnostics_channel";
        const ch = diagnosticsChannel.channel("test.unsub");
        let count = 0;
        const handler = (_msg: unknown): void => { count = count + 1; };
        ch.subscribe(handler);
        ch.publish("a");
        ch.unsubscribe(handler);
        ch.publish("b");
        return count == 1;
    "#,
        true,
    );
}

// ── node:perf_hooks ──

#[test]
fn test_node_perf_hooks_now() {
    expect_runtime_eval_bool(
        r#"
        import perfHooks from "node:perf_hooks";
        const t: number = perfHooks.performance.now();
        return t >= 0;
    "#,
        true,
    );
}

#[test]
fn test_node_perf_hooks_mark_and_measure() {
    expect_runtime_eval_bool(
        r#"
        import perfHooks from "node:perf_hooks";
        const perf = perfHooks.performance;
        perf.mark("start");
        perf.mark("end");
        const entry = perf.measure("test", "start", "end");
        return entry.name == "test" && entry.entryType == "measure" && entry.duration >= 0;
    "#,
        true,
    );
}

#[test]
fn test_node_perf_hooks_clear() {
    expect_runtime_eval_bool(
        r#"
        import perfHooks from "node:perf_hooks";
        const perf = perfHooks.performance;
        perf.mark("x");
        perf.clearMarks();
        perf.clearMeasures();
        return perf.now() >= 0;
    "#,
        true,
    );
}

// ── node:v8 ──

#[test]
fn test_node_v8_heap_statistics() {
    expect_runtime_eval_bool(
        r#"
        import v8 from "node:v8";
        const stats = v8.getHeapStatistics();
        return stats.usedHeapSize >= 0 && stats.totalHeapSize >= 0 && stats.heapSizeLimit >= 0;
    "#,
        true,
    );
}

#[test]
fn test_node_v8_heap_space_statistics() {
    expect_runtime_eval_bool(
        r#"
        import v8 from "node:v8";
        const spaces = v8.getHeapSpaceStatistics();
        return spaces.length > 0;
    "#,
        true,
    );
}

// ── node:vm (expanded) ──

#[test]
fn test_node_vm_script_class() {
    expect_runtime_eval_bool(
        r#"
        import vm from "node:vm";
        const script = vm.createScript("2 + 2");
        const result: number = script.runInThisContext();
        return result == 4;
    "#,
        true,
    );
}

#[test]
fn test_node_vm_is_context() {
    expect_runtime_eval_bool(
        r#"
        import vm from "node:vm";
        return vm.isContext({}) == false;
    "#,
        true,
    );
}

#[test]
fn test_node_vm_run_in_this_context() {
    expect_runtime_eval_bool(
        r#"
        import vm from "node:vm";
        return vm.runInThisContext("3 * 4") == 12;
    "#,
        true,
    );
}

// ── node:test ──

#[test]
fn test_node_test_runner_describe_it() {
    expect_runtime_eval_bool(
        r#"
        import testRunner from "node:test";
        let ran = false;
        testRunner.describe("suite", (): void => {
            testRunner.it("case", (): void => {
                ran = true;
            });
        });
        return ran == true && testRunner.passed() > 0;
    "#,
        true,
    );
}

#[test]
fn test_node_test_runner_before_after_each() {
    expect_runtime_eval_bool(
        r#"
        import { describe, it, beforeEach, afterEach } from "node:test";
        let count = 0;
        describe("hooks", (): void => {
            beforeEach((): void => { count = count + 1; });
            it("test1", (): void => {});
            it("test2", (): void => {});
        });
        return count == 2;
    "#,
        true,
    );
}

// ── node:fs/promises ──

#[test]
fn test_node_fs_promises_write_and_read() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        import fsp from "node:fs/promises";
        const fp: string = fs.tempFile("raya_test_fsp_");
        await fsp.writeFile(fp, "async content");
        const txt: string = await fsp.readFile(fp);
        await fsp.unlink(fp);
        return txt == "async content";
    "#,
        true,
    );
}

#[test]
fn test_node_fs_promises_stat() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        import fsp from "node:fs/promises";
        const fp: string = fs.tempFile("raya_test_fsp_stat_");
        fs.writeFileSync(fp, "stat data");
        const stats = await fsp.stat(fp);
        const ok = stats.isFile() && stats.size() == 9;
        fs.unlinkSync(fp);
        return ok;
    "#,
        true,
    );
}

#[test]
fn test_node_fs_promises_copy_file() {
    expect_runtime_eval_bool(
        r#"
        import fs from "node:fs";
        import fsp from "node:fs/promises";
        const src: string = fs.tempFile("raya_test_fsp_cp_src_");
        const dst: string = fs.tempFile("raya_test_fsp_cp_dst_");
        fs.writeFileSync(src, "copy async");
        await fsp.copyFile(src, dst);
        const txt: string = await fsp.readFile(dst);
        fs.unlinkSync(src);
        fs.unlinkSync(dst);
        return txt == "copy async";
    "#,
        true,
    );
}

// ── node:net ──

#[test]
fn test_node_net_import_smoke() {
    expect_runtime_eval_bool(
        r#"
        import net from "node:net";
        return net != null;
    "#,
        true,
    );
}

// ── node:http ──
// Note: node:http imports std:http2/std:net internally. Tested via std:http2 test above.

// ── node:dns ──

#[test]
fn test_node_dns_import_smoke() {
    expect_runtime_eval_bool(
        r#"
        import dns from "node:dns";
        return dns != null;
    "#,
        true,
    );
}

// ── node:assert (expanded) ──

#[test]
fn test_node_assert_ok() {
    expect_runtime_eval_bool(
        r#"
        import assert from "node:assert";
        assert.ok(true);
        let threw = false;
        try {
            assert.ok(false);
        } catch (e) {
            threw = true;
        }
        return threw;
    "#,
        true,
    );
}

#[test]
fn test_node_assert_not_equal() {
    expect_runtime_eval_bool(
        r#"
        import assert from "node:assert";
        assert.notEqual(1, 2);
        return true;
    "#,
        true,
    );
}

#[test]
fn test_node_assert_does_not_throw() {
    expect_runtime_eval_bool(
        r#"
        import assert from "node:assert";
        assert.doesNotThrow((): void => {
            const x = 1 + 1;
        });
        return true;
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
