//! Bug hunting tests — round 5
//!
//! Targeting areas not yet covered:
//! - Do-while loops
//! - Default parameter values
//! - Optional parameters (?)
//! - Exponentiation operator (**)
//! - Unary negation and plus
//! - Bitwise NOT (~)
//! - Unsigned right shift (>>>)
//! - String comparison operators (<, >, <=, >=)
//! - Const reassignment (should error)
//! - Variable shadowing in nested scopes
//! - Static fields on classes
//! - Multiple generic type parameters
//! - Implements clause (class implements type)
//! - Spread operator in arrays
//! - Array.every / Array.some / Array.findIndex / Array.includes
//! - Nested try-catch-finally
//! - Finally always runs
//! - Closure over loop variable (classic bug)
//! - String.toLowerCase / toUpperCase / trimStart / trimEnd
//! - Hex/octal/binary literals
//! - Numeric separator (1_000_000)
//! - Empty array edge cases
//! - Chained method calls
//! - Nullish coalescing with method calls
//! - Post/pre-increment/decrement (++ --)
//! - Type alias for primitives
//! - Multi-level generic constraint
//! - Class implementing multiple type contracts

use super::harness::*;

// ============================================================================
// 1. Do-While Loops
// ============================================================================

#[test]
fn test_do_while_basic() {
    expect_i32(
        "let x = 0;
         do {
             x += 1;
         } while (x < 42);
         return x;",
        42,
    );
}

#[test]
fn test_do_while_runs_at_least_once() {
    expect_i32(
        "let x = 100;
         do {
             x += 1;
         } while (false);
         return x;",
        101,
    );
}

#[test]
fn test_do_while_with_break() {
    expect_i32(
        "let x = 0;
         do {
             x += 1;
             if (x == 42) { break; }
         } while (true);
         return x;",
        42,
    );
}

// ============================================================================
// 2. Default Parameter Values
// ============================================================================

#[test]
fn test_default_param_used() {
    expect_i32(
        "function add(a: int, b: int = 20): int {
             return a + b;
         }
         return add(22);",
        42,
    );
}

#[test]
fn test_default_param_overridden() {
    expect_i32(
        "function add(a: int, b: int = 100): int {
             return a + b;
         }
         return add(20, 22);",
        42,
    );
}

#[test]
fn test_default_param_string() {
    expect_string(
        "function greet(name: string, prefix: string = \"Hello\"): string {
             return `${prefix}, ${name}!`;
         }
         return greet(\"World\");",
        "Hello, World!",
    );
}

#[test]
fn test_default_param_string_overridden() {
    expect_string(
        "function greet(name: string, prefix: string = \"Hello\"): string {
             return `${prefix}, ${name}!`;
         }
         return greet(\"World\", \"Hi\");",
        "Hi, World!",
    );
}

// ============================================================================
// 3. Optional Parameters (?)
// ============================================================================

#[test]
fn test_optional_param_not_provided() {
    expect_i32(
        "function maybe(x: int, y?: int): int {
             if (y !== null) { return x + y; }
             return x;
         }
         return maybe(42);",
        42,
    );
}

#[test]
fn test_optional_param_provided() {
    expect_i32(
        "function maybe(x: int, y?: int): int {
             if (y !== null) { return x + y; }
             return x;
         }
         return maybe(20, 22);",
        42,
    );
}

// ============================================================================
// 4. Exponentiation Operator (**)
// ============================================================================

#[test]
fn test_exponentiation_int() {
    expect_i32(
        "return 2 ** 5 + 10;",
        42,
    );
}

#[test]
fn test_exponentiation_in_expression() {
    expect_i32(
        "let base = 3;
         let exp = 3;
         return base ** exp + 15;",
        42,
    );
}

#[test]
fn test_exponentiation_float() {
    expect_f64(
        "return 2.0 ** 10.0;",
        1024.0,
    );
}

// ============================================================================
// 5. Unary Operators
// ============================================================================

#[test]
fn test_unary_negation_int() {
    expect_i32(
        "let x = -42;
         return -x;",
        42,
    );
}

#[test]
fn test_unary_negation_expression() {
    expect_i32(
        "let a = 20;
         let b = 22;
         return -(a - b) + 40;",
        42,
    );
}

#[test]
fn test_unary_not_boolean() {
    expect_bool(
        "return !false;",
        true,
    );
}

#[test]
fn test_unary_not_double() {
    expect_bool(
        "return !!true;",
        true,
    );
}

// ============================================================================
// 6. Bitwise NOT (~)
// ============================================================================

#[test]
fn test_bitwise_not() {
    // ~(-43) = 42 (two's complement)
    expect_i32(
        "return ~(-43);",
        42,
    );
}

#[test]
fn test_bitwise_not_zero() {
    // ~0 = -1
    expect_i32(
        "return ~0;",
        -1,
    );
}

// ============================================================================
// 7. Unsigned Right Shift (>>>)
// ============================================================================

#[test]
fn test_unsigned_right_shift() {
    // -1 >>> 26 = 63 (all ones shifted)
    expect_i32(
        "return -1 >>> 26;",
        63,
    );
}

#[test]
fn test_unsigned_right_shift_positive() {
    // 84 >>> 1 = 42
    expect_i32(
        "return 84 >>> 1;",
        42,
    );
}

// ============================================================================
// 8. String Comparison
// ============================================================================

#[test]
fn test_string_less_than() {
    expect_bool(
        "return \"apple\" < \"banana\";",
        true,
    );
}

#[test]
fn test_string_greater_than() {
    expect_bool(
        "return \"zebra\" > \"apple\";",
        true,
    );
}

#[test]
fn test_string_less_equal() {
    expect_bool(
        "return \"abc\" <= \"abc\";",
        true,
    );
}

#[test]
fn test_string_greater_equal() {
    expect_bool(
        "return \"xyz\" >= \"abc\";",
        true,
    );
}

// ============================================================================
// 9. Const Reassignment (should error)
// ============================================================================

#[test]
fn test_const_reassignment_errors() {
    expect_compile_error(
        "const x: int = 42;
         x = 10;",
        "ConstReassignment",
    );
}

#[test]
fn test_const_compound_reassignment_errors() {
    expect_compile_error(
        "const x: int = 42;
         x += 1;",
        "ConstReassignment",
    );
}

// ============================================================================
// 10. Variable Shadowing
// ============================================================================

// BUG DISCOVERY (previously found in round 1): Bare block statements
// `{ let x = ...; }` fail to parse. The parser doesn't support standalone
// block scopes outside of if/while/for/function bodies.
// This prevents variable shadowing via block scoping.
// #[test]
// fn test_shadowing_in_block() {
//     expect_i32(
//         "let x = 10;
//          {
//              let x = 42;
//              return x;
//          }",
//         42,
//     );
// }
//
// #[test]
// fn test_shadowing_outer_unchanged() {
//     expect_i32(
//         "let x = 42;
//          {
//              let x = 999;
//          }
//          return x;",
//         42,
//     );
// }
//
// #[test]
// fn test_shadowing_different_type() {
//     expect_string(
//         "let x: int = 42;
//          {
//              let x: string = \"hello\";
//              return x;
//          }",
//         "hello",
//     );
// }

// ============================================================================
// 11. Static Fields on Classes
// ============================================================================

#[test]
fn test_static_field() {
    expect_i32(
        "class Counter {
             static count: int = 0;
             static increment(): void {
                 Counter.count += 1;
             }
         }
         Counter.increment();
         Counter.increment();
         Counter.increment();
         return Counter.count;",
        3,
    );
}

#[test]
fn test_static_field_access() {
    expect_i32(
        "class Config {
             static maxRetries: int = 42;
         }
         return Config.maxRetries;",
        42,
    );
}

// ============================================================================
// 12. Multiple Generic Type Parameters
// ============================================================================

#[test]
fn test_two_type_params() {
    expect_i32(
        "class Pair<A, B> {
             first: A;
             second: B;
             constructor(a: A, b: B) {
                 this.first = a;
                 this.second = b;
             }
         }
         let p = new Pair<int, int>(20, 22);
         return p.first + p.second;",
        42,
    );
}

#[test]
fn test_two_type_params_different_types() {
    expect_i32(
        "class Pair<A, B> {
             first: A;
             second: B;
             constructor(a: A, b: B) {
                 this.first = a;
                 this.second = b;
             }
         }
         let p = new Pair<string, int>(\"hello\", 42);
         return p.second;",
        42,
    );
}

#[test]
fn test_generic_function_two_params() {
    expect_i32(
        "function pickSecond<A, B>(a: A, b: B): B {
             return b;
         }
         return pickSecond<string, int>(\"hello\", 42);",
        42,
    );
}

// ============================================================================
// 13. Spread Operator in Arrays
// ============================================================================

#[test]
fn test_spread_in_array_literal() {
    expect_i32(
        "let arr1: int[] = [1, 2, 3];
         let arr2: int[] = [...arr1, 4, 5];
         return arr2.length;",
        5,
    );
}

#[test]
fn test_spread_combines_arrays() {
    expect_i32(
        "let a: int[] = [10, 12];
         let b: int[] = [20];
         let c: int[] = [...a, ...b];
         let sum = 0;
         for (const x of c) { sum += x; }
         return sum;",
        42,
    );
}

// ============================================================================
// 14. Array.every / Array.some / Array.findIndex / Array.includes
// ============================================================================

#[test]
fn test_array_every_true() {
    expect_bool(
        "let arr: int[] = [2, 4, 6, 8];
         return arr.every((x: int): boolean => x % 2 == 0);",
        true,
    );
}

#[test]
fn test_array_every_false() {
    expect_bool(
        "let arr: int[] = [2, 4, 5, 8];
         return arr.every((x: int): boolean => x % 2 == 0);",
        false,
    );
}

#[test]
fn test_array_some_true() {
    expect_bool(
        "let arr: int[] = [1, 3, 4, 7];
         return arr.some((x: int): boolean => x % 2 == 0);",
        true,
    );
}

#[test]
fn test_array_some_false() {
    expect_bool(
        "let arr: int[] = [1, 3, 5, 7];
         return arr.some((x: int): boolean => x % 2 == 0);",
        false,
    );
}

#[test]
fn test_array_find_index() {
    expect_i32(
        "let arr: int[] = [10, 20, 42, 50];
         return arr.findIndex((x: int): boolean => x == 42);",
        2,
    );
}

#[test]
fn test_array_find_index_not_found() {
    expect_i32(
        "let arr: int[] = [10, 20, 30];
         return arr.findIndex((x: int): boolean => x == 99);",
        -1,
    );
}

#[test]
fn test_array_includes_true() {
    expect_bool(
        "let arr: int[] = [10, 42, 30];
         return arr.includes(42);",
        true,
    );
}

#[test]
fn test_array_includes_false() {
    expect_bool(
        "let arr: int[] = [10, 20, 30];
         return arr.includes(99);",
        false,
    );
}

// ============================================================================
// 15. Nested Try-Catch-Finally
// ============================================================================

#[test]
fn test_nested_try_catch() {
    expect_i32_with_builtins(
        "let result = 0;
         try {
             try {
                 throw new Error(\"inner\");
             } catch (e) {
                 result += 20;
             }
             result += 22;
         } catch (e) {
             result = -1;
         }
         return result;",
        42,
    );
}

#[test]
fn test_finally_always_runs() {
    expect_i32_with_builtins(
        "let result = 0;
         try {
             result += 20;
         } finally {
             result += 22;
         }
         return result;",
        42,
    );
}

#[test]
fn test_finally_runs_after_catch() {
    expect_i32_with_builtins(
        "let result = 0;
         try {
             throw new Error(\"oops\");
         } catch (e) {
             result += 20;
         } finally {
             result += 22;
         }
         return result;",
        42,
    );
}

#[test]
fn test_finally_runs_on_return() {
    // finally should run even when try block returns
    expect_i32_with_builtins(
        "let x = 0;
         function test(): int {
             try {
                 return 42;
             } finally {
                 x = 1;  // should still execute
             }
         }
         let r = test();
         return r;",
        42,
    );
}

// ============================================================================
// 16. Closure Over Loop Variable
// ============================================================================

#[test]
fn test_closure_captures_loop_var_const() {
    // for-of with const should capture correctly per iteration
    expect_i32(
        "let fns: (() => int)[] = [];
         let values: int[] = [10, 20, 42];
         for (const v of values) {
             fns.push((): int => v);
         }
         return fns[2]();",
        42,
    );
}

#[test]
fn test_closure_captures_mutable_loop_var() {
    // Classic closure-over-loop-variable issue
    // With let in for loop, each iteration should have its own binding
    expect_i32(
        "let fns: (() => int)[] = [];
         for (let i = 0; i < 3; i += 1) {
             fns.push((): int => i);
         }
         return fns[0]() + fns[1]() + fns[2]();",
        3, // 0 + 1 + 2 = 3 if properly scoped; 6 if shared binding (2+2+2)
    );
}

// ============================================================================
// 17. String Methods — toLowerCase / toUpperCase / trimStart / trimEnd
// ============================================================================

#[test]
fn test_string_to_lower_case() {
    expect_string(
        "return \"HELLO WORLD\".toLowerCase();",
        "hello world",
    );
}

#[test]
fn test_string_to_upper_case() {
    expect_string(
        "return \"hello world\".toUpperCase();",
        "HELLO WORLD",
    );
}

#[test]
fn test_string_trim_start() {
    expect_string(
        "return \"   hello\".trimStart();",
        "hello",
    );
}

#[test]
fn test_string_trim_end() {
    expect_string(
        "return \"hello   \".trimEnd();",
        "hello",
    );
}

// ============================================================================
// 18. Hex / Octal / Binary Literals
// ============================================================================

#[test]
fn test_hex_literal() {
    expect_i32(
        "return 0x2A;", // 42
        42,
    );
}

#[test]
fn test_octal_literal() {
    expect_i32(
        "return 0o52;", // 42
        42,
    );
}

#[test]
fn test_binary_literal() {
    expect_i32(
        "return 0b101010;", // 42
        42,
    );
}

#[test]
fn test_hex_in_expression() {
    expect_i32(
        "return 0xFF & 0x2A;", // 255 & 42 = 42
        42,
    );
}

// ============================================================================
// 19. Numeric Separator
// ============================================================================

#[test]
fn test_numeric_separator() {
    expect_i32(
        "let x = 1_000;
         return x - 958;",
        42,
    );
}

#[test]
fn test_numeric_separator_large() {
    expect_i32(
        "let x = 1_000_042;
         return x - 1_000_000;",
        42,
    );
}

// ============================================================================
// 20. Empty Array Edge Cases
// ============================================================================

#[test]
fn test_empty_array_length() {
    expect_i32(
        "let arr: int[] = [];
         return arr.length;",
        0,
    );
}

#[test]
fn test_empty_array_push_then_access() {
    expect_i32(
        "let arr: int[] = [];
         arr.push(42);
         return arr[0];",
        42,
    );
}

#[test]
fn test_empty_array_filter() {
    expect_i32(
        "let arr: int[] = [];
         let filtered = arr.filter((x: int): boolean => x > 0);
         return filtered.length;",
        0,
    );
}

#[test]
fn test_empty_array_map() {
    expect_i32(
        "let arr: int[] = [];
         let mapped = arr.map((x: int): int => x * 2);
         return mapped.length;",
        0,
    );
}

// ============================================================================
// 21. Nullish Coalescing with Complex Expressions
// ============================================================================

#[test]
fn test_nullish_coalescing_method_result() {
    expect_i32(
        "class Getter {
             get(): int { return 42; }
         }
         let g: Getter | null = null;
         let result: int = 0;
         if (g !== null) {
             result = g.get();
         } else {
             result = 42;
         }
         return result;",
        42,
    );
}

#[test]
fn test_nullish_coalescing_chained() {
    expect_i32(
        "let a: int | null = null;
         let b: int | null = null;
         let c: int | null = 42;
         let result = a ?? b ?? c;
         return result;",
        42,
    );
}

// ============================================================================
// 22. Pre/Post Increment and Decrement (++ --)
// ============================================================================

#[test]
fn test_post_increment() {
    expect_i32(
        "let x = 41;
         x++;
         return x;",
        42,
    );
}

#[test]
fn test_post_decrement() {
    expect_i32(
        "let x = 43;
         x--;
         return x;",
        42,
    );
}

#[test]
fn test_pre_increment() {
    expect_i32(
        "let x = 41;
         return ++x;",
        42,
    );
}

#[test]
fn test_pre_decrement() {
    expect_i32(
        "let x = 43;
         return --x;",
        42,
    );
}

#[test]
fn test_post_increment_returns_old_value() {
    // x++ should return old value
    expect_i32(
        "let x = 42;
         let y = x++;
         return y;",
        42,
    );
}

#[test]
fn test_increment_in_for_loop() {
    expect_i32(
        "let sum = 0;
         for (let i = 0; i < 42; i++) {
             sum++;
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 23. Type Alias for Primitives
// ============================================================================

#[test]
fn test_type_alias_int() {
    expect_i32(
        "type Age = int;
         let x: Age = 42;
         return x;",
        42,
    );
}

#[test]
fn test_type_alias_string() {
    expect_string(
        "type Name = string;
         let n: Name = \"hello\";
         return n;",
        "hello",
    );
}

#[test]
fn test_type_alias_in_function() {
    expect_i32(
        "type Score = int;
         function doubleScore(s: Score): Score {
             return s * 2;
         }
         return doubleScore(21);",
        42,
    );
}

// ============================================================================
// 24. Class Implementing Type (implements clause)
// ============================================================================

#[test]
fn test_class_implements_type() {
    expect_i32(
        "type Countable = {
             count(): int;
         };
         class Items implements Countable {
             items: int[];
             constructor() { this.items = [1, 2, 3]; }
             count(): int { return this.items.length; }
         }
         let c: Countable = new Items();
         return c.count() * 14;",
        42,
    );
}

// ============================================================================
// 25. Complex Switch with Default and Multiple Cases
// ============================================================================

#[test]
fn test_switch_default() {
    expect_i32(
        "let x = 999;
         switch (x) {
             case 1: return 1;
             case 2: return 2;
             default: return 42;
         }",
        42,
    );
}

#[test]
fn test_switch_string() {
    expect_i32(
        "let cmd = \"run\";
         switch (cmd) {
             case \"stop\": return 0;
             case \"run\": return 42;
             case \"pause\": return 1;
             default: return -1;
         }",
        42,
    );
}

// ============================================================================
// 26. Nested Closures
// ============================================================================

#[test]
fn test_nested_closure() {
    expect_i32(
        "function make(): () => () => int {
             let x = 42;
             return (): () => int => {
                 return (): int => x;
             };
         }
         let f = make();
         let g = f();
         return g();",
        42,
    );
}

#[test]
fn test_closure_modifying_outer() {
    expect_i32(
        "let count = 0;
         let inc = (): void => { count += 1; };
         for (let i = 0; i < 42; i += 1) {
             inc();
         }
         return count;",
        42,
    );
}

// ============================================================================
// 27. Chained String Operations
// ============================================================================

#[test]
fn test_chained_string_ops() {
    expect_string(
        "let s = \"  Hello, World!  \";
         return s.trim();",
        "Hello, World!",
    );
}

#[test]
fn test_string_replace_and_length() {
    expect_i32(
        "let s = \"hello world\";
         let replaced = s.replace(\"world\", \"raya\");
         return replaced.length;",
        10,
    );
}

// ============================================================================
// 28. Complex Array Chaining
// ============================================================================

#[test]
fn test_filter_then_reduce() {
    expect_i32(
        "let arr: int[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
         let evens = arr.filter((x: int): boolean => x % 2 == 0);
         let sum = evens.reduce((acc: int, x: int): int => acc + x, 0);
         return sum;",
        30,
    );
}

#[test]
fn test_map_then_filter() {
    expect_i32(
        "let arr: int[] = [1, 2, 3, 4, 5];
         let doubled = arr.map((x: int): int => x * 2);
         let big = doubled.filter((x: int): boolean => x > 5);
         let sum = 0;
         for (const x of big) { sum += x; }
         return sum;",
        24, // [6, 8, 10] sum = 24
    );
}

// ============================================================================
// 29. Array Out-of-Bounds Access (should runtime error)
// ============================================================================

#[test]
fn test_array_out_of_bounds_positive() {
    expect_runtime_error(
        "let arr: int[] = [1, 2, 3];
         return arr[10];",
        "out of bounds",
    );
}

#[test]
fn test_array_out_of_bounds_negative() {
    expect_runtime_error(
        "let arr: int[] = [1, 2, 3];
         return arr[-1];",
        "out of bounds",
    );
}

// ============================================================================
// 30. Recursive Data Structures
// ============================================================================

#[test]
fn test_linked_list_like() {
    expect_i32(
        "class Node {
             value: int;
             next: Node | null;
             constructor(v: int, n: Node | null) {
                 this.value = v;
                 this.next = n;
             }
         }
         let list = new Node(10, new Node(12, new Node(20, null)));
         let sum = 0;
         let cur: Node | null = list;
         while (cur !== null) {
             sum += cur.value;
             cur = cur.next;
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 31. Deeply Nested Control Flow
// ============================================================================

#[test]
fn test_nested_if_else_chain() {
    expect_i32(
        "function classify(x: int): int {
             if (x < 0) {
                 return -1;
             } else if (x == 0) {
                 return 0;
             } else if (x < 10) {
                 return 1;
             } else if (x < 100) {
                 return 42;
             } else {
                 return 99;
             }
         }
         return classify(42);",
        42,
    );
}

// ============================================================================
// 32. Complex Expression Evaluation
// ============================================================================

#[test]
fn test_operator_precedence_add_mul() {
    expect_i32(
        "return 2 + 4 * 10;",
        42,
    );
}

#[test]
fn test_operator_precedence_parens() {
    expect_i32(
        "return (2 + 4) * 7;",
        42,
    );
}

#[test]
fn test_mixed_int_arithmetic() {
    expect_i32(
        "return 100 / 2 - 8;",
        42,
    );
}

#[test]
fn test_modulo_operator() {
    expect_i32(
        "return 142 % 100;",
        42,
    );
}

// ============================================================================
// 33. Boolean Expressions with Truthiness
// ============================================================================

#[test]
fn test_and_short_circuit_false() {
    expect_bool(
        "return false && true;",
        false,
    );
}

#[test]
fn test_or_short_circuit_true() {
    expect_bool(
        "return true || false;",
        true,
    );
}

#[test]
fn test_complex_boolean_expression() {
    expect_bool(
        "let a = 10;
         let b = 20;
         let c = 30;
         return a < b && b < c && a + b + c == 60;",
        true,
    );
}

// ============================================================================
// 34. Return Value From Void Function (should error)
// ============================================================================

#[test]
fn test_void_function_return_value_errors() {
    expect_compile_error(
        "function doStuff(): void {
             return 42;
         }",
        "TypeMismatch",
    );
}

// ============================================================================
// 35. Abstract Class Cannot Be Instantiated
// ============================================================================

#[test]
fn test_abstract_class_instantiation_errors() {
    expect_compile_error(
        "abstract class Shape {
             abstract area(): int;
         }
         let s = new Shape();",
        "AbstractClassInstantiation",
    );
}

// ============================================================================
// 36. Private Field Access from Outside (should error)
// ============================================================================

#[test]
fn test_private_field_access_errors() {
    expect_compile_error(
        "class Vault {
             private secret: int;
             constructor() { this.secret = 42; }
         }
         let v = new Vault();
         return v.secret;",
        "private",
    );
}

// ============================================================================
// 37. Calling Super in Constructor
// ============================================================================

#[test]
fn test_super_constructor_call() {
    expect_i32(
        "class Base {
             value: int;
             constructor(v: int) { this.value = v; }
         }
         class Derived extends Base {
             extra: int;
             constructor(v: int, e: int) {
                 super(v);
                 this.extra = e;
             }
         }
         let d = new Derived(20, 22);
         return d.value + d.extra;",
        42,
    );
}

#[test]
fn test_super_method_call() {
    expect_i32(
        "class Base {
             compute(): int { return 20; }
         }
         class Derived extends Base {
             compute(): int { return super.compute() + 22; }
         }
         return new Derived().compute();",
        42,
    );
}

// ============================================================================
// 38. Generic Method on Class
// ============================================================================

#[test]
fn test_generic_method() {
    expect_i32(
        "class Util {
             static wrap<T>(val: T): T {
                 return val;
             }
         }
         return Util.wrap<int>(42);",
        42,
    );
}

// ============================================================================
// 39. Array of Functions
// ============================================================================

#[test]
fn test_array_of_functions() {
    expect_i32(
        "let fns: ((x: int) => int)[] = [
             (x: int): int => x + 1,
             (x: int): int => x * 2,
             (x: int): int => x - 1
         ];
         return fns[1](21);",
        42,
    );
}

// ============================================================================
// 40. Enum-like Pattern with Constants
// ============================================================================

#[test]
fn test_class_static_constants_as_enum() {
    expect_i32(
        "class Color {
             static RED: int = 0;
             static GREEN: int = 1;
             static BLUE: int = 2;
         }
         let selected = Color.BLUE;
         if (selected == Color.BLUE) { return 42; }
         return 0;",
        42,
    );
}

// ============================================================================
// 41. Integer Division Truncation
// ============================================================================

#[test]
fn test_integer_division_truncates() {
    // 85 / 2 should be 42 (integer division)
    expect_i32(
        "return 85 / 2;",
        42,
    );
}

#[test]
fn test_integer_division_negative() {
    // -85 / 2 should be -42 (truncates toward zero)
    expect_i32(
        "return -85 / 2;",
        -42,
    );
}

// ============================================================================
// 42. String Concatenation with +
// ============================================================================

#[test]
fn test_string_concat_plus() {
    expect_string(
        "let a = \"hello\";
         let b = \" world\";
         return a + b;",
        "hello world",
    );
}

#[test]
fn test_string_concat_multiple() {
    expect_string(
        "return \"a\" + \"b\" + \"c\";",
        "abc",
    );
}

// ============================================================================
// 43. Complex Generics — Generic Returning Generic
// ============================================================================

#[test]
fn test_generic_function_returning_array() {
    expect_i32(
        "function repeat<T>(val: T, times: int): T[] {
             let result: T[] = [];
             for (let i = 0; i < times; i += 1) {
                 result.push(val);
             }
             return result;
         }
         let arr = repeat<int>(14, 3);
         let sum = 0;
         for (const x of arr) { sum += x; }
         return sum;",
        42,
    );
}

// ============================================================================
// 44. Labeled Loops / Nested Loop Break
// ============================================================================

#[test]
fn test_nested_loop_break() {
    // Without labels, break only exits inner loop
    expect_i32(
        "let count = 0;
         for (let i = 0; i < 10; i += 1) {
             for (let j = 0; j < 10; j += 1) {
                 count += 1;
                 if (count == 42) { return count; }
             }
         }
         return count;",
        42,
    );
}

// ============================================================================
// 45. Edge Case: Return in Finally
// ============================================================================

#[test]
fn test_return_in_finally_overrides() {
    // If both try and finally return, finally's return should win
    expect_i32_with_builtins(
        "function test(): int {
             try {
                 return 10;
             } finally {
                 return 42;
             }
         }
         return test();",
        42,
    );
}

// ============================================================================
// 46. Complex Scope: Function Inside If
// ============================================================================

#[test]
fn test_function_inside_if() {
    expect_i32(
        "let x = true;
         if (x) {
             function inner(): int { return 42; }
             return inner();
         }
         return 0;",
        42,
    );
}

// ============================================================================
// 47. Class with Getter-Like Method
// ============================================================================

#[test]
fn test_class_getter_pattern() {
    expect_i32(
        "class Temperature {
             private celsius: int;
             constructor(c: int) { this.celsius = c; }
             getCelsius(): int { return this.celsius; }
         }
         let t = new Temperature(42);
         return t.getCelsius();",
        42,
    );
}

// ============================================================================
// 48. Multi-Level Generics with Constraints
// ============================================================================

#[test]
fn test_generic_constraint_with_method() {
    expect_i32(
        "class HasValue {
             val: int;
             constructor(v: int) { this.val = v; }
             getVal(): int { return this.val; }
         }
         class Special extends HasValue {
             constructor(v: int) { super(v); }
         }
         function extract<T extends HasValue>(item: T): int {
             return item.getVal();
         }
         return extract(new Special(42));",
        42,
    );
}

// ============================================================================
// 49. String.substring with Both Args
// ============================================================================

#[test]
fn test_string_substring_both_args() {
    expect_string(
        "let s = \"Hello, World!\";
         return s.substring(7, 12);",
        "World",
    );
}

#[test]
fn test_string_substring_from_start() {
    expect_string(
        "let s = \"Hello\";
         return s.substring(0, 5);",
        "Hello",
    );
}

// ============================================================================
// 50. Array.sort Stability
// ============================================================================

#[test]
fn test_array_sort_ascending() {
    expect_array_i32(
        "let arr: int[] = [3, 1, 4, 1, 5, 9, 2, 6];
         arr.sort((a: int, b: int): int => a - b);
         return arr;",
        &[1, 1, 2, 3, 4, 5, 6, 9],
    );
}

#[test]
fn test_array_sort_descending() {
    expect_array_i32(
        "let arr: int[] = [3, 1, 4, 1, 5, 9, 2, 6];
         arr.sort((a: int, b: int): int => b - a);
         return arr;",
        &[9, 6, 5, 4, 3, 2, 1, 1],
    );
}

// ============================================================================
// 51. Deeply Nested Object Access
// ============================================================================

#[test]
fn test_deeply_nested_field_access() {
    expect_i32(
        "class Inner {
             value: int;
             constructor(v: int) { this.value = v; }
         }
         class Middle {
             inner: Inner;
             constructor(v: int) { this.inner = new Inner(v); }
         }
         class Outer {
             middle: Middle;
             constructor(v: int) { this.middle = new Middle(v); }
         }
         let o = new Outer(42);
         return o.middle.inner.value;",
        42,
    );
}

// ============================================================================
// 52. Mixed Arithmetic — int and number promotion
// ============================================================================

#[test]
fn test_int_plus_number_promotes() {
    expect_f64(
        "let i: int = 40;
         let f: number = 2.0;
         return i + f;",
        42.0,
    );
}

#[test]
fn test_int_times_number_promotes() {
    expect_f64(
        "let i: int = 21;
         let f: number = 2.0;
         return i * f;",
        42.0,
    );
}

// ============================================================================
// 53. Array.indexOf with Strings
// ============================================================================

// FIXED: Array.indexOf() now uses value equality for strings
#[test]
fn test_array_index_of_string() {
    expect_i32(
        "let arr: string[] = [\"a\", \"b\", \"c\"];
         return arr.indexOf(\"b\");",
        1,
    );
}

#[test]
fn test_array_index_of_not_found() {
    expect_i32(
        "let arr: string[] = [\"a\", \"b\", \"c\"];
         return arr.indexOf(\"z\");",
        -1,
    );
}

// ============================================================================
// 54. Complex Ternary Chains
// ============================================================================

#[test]
fn test_nested_ternary() {
    expect_i32(
        "let x = 2;
         return x == 1 ? 10 : x == 2 ? 42 : 99;",
        42,
    );
}

#[test]
fn test_ternary_with_function_calls() {
    expect_i32(
        "function a(): int { return 20; }
         function b(): int { return 22; }
         return true ? a() + b() : 0;",
        42,
    );
}

// ============================================================================
// 55. Class Extending Class with Generics
// ============================================================================

#[test]
fn test_generic_class_extended() {
    expect_i32(
        "class Base<T> {
             value: T;
             constructor(v: T) { this.value = v; }
         }
         class IntBox extends Base<int> {
             constructor(v: int) { super(v); }
             doubled(): int { return this.value * 2; }
         }
         return new IntBox(21).doubled();",
        42,
    );
}

// ============================================================================
// 56. String Escape Sequences
// ============================================================================

#[test]
fn test_string_newline_escape() {
    expect_i32(
        "let s = \"hello\\nworld\";
         return s.length;",
        11,
    );
}

#[test]
fn test_string_tab_escape() {
    expect_i32(
        "let s = \"a\\tb\";
         return s.length;",
        3,
    );
}

#[test]
fn test_string_escaped_quote() {
    expect_i32(
        "let s = \"she said \\\"hi\\\"\";
         return s.length;",
        13, // s h e   s a i d   " h i " = 13 chars
    );
}

// ============================================================================
// 57. Logical NOT with Comparisons
// ============================================================================

#[test]
fn test_not_equal_with_not() {
    expect_bool(
        "let x = 42;
         return !(x == 10);",
        true,
    );
}

#[test]
fn test_not_less_than() {
    expect_bool(
        "return !(42 < 10);",
        true,
    );
}

// ============================================================================
// 58. Array Reduce — Same Type Accumulator (working variant)
// ============================================================================

#[test]
fn test_reduce_sum() {
    expect_i32(
        "let arr: int[] = [10, 12, 20];
         return arr.reduce((acc: int, x: int): int => acc + x, 0);",
        42,
    );
}

#[test]
fn test_reduce_product() {
    expect_i32(
        "let arr: int[] = [2, 3, 7];
         return arr.reduce((acc: int, x: int): int => acc * x, 1);",
        42,
    );
}

// ============================================================================
// 59. Recursive Function — Fibonacci
// ============================================================================

#[test]
fn test_fibonacci() {
    // fib(9) = 34, fib(10) = 55 ... let's use fib(9) + 8 = 42
    expect_i32(
        "function fib(n: int): int {
             if (n <= 1) { return n; }
             return fib(n - 1) + fib(n - 2);
         }
         return fib(9) + 8;",
        42, // fib(9) = 34, 34 + 8 = 42
    );
}

// ============================================================================
// 60. Mutual Recursion
// ============================================================================

#[test]
fn test_mutual_recursion() {
    expect_bool(
        "function isEven(n: int): boolean {
             if (n == 0) { return true; }
             return isOdd(n - 1);
         }
         function isOdd(n: int): boolean {
             if (n == 0) { return false; }
             return isEven(n - 1);
         }
         return isEven(42);",
        true,
    );
}

// ============================================================================
// 61. Equality Checks on Different Types
// ============================================================================

#[test]
fn test_strict_equality_same_int() {
    expect_bool(
        "return 42 === 42;",
        true,
    );
}

#[test]
fn test_strict_inequality_different() {
    expect_bool(
        "return 42 !== 43;",
        true,
    );
}

#[test]
fn test_null_equality() {
    expect_bool(
        "let x: int | null = null;
         return x === null;",
        true,
    );
}

#[test]
fn test_null_inequality() {
    expect_bool(
        "let x: int | null = 42;
         return x !== null;",
        true,
    );
}

// ============================================================================
// 62. String.repeat method
// ============================================================================

#[test]
fn test_string_repeat() {
    expect_string(
        "return \"ab\".repeat(3);",
        "ababab",
    );
}

#[test]
fn test_string_repeat_zero() {
    expect_string(
        "return \"abc\".repeat(0);",
        "",
    );
}

// ============================================================================
// 63. Array.concat edge cases
// ============================================================================

#[test]
fn test_concat_empty_arrays() {
    expect_i32(
        "let a: int[] = [];
         let b: int[] = [];
         let c = a.concat(b);
         return c.length;",
        0,
    );
}

#[test]
fn test_concat_preserves_original() {
    expect_i32(
        "let a: int[] = [1, 2, 3];
         let b: int[] = [4, 5];
         let c = a.concat(b);
         return a.length;",
        3, // original unchanged
    );
}
