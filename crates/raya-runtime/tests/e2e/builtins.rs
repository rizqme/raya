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

use super::harness::{expect_i32_with_builtins, expect_bool_with_builtins, expect_string_with_builtins, expect_i32, expect_bool, expect_string};

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

// ============================================================================
// RegExp tests (primitive type - uses regular expect_* functions)
// ============================================================================

#[test]
fn test_regexp_test_basic_match() {
    expect_bool(r#"
        let re = new RegExp("hello", "");
        return re.test("hello world");
    "#, true);
}

#[test]
fn test_regexp_test_no_match() {
    expect_bool(r#"
        let re = new RegExp("xyz", "");
        return re.test("hello world");
    "#, false);
}

#[test]
fn test_regexp_test_case_sensitive() {
    expect_bool(r#"
        let re = new RegExp("HELLO", "");
        return re.test("hello world");
    "#, false);
}

#[test]
fn test_regexp_test_case_insensitive() {
    expect_bool(r#"
        let re = new RegExp("HELLO", "i");
        return re.test("hello world");
    "#, true);
}

#[test]
fn test_regexp_exec_match() {
    // exec returns [matched_text, index, ...groups] or null
    expect_i32(r#"
        let re = new RegExp("world", "");
        let result = re.exec("hello world");
        if (result == null) {
            return -1;
        }
        return result[1];
    "#, 6);
}

#[test]
fn test_regexp_exec_no_match() {
    expect_bool(r#"
        let re = new RegExp("xyz", "");
        let result = re.exec("hello world");
        return result == null;
    "#, true);
}

#[test]
fn test_regexp_exec_matched_text() {
    expect_string(r#"
        let re = new RegExp("wor..", "");
        let result = re.exec("hello world");
        if (result == null) {
            return "";
        }
        return result[0];
    "#, "world");
}

#[test]
fn test_regexp_exec_all_basic() {
    expect_i32(r#"
        let re = new RegExp("l", "g");
        let results = re.execAll("hello");
        return results.length;
    "#, 2);
}

#[test]
fn test_regexp_replace_simple() {
    expect_string(r#"
        let re = new RegExp("world", "");
        return re.replace("hello world", "universe");
    "#, "hello universe");
}

#[test]
fn test_regexp_replace_global() {
    expect_string(r#"
        let re = new RegExp("l", "g");
        return re.replace("hello", "L");
    "#, "heLLo");
}

#[test]
fn test_regexp_replace_no_match() {
    expect_string(r#"
        let re = new RegExp("xyz", "");
        return re.replace("hello", "world");
    "#, "hello");
}

#[test]
fn test_regexp_split_basic() {
    expect_i32(r#"
        let re = new RegExp(",", "");
        let parts = re.split("a,b,c", 0);
        return parts.length;
    "#, 3);
}

#[test]
fn test_regexp_split_content() {
    expect_string(r#"
        let re = new RegExp(",", "");
        let parts = re.split("a,b,c", 0);
        return parts[1];
    "#, "b");
}

#[test]
fn test_regexp_split_with_limit() {
    expect_i32(r#"
        let re = new RegExp(",", "");
        let parts = re.split("a,b,c,d,e", 3);
        return parts.length;
    "#, 3);
}

#[test]
fn test_regexp_stateless() {
    // RegExp is stateless - same result on repeated calls
    expect_bool(r#"
        let re = new RegExp("test", "");
        let r1 = re.test("test string");
        let r2 = re.test("test string");
        let r3 = re.test("test string");
        return r1 && r2 && r3;
    "#, true);
}

#[test]
fn test_regexp_with_special_chars() {
    // Test regex special characters
    expect_bool(r#"
        let re = new RegExp("a.b", "");
        return re.test("a*b");
    "#, true);
}

#[test]
fn test_regexp_digit_pattern() {
    expect_bool(r#"
        let re = new RegExp("[0-9]+", "");
        return re.test("abc123def");
    "#, true);
}

#[test]
fn test_regexp_multiline_flag() {
    expect_bool(r#"
        let re = new RegExp("^test", "m");
        return re.test("first line\ntest line");
    "#, true);
}

// ============================================================================
// String + RegExp method tests
// ============================================================================

#[test]
fn test_string_match_returns_array() {
    // match without global flag returns first match
    expect_bool(r#"
        let re = new RegExp("l+", "");
        let result = "hello".match(re);
        return result != null;
    "#, true);
}

#[test]
fn test_string_match_matched_text() {
    expect_string(r#"
        let re = new RegExp("l+", "");
        let result = "hello".match(re);
        if (result == null) {
            return "";
        }
        return result[0];
    "#, "ll");
}

#[test]
fn test_string_match_no_match() {
    expect_bool(r#"
        let re = new RegExp("xyz", "");
        let result = "hello".match(re);
        return result == null;
    "#, true);
}

#[test]
fn test_string_match_global_returns_all() {
    expect_i32(r#"
        let re = new RegExp("l", "g");
        let result = "hello world".match(re);
        if (result == null) {
            return 0;
        }
        return result.length;
    "#, 3);
}

#[test]
fn test_string_match_global_content() {
    expect_string(r#"
        let re = new RegExp("o", "g");
        let result = "hello world".match(re);
        if (result == null) {
            return "";
        }
        return result[0];
    "#, "o");
}

#[test]
fn test_string_match_all_basic() {
    expect_i32(r#"
        let re = new RegExp("l", "g");
        let results = "hello".matchAll(re);
        return results.length;
    "#, 2);
}

#[test]
fn test_string_match_all_with_index() {
    // matchAll returns array of [match, index, ...groups]
    expect_i32(r#"
        let re = new RegExp("o", "g");
        let results = "hello world".matchAll(re);
        // First match at index 4
        return results[0][1];
    "#, 4);
}

#[test]
fn test_string_search_found() {
    expect_i32(r#"
        let re = new RegExp("world", "");
        return "hello world".search(re);
    "#, 6);
}

#[test]
fn test_string_search_not_found() {
    expect_i32(r#"
        let re = new RegExp("xyz", "");
        return "hello world".search(re);
    "#, -1);
}

#[test]
fn test_string_search_pattern() {
    expect_i32(r#"
        let re = new RegExp("[0-9]+", "");
        return "abc123def".search(re);
    "#, 3);
}

#[test]
fn test_string_replace_regexp() {
    expect_string(r#"
        let re = new RegExp("world", "");
        return "hello world".replace(re, "universe");
    "#, "hello universe");
}

#[test]
fn test_string_replace_regexp_global() {
    expect_string(r#"
        let re = new RegExp("l", "g");
        return "hello".replace(re, "L");
    "#, "heLLo");
}

#[test]
fn test_string_replace_regexp_no_match() {
    expect_string(r#"
        let re = new RegExp("xyz", "");
        return "hello".replace(re, "world");
    "#, "hello");
}

#[test]
fn test_string_replace_regexp_pattern() {
    expect_string(r#"
        let re = new RegExp("[0-9]+", "");
        return "abc123def".replace(re, "X");
    "#, "abcXdef");
}

// Test string split with string separator (now requires 2 args: separator, limit)
#[test]
fn test_string_split_string() {
    expect_i32(r#"
        let parts = "a,b,c".split(",", 0);
        return parts.length;
    "#, 3);
}

#[test]
fn test_string_split_string_content() {
    expect_string(r#"
        let parts = "a,b,c".split(",", 0);
        return parts[1];
    "#, "b");
}

#[test]
fn test_string_split_string_with_limit() {
    expect_i32(r#"
        let parts = "a,b,c,d,e".split(",", 3);
        return parts.length;
    "#, 3);
}

#[test]
fn test_string_split_regexp() {
    expect_i32(r#"
        let re = new RegExp(",", "");
        let parts = "a,b,c".split(re, 0);
        return parts.length;
    "#, 3);
}

#[test]
fn test_string_split_regexp_content() {
    expect_string(r#"
        let re = new RegExp(",", "");
        let parts = "a,b,c".split(re, 0);
        return parts[1];
    "#, "b");
}

#[test]
fn test_string_split_regexp_with_limit() {
    expect_i32(r#"
        let re = new RegExp(",", "");
        let parts = "a,b,c,d,e".split(re, 3);
        return parts.length;
    "#, 3);
}

#[test]
fn test_string_split_regexp_pattern() {
    // Split on whitespace pattern
    expect_i32(r#"
        let re = new RegExp("\\s+", "");
        let parts = "hello   world   test".split(re, 0);
        return parts.length;
    "#, 3);
}

#[test]
fn test_string_split_regexp_pattern_content() {
    expect_string(r#"
        let re = new RegExp("\\s+", "");
        let parts = "hello   world   test".split(re, 0);
        return parts[1];
    "#, "world");
}

// ============================================================================
// String replaceWith (callback-based replacement)
// ============================================================================

#[test]
fn test_string_replace_with_simple() {
    // Replace match with constant string
    expect_string(r#"
        let re = new RegExp("world", "");
        let result = "hello world".replaceWith(re, (match: (string | number)[]): string => {
            return "UNIVERSE";
        });
        return result;
    "#, "hello UNIVERSE");
}

#[test]
fn test_string_replace_with_global() {
    // Replace all matches globally
    expect_string(r#"
        let re = new RegExp("l", "g");
        let result = "hello".replaceWith(re, (match: (string | number)[]): string => {
            return "L";
        });
        return result;
    "#, "heLLo");
}

#[test]
fn test_string_replace_with_no_match() {
    // No match - return original string
    expect_string(r#"
        let re = new RegExp("xyz", "");
        let result = "hello".replaceWith(re, (match: (string | number)[]): string => {
            return "REPLACED";
        });
        return result;
    "#, "hello");
}

#[test]
fn test_string_replace_with_pattern() {
    // Replace digits with X
    expect_string(r#"
        let re = new RegExp("[0-9]+", "g");
        let result = "a1b22c333".replaceWith(re, (match: (string | number)[]): string => {
            return "X";
        });
        return result;
    "#, "aXbXcX");
}

#[test]
fn test_string_replace_with_non_global() {
    // Without global flag, replace only first match
    expect_string(r#"
        let re = new RegExp("o", "");
        let result = "foo bar boo".replaceWith(re, (match: (string | number)[]): string => {
            return "O";
        });
        return result;
    "#, "fOo bar boo");
}

// ============================================================================
// Native Call Basic Test
// ============================================================================

#[test]
fn test_native_call_basic() {
    // Test that native calls work correctly through string methods
    // String.charAt calls NATIVE_CALL(0x0200) internally
    expect_string(r#"
        let s = "hello";
        return s.charAt(1);
    "#, "e");
}

// ============================================================================
// RegExpMatch Class Tests
// ============================================================================

#[test]
fn test_regexp_match_properties() {
    // Test that exec returns an array with [matched_text, index, ...groups]
    // The match text should be at index 0
    expect_string(r#"
        let re = new RegExp("world", "");
        let result = re.exec("hello world");
        if (result == null) {
            return "";
        }
        return result[0];
    "#, "world");
}

#[test]
fn test_regexp_match_index() {
    // Test that the match index is correct (index 1 of result array)
    expect_i32(r#"
        let re = new RegExp("world", "");
        let result = re.exec("hello world");
        if (result == null) {
            return -1;
        }
        return result[1];
    "#, 6);
}

#[test]
fn test_regexp_match_groups() {
    // Test capture groups - pattern with groups returns captured content
    expect_string(r#"
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
    "#, "user");
}

#[test]
fn test_regexp_match_groups_second() {
    // Test second capture group
    expect_string(r#"
        let re = new RegExp("(\\w+)@(\\w+)", "");
        let result = re.exec("email: user@domain");
        if (result == null) {
            return "";
        }
        // result[3] = second group "domain"
        return result[3];
    "#, "domain");
}

// ============================================================================
// Date getter tests (new handlers)
// ============================================================================

#[test]
fn test_date_get_hours() {
    // Epoch (setTime(0)) = Jan 1, 1970 00:00:00 UTC
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        return date.getHours();
    "#, 0);
}

#[test]
fn test_date_get_minutes() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        return date.getMinutes();
    "#, 0);
}

#[test]
fn test_date_get_seconds() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        return date.getSeconds();
    "#, 0);
}

#[test]
fn test_date_get_milliseconds() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        return date.getMilliseconds();
    "#, 0);
}

#[test]
fn test_date_get_hours_nonzero() {
    // 3600000ms = 1 hour from epoch
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(3600000);
        return date.getHours();
    "#, 1);
}

#[test]
fn test_date_get_minutes_nonzero() {
    // 900000ms = 15 minutes from epoch
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(900000);
        return date.getMinutes();
    "#, 15);
}

#[test]
fn test_date_get_seconds_nonzero() {
    // 45000ms = 45 seconds from epoch
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(45000);
        return date.getSeconds();
    "#, 45);
}

#[test]
fn test_date_get_milliseconds_nonzero() {
    // 123ms from epoch
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(123);
        return date.getMilliseconds();
    "#, 123);
}

// ============================================================================
// Date setter tests
// ============================================================================

#[test]
fn test_date_set_full_year() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        date.setFullYear(2024);
        return date.getFullYear();
    "#, 2024);
}

#[test]
fn test_date_set_month() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        date.setMonth(5);
        return date.getMonth();
    "#, 5);
}

#[test]
fn test_date_set_date() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        date.setDate(15);
        return date.getDate();
    "#, 15);
}

#[test]
fn test_date_set_hours() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        date.setHours(14);
        return date.getHours();
    "#, 14);
}

#[test]
fn test_date_set_minutes() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        date.setMinutes(30);
        return date.getMinutes();
    "#, 30);
}

#[test]
fn test_date_set_seconds() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        date.setSeconds(45);
        return date.getSeconds();
    "#, 45);
}

#[test]
fn test_date_set_milliseconds() {
    expect_i32_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        date.setMilliseconds(999);
        return date.getMilliseconds();
    "#, 999);
}

// ============================================================================
// Date formatting tests
// ============================================================================

#[test]
fn test_date_to_iso_string() {
    expect_string_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        return date.toISOString();
    "#, "1970-01-01T00:00:00.000Z");
}

#[test]
fn test_date_to_date_string() {
    expect_string_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        return date.toDateString();
    "#, "Thu Jan 01 1970");
}

#[test]
fn test_date_to_time_string() {
    expect_string_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        return date.toTimeString();
    "#, "00:00:00");
}

#[test]
fn test_date_to_string() {
    expect_string_with_builtins(r#"
        let date = new Date();
        date.setTime(0);
        return date.toString();
    "#, "Thu Jan 01 1970 00:00:00");
}

// Note: Date.parse tests require adding a static parse() method to date.raya
// The VM handler (DATE_PARSE) exists but the class method is not yet defined

// ============================================================================
// Object tests
// ============================================================================

#[test]
fn test_object_hash_code() {
    // hashCode should return an integer
    expect_bool_with_builtins(r#"
        let obj = new Object();
        let hash = obj.hashCode();
        return hash == hash;
    "#, true);
}

#[test]
fn test_object_equals_same() {
    // An object should equal itself
    expect_bool_with_builtins(r#"
        let obj = new Object();
        return obj.equals(obj);
    "#, true);
}

#[test]
fn test_object_to_string() {
    expect_string_with_builtins(r#"
        let obj = new Object();
        return obj.toString();
    "#, "[object Object]");
}

// ============================================================================
// Number method tests
// ============================================================================

#[test]
fn test_number_to_fixed_zero() {
    expect_string(r#"
        let x: number = 3.14159;
        return x.toFixed(0);
    "#, "3");
}

#[test]
fn test_number_to_fixed_two() {
    expect_string(r#"
        let x: number = 3.14159;
        return x.toFixed(2);
    "#, "3.14");
}

#[test]
fn test_number_to_fixed_four() {
    expect_string(r#"
        let x: number = 3.14159;
        return x.toFixed(4);
    "#, "3.1416");
}

#[test]
fn test_number_to_precision() {
    expect_string(r#"
        let x: number = 123.456;
        return x.toPrecision(5);
    "#, "123.46");
}

#[test]
fn test_number_to_precision_one() {
    expect_string(r#"
        let x: number = 123.456;
        return x.toPrecision(1);
    "#, "100");
}

#[test]
fn test_number_to_string_decimal() {
    expect_string(r#"
        let x: number = 255;
        return x.toString(10);
    "#, "255");
}

#[test]
fn test_number_to_string_binary() {
    expect_string(r#"
        let x: number = 255;
        return x.toString(2);
    "#, "11111111");
}

#[test]
fn test_number_to_string_hex() {
    expect_string(r#"
        let x: number = 255;
        return x.toString(16);
    "#, "ff");
}

#[test]
fn test_number_to_string_octal() {
    expect_string(r#"
        let x: number = 255;
        return x.toString(8);
    "#, "377");
}

// ============================================================================
// Map.keys / Map.values / Map.entries
// ============================================================================

#[test]
fn test_map_keys() {
    expect_i32_with_builtins(r#"
        let m = new Map<string, number>();
        m.set("a", 1);
        m.set("b", 2);
        m.set("c", 3);
        let keys = m.keys();
        return keys.length;
    "#, 3);
}

#[test]
fn test_map_keys_transform() {
    // Same pattern as the failing test_collection_map_transform
    expect_i32_with_builtins(r#"
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

        return result + inverse.size();
    "#, 114);
}

#[test]
fn test_map_values() {
    expect_i32_with_builtins(r#"
        let m = new Map<string, number>();
        m.set("x", 10);
        m.set("y", 20);
        let vals = m.values();
        let sum = 0;
        for (let i = 0; i < vals.length; i = i + 1) {
            sum = sum + vals[i];
        }
        return sum;
    "#, 30);
}

// ============================================================================
// Set.values / Set.union / Set.intersection / Set.difference
// ============================================================================

#[test]
fn test_set_values() {
    expect_i32_with_builtins(r#"
        let s = new Set<number>();
        s.add(10);
        s.add(20);
        s.add(30);
        let vals = s.values();
        return vals.length;
    "#, 3);
}

#[test]
fn test_set_intersection() {
    expect_i32_with_builtins(r#"
        let setA = new Set<number>();
        setA.add(1); setA.add(2); setA.add(3); setA.add(4); setA.add(5);

        let setB = new Set<number>();
        setB.add(3); setB.add(4); setB.add(5); setB.add(6); setB.add(7);

        let inter = setA.intersection(setB);
        return inter.size();
    "#, 3);
}

#[test]
fn test_set_union() {
    expect_i32_with_builtins(r#"
        let setA = new Set<number>();
        setA.add(1); setA.add(2); setA.add(3);

        let setB = new Set<number>();
        setB.add(3); setB.add(4); setB.add(5);

        let uni = setA.union(setB);
        return uni.size();
    "#, 5);
}

#[test]
fn test_set_difference() {
    expect_i32_with_builtins(r#"
        let setA = new Set<number>();
        setA.add(1); setA.add(2); setA.add(3); setA.add(4);

        let setB = new Set<number>();
        setB.add(2); setB.add(4);

        let diff = setA.difference(setB);
        return diff.size();
    "#, 2);
}

#[test]
fn test_set_operations_combined() {
    // Same pattern as the failing test_collection_set_operations
    expect_i32_with_builtins(r#"
        let setA = new Set<number>();
        setA.add(1); setA.add(2); setA.add(3); setA.add(4); setA.add(5);

        let setB = new Set<number>();
        setB.add(3); setB.add(4); setB.add(5); setB.add(6); setB.add(7);

        let inter = setA.intersection(setB);
        let uni = setA.union(setB);

        return inter.size() * 10 + uni.size();
    "#, 37);
}

#[test]
fn test_set_deduplication() {
    // Same pattern as the failing test_collection_deduplication
    expect_i32_with_builtins(r#"
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
    "#, 155);
}

