//! End-to-end tests for rest parameters
//!
//! Tests the rest parameter feature: `function foo(...args: T[])`

use super::harness::*;

// ============================================================================
// Basic Rest Parameter Tests
// ============================================================================

#[test]
fn test_basic_rest_parameter() {
    expect_i32(
        "function sum(...numbers: number[]): number {
            let total = 0;
            for (let n of numbers) {
                total += n;
            }
            return total;
        }
        return sum(1, 2, 3);",
        6,
    );

    expect_i32(
        "function sum(...numbers: number[]): number {
            let total = 0;
            for (let n of numbers) {
                total += n;
            }
            return total;
        }
        return sum();",
        0,
    );

    expect_i32(
        "function sum(...numbers: number[]): number {
            let total = 0;
            for (let n of numbers) {
                total += n;
            }
            return total;
        }
        return sum(10);",
        10,
    );

    expect_i32(
        "function sum(...numbers: number[]): number {
            let total = 0;
            for (let n of numbers) {
                total += n;
            }
            return total;
        }
        return sum(1, 2, 3, 4, 5);",
        15,
    );
}

#[test]
fn test_rest_with_fixed_parameters() {
    // Test that fixed parameters work alongside rest parameters
    expect_i32(
        "function greet(greeting: string, ...names: string[]): number {
            let count = 0;
            for (let name of names) {
                count += 1;
            }
            return count;
        }
        return greet('Hello', 'Alice', 'Bob');",
        2,
    );
}

#[test]
fn test_rest_parameter_empty() {
    expect_i32(
        "function countAll(...items: number[]): number {
            let count = 0;
            for (let _ of items) {
                count += 1;
            }
            return count;
        }
        return countAll();",
        0,
    );

    expect_i32(
        "function countAll(...items: number[]): number {
            let count = 0;
            for (let _ of items) {
                count += 1;
            }
            return count;
        }
        return countAll(1);",
        1,
    );

    expect_i32(
        "function countAll(...items: number[]): number {
            let count = 0;
            for (let _ of items) {
                count += 1;
            }
            return count;
        }
        return countAll(1, 2);",
        2,
    );
}

#[test]
fn test_rest_parameter_array_methods() {
    expect_string(
        "function joinAll(...parts: string[]): string {
            return parts.join(', ');
        }
        return joinAll('a', 'b', 'c');",
        "a, b, c",
    );

    expect_string(
        "function joinAll(...parts: string[]): string {
            return parts.join(', ');
        }
        return joinAll('hello');",
        "hello",
    );
}

#[test]
fn test_rest_parameter_with_array_operations() {
    expect_i32(
        "function getFirst(...numbers: number[]): number {
            if (numbers.length === 0) {
                return 0;
            }
            return numbers[0];
        }
        return getFirst(1, 2, 3);",
        1,
    );

    expect_i32(
        "function getFirst(...numbers: number[]): number {
            if (numbers.length === 0) {
                return 0;
            }
            return numbers[0];
        }
        return getFirst(42);",
        42,
    );

    expect_i32(
        "function getFirst(...numbers: number[]): number {
            if (numbers.length === 0) {
                return 0;
            }
            return numbers[0];
        }
        return getFirst();",
        0,
    );
}

#[test]
fn test_rest_parameter_in_nested_function() {
    expect_i32(
        "function outer(x: number): number {
            function inner(...rest: number[]): number {
                let sum = x;
                for (let n of rest) {
                    sum += n;
                }
                return sum;
            }
            return inner(1, 2, 3);
        }
        return outer(1);",
        7,  // 1 (x) + 1 + 2 + 3
    );
}

#[test]
fn test_rest_parameter_in_method() {
    expect_i32(
        "class Calculator {
            sum(...numbers: number[]): number {
                let total = 0;
                for (let n of numbers) {
                    total += n;
                }
                return total;
            }
        }
        let calc = new Calculator();
        return calc.sum(1, 2, 3);",
        6,
    );

    expect_i32(
        "class Calculator {
            sum(...numbers: number[]): number {
                let total = 0;
                for (let n of numbers) {
                    total += n;
                }
                return total;
            }
        }
        let calc = new Calculator();
        return calc.sum();",
        0,
    );
}

#[test]
fn test_rest_parameter_with_string_type() {
    expect_string(
        "function concatenate(...strings: string[]): string {
            let result = '';
            for (let s of strings) {
                result += s;
            }
            return result;
        }
        return concatenate('a', 'b', 'c');",
        "abc",
    );
}

#[test]
fn test_rest_parameter_large_number_of_args() {
    expect_i32(
        "function countAll(...items: number[]): number {
            return items.length;
        }
        return countAll(
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
            11, 12, 13, 14, 15, 16, 17, 18, 19, 20
        );",
        20,
    );
}

#[test]
fn test_rest_parameter_mutation() {
    expect_i32(
        "function modifyRest(...nums: number[]): number {
            nums[0] = 999;
            return nums[0];
        }
        return modifyRest(1, 2, 3);",
        999,
    );
}

#[test]
fn test_rest_parameter_return_type() {
    expect_i32(
        "function getRest(...items: string[]): number {
            return items.length;
        }
        return getRest('a', 'b', 'c');",
        3,
    );
}

#[test]
fn test_rest_parameter_arrow_function() {
    expect_i32(
        "let sum = (...numbers: number[]): number => {
            let total = 0;
            for (let n of numbers) {
                total += n;
            }
            return total;
        };
        return sum(1, 2, 3);",
        6,
    );

    expect_i32(
        "let sum = (...numbers: number[]): number => {
            let total = 0;
            for (let n of numbers) {
                total += n;
            }
            return total;
        };
        return sum();",
        0,
    );
}

#[test]
fn test_rest_parameter_in_expression_position() {
    expect_i32(
        "let compute = (...nums: number[]): number => {
            let result = 0;
            for (let n of nums) {
                result += n;
            }
            return result;
        };
        let values = compute(10, 20, 30);
        return values;",
        60,
    );
}

#[test]
fn test_rest_parameter_with_boolean_type() {
    expect_bool(
        "function allTrue(...values: boolean[]): boolean {
            for (let v of values) {
                if (!v) {
                    return false;
                }
            }
            return true;
        }
        return allTrue(true, true, true);",
        true,
    );

    expect_bool(
        "function allTrue(...values: boolean[]): boolean {
            for (let v of values) {
                if (!v) {
                    return false;
                }
            }
            return true;
        }
        return allTrue(true, false, true);",
        false,
    );
}

#[test]
fn test_rest_parameter_multiple_fixed_params() {
    expect_i32(
        "function add(a: number, b: number, ...rest: number[]): number {
            let sum = a + b;
            for (let n of rest) {
                sum += n;
            }
            return sum;
        }
        return add(1, 2, 3, 4, 5);",
        15,  // 1 + 2 + 3 + 4 + 5
    );

    expect_i32(
        "function add(a: number, b: number, ...rest: number[]): number {
            let sum = a + b;
            for (let n of rest) {
                sum += n;
            }
            return sum;
        }
        return add(1, 2);",
        3,  // 1 + 2, no rest args
    );
}

#[test]
fn test_rest_parameter_array_indexing() {
    expect_i32(
        "function getItems(...items: number[]): number {
            return items.length;
        }
        return getItems(10, 20, 30, 40);",
        4,
    );
}

#[test]
fn test_rest_parameter_array_iteration() {
    expect_i32(
        "function sumFirstTwo(...nums: number[]): number {
            let count = 0;
            let sum = 0;
            for (let n of nums) {
                if (count < 2) {
                    sum += n;
                    count += 1;
                }
            }
            return sum;
        }
        return sumFirstTwo(5, 10, 15, 20);",
        15,  // 5 + 10
    );
}

#[test]
fn test_rest_parameter_type_inference() {
    // Test that the compiler correctly infers rest parameter array types
    expect_string(
        "function join(...strings: string[]): string {
            return strings.join('-');
        }
        return join('a', 'b', 'c');",
        "a-b-c",
    );
}

#[test]
fn test_rest_parameter_in_generic_context() {
    // This test verifies rest parameters work with type parameters
    expect_i32(
        "function first<T>(...items: T[]): T {
            if (items.length === 0) {
                throw 'empty array';
            }
            return items[0];
        }
        return first(1, 2, 3);",
        1,
    );
}

#[test]
fn test_rest_parameter_with_any_type() {
    // Test rest parameter with union type for mixed types
    expect_i32(
        "type Mixed = number | string | boolean;
        function logAll(...items: Mixed[]): number {
            return items.length;
        }
        return logAll(42, 'hello', true);",
        3,
    );
}

#[test]
fn test_rest_array_length() {
    // Test that rest array has correct length
    expect_i32(
        "function test(...args: number[]): number {
            return args.length;
        }
        return test(1, 2, 3);",
        3,
    );
}

#[test]
fn test_rest_array_access() {
    // Test accessing individual elements of rest array
    expect_i32(
        "function test(...args: number[]): number {
            return args[0] + args[1] + args[2];
        }
        return test(1, 2, 3);",
        6,
    );
}

#[test]
fn test_rest_first_element() {
    // Test accessing first element
    expect_i32(
        "function test(...args: number[]): number {
            return args[0];
        }
        return test(42);",
        42,
    );
}

#[test]
fn test_rest_manual_loop() {
    // Test manual loop instead of for-of
    expect_i32(
        "function sum(...numbers: number[]): number {
            let total = 0;
            let i = 0;
            while (i < numbers.length) {
                total = total + numbers[i];
                i = i + 1;
            }
            return total;
        }
        return sum(1, 2);",
        3,
    );
}

#[test]
fn test_single_arg() {
    // Test with single argument
    expect_i32(
        "function test(...args: number[]): number {
            return args[0];
        }
        return test(99);",
        99,
    );
}

#[test]
fn test_fixed_with_rest() {
    // Test fixed parameter before rest parameter
    expect_i32(
        "function test(first: number, ...rest: number[]): number {
            return first;
        }
        return test(42, 1, 2);",
        42,
    );
}

#[test]
fn test_rest_array_with_fixed() {
    // Test rest array when there's a fixed parameter
    expect_i32(
        "function test(first: number, ...rest: number[]): number {
            return rest.length;
        }
        return test(42, 1, 2);",
        2,
    );
}

#[test]
fn test_rest_first_two_elements() {
    // Test accessing first two elements individually
    expect_i32(
        "function test(...args: number[]): number {
            let a = args[0];
            let b = args[1];
            return a + b;
        }
        return test(10, 20);",
        30,
    );
}

#[test]
fn test_rest_second_element() {
    // Test accessing second element
    expect_i32(
        "function test(...args: number[]): number {
            return args[1];
        }
        return test(1, 99);",
        99,
    );
}

#[test]
fn test_rest_for_of_loop() {
    // Test for-of iteration (the original failing test)
    expect_i32(
        "function sum(...numbers: number[]): number {
            let total = 0;
            for (let n of numbers) {
                total += n;
            }
            return total;
        }
        return sum(1, 2);",
        3,
    );
}
