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

use super::harness::{expect_i32_with_builtins, expect_bool_with_builtins};

// ============================================================================
// Map tests
// ============================================================================

#[test]
fn test_map_new_and_size() {
    expect_i32_with_builtins(r#"
        let map = new Map<string, number>();
        return map.size();
    "#, 0);
}

// Minimal test: just get and return directly
#[test]
fn test_map_get_simple() {
    expect_i32_with_builtins(r#"
        let map = new Map<string, number>();
        map.set("a", 10);
        let a = map.get("a");
        return 0;
    "#, 0);
}

// Test null comparison
#[test]
fn test_map_get_null_check() {
    expect_bool_with_builtins(r#"
        let map = new Map<string, number>();
        map.set("a", 10);
        let a = map.get("a");
        return a != null;
    "#, true);
}

#[test]
fn test_map_set_and_get() {
    expect_i32_with_builtins(r#"
        let map = new Map<string, number>();
        map.set("a", 10);
        map.set("b", 20);
        let a = map.get("a");
        if (a != null) {
            return a;
        }
        return 0;
    "#, 10);
}

#[test]
fn test_map_has() {
    expect_bool_with_builtins(r#"
        let map = new Map<string, number>();
        map.set("key", 42);
        return map.has("key");
    "#, true);
}

#[test]
fn test_map_has_missing() {
    expect_bool_with_builtins(r#"
        let map = new Map<string, number>();
        map.set("key", 42);
        return map.has("other");
    "#, false);
}

#[test]
fn test_map_delete() {
    expect_bool_with_builtins(r#"
        let map = new Map<string, number>();
        map.set("key", 42);
        let deleted = map.delete("key");
        return deleted && !map.has("key");
    "#, true);
}

#[test]
fn test_map_clear() {
    expect_i32_with_builtins(r#"
        let map = new Map<string, number>();
        map.set("a", 1);
        map.set("b", 2);
        map.clear();
        return map.size();
    "#, 0);
}

// ============================================================================
// Set tests
// ============================================================================

#[test]
fn test_set_new_and_size() {
    expect_i32_with_builtins(r#"
        let set = new Set<number>();
        return set.size();
    "#, 0);
}

#[test]
fn test_set_add_and_has() {
    expect_bool_with_builtins(r#"
        let set = new Set<number>();
        set.add(42);
        return set.has(42);
    "#, true);
}

#[test]
fn test_set_add_unique() {
    expect_i32_with_builtins(r#"
        let set = new Set<number>();
        set.add(1);
        set.add(2);
        set.add(1);
        return set.size();
    "#, 2);
}

#[test]
fn test_set_delete() {
    expect_bool_with_builtins(r#"
        let set = new Set<number>();
        set.add(42);
        let deleted = set.delete(42);
        return deleted && !set.has(42);
    "#, true);
}

#[test]
fn test_set_clear() {
    expect_i32_with_builtins(r#"
        let set = new Set<number>();
        set.add(1);
        set.add(2);
        set.add(3);
        set.clear();
        return set.size();
    "#, 0);
}

// ============================================================================
// Buffer tests
// ============================================================================

#[test]
fn test_buffer_new_and_length() {
    expect_i32_with_builtins(r#"
        let buf = new Buffer(16);
        return buf.length();
    "#, 16);
}

#[test]
fn test_buffer_get_set_byte() {
    expect_i32_with_builtins(r#"
        let buf = new Buffer(4);
        buf.setByte(0, 42);
        buf.setByte(1, 100);
        return buf.getByte(0) + buf.getByte(1);
    "#, 142);
}

#[test]
fn test_buffer_get_set_int32() {
    expect_i32_with_builtins(r#"
        let buf = new Buffer(8);
        buf.setInt32(0, 12345);
        return buf.getInt32(0);
    "#, 12345);
}

#[test]
fn test_buffer_get_set_float64() {
    expect_bool_with_builtins(r#"
        let buf = new Buffer(16);
        buf.setFloat64(0, 3.14159);
        let val = buf.getFloat64(0);
        return val > 3.14 && val < 3.15;
    "#, true);
}

// ============================================================================
// Date tests
// ============================================================================

#[test]
fn test_date_new() {
    // Date.new returns current timestamp, should be > 0
    expect_bool_with_builtins(r#"
        let date = new Date();
        return date.getTime() > 0;
    "#, true);
}

#[test]
fn test_date_get_components() {
    // Date should have reasonable year (2020+)
    expect_bool_with_builtins(r#"
        let date = new Date();
        return date.getFullYear() >= 2020;
    "#, true);
}

#[test]
fn test_date_get_month() {
    // Month should be 0-11
    expect_bool_with_builtins(r#"
        let date = new Date();
        let month = date.getMonth();
        return month >= 0 && month <= 11;
    "#, true);
}

#[test]
fn test_date_get_day() {
    // Day should be 1-31
    expect_bool_with_builtins(r#"
        let date = new Date();
        let day = date.getDate();
        return day >= 1 && day <= 31;
    "#, true);
}

// ============================================================================
// Channel tests
// ============================================================================

#[test]
fn test_channel_new() {
    expect_i32_with_builtins(r#"
        let ch = new Channel<number>(10);
        return ch.capacity();
    "#, 10);
}

#[test]
fn test_channel_send_receive() {
    expect_i32_with_builtins(r#"
        let ch = new Channel<number>(1);
        ch.send(42);
        return ch.receive();
    "#, 42);
}

#[test]
fn test_channel_length() {
    expect_i32_with_builtins(r#"
        let ch = new Channel<number>(10);
        ch.send(1);
        ch.send(2);
        ch.send(3);
        return ch.length();
    "#, 3);
}

#[test]
fn test_channel_try_send() {
    expect_bool_with_builtins(r#"
        let ch = new Channel<number>(1);
        let sent = ch.trySend(42);
        return sent;
    "#, true);
}

#[test]
fn test_channel_close() {
    expect_bool_with_builtins(r#"
        let ch = new Channel<number>(1);
        ch.close();
        return ch.isClosed();
    "#, true);
}

// ============================================================================
// Integration tests - combining multiple builtins
// ============================================================================

#[test]
fn test_map_with_multiple_operations() {
    expect_i32_with_builtins(r#"
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
    "#, 15);
}

#[test]
fn test_set_operations() {
    expect_i32_with_builtins(r#"
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
        return set.size();
    "#, 2);
}
