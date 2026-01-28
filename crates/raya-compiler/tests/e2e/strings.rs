//! Phase 13: String Operations tests
//!
//! Tests for string literals, operations, and methods.

use super::harness::*;

// ============================================================================
// String Literals
// ============================================================================

#[test]
fn test_string_literal_double_quotes() {
    expect_string("return \"hello\";", "hello");
}

#[test]
fn test_string_literal_single_quotes() {
    expect_string("return 'hello';", "hello");
}

#[test]
fn test_string_empty() {
    expect_string("return \"\";", "");
}

// ============================================================================
// String Escape Sequences
// ============================================================================

#[test]
fn test_string_escape_newline() {
    expect_string("return \"hello\\nworld\";", "hello\nworld");
}

#[test]
fn test_string_escape_tab() {
    expect_string("return \"hello\\tworld\";", "hello\tworld");
}

#[test]
fn test_string_escape_backslash() {
    expect_string("return \"a\\\\b\";", "a\\b");
}

#[test]
fn test_string_escape_quote() {
    expect_string("return \"say \\\"hello\\\"\";", "say \"hello\"");
}

// ============================================================================
// String Concatenation
// ============================================================================

#[test]
fn test_string_concatenation() {
    expect_string("return \"hello\" + \" \" + \"world\";", "hello world");
}

#[test]
fn test_string_concat_with_variable() {
    expect_string("let s = \"hello\"; return s + \" world\";", "hello world");
}

// ============================================================================
// String Comparison
// ============================================================================

#[test]
fn test_string_equality() {
    expect_bool("return \"abc\" == \"abc\";", true);
}

#[test]
fn test_string_inequality() {
    expect_bool("return \"abc\" != \"def\";", true);
}

#[test]
fn test_string_less_than() {
    expect_bool("return \"abc\" < \"abd\";", true);
}

#[test]
fn test_string_greater_than() {
    expect_bool("return \"xyz\" > \"abc\";", true);
}

// ============================================================================
// String Length
// ============================================================================

#[test]
fn test_string_length() {
    expect_i32("return \"hello\".length;", 5);
}

#[test]
fn test_string_length_empty() {
    expect_i32("return \"\".length;", 0);
}

// ============================================================================
// Template Strings
// ============================================================================

#[test]
fn test_template_string_simple() {
    expect_string("return `hello world`;", "hello world");
}

#[test]
fn test_template_string_interpolation() {
    expect_string("let name = \"World\"; return `Hello, ${name}!`;", "Hello, World!");
}

#[test]
fn test_template_string_expression() {
    expect_string("let x = 5; return `result: ${x + 3}`;", "result: 8");
}

#[test]
fn test_template_string_multiline() {
    expect_string("return `line1\nline2`;", "line1\nline2");
}

// ============================================================================
// String Methods
// ============================================================================

#[test]
fn test_string_to_uppercase() {
    expect_string("return \"hello\".toUpperCase();", "HELLO");
}

#[test]
fn test_string_to_lowercase() {
    expect_string("return \"HELLO\".toLowerCase();", "hello");
}

#[test]
fn test_string_trim() {
    expect_string("return \"  hello  \".trim();", "hello");
}

#[test]
fn test_string_char_at() {
    expect_string("return \"hello\".charAt(1);", "e");
}

#[test]
fn test_string_substring() {
    expect_string("return \"hello world\".substring(0, 5);", "hello");
}

#[test]
fn test_string_index_of() {
    expect_i32("return \"hello world\".indexOf(\"world\");", 6);
}

#[test]
fn test_string_index_of_not_found() {
    expect_i32("return \"hello\".indexOf(\"xyz\");", -1);
}

#[test]
fn test_string_includes() {
    expect_bool("return \"hello world\".includes(\"world\");", true);
}

#[test]
fn test_string_includes_false() {
    expect_bool("return \"hello\".includes(\"xyz\");", false);
}

#[test]
fn test_string_starts_with() {
    expect_bool("return \"hello world\".startsWith(\"hello\");", true);
}

#[test]
fn test_string_starts_with_false() {
    expect_bool("return \"hello world\".startsWith(\"world\");", false);
}

#[test]
fn test_string_ends_with() {
    expect_bool("return \"hello world\".endsWith(\"world\");", true);
}

#[test]
fn test_string_ends_with_false() {
    expect_bool("return \"hello world\".endsWith(\"hello\");", false);
}

#[test]
fn test_string_char_code_at() {
    expect_i32("return \"ABC\".charCodeAt(0);", 65); // 'A' = 65
}

#[test]
fn test_string_char_code_at_middle() {
    expect_i32("return \"hello\".charCodeAt(1);", 101); // 'e' = 101
}

#[test]
fn test_string_last_index_of() {
    expect_i32("return \"hello hello\".lastIndexOf(\"hello\");", 6);
}

#[test]
fn test_string_last_index_of_not_found() {
    expect_i32("return \"hello\".lastIndexOf(\"xyz\");", -1);
}

#[test]
fn test_string_trim_start() {
    expect_string("return \"  hello  \".trimStart();", "hello  ");
}

#[test]
fn test_string_trim_end() {
    expect_string("return \"  hello  \".trimEnd();", "  hello");
}

#[test]
fn test_string_pad_start() {
    expect_string("return \"5\".padStart(3, \"0\");", "005");
}

#[test]
fn test_string_pad_start_no_padding_needed() {
    expect_string("return \"hello\".padStart(3, \"0\");", "hello");
}

#[test]
fn test_string_pad_end() {
    expect_string("return \"5\".padEnd(3, \"0\");", "500");
}

#[test]
fn test_string_pad_end_no_padding_needed() {
    expect_string("return \"hello\".padEnd(3, \"0\");", "hello");
}
