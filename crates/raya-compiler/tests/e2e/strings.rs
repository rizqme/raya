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
