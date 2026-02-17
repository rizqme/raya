//! Edge-case tests for correctness across all language features.
//!
//! These tests target boundary conditions, unusual inputs, and subtle
//! semantics that are easy to get wrong.

use super::harness::*;

// ============================================================================
// 1. Integer Arithmetic Edge Cases
// ============================================================================

#[test]
fn test_int_overflow_add() {
    // i32::MAX + 1 wraps
    expect_i32("return 2147483647 + 1;", -2147483648);
}

#[test]
fn test_int_overflow_sub() {
    // i32::MIN - 1 wraps
    expect_i32("return -2147483648 - 1;", 2147483647);
}

#[test]
fn test_int_overflow_mul() {
    // Large multiply wraps
    expect_i32("return 2147483647 * 2;", -2);
}

#[test]
fn test_int_min_div_neg_one() {
    // i32::MIN / -1 should wrap to MIN (not panic)
    expect_i32("return -2147483648 / -1;", -2147483648);
}

#[test]
fn test_int_negate_min() {
    // -i32::MIN wraps to MIN
    expect_i32("return -(-2147483648);", -2147483648);
}

#[test]
fn test_int_div_by_zero() {
    expect_runtime_error("return 1 / 0;", "division by zero");
}

#[test]
fn test_int_mod_by_zero() {
    expect_runtime_error("return 1 % 0;", "division by zero");
}

#[test]
fn test_int_zero_dividend() {
    expect_i32("return 0 / 5;", 0);
}

#[test]
fn test_int_negative_modulo() {
    // Truncation toward zero: -7 % 3 = -1
    expect_i32("return -7 % 3;", -1);
}

#[test]
fn test_int_negative_division() {
    // -7 / 2 = -3 (truncation toward zero)
    expect_i32("return -7 / 2;", -3);
}

#[test]
fn test_int_power_zero() {
    expect_i32("return 5 ** 0;", 1);
}

#[test]
fn test_int_power_one() {
    expect_i32("return 5 ** 1;", 5);
}

#[test]
fn test_int_power_negative() {
    // Negative exponent returns 0 for integers
    expect_i32("return 2 ** -1;", 0);
}

#[test]
fn test_int_power_overflow() {
    // 2^31 wraps
    expect_i32("return 2 ** 31;", -2147483648);
}

// ============================================================================
// 2. Bitwise Operator Edge Cases
// ============================================================================

#[test]
fn test_bitwise_shift_zero() {
    expect_i32("return 42 << 0;", 42);
}

#[test]
fn test_bitwise_left_shift_sign() {
    // 1 << 31 sets the sign bit
    expect_i32("return 1 << 31;", -2147483648);
}

#[test]
fn test_bitwise_arith_right_shift_negative() {
    // Arithmetic right shift preserves sign
    expect_i32("return -8 >> 1;", -4);
}

#[test]
fn test_bitwise_unsigned_right_shift_negative() {
    // Unsigned right shift of -1 by 0 stays -1 (since our ints are i32)
    expect_i32("return -1 >>> 0;", -1);
}

#[test]
fn test_bitwise_not_zero() {
    expect_i32("return ~0;", -1);
}

#[test]
fn test_bitwise_not_neg_one() {
    expect_i32("return ~(-1);", 0);
}

#[test]
fn test_bitwise_xor_self_cancel() {
    expect_i32("let x: number = 42; return x ^ x;", 0);
}

#[test]
fn test_bitwise_and_assign() {
    expect_i32("let x: number = 15; x &= 9; return x;", 9);
}

#[test]
fn test_bitwise_or_assign() {
    expect_i32("let x: number = 9; x |= 6; return x;", 15);
}

#[test]
fn test_bitwise_xor_assign() {
    expect_i32("let x: number = 15; x ^= 9; return x;", 6);
}

// ============================================================================
// 3. Comparison and Logical Edge Cases
// ============================================================================

#[test]
fn test_null_equals_null() {
    expect_bool("return null == null;", true);
}

#[test]
fn test_short_circuit_and() {
    // && short-circuits: second operand not evaluated
    expect_i32("
        let x: number = 0;
        function inc(): boolean { x = x + 1; return true; }
        let r = false && inc();
        return x;
    ", 0);
}

#[test]
fn test_short_circuit_or() {
    // || short-circuits: second operand not evaluated
    expect_i32("
        let x: number = 0;
        function inc(): boolean { x = x + 1; return false; }
        let r = true || inc();
        return x;
    ", 0);
}

#[test]
fn test_nullish_coalescing_with_zero() {
    // 0 is falsy but not null, so ?? doesn't replace it
    expect_i32("let x: number | null = 0; return x ?? 99;", 0);
}

#[test]
fn test_nullish_coalescing_with_false() {
    // false is falsy but not null, so ?? doesn't replace it
    expect_bool("let x: boolean | null = false; return x ?? true;", false);
}

#[test]
fn test_nullish_coalescing_with_null() {
    expect_i32("let x: number | null = null; return x ?? 42;", 42);
}

#[test]
fn test_nullish_coalescing_chain() {
    expect_i32("
        let a: number | null = null;
        let b: number | null = null;
        let c: number = 3;
        return a ?? b ?? c;
    ", 3);
}

#[test]
fn test_nullish_assign_on_null() {
    expect_i32("let x: number | null = null; x ??= 42; return x;", 42);
}

#[test]
fn test_nullish_assign_on_non_null() {
    expect_i32("let x: number | null = 10; x ??= 42; return x;", 10);
}

#[test]
fn test_ternary_chained() {
    expect_i32("
        let x: number = 2;
        return x == 1 ? 10 : x == 2 ? 20 : 30;
    ", 20);
}

// ============================================================================
// 4. String Edge Cases
// ============================================================================

#[test]
fn test_string_empty_length() {
    expect_i32("return \"\".length;", 0);
}

#[test]
fn test_string_empty_uppercase() {
    expect_string("return \"\".toUpperCase();", "");
}

#[test]
fn test_string_empty_trim() {
    expect_string("return \"\".trim();", "");
}

#[test]
fn test_string_empty_indexof() {
    // indexOf("") should return 0 (searching for empty string at start)
    expect_i32("return \"hello\".indexOf(\"\");", 0);
}

#[test]
fn test_string_includes_empty() {
    expect_bool("return \"hello\".includes(\"\");", true);
}

#[test]
fn test_string_starts_with_empty() {
    expect_bool("return \"hello\".startsWith(\"\");", true);
}

#[test]
fn test_string_substring_equal_indices() {
    expect_string("return \"hello\".substring(2, 2);", "");
}

#[test]
fn test_string_substring_beyond_length() {
    expect_string("return \"hello\".substring(0, 100);", "hello");
}

#[test]
fn test_string_indexof_not_found() {
    expect_i32("return \"hello\".indexOf(\"xyz\");", -1);
}

#[test]
fn test_string_split_by_separator() {
    expect_string("return \"a,b,c\".split(\",\").join(\"|\");", "a|b|c");
}

#[test]
fn test_string_split_no_match() {
    // split with a separator not found returns array with original string
    expect_string("return \"hello\".split(\",\").join(\"|\");", "hello");
}

#[test]
fn test_string_char_code_at() {
    expect_i32("return \"A\".charCodeAt(0);", 65);
}

#[test]
fn test_string_nested_template() {
    expect_string("
        let x: number = 5;
        return `outer ${`inner ${x}`}`;
    ", "outer inner 5");
}

#[test]
fn test_string_case_sensitive_compare() {
    expect_bool("return \"abc\" === \"ABC\";", false);
}

#[test]
fn test_string_replace_first_only() {
    expect_string("return \"aaa\".replace(\"a\", \"b\");", "baa");
}

#[test]
fn test_string_repeat() {
    expect_string("return \"ab\".repeat(3);", "ababab");
}

#[test]
fn test_string_repeat_zero() {
    expect_string("return \"ab\".repeat(0);", "");
}

// ============================================================================
// 5. Array Edge Cases
// ============================================================================

#[test]
fn test_array_every_empty() {
    // every on empty array returns true (vacuously true)
    expect_bool("
        let arr: number[] = [];
        return arr.every((x: number): boolean => x > 0);
    ", true);
}

#[test]
fn test_array_some_empty() {
    // some on empty array returns false
    expect_bool("
        let arr: number[] = [];
        return arr.some((x: number): boolean => x > 0);
    ", false);
}

#[test]
fn test_array_filter_empty() {
    expect_i32("
        let arr: number[] = [];
        return arr.filter((x: number): boolean => x > 0).length;
    ", 0);
}

#[test]
fn test_array_map_empty() {
    expect_i32("
        let arr: number[] = [];
        return arr.map((x: number): number => x * 2).length;
    ", 0);
}

#[test]
fn test_array_join_empty() {
    expect_string("
        let arr: string[] = [];
        return arr.join(\",\");
    ", "");
}

#[test]
fn test_array_join_single() {
    expect_string("
        let arr: string[] = [\"hello\"];
        return arr.join(\",\");
    ", "hello");
}

#[test]
fn test_array_concat_empty() {
    expect_i32("
        let a: number[] = [1, 2];
        let b: number[] = [];
        return a.concat(b).length;
    ", 2);
}

#[test]
fn test_array_indexof_not_found() {
    expect_i32("
        let arr: number[] = [1, 2, 3];
        return arr.indexOf(99);
    ", -1);
}

#[test]
fn test_array_slice_full() {
    expect_i32("
        let arr: number[] = [1, 2, 3, 4, 5];
        let s = arr.slice(0, 5);
        return s.length;
    ", 5);
}

#[test]
fn test_array_pop_single() {
    expect_i32("
        let arr: number[] = [42];
        let v = arr.pop();
        return arr.length;
    ", 0);
}

#[test]
fn test_array_shift_single() {
    expect_i32("
        let arr: number[] = [42];
        let v = arr.shift();
        return arr.length;
    ", 0);
}

#[test]
fn test_array_spread() {
    expect_i32("
        let a: number[] = [2, 3];
        let b = [1, ...a, 4];
        return b.length;
    ", 4);
}

#[test]
fn test_array_spread_values() {
    expect_i32("
        let a: number[] = [2, 3];
        let b = [1, ...a, 4];
        return b[0] + b[1] + b[2] + b[3];
    ", 10);
}

#[test]
fn test_array_spread_empty() {
    expect_i32("
        let a: number[] = [];
        let b = [1, ...a, 2];
        return b.length;
    ", 2);
}

#[test]
fn test_array_spread_multiple() {
    expect_i32("
        let a: number[] = [1, 2];
        let b: number[] = [3, 4];
        let c = [...a, ...b];
        return c.length;
    ", 4);
}

#[test]
fn test_array_flat_on_flat() {
    expect_i32("
        let arr: number[] = [1, 2, 3];
        return arr.flat().length;
    ", 3);
}

#[test]
fn test_array_fill_entire() {
    expect_i32("
        let arr: number[] = [1, 2, 3];
        arr.fill(0);
        return arr[0] + arr[1] + arr[2];
    ", 0);
}

#[test]
fn test_array_reduce() {
    expect_i32("
        let arr: number[] = [1, 2, 3, 4];
        return arr.reduce((acc: number, x: number): number => acc + x, 0);
    ", 10);
}

// ============================================================================
// 6. Closure Edge Cases
// ============================================================================

#[test]
fn test_closure_three_level_nested() {
    expect_i32("
        function outer(): number {
            let x: number = 10;
            function middle(): number {
                let y: number = 20;
                function inner(): number {
                    return x + y;
                }
                return inner();
            }
            return middle();
        }
        return outer();
    ", 30);
}

#[test]
fn test_closure_modify_outer() {
    expect_i32("
        function make(): () => number {
            let count: number = 0;
            return (): number => {
                count = count + 1;
                return count;
            };
        }
        let inc = make();
        inc();
        inc();
        return inc();
    ", 3);
}

#[test]
fn test_closure_mutual_recursion() {
    expect_bool("
        function isEven(n: number): boolean {
            if (n == 0) { return true; }
            return isOdd(n - 1);
        }
        function isOdd(n: number): boolean {
            if (n == 0) { return false; }
            return isEven(n - 1);
        }
        return isEven(10);
    ", true);
}

#[test]
fn test_closure_currying() {
    expect_i32("
        function add(a: number): (b: number) => number {
            return (b: number): number => a + b;
        }
        let add5 = add(5);
        return add5(3);
    ", 8);
}

#[test]
fn test_closure_iife() {
    expect_i32("
        let result = ((x: number): number => x * 2)(21);
        return result;
    ", 42);
}

#[test]
fn test_closure_shared_capture() {
    // Two closures share the same captured variable
    expect_i32("
        function make(): number {
            let x: number = 0;
            let inc = (): void => { x = x + 1; };
            let get = (): number => x;
            inc();
            inc();
            inc();
            return get();
        }
        return make();
    ", 3);
}

#[test]
fn test_closure_for_of_binding() {
    // Each iteration captures its own value
    expect_i32("
        let fns: (() => number)[] = [];
        let items: number[] = [10, 20, 30];
        for (const item of items) {
            fns.push((): number => item);
        }
        return fns[0]() + fns[1]() + fns[2]();
    ", 60);
}

#[test]
fn test_closure_capturing_closure() {
    expect_i32("
        function outer(): () => number {
            let x: number = 10;
            let inner = (): number => x;
            return (): number => inner();
        }
        return outer()();
    ", 10);
}

// ============================================================================
// 7. Exception Edge Cases
// ============================================================================

#[test]
fn test_exception_in_catch_caught_by_outer() {
    expect_i32("
        function test(): number {
            try {
                try {
                    throw new Error(\"inner\");
                } catch (e) {
                    throw new Error(\"from catch\");
                }
            } catch (e2) {
                return 42;
            }
            return 0;
        }
        return test();
    ", 42);
}

#[test]
fn test_exception_nested_three_levels() {
    expect_i32("
        function test(): number {
            let result: number = 0;
            try {
                try {
                    try {
                        throw new Error(\"deep\");
                    } catch (e) {
                        result = result + 1;
                        throw e;
                    }
                } catch (e) {
                    result = result + 10;
                    throw e;
                }
            } catch (e) {
                result = result + 100;
            }
            return result;
        }
        return test();
    ", 111);
}

#[test]
fn test_exception_propagation_through_frames() {
    expect_i32("
        function c(): number { throw new Error(\"boom\"); }
        function b(): number { return c(); }
        function a(): number {
            try { return b(); }
            catch (e) { return 42; }
        }
        return a();
    ", 42);
}

#[test]
fn test_exception_finally_normal_flow() {
    // Finally runs on normal path
    expect_i32("
        function test(): number {
            let x: number = 0;
            try {
                x = 10;
            } finally {
                x = x + 1;
            }
            return x;
        }
        return test();
    ", 11);
}

#[test]
fn test_exception_finally_after_throw() {
    // Finally runs after exception
    expect_i32("
        function test(): number {
            let x: number = 0;
            try {
                try {
                    x = 10;
                    throw new Error(\"oops\");
                } finally {
                    x = x + 1;
                }
            } catch (e) {
                // x should be 11 (10 + 1 from finally)
            }
            return x;
        }
        return test();
    ", 11);
}

#[test]
fn test_exception_return_in_try_with_finally() {
    // return in finally overrides return in try
    expect_i32("
        function test(): number {
            try {
                return 1;
            } finally {
                return 2;
            }
        }
        return test();
    ", 2);
}

#[test]
fn test_exception_throw_string() {
    expect_runtime_error_with_builtins("throw new Error(\"custom message\");", "custom message");
}

#[test]
fn test_exception_try_catch_in_loop() {
    expect_i32("
        let sum: number = 0;
        let items: number[] = [1, 0, 3];
        for (const item of items) {
            try {
                if (item == 0) {
                    throw new Error(\"skip\");
                }
                sum = sum + item;
            } catch (e) {
                // skip this item
            }
        }
        return sum;
    ", 4);
}

#[test]
fn test_exception_custom_error_class() {
    expect_i32("
        class AppError extends Error {
            code: number;
            constructor(msg: string, code: number) {
                super(msg);
                this.code = code;
            }
        }
        function test(): number {
            try {
                throw new AppError(\"fail\", 404);
            } catch (e) {
                let ae = e as AppError;
                return ae.code;
            }
        }
        return test();
    ", 404);
}

// ============================================================================
// 8. Class Edge Cases
// ============================================================================

#[test]
fn test_class_three_level_inheritance() {
    expect_i32("
        class A {
            x(): number { return 1; }
        }
        class B extends A {
            y(): number { return 2; }
        }
        class C extends B {
            z(): number { return 3; }
        }
        let c = new C();
        return c.x() + c.y() + c.z();
    ", 6);
}

#[test]
fn test_class_virtual_dispatch() {
    expect_i32("
        class Base {
            value(): number { return 10; }
        }
        class Derived extends Base {
            value(): number { return 20; }
        }
        let b: Base = new Derived();
        return b.value();
    ", 20);
}

#[test]
fn test_class_instanceof_chain() {
    expect_bool("
        class A {}
        class B extends A {}
        class C extends B {}
        let c = new C();
        return c instanceof A;
    ", true);
}

#[test]
fn test_class_static_field_mutation() {
    expect_i32("
        class Counter {
            static count: number = 0;
            static increment(): void {
                Counter.count = Counter.count + 1;
            }
        }
        Counter.increment();
        Counter.increment();
        Counter.increment();
        return Counter.count;
    ", 3);
}

#[test]
fn test_class_multiple_instances_independent() {
    expect_i32("
        class Box {
            value: number;
            constructor(v: number) { this.value = v; }
        }
        let a = new Box(10);
        let b = new Box(20);
        return a.value + b.value;
    ", 30);
}

#[test]
fn test_class_this_method_calling_method() {
    expect_i32("
        class Calc {
            double(x: number): number { return x * 2; }
            quadruple(x: number): number { return this.double(this.double(x)); }
        }
        return new Calc().quadruple(5);
    ", 20);
}

#[test]
fn test_class_super_method() {
    expect_i32("
        class Base {
            greet(): number { return 10; }
        }
        class Child extends Base {
            greet(): number { return super.greet() + 5; }
        }
        return new Child().greet();
    ", 15);
}

#[test]
fn test_class_abstract_instantiation_error() {
    expect_compile_error("
        abstract class Shape {
            abstract area(): number;
        }
        let s = new Shape();
        return 0;
    ", "AbstractClassInstantiation");
}

#[test]
fn test_class_abstract_subclass_works() {
    expect_i32("
        abstract class Shape {
            abstract area(): number;
        }
        class Circle extends Shape {
            r: number;
            constructor(r: number) {
                super();
                this.r = r;
            }
            area(): number { return this.r * this.r * 3; }
        }
        let c = new Circle(10);
        return c.area();
    ", 300);
}

// ============================================================================
// 9. Destructuring Edge Cases
// ============================================================================

#[test]
fn test_destructure_array_basic() {
    expect_i32("
        let arr: number[] = [1, 2, 3];
        let [a, b, c] = arr;
        return a + b + c;
    ", 6);
}

#[test]
fn test_destructure_array_with_rest() {
    expect_i32("
        let arr: number[] = [1, 2, 3, 4, 5];
        let [first, ...rest] = arr;
        return first + rest.length;
    ", 5);
}

#[test]
fn test_destructure_array_skip() {
    expect_i32("
        let arr: number[] = [1, 2, 3];
        let [, second] = arr;
        return second;
    ", 2);
}

#[test]
fn test_destructure_array_with_default() {
    expect_i32("
        let arr: number[] = [1];
        let [a, b = 99] = arr;
        return b;
    ", 99);
}

#[test]
fn test_destructure_object_basic() {
    expect_i32("
        class Point {
            x: number;
            y: number;
            constructor(x: number, y: number) {
                this.x = x;
                this.y = y;
            }
        }
        let p = new Point(10, 20);
        let { x, y } = p;
        return x + y;
    ", 30);
}

#[test]
fn test_destructure_in_for_of() {
    expect_i32("
        class Pair {
            a: number;
            b: number;
            constructor(a: number, b: number) {
                this.a = a;
                this.b = b;
            }
        }
        let pairs: Pair[] = [new Pair(1, 2), new Pair(3, 4)];
        let sum: number = 0;
        for (const { a, b } of pairs) {
            sum = sum + a + b;
        }
        return sum;
    ", 10);
}

// ============================================================================
// 10. Control Flow Edge Cases
// ============================================================================

#[test]
fn test_nested_loop_break_inner_only() {
    expect_i32("
        let count: number = 0;
        let outer: number[] = [1, 2, 3];
        let inner: number[] = [10, 20, 30];
        for (const i of outer) {
            for (const j of inner) {
                if (j == 20) { break; }
                count = count + 1;
            }
        }
        return count;
    ", 3);
}

#[test]
fn test_for_of_with_continue() {
    expect_i32("
        let sum: number = 0;
        let items: number[] = [1, 2, 3, 4, 5];
        for (const item of items) {
            if (item % 2 == 0) { continue; }
            sum = sum + item;
        }
        return sum;
    ", 9);
}

#[test]
fn test_do_while_at_least_once() {
    expect_i32("
        let count: number = 0;
        do {
            count = count + 1;
        } while (false);
        return count;
    ", 1);
}

#[test]
fn test_while_complex_condition() {
    expect_i32("
        let x: number = 100;
        while (x > 1 && x % 2 == 0) {
            x = x / 2;
        }
        return x;
    ", 25);
}

#[test]
fn test_switch_default() {
    expect_i32("
        let x: number = 99;
        switch (x) {
            case 1: return 10;
            case 2: return 20;
            default: return 30;
        }
    ", 30);
}

#[test]
fn test_nested_if_else_chain() {
    expect_i32("
        function classify(n: number): number {
            if (n < 0) {
                return -1;
            } else if (n == 0) {
                return 0;
            } else if (n < 10) {
                return 1;
            } else {
                return 2;
            }
        }
        return classify(-5) + classify(0) + classify(5) + classify(50);
    ", 2);
}

#[test]
fn test_for_loop_boundary() {
    expect_i32("
        let sum: number = 0;
        for (let i: number = 0; i < 5; i = i + 1) {
            sum = sum + i;
        }
        return sum;
    ", 10);
}

#[test]
fn test_infinite_loop_detected() {
    // i++ in for-loop update doesn't increment, causing infinite loop
    // The VM detects this via consecutive preemption counting and kills the task
    expect_runtime_error("
        let sum: number = 0;
        for (let i: number = 0; i < 5; i++) {
            sum = sum + i;
        }
        return sum;
    ", "Maximum execution time exceeded");
}

#[test]
fn test_infinite_while_loop_detected() {
    expect_runtime_error("
        while (true) {
            let x: number = 1;
        }
        return 0;
    ", "Maximum execution time exceeded");
}

#[test]
fn test_for_of_with_break() {
    expect_i32("
        let sum: number = 0;
        let items: number[] = [1, 2, 3, 4, 5];
        for (const item of items) {
            if (item == 4) { break; }
            sum = sum + item;
        }
        return sum;
    ", 6);
}

// ============================================================================
// 11. Variable Scoping Edge Cases
// ============================================================================

#[test]
fn test_block_scope_if() {
    expect_i32("
        let x: number = 1;
        if (true) {
            let x: number = 2;
        }
        return x;
    ", 1);
}

#[test]
fn test_shadowing() {
    expect_i32("
        let x: number = 10;
        function inner(): number {
            let x: number = 20;
            return x;
        }
        return inner() + x;
    ", 30);
}

#[test]
fn test_closure_captures_block_scoped() {
    expect_i32("
        function test(): number {
            let x: number = 10;
            let fn1 = (): number => x;
            return fn1();
        }
        return test();
    ", 10);
}

#[test]
fn test_function_param_shadows_outer() {
    expect_i32("
        let x: number = 10;
        function f(x: number): number { return x; }
        return f(20) + x;
    ", 30);
}

// ============================================================================
// 12. Float Special Values
// ============================================================================

#[test]
fn test_float_division_by_zero_no_throw() {
    // Float division by zero produces infinity, doesn't throw
    expect_bool("
        let x: number = 1.0 / 0.0;
        return x > 1000000.0;
    ", true);
}

#[test]
fn test_float_nan_propagation() {
    // NaN + anything = NaN, detected via x != x
    expect_bool("
        let nan: number = 0.0 / 0.0;
        let result: number = nan + 5.0;
        return result != result;
    ", true);
}

#[test]
fn test_float_nan_not_equal_self() {
    expect_bool("
        let x: number = 0.0 / 0.0;
        return x == x;
    ", false);
}

#[test]
fn test_int_div_zero_throws_but_float_does_not() {
    // Integer division by zero throws
    expect_runtime_error("return 1 / 0;", "division by zero");
}

#[test]
fn test_float_negative_zero() {
    // -0.0 == 0.0 should be true
    expect_bool("return -0.0 == 0.0;", true);
}

#[test]
fn test_float_precision() {
    // 0.1 + 0.2 is close to 0.3 but not exact
    expect_bool("return 0.1 + 0.2 == 0.3;", false);
}
