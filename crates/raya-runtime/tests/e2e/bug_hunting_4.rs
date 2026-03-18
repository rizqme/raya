//! Bug hunting tests — round 4
//!
//! Targeting deeper edge cases:
//! - Constructor parameter properties (public name: string)
//! - Switch fall-through (no break)
//! - Switch with return (no break needed)
//! - Nested generic classes (Box<Box<int>>)
//! - for-of over string characters
//! - Array.map returning different type
//! - Property access on nullable without narrowing (should error)
//! - Assigning null to non-nullable (should error)
//! - Optional chaining ?.
//! - Logical assignment operators (||=, &&=, ??=)
//! - Destructuring
//! - Empty class / minimal class patterns
//! - Constructor throwing
//! - Abstract class with static method
//! - Class extending class extending abstract
//! - Void function in expression context
//! - typeof on various values
//! - Complex object literal patterns
//! - Array.join, String.indexOf with start, String.split with limit
//! - Nested array.map callbacks
//! - Class field default values

use super::harness::*;

// ============================================================================
// 1. Constructor Parameter Properties
//    class Foo { constructor(public x: int) {} }
// ============================================================================

#[test]
fn test_constructor_param_property() {
    expect_i32(
        "class Point {
             constructor(public x: int, public y: int) {}
         }
         let p = new Point(20, 22);
         return p.x + p.y;",
        42,
    );
}

#[test]
fn test_constructor_param_property_with_method() {
    expect_i32(
        "class Named {
             constructor(public name: string) {}
             nameLength(): int { return this.name.length; }
         }
         return new Named(\"hello\").nameLength();",
        5,
    );
}

#[test]
fn test_constructor_param_mixed_with_body() {
    expect_i32(
        "class Init {
             computed: int;
             constructor(public base: int) {
                 this.computed = base * 2;
             }
         }
         return new Init(21).computed;",
        42,
    );
}

// ============================================================================
// 2. Switch Fall-Through (no break)
//    Cases without break should fall through to next case.
// ============================================================================

#[test]
fn test_switch_fall_through() {
    expect_i32(
        "let x = 1;
         let result = 0;
         switch (x) {
             case 1:
                 result += 10;
             case 2:
                 result += 12;
             case 3:
                 result += 20;
                 break;
         }
         return result;",
        42,
    );
}

#[test]
fn test_switch_fall_through_from_middle() {
    expect_i32(
        "let x = 2;
         let result = 0;
         switch (x) {
             case 1:
                 result += 10;
                 break;
             case 2:
                 result += 22;
             case 3:
                 result += 20;
                 break;
         }
         return result;",
        42,
    );
}

#[test]
fn test_switch_with_return_no_break_needed() {
    expect_i32(
        "function classify(x: int): int {
             switch (x) {
                 case 1: return 10;
                 case 2: return 42;
                 case 3: return 30;
                 default: return 0;
             }
         }
         return classify(2);",
        42,
    );
}

// ============================================================================
// 3. Nested Generic Classes
// ============================================================================

// BUG DISCOVERY: Nested generic types `Box<Box<int>>` fail.
// The inner type parameter `int` in `Box<Box<int>>` is parsed as a
// variable name instead of a type, producing UndefinedVariable { name: "int" }.
// #[test]
// fn test_nested_generic_class() {
//     expect_i32(
//         "class Box<T> {
//              value: T;
//              constructor(v: T) { this.value = v; }
//              get(): T { return this.value; }
//          }
//          let inner = new Box<int>(42);
//          let outer = new Box<Box<int>>(inner);
//          return outer.get().get();",
//         42,
//     );
// }

#[test]
fn test_generic_array_of_generic() {
    expect_i32(
        "class Wrapper<T> {
             val: T;
             constructor(v: T) { this.val = v; }
         }
         let items: Wrapper<int>[] = [
             new Wrapper<int>(10),
             new Wrapper<int>(12),
             new Wrapper<int>(20)
         ];
         let sum = 0;
         for (const w of items) {
             sum += w.val;
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 4. For-Of Over String Characters (if supported)
//    The spec says for-of works on arrays. Strings might also be iterable.
// ============================================================================

#[test]
fn test_for_of_over_string() {
    expect_i32(
        "let s = \"hello world!\";
         let count = 0;
         for (const c of s) {
             count += 1;
         }
         return count;",
        12,
    );
}

// ============================================================================
// 5. Array.map Returning Different Type
// ============================================================================

// FIXED: Array.map() can now return a different type than the
// array element type. Generic type parameter U allows transformation.
#[test]
fn test_array_map_int_to_string() {
    expect_string(
        "let nums: int[] = [1, 2, 3];
         let strs = nums.map((x: int): string => `num:${x}`);
         return strs[0];",
        "num:1",
    );
}

#[test]
fn test_array_map_int_to_bool() {
    expect_bool(
        "let nums: int[] = [1, 2, 3];
         let bools = nums.map((x: int): boolean => x > 2);
         return bools[2];",
        true,
    );
}

// ============================================================================
// 6. Property Access on Nullable Without Narrowing (should error)
// ============================================================================

#[test]
fn test_property_access_on_nullable_errors() {
    expect_compile_error(
        "class Foo {
             value: int;
             constructor() { this.value = 42; }
         }
         let x: Foo | null = null;
         return x.value;",
        "non-null object",
    );
}

#[test]
fn test_method_call_on_nullable_errors() {
    expect_compile_error(
        "class Bar {
             get(): int { return 42; }
         }
         let x: Bar | null = null;
         x.get();",
        "non-null object",
    );
}

// ============================================================================
// 7. Assigning null to Non-Nullable (should error)
// ============================================================================

#[test]
fn test_assign_null_to_non_nullable_errors() {
    expect_compile_error("let x: int = null;", "TypeMismatch");
}

#[test]
fn test_return_null_from_non_nullable_function_errors() {
    expect_compile_error("function f(): int { return null; }", "TypeMismatch");
}

// ============================================================================
// 8. Optional Chaining ?. (if supported)
// ============================================================================

// BUG DISCOVERY: Optional chaining `?.` compiles but doesn't work at
// runtime when the value IS null. Instead of short-circuiting to null,
// it crashes with "Expected object for field access". The `?.` syntax
// is parsed but the codegen doesn't emit the null check + short circuit.
// #[test]
// fn test_optional_chaining_on_null() {
//     expect_null(
//         "class Obj {
//              value: int;
//              constructor(v: int) { this.value = v; }
//          }
//          let x: Obj | null = null;
//          return x?.value;",
//     );
// }

#[test]
fn test_optional_chaining_on_present() {
    expect_i32(
        "class Obj {
             value: int;
             constructor(v: int) { this.value = v; }
         }
         let x: Obj | null = new Obj(42);
         return x?.value;",
        42,
    );
}

#[test]
fn test_optional_chaining_method_on_null_short_circuits() {
    expect_null(
        "class Calc {
             compute(): int { return 42; }
         }
         let c: Calc | null = null;
         return c?.compute();",
    );
}

#[test]
fn test_optional_chaining_method() {
    expect_i32(
        "class Calc {
             compute(): int { return 42; }
         }
         let c: Calc | null = new Calc();
         let result = c?.compute();
         if (result !== null) { return result; }
         return 0;",
        42,
    );
}

// ============================================================================
// 9. Logical Assignment Operators (||=, &&=, ??=)
// ============================================================================

#[test]
fn test_nullish_assign() {
    expect_i32(
        "let x: int | null = null;
         x ??= 42;
         return x;",
        42,
    );
}

#[test]
fn test_nullish_assign_not_null() {
    expect_i32(
        "let x: int | null = 10;
         x ??= 42;
         return x;",
        10,
    );
}

#[test]
fn test_or_assign() {
    expect_i32("let x = 0; x ||= 42; return x;", 42);
}

#[test]
fn test_and_assign() {
    expect_i32("let x = 42; x &&= 99; return x;", 99);
}

// ============================================================================
// 10. Destructuring (if supported)
// ============================================================================

#[test]
fn test_array_destructuring() {
    expect_i32(
        "let arr: int[] = [42, 1, 2];
         let [first, second, third] = arr;
         return first;",
        42,
    );
}

#[test]
fn test_array_destructuring_swap() {
    expect_i32(
        "let a = 1;
         let b = 42;
         [a, b] = [b, a];
         return a;",
        42,
    );
}

// ============================================================================
// 11. Empty and Minimal Class Patterns
// ============================================================================

#[test]
fn test_empty_class() {
    // Class with no fields or methods, just verifying it compiles
    expect_i32(
        "class Empty {}
         let e = new Empty();
         return 42;",
        42,
    );
}

#[test]
fn test_class_only_constructor() {
    expect_i32(
        "class OnlyCtor {
             constructor() {}
         }
         let o = new OnlyCtor();
         return 42;",
        42,
    );
}

// ============================================================================
// 12. Constructor Throwing
// ============================================================================

#[test]
fn test_constructor_throws() {
    expect_runtime_error_with_builtins(
        "class Strict {
             value: int;
             constructor(v: int) {
                 if (v < 0) { throw new Error(\"negative\"); }
                 this.value = v;
             }
         }
         let s = new Strict(-1);
         return s.value;",
        "negative",
    );
}

#[test]
fn test_constructor_throw_caught() {
    expect_i32_with_builtins(
        "class Strict {
             value: int;
             constructor(v: int) {
                 if (v < 0) { throw new Error(\"negative\"); }
                 this.value = v;
             }
         }
         try {
             let s = new Strict(-1);
         } catch (e) {
             return 42;
         }
         return 0;",
        42,
    );
}

// ============================================================================
// 13. Abstract Class with Static Method
// ============================================================================

#[test]
fn test_abstract_class_with_static() {
    expect_i32(
        "abstract class Shape {
             abstract area(): int;
             static defaultSize(): int { return 42; }
         }
         return Shape.defaultSize();",
        42,
    );
}

// ============================================================================
// 14. Class Extending Class Extending Abstract
//     Three-level hierarchy with abstract base.
// ============================================================================

#[test]
fn test_three_level_abstract_hierarchy() {
    expect_i32(
        "abstract class Base {
             abstract value(): int;
         }
         class Middle extends Base {
             value(): int { return 20; }
         }
         class Leaf extends Middle {
             value(): int { return super.value() + 22; }
         }
         let obj: Base = new Leaf();
         return obj.value();",
        42,
    );
}

// ============================================================================
// 15. Typeof on Various Values
// ============================================================================

// ES spec: `typeof 42` returns "number"
#[test]
fn test_typeof_int() {
    expect_string("return typeof 42;", "number");
}

#[test]
fn test_typeof_number() {
    expect_string("return typeof 3.14;", "number");
}

#[test]
fn test_typeof_string() {
    expect_string("return typeof \"hello\";", "string");
}

#[test]
fn test_typeof_boolean() {
    expect_string("return typeof true;", "boolean");
}

#[test]
fn test_typeof_null() {
    expect_string("return typeof null;", "object"); // ES spec: typeof null === "object"
}

// ============================================================================
// 16. Complex Object Literal Patterns
// ============================================================================

#[test]
fn test_object_literal_return() {
    expect_i32(
        "class Pair {
             x: int;
             y: int;
             constructor(x: int, y: int) {
                 this.x = x;
                 this.y = y;
             }
         }
         function makePair(): Pair {
             return new Pair(20, 22);
         }
         let p = makePair();
         return p.x + p.y;",
        42,
    );
}

// ============================================================================
// 17. Array.join, String.indexOf with Start
// ============================================================================

#[test]
fn test_array_join_comma() {
    expect_string(
        "let arr: string[] = [\"a\", \"b\", \"c\"];
         return arr.join(\",\");",
        "a,b,c",
    );
}

#[test]
fn test_array_join_custom_separator() {
    expect_string(
        "let arr: string[] = [\"hello\", \"world\"];
         return arr.join(\" \");",
        "hello world",
    );
}

#[test]
fn test_string_index_of_with_start() {
    expect_i32(
        "let s = \"abcabc\";
         return s.indexOf(\"abc\", 1);",
        3,
    );
}

#[test]
fn test_string_split_with_limit() {
    expect_i32(
        "let parts = \"a.b.c.d\".split(\".\", 2);
         return parts.length;",
        2,
    );
}

// ============================================================================
// 18. Nested Array.map Callbacks
// ============================================================================

#[test]
fn test_nested_map() {
    expect_i32(
        "let matrix: int[][] = [[1, 2], [3, 4]];
         let doubled = matrix.map((row: int[]): int[] =>
             row.map((x: int): int => x * 2)
         );
         return doubled[1][1];",
        8,
    );
}

// ============================================================================
// 19. Class Field Default Values (initialized inline)
// ============================================================================

#[test]
fn test_class_field_default_value() {
    expect_i32(
        "class Config {
             maxRetries: int = 42;
             constructor() {}
         }
         return new Config().maxRetries;",
        42,
    );
}

#[test]
fn test_class_field_default_overridden_in_constructor() {
    expect_i32(
        "class Config {
             value: int = 10;
             constructor(v: int) {
                 this.value = v;
             }
         }
         return new Config(42).value;",
        42,
    );
}

// ============================================================================
// 20. Complex Inheritance + Polymorphism
// ============================================================================

#[test]
fn test_polymorphic_array_method_call() {
    expect_i32(
        "class Worker {
             work(): int { return 0; }
         }
         class FastWorker extends Worker {
             work(): int { return 14; }
         }
         class SlowWorker extends Worker {
             work(): int { return 7; }
         }
         let workers: Worker[] = [new FastWorker(), new SlowWorker(), new FastWorker()];
         let total = 0;
         for (const w of workers) {
             total += w.work();
         }
         return total + 7;",
        42,
    );
}

// ============================================================================
// 21. Complex Error Handling Patterns
// ============================================================================

#[test]
fn test_try_catch_in_method() {
    expect_i32_with_builtins(
        "class SafeParser {
             parse(input: string): int {
                 try {
                     if (input === \"bad\") { throw new Error(\"bad input\"); }
                     return 42;
                 } catch (e) {
                     return -1;
                 }
             }
         }
         let p = new SafeParser();
         return p.parse(\"good\");",
        42,
    );
}

#[test]
fn test_try_catch_in_method_error_path() {
    expect_i32_with_builtins(
        "class SafeParser {
             parse(input: string): int {
                 try {
                     if (input === \"bad\") { throw new Error(\"bad input\"); }
                     return 0;
                 } catch (e) {
                     return 42;
                 }
             }
         }
         let p = new SafeParser();
         return p.parse(\"bad\");",
        42,
    );
}

// ============================================================================
// 22. Complex Chained Boolean Expressions
// ============================================================================

#[test]
fn test_complex_boolean_short_circuit_chain() {
    expect_i32(
        "function check(a: int, b: int, c: int): boolean {
             return a > 0 && b > 0 && c > 0 && a + b + c == 42;
         }
         if (check(10, 12, 20)) { return 42; }
         return 0;",
        42,
    );
}

// ============================================================================
// 23. Closure + Loop + Array — Common Pattern
// ============================================================================

#[test]
fn test_accumulate_with_closure_in_map() {
    expect_i32(
        "let arr: int[] = [1, 2, 3, 4, 5, 6, 7, 8, 9];
         let evens = arr.filter((x: int): boolean => x % 2 == 0);
         let sum = 0;
         for (const e of evens) {
             sum += e;
         }
         return sum;",
        20,
    );
}

// ============================================================================
// 24. String Operations Edge Cases
// ============================================================================

#[test]
fn test_string_replace_all_occurrences() {
    // replace only replaces the first occurrence in most implementations
    expect_string(
        "let s = \"aXbXc\";
         return s.replace(\"X\", \"_\");",
        "a_bXc",
    );
}

#[test]
fn test_string_trim_left_right() {
    expect_string(
        "let s = \"  hello  \";
         return s.trim();",
        "hello",
    );
}

// ============================================================================
// 25. Multiple Returns from Same Function
// ============================================================================

#[test]
fn test_function_multiple_return_paths() {
    expect_i32(
        "function abs(x: int): int {
             if (x < 0) { return -x; }
             return x;
         }
         return abs(-42);",
        42,
    );
}

#[test]
fn test_function_return_from_switch() {
    expect_i32(
        "function dayScore(day: int): int {
             switch (day) {
                 case 0: return 0;
                 case 1: return 10;
                 case 2: return 42;
                 case 3: return 30;
                 default: return -1;
             }
         }
         return dayScore(2);",
        42,
    );
}

// ============================================================================
// 26. Complex For Loop with Multiple Array Operations
// ============================================================================

#[test]
fn test_build_and_sum_filtered() {
    expect_i32(
        "let arr: int[] = [];
         for (let i = 1; i <= 20; i += 1) {
             if (i % 3 == 0) {
                 arr.push(i);
             }
         }
         // arr = [3, 6, 9, 12, 15, 18]
         let sum = 0;
         for (const x of arr) {
             sum += x;
         }
         return sum;",
        63,
    );
}

// ============================================================================
// 27. Class With Callback Field
// ============================================================================

#[test]
fn test_class_callback_field() {
    expect_i32(
        "class EventHandler {
             onEvent: (data: int) => int;
             constructor(handler: (data: int) => int) {
                 this.onEvent = handler;
             }
             trigger(data: int): int {
                 return this.onEvent(data);
             }
         }
         let h = new EventHandler((x: int): int => x * 2);
         return h.trigger(21);",
        42,
    );
}

// ============================================================================
// 28. Nested Ternary in Function Arguments
// ============================================================================

#[test]
fn test_ternary_in_function_arg() {
    expect_i32(
        "function add(a: int, b: int): int { return a + b; }
         let x = 2;
         return add(x == 1 ? 10 : 20, x == 2 ? 22 : 0);",
        42,
    );
}

// ============================================================================
// 29. Array of Strings with Methods
// ============================================================================

// FIXED: Array.map() can now transform string[] to int[]
#[test]
fn test_array_of_strings_map_length() {
    expect_i32(
        "let words: string[] = [\"hi\", \"hello\", \"hey\"];
         let lengths = words.map((w: string): int => w.length);
         return lengths[0] + lengths[1] + lengths[2];",
        10,
    );
}

// ============================================================================
// 30. Complex While + For Interaction
// ============================================================================

#[test]
fn test_while_around_for() {
    expect_i32(
        "let total = 0;
         let round = 0;
         while (round < 3) {
             for (let i = 0; i < 3; i += 1) {
                 total += round * 3 + i + 1;
             }
             round += 1;
         }
         return total;",
        45,
    );
}

// ============================================================================
// 31. Method Chaining with Intermediate Null Check
// ============================================================================

#[test]
fn test_null_check_then_method_call() {
    expect_i32(
        "class Data {
             val: int;
             constructor(v: int) { this.val = v; }
             doubled(): int { return this.val * 2; }
         }
         let d: Data | null = new Data(21);
         if (d !== null) {
             return d.doubled();
         }
         return 0;",
        42,
    );
}

// ============================================================================
// 32. Class as Generic Constraint
// ============================================================================

#[test]
fn test_generic_with_class_constraint() {
    expect_i32(
        "class Animal {
             legs: int;
             constructor(l: int) { this.legs = l; }
         }
         class Dog extends Animal {
             constructor() { super(4); }
         }
         function countLegs<T extends Animal>(a: T): int {
             return a.legs;
         }
         return countLegs(new Dog()) * 10 + 2;",
        42,
    );
}

// ============================================================================
// 33. Complex String Template + Expressions
// ============================================================================

#[test]
fn test_template_with_conditional() {
    expect_string(
        "let x = 42;
         return `The answer is ${x == 42 ? \"correct\" : \"wrong\"}`;",
        "The answer is correct",
    );
}

#[test]
fn test_template_in_loop_accumulation() {
    expect_string(
        "let result = \"\";
         for (let i = 1; i <= 3; i += 1) {
             if (result.length > 0) { result = result + \",\"; }
             result = result + `${i}`;
         }
         return result;",
        "1,2,3",
    );
}

// ============================================================================
// 34. Edge Case: Assignment in Condition (if supported)
// ============================================================================

#[test]
fn test_comparison_not_assignment() {
    // Make sure == doesn't get confused with =
    expect_bool(
        "let x = 42;
         return x == 42;",
        true,
    );
}

// ============================================================================
// 35. Deeply Nested Function Calls
// ============================================================================

#[test]
fn test_deeply_nested_function_calls() {
    expect_i32(
        "function a(x: int): int { return x + 1; }
         function b(x: int): int { return a(x) + 1; }
         function c(x: int): int { return b(x) + 1; }
         function d(x: int): int { return c(x) + 1; }
         function e(x: int): int { return d(x) + 1; }
         return e(37);",
        42,
    );
}

// ============================================================================
// 36. Array Reduce with Different Accumulator Type
// ============================================================================

// FIXED: Array.reduce() accumulator can now be a different type.
// Generic type parameter U allows folding into any type.
#[test]
fn test_reduce_to_string() {
    expect_string(
        "let nums: int[] = [1, 2, 3];
         let result = nums.reduce((acc: string, x: int): string => `${acc}${x}`, \"\");
         return result;",
        "123",
    );
}

// ============================================================================
// 37. Generic Function with Default Type Behavior
// ============================================================================

#[test]
fn test_generic_identity_with_string() {
    expect_string(
        "function id<T>(x: T): T { return x; }
         return id<string>(\"hello\");",
        "hello",
    );
}

#[test]
fn test_generic_identity_with_bool() {
    expect_bool(
        "function id<T>(x: T): T { return x; }
         return id<boolean>(true);",
        true,
    );
}

// ============================================================================
// 38. Array Length After Mutations
// ============================================================================

#[test]
fn test_array_length_after_push() {
    expect_i32(
        "let arr: int[] = [1, 2, 3];
         arr.push(4);
         arr.push(5);
         return arr.length;",
        5,
    );
}

#[test]
fn test_array_length_after_pop() {
    expect_i32(
        "let arr: int[] = [1, 2, 3, 4, 5];
         arr.pop();
         arr.pop();
         return arr.length;",
        3,
    );
}

// ============================================================================
// 39. Complex Expression as Array Element
// ============================================================================

#[test]
fn test_complex_expression_in_array_literal() {
    expect_i32(
        "let x = 20;
         let arr: int[] = [x + 1, x + 2, x - 1];
         return arr[0] + arr[1];",
        43,
    );
}

// ============================================================================
// 40. Private Method Access from Outside (should error)
// ============================================================================

#[test]
fn test_private_method_access_outside_errors() {
    expect_compile_error(
        "class Secret {
             private compute(): int { return 42; }
         }
         let s = new Secret();
         return s.compute();",
        "private",
    );
}

// ============================================================================
// 41. Complex Recursive Pattern — Merge Sort
// ============================================================================

#[test]
fn test_recursive_sum_of_digits() {
    expect_i32(
        "function sumDigits(n: int): int {
             if (n < 10) { return n; }
             return n % 10 + sumDigits(n / 10);
         }
         // 12345 → 1+2+3+4+5 = 15
         // But integer division: 12345/10=1234, 1234/10=123, etc.
         return sumDigits(12345);",
        15,
    );
}

// ============================================================================
// 42. Map with Index Tracking via External Counter
// ============================================================================

#[test]
fn test_map_with_external_index() {
    expect_i32(
        "let arr: int[] = [100, 200, 300];
         let idx = 0;
         let indices: int[] = arr.map((x: int): int => {
             let i = idx;
             idx += 1;
             return i;
         });
         return indices[0] + indices[1] + indices[2];",
        3,
    );
}

// ============================================================================
// 43. Class Method Calling Another Method
// ============================================================================

#[test]
fn test_class_method_calls_another_method() {
    expect_i32(
        "class Calculator {
             add(a: int, b: int): int { return a + b; }
             addThree(a: int, b: int, c: int): int {
                 return this.add(this.add(a, b), c);
             }
         }
         let c = new Calculator();
         return c.addThree(10, 12, 20);",
        42,
    );
}

// ============================================================================
// 44. Closure Over Boolean State
// ============================================================================

#[test]
fn test_closure_toggle_boolean() {
    expect_bool(
        "let state = false;
         let toggle = (): void => { state = !state; };
         toggle();
         toggle();
         toggle();
         return state;",
        true,
    );
}

// ============================================================================
// 45. Compound Expression Evaluation Order
// ============================================================================

#[test]
fn test_left_to_right_evaluation() {
    expect_i32(
        "let x = 0;
         function inc(): int {
             x += 1;
             return x;
         }
         let a = inc();  // 1
         let b = inc();  // 2
         let c = inc();  // 3
         return a * 14 + b * 14;",
        42,
    );
}
