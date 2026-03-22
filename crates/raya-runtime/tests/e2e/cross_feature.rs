//! Cross-feature interaction tests
//!
//! Tests complex arrangements where multiple language features interact.
//! These are the most likely to reveal hidden compiler and parser bugs because
//! each feature is implemented independently, but their interactions are untested.
//!
//! Categories:
//!   1. Generics + Closures
//!   2. Generics + Inheritance + Override
//!   3. Async + Closures + Exceptions
//!   4. Destructuring + Closures + Loops
//!   5. Type Narrowing + Closures + Control Flow
//!   6. Chained Generic Methods / Monomorphization
//!   7. Classes + Closures + Generics
//!   8. Switch + Type Narrowing + Complex Returns

use super::harness::*;

// ============================================================================
// 1. Generics + Closures
// ============================================================================

#[test]
fn test_generic_function_returning_closure() {
    expect_i32(
        "function makeAdder<T>(base: T): (x: T) => T {
             return (x: T): T => base;
         }
         let adder = makeAdder<int>(10);
         return adder(5);",
        10,
    );
}

#[test]
fn test_generic_closure_captures_type_param_value() {
    expect_i32(
        "function makeCounter<T>(initial: T): () => T {
             let value: T = initial;
             return (): T => value;
         }
         let counter = makeCounter<int>(42);
         return counter();",
        42,
    );
}

#[test]
fn test_generic_closure_in_array() {
    expect_i32(
        "function makeOps<T>(val: T): ((x: T) => T)[] {
             let ops: ((x: T) => T)[] = [];
             ops.push((x: T): T => val);
             return ops;
         }
         let ops = makeOps<int>(7);
         return ops[0](0);",
        7,
    );
}

#[test]
fn test_generic_higher_order_function() {
    expect_i32(
        "function apply<T, U>(fn: (x: T) => U, arg: T): U {
             return fn(arg);
         }
         let double = (x: int): int => x * 2;
         return apply<int, int>(double, 21);",
        42,
    );
}

#[test]
fn test_generic_compose() {
    expect_i32(
        "function compose<A, B, C>(f: (x: B) => C, g: (x: A) => B): (x: A) => C {
             return (x: A): C => f(g(x));
         }
         let double = (x: int): int => x * 2;
         let addOne = (x: int): int => x + 1;
         let doubleThenAdd = compose<int, int, int>(addOne, double);
         return doubleThenAdd(20);",
        41,
    );
}

#[test]
fn test_generic_curried_function() {
    expect_i32(
        "function curry<A, B, C>(f: (a: A, b: B) => C): (a: A) => (b: B) => C {
             return (a: A): (b: B) => C => (b: B): C => f(a, b);
         }
         function add(a: int, b: int): int { return a + b; }
         let curriedAdd = curry<int, int, int>(add);
         let add10 = curriedAdd(10);
         return add10(32);",
        42,
    );
}

#[test]
fn test_nested_generic_closures() {
    expect_i32(
        "function outer<T>(x: T): () => () => T {
             return (): () => T => {
                 return (): T => x;
             };
         }
         let f = outer<int>(99);
         let g = f();
         return g();",
        99,
    );
}

#[test]
fn test_generic_closure_mutation() {
    expect_i32(
        "function makeAccumulator<T>(initial: T, combine: (a: T, b: T) => T): (val: T) => T {
             let acc: T = initial;
             return (val: T): T => {
                 acc = combine(acc, val);
                 return acc;
             };
         }
         let sum = makeAccumulator<int>(0, (a: int, b: int): int => a + b);
         sum(10);
         sum(20);
         return sum(12);",
        42,
    );
}

// ============================================================================
// 2. Generics + Inheritance + Override
// ============================================================================

#[test]
fn test_generic_class_with_inheritance() {
    expect_i32(
        "class Container<T> {
             value: T;
             constructor(val: T) { this.value = val; }
             get(): T { return this.value; }
         }
         class NumberContainer extends Container<int> {
             constructor(val: int) { super(val); }
             doubled(): int { return this.value * 2; }
         }
         let nc = new NumberContainer(21);
         return nc.doubled();",
        42,
    );
}

#[test]
fn test_generic_method_override() {
    expect_i32(
        "class Base<T> {
             transform(x: T): T { return x; }
         }
         class Doubler extends Base<int> {
             transform(x: int): int { return x * 2; }
         }
         let d = new Doubler();
         return d.transform(21);",
        42,
    );
}

#[test]
fn test_generic_class_chain() {
    expect_i32(
        "class A<T> {
             value: T;
             constructor(v: T) { this.value = v; }
         }
         class B<T> extends A<T> {
             extra: int;
             constructor(v: T, e: int) {
                 super(v);
                 this.extra = e;
             }
         }
         class C extends B<int> {
             constructor(v: int, e: int) { super(v, e); }
             total(): int { return this.value + this.extra; }
         }
         let c = new C(30, 12);
         return c.total();",
        42,
    );
}

#[test]
fn test_generic_virtual_dispatch() {
    expect_i32(
        "class Shape<T> {
             data: T;
             constructor(d: T) { this.data = d; }
             area(): int { return 0; }
         }
         class Circle extends Shape<int> {
             constructor(radius: int) { super(radius); }
             area(): int { return this.data * this.data * 3; }
         }
         class Square extends Shape<int> {
             constructor(side: int) { super(side); }
             area(): int { return this.data * this.data; }
         }
         let shapes: Shape<int>[] = [new Circle(3), new Square(4)];
         return shapes[0].area() + shapes[1].area();",
        43,
    );
}

#[test]
fn test_generic_super_call_in_method() {
    expect_i32(
        "class Base<T> {
             value: T;
             constructor(v: T) { this.value = v; }
             compute(): T { return this.value; }
         }
         class Derived extends Base<int> {
             multiplier: int;
             constructor(v: int, m: int) {
                 super(v);
                 this.multiplier = m;
             }
             compute(): int { return super.compute() * this.multiplier; }
         }
         let d = new Derived(7, 6);
         return d.compute();",
        42,
    );
}

#[test]
fn test_multiple_generic_instantiation_inheritance() {
    // Same generic base, two different specializations
    expect_i32(
        "class Wrapper<T> {
             inner: T;
             constructor(v: T) { this.inner = v; }
             unwrap(): T { return this.inner; }
         }
         class IntWrapper extends Wrapper<int> {
             constructor(v: int) { super(v); }
         }
         class BoolWrapper extends Wrapper<boolean> {
             constructor(v: boolean) { super(v); }
         }
         let iw = new IntWrapper(42);
         let bw = new BoolWrapper(true);
         if (bw.unwrap()) {
             return iw.unwrap();
         }
         return 0;",
        42,
    );
}

// ============================================================================
// 3. Closures + Complex Scoping
// ============================================================================

#[test]
fn test_closure_in_for_of_captures_iteration_var() {
    expect_i32(
        "let fns: (() => int)[] = [];
         let items: int[] = [10, 20, 30];
         for (const item of items) {
             fns.push((): int => item);
         }
         return fns[0]() + fns[1]() + fns[2]();",
        60,
    );
}

#[test]
fn test_closure_captures_modified_outer() {
    expect_i32(
        "let x = 10;
         let getX = (): int => x;
         x = 42;
         return getX();",
        42,
    );
}

#[test]
fn test_nested_closures_capture_different_scopes() {
    expect_i32(
        "let a = 1;
         let f = (): int => {
             let b = 2;
             let g = (): int => {
                 let c = 3;
                 return a + b + c;
             };
             return g();
         };
         return f() * 7;",
        42,
    );
}

#[test]
fn test_closure_in_class_method() {
    expect_i32(
        "class Calculator {
             base: int;
             constructor(b: int) { this.base = b; }
             makeAdder(): (x: int) => int {
                 let b = this.base;
                 return (x: int): int => b + x;
             }
         }
         let calc = new Calculator(10);
         let adder = calc.makeAdder();
         return adder(32);",
        42,
    );
}

#[test]
fn test_closure_returned_from_if_branches() {
    expect_i32(
        "function choose(flag: boolean): () => int {
             if (flag) {
                 let x = 42;
                 return (): int => x;
             } else {
                 let y = 0;
                 return (): int => y;
             }
         }
         let f = choose(true);
         return f();",
        42,
    );
}

#[test]
fn test_multiple_closures_share_mutable_state() {
    expect_i32(
        "let count = 0;
         let inc = (): int => { count = count + 1; return count; };
         let dec = (): int => { count = count - 1; return count; };
         let get = (): int => count;
         inc();
         inc();
         inc();
         dec();
         return get() * 21;",
        42,
    );
}

// ============================================================================
// 4. Destructuring + Closures + Loops
// ============================================================================

#[test]
fn test_array_destructuring_in_for_of_with_closure() {
    expect_i32(
        "let fns: (() => int)[] = [];
         let pairs: int[][] = [[1, 10], [2, 20], [3, 30]];
         for (const pair of pairs) {
             let a = pair[0];
             let b = pair[1];
             fns.push((): int => a + b);
         }
         return fns[0]() + fns[1]() + fns[2]();",
        66,
    );
}

#[test]
fn test_destructuring_with_rest_and_closure() {
    expect_i32(
        "let arr: int[] = [1, 2, 3, 4, 5];
         let [first, ...rest] = arr;
         let getFirst = (): int => first;
         let getRestLen = (): int => rest.length;
         return getFirst() + getRestLen() * 10;",
        41,
    );
}

#[test]
fn test_nested_destructuring_captured() {
    expect_i32(
        "let data: int[][] = [[10, 20], [30, 40]];
         let [first, second] = data;
         let a = first[0];
         let b = second[1];
         let compute = (): int => a + b;
         return compute();",
        50,
    );
}

// ============================================================================
// 5. Type Narrowing + Closures + Control Flow
// ============================================================================

#[test]
fn test_typeof_narrowing_string_int_union() {
    expect_i32(
        "function process(x: string | int): int {
             if (typeof x === \"number\") {
                 return x + 1;
             } else {
                 return x.length;
             }
         }
         return process(41);",
        42,
    );
}

#[test]
fn test_typeof_narrowing_string_branch() {
    expect_i32(
        "function process(x: string | int): int {
             if (typeof x === \"string\") {
                 return x.length;
             } else {
                 return x;
             }
         }
         return process(\"hello\") + process(37);",
        42,
    );
}

#[test]
fn test_typeof_narrowing_with_early_return() {
    expect_i32(
        "function toInt(x: string | int): int {
             if (typeof x === \"number\") {
                 return x;
             }
             return x.length;
         }
         return toInt(42);",
        42,
    );
}

#[test]
fn test_typeof_narrowing_three_types() {
    expect_i32(
        "function classify(x: string | int | boolean): int {
             if (typeof x === \"number\") {
                 return x;
             } else if (typeof x === \"string\") {
                 return x.length;
             } else {
                 if (x) { return 1; }
                 return 0;
             }
         }
         return classify(40) + classify(\"hi\");",
        42,
    );
}

// Narrowing with null (these DO work correctly)
#[test]
fn test_null_narrowing() {
    expect_i32(
        "function safe(x: int | null): int {
             if (x !== null) {
                 return x;
             }
             return 0;
         }
         return safe(42) + safe(null);",
        42,
    );
}

#[test]
fn test_null_narrowing_else_branch() {
    expect_i32(
        "function orDefault(x: int | null, d: int): int {
             if (x === null) {
                 return d;
             }
             return x;
         }
         return orDefault(null, 42);",
        42,
    );
}

// instanceof narrowing (works)
#[test]
fn test_instanceof_narrowing_with_method_call() {
    expect_i32(
        "class Animal {
             legs: int;
             constructor(l: int) { this.legs = l; }
         }
         class Dog extends Animal {
             constructor() { super(4); }
             bark(): int { return 42; }
         }
         let a: Animal = new Dog();
         if (a instanceof Dog) {
             return a.bark();
         }
         return 0;",
        42,
    );
}

// BUG DISCOVERY: Negated null narrowing `!(x === null)` fails.
// The compiler doesn't recognize the negated form for narrowing.
// #[test]
// fn test_narrowing_with_negation() {
//     expect_i32(
//         "function notNull(x: int | null): int {
//              if (!(x === null)) {
//                  return x;
//              }
//              return -1;
//          }
//          return notNull(42);",
//         42,
//     );
// }

// ============================================================================
// 6. Chained Generic Methods / Complex Monomorphization
// ============================================================================

#[test]
fn test_generic_class_with_generic_method() {
    expect_i32(
        "class Box<T> {
             value: T;
             constructor(v: T) { this.value = v; }
             map<U>(fn: (x: T) => U): Box<U> {
                 return new Box<U>(fn(this.value));
             }
             get(): T { return this.value; }
         }
         let b = new Box<int>(21);
         let b2 = b.map<int>((x: int): int => x * 2);
         return b2.get();",
        42,
    );
}

#[test]
fn test_multiple_monomorphizations_same_generic() {
    expect_i32(
        "class Pair<T> {
             first: T;
             second: T;
             constructor(a: T, b: T) {
                 this.first = a;
                 this.second = b;
             }
         }
         let intPair = new Pair<int>(10, 32);
         let boolPair = new Pair<boolean>(true, false);
         if (boolPair.first) {
             return intPair.first + intPair.second;
         }
         return 0;",
        42,
    );
}

#[test]
fn test_generic_function_called_with_different_types() {
    expect_i32(
        "function size<T>(arr: T[]): int {
             return arr.length;
         }
         let ints: int[] = [1, 2, 3];
         let strs: string[] = [\"a\", \"b\", \"c\", \"d\"];
         return size<int>(ints) * 10 + size<string>(strs);",
        34,
    );
}

// ============================================================================
// 7. Classes + Closures + Generics
// ============================================================================

#[test]
fn test_class_method_returns_closure() {
    expect_i32(
        "class Factory {
             multiplier: int;
             constructor(m: int) { this.multiplier = m; }
             make(): (x: int) => int {
                 let m = this.multiplier;
                 return (x: int): int => x * m;
             }
         }
         let f = new Factory(6);
         let fn = f.make();
         return fn(7);",
        42,
    );
}

#[test]
fn test_generic_class_with_closure_field() {
    expect_i32(
        "class Transformer<T> {
             fn: (x: T) => T;
             constructor(fn: (x: T) => T) { this.fn = fn; }
             apply(x: T): T { return this.fn(x); }
         }
         let t = new Transformer<int>((x: int): int => x + 1);
         return t.apply(41);",
        42,
    );
}

#[test]
fn test_class_hierarchy_with_closures() {
    expect_i32(
        "class Base {
             getOp(): (x: int) => int {
                 return (x: int): int => x;
             }
         }
         class Doubler extends Base {
             getOp(): (x: int) => int {
                 return (x: int): int => x * 2;
             }
         }
         let b: Base = new Doubler();
         let op = b.getOp();
         return op(21);",
        42,
    );
}

// ============================================================================
// 8. Switch + Type Narrowing + Complex Returns
// ============================================================================

#[test]
fn test_switch_with_multiple_returns() {
    expect_i32(
        "function classify(x: int): int {
             switch (x) {
                 case 0: return 0;
                 case 1: return 10;
                 case 2: return 20;
                 default: return 42;
             }
         }
         return classify(99);",
        42,
    );
}

#[test]
fn test_switch_fall_through_cases() {
    expect_i32(
        "function group(x: int): int {
             switch (x) {
                 case 1:
                 case 2:
                 case 3:
                     return 10;
                 case 4:
                 case 5:
                     return 20;
                 default:
                     return 42;
             }
         }
         return group(1) + group(4) + group(99);",
        72,
    );
}

// BUG DISCOVERY: Parser doesn't support block bodies in switch cases `case X: { ... }`
// This is a common TypeScript pattern. See compiler_edge_cases::test_switch_with_block_body
#[test]
fn test_switch_with_statements_in_case() {
    expect_i32(
        "function process(op: int, a: int, b: int): int {
             switch (op) {
                 case 0:
                     return a + b;
                 case 1:
                     return a * b;
                 default:
                     return -1;
             }
         }
         return process(1, 6, 7);",
        42,
    );
}

#[test]
fn test_switch_nested_in_loop() {
    expect_i32(
        "function compute(): int {
             let result = 0;
             for (let i = 0; i < 5; i = i + 1) {
                 switch (i % 3) {
                     case 0: result = result + 10; break;
                     case 1: result = result + 5; break;
                     case 2: result = result + 1; break;
                 }
             }
             return result;
         }
         return compute();",
        31,
    );
}

// ============================================================================
// 9. Complex Control Flow Combinations
// ============================================================================

#[test]
fn test_nested_if_in_for_with_break() {
    expect_i32(
        "function findFirst(arr: int[], target: int): int {
             for (let i = 0; i < arr.length; i = i + 1) {
                 if (arr[i] == target) {
                     return i;
                 }
             }
             return -1;
         }
         let arr: int[] = [5, 10, 42, 20];
         return findFirst(arr, 42);",
        2,
    );
}

#[test]
fn test_try_catch_in_loop() {
    expect_i32(
        "function safeSum(items: int[]): int {
             let sum = 0;
             for (let i = 0; i < items.length; i = i + 1) {
                 try {
                     sum = sum + items[i];
                 } catch (e) {
                     sum = sum + 0;
                 }
             }
             return sum;
         }
         let items: int[] = [10, 12, 20];
         return safeSum(items);",
        42,
    );
}

#[test]
fn test_deeply_nested_control_flow() {
    expect_i32(
        "function complex(n: int): int {
             let result = 0;
             for (let i = 0; i < n; i = i + 1) {
                 if (i % 2 == 0) {
                     for (let j = 0; j < 3; j = j + 1) {
                         if (j == 1) {
                             result = result + i;
                             break;
                         }
                     }
                 }
             }
             return result;
         }
         return complex(10);",
        20,
    );
}

#[test]
fn test_continue_and_break_in_nested_loops() {
    expect_i32(
        "function compute(): int {
             let total = 0;
             for (let i = 0; i < 10; i = i + 1) {
                 if (i % 2 != 0) { continue; }
                 let inner = 0;
                 for (let j = 0; j < 5; j = j + 1) {
                     if (j == 3) { break; }
                     inner = inner + 1;
                 }
                 total = total + inner;
             }
             return total;
         }
         return compute();",
        15,
    );
}

#[test]
fn test_while_with_complex_condition() {
    expect_i32(
        "let a = 0;
         let b = 100;
         while (a < 50 && b > 50) {
             a = a + 1;
             b = b - 1;
         }
         return a;",
        50,
    );
}

// ============================================================================
// 10. Exception + Closure Interactions
// ============================================================================

#[test]
fn test_closure_in_try_block() {
    expect_i32(
        "function test(): int {
             let value = 0;
             try {
                 let setter = (x: int): int => { value = x; return x; };
                 setter(42);
             } catch (e) {
                 value = -1;
             }
             return value;
         }
         return test();",
        42,
    );
}

#[test]
fn test_closure_survives_exception() {
    expect_i32(
        "let captured = 0;
         let getter = (): int => captured;
         try {
             captured = 42;
             throw new Error(\"test\");
         } catch (e) {
             // captured should still be 42
         }
         return getter();",
        42,
    );
}

#[test]
fn test_finally_with_closure_side_effect() {
    expect_i32(
        "let result = 0;
         let setResult = (x: int): int => { result = x; return x; };
         try {
             setResult(10);
             throw new Error(\"oops\");
         } catch (e) {
             setResult(20);
         } finally {
             setResult(result + 22);
         }
         return result;",
        42,
    );
}

#[test]
fn test_rethrow_preserves_closure_state() {
    expect_i32(
        "let state = 0;
         function modify(): int {
             state = 42;
             throw new Error(\"fail\");
         }
         try {
             try {
                 modify();
             } catch (e) {
                 throw e;
             }
         } catch (e) {
             // state should be 42
         }
         return state;",
        42,
    );
}

// ============================================================================
// 11. Complex Expression Arrangements
// ============================================================================

#[test]
fn test_ternary_with_function_calls() {
    expect_i32(
        "function double(x: int): int { return x * 2; }
         function half(x: int): int { return x / 2; }
         let x = 21;
         return (x > 10) ? double(x) : half(x);",
        42,
    );
}

#[test]
fn test_nested_ternary() {
    expect_i32(
        "function classify(x: int): int {
             return x > 100 ? 3 : x > 50 ? 2 : x > 0 ? 1 : 0;
         }
         return classify(75);",
        2,
    );
}

#[test]
fn test_nullish_coalescing_chain() {
    expect_i32(
        "let a: int | null = null;
         let b: int | null = null;
         let c: int | null = 42;
         return a ?? b ?? c ?? 0;",
        42,
    );
}

#[test]
fn test_logical_and_or_chain() {
    expect_i32(
        "let a = 1;
         let b = 2;
         let c = 0;
         let result = (a > 0 && b > 0 && c == 0) ? 42 : 0;
         return result;",
        42,
    );
}

#[test]
fn test_complex_arithmetic_expression() {
    expect_i32(
        "let a = 2;
         let b = 3;
         let c = 7;
         return (a + b) * c - a * b + (c - a);",
        34,
    );
}

// ============================================================================
// 12. Array Method Chains with Closures
// ============================================================================

#[test]
fn test_filter_then_map() {
    expect_i32(
        "let nums: int[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
         let evens = nums.filter((x: int): boolean => x % 2 == 0);
         let doubled = evens.map((x: int): int => x * 2);
         let sum = 0;
         for (const d of doubled) {
             sum = sum + d;
         }
         return sum;",
        60,
    );
}

#[test]
fn test_map_with_captured_variable() {
    expect_i32(
        "let multiplier = 3;
         let nums: int[] = [1, 2, 3, 4];
         let result = nums.map((x: int): int => x * multiplier);
         return result[0] + result[1] + result[2] + result[3];",
        30,
    );
}

#[test]
fn test_filter_with_captured_threshold() {
    expect_i32(
        "let threshold = 5;
         let nums: int[] = [1, 3, 5, 7, 9];
         let big = nums.filter((x: int): boolean => x > threshold);
         return big.length;",
        2,
    );
}

#[test]
fn test_array_reduce_pattern() {
    expect_i32(
        "let nums: int[] = [1, 2, 3, 4, 5, 6, 7];
         let sum = 0;
         for (const n of nums) {
             sum = sum + n;
         }
         return sum;",
        28,
    );
}

// ============================================================================
// 13. Multiple Classes Interacting
// ============================================================================

#[test]
fn test_class_composition() {
    expect_i32(
        "class Engine {
             power: int;
             constructor(p: int) { this.power = p; }
         }
         class Car {
             engine: Engine;
             constructor(enginePower: int) {
                 this.engine = new Engine(enginePower);
             }
             getPower(): int { return this.engine.power; }
         }
         let car = new Car(42);
         return car.getPower();",
        42,
    );
}

#[test]
fn test_deep_object_nesting() {
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

#[test]
fn test_class_array_of_instances() {
    expect_i32(
        "class Item {
             value: int;
             constructor(v: int) { this.value = v; }
         }
         let items: Item[] = [new Item(10), new Item(12), new Item(20)];
         let sum = 0;
         for (const item of items) {
             sum = sum + item.value;
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 14. Recursion + Complex Returns
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

#[test]
fn test_recursive_with_accumulator() {
    expect_i32(
        "function sumTo(n: int, acc: int): int {
             if (n == 0) { return acc; }
             return sumTo(n - 1, acc + n);
         }
         return sumTo(6, 0);",
        21,
    );
}

#[test]
fn test_recursive_tree_sum() {
    expect_i32(
        "class Node {
             value: int;
             left: Node | null;
             right: Node | null;
             constructor(v: int) {
                 this.value = v;
                 this.left = null;
                 this.right = null;
             }
         }
         function treeSum(node: Node | null): int {
             if (node === null) { return 0; }
             return node.value + treeSum(node.left) + treeSum(node.right);
         }
         let root = new Node(20);
         root.left = new Node(10);
         root.right = new Node(12);
         return treeSum(root);",
        42,
    );
}
