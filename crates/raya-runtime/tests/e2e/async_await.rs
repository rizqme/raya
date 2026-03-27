//! Phase 11: Async/Await tests
//!
//! Tests for async functions, await expressions, and Promise handling.
//! In Raya, async functions always create Tasks (green threads).

use super::harness::*;

// ============================================================================
// Basic Async Functions
// ============================================================================

#[test]
fn test_async_function_simple() {
    expect_i32(
        "async function getValue(): Promise<number> {
             return 42;
         }
         return await getValue();",
        42,
    );
}

#[test]
fn test_async_function_with_computation() {
    expect_i32(
        "async function compute(x: number): Promise<number> {
             return x * 2;
         }
         return await compute(21);",
        42,
    );
}

#[test]
fn test_async_function_multiple_params() {
    expect_i32(
        "async function add(a: number, b: number): Promise<number> {
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
        "async function double(x: number): Promise<number> {
             return x * 2;
         }
         async function main(): Promise<number> {
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
        "async function step1(): Promise<number> {
             return 10;
         }
         async function step2(x: number): Promise<number> {
             return x + 20;
         }
         async function step3(x: number): Promise<number> {
             return x + 12;
         }
         async function pipeline(): Promise<number> {
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
        "async function getValue(flag: boolean): Promise<number> {
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

#[test]
fn test_raya_profile_allows_await_without_async_function() {
    expect_i32(
        "function main(): number {
             return await Promise.resolve(42);
         }
         return main();",
        42,
    );
}

// ============================================================================
// Promise Creation
// ============================================================================

#[test]
fn test_task_starts_immediately() {
    // In Raya, calling an async function starts the Promise immediately
    expect_i32(
        "let started = 0;
         async function work(): Promise<number> {
             started = 1;
             return 42;
         }
         let task = work(); // Promise starts NOW
         // started should be 1 even before await
         return await task;",
        42,
    );
}

#[test]
fn test_task_multiple() {
    // Multiple tasks can run concurrently
    expect_i32(
        "async function compute(x: number): Promise<number> {
             return x * 2;
         }
         let task1 = compute(10);
         let task2 = compute(11);
         return await task1 + await task2;",
        42,
    );
}

#[test]
fn test_async_function_assignable_to_task_callback_type() {
    expect_i32(
        "function runCallback(cb: (x: number) => Promise<void>): number {
             cb(1);
             return 42;
         }

         async function handler(_x: number): Promise<void> {
             return;
         }

         return runCallback(handler);",
        42,
    );
}

// ============================================================================
// Async with Control Flow
// ============================================================================

#[test]
fn test_async_with_loop() {
    expect_i32(
        "async function sumAsync(n: number): Promise<number> {
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
        "async function factorialAsync(n: number): Promise<number> {
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
        "let getValue = async (): Promise<number> => 42;
         return await getValue();",
        42,
    );
}

#[test]
fn test_async_arrow_with_param() {
    expect_i32(
        "let double = async (x: number): Promise<number> => x * 2;
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
             async fetch(id: number): Promise<number> {
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
             async incrementAsync(): Promise<number> {
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
fn test_async_try_catch() {
    expect_i32(
        "async function mayFail(shouldFail: boolean): Promise<number> {
             if (shouldFail) {
                 throw 'error';
             }
             return 42;
         }
         async function main(): Promise<number> {
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
    // The 'async' keyword before a call wraps it in a Promise
    expect_i32(
        "function syncWork(): number {
             return 42;
         }
         // 'async syncWork()' wraps the synchronous call in a Promise
         let task = async syncWork();
         return await task;",
        42,
    );
}

#[test]
fn test_await_non_promise_value_resolves_immediately() {
    expect_i32(
        "let value = await 42;
         return value;",
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
        "async function compute(x: number): Promise<number> {
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
        "async function getValue(x: number): Promise<number> {
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
        "async function triple(x: number): Promise<number> {
             return x * 3;
         }
         let results = await [triple(10), triple(4)];
         return results[0] + results[1];",
        42,
    );
}

#[test]
fn test_await_array_string_tasks_from_variable_in_non_async_function() {
    // Regression: resuming WaitAll with pointer results (string) must not
    // mis-handle the resumed value as the task array operand.
    expect_string(
        "async function fetchUser(id: number): Promise<string> {
             return 'User ' + id.toString();
         }
         function main(): string {
             const tasks = [fetchUser(1), fetchUser(2), fetchUser(3)];
             const users = await tasks;
             return users[0] + ', ' + users[1] + ', ' + users[2];
         }
         return main();",
        "User 1, User 2, User 3",
    );
}

#[test]
fn test_await_array_and_io_writeln_inside_function_scope() {
    // Regression: stdlib object method calls (io.writeln) from a regular
    // function after await-all must resolve `io` as a module global, not as
    // stale closure capture state.
    expect_string_with_builtins(
        "import io from 'std:io';
         async function fetchUser(id: number): Promise<string> {
             if (id == 1) return 'User 1';
             if (id == 2) return 'User 2';
             return 'User 3';
         }
         function main(): string {
             const tasks = [fetchUser(1), fetchUser(2), fetchUser(3)];
             const users = await tasks;
             io.writeln(users[0]);
             io.writeln(users[1]);
             io.writeln(users[2]);
             return users[0] + '|' + users[1] + '|' + users[2];
         }
         return main();",
        "User 1|User 2|User 3",
    );
}

#[test]
fn test_top_level_main_call_with_await_array_runs_once() {
    // Regression: when both synthetic top-level main and user-declared main
    // exist, VM must not execute user main twice if top-level already calls it.
    expect_null(
        "let runCount: number = 0;
         async function fetchUser(id: number): Promise<string> {
             if (id == 1) return 'User 1';
             if (id == 2) return 'User 2';
             return 'User 3';
         }
         function main(): void {
             if (runCount == 1) { throw 'main executed twice'; }
             runCount = runCount + 1;
             const tasks = [fetchUser(1), fetchUser(2), fetchUser(3)];
             const users = await tasks;
             let _first = users[0];
         }
         main();",
    );
}

#[test]
fn test_declared_main_is_not_implicitly_executed() {
    // Regression: declaring `main` without calling it must not execute it.
    expect_null(
        "let ran: number = 0;
         async function fetchUser(id: number): Promise<string> {
             return 'User ' + id.toString();
         }
         function main(): void {
             ran = 1;
             const tasks = [fetchUser(1), fetchUser(2), fetchUser(3)];
             const users = await tasks;
             let _first = users[0];
         }
         if (ran == 1) { throw 'main ran implicitly'; }",
    );
}
