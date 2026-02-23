//! Tests for optional parameters and default values in builtins

use super::{expect_i32_with_builtins, expect_string, expect_string_with_builtins};

// ============================================================================
// Array Methods
// ============================================================================

#[test]
fn test_array_slice_with_end() {
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        let sliced: number[] = arr.slice(1, 4);
        return sliced.join(",");
    "#,
        "2,3,4",
    );
}

#[test]
fn test_array_slice_without_end() {
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        let sliced: number[] = arr.slice(2);
        return sliced.join(",");
    "#,
        "3,4,5",
    );
}

#[test]
fn test_array_fill_all_params() {
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        arr.fill(9, 1, 4);
        return arr.join(",");
    "#,
        "1,9,9,9,5",
    );
}

#[test]
fn test_array_fill_no_range() {
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        arr.fill(7);
        return arr.join(",");
    "#,
        "7,7,7,7,7",
    );
}

#[test]
fn test_array_fill_with_start_only() {
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        arr.fill(8, 2);
        return arr.join(",");
    "#,
        "1,2,8,8,8",
    );
}

#[test]
fn test_array_splice_with_deletecount() {
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        let removed: number[] = arr.splice(1, 2);
        return removed.join(",") + ":" + arr.join(",");
    "#,
        "2,3:1,4,5",
    );
}

#[test]
fn test_array_splice_without_deletecount() {
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        let removed: number[] = arr.splice(2);
        return removed.join(",") + ":" + arr.join(",");
    "#,
        "3,4,5:1,2",
    );
}

// ============================================================================
// String Methods
// ============================================================================

#[test]
fn test_string_slice_with_end() {
    expect_string(
        r#"
        let str: string = "Hello, World!";
        return str.slice(0, 5);
    "#,
        "Hello",
    );
}

#[test]
fn test_string_slice_without_end() {
    expect_string(
        r#"
        let str: string = "Hello, World!";
        return str.slice(7);
    "#,
        "World!",
    );
}

#[test]
fn test_string_substring_with_end() {
    expect_string(
        r#"
        let str: string = "Hello, World!";
        return str.substring(7, 12);
    "#,
        "World",
    );
}

#[test]
fn test_string_substring_without_end() {
    expect_string(
        r#"
        let str: string = "Hello, World!";
        return str.substring(7);
    "#,
        "World!",
    );
}

#[test]
fn test_string_padstart_with_pad() {
    expect_string(
        r#"
        let str: string = "5";
        return str.padStart(3, "0");
    "#,
        "005",
    );
}

#[test]
fn test_string_padstart_default_pad() {
    expect_string(
        r#"
        let str: string = "hi";
        return str.padStart(5);
    "#,
        "   hi",
    );
}

#[test]
fn test_string_padend_with_pad() {
    expect_string(
        r#"
        let str: string = "5";
        return str.padEnd(3, "0");
    "#,
        "500",
    );
}

#[test]
fn test_string_padend_default_pad() {
    expect_string(
        r#"
        let str: string = "hi";
        return str.padEnd(5);
    "#,
        "hi   ",
    );
}

#[test]
fn test_string_repeat_with_count() {
    expect_string(
        r#"
        let str: string = "ab";
        return str.repeat(3);
    "#,
        "ababab",
    );
}

#[test]
fn test_string_repeat_default_count() {
    expect_string(
        r#"
        let str: string = "test";
        return str.repeat();
    "#,
        "test",
    );
}

// ============================================================================
// Number Methods
// ============================================================================

#[test]
fn test_number_tofixed_with_digits() {
    expect_string(
        r#"
        let num: number = 3.14159;
        return num.toFixed(2);
    "#,
        "3.14",
    );
}

#[test]
fn test_number_tofixed_default_digits() {
    expect_string(
        r#"
        let num: number = 3.14159;
        return num.toFixed();
    "#,
        "3",
    );
}

#[test]
fn test_number_toprecision_with_precision() {
    expect_string(
        r#"
        let num: number = 123.456;
        return num.toPrecision(4);
    "#,
        "123.5",
    );
}

#[test]
fn test_number_toprecision_no_precision() {
    expect_string(
        r#"
        let num: number = 123.456;
        return num.toPrecision();
    "#,
        "123.456",
    );
}

#[test]
fn test_number_tostring_with_radix() {
    expect_string(
        r#"
        let num: number = 255;
        return num.toString(16);
    "#,
        "ff",
    );
}

#[test]
fn test_number_tostring_default_radix() {
    expect_string(
        r#"
        let num: number = 42;
        return num.toString();
    "#,
        "42",
    );
}

// ============================================================================
// Buffer Methods
// ============================================================================

#[test]
fn test_buffer_slice_with_end() {
    expect_i32_with_builtins(
        r#"
        let buf: Buffer = new Buffer(10);
        buf.setByte(5, 42);
        let sliced: Buffer = buf.slice(5, 8);
        return sliced.getByte(0);
    "#,
        42,
    );
}

#[test]
fn test_buffer_slice_without_end() {
    expect_i32_with_builtins(
        r#"
        let buf: Buffer = new Buffer(10);
        buf.setByte(5, 99);
        let sliced: Buffer = buf.slice(5);
        return sliced.getByte(0);
    "#,
        99,
    );
}

#[test]
fn test_buffer_copy_all_params() {
    expect_i32_with_builtins(
        r#"
        let src: Buffer = new Buffer(5);
        src.setByte(2, 55);
        let dest: Buffer = new Buffer(5);
        src.copy(dest, 1, 2, 3);
        return dest.getByte(1);
    "#,
        55,
    );
}

#[test]
fn test_buffer_copy_defaults() {
    expect_i32_with_builtins(
        r#"
        let src: Buffer = new Buffer(5);
        src.setByte(0, 77);
        let dest: Buffer = new Buffer(5);
        src.copy(dest);
        return dest.getByte(0);
    "#,
        77,
    );
}

#[test]
fn test_buffer_tostring_with_encoding() {
    expect_string_with_builtins(
        r#"
        let buf: Buffer = new Buffer(5);
        buf.setByte(0, 72);  // 'H'
        buf.setByte(1, 105); // 'i'
        buf.setByte(2, 0);
        buf.setByte(3, 0);
        buf.setByte(4, 0);
        return buf.slice(0, 2).toString("utf8");
    "#,
        "Hi",
    );
}

#[test]
fn test_buffer_tostring_default_encoding() {
    expect_string_with_builtins(
        r#"
        let buf: Buffer = new Buffer(5);
        buf.setByte(0, 72);  // 'H'
        buf.setByte(1, 105); // 'i'
        buf.setByte(2, 0);
        buf.setByte(3, 0);
        buf.setByte(4, 0);
        return buf.slice(0, 2).toString();
    "#,
        "Hi",
    );
}

// ============================================================================
// Compress Module (stdlib) - Removed, needs separate stdlib testing
// ============================================================================

// ============================================================================
// Edge Cases & Backward Compatibility
// ============================================================================

#[test]
fn test_array_slice_negative_start() {
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        let sliced: number[] = arr.slice(-2);
        return sliced.join(",");
    "#,
        "4,5",
    );
}

#[test]
fn test_string_slice_negative_indices() {
    expect_string(
        r#"
        let str: string = "Hello";
        return str.slice(-3);
    "#,
        "llo",
    );
}

#[test]
fn test_backward_compatibility_all_args_still_work() {
    // Ensure that calling with all arguments still works
    expect_string(
        r#"
        let arr: number[] = [1, 2, 3, 4, 5];
        let sliced1: number[] = arr.slice(1, 3);
        let str: string = "test";
        let sliced2: string = str.slice(0, 2);
        let num: number = 3.14;
        let fixed: string = num.toFixed(1);
        return sliced1.join(",") + ":" + sliced2 + ":" + fixed;
    "#,
        "2,3:te:3.1",
    );
}
