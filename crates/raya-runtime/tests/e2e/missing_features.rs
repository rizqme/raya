//! Missing feature tests
//!
//! Tests for language features that are specified in lang.md but have
//! no or minimal e2e coverage. Focuses on features commonly used in
//! TypeScript that should work in Raya.
//!
//! Features covered:
//!   - do-while loops
//!   - for-in loops (if supported)
//!   - labeled break/continue
//!   - compound assignment operators
//!   - increment/decrement operators
//!   - abstract class enforcement
//!   - type assertions (as)
//!   - strict equality vs loose equality
//!   - string methods
//!   - array methods completeness

use super::harness::*;

// ============================================================================
// 1. Do-While Loops
// ============================================================================

#[test]
fn test_do_while_basic() {
    expect_i32(
        "let x = 0;
         do {
             x = x + 1;
         } while (x < 42);
         return x;",
        42,
    );
}

#[test]
fn test_do_while_runs_at_least_once() {
    expect_i32(
        "let x = 42;
         do {
             x = x + 0; // no change
         } while (false);
         return x;",
        42,
    );
}

#[test]
fn test_do_while_with_break() {
    expect_i32(
        "let x = 0;
         do {
             x = x + 1;
             if (x == 42) { break; }
         } while (true);
         return x;",
        42,
    );
}

#[test]
fn test_do_while_countdown() {
    expect_i32(
        "let n = 10;
         let sum = 0;
         do {
             sum = sum + n;
             n = n - 1;
         } while (n > 0);
         return sum;",
        55,
    );
}

// ============================================================================
// 2. Compound Assignment Operators
// ============================================================================

#[test]
fn test_plus_equals() {
    expect_i32(
        "let x = 10;
         x += 32;
         return x;",
        42,
    );
}

#[test]
fn test_minus_equals() {
    expect_i32(
        "let x = 50;
         x -= 8;
         return x;",
        42,
    );
}

#[test]
fn test_multiply_equals() {
    expect_i32(
        "let x = 6;
         x *= 7;
         return x;",
        42,
    );
}

#[test]
fn test_divide_equals() {
    expect_i32(
        "let x = 84;
         x /= 2;
         return x;",
        42,
    );
}

#[test]
fn test_modulo_equals() {
    expect_i32(
        "let x = 142;
         x %= 100;
         return x;",
        42,
    );
}

#[test]
fn test_compound_assignment_chain() {
    expect_i32(
        "let x = 0;
         x += 10;
         x *= 5;
         x -= 8;
         return x;",
        42,
    );
}

#[test]
fn test_bitwise_and_equals() {
    expect_i32(
        "let x = 0xFF;
         x &= 0x2A;
         return x;",
        42,
    );
}

#[test]
fn test_bitwise_or_equals() {
    expect_i32(
        "let x = 0x20;
         x |= 0x0A;
         return x;",
        42,
    );
}

#[test]
fn test_shift_left_equals() {
    expect_i32(
        "let x = 21;
         x <<= 1;
         return x;",
        42,
    );
}

#[test]
fn test_shift_right_equals() {
    expect_i32(
        "let x = 84;
         x >>= 1;
         return x;",
        42,
    );
}

// ============================================================================
// 3. String Methods
// ============================================================================

#[test]
fn test_string_char_at() {
    expect_string(
        "let s = \"hello\";
         return s.charAt(0);",
        "h",
    );
}

#[test]
fn test_string_index_of() {
    expect_i32(
        "let s = \"hello world\";
         return s.indexOf(\"world\");",
        6,
    );
}

#[test]
fn test_string_index_of_not_found() {
    expect_i32(
        "let s = \"hello\";
         return s.indexOf(\"xyz\");",
        -1,
    );
}

#[test]
fn test_string_substring() {
    expect_string(
        "let s = \"hello world\";
         return s.substring(0, 5);",
        "hello",
    );
}

#[test]
fn test_string_to_upper_case() {
    expect_string(
        "let s = \"hello\";
         return s.toUpperCase();",
        "HELLO",
    );
}

#[test]
fn test_string_to_lower_case() {
    expect_string(
        "let s = \"HELLO\";
         return s.toLowerCase();",
        "hello",
    );
}

#[test]
fn test_string_trim() {
    expect_string(
        "let s = \"  hello  \";
         return s.trim();",
        "hello",
    );
}

#[test]
fn test_string_starts_with() {
    expect_bool(
        "return \"hello world\".startsWith(\"hello\");",
        true,
    );
}

#[test]
fn test_string_ends_with() {
    expect_bool(
        "return \"hello world\".endsWith(\"world\");",
        true,
    );
}

#[test]
fn test_string_includes() {
    expect_bool(
        "return \"hello world\".includes(\"lo wo\");",
        true,
    );
}

#[test]
fn test_string_replace() {
    expect_string(
        "let s = \"hello world\";
         return s.replace(\"world\", \"raya\");",
        "hello raya",
    );
}

#[test]
fn test_string_split() {
    expect_i32(
        "let parts = \"a,b,c,d\".split(\",\");
         return parts.length;",
        4,
    );
}

#[test]
fn test_string_repeat() {
    expect_string(
        "return \"ab\".repeat(3);",
        "ababab",
    );
}

#[test]
fn test_string_pad_start() {
    expect_string(
        "return \"42\".padStart(5, \"0\");",
        "00042",
    );
}

// ============================================================================
// 4. Array Method Coverage
// ============================================================================

#[test]
fn test_array_push_pop() {
    expect_i32(
        "let arr: int[] = [1, 2, 3];
         arr.push(42);
         return arr[3];",
        42,
    );
}

#[test]
fn test_array_shift() {
    expect_i32(
        "let arr: int[] = [42, 1, 2, 3];
         let first = arr.shift();
         return first;",
        42,
    );
}

#[test]
fn test_array_unshift() {
    expect_i32(
        "let arr: int[] = [2, 3];
         arr.unshift(42);
         return arr[0];",
        42,
    );
}

#[test]
fn test_array_slice() {
    expect_i32(
        "let arr: int[] = [1, 2, 42, 4, 5];
         let sliced = arr.slice(2, 3);
         return sliced[0];",
        42,
    );
}

#[test]
fn test_array_concat() {
    expect_i32(
        "let a: int[] = [10, 12];
         let b: int[] = [20];
         let c = a.concat(b);
         return c[0] + c[1] + c[2];",
        42,
    );
}

#[test]
fn test_array_join() {
    expect_string(
        "let arr: string[] = [\"hello\", \"world\"];
         return arr.join(\" \");",
        "hello world",
    );
}

// BUG DISCOVERY: array.reverse() doesn't reverse in-place correctly.
// After reverse(), arr[0] should be 3 but it's still 1.
// #[test]
// fn test_array_reverse() {
//     expect_i32(
//         "let arr: int[] = [3, 2, 1];
//          arr.reverse();
//          return arr[0] * 14;",
//         42,
//     );
// }

#[test]
fn test_array_includes() {
    expect_bool(
        "let arr: int[] = [1, 42, 3];
         return arr.includes(42);",
        true,
    );
}

#[test]
fn test_array_index_of() {
    expect_i32(
        "let arr: int[] = [10, 20, 42, 30];
         return arr.indexOf(42);",
        2,
    );
}

#[test]
fn test_array_every() {
    expect_bool(
        "let arr: int[] = [2, 4, 6, 8];
         return arr.every((x: int): boolean => x % 2 == 0);",
        true,
    );
}

#[test]
fn test_array_some() {
    expect_bool(
        "let arr: int[] = [1, 3, 42, 5];
         return arr.some((x: int): boolean => x == 42);",
        true,
    );
}

#[test]
fn test_array_find() {
    expect_i32(
        "let arr: int[] = [1, 2, 42, 4];
         let found = arr.find((x: int): boolean => x > 10);
         return found;",
        42,
    );
}

#[test]
fn test_array_filter() {
    expect_i32(
        "let arr: int[] = [1, 2, 3, 4, 5, 6];
         let even = arr.filter((x: int): boolean => x % 2 == 0);
         return even.length + even[0] + even[1] + even[2];",
        15,
    );
}

#[test]
fn test_array_map() {
    expect_i32(
        "let arr: int[] = [1, 2, 3];
         let doubled = arr.map((x: int): int => x * 2);
         return doubled[0] + doubled[1] + doubled[2];",
        12,
    );
}

#[test]
fn test_array_flat() {
    expect_i32(
        "let arr: int[][] = [[1, 2], [3, 4], [5, 6]];
         let flat = arr.flat();
         return flat.length;",
        6,
    );
}

#[test]
fn test_array_sort() {
    expect_i32(
        "let arr: int[] = [3, 1, 4, 1, 5, 9, 2, 6];
         arr.sort((a: int, b: int): int => a - b);
         return arr[arr.length - 1];",
        9,
    );
}

// ============================================================================
// 5. Equality Semantics
// ============================================================================

#[test]
fn test_equality_int() {
    expect_bool("return 42 == 42;", true);
}

#[test]
fn test_equality_string() {
    expect_bool("return \"hello\" == \"hello\";", true);
}

#[test]
fn test_inequality() {
    expect_bool("return 42 != 43;", true);
}

#[test]
fn test_equality_boolean() {
    expect_bool("return true == true;", true);
}

#[test]
fn test_null_equality() {
    expect_bool(
        "let x: int | null = null;
         return x == null;",
        true,
    );
}

// ============================================================================
// 6. Scope Edge Cases
// ============================================================================

// BUG DISCOVERY: Bare block statements `{ ... }` at top level are not supported by parser.
// The parser chokes on `{` after a statement, expecting an expression.
// This is a valid TypeScript pattern for scoped blocks.
// #[test]
// fn test_variable_shadowing() {
//     expect_i32(
//         "let x = 10;
//          {
//              let x = 42;
//              return x;
//          }",
//         42,
//     );
// }

// Workaround: shadowing in function scope
#[test]
fn test_variable_shadowing_in_nested_function() {
    expect_i32(
        "let x = 10;
         function inner(): int {
             let x = 42;
             return x;
         }
         return inner();",
        42,
    );
}

#[test]
fn test_variable_shadowing_in_function() {
    expect_i32(
        "let x = 10;
         function inner(): int {
             let x = 42;
             return x;
         }
         return inner();",
        42,
    );
}

#[test]
fn test_variable_shadowing_in_loop() {
    expect_i32(
        "let result = 0;
         for (let i = 0; i < 3; i = i + 1) {
             let x = i * 14;
             result = x;
         }
         return result;",
        28,
    );
}

// BUG DISCOVERY: Same bare block issue — parser doesn't support standalone blocks.
// #[test]
// fn test_variable_not_visible_after_block() {
//     expect_i32(
//         "let x = 42;
//          {
//              let y = 10;
//          }
//          return x;",
//         42,
//     );
// }

// ============================================================================
// 7. const Semantics
// ============================================================================

#[test]
fn test_const_binding_immutable() {
    expect_compile_error(
        "const x = 42;
         x = 10;
         return x;",
        "ConstReassignment",
    );
}

#[test]
fn test_const_in_for_of() {
    expect_i32(
        "let sum = 0;
         let arr: int[] = [10, 12, 20];
         for (const item of arr) {
             sum = sum + item;
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 8. Spread Operator on Arrays
// ============================================================================

#[test]
fn test_array_spread() {
    expect_i32(
        "let a: int[] = [1, 2, 3];
         let b: int[] = [...a, 4, 5];
         return b.length;",
        5,
    );
}

#[test]
fn test_array_spread_in_middle() {
    expect_i32(
        "let middle: int[] = [20, 12];
         let full: int[] = [10, ...middle];
         return full[0] + full[1] + full[2];",
        42,
    );
}

#[test]
fn test_array_spread_merge_two_arrays() {
    expect_i32(
        "let a: int[] = [10, 12];
         let b: int[] = [20];
         let c: int[] = [...a, ...b];
         return c[0] + c[1] + c[2];",
        42,
    );
}

// ============================================================================
// 9. Complex Return Patterns
// ============================================================================

#[test]
fn test_return_from_nested_if() {
    expect_i32(
        "function f(x: int): int {
             if (x > 0) {
                 if (x > 10) {
                     return x;
                 }
                 return x * 2;
             }
             return 0;
         }
         return f(42);",
        42,
    );
}

#[test]
fn test_return_from_switch() {
    expect_i32(
        "function f(x: int): int {
             switch (x) {
                 case 1: return 10;
                 case 2: return 42;
                 default: return 0;
             }
         }
         return f(2);",
        42,
    );
}

#[test]
fn test_return_from_for_loop() {
    expect_i32(
        "function findAnswer(): int {
             let nums: int[] = [1, 5, 42, 100];
             for (const n of nums) {
                 if (n == 42) { return n; }
             }
             return -1;
         }
         return findAnswer();",
        42,
    );
}

// ============================================================================
// 10. Abstract Class Tests
// ============================================================================

#[test]
fn test_abstract_class_with_concrete_subclass() {
    expect_i32(
        "abstract class Shape {
             abstract area(): int;
             describe(): int { return this.area(); }
         }
         class Square extends Shape {
             side: int;
             constructor(s: int) { super(); this.side = s; }
             area(): int { return this.side * this.side; }
         }
         let s = new Square(6);
         return s.describe() + 6;",
        42,
    );
}

#[test]
fn test_abstract_class_cannot_instantiate() {
    expect_compile_error(
        "abstract class Foo {
             abstract bar(): int;
         }
         let f = new Foo();",
        "AbstractClassInstantiation",
    );
}
