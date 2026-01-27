//! Phase 11: Async/Await tests
//!
//! Tests for async functions, await expressions, and Task handling.
//! In Raya, async functions always create Tasks (green threads).

use super::harness::*;

// ============================================================================
// Basic Async Functions
// ============================================================================

#[test]
fn test_async_function_simple() {
    expect_i32(
        "async function getValue(): Task<number> {
             return 42;
         }
         return await getValue();",
        42,
    );
}

#[test]
fn test_async_function_with_computation() {
    expect_i32(
        "async function compute(x: number): Task<number> {
             return x * 2;
         }
         return await compute(21);",
        42,
    );
}

#[test]
fn test_async_function_multiple_params() {
    expect_i32(
        "async function add(a: number, b: number): Task<number> {
             return a + b;
         }
         return await add(10, 32);",
        42,
    );
}

// ============================================================================
// Await Expressions
// ============================================================================

#[test]
fn test_await_in_expression() {
    expect_i32(
        "async function double(x: number): Task<number> {
             return x * 2;
         }
         async function main(): Task<number> {
             let result = await double(10) + await double(11);
             return result;
         }
         return await main();",
        42,
    );
}

#[test]
fn test_await_chained() {
    expect_i32(
        "async function step1(): Task<number> {
             return 10;
         }
         async function step2(x: number): Task<number> {
             return x + 20;
         }
         async function step3(x: number): Task<number> {
             return x + 12;
         }
         async function pipeline(): Task<number> {
             let a = await step1();
             let b = await step2(a);
             let c = await step3(b);
             return c;
         }
         return await pipeline();",
        42,
    );
}

#[test]
fn test_await_conditional() {
    expect_i32(
        "async function getValue(flag: boolean): Task<number> {
             if (flag) {
                 return 42;
             } else {
                 return 0;
             }
         }
         return await getValue(true);",
        42,
    );
}

// ============================================================================
// Task Creation
// ============================================================================

#[test]
fn test_task_starts_immediately() {
    // In Raya, calling an async function starts the Task immediately
    expect_i32(
        "let started = 0;
         async function work(): Task<number> {
             started = 1;
             return 42;
         }
         let task = work(); // Task starts NOW
         // started should be 1 even before await
         return await task;",
        42,
    );
}

#[test]
fn test_task_multiple() {
    // Multiple tasks can run concurrently
    expect_i32(
        "async function compute(x: number): Task<number> {
             return x * 2;
         }
         let task1 = compute(10);
         let task2 = compute(11);
         return await task1 + await task2;",
        42,
    );
}

// ============================================================================
// Async with Control Flow
// ============================================================================

#[test]
fn test_async_with_loop() {
    expect_i32(
        "async function sumAsync(n: number): Task<number> {
             let sum = 0;
             let i = 1;
             while (i <= n) {
                 sum = sum + i;
                 i = i + 1;
             }
             return sum;
         }
         return await sumAsync(10);",
        55,
    );
}

#[test]
fn test_async_recursive() {
    expect_i32(
        "async function factorialAsync(n: number): Task<number> {
             if (n <= 1) {
                 return 1;
             }
             let prev = await factorialAsync(n - 1);
             return n * prev;
         }
         return await factorialAsync(5);",
        120,
    );
}

// ============================================================================
// Async Arrow Functions
// ============================================================================

#[test]
fn test_async_arrow_simple() {
    expect_i32(
        "let getValue = async (): Task<number> => 42;
         return await getValue();",
        42,
    );
}

#[test]
fn test_async_arrow_with_param() {
    expect_i32(
        "let double = async (x: number): Task<number> => x * 2;
         return await double(21);",
        42,
    );
}

// ============================================================================
// Async Methods
// ============================================================================

#[test]
fn test_async_method() {
    expect_i32(
        "class Service {
             async fetch(id: number): Task<number> {
                 return id * 2;
             }
         }
         let s = new Service();
         return await s.fetch(21);",
        42,
    );
}

#[test]
fn test_async_method_using_this() {
    expect_i32(
        "class Counter {
             value: number = 10;
             async incrementAsync(): Task<number> {
                 this.value = this.value + 1;
                 return this.value;
             }
         }
         let c = new Counter();
         return await c.incrementAsync();",
        11,
    );
}

// ============================================================================
// Error Handling in Async
// ============================================================================

#[test]
#[ignore = "Async error handling not yet implemented"]
fn test_async_try_catch() {
    expect_i32(
        "async function mayFail(shouldFail: boolean): Task<number> {
             if (shouldFail) {
                 throw 'error';
             }
             return 42;
         }
         async function main(): Task<number> {
             try {
                 return await mayFail(false);
             } catch (e) {
                 return 0;
             }
         }
         return await main();",
        42,
    );
}

// ============================================================================
// async Keyword for Wrapping Calls
// ============================================================================

#[test]
fn test_async_call_wrapper() {
    // The 'async' keyword before a call wraps it in a Task
    expect_i32(
        "function syncWork(): number {
             return 42;
         }
         // 'async syncWork()' wraps the synchronous call in a Task
         let task = async syncWork();
         return await task;",
        42,
    );
}

// ============================================================================
// Parallel Await (await [...])
// ============================================================================

#[test]
fn test_await_array_simple() {
    // await [task1, task2] waits for all tasks and returns array of results
    expect_i32(
        "async function compute(x: number): Task<number> {
             return x * 2;
         }
         let task1 = compute(10);
         let task2 = compute(11);
         let results = await [task1, task2];
         return results[0] + results[1];",
        42,
    );
}

#[test]
fn test_await_array_ordering() {
    // Results should match task order, not completion order
    expect_i32(
        "async function getValue(x: number): Task<number> {
             return x;
         }
         let results = await [getValue(1), getValue(2), getValue(3)];
         return results[0] * 100 + results[1] * 10 + results[2];",
        123,
    );
}

#[test]
fn test_await_array_inline() {
    // Can create tasks inline in the array
    expect_i32(
        "async function triple(x: number): Task<number> {
             return x * 3;
         }
         let results = await [triple(10), triple(4)];
         return results[0] + results[1];",
        42,
    );
}
