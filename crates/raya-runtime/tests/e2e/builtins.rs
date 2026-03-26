//! End-to-end tests for builtins usage in Raya scripts
//!
//! These tests compile and run Raya code that uses builtin types
//! like Map, Set, Buffer, Date, Channel.
//!
//! These tests use `expect_*_with_builtins` functions which compile
//! the builtin .raya source files together with the test code.
//!
//! NOTE: These tests will fail at runtime until native function implementations
//! are added to the VM. The tests verify compilation succeeds.

use super::harness::{
    expect_bool, expect_bool_runtime, expect_bool_runtime_node_compat, expect_bool_with_builtins,
    expect_i32, expect_i32_runtime, expect_i32_runtime_node_compat, expect_i32_with_builtins,
    expect_string, expect_string_runtime, expect_string_runtime_node_compat,
    expect_string_with_builtins,
};

// ============================================================================
// Map tests
// ============================================================================

#[test]
fn test_map_new_and_size() {
    expect_i32_with_builtins(
        r#"
        let map = new Map<string, number>();
        return map.size;
    "#,
        0,
    );
}

// Minimal test: just get and return directly
#[test]
fn test_map_get_simple() {
    expect_i32_with_builtins(
        r#"
        let map = new Map<string, number>();
        map.set("a", 10);
        let a = map.get("a");
        return 0;
    "#,
        0,
    );
}

// Test null comparison
#[test]
fn test_map_get_null_check() {
    expect_bool_with_builtins(
        r#"
        let map = new Map<string, number>();
        map.set("a", 10);
        let a = map.get("a");
        return a != null;
    "#,
        true,
    );
}

#[test]
fn test_map_set_and_get() {
    expect_i32_with_builtins(
        r#"
        let map = new Map<string, number>();
        map.set("a", 10);
        map.set("b", 20);
        let a = map.get("a");
        if (a != null) {
            return a;
        }
        return 0;
    "#,
        10,
    );
}

#[test]
fn test_map_has() {
    expect_bool_with_builtins(
        r#"
        let map = new Map<string, number>();
        map.set("key", 42);
        return map.has("key");
    "#,
        true,
    );
}

#[test]
fn test_map_has_missing() {
    expect_bool_with_builtins(
        r#"
        let map = new Map<string, number>();
        map.set("key", 42);
        return map.has("other");
    "#,
        false,
    );
}

#[test]
fn test_map_delete() {
    expect_bool_with_builtins(
        r#"
        let map = new Map<string, number>();
        map.set("key", 42);
        let deleted = map.delete("key");
        return deleted && !map.has("key");
    "#,
        true,
    );
}

#[test]
fn test_map_clear() {
    expect_i32_with_builtins(
        r#"
        let map = new Map<string, number>();
        map.set("a", 1);
        map.set("b", 2);
        map.clear();
        return map.size;
    "#,
        0,
    );
}

// ============================================================================
// Set tests
// ============================================================================

#[test]
fn test_set_new_and_size() {
    expect_i32_with_builtins(
        r#"
        let set = new Set<number>();
        return set.size;
    "#,
        0,
    );
}

#[test]
fn test_set_add_and_has() {
    expect_bool_with_builtins(
        r#"
        let set = new Set<number>();
        set.add(42);
        return set.has(42);
    "#,
        true,
    );
}

#[test]
fn test_set_add_unique() {
    expect_i32_with_builtins(
        r#"
        let set = new Set<number>();
        set.add(1);
        set.add(2);
        set.add(1);
        return set.size;
    "#,
        2,
    );
}

#[test]
fn test_set_delete() {
    expect_bool_with_builtins(
        r#"
        let set = new Set<number>();
        set.add(42);
        let deleted = set.delete(42);
        return deleted && !set.has(42);
    "#,
        true,
    );
}

#[test]
fn test_set_clear() {
    expect_i32_with_builtins(
        r#"
        let set = new Set<number>();
        set.add(1);
        set.add(2);
        set.add(3);
        set.clear();
        return set.size;
    "#,
        0,
    );
}

#[test]
fn test_set_keys_alias_values() {
    expect_bool_with_builtins(
        r#"
        let set = new Set<number>();
        set.add(7);
        set.add(9);
        let keys = set.keys();
        let values = set.values();
        return keys.length == values.length && keys.includes(7) && keys.includes(9);
    "#,
        true,
    );
}

#[test]
fn test_set_entries_value_value_pairs() {
    expect_bool_with_builtins(
        r#"
        let set = new Set<number>();
        set.add(5);
        set.add(8);
        let entries = set.entries();
        let ok = true;
        let i = 0;
        while (i < entries.length) {
            if (entries[i][0] != entries[i][1]) {
                ok = false;
            }
            i = i + 1;
        }
        return ok && entries.length == 2;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_class_getter_after_generator_method_returns_plain_value() {
    expect_i32_runtime_node_compat(
        r#"
        let C = class {
            *m(a,) { return arguments.length; }
            get g() { return 123; }
        };
        return new C().g;
    "#,
        123,
    );
}

#[test]
fn test_node_compat_class_private_generator_extraction_remains_callable() {
    expect_i32_runtime_node_compat(
        r#"
        let C = class {
            *#method(a,) { return arguments.length; }
            get method() { return this.#method; }
        };
        let f = new C().method;
        return f(42).next().value;
    "#,
        1,
    );
}

#[test]
fn test_node_compat_live_generator_next_sequence_returns_each_step() {
    expect_string_runtime_node_compat(
        r#"
        function* gen() {
            yield 1;
            yield 2;
            return 3;
        }
        let it = gen();
        let a = it.next();
        let b = it.next();
        let c = it.next();
        return JSON.stringify([a.value, a.done, b.value, b.done, c.value, c.done]);
    "#,
        r#"[1,false,2,false,3,true]"#,
    );
}

#[test]
fn test_node_compat_generator_instance_uses_function_prototype() {
    expect_bool_runtime_node_compat(
        r#"
        function* gen() {}
        let proto = {};
        gen.prototype = proto;
        return Object.getPrototypeOf(gen()) === proto;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_generator_instance_falls_back_to_generator_prototype() {
    expect_bool_runtime_node_compat(
        r#"
        function* gen() {}
        let defaultProto = Object.getPrototypeOf(gen).prototype;
        gen.prototype = null;
        return Object.getPrototypeOf(gen()) === defaultProto;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_async_generator_instance_falls_back_to_async_generator_prototype() {
    expect_bool_runtime_node_compat(
        r#"
        async function* gen() {}
        let defaultProto = Object.getPrototypeOf(gen.prototype);
        gen.prototype = null;
        return Object.getPrototypeOf(gen()) === defaultProto;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_static_computed_method_is_visible_on_class_constructor() {
    expect_string_runtime_node_compat(
        r#"
        class C { static ["b"](){ return 2; } }
        return JSON.stringify([typeof C.b, C.b(), Object.getOwnPropertyNames(C).includes("b")]);
    "#,
        r#"["function",2,true]"#,
    );
}

#[test]
fn test_node_compat_static_computed_getter_reads_through_class_constructor() {
    expect_string_runtime_node_compat(
        r#"
        class C { static x = 3; static get ["g"](){ return this.x; } }
        return JSON.stringify([C.g, Object.getOwnPropertyNames(C).includes("g")]);
    "#,
        r#"[3,true]"#,
    );
}

#[test]
fn test_node_compat_static_getter_can_read_static_field_via_this() {
    expect_i32_runtime_node_compat(
        r#"
        class C {
            static x = 3;
            static get g() { return this.x; }
        }
        return C.g;
    "#,
        3,
    );
}

#[test]
fn test_node_compat_static_getter_can_extract_private_static_method_via_this() {
    expect_i32_runtime_node_compat(
        r#"
        class C {
            static #m(a,) { return arguments.length; }
            static get g() { return this.#m; }
        }
        return C.g(42);
    "#,
        1,
    );
}

// ============================================================================
// Promise tests
// ============================================================================

#[test]
fn test_promise_resolve_static() {
    expect_i32_with_builtins(
        r#"
        return await Promise.resolve(42);
    "#,
        42,
    );
}

#[test]
fn test_promise_all_static() {
    expect_i32_with_builtins(
        r#"
        let out = await Promise.all([Promise.resolve(40), Promise.resolve(2)]);
        return out[0] + out[1];
    "#,
        42,
    );
}

#[test]
fn test_promise_race_static() {
    expect_i32_with_builtins(
        r#"
        return await Promise.race([Promise.resolve(41), Promise.resolve(42)]);
    "#,
        41,
    );
}

#[test]
fn test_promise_then_instance() {
    expect_i32_with_builtins(
        r#"
        let p = Promise.resolve(21);
        let q = p.then((n: number): number => n * 2);
        return await q;
    "#,
        42,
    );
}

#[test]
fn test_promise_then_rejection_passthrough_to_catch() {
    expect_i32_with_builtins(
        r#"
        let p = Promise
            .reject<number>("boom")
            .then((reason: PromiseRejectionReason): unknown => reason)
            .catch<number>((reason: Object | string | number | boolean | null): number => 42);
        return await p;
    "#,
        42,
    );
}

#[test]
fn test_promise_catch_instance_recovers_rejection() {
    expect_i32_with_builtins(
        r#"
        let p = Promise.reject<number>("boom");
        let q = p.catch((reason: Object | string | number | boolean | null): number => 42);
        return await q;
    "#,
        42,
    );
}

#[test]
fn test_promise_catch_instance_passthrough_on_success() {
    expect_i32_with_builtins(
        r#"
        let p = Promise.resolve(41);
        let q = p.catch((reason: Object | string | number | boolean | null): number => 0);
        return await q;
    "#,
        41,
    );
}

#[test]
fn test_promise_finally_instance() {
    expect_i32_with_builtins(
        r#"
        let marker = 1;
        let p = Promise.resolve(41);
        let q = p.finally((): void => { marker = marker + 1; });
        let out = await q;
        return out + marker;
    "#,
        43,
    );
}

#[test]
fn test_promise_finally_runs_on_rejection_and_passthrough() {
    expect_i32_with_builtins(
        r#"
        let marker = 1;
        let p = Promise
            .reject<number>("boom")
            .finally((): void => { marker = marker + 1; })
            .catch((_: Object | string | number | boolean | null): number => 40);
        let out = await p;
        return out + marker;
    "#,
        42,
    );
}

#[test]
fn test_promise_then_fifo_order() {
    expect_i32_with_builtins(
        r#"
        let order: number[] = [];
        Promise.resolve(1).then((_: number): number => {
            order.push(1);
            return 1;
        });
        Promise.resolve(2).then((_: number): number => {
            order.push(2);
            return 2;
        });
        await Promise.resolve(0);
        return order[0] * 10 + order[1];
    "#,
        12,
    );
}

#[test]
fn test_promise_catch_rethrow_stays_rejected() {
    expect_i32_with_builtins(
        r#"
        async function run(): Promise<number> {
            let p = Promise
                .reject<number>("boom")
                .catch((_: Object | string | number | boolean | null): number => {
                    throw "again";
                });
            try {
                let _v = await p;
                return 0;
            } catch (e) {
                return 1;
            }
        }
        return await run();
    "#,
        1,
    );
}

// ============================================================================
// Symbol tests
// ============================================================================

#[test]
fn test_symbol_for_and_key_for() {
    expect_string_with_builtins(
        r#"
        let s = Symbol.for("alpha");
        return Symbol.keyFor(s);
    "#,
        "alpha",
    );
}

#[test]
fn test_symbol_to_string_surface() {
    expect_string_with_builtins(
        r#"
        let s = Symbol.for("iter");
        return s.toString();
    "#,
        "Symbol(iter)",
    );
}

#[test]
fn test_symbol_iterator_key() {
    expect_string_runtime(
        r#"
        function main(): string {
            let it = Symbol.iterator();
            return it.valueOf();
        }
    "#,
        "Symbol.iterator",
    );
}

// ============================================================================
// Buffer tests
// ============================================================================

#[test]
fn test_buffer_new_and_length() {
    expect_i32_with_builtins(
        r#"
        let buf = new Buffer(16);
        return buf.length;
    "#,
        16,
    );
}

#[test]
fn test_buffer_get_set_byte() {
    expect_i32_with_builtins(
        r#"
        let buf = new Buffer(4);
        buf.setByte(0, 42);
        buf.setByte(1, 100);
        return buf.getByte(0) + buf.getByte(1);
    "#,
        142,
    );
}

#[test]
fn test_buffer_get_set_int32() {
    expect_i32_with_builtins(
        r#"
        let buf = new Buffer(8);
        buf.setInt32(0, 12345);
        return buf.getInt32(0);
    "#,
        12345,
    );
}

#[test]
fn test_buffer_get_set_float64() {
    expect_bool_with_builtins(
        r#"
        let buf = new Buffer(16);
        buf.setFloat64(0, 3.14159);
        let val = buf.getFloat64(0);
        return val > 3.14 && val < 3.15;
    "#,
        true,
    );
}

// ============================================================================
// TypedArray / DataView tests
// ============================================================================

#[test]
fn test_arraybuffer_slice_length() {
    expect_i32_runtime_node_compat(
        r#"
        let ab = new ArrayBuffer(16);
        let sub = ab.slice(4, 10);
        return sub.byteLength;
    "#,
        6,
    );
}

#[test]
fn test_uint8array_get_set() {
    expect_i32_runtime_node_compat(
        r#"
        let arr = new Uint8Array(4);
        arr.set(0, 7);
        arr.set(1, 8);
        return arr.get(0) + arr.get(1);
    "#,
        15,
    );
}

#[test]
fn test_int8array_signed_roundtrip() {
    expect_i32_runtime_node_compat(
        r#"
        let arr = new Int8Array(2);
        arr.set(0, -1);
        arr.set(1, -128);
        return arr.get(0) + arr.get(1);
    "#,
        -129,
    );
}

#[test]
fn test_int32array_get_set() {
    expect_i32_runtime_node_compat(
        r#"
        let backing = new ArrayBuffer(8);
        let arr = new Int32Array(backing);
        arr.set(0, 123456);
        return arr.get(0);
    "#,
        123456,
    );
}

#[test]
fn test_extended_integer_typed_arrays_roundtrip() {
    expect_i32_runtime_node_compat(
        r#"
        let u16 = new Uint16Array(2);
        try {
            u16.set(0, 65535);
        } catch (e) {
            return 101;
        }
        let i16 = new Int16Array(2);
        try {
            i16.set(0, -2);
        } catch (e) {
            return 102;
        }
        let c = new Uint8ClampedArray(1);
        try {
            c.set(0, 999);
        } catch (e) {
            return 104;
        }
        return u16.get(0) + i16.get(0) + c.get(0);
    "#,
        65788,
    );
}

#[test]
fn test_extended_float_and_bigint_typed_arrays_pragmatic_subset() {
    expect_bool_runtime_node_compat(
        r#"
        let f32 = new Float32Array(1);
        try { f32.set(0, 3.25); } catch (e) { return false; }
        let f16 = new Float16Array(1);
        try { f16.set(0, 2.5); } catch (e) { return false; }
        let i64 = new BigInt64Array(1);
        try { i64.set(0, 11); } catch (e) { return false; }
        let u64 = new BigUint64Array(1);
        try { u64.set(0, 12); } catch (e) { return false; }
        return f32.length == 1 && f16.length == 1 && i64.length == 1 && u64.length == 1;
    "#,
        true,
    );
}

#[test]
fn test_typedarray_global_pragmatic_surface() {
    expect_i32_runtime_node_compat(
        r#"
        let t = new TypedArray<number>(5);
        return t.length;
    "#,
        5,
    );
}

#[test]
fn test_dataview_get_set_int32() {
    expect_i32_runtime_node_compat(
        r#"
        let ab = new ArrayBuffer(16);
        let view = new DataView(ab);
        view.setInt32(4, 42, true);
        return view.getInt32(4, true);
    "#,
        42,
    );
}

#[test]
fn test_dataview_out_of_range_error_code() {
    expect_string_runtime_node_compat(
        r#"
        let ab = new ArrayBuffer(8);
        let view = new DataView(ab);
        try {
            view.getInt32(6, true);
            return "NO_ERR";
        } catch (e) {
            return e.code;
        }
    "#,
        "ERR_OUT_OF_RANGE",
    );
}

#[test]
fn test_dataview_big_endian_unimplemented_behavior_error_code() {
    expect_string_runtime_node_compat(
        r#"
        let ab = new ArrayBuffer(8);
        let view = new DataView(ab);
        try {
            view.getInt32(0, false);
            return "NO_ERR";
        } catch (e) {
            return e.code;
        }
    "#,
        "E_UNIMPLEMENTED_BUILTIN_BEHAVIOR",
    );
}

#[test]
fn test_err_factory_sets_code() {
    expect_string_runtime_node_compat(
        r#"
        let err = createRangeError(ERR_OUT_OF_RANGE, "bad index");
        return err.code;
    "#,
        "ERR_OUT_OF_RANGE",
    );
}

#[test]
fn test_internal_error_name_surface() {
    expect_string_with_builtins(
        r#"
        let err = new InternalError("boom");
        return err.name;
    "#,
        "InternalError",
    );
}

#[test]
fn test_suppressed_error_payloads() {
    expect_bool_with_builtins(
        r#"
        let cause = new Error("cause");
        let suppressed = new Error("suppressed");
        let err = new SuppressedError(cause, suppressed, "wrapped");
        return err.error != null && err.suppressed != null && err.name == "SuppressedError";
    "#,
        true,
    );
}

// ============================================================================
// Date tests
// ============================================================================

#[test]
fn test_date_new() {
    // Date.new returns current timestamp, should be > 0
    expect_bool_with_builtins(
        r#"
        let date = new Date();
        return date.getTime() > 0;
    "#,
        true,
    );
}

#[test]
fn test_date_get_components() {
    // Date should have reasonable year (2020+)
    expect_bool_with_builtins(
        r#"
        let date = new Date();
        return date.getFullYear() >= 2020;
    "#,
        true,
    );
}

#[test]
fn test_date_get_month() {
    // Month should be 0-11
    expect_bool_with_builtins(
        r#"
        let date = new Date();
        let month = date.getMonth();
        return month >= 0 && month <= 11;
    "#,
        true,
    );
}

#[test]
fn test_date_get_day() {
    // Day should be 1-31
    expect_bool_with_builtins(
        r#"
        let date = new Date();
        let day = date.getDate();
        return day >= 1 && day <= 31;
    "#,
        true,
    );
}

// ============================================================================
// Channel tests
// ============================================================================

#[test]
fn test_channel_new() {
    expect_i32_with_builtins(
        r#"
        let ch = new Channel<number>(10);
        return ch.capacity();
    "#,
        10,
    );
}

#[test]
fn test_channel_send_receive() {
    expect_i32_with_builtins(
        r#"
        let ch = new Channel<number>(1);
        ch.send(42);
        return ch.receive();
    "#,
        42,
    );
}

#[test]
fn test_channel_length() {
    expect_i32_with_builtins(
        r#"
        let ch = new Channel<number>(10);
        ch.send(1);
        ch.send(2);
        ch.send(3);
        return ch.length();
    "#,
        3,
    );
}

#[test]
fn test_channel_try_send() {
    expect_bool_with_builtins(
        r#"
        let ch = new Channel<number>(1);
        let sent = ch.trySend(42);
        return sent;
    "#,
        true,
    );
}

#[test]
fn test_channel_close() {
    expect_bool_with_builtins(
        r#"
        let ch = new Channel<number>(1);
        ch.close();
        return ch.isClosed();
    "#,
        true,
    );
}

// ============================================================================
// Integration tests - combining multiple builtins
// ============================================================================

#[test]
fn test_map_with_multiple_operations() {
    expect_i32_with_builtins(
        r#"
        let counts = new Map<string, number>();
        counts.set("apples", 5);
        counts.set("bananas", 3);
        counts.set("oranges", 7);

        let total = 0;
        let apples = counts.get("apples");
        let bananas = counts.get("bananas");
        let oranges = counts.get("oranges");

        if (apples != null) { total = total + apples; }
        if (bananas != null) { total = total + bananas; }
        if (oranges != null) { total = total + oranges; }

        return total;
    "#,
        15,
    );
}

#[test]
fn test_set_operations() {
    expect_i32_with_builtins(
        r#"
        let set = new Set<number>();

        // Add some numbers
        let i = 0;
        while (i < 5) {
            set.add(i);
            i = i + 1;
        }

        // Remove even numbers
        set.delete(0);
        set.delete(2);
        set.delete(4);

        // Count remaining (1, 3)
        return set.size;
    "#,
        2,
    );
}

// ============================================================================
// RegExp tests (primitive type - uses regular expect_* functions)
// ============================================================================

#[test]
fn test_regexp_test_basic_match() {
    expect_bool(
        r#"
        let re = new RegExp("hello", "");
        return re.test("hello world");
    "#,
        true,
    );
}

#[test]
fn test_regexp_test_no_match() {
    expect_bool(
        r#"
        let re = new RegExp("xyz", "");
        return re.test("hello world");
    "#,
        false,
    );
}

#[test]
fn test_regexp_test_case_sensitive() {
    expect_bool(
        r#"
        let re = new RegExp("HELLO", "");
        return re.test("hello world");
    "#,
        false,
    );
}

#[test]
fn test_regexp_test_case_insensitive() {
    expect_bool(
        r#"
        let re = new RegExp("HELLO", "i");
        return re.test("hello world");
    "#,
        true,
    );
}

#[test]
fn test_regexp_exec_match() {
    // exec returns [matched_text, index, ...groups] or null
    expect_i32(
        r#"
        let re = new RegExp("world", "");
        let result = re.exec("hello world");
        if (result == null) {
            return -1;
        }
        return result[1];
    "#,
        6,
    );
}

#[test]
fn test_regexp_exec_no_match() {
    expect_bool(
        r#"
        let re = new RegExp("xyz", "");
        let result = re.exec("hello world");
        return result == null;
    "#,
        true,
    );
}

#[test]
fn test_regexp_exec_matched_text() {
    expect_string(
        r#"
        let re = new RegExp("wor..", "");
        let result = re.exec("hello world");
        if (result == null) {
            return "";
        }
        return result[0];
    "#,
        "world",
    );
}

#[test]
fn test_regexp_exec_all_basic() {
    expect_i32(
        r#"
        let re = new RegExp("l", "g");
        let results = re.execAll("hello");
        return results.length;
    "#,
        2,
    );
}

#[test]
fn test_regexp_replace_simple() {
    expect_string(
        r#"
        let re = new RegExp("world", "");
        return re.replace("hello world", "universe");
    "#,
        "hello universe",
    );
}

#[test]
fn test_regexp_replace_global() {
    expect_string(
        r#"
        let re = new RegExp("l", "g");
        return re.replace("hello", "L");
    "#,
        "heLLo",
    );
}

#[test]
fn test_regexp_replace_no_match() {
    expect_string(
        r#"
        let re = new RegExp("xyz", "");
        return re.replace("hello", "world");
    "#,
        "hello",
    );
}

#[test]
fn test_regexp_split_basic() {
    expect_i32(
        r#"
        let re = new RegExp(",", "");
        let parts = re.split("a,b,c", 0);
        return parts.length;
    "#,
        3,
    );
}

#[test]
fn test_regexp_split_content() {
    expect_string(
        r#"
        let re = new RegExp(",", "");
        let parts = re.split("a,b,c", 0);
        return parts[1];
    "#,
        "b",
    );
}

#[test]
fn test_regexp_split_with_limit() {
    expect_i32(
        r#"
        let re = new RegExp(",", "");
        let parts = re.split("a,b,c,d,e", 3);
        return parts.length;
    "#,
        3,
    );
}

#[test]
fn test_regexp_stateless() {
    // RegExp is stateless - same result on repeated calls
    expect_bool(
        r#"
        let re = new RegExp("test", "");
        let r1 = re.test("test string");
        let r2 = re.test("test string");
        let r3 = re.test("test string");
        return r1 && r2 && r3;
    "#,
        true,
    );
}

#[test]
fn test_regexp_with_special_chars() {
    // Test regex special characters
    expect_bool(
        r#"
        let re = new RegExp("a.b", "");
        return re.test("a*b");
    "#,
        true,
    );
}

#[test]
fn test_regexp_digit_pattern() {
    expect_bool(
        r#"
        let re = new RegExp("[0-9]+", "");
        return re.test("abc123def");
    "#,
        true,
    );
}

#[test]
fn test_regexp_multiline_flag() {
    expect_bool(
        r#"
        let re = new RegExp("^test", "m");
        return re.test("first line\ntest line");
    "#,
        true,
    );
}

// ============================================================================
// String + RegExp method tests
// ============================================================================

#[test]
fn test_string_match_returns_array() {
    // match without global flag returns first match
    expect_bool(
        r#"
        let re = new RegExp("l+", "");
        let result = "hello".match(re);
        return result != null;
    "#,
        true,
    );
}

#[test]
fn test_string_match_matched_text() {
    expect_string(
        r#"
        let re = new RegExp("l+", "");
        let result = "hello".match(re);
        if (result == null) {
            return "";
        }
        return result[0];
    "#,
        "ll",
    );
}

#[test]
fn test_string_match_no_match() {
    expect_bool(
        r#"
        let re = new RegExp("xyz", "");
        let result = "hello".match(re);
        return result == null;
    "#,
        true,
    );
}

#[test]
fn test_string_match_global_returns_all() {
    expect_i32(
        r#"
        let re = new RegExp("l", "g");
        let result = "hello world".match(re);
        if (result == null) {
            return 0;
        }
        return result.length;
    "#,
        3,
    );
}

#[test]
fn test_string_match_global_content() {
    expect_string(
        r#"
        let re = new RegExp("o", "g");
        let result = "hello world".match(re);
        if (result == null) {
            return "";
        }
        return result[0];
    "#,
        "o",
    );
}

#[test]
fn test_string_match_all_basic() {
    expect_i32(
        r#"
        let re = new RegExp("l", "g");
        let results = "hello".matchAll(re);
        return results.length;
    "#,
        2,
    );
}

#[test]
fn test_string_match_all_with_index() {
    // matchAll returns array of [match, index, ...groups]
    expect_i32(
        r#"
        let re = new RegExp("o", "g");
        let results = "hello world".matchAll(re);
        // First match at index 4
        return results[0][1];
    "#,
        4,
    );
}

#[test]
fn test_string_search_found() {
    expect_i32(
        r#"
        let re = new RegExp("world", "");
        return "hello world".search(re);
    "#,
        6,
    );
}

#[test]
fn test_string_search_not_found() {
    expect_i32(
        r#"
        let re = new RegExp("xyz", "");
        return "hello world".search(re);
    "#,
        -1,
    );
}

#[test]
fn test_string_search_pattern() {
    expect_i32(
        r#"
        let re = new RegExp("[0-9]+", "");
        return "abc123def".search(re);
    "#,
        3,
    );
}

#[test]
fn test_string_replace_regexp() {
    expect_string(
        r#"
        let re = new RegExp("world", "");
        return "hello world".replace(re, "universe");
    "#,
        "hello universe",
    );
}

#[test]
fn test_string_replace_regexp_global() {
    expect_string(
        r#"
        let re = new RegExp("l", "g");
        return "hello".replace(re, "L");
    "#,
        "heLLo",
    );
}

#[test]
fn test_string_replace_regexp_no_match() {
    expect_string(
        r#"
        let re = new RegExp("xyz", "");
        return "hello".replace(re, "world");
    "#,
        "hello",
    );
}

#[test]
fn test_string_replace_regexp_pattern() {
    expect_string(
        r#"
        let re = new RegExp("[0-9]+", "");
        return "abc123def".replace(re, "X");
    "#,
        "abcXdef",
    );
}

// Test string split with string separator (now requires 2 args: separator, limit)
#[test]
fn test_string_split_string() {
    expect_i32(
        r#"
        let parts = "a,b,c".split(",", 0);
        return parts.length;
    "#,
        3,
    );
}

#[test]
fn test_string_split_string_content() {
    expect_string(
        r#"
        let parts = "a,b,c".split(",", 0);
        return parts[1];
    "#,
        "b",
    );
}

#[test]
fn test_string_split_string_with_limit() {
    expect_i32(
        r#"
        let parts = "a,b,c,d,e".split(",", 3);
        return parts.length;
    "#,
        3,
    );
}

#[test]
fn test_string_split_regexp() {
    expect_i32(
        r#"
        let re = new RegExp(",", "");
        let parts = "a,b,c".split(re, 0);
        return parts.length;
    "#,
        3,
    );
}

#[test]
fn test_string_split_regexp_content() {
    expect_string(
        r#"
        let re = new RegExp(",", "");
        let parts = "a,b,c".split(re, 0);
        return parts[1];
    "#,
        "b",
    );
}

#[test]
fn test_string_split_regexp_with_limit() {
    expect_i32(
        r#"
        let re = new RegExp(",", "");
        let parts = "a,b,c,d,e".split(re, 3);
        return parts.length;
    "#,
        3,
    );
}

#[test]
fn test_string_split_regexp_pattern() {
    // Split on whitespace pattern
    expect_i32(
        r#"
        let re = new RegExp("\\s+", "");
        let parts = "hello   world   test".split(re, 0);
        return parts.length;
    "#,
        3,
    );
}

#[test]
fn test_string_split_regexp_pattern_content() {
    expect_string(
        r#"
        let re = new RegExp("\\s+", "");
        let parts = "hello   world   test".split(re, 0);
        return parts[1];
    "#,
        "world",
    );
}

// ============================================================================
// String replaceWith (callback-based replacement)
// ============================================================================

#[test]
fn test_string_replace_with_simple() {
    // Replace match with constant string
    expect_string(
        r#"
        let re = new RegExp("world", "");
        let result = "hello world".replaceWith(re, (match: (string | number)[]): string => {
            return "UNIVERSE";
        });
        return result;
    "#,
        "hello UNIVERSE",
    );
}

#[test]
fn test_string_replace_with_global() {
    // Replace all matches globally
    expect_string(
        r#"
        let re = new RegExp("l", "g");
        let result = "hello".replaceWith(re, (match: (string | number)[]): string => {
            return "L";
        });
        return result;
    "#,
        "heLLo",
    );
}

#[test]
fn test_string_replace_with_no_match() {
    // No match - return original string
    expect_string(
        r#"
        let re = new RegExp("xyz", "");
        let result = "hello".replaceWith(re, (match: (string | number)[]): string => {
            return "REPLACED";
        });
        return result;
    "#,
        "hello",
    );
}

#[test]
fn test_string_replace_with_pattern() {
    // Replace digits with X
    expect_string(
        r#"
        let re = new RegExp("[0-9]+", "g");
        let result = "a1b22c333".replaceWith(re, (match: (string | number)[]): string => {
            return "X";
        });
        return result;
    "#,
        "aXbXcX",
    );
}

#[test]
fn test_string_replace_with_non_global() {
    // Without global flag, replace only first match
    expect_string(
        r#"
        let re = new RegExp("o", "");
        let result = "foo bar boo".replaceWith(re, (match: (string | number)[]): string => {
            return "O";
        });
        return result;
    "#,
        "fOo bar boo",
    );
}

// ============================================================================
// Native Call Basic Test
// ============================================================================

#[test]
fn test_native_call_basic() {
    // Test that native calls work correctly through string methods
    // String.charAt calls NATIVE_CALL(0x0200) internally
    expect_string(
        r#"
        let s = "hello";
        return s.charAt(1);
    "#,
        "e",
    );
}

// ============================================================================
// RegExpMatch Class Tests
// ============================================================================

#[test]
fn test_regexp_match_properties() {
    // Test that exec returns an array with [matched_text, index, ...groups]
    // The match text should be at index 0
    expect_string(
        r#"
        let re = new RegExp("world", "");
        let result = re.exec("hello world");
        if (result == null) {
            return "";
        }
        return result[0];
    "#,
        "world",
    );
}

#[test]
fn test_regexp_match_index() {
    // Test that the match index is correct (index 1 of result array)
    expect_i32(
        r#"
        let re = new RegExp("world", "");
        let result = re.exec("hello world");
        if (result == null) {
            return -1;
        }
        return result[1];
    "#,
        6,
    );
}

#[test]
fn test_regexp_match_groups() {
    // Test capture groups - pattern with groups returns captured content
    expect_string(
        r#"
        let re = new RegExp("(\\w+)@(\\w+)", "");
        let result = re.exec("email: user@domain");
        if (result == null) {
            return "";
        }
        // result[0] = full match "user@domain"
        // result[1] = index
        // result[2] = first group "user"
        // result[3] = second group "domain"
        return result[2];
    "#,
        "user",
    );
}

#[test]
fn test_regexp_match_groups_second() {
    // Test second capture group
    expect_string(
        r#"
        let re = new RegExp("(\\w+)@(\\w+)", "");
        let result = re.exec("email: user@domain");
        if (result == null) {
            return "";
        }
        // result[3] = second group "domain"
        return result[3];
    "#,
        "domain",
    );
}

// ============================================================================
// Date getter tests (new handlers)
// ============================================================================

#[test]
fn test_date_get_hours() {
    // Epoch (setTime(0)) = Jan 1, 1970 00:00:00 UTC
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        return date.getHours();
    "#,
        0,
    );
}

#[test]
fn test_date_get_minutes() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        return date.getMinutes();
    "#,
        0,
    );
}

#[test]
fn test_date_get_seconds() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        return date.getSeconds();
    "#,
        0,
    );
}

#[test]
fn test_date_get_milliseconds() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        return date.getMilliseconds();
    "#,
        0,
    );
}

#[test]
fn test_date_get_hours_nonzero() {
    // 3600000ms = 1 hour from epoch
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(3600000);
        return date.getHours();
    "#,
        1,
    );
}

#[test]
fn test_date_get_minutes_nonzero() {
    // 900000ms = 15 minutes from epoch
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(900000);
        return date.getMinutes();
    "#,
        15,
    );
}

#[test]
fn test_date_get_seconds_nonzero() {
    // 45000ms = 45 seconds from epoch
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(45000);
        return date.getSeconds();
    "#,
        45,
    );
}

#[test]
fn test_date_get_milliseconds_nonzero() {
    // 123ms from epoch
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(123);
        return date.getMilliseconds();
    "#,
        123,
    );
}

// ============================================================================
// Date setter tests
// ============================================================================

#[test]
fn test_date_set_full_year() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        date.setFullYear(2024);
        return date.getFullYear();
    "#,
        2024,
    );
}

#[test]
fn test_date_set_month() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        date.setMonth(5);
        return date.getMonth();
    "#,
        5,
    );
}

#[test]
fn test_date_set_date() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        date.setDate(15);
        return date.getDate();
    "#,
        15,
    );
}

#[test]
fn test_date_set_hours() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        date.setHours(14);
        return date.getHours();
    "#,
        14,
    );
}

#[test]
fn test_date_set_minutes() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        date.setMinutes(30);
        return date.getMinutes();
    "#,
        30,
    );
}

#[test]
fn test_date_set_seconds() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        date.setSeconds(45);
        return date.getSeconds();
    "#,
        45,
    );
}

#[test]
fn test_date_set_milliseconds() {
    expect_i32_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        date.setMilliseconds(999);
        return date.getMilliseconds();
    "#,
        999,
    );
}

// ============================================================================
// Date formatting tests
// ============================================================================

#[test]
fn test_date_to_iso_string() {
    expect_string_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        return date.toISOString();
    "#,
        "1970-01-01T00:00:00.000Z",
    );
}

#[test]
fn test_date_to_date_string() {
    expect_string_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        return date.toDateString();
    "#,
        "Thu Jan 01 1970",
    );
}

#[test]
fn test_date_to_time_string() {
    expect_string_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        return date.toTimeString();
    "#,
        "00:00:00",
    );
}

#[test]
fn test_date_to_string() {
    expect_string_with_builtins(
        r#"
        let date = new Date();
        date.setTime(0);
        return date.toString();
    "#,
        "Thu Jan 01 1970 00:00:00",
    );
}

// Note: Date.parse tests require adding a static parse() method to date.raya
// The VM handler (DATE_PARSE) exists but the class method is not yet defined

// ============================================================================
// Object tests
// ============================================================================

#[test]
fn test_object_hash_code() {
    // hashCode should return an integer
    expect_bool_with_builtins(
        r#"
        let obj = new Object();
        let hash = obj.hashCode();
        return hash == hash;
    "#,
        true,
    );
}

#[test]
fn test_object_equals_same() {
    // An object should equal itself
    expect_bool_with_builtins(
        r#"
        let obj = new Object();
        return obj.equals(obj);
    "#,
        true,
    );
}

#[test]
fn test_object_to_string() {
    expect_string_with_builtins(
        r#"
        let obj = new Object();
        return obj.toString();
    "#,
        "[object Object]",
    );
}

// ============================================================================
// Number method tests
// ============================================================================

#[test]
fn test_number_to_fixed_zero() {
    expect_string(
        r#"
        let x: number = 3.14159;
        return x.toFixed(0);
    "#,
        "3",
    );
}

#[test]
fn test_number_to_fixed_two() {
    expect_string(
        r#"
        let x: number = 3.14159;
        return x.toFixed(2);
    "#,
        "3.14",
    );
}

#[test]
fn test_number_to_fixed_four() {
    expect_string(
        r#"
        let x: number = 3.14159;
        return x.toFixed(4);
    "#,
        "3.1416",
    );
}

#[test]
fn test_number_to_precision() {
    expect_string(
        r#"
        let x: number = 123.456;
        return x.toPrecision(5);
    "#,
        "123.46",
    );
}

#[test]
fn test_number_to_precision_one() {
    expect_string(
        r#"
        let x: number = 123.456;
        return x.toPrecision(1);
    "#,
        "100",
    );
}

#[test]
fn test_number_to_string_decimal() {
    expect_string(
        r#"
        let x: number = 255;
        return x.toString(10);
    "#,
        "255",
    );
}

#[test]
fn test_number_to_string_binary() {
    expect_string(
        r#"
        let x: number = 255;
        return x.toString(2);
    "#,
        "11111111",
    );
}

#[test]
fn test_number_to_string_hex() {
    expect_string(
        r#"
        let x: number = 255;
        return x.toString(16);
    "#,
        "ff",
    );
}

#[test]
fn test_number_to_string_octal() {
    expect_string(
        r#"
        let x: number = 255;
        return x.toString(8);
    "#,
        "377",
    );
}

// ============================================================================
// Map.keys / Map.values / Map.entries
// ============================================================================

#[test]
fn test_map_keys() {
    expect_i32_with_builtins(
        r#"
        let m = new Map<string, number>();
        m.set("a", 1);
        m.set("b", 2);
        m.set("c", 3);
        let keys = m.keys();
        return keys.length;
    "#,
        3,
    );
}

#[test]
fn test_map_keys_transform() {
    // Same pattern as the failing test_collection_map_transform
    expect_i32_with_builtins(
        r#"
        let original = new Map<string, number>();
        original.set("a", 1);
        original.set("b", 2);
        original.set("c", 3);

        let inverse = new Map<number, string>();
        let keys = original.keys();
        for (let i = 0; i < keys.length; i = i + 1) {
            let val = original.get(keys[i]);
            if (val != null) {
                inverse.set(val, keys[i]);
            }
        }

        let result = 0;
        let valFor1 = inverse.get(1);
        let valFor2 = inverse.get(2);
        let valFor3 = inverse.get(3);
        if (valFor1 == "a") { result = result + 1; }
        if (valFor2 == "b") { result = result + 10; }
        if (valFor3 == "c") { result = result + 100; }

        return result + inverse.size;
    "#,
        114,
    );
}

#[test]
fn test_map_values() {
    expect_i32_with_builtins(
        r#"
        let m = new Map<string, number>();
        m.set("x", 10);
        m.set("y", 20);
        let vals = m.values();
        let sum = 0;
        for (let i = 0; i < vals.length; i = i + 1) {
            sum = sum + vals[i];
        }
        return sum;
    "#,
        30,
    );
}

// ============================================================================
// Set.values / Set.union / Set.intersection / Set.difference
// ============================================================================

#[test]
fn test_set_values() {
    expect_i32_with_builtins(
        r#"
        let s = new Set<number>();
        s.add(10);
        s.add(20);
        s.add(30);
        let vals = s.values();
        return vals.length;
    "#,
        3,
    );
}

#[test]
fn test_set_intersection() {
    expect_i32_with_builtins(
        r#"
        let setA = new Set<number>();
        setA.add(1); setA.add(2); setA.add(3); setA.add(4); setA.add(5);

        let setB = new Set<number>();
        setB.add(3); setB.add(4); setB.add(5); setB.add(6); setB.add(7);

        let inter = setA.intersection(setB);
        return inter.size;
    "#,
        3,
    );
}

#[test]
fn test_set_union() {
    expect_i32_with_builtins(
        r#"
        let setA = new Set<number>();
        setA.add(1); setA.add(2); setA.add(3);

        let setB = new Set<number>();
        setB.add(3); setB.add(4); setB.add(5);

        let uni = setA.union(setB);
        return uni.size;
    "#,
        5,
    );
}

#[test]
fn test_set_difference() {
    expect_i32_with_builtins(
        r#"
        let setA = new Set<number>();
        setA.add(1); setA.add(2); setA.add(3); setA.add(4);

        let setB = new Set<number>();
        setB.add(2); setB.add(4);

        let diff = setA.difference(setB);
        return diff.size;
    "#,
        2,
    );
}

#[test]
fn test_set_operations_combined() {
    // Same pattern as the failing test_collection_set_operations
    expect_i32_with_builtins(
        r#"
        let setA = new Set<number>();
        setA.add(1); setA.add(2); setA.add(3); setA.add(4); setA.add(5);

        let setB = new Set<number>();
        setB.add(3); setB.add(4); setB.add(5); setB.add(6); setB.add(7);

        let inter = setA.intersection(setB);
        let uni = setA.union(setB);

        return inter.size * 10 + uni.size;
    "#,
        37,
    );
}

#[test]
fn test_set_deduplication() {
    // Same pattern as the failing test_collection_deduplication
    expect_i32_with_builtins(
        r#"
        let input: number[] = [1, 3, 2, 3, 1, 4, 2, 5, 4, 3];
        let seen = new Set<number>();
        let unique: number[] = [];

        for (let i = 0; i < input.length; i = i + 1) {
            if (!seen.has(input[i])) {
                seen.add(input[i]);
                unique.push(input[i]);
            }
        }

        let sum = 0;
        for (let i = 0; i < unique.length; i = i + 1) {
            sum = sum + unique[i];
        }

        return sum * 10 + unique.length;
    "#,
        155,
    );
}

// ============================================================================
// Node-compat globals tests
// ============================================================================

#[test]
fn test_node_compat_parseint_basic() {
    expect_i32_runtime_node_compat(
        r#"
        return parseInt("42");
    "#,
        42,
    );
}

#[test]
fn test_node_compat_parseint_signed_decimal() {
    expect_i32_runtime_node_compat(
        r#"
        return parseInt("  -42");
    "#,
        -42,
    );
}

#[test]
fn test_node_compat_parsefloat_basic() {
    expect_i32_runtime_node_compat(
        r#"
        let v = parseFloat("3.5");
        return (v * 10.0) as int;
    "#,
        35,
    );
}

#[test]
fn test_node_compat_isnan_and_isfinite() {
    expect_bool_runtime_node_compat(
        r#"
        let nan = parseFloat("not-a-number");
        let n = 10.5;
        return isNaN(nan) && !isFinite(nan) && isFinite(n);
    "#,
        true,
    );
}

#[test]
fn test_node_compat_escape_unescape_roundtrip_space() {
    expect_string_runtime_node_compat(
        r#"
        let s = "hello world";
        return unescape(escape(s));
    "#,
        "hello world",
    );
}

#[test]
fn test_node_compat_globalthis_exists() {
    expect_bool_runtime_node_compat(
        r#"
        return globalThis != null;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_array_global_from() {
    expect_i32_runtime_node_compat(
        r#"
        let values = Array.from([1, 2, 3]);
        return values[2];
    "#,
        3,
    );
}

#[test]
fn test_node_compat_object_is_basic() {
    expect_bool_runtime_node_compat(
        r#"
        return Object.is(1, 1);
    "#,
        true,
    );
}

#[test]
fn test_node_compat_reflect_global_basic_ops() {
    expect_bool_runtime_node_compat(
        r#"
        let o = new Object();
        let okSet = Reflect.set(o, "x", 10);
        let okHas = Reflect.has(o, "x");
        let got = Reflect.get(o, "x");
        return okSet && okHas && got == 10;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_reflect_structural_object_ops() {
    expect_bool_runtime_node_compat(
        r#"
        let o = { a: 1 };
        let okSetFixed = Reflect.set(o, "a", 7);
        let okSetDyn = Reflect.set(o, "extra", 9);
        let names = Reflect.getFieldNames(o);
        let sawA = false;
        let sawExtra = false;
        for (let i = 0; i < names.length; i = i + 1) {
            if (names[i] == "a") sawA = true;
            if (names[i] == "extra") sawExtra = true;
        }
        return okSetFixed
            && okSetDyn
            && Reflect.get(o, "a") == 7
            && Reflect.get(o, "extra") == 9
            && Reflect.has(o, "a")
            && Reflect.has(o, "extra")
            && sawA
            && sawExtra;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_reflect_descriptor_uses_layout_field_names() {
    expect_bool_runtime_node_compat(
        r#"
        let d = Object.getOwnPropertyDescriptor({ a: 1 }, "a");
        if (d == null) return false;
        let names = Reflect.getFieldNames(d);
        let sawValue = false;
        let sawWritable = false;
        for (let i = 0; i < names.length; i = i + 1) {
            if (names[i] == "value") sawValue = true;
            if (names[i] == "writable") sawWritable = true;
        }
        return Reflect.get(d, "value") == 1
            && Reflect.has(d, "configurable")
            && Reflect.getFieldInfo(d, "value") != null
            && sawValue
            && sawWritable;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_reflect_has_method_on_callable_structural_field() {
    expect_bool_runtime_node_compat(
        r#"
        let o = {
            run: (): number => {
                return 1;
            }
        };
        return Reflect.hasMethod(o, "run") && Reflect.get(o, "run") != null;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_proxy_reflect_runtime_integration() {
    expect_bool_runtime_node_compat(
        r#"
        function main(): boolean {
            let target = new Object();
            Reflect.set(target, "x", 7);
            let handler = new Object();
            let proxy = new Proxy<Object>(target, handler);
            let a = proxy.isProxy();
            let b = Reflect.isProxy(proxy);
            let t1 = proxy.getTarget();
            let t2 = Reflect.getProxyTarget(proxy);
            if (t1 == null || t2 == null) {
                return false;
            }
            let v = Reflect.get(target, "x");
            return a && b && v == 7;
        }
    "#,
        true,
    );
}

#[test]
fn test_node_compat_proxy_reflect_get_trap() {
    expect_bool_runtime_node_compat(
        r#"
        function main(): boolean {
            let target = new Object();
            Reflect.set(target, "x", 7);
            let handler = new Object();
            handler["get"] = (t: Object, k: string): number => {
                return 99;
            };
            let proxy = new Proxy<Object>(target, handler);
            return Reflect.get(proxy, "x") == 99;
        }
    "#,
        true,
    );
}

#[test]
fn test_node_compat_proxy_reflect_set_trap() {
    expect_bool_runtime_node_compat(
        r#"
        function main(): boolean {
            let target = new Object();
            let handler = new Object();
            handler["set"] = (t: Object, k: string, v: Object | string | number | boolean | null): boolean => {
                Reflect.set(t, k, (v as number) + 1);
                return true;
            };
            let proxy = new Proxy<Object>(target, handler);
            let ok = Reflect.set(proxy, "x", 7);
            return ok && Reflect.get(target, "x") == 8;
        }
    "#,
        true,
    );
}

#[test]
fn test_node_compat_proxy_reflect_has_trap() {
    expect_bool_runtime_node_compat(
        r#"
        function main(): boolean {
            let target = new Object();
            let handler = new Object();
            handler["has"] = (_t: Object, _k: string): boolean => {
                return true;
            };
            let proxy = new Proxy<Object>(target, handler);
            return Reflect.has(proxy, "missing");
        }
    "#,
        true,
    );
}

#[test]
fn test_node_compat_proxy_has_trap_respects_nonconfigurable_invariant() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let target = new Object();
            Object.defineProperty(target, "x", { value: 1, configurable: false });
            let handler = new Object();
            handler["has"] = (_t: Object, _k: string): boolean => false;
            let proxy = new Proxy<Object>(target, handler);
            try {
                Reflect.has(proxy, "x");
                return "NO_THROW";
            } catch (e) {
                return e.name;
            }
        }
    "#,
        "TypeError",
    );
}

#[test]
fn test_node_compat_proxy_get_trap_respects_frozen_data_invariant() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let target = new Object();
            Object.defineProperty(target, "x", {
                value: 1,
                writable: false,
                configurable: false
            });
            let handler = new Object();
            handler["get"] = (_t: Object, _k: string): number => 2;
            let proxy = new Proxy<Object>(target, handler);
            try {
                Reflect.get(proxy, "x");
                return "NO_THROW";
            } catch (e) {
                return e.name;
            }
        }
    "#,
        "TypeError",
    );
}

#[test]
fn test_node_compat_proxy_set_trap_respects_frozen_data_invariant() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let target = new Object();
            Object.defineProperty(target, "x", {
                value: 1,
                writable: false,
                configurable: false
            });
            let handler = new Object();
            handler["set"] = (_t: Object, _k: string, _v: number): boolean => true;
            let proxy = new Proxy<Object>(target, handler);
            try {
                Reflect.set(proxy, "x", 2);
                return "NO_THROW";
            } catch (e) {
                return JSON.stringify([e.name, Reflect.get(target, "x")]);
            }
        }
    "#,
        "[\"TypeError\",1]",
    );
}

#[test]
fn test_node_compat_proxy_delete_trap_respects_nonconfigurable_invariant() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let target = new Object();
            Object.defineProperty(target, "x", { value: 1, configurable: false });
            let handler = new Object();
            handler["deleteProperty"] = (_t: Object, _k: string): boolean => true;
            let proxy = new Proxy<Object>(target, handler);
            try {
                Object.deleteProperty(proxy, "x");
                return "NO_THROW";
            } catch (e) {
                return e.name;
            }
        }
    "#,
        "TypeError",
    );
}

#[test]
fn test_node_compat_proxy_define_trap_respects_existing_property_invariant() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let target = new Object();
            Object.defineProperty(target, "x", {
                value: 1,
                writable: false,
                configurable: false
            });
            let handler = new Object();
            handler["defineProperty"] = (_t: Object, _k: string, _d: Object): boolean => true;
            let proxy = new Proxy<Object>(target, handler);
            try {
                Object.defineProperty(proxy, "x", { value: 2 });
                return "NO_THROW";
            } catch (e) {
                return JSON.stringify([e.name, Reflect.get(target, "x")]);
            }
        }
    "#,
        "[\"TypeError\",1]",
    );
}

#[test]
fn test_node_compat_intl_number_format_basic() {
    expect_string_runtime_node_compat(
        r#"
        let nf = Intl.NumberFormat("en-US", null);
        return nf.format(1234.5);
    "#,
        "1234.5",
    );
}

#[test]
fn test_node_compat_intl_datetime_format_iso_fallback() {
    expect_bool_runtime_node_compat(
        r#"
        let d = new Date();
        let df = Intl.DateTimeFormat("en-US", null);
        return df.format(d) == d.toISOString();
    "#,
        true,
    );
}

#[test]
fn test_node_compat_intl_resolved_options_locale() {
    expect_string_runtime_node_compat(
        r#"
        let nf = Intl.NumberFormat("id-ID", null);
        let opts = nf.resolvedOptions();
        return opts.locale;
    "#,
        "id-ID",
    );
}

#[test]
fn test_temporal_instant_to_string_epoch() {
    expect_string_with_builtins(
        r#"
        let inst = Temporal.Instant(0);
        return inst.toString();
    "#,
        "1970-01-01T00:00:00.000Z",
    );
}

#[test]
fn test_temporal_plain_date_to_string() {
    expect_string_with_builtins(
        r#"
        let d = Temporal.PlainDate(2026, 2, 6);
        return d.toString();
    "#,
        "2026-02-06",
    );
}

#[test]
fn test_temporal_plain_time_to_string() {
    expect_string_with_builtins(
        r#"
        let t = Temporal.PlainTime(3, 4, 5, 6);
        return t.toString();
    "#,
        "03:04:05.006",
    );
}

#[test]
fn test_temporal_zoned_datetime_to_string_suffix() {
    expect_bool_with_builtins(
        r#"
        let z = Temporal.ZonedDateTime(0, "UTC");
        return z.toString().endsWith("[UTC]");
    "#,
        true,
    );
}

#[test]
fn test_iterator_from_array_next_and_done() {
    expect_i32_runtime(
        r#"
        function main(): int {
            let it = Iterator.fromArray<number>([7, 8]);
            let a = it.next();
            let b = it.next();
            let c = it.next();
            if (a.value == null || b.value == null) {
                return -1;
            }
            return (a.value as int) * 100 + (b.value as int) * 10 + (c.done ? 1 : 0);
        }
    "#,
        781,
    );
}

#[test]
fn test_iterator_to_array_remaining_values() {
    expect_i32_runtime(
        r#"
        function main(): int {
            let it = Iterator.fromArray<number>([1, 2, 3, 4]);
            let _first = it.next();
            let rest = it.toArray();
            return rest.length * 10 + rest[0];
        }
    "#,
        32,
    );
}

#[test]
fn test_node_compat_for_of_uses_symbol_iterator_protocol_and_closes_on_break() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let log: string[] = [];
            let iterable = {
                [Symbol.iterator]() {
                    return {
                        i: 0,
                        next() {
                            if (this.i < 2) {
                                this.i = this.i + 1;
                                return { value: this.i, done: false };
                            }
                            return { value: undefined, done: true };
                        },
                        return() {
                            log.push("closed");
                            return { done: true };
                        }
                    };
                }
            };
            let out: number[] = [];
            for (let value of iterable) {
                out.push(value);
                break;
            }
            return JSON.stringify([out, log]);
        }
        main()
    "#,
        "[[1],[\"closed\"]]",
    );
}

#[test]
fn test_node_compat_array_spread_uses_symbol_iterator_protocol() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let iterable = {
                [Symbol.iterator]() {
                    return {
                        i: 0,
                        next() {
                            if (this.i < 3) {
                                let value = this.i;
                                this.i = this.i + 1;
                                return { value, done: false };
                            }
                            return { done: true };
                        }
                    };
                }
            };
            return JSON.stringify([...iterable]);
        }
        main()
    "#,
        "[0,1,2]",
    );
}

#[test]
fn test_node_compat_array_destructuring_uses_symbol_iterator_protocol() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let iterable = {
                [Symbol.iterator]() {
                    return {
                        i: 0,
                        next() {
                            if (this.i < 3) {
                                let value = this.i;
                                this.i = this.i + 1;
                                return { value, done: false };
                            }
                            return { done: true };
                        }
                    };
                }
            };
            let [a, b, ...rest] = iterable;
            return JSON.stringify([a, b, rest]);
        }
        main()
    "#,
        "[0,1,[2]]",
    );
}

#[test]
fn test_node_compat_function_constructor_unimplemented_behavior_error_code() {
    expect_string_runtime_node_compat(
        r#"
        try {
            let f = new Function("return 1;");
            return "NO_ERR";
        } catch (e) {
            return e.code;
        }
    "#,
        "E_UNIMPLEMENTED_BUILTIN_BEHAVIOR",
    );
}

#[test]
fn test_node_compat_disposable_stack_lifo_order() {
    expect_i32_runtime_node_compat(
        r#"
        let out = 0;
        let s = new DisposableStack();
        s.defer((): void => {
            out = out * 10 + 1;
        });
        s.defer((): void => {
            out = out * 10 + 2;
        });
        s.dispose();
        return out;
    "#,
        21,
    );
}

#[test]
fn test_node_compat_disposable_stack_move_transfers_callbacks() {
    expect_i32_runtime_node_compat(
        r#"
        let out = 0;
        let s1 = new DisposableStack();
        s1.defer((): void => {
            out = out + 7;
        });
        let s2 = s1.move();
        s1.dispose();
        s2.dispose();
        return out;
    "#,
        7,
    );
}

#[test]
fn test_node_compat_async_disposable_stack_lifo_order() {
    expect_i32_runtime_node_compat(
        r#"
        let out = 0;
        let s = new AsyncDisposableStack();
        s.defer(async (): Promise<void> => {
            out = out * 10 + 1;
        });
        s.defer(async (): Promise<void> => {
            out = out * 10 + 2;
        });
        await s.disposeAsync();
        return out;
    "#,
        21,
    );
}

#[test]
fn test_node_compat_shared_array_buffer_byte_length() {
    expect_i32_runtime_node_compat(
        r#"
        let sab = new SharedArrayBuffer(24);
        return sab.byteLength;
    "#,
        24,
    );
}

#[test]
fn test_node_compat_atomics_add_and_load() {
    expect_i32_runtime_node_compat(
        r#"
        let sab = new SharedArrayBuffer(16);
        let a = new Int32Array(sab);
        Atomics.store(a, 0, 10);
        let old = Atomics.add(a, 0, 5);
        return old * 10 + Atomics.load(a, 0);
    "#,
        115,
    );
}

#[test]
fn test_node_compat_atomics_compare_exchange() {
    expect_i32_runtime_node_compat(
        r#"
        let sab = new SharedArrayBuffer(16);
        let a = new Int32Array(sab);
        Atomics.store(a, 0, 9);
        let old1 = Atomics.compareExchange(a, 0, 9, 12);
        let old2 = Atomics.compareExchange(a, 0, 9, 20);
        return old1 * 100 + old2 * 10 + Atomics.load(a, 0);
    "#,
        1032,
    );
}

#[test]
fn test_node_compat_atomics_wait_unimplemented_behavior_error_code() {
    expect_string_runtime_node_compat(
        r#"
        try {
            let sab = new SharedArrayBuffer(16);
            let a = new Int32Array(sab);
            return Atomics.wait(a, 0, 0, 0);
        } catch (e) {
            return e.code;
        }
    "#,
        "E_UNIMPLEMENTED_BUILTIN_BEHAVIOR",
    );
}

#[test]
fn test_uri_helpers_roundtrip_strict_surface() {
    expect_bool_with_builtins(
        r#"
        let s = "a b%";
        let e1 = encodeURI(s);
        let d1 = decodeURI(e1);
        let e2 = encodeURIComponent(s);
        let d2 = decodeURIComponent(e2);
        return d1 == s && d2 == s;
    "#,
        true,
    );
}

#[test]
fn test_shared_numeric_constants_and_undefined_surface() {
    expect_bool_with_builtins(
        r#"
        return Infinity > 1.0 && NaN != NaN && undefined == null;
    "#,
        true,
    );
}

#[test]
fn test_constructor_globals_strict_surface() {
    expect_bool_runtime(
        r#"
        let b = Boolean("x");
        let n = Number("42");
        let s = String(42);
        let a = new Array<number>(2);
        a[0] = 7;
        a[1] = 8;
        let b2 = new Array<number>(1, 2);
        return b && n == 42 && s == "42" && a.length == 2 && (a[0] + a[1]) == 15 && b2.length == 2;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_eval_unimplemented_behavior_error_code() {
    expect_string_runtime_node_compat(
        r#"
        try {
            eval("1 + 1");
            return "NO_ERR";
        } catch (e) {
            return e.code;
        }
    "#,
        "E_UNIMPLEMENTED_BUILTIN_BEHAVIOR",
    );
}

#[test]
fn test_node_compat_arguments_object_is_stable_within_activation() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function(a) {
                return JSON.stringify([arguments === arguments, arguments.length]);
            };
            return probe(7);
        }
    "#,
        "[true,1]",
    );
}

#[test]
fn test_node_compat_direct_eval_in_default_param_closure_sees_eval_arguments_binding() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let f = (p = eval("var arguments = 'param'"), q = () => arguments) => {
                function arguments() {}
                return JSON.stringify([typeof arguments, q()]);
            };
            return f();
        }
    "#,
        "[\"function\",\"param\"]",
    );
}

#[test]
fn test_node_compat_direct_eval_in_default_param_rejects_arguments_decl_for_function() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            function probe(p = eval("var arguments")) {
                function arguments() {}
            }
            try {
                probe();
                return "NO_THROW";
            } catch (e) {
                return e.name;
            }
        }
    "#,
        "SyntaxError",
    );
}

#[test]
fn test_node_compat_eval_returns_non_string_argument_unchanged() {
    expect_bool_runtime_node_compat(
        r#"
        function main(): boolean {
            let value = { marker: 1 };
            return eval(value) === value;
        }
    "#,
        true,
    );
}

#[test]
fn test_node_compat_hoisted_js_closure_capture_sees_post_init_value() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            function testcase() {
                var x = 0;
                function f() {
                    return JSON.stringify([x, typeof x]);
                }
                return f();
            }
            return testcase();
        }
    "#,
        "[0,\"number\"]",
    );
}

#[test]
fn test_node_compat_strict_direct_eval_var_does_not_leak_to_caller_binding() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            "use strict";
            function testcase() {
                var x = 0;
                function f() {
                    eval("var x = 1");
                    return JSON.stringify([x, typeof x]);
                }
                return f();
            }
            return testcase();
        }
    "#,
        "[0,\"number\"]",
    );
}

#[test]
fn test_node_compat_js_eval_wrapper_hoisted_function_captures_top_level_lexical() {
    expect_string_runtime_node_compat(
        r#"
        "use strict";
        let x = 1;
        function f() {
            return JSON.stringify([x, typeof x]);
        }
        return f();
    "#,
        "[1,\"number\"]",
    );
}

#[test]
fn test_node_compat_strict_direct_eval_block_function_stays_block_local() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let errName = "NONE";
            eval("\"use strict\"; { function hidden() {} }");
            try {
                hidden;
            } catch (e) {
                errName = e.name;
            }
            return errName;
        }
    "#,
        "ReferenceError",
    );
}

#[test]
fn test_node_compat_top_level_strict_direct_eval_inherits_strictness() {
    expect_string_runtime_node_compat(
        r#"
        "use strict";
        let errName = "NONE";
        eval("{ function hidden() {} }");
        try {
            hidden;
        } catch (e) {
            errName = e.name;
        }
        return errName;
    "#,
        "ReferenceError",
    );
}

#[test]
fn test_node_compat_direct_eval_new_function_binding_may_be_deleted() {
    expect_string_runtime_node_compat(
        r#"
        var initial;
        var postDeletion;

        function main(): string {
            (function() {
                eval("initial = typeof f; delete f; postDeletion = function(){ try { f; return 'alive'; } catch (e) { return e.name; } }; function f() { return 33; }");
            }());
            return JSON.stringify([initial, Reflect.apply(postDeletion, null, [])]);
        }
    "#,
        "[\"function\",\"ReferenceError\"]",
    );
}

#[test]
fn test_node_compat_direct_eval_new_var_binding_may_be_deleted() {
    expect_string_runtime_node_compat(
        r#"
        var initial = null;
        var postDeletion;

        function main(): string {
            (function() {
                eval("initial = x; delete x; postDeletion = function(){ try { x; return 'alive'; } catch (e) { return e.name; } }; var x;");
            }());
            return JSON.stringify([initial === undefined, Reflect.apply(postDeletion, null, [])]);
        }
    "#,
        "[true,\"ReferenceError\"]",
    );
}

#[test]
fn test_node_compat_arguments_object_sloppy_mapping_tracks_param_assignment() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function(a) {
                a = 9;
                return JSON.stringify([a, arguments[0], arguments.length]);
            };
            return probe(7);
        }
    "#,
        "[9,9,1]",
    );
}

#[test]
fn test_node_compat_arguments_object_sloppy_mapping_tracks_arguments_write() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function(a) {
                arguments[0] = 9;
                return JSON.stringify([a, arguments[0], arguments.length]);
            };
            return probe(7);
        }
    "#,
        "[9,9,1]",
    );
}

#[test]
fn test_node_compat_arguments_object_strict_mode_is_unmapped() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function(a) {
                "use strict";
                a = 9;
                return JSON.stringify([a, arguments[0], arguments.length]);
            };
            return probe(7);
        }
    "#,
        "[9,7,1]",
    );
}

#[test]
fn test_node_compat_arrow_inherits_outer_arguments_object() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function(a) {
                return (() => JSON.stringify([arguments[0], arguments === arguments]))();
            };
            return probe(7);
        }
    "#,
        "[7,true]",
    );
}

#[test]
fn test_node_compat_arguments_object_define_property_disconnects_mapping() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function(a) {
                Object.defineProperty(arguments, "0", {
                    configurable: false,
                    enumerable: false,
                    writable: false
                });
                a = 2;
                let desc = Object.getOwnPropertyDescriptor(arguments, "0");
                return JSON.stringify([
                    desc.value,
                    desc.writable,
                    desc.enumerable,
                    desc.configurable,
                    arguments[0],
                    a
                ]);
            };
            return probe(1);
        }
    "#,
        "[1,false,false,false,1,2]",
    );
}

#[test]
fn test_node_compat_arguments_object_has_symbol_iterator() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function() {
                "use strict";
                let desc = Object.getOwnPropertyDescriptor(arguments, Symbol.iterator);
                return JSON.stringify([
                    desc !== undefined,
                    desc.writable,
                    desc.enumerable,
                    desc.configurable,
                    arguments[Symbol.iterator] === [][Symbol.iterator]
                ]);
            };
            return probe();
        }
    "#,
        "[true,true,false,true,true]",
    );
}

#[test]
fn test_node_compat_delete_arguments_binding_returns_false() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function() {
                return JSON.stringify([delete arguments, typeof arguments]);
            };
            return probe();
        }
    "#,
        "[false,\"object\"]",
    );
}

#[test]
fn test_node_compat_arguments_caller_is_absent() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function() {
                return JSON.stringify([
                    Object.getOwnPropertyDescriptor(arguments, "caller") === undefined,
                    arguments.hasOwnProperty("caller")
                ]);
            };
            return probe();
        }
    "#,
        "[true,false]",
    );
}

#[test]
fn test_node_compat_arguments_callee_descriptor_hides_accessor_fields_in_sloppy_mode() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function() {
                let desc = Object.getOwnPropertyDescriptor(arguments, "callee");
                return JSON.stringify([
                    desc.writable,
                    desc.enumerable,
                    desc.configurable,
                    desc.hasOwnProperty("get"),
                    desc.hasOwnProperty("set")
                ]);
            };
            return probe();
        }
    "#,
        "[true,false,true,false,false]",
    );
}

#[test]
fn test_node_compat_descriptor_objects_hide_absent_fields_in_reflection() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function() {
                let desc = Object.getOwnPropertyDescriptor(arguments, "callee");
                return JSON.stringify([
                    Object.getOwnPropertyDescriptor(desc, "get") === undefined,
                    Object.getOwnPropertyDescriptor(desc, "set") === undefined,
                    Object.getOwnPropertyDescriptor(desc, "value") !== undefined
                ]);
            };
            return probe();
        }
    "#,
        "[true,true,true]",
    );
}

#[test]
fn test_node_compat_descriptor_objects_hide_absent_and_internal_names() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let probe = function() {
                let desc = Object.getOwnPropertyDescriptor(arguments, "callee");
                let names = Object.getOwnPropertyNames(desc);
                return JSON.stringify(names);
            };
            return probe();
        }
    "#,
        "[\"value\",\"writable\",\"configurable\",\"enumerable\"]",
    );
}

#[test]
fn test_node_compat_define_property_updates_existing_field_enumerability() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let o = { a: 1, b: 2 };
            Object.defineProperty(o, "b", { enumerable: false });
            let desc = Object.getOwnPropertyDescriptor(o, "b");
            return JSON.stringify([
                Object.keys(o),
                Object.getOwnPropertyNames(o),
                o.propertyIsEnumerable("a"),
                o.propertyIsEnumerable("b"),
                desc.enumerable
            ]);
        }
    "#,
        "[[\"a\"],[\"a\",\"b\"],true,false,false]",
    );
}

#[test]
fn test_node_compat_non_configurable_accessor_redefinition_preserves_descriptor() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let getter = function() { return 1; };
            let o = {};
            Object.defineProperty(o, "x", {
                get: getter,
                enumerable: true,
                configurable: false
            });
            try {
                Object.defineProperty(o, "x", {
                    set: function(v) {}
                });
                return "NO_THROW";
            } catch (e) {
                let d = Object.getOwnPropertyDescriptor(o, "x");
                return JSON.stringify([
                    e.name,
                    d.get === getter,
                    d.set === undefined,
                    d.enumerable,
                    d.configurable
                ]);
            }
        }
    "#,
        "[\"TypeError\",true,true,true,false]",
    );
}

#[test]
fn test_node_compat_error_instance_own_names_exclude_prototype_helpers() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            try {
                throw new TypeError("x");
            } catch (e) {
                return JSON.stringify([
                    Object.getOwnPropertyNames(e),
                    Object.keys(e),
                    Object.prototype.hasOwnProperty.call(e, "constructor"),
                    Object.prototype.hasOwnProperty.call(e, "valueOf")
                ]);
            }
            return "NO_THROW";
        }
    "#,
        "[[\"message\",\"name\",\"stack\",\"cause\",\"code\",\"errno\",\"syscall\",\"path\"],[\"message\",\"name\",\"stack\",\"cause\",\"code\",\"errno\",\"syscall\",\"path\"],false,false]",
    );
}

#[test]
fn test_node_compat_data_descriptors_hide_absent_accessor_fields() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            try {
                throw new TypeError("x");
            } catch (e) {
                let desc = Object.getOwnPropertyDescriptor(e, "name");
                return JSON.stringify([
                    Object.getOwnPropertyNames(desc),
                    Object.getOwnPropertyDescriptor(desc, "get") === undefined,
                    Object.getOwnPropertyDescriptor(desc, "set") === undefined
                ]);
            }
            return "NO_THROW";
        }
    "#,
        "[[\"value\",\"writable\",\"configurable\",\"enumerable\"],true,true]",
    );
}

#[test]
fn test_node_compat_function_virtual_keys_are_non_enumerable() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            function sample(a, b) {}
            return JSON.stringify([
                Object.getOwnPropertyNames(sample),
                Object.keys(sample)
            ]);
        }
    "#,
        "[[\"length\",\"name\",\"prototype\"],[]]",
    );
}

#[test]
fn test_node_compat_get_own_property_names_orders_indices_before_strings() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let o = { 2: "c", 1: "b", a: 1, 0: "a" };
            return JSON.stringify([
                Object.getOwnPropertyNames(o),
                Object.keys(o),
                (function() {
                    let out = [];
                    for (let k in o) out.push(k);
                    return out;
                })()
            ]);
        }
    "#,
        "[[\"0\",\"1\",\"2\",\"a\"],[\"0\",\"1\",\"2\",\"a\"],[\"0\",\"1\",\"2\",\"a\"]]",
    );
}

#[test]
fn test_node_compat_typed_array_own_names_hide_internal_state() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let ta = new Uint8Array([1, 2]);
            let seen = [];
            for (let k in ta) seen.push(k);
            return JSON.stringify([
                Object.getOwnPropertyNames(ta),
                Object.keys(ta),
                seen
            ]);
        }
    "#,
        "[[\"0\",\"1\"],[\"0\",\"1\"],[\"0\",\"1\"]]",
    );
}

#[test]
fn test_node_compat_arguments_own_names_exclude_symbol_iterator_string() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            function probe(a, b) {
                let seen = [];
                for (let k in arguments) seen.push(k);
                return JSON.stringify([
                    Object.getOwnPropertyNames(arguments),
                    Object.keys(arguments),
                    seen
                ]);
            }
            return probe(1, 2);
        }
    "#,
        "[[\"0\",\"1\",\"length\",\"callee\"],[\"0\",\"1\"],[\"0\",\"1\"]]",
    );
}

#[test]
fn test_node_compat_array_length_define_property_locks_writable_flag() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let a = [1, 2, 3];
            Object.defineProperty(a, "length", { value: 1, writable: false });
            let d = Object.getOwnPropertyDescriptor(a, "length");
            return JSON.stringify([a.length, a[1], d.writable]);
        }
    "#,
        "[1,null,false]",
    );
}

#[test]
fn test_node_compat_array_index_accessor_define_property_creates_real_property() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let a = [];
            Object.defineProperty(a, "2", {
                get: function() { return 9; },
                configurable: true
            });
            let d = Object.getOwnPropertyDescriptor(a, "2");
            return JSON.stringify([a.length, a[2], d.get !== undefined, d.set === undefined]);
        }
    "#,
        "[3,9,true,true]",
    );
}

#[test]
fn test_node_compat_reflect_set_uses_inherited_setter_on_receiver() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let hits: any[] = [];
            let proto = {};
            Object.defineProperty(proto, "x", {
                set: function(v) {
                    this._x = v;
                    hits.push(v);
                },
                configurable: true
            });
            let obj = {};
            Object.setPrototypeOf(obj, proto);
            let ok = Reflect.set(obj, "x", 7);
            return JSON.stringify([
                ok,
                hits.length,
                obj._x,
                Object.prototype.hasOwnProperty.call(obj, "x"),
                Object.prototype.hasOwnProperty.call(obj, "_x")
            ]);
        }
    "#,
        "[true,1,7,false,true]",
    );
}

#[test]
fn test_node_compat_reflect_set_respects_inherited_non_writable_data_property() {
    expect_string_runtime_node_compat(
        r#"
        function main(): string {
            let proto = {};
            Object.defineProperty(proto, "x", {
                value: 1,
                writable: false,
                configurable: true
            });
            let obj = {};
            Object.setPrototypeOf(obj, proto);
            let ok = Reflect.set(obj, "x", 7);
            return JSON.stringify([
                ok,
                obj.x,
                Object.prototype.hasOwnProperty.call(obj, "x")
            ]);
        }
    "#,
        "[false,1,false]",
    );
}

fn test_node_compat_top_level_strict_functions_inherit_arguments_poisoning() {
    expect_string_runtime_node_compat(
        r#"
        "use strict";
        function main(): string {
            let probe = function() {
                try {
                    let value = arguments.callee;
                    return JSON.stringify(["NO_THROW", value === undefined]);
                } catch (e) {
                    return JSON.stringify([e.constructor === TypeError, e.name]);
                }
            };
            return probe();
        }
    "#,
        "[true,\"TypeError\"]",
    );
}

#[test]
fn test_node_compat_sloppy_arguments_callee_matches_active_function_object() {
    expect_bool_runtime_node_compat(
        r#"
        function f() {
            return arguments.callee === f;
        }
        return f();
    "#,
        true,
    );
}

#[test]
fn test_node_compat_sloppy_arguments_callee_caller_is_observable() {
    expect_string_runtime_node_compat(
        r#"
        function outer(): string {
            return inner();
        }
        function inner(): string {
            return JSON.stringify(arguments.callee.caller === undefined);
        }
        return outer();
    "#,
        "true",
    );
}

#[test]
fn test_node_compat_sloppy_arguments_non_writable_assignment_does_not_throw() {
    expect_string_runtime_node_compat(
        r#"
        function fn(a): string {
            Object.defineProperty(arguments, "0", { writable: false });
            arguments[0] = 2;
            return JSON.stringify([a, arguments[0]]);
        }
        return fn(1);
    "#,
        "[1,1]",
    );
}

#[test]
fn test_node_compat_weakmap_basic_object_key_roundtrip() {
    expect_i32_runtime_node_compat(
        r#"
        let wm = new WeakMap<number>();
        let k = new Object();
        wm.set(k, 42);
        let v = wm.get(k);
        if (v == null) {
            return 0;
        }
        return v;
    "#,
        42,
    );
}

#[test]
fn test_node_compat_weakset_basic_identity_membership() {
    expect_bool_runtime_node_compat(
        r#"
        let ws = new WeakSet<Object>();
        let a = new Object();
        ws.add(a);
        return ws.has(a) && ws.delete(a) && !ws.has(a);
    "#,
        true,
    );
}

#[test]
fn test_node_compat_weakset_distinct_objects_do_not_alias() {
    expect_bool_runtime_node_compat(
        r#"
        let ws = new WeakSet<Object>();
        let a = new Object();
        let b = new Object();
        ws.add(a);
        return ws.has(a) && !ws.has(b);
    "#,
        true,
    );
}

#[test]
fn test_node_compat_weakref_deref_roundtrip() {
    expect_bool_runtime_node_compat(
        r#"
        let o = new Object();
        let wr = new WeakRef<Object>(o);
        return wr.deref() != null;
    "#,
        true,
    );
}

#[test]
fn test_node_compat_finalization_registry_unregister_with_token() {
    expect_bool_runtime_node_compat(
        r#"
        let reg = new FinalizationRegistry<string>((heldValue: string): void => {});
        let target = new Object();
        let token = new Object();
        reg.register(target, "held", token);
        return reg.unregister(token) && !reg.unregister(token);
    "#,
        true,
    );
}

#[test]
fn test_node_compat_finalization_registry_cleanup_some_callback() {
    expect_bool_runtime_node_compat(
        r#"
        let reg = new FinalizationRegistry<number>((heldValue: number): void => {
            let _x = heldValue;
        });
        let token = new Object();
        reg.register(new Object(), 15, token);
        reg.cleanupSome(null);
        return !reg.unregister(token);
    "#,
        true,
    );
}

#[test]
fn test_node_compat_finalization_registry_cleanup_some_override_callback() {
    expect_bool_runtime_node_compat(
        r#"
        let reg = new FinalizationRegistry<number>((heldValue: number): void => {
            let _x = heldValue;
        });
        let token = new Object();
        reg.register(new Object(), 3, token);
        reg.register(new Object(), 4, token);
        reg.cleanupSome((heldValue: number): void => {
            let _x = heldValue;
        });
        return !reg.unregister(token);
    "#,
        true,
    );
}

// ============================================================================
// EventEmitter tests
// ============================================================================

#[test]
fn test_event_emitter_on_and_emit() {
    expect_i32_with_builtins(
        r#"
        let emitter = new EventEmitter<{ tick: [number] }>();
        let total = 0;
        emitter.on("tick", (payload: number): void => {
            total = total + payload;
        });
        emitter.emit("tick", 10);
        emitter.emit("tick", 5);
        return total;
    "#,
        15,
    );
}

#[test]
fn test_event_emitter_once_and_listener_count() {
    expect_i32_with_builtins(
        r#"
        let emitter = new EventEmitter<{ tick: [number] }>();
        let total = 0;
        emitter.once("tick", (payload: number): void => {
            total = total + payload;
        });
        emitter.emit("tick", 7);
        emitter.emit("tick", 9);
        return total * 10 + emitter.listenerCount("tick");
    "#,
        70,
    );
}

#[test]
fn test_event_emitter_remove_all_listeners() {
    expect_bool_with_builtins(
        r#"
        let emitter = new EventEmitter<{ a: [number], b: [number] }>();
        emitter.on("a", (_: number): void => {});
        emitter.on("b", (_: number): void => {});
        emitter.removeAllListeners("a");
        emitter.removeAllListeners("b");
        return emitter.listenerCount("a") == 0 && emitter.listenerCount("b") == 0;
    "#,
        true,
    );
}
