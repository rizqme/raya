//! Syntax parsing edge case tests
//!
//! Tests that exercise complex and intersecting syntax features
//! to expose parser, type checker, and lowering bugs.

use super::{expect_bool, expect_compile_error, expect_i32, expect_null, expect_string};

// ============================================================================
// 1. Arrow Function Forms
// ============================================================================

#[test]
fn test_arrow_as_function_argument() {
    // Arrow passed as callback to a higher-order function
    expect_i32(
        "
        function apply(f: (x: number) => number, val: number): number {
            return f(val);
        }
        return apply((x: number): number => x * 2, 21);
    ",
        42,
    );
}

#[test]
fn test_arrow_returning_arrow() {
    // Currying: arrow returning arrow, called with two invocations
    expect_i32(
        "
        let add = (x: number): (y: number) => number => {
            return (y: number): number => x + y;
        };
        let add10 = add(10);
        return add10(32);
    ",
        42,
    );
}

#[test]
fn test_nested_arrow_three_levels() {
    // Three levels of nested arrows
    expect_i32(
        "
        let f = (a: number): (b: number) => (c: number) => number => {
            return (b: number): (c: number) => number => {
                return (c: number): number => a + b + c;
            };
        };
        return f(10)(20)(12);
    ",
        42,
    );
}

#[test]
fn test_arrow_with_default_param() {
    // Function with default parameter value — called with fewer args
    expect_i32(
        "
        function add(x: number, y: number = 10): number {
            return x + y;
        }
        return add(32);
    ",
        42,
    );
}

#[test]
fn test_arrow_block_body_multiple_returns() {
    // Arrow with block body containing conditional returns
    expect_i32(
        "
        let classify = (x: number): number => {
            if (x > 0) {
                return 1;
            }
            if (x < 0) {
                return -1;
            }
            return 0;
        };
        return classify(5) + classify(-3) + classify(0);
    ",
        0,
    );
}

#[test]
fn test_arrow_immediately_invoked() {
    // IIFE with arrow function
    expect_i32(
        "
        return ((x: number): number => x + 1)(41);
    ",
        42,
    );
}

#[test]
fn test_arrow_in_array_literal() {
    // Array of arrow functions
    expect_i32(
        "
        let fns: ((x: number) => number)[] = [
            (x: number): number => x + 1,
            (x: number): number => x + 2,
            (x: number): number => x + 3
        ];
        return fns[0](10) + fns[1](10) + fns[2](10);
    ",
        36,
    );
}

#[test]
fn test_arrow_in_ternary_result() {
    // Arrow function selected by ternary
    expect_i32(
        "
        let flag: boolean = true;
        let f = flag ? (x: number): number => x + 1 : (x: number): number => x - 1;
        return f(41);
    ",
        42,
    );
}

// ============================================================================
// 2. Nested Declarations
// ============================================================================

#[test]
fn test_function_inside_function() {
    // Inner function defined and called inside outer
    expect_i32(
        "
        function outer(x: number): number {
            function inner(y: number): number {
                return y * 2;
            }
            return inner(x) + 1;
        }
        return outer(20);
    ",
        41,
    );
}

#[test]
fn test_function_inside_function_with_capture() {
    // Inner function accesses outer's parameter
    expect_i32(
        "
        function outer(x: number): number {
            function inner(y: number): number {
                return x + y;
            }
            return inner(32);
        }
        return outer(10);
    ",
        42,
    );
}

#[test]
fn test_two_nested_functions() {
    // Two inner functions in same outer function
    expect_i32(
        "
        function compute(x: number): number {
            function double(n: number): number { return n * 2; }
            function addOne(n: number): number { return n + 1; }
            return addOne(double(x));
        }
        return compute(20);
    ",
        41,
    );
}

#[test]
fn test_class_inside_function() {
    // Class defined and used inside a function body
    expect_i32(
        "
        function makeValue(): number {
            class Box {
                value: number = 0;
                constructor(v: number) {
                    this.value = v;
                }
                get(): number {
                    return this.value;
                }
            }
            let b = new Box(42);
            return b.get();
        }
        return makeValue();
    ",
        42,
    );
}

#[test]
fn test_class_inside_function_with_methods() {
    // Class with multiple methods defined inside function
    expect_i32(
        "
        function compute(): number {
            class Calc {
                x: number = 0;
                constructor(x: number) { this.x = x; }
                double(): number { return this.x * 2; }
                addTo(other: number): number { return this.x + other; }
            }
            let c = new Calc(10);
            return c.double() + c.addTo(5);
        }
        return compute();
    ",
        35,
    );
}

#[test]
fn test_function_inside_if_block() {
    // Function declared inside if body
    expect_i32(
        "
        let x: number = 10;
        let result: number = 0;
        if (x > 5) {
            function helper(n: number): number { return n * 3; }
            result = helper(x);
        }
        return result;
    ",
        30,
    );
}

#[test]
fn test_function_inside_for_loop() {
    // Function declared and called inside for loop
    expect_i32(
        "
        let sum: number = 0;
        let items: number[] = [1, 2, 3, 4, 5];
        for (let item of items) {
            function square(n: number): number { return n * n; }
            sum = sum + square(item);
        }
        return sum;
    ",
        55,
    );
}

#[test]
fn test_class_extending_inside_function() {
    // Two classes with inheritance inside a function
    expect_i32(
        "
        function create(): number {
            class Base {
                x: number = 0;
                constructor(x: number) { this.x = x; }
                value(): number { return this.x; }
            }
            class Child extends Base {
                y: number = 0;
                constructor(x: number, y: number) {
                    super(x);
                    this.y = y;
                }
                total(): number { return this.x + this.y; }
            }
            let c = new Child(10, 32);
            return c.total();
        }
        return create();
    ",
        42,
    );
}

// ============================================================================
// 3. Optional Chaining
// ============================================================================

#[test]
fn test_optional_chain_on_object() {
    // Optional chaining on an object that has the property
    expect_i32(
        "
        class Box { value: number = 42; }
        let b = new Box();
        return b?.value;
    ",
        42,
    );
}

#[test]
fn test_optional_chain_on_null_object() {
    expect_i32(
        "
        class Box { value: number = 42; }
        let b: Box | null = null;
        let v = b?.value;
        if (v === null) { return 42; }
        return 0;
    ",
        42,
    );
}

#[test]
fn test_optional_chain_with_nullish_coalescing() {
    // Optional chaining with ?? fallback
    expect_i32(
        "
        class Box { value: number = 42; }
        let b = new Box();
        return b?.value ?? 0;
    ",
        42,
    );
}

#[test]
fn test_optional_chain_deep() {
    // Multi-level optional chaining
    expect_i32(
        "
        class Inner { value: number = 42; }
        class Middle { inner: Inner = new Inner(); }
        class Outer { middle: Middle = new Middle(); }
        let o = new Outer();
        return o?.middle?.inner?.value;
    ",
        42,
    );
}

#[test]
fn test_optional_chain_deep_null_short_circuit() {
    expect_i32(
        "
        class Inner { value: number = 42; }
        class Middle { inner: Inner = new Inner(); }
        class Outer { middle: Middle = new Middle(); }
        let o: Outer | null = null;
        let v = o?.middle?.inner?.value;
        if (v === null) { return 42; }
        return 0;
    ",
        42,
    );
}

#[test]
fn test_optional_chain_mixed_with_regular() {
    // Regular access followed by optional chaining
    expect_i32(
        "
        class Inner { value: number = 42; }
        class Outer { inner: Inner = new Inner(); }
        let o = new Outer();
        return o.inner?.value;
    ",
        42,
    );
}

// ============================================================================
// 4. Operator Precedence
// ============================================================================

#[test]
fn test_nested_ternary_right_associative() {
    // a ? 1 : b ? 2 : 3 should parse as a ? 1 : (b ? 2 : 3)
    expect_i32(
        "
        let a: boolean = false;
        let b: boolean = true;
        return a ? 1 : b ? 2 : 3;
    ",
        2,
    );
}

#[test]
fn test_ternary_with_assignment() {
    // Assignment of ternary result
    expect_i32(
        "
        let flag: boolean = true;
        let x: number = flag ? 42 : 0;
        return x;
    ",
        42,
    );
}

#[test]
fn test_exponentiation_right_associative() {
    // 2 ** 3 ** 2 = 2 ** (3 ** 2) = 2 ** 9 = 512
    expect_i32(
        "
        return 2 ** 3 ** 2;
    ",
        512,
    );
}

#[test]
fn test_exponentiation_with_unary() {
    // -(2 ** 3) = -8
    expect_i32(
        "
        return -(2 ** 3);
    ",
        -8,
    );
}

#[test]
fn test_nullish_coalescing_chain() {
    // null ?? null ?? 42 → 42 (left-to-right, first non-null wins)
    expect_i32(
        "
        let a: number | null = null;
        let b: number | null = null;
        let c: number = 42;
        return a ?? b ?? c;
    ",
        42,
    );
}

#[test]
fn test_mixed_logical_short_circuit() {
    // false && true || true = (false && true) || true = false || true = true
    expect_bool(
        "
        return false && true || true;
    ",
        true,
    );
}

#[test]
fn test_comparison_with_arithmetic() {
    // Arithmetic has higher precedence than comparison
    // (1 + 2) < (3 + 4) = 3 < 7 = true → 1
    expect_i32(
        "
        let a: number = 1 + 2;
        let b: number = 3 + 4;
        let result: boolean = a < b;
        return result ? 1 : 0;
    ",
        1,
    );
}

#[test]
fn test_ternary_nested_both_branches() {
    // Ternary in both consequent and alternate
    expect_i32(
        "
        let a: boolean = true;
        let b: boolean = false;
        return a ? (b ? 1 : 2) : (b ? 3 : 4);
    ",
        2,
    );
}

// ============================================================================
// 5. Complex Destructuring
// ============================================================================

#[test]
fn test_nested_array_destructuring() {
    // Destructuring nested arrays
    expect_i32(
        "
        let arr: number[][] = [[1, 2], [3, 4]];
        let [[a, b], [c, d]] = arr;
        return a + b + c + d;
    ",
        10,
    );
}

#[test]
fn test_array_destructure_with_rest() {
    // Rest element in array destructuring
    expect_i32(
        "
        let arr: number[] = [1, 2, 3, 4, 5];
        let [first, ...rest] = arr;
        return first + rest.length;
    ",
        5,
    );
}

#[test]
fn test_object_destructure_with_rename() {
    // Object destructuring with renamed bindings
    expect_i32(
        "
        let obj = { x: 10, y: 32 };
        let { x: a, y: b } = obj;
        return a + b;
    ",
        42,
    );
}

#[test]
fn test_object_destructure_with_default() {
    // Object destructuring with default values
    expect_i32(
        "
        let obj = { x: 42 };
        let { x = 0, y = 10 } = obj;
        return x + y;
    ",
        52,
    );
}

#[test]
fn test_object_spread_basic_merge() {
    expect_i32(
        "
        let base = { x: 10, y: 20 };
        let merged = { a: 1, ...base };
        return merged.a + merged.x + merged.y;
    ",
        31,
    );
}

#[test]
fn test_object_spread_override_order() {
    expect_i32(
        "
        let base = { x: 10, y: 20 };
        let merged = { x: 1, ...base, y: 99 };
        return merged.x + merged.y;
    ",
        109,
    );
}

#[test]
fn test_typed_object_spread_filters_extra_fields() {
    expect_i32(
        "
        type Pair = { x: number; y: number };
        let source = { x: 10, y: 20, z: 999 };
        let pair: Pair = { ...source };
        return pair.x + pair.y;
    ",
        30,
    );
}

#[test]
fn test_typed_object_spread_still_type_checks() {
    expect_compile_error(
        "
        type Pair = { x: number; y: number };
        let source = { x: 10 };
        let pair: Pair = { ...source };
    ",
        "TypeMismatch",
    );
}

#[test]
fn test_destructure_in_for_of_array() {
    // Array destructuring in for-of
    expect_i32(
        "
        let pairs: number[][] = [[1, 10], [2, 20], [3, 30]];
        let sum: number = 0;
        for (let [a, b] of pairs) {
            sum = sum + a + b;
        }
        return sum;
    ",
        66,
    );
}

#[test]
fn test_destructure_array_skip_elements() {
    // Skip elements with holes in destructuring
    expect_i32(
        "
        let arr: number[] = [10, 20, 30, 40];
        let [, second, , fourth] = arr;
        return second + fourth;
    ",
        60,
    );
}

#[test]
fn test_destructure_with_rest_sum() {
    // Rest destructuring and computing with rest
    expect_i32(
        "
        let arr: number[] = [10, 20, 30, 40, 50];
        let [head, ...tail] = arr;
        let sum: number = head;
        for (let t of tail) {
            sum = sum + t;
        }
        return sum;
    ",
        150,
    );
}

// ============================================================================
// Const Destructuring
// ============================================================================

#[test]
fn test_const_object_destructuring_basic() {
    // Basic const object destructuring
    expect_i32(
        "
        const obj = { x: 10, y: 20 };
        const { x, y } = obj;
        return x + y;
    ",
        30,
    );
}

#[test]
fn test_const_object_destructuring_with_rename() {
    // Const object destructuring with renaming
    expect_i32(
        "
        const obj = { x: 10, y: 32 };
        const { x: a, y: b } = obj;
        return a + b;
    ",
        42,
    );
}

#[test]
fn test_const_object_destructuring_with_default() {
    // Const object destructuring with default values
    expect_i32(
        "
        const obj = { x: 42 };
        const { x = 0, y = 10 } = obj;
        return x + y;
    ",
        52,
    );
}

#[test]
fn test_const_array_destructuring_basic() {
    // Basic const array destructuring
    expect_i32(
        "
        const arr: number[] = [10, 20, 30];
        const [a, b, c] = arr;
        return a + b + c;
    ",
        60,
    );
}

#[test]
fn test_const_array_destructuring_with_rest() {
    // Const array destructuring with rest element
    expect_i32(
        "
        const arr: number[] = [1, 2, 3, 4, 5];
        const [first, ...rest] = arr;
        return first + rest.length;
    ",
        5,
    );
}

#[test]
fn test_const_nested_destructuring() {
    // Const nested destructuring
    expect_i32(
        "
        const data = { points: [{ x: 1, y: 2 }, { x: 3, y: 4 }] };
        const { points: [{ x: x1, y: y1 }, { x: x2, y: y2 }] } = data;
        return x1 + y1 + x2 + y2;
    ",
        10,
    );
}

#[test]
fn test_const_destructuring_at_module_scope() {
    // Const destructuring at module scope
    expect_i32(
        "
        const { a, b } = { a: 15, b: 27 };
        function sum(): number { return a + b; }
        return sum();
    ",
        42,
    );
}

// ============================================================================
// Function Parameter Destructuring
// ============================================================================

#[test]
fn test_function_param_object_destructuring() {
    // Function parameter with object destructuring
    expect_i32(
        "
        function add({ x, y }: { x: number; y: number }): number {
            return x + y;
        }
        const point = { x: 10, y: 32 };
        return add(point);
    ",
        42,
    );
}

#[test]
fn test_function_param_array_destructuring() {
    // Function parameter with array destructuring
    expect_i32(
        "
        function sum([a, b, c]: number[]): number {
            return a + b + c;
        }
        const arr: number[] = [10, 20, 12];
        return sum(arr);
    ",
        42,
    );
}

#[test]
fn test_function_param_destructuring_with_rename() {
    // Function parameter destructuring with renaming
    expect_i32(
        "
        function multiply({ x: a, y: b }: { x: number; y: number }): number {
            return a * b;
        }
        const point = { x: 6, y: 7 };
        return multiply(point);
    ",
        42,
    );
}

#[test]
fn test_function_param_destructuring_nested() {
    // Function parameter with nested destructuring
    expect_i32(
        "
        function getFirstY({ points: [{ y }] }: { points: { y: number }[] }): number {
            return y;
        }
        const data = { points: [{ y: 42 }, { y: 10 }] };
        return getFirstY(data);
    ",
        42,
    );
}

#[test]
fn test_arrow_function_param_destructuring() {
    // Arrow function with destructured parameter
    expect_i32(
        "
        const add = ({ x, y }: { x: number; y: number }): number => x + y;
        return add({ x: 10, y: 32 });
    ",
        42,
    );
}

#[test]
fn test_method_param_destructuring() {
    // Class method with destructured parameter
    expect_i32(
        "
        class Calculator {
            add({ x, y }: { x: number; y: number }): number {
                return x + y;
            }
        }
        const calc = new Calculator();
        return calc.add({ x: 15, y: 27 });
    ",
        42,
    );
}

#[test]
fn test_destructured_param_with_optional() {
    // Destructured parameter with optional property
    expect_i32(
        "
        function getValue({ x, y = 10 }: { x: number; y?: number }): number {
            return x + y;
        }
        return getValue({ x: 32 });
    ",
        42,
    );
}

#[test]
fn test_destructured_param_with_provided_optional() {
    // Destructured parameter with provided optional value
    expect_i32(
        "
        function getValue({ x, y = 10 }: { x: number; y?: number }): number {
            return x + y;
        }
        return getValue({ x: 30, y: 12 });
    ",
        42,
    );
}

// ============================================================================
// Destructuring Edge Cases
// ============================================================================

#[test]
fn test_catch_clause_destructuring() {
    // Destructuring in catch clause parameter
    expect_i32(
        "
        class CustomError {
            code: number;
            message: string;
            constructor(code: number, message: string) {
                this.code = code;
                this.message = message;
            }
        }
        function test(): number {
            try {
                throw new CustomError(42, \"error\");
            } catch (e) {
                const err = e as CustomError;
                const { code, message } = err;
                return code;
            }
            return 0;
        }
        return test();
    ",
        42,
    );
}

#[test]
fn test_destructuring_mixed_object_array() {
    // Mixed object and array destructuring
    expect_i32(
        "
        const data = { items: [[1, 2], [3, 4]] };
        const { items: [[a, b], [c, d]] } = data;
        return a + b + c + d;
    ",
        10,
    );
}

#[test]
fn test_const_destructuring_in_closure() {
    // Const destructuring captured by closure
    expect_i32(
        "
        function makeAdder(): () => number {
            const { x, y } = { x: 10, y: 32 };
            return (): number => x + y;
        }
        const adder = makeAdder();
        return adder();
    ",
        42,
    );
}

#[test]
fn test_destructuring_with_rest_property() {
    // Object destructuring with rest property
    expect_i32(
        "
        const obj = { a: 1, b: 2, c: 3, d: 4 };
        const { a, b, ...rest } = obj;
        return a + b;  // rest is ignored but rest properties exist
    ",
        3,
    );
}

#[test]
fn test_destructuring_in_arrow_body() {
    // Destructuring inside arrow function body
    expect_i32(
        "
        const process = (obj: { x: number; y: number }): number => {
            const { x, y } = obj;
            return x + y;
        };
        return process({ x: 15, y: 27 });
    ",
        42,
    );
}

#[test]
fn test_multiple_destructuring_same_scope() {
    // Multiple destructuring declarations in same scope
    expect_i32(
        "
        const { a } = { a: 10 };
        const { b } = { b: 20 };
        const { c } = { c: 12 };
        return a + b + c;
    ",
        42,
    );
}

#[test]
fn test_destructuring_shadowing() {
    // Destructuring can shadow outer variables
    expect_i32(
        "
        let x = 100;
        {
            const { x } = { x: 42 };
            return x;
        }
    ",
        42,
    );
}

// ============================================================================
// 6. Async/Await in Complex Positions
// ============================================================================

#[test]
fn test_await_in_binary_expression() {
    // Await used in both sides of binary expression
    expect_i32(
        "
        async function getA(): Promise<number> { return 10; }
        async function getB(): Promise<number> { return 32; }
        async function main(): Promise<number> {
            return await getA() + await getB();
        }
        return await main();
    ",
        42,
    );
}

#[test]
fn test_await_in_ternary() {
    // Await in both branches of ternary
    expect_i32(
        "
        async function yes(): Promise<number> { return 42; }
        async function no(): Promise<number> { return 0; }
        async function main(): Promise<number> {
            let cond: boolean = true;
            return cond ? await yes() : await no();
        }
        return await main();
    ",
        42,
    );
}

#[test]
fn test_await_as_function_argument() {
    // Await result passed as argument
    expect_i32(
        "
        function add(a: number, b: number): number { return a + b; }
        async function getX(): Promise<number> { return 10; }
        async function getY(): Promise<number> { return 32; }
        async function main(): Promise<number> {
            return add(await getX(), await getY());
        }
        return await main();
    ",
        42,
    );
}

#[test]
fn test_await_in_comparison() {
    // Await in comparison expression
    expect_bool(
        "
        async function getCount(): Promise<number> { return 10; }
        async function main(): Promise<boolean> {
            return await getCount() > 5;
        }
        return await main();
    ",
        true,
    );
}

#[test]
fn test_await_chained_method() {
    // Await on method call result
    expect_i32(
        "
        class AsyncProvider {
            value: number = 0;
            constructor(v: number) { this.value = v; }
            async getValue(): Promise<number> { return this.value; }
        }
        async function main(): Promise<number> {
            let p = new AsyncProvider(42);
            return await p.getValue();
        }
        return await main();
    ",
        42,
    );
}

#[test]
fn test_await_in_array_literal() {
    // Await results collected into array
    expect_i32(
        "
        async function getA(): Promise<number> { return 10; }
        async function getB(): Promise<number> { return 20; }
        async function main(): Promise<number> {
            let a: number = await getA();
            let b: number = await getB();
            let arr: number[] = [a, b];
            return arr[0] + arr[1];
        }
        return await main();
    ",
        30,
    );
}

#[test]
fn test_multiple_awaits_in_expression() {
    // Three awaits in a single arithmetic expression
    expect_i32(
        "
        async function a(): Promise<number> { return 10; }
        async function b(): Promise<number> { return 20; }
        async function c(): Promise<number> { return 12; }
        async function main(): Promise<number> {
            return await a() + await b() + await c();
        }
        return await main();
    ",
        42,
    );
}

#[test]
fn test_async_arrow_as_variable() {
    // Async arrow stored in variable and called
    expect_i32(
        "
        async function main(): Promise<number> {
            let compute = async (): Promise<number> => {
                return 42;
            };
            return await compute();
        }
        return await main();
    ",
        42,
    );
}

// ============================================================================
// 7. Template Literal Edge Cases
// ============================================================================

#[test]
fn test_template_with_arrow_call() {
    // Template with immediately invoked arrow
    expect_string(
        "
        return `${((x: number): number => x * 2)(21)}`;
    ",
        "42",
    );
}

#[test]
fn test_template_with_binary_expr() {
    // Template with complex arithmetic expression
    expect_string(
        "
        let a: number = 2;
        let b: number = 3;
        let c: number = 4;
        return `${a + b * c}`;
    ",
        "14",
    );
}

#[test]
fn test_template_multiple_interpolations() {
    // Template with multiple expressions and text segments
    expect_string(
        "
        let x: number = 10;
        let y: number = 20;
        return `${x} + ${y} = ${x + y}`;
    ",
        "10 + 20 = 30",
    );
}

#[test]
fn test_template_with_ternary() {
    // Template literal containing ternary expression
    expect_string(
        "
        let x: number = 5;
        return `${x > 0 ? x : -x}`;
    ",
        "5",
    );
}

#[test]
fn test_template_with_method_call() {
    // Template with string method call result
    expect_string(
        "
        let greeting: string = \"hello\";
        return `${greeting.toUpperCase()}`;
    ",
        "HELLO",
    );
}

// ============================================================================
// 8. Complex For-Of Patterns
// ============================================================================

#[test]
fn test_for_of_with_destructure_pairs() {
    // For-of with array destructuring of pairs
    expect_i32(
        "
        let pairs: number[][] = [[1, 10], [2, 20], [3, 30]];
        let total: number = 0;
        for (let [key, val] of pairs) {
            total = total + key * val;
        }
        return total;
    ",
        140,
    ); // 1*10 + 2*20 + 3*30 = 10 + 40 + 90 = 140
}

#[test]
fn test_for_of_over_function_result() {
    // For-of iterating over function return value
    expect_i32(
        "
        function getItems(): number[] {
            return [10, 20, 30];
        }
        let sum: number = 0;
        for (let item of getItems()) {
            sum = sum + item;
        }
        return sum;
    ",
        60,
    );
}

#[test]
fn test_for_of_nested_with_break() {
    // Nested for-of with break only affecting inner loop
    expect_i32(
        "
        let outer: number[][] = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
        let sum: number = 0;
        for (let row of outer) {
            for (let val of row) {
                if (val > 5) { break; }
                sum = sum + val;
            }
        }
        return sum;
    ",
        15,
    ); // Row [1,2,3]: 1+2+3=6, Row [4,5,6]: 4+5=9 (break at 6>5), Row [7,8,9]: break at 7>5 → 0
       // Total: 6 + 9 + 0 = 15
}

#[test]
fn test_for_of_with_continue() {
    // For-of with continue to skip elements
    expect_i32(
        "
        let items: number[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let sum: number = 0;
        for (let item of items) {
            if (item % 2 == 0) { continue; }
            sum = sum + item;
        }
        return sum;
    ",
        25,
    ); // 1+3+5+7+9 = 25
}

#[test]
fn test_for_of_with_closure_per_iteration() {
    // Create closure in each iteration that captures loop variable
    expect_i32(
        "
        let items: number[] = [10, 20, 30];
        let sum: number = 0;
        for (let item of items) {
            let add = (x: number): number => item + x;
            sum = sum + add(1);
        }
        return sum;
    ",
        63,
    ); // 11 + 21 + 31 = 63
}

// ============================================================================
// 9. Class Method Interactions
// ============================================================================

#[test]
fn test_class_method_returns_arrow() {
    // Method returning arrow that captures this
    expect_i32(
        "
        class Adder {
            base: number = 0;
            constructor(base: number) { this.base = base; }
            getAdder(): (x: number) => number {
                let b: number = this.base;
                return (x: number): number => b + x;
            }
        }
        let a = new Adder(10);
        let f = a.getAdder();
        return f(32);
    ",
        42,
    );
}

#[test]
fn test_class_method_chaining_returns_this() {
    // Method chaining by returning this
    expect_i32(
        "
        class Builder {
            value: number = 0;
            add(x: number): Builder {
                this.value = this.value + x;
                return this;
            }
            result(): number { return this.value; }
        }
        let b = new Builder();
        return b.add(10).add(20).add(12).result();
    ",
        42,
    );
}

#[test]
fn test_class_with_static_and_instance() {
    // Static method and instance method together
    expect_i32(
        "
        class MathHelper {
            factor: number = 0;
            constructor(f: number) { this.factor = f; }
            static add(a: number, b: number): number { return a + b; }
            multiply(x: number): number { return this.factor * x; }
        }
        let m = new MathHelper(2);
        return MathHelper.add(m.multiply(10), m.multiply(11));
    ",
        42,
    );
}

#[test]
fn test_class_method_with_default_param() {
    // Method with default parameter
    expect_i32(
        "
        class Calc {
            compute(x: number, y: number = 10): number {
                return x + y;
            }
        }
        let c = new Calc();
        return c.compute(32);
    ",
        42,
    );
}

#[test]
fn test_class_virtual_dispatch_three_levels() {
    // Three-level inheritance with virtual dispatch
    expect_i32(
        "
        class A {
            value(): number { return 10; }
        }
        class B extends A {
            value(): number { return 20; }
        }
        class C extends B {
            value(): number { return 42; }
        }
        let obj: A = new C();
        return obj.value();
    ",
        42,
    );
}

// ============================================================================
// 10. Complex Type Annotations
// ============================================================================

#[test]
fn test_function_type_as_param() {
    // Function with function-typed parameter
    expect_i32(
        "
        function applyTwice(f: (x: number) => number, val: number): number {
            return f(f(val));
        }
        return applyTwice((x: number): number => x + 10, 22);
    ",
        42,
    );
}

#[test]
fn test_generic_function_call() {
    // Generic function with explicit type argument
    expect_i32(
        "
        function identity<T>(x: T): T { return x; }
        return identity<number>(42);
    ",
        42,
    );
}

#[test]
fn test_union_type_param() {
    // Function with union type parameter
    expect_i32(
        "
        function orDefault(x: number | null, def: number): number {
            if (x != null) { return x; }
            return def;
        }
        return orDefault(null, 42);
    ",
        42,
    );
}

#[test]
fn test_array_of_function_type() {
    // Variable typed as array of functions
    expect_i32(
        "
        let fns: ((x: number) => number)[] = [];
        fns.push((x: number): number => x + 1);
        fns.push((x: number): number => x + 2);
        return fns[0](10) + fns[1](10);
    ",
        23,
    );
}

#[test]
fn test_function_returning_function_type() {
    // Function whose return type is another function type
    expect_i32(
        "
        function multiplier(factor: number): (x: number) => number {
            return (x: number): number => factor * x;
        }
        let double = multiplier(2);
        return double(21);
    ",
        42,
    );
}

// ============================================================================
// 11. Mixed Feature Interactions
// ============================================================================

#[test]
fn test_for_of_with_arrow_callback() {
    // For-of combined with arrow function call each iteration
    expect_i32(
        "
        let items: number[] = [1, 2, 3, 4, 5];
        let transform = (x: number): number => x * x;
        let sum: number = 0;
        for (let item of items) {
            sum = sum + transform(item);
        }
        return sum;
    ",
        55,
    ); // 1+4+9+16+25 = 55
}

#[test]
fn test_nested_function_returning_class_instance() {
    // Function defines a class and returns an instance
    expect_i32(
        "
        function createCounter(start: number): number {
            class Counter {
                count: number = 0;
                constructor(n: number) { this.count = n; }
                increment(): number {
                    this.count = this.count + 1;
                    return this.count;
                }
            }
            let c = new Counter(start);
            c.increment();
            c.increment();
            return c.increment();
        }
        return createCounter(39);
    ",
        42,
    );
}

#[test]
fn test_arrow_with_try_catch_block_body() {
    // Arrow function with try-catch inside block body
    expect_i32(
        "
        let safe = (x: number): number => {
            try {
                if (x < 0) { throw 'negative'; }
                return x * 2;
            } catch (e) {
                return 0;
            }
        };
        return safe(21) + safe(-5);
    ",
        42,
    );
}

#[test]
fn test_switch_inside_arrow_function() {
    // Arrow with block body containing switch
    expect_i32(
        "
        let classify = (x: number): number => {
            switch (x) {
                case 1: return 10;
                case 2: return 20;
                case 3: return 30;
                default: return 0;
            }
        };
        return classify(1) + classify(2) + classify(3);
    ",
        60,
    );
}

#[test]
fn test_ternary_with_await() {
    // Ternary choosing between sync and async result
    expect_i32(
        "
        async function asyncVal(): Promise<number> { return 42; }
        function syncVal(): number { return 0; }
        async function main(): Promise<number> {
            let useAsync: boolean = true;
            return useAsync ? await asyncVal() : syncVal();
        }
        return await main();
    ",
        42,
    );
}

#[test]
fn test_class_in_function_iterated_in_for_of() {
    // Class defined in function, instances stored in array and iterated
    expect_i32(
        "
        function run(): number {
            class Item {
                val: number = 0;
                constructor(v: number) { this.val = v; }
            }
            let items: Item[] = [new Item(10), new Item(20), new Item(12)];
            let total: number = 0;
            for (let item of items) {
                total = total + item.val;
            }
            return total;
        }
        return run();
    ",
        42,
    );
}

#[test]
fn test_closure_over_destructured_variable() {
    // Arrow captures a variable from destructuring
    expect_i32(
        "
        let arr: number[] = [10, 32];
        let [a, b] = arr;
        let sum = (): number => a + b;
        return sum();
    ",
        42,
    );
}

#[test]
fn test_async_with_closure_and_class() {
    // Async function using both closure and class
    expect_i32(
        "
        class Multiplier {
            factor: number = 0;
            constructor(f: number) { this.factor = f; }
            apply(x: number): number { return this.factor * x; }
        }
        async function compute(): Promise<number> {
            let m = new Multiplier(2);
            let addOne = (x: number): number => x + 1;
            return addOne(m.apply(20));
        }
        return await compute();
    ",
        41,
    );
}

#[test]
fn test_deeply_nested_expression() {
    // Complex nested expression combining multiple operators
    expect_i32(
        "
        let a: number = 2;
        let b: number = 3;
        let c: number = 4;
        return (a + b) * c - (a * b) + (c ** a);
    ",
        30,
    ); // (2+3)*4 - (2*3) + (4**2) = 20 - 6 + 16 = 30
}

#[test]
fn test_multiple_feature_pipeline() {
    // Combines: arrow, class, for-of, destructuring, ternary
    expect_i32(
        "
        class Pair {
            x: number = 0;
            y: number = 0;
            constructor(x: number, y: number) { this.x = x; this.y = y; }
        }
        function process(pairs: Pair[]): number {
            let transform = (p: Pair): number => p.x > p.y ? p.x : p.y;
            let sum: number = 0;
            for (let p of pairs) {
                sum = sum + transform(p);
            }
            return sum;
        }
        let data: Pair[] = [new Pair(5, 10), new Pair(20, 15), new Pair(7, 12)];
        return process(data);
    ",
        42,
    ); // max(5,10)=10, max(20,15)=20, max(7,12)=12 → 10+20+12=42
}

// ============================================================================
// Optional Parameters (`?` syntax)
// ============================================================================

#[test]
fn test_optional_param_receives_null_when_omitted() {
    // Optional parameter receives null when not provided
    expect_null(
        "
        function greet(name?: string): string | null {
            return name;
        }
        return greet();
    ",
    );
}

#[test]
fn test_optional_param_receives_value_when_provided() {
    // Optional parameter receives the value when provided
    expect_string(
        "
        function greet(name?: string): string | null {
            if (name == null) {
                return \"world\";
            }
            return name;
        }
        return greet(\"Raya\");
    ",
        "Raya",
    );
}

#[test]
fn test_optional_param_with_null_check() {
    // Optional parameter with null check fallback
    expect_string(
        "
        function greet(name?: string): string {
            if (name == null) {
                return \"hello world\";
            }
            return \"hello \" + name;
        }
        return greet();
    ",
        "hello world",
    );
}

#[test]
fn test_optional_param_mixed_with_required() {
    // Required params followed by optional param
    expect_i32(
        "
        function add(x: number, y?: number): number {
            if (y == null) {
                return x;
            }
            return x + y;
        }
        return add(32, 10);
    ",
        42,
    );
}

#[test]
fn test_optional_param_mixed_with_required_omitted() {
    // Required params followed by optional param (omitted)
    expect_i32(
        "
        function add(x: number, y?: number): number {
            if (y == null) {
                return x;
            }
            return x + y;
        }
        return add(42);
    ",
        42,
    );
}

#[test]
fn test_optional_and_default_params_together() {
    // Mixing optional params (?) and default value params
    expect_i32(
        "
        function calc(x: number, y?: number, z: number = 10): number {
            let result = x;
            if (y != null) {
                result = result + y;
            }
            return result + z;
        }
        return calc(32);
    ",
        42,
    );
}

#[test]
fn test_default_param_can_reference_previous_param() {
    // Default value expression may reference an earlier parameter
    expect_i32(
        "
        function mirror(x: number, y: number = x): number {
            return y;
        }
        return mirror(42);
    ",
        42,
    );
}

#[test]
fn test_multiple_default_params_partial_call() {
    // Multiple defaults should apply for omitted trailing arguments
    expect_i32(
        "
        function total(a: number, b: number = 10, c: number = 5): number {
            return a + b + c;
        }
        return total(27);
    ",
        42,
    );
}

#[test]
fn test_optional_param_explicit_null_type_error() {
    // Explicit null is rejected at call site for optional number parameters
    expect_compile_error(
        "
        function maybe(x: number, y?: number): number {
            if (y == null) {
                return x;
            }
            return x + y;
        }
        return maybe(42, null);
    ",
        "TypeMismatch",
    );
}

#[test]
fn test_constructor_default_params_partial_override() {
    // Constructor uses provided leading args and defaults for omitted trailing args
    expect_i32(
        "
        class Config {
            value: number;
            constructor(base: number, extra: number = 10, bonus: number = 5) {
                this.value = base + extra + bonus;
            }
        }
        let c = new Config(27);
        return c.value;
    ",
        42,
    );
}

#[test]
fn test_class_method_optional_param() {
    // Class method with optional parameter
    expect_string(
        "
        class Greeter {
            greet(name?: string): string {
                if (name == null) {
                    return \"hello\";
                }
                return \"hello \" + name;
            }
        }
        let g = new Greeter();
        return g.greet();
    ",
        "hello",
    );
}

#[test]
fn test_class_method_with_default_param_overridden() {
    // Method default should be ignored when explicit argument is provided
    expect_i32(
        "
        class Calc {
            compute(x: number, y: number = 100): number {
                return x + y;
            }
        }
        let c = new Calc();
        return c.compute(20, 22);
    ",
        42,
    );
}

#[test]
fn test_arrow_function_with_default_param_checker() {
    // Arrow function with default param — tests checker min_params fix
    expect_i32(
        "
        let add = (x: number, y: number = 10): number => x + y;
        return add(32);
    ",
        42,
    );
}

#[test]
fn test_required_after_optional_error() {
    // Required parameter after optional parameter should be a compile error
    expect_compile_error(
        "
        function bad(x?: number, y: number): number {
            return y;
        }
        return bad(1, 2);
    ",
        "RequiredAfterOptional",
    );
}

#[test]
fn test_required_after_default_error() {
    // Required parameter after default-value parameter should be a compile error
    expect_compile_error(
        "
        function bad(x: number = 10, y: number): number {
            return y;
        }
        return bad(1, 2);
    ",
        "RequiredAfterOptional",
    );
}

#[test]
fn test_constructor_optional_param() {
    // Constructor with optional parameter
    expect_i32(
        "
        class Config {
            value: number;
            constructor(value?: number) {
                if (value == null) {
                    this.value = 42;
                } else {
                    this.value = value;
                }
            }
        }
        let c = new Config();
        return c.value;
    ",
        42,
    );
}

#[test]
fn test_multiple_optional_params() {
    // Multiple optional parameters
    expect_i32(
        "
        function sum(a: number, b?: number, c?: number): number {
            let result = a;
            if (b != null) {
                result = result + b;
            }
            if (c != null) {
                result = result + c;
            }
            return result;
        }
        return sum(42);
    ",
        42,
    );
}

#[test]
fn test_multiple_optional_params_partial() {
    // Providing some optional parameters
    expect_i32(
        "
        function sum(a: number, b?: number, c?: number): number {
            let result = a;
            if (b != null) {
                result = result + b;
            }
            if (c != null) {
                result = result + c;
            }
            return result;
        }
        return sum(30, 12);
    ",
        42,
    );
}

#[test]
fn test_labeled_break_exits_outer_loop() {
    expect_i32(
        "
        let out = 0;
        outer: for (let i = 0; i < 3; i = i + 1) {
            for (let j = 0; j < 3; j = j + 1) {
                out = out + 1;
                break outer;
            }
        }
        return out;
    ",
        1,
    );
}

#[test]
fn test_labeled_continue_targets_outer_loop() {
    expect_i32(
        "
        let out = 0;
        outer: for (let i = 0; i < 3; i = i + 1) {
            for (let j = 0; j < 3; j = j + 1) {
                out = out + 1;
                continue outer;
            }
        }
        return out;
    ",
        3,
    );
}

#[test]
fn test_unknown_label_reports_compile_error() {
    expect_compile_error(
        "
        for (let i = 0; i < 1; i = i + 1) {
            break missingLabel;
        }
        return 0;
    ",
        "UnknownLabel",
    );
}

#[test]
fn test_continue_non_loop_label_reports_compile_error() {
    expect_compile_error(
        "
        blockLabel: {
            continue blockLabel;
        }
        return 0;
    ",
        "ContinueLabelNotLoop",
    );
}
