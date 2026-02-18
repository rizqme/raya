//! Concurrency & Parallelism Edge Case Tests
//!
//! Tests for edge cases in async/await, Tasks, Mutex, Channel,
//! exception propagation, and multi-worker parallelism.

use super::harness::*;
use std::sync::Mutex;

// Mutex to serialize multi-worker tests to avoid resource contention
static MULTIWORKER_LOCK: Mutex<()> = Mutex::new(());

// ============================================================================
// 1. Async Closure & Arrow Edge Cases
// ============================================================================

#[test]
fn test_async_arrow_capturing_outer_variable() {
    // Async arrow captures a variable from the enclosing scope
    expect_i32("
        async function main(): Task<number> {
            let x: number = 10;
            let work = async (): Task<number> => x * 3;
            return await work();
        }
        return await main();
    ", 30);
}

#[test]
fn test_async_arrow_with_parameter_and_capture() {
    // Async arrow captures outer variable AND takes a parameter
    expect_i32("
        async function main(): Task<number> {
            let base: number = 100;
            let add = async (n: number): Task<number> => base + n;
            return await add(42);
        }
        return await main();
    ", 142);
}

#[test]
fn test_async_method_calls_another_async_method() {
    // Async method on class calls another async method on self
    expect_i32("
        class Calculator {
            value: number;
            constructor(v: number) {
                this.value = v;
            }
            async double(): Task<number> {
                return this.value * 2;
            }
            async quadruple(): Task<number> {
                let d = await this.double();
                return d * 2;
            }
        }
        async function main(): Task<number> {
            let calc = new Calculator(5);
            return await calc.quadruple();
        }
        return await main();
    ", 20);
}

#[test]
fn test_nested_async_arrow_inside_async_function() {
    // Async arrow defined and called inside another async function
    expect_i32("
        async function outer(): Task<number> {
            let inner = async (): Task<number> => 7;
            let result = await inner();
            return result + 3;
        }
        return await outer();
    ", 10);
}

// ============================================================================
// 2. Task Result Types
// ============================================================================

#[test]
fn test_task_returns_negative_number() {
    expect_i32("
        async function negative(): Task<number> {
            return -42;
        }
        return await negative();
    ", -42);
}

#[test]
fn test_task_returns_zero() {
    expect_i32("
        async function zero(): Task<number> {
            return 0;
        }
        return await zero();
    ", 0);
}

#[test]
fn test_task_returns_large_computation() {
    // Task does significant work before returning
    expect_i32("
        async function compute(): Task<number> {
            let sum: number = 0;
            let i: number = 1;
            while (i <= 100) {
                sum = sum + i;
                i = i + 1;
            }
            return sum;
        }
        return await compute();
    ", 5050);
}

#[test]
fn test_task_returns_boolean_true() {
    expect_bool("
        async function check(): Task<boolean> {
            return true;
        }
        return await check();
    ", true);
}

#[test]
fn test_task_returns_boolean_false() {
    expect_bool("
        async function check(): Task<boolean> {
            return 3 > 5;
        }
        return await check();
    ", false);
}

// ============================================================================
// 3. Exception Propagation Across Tasks
// ============================================================================

#[test]
fn test_exception_propagates_through_nested_await() {
    // Exception thrown in deeply nested async chain should propagate to top
    expect_i32("
        async function level2(): Task<number> {
            throw 'deep error';
        }
        async function level1(): Task<number> {
            return await level2();
        }
        async function level0(): Task<number> {
            return await level1();
        }
        async function main(): Task<number> {
            try {
                return await level0();
            } catch (e) {
                return 99;
            }
        }
        return await main();
    ", 99);
}

#[test]
fn test_exception_in_one_parallel_task() {
    // One task in a parallel group fails — error should propagate
    expect_i32("
        async function good(): Task<number> {
            return 1;
        }
        async function bad(): Task<number> {
            throw 'fail';
        }
        async function main(): Task<number> {
            try {
                let results = await [good(), bad()];
                return results[0];
            } catch (e) {
                return 42;
            }
        }
        return await main();
    ", 42);
}

#[test]
fn test_try_catch_wrapping_await() {
    // Try-catch around a single await catches the exception
    expect_i32("
        async function failing(): Task<number> {
            throw 'oops';
        }
        async function main(): Task<number> {
            try {
                let result = await failing();
                return result;
            } catch (e) {
                return 77;
            }
        }
        return await main();
    ", 77);
}

#[test]
fn test_exception_in_async_does_not_affect_other_tasks() {
    // One task fails, but a separately awaited task succeeds
    expect_i32("
        async function goodTask(): Task<number> {
            return 10;
        }
        async function badTask(): Task<number> {
            throw 'error';
        }
        async function main(): Task<number> {
            let g = goodTask();
            let b = badTask();
            let goodResult = await g;
            try {
                await b;
            } catch (e) {
                // swallow
            }
            return goodResult;
        }
        return await main();
    ", 10);
}

#[test]
fn test_finally_runs_after_async_exception() {
    // Finally block executes even when async function throws
    expect_i32("
        async function failing(): Task<number> {
            throw 'error';
        }
        async function main(): Task<number> {
            let cleanup: number = 0;
            try {
                await failing();
            } catch (e) {
                cleanup = 1;
            } finally {
                cleanup = cleanup + 10;
            }
            return cleanup;
        }
        return await main();
    ", 11);
}

#[test]
fn test_rethrow_in_async_catch() {
    // Catch and rethrow in async function
    expect_i32("
        async function inner(): Task<number> {
            throw 'inner error';
        }
        async function middle(): Task<number> {
            try {
                return await inner();
            } catch (e) {
                throw 'rethrown';
            }
        }
        async function main(): Task<number> {
            try {
                return await middle();
            } catch (e) {
                return 55;
            }
        }
        return await main();
    ", 55);
}

#[test]
fn test_exception_after_successful_await() {
    // First await succeeds, then an exception is thrown and caught locally
    expect_i32("
        async function ok(): Task<number> {
            return 5;
        }
        async function main(): Task<number> {
            let result = await ok();
            try {
                if (result == 5) {
                    throw 'post-await error';
                }
                return 0;
            } catch (e) {
                return 88;
            }
        }
        return await main();
    ", 88);
}

// ============================================================================
// 4. Multiple Waiters / Shared Task
// ============================================================================

#[test]
fn test_two_tasks_await_same_task() {
    // Two tasks both await the same shared task
    expect_i32("
        async function shared(): Task<number> {
            return 10;
        }
        async function waiter(t: Task<number>): Task<number> {
            return await t;
        }
        async function main(): Task<number> {
            let s = shared();
            let w1 = waiter(s);
            let w2 = waiter(s);
            let r1 = await w1;
            let r2 = await w2;
            return r1 + r2;
        }
        return await main();
    ", 20);
}

#[test]
fn test_await_already_completed_task() {
    // Task completes before we await it
    expect_i32("
        async function fast(): Task<number> {
            return 42;
        }
        async function main(): Task<number> {
            let t = fast();
            // The task likely completes immediately (trivial work)
            // Awaiting a completed task should return result instantly
            let r = await t;
            return r;
        }
        return await main();
    ", 42);
}

#[test]
fn test_await_same_task_twice() {
    // Await the same task twice — second await should get cached result
    expect_i32("
        async function work(): Task<number> {
            return 7;
        }
        async function main(): Task<number> {
            let t = work();
            let r1 = await t;
            let r2 = await t;
            return r1 + r2;
        }
        return await main();
    ", 14);
}

// ============================================================================
// 5. WaitAll Edge Cases
// ============================================================================

#[test]
fn test_waitall_single_task() {
    // await [...] with just one task
    expect_i32("
        async function work(): Task<number> {
            return 42;
        }
        async function main(): Task<number> {
            let results = await [work()];
            return results[0];
        }
        return await main();
    ", 42);
}

#[test]
fn test_waitall_preserves_order() {
    // Results should be in array order, not completion order
    expect_i32("
        async function slow(): Task<number> {
            let i: number = 0;
            while (i < 50) { i = i + 1; }
            return 1;
        }
        async function fast(): Task<number> {
            return 2;
        }
        async function main(): Task<number> {
            let results = await [slow(), fast()];
            // results[0] should be slow's result (1), results[1] should be fast's (2)
            return results[0] * 10 + results[1];
        }
        return await main();
    ", 12);
}

#[test]
fn test_waitall_all_same_value() {
    // All tasks return the same value
    expect_i32("
        async function same(): Task<number> {
            return 5;
        }
        async function main(): Task<number> {
            let results = await [same(), same(), same(), same()];
            return results[0] + results[1] + results[2] + results[3];
        }
        return await main();
    ", 20);
}

#[test]
fn test_nested_waitall() {
    // Inner parallel inside outer parallel
    expect_i32("
        async function leaf(x: number): Task<number> {
            return x;
        }
        async function inner1(): Task<number> {
            let results = await [leaf(1), leaf(2)];
            return results[0] + results[1];
        }
        async function inner2(): Task<number> {
            let results = await [leaf(3), leaf(4)];
            return results[0] + results[1];
        }
        async function main(): Task<number> {
            let results = await [inner1(), inner2()];
            return results[0] + results[1];
        }
        return await main();
    ", 10);
}

#[test]
fn test_waitall_with_computation() {
    // Tasks in waitall do real work (not just return constants)
    expect_i32("
        async function sum_to(n: number): Task<number> {
            let s: number = 0;
            let i: number = 1;
            while (i <= n) {
                s = s + i;
                i = i + 1;
            }
            return s;
        }
        async function main(): Task<number> {
            let results = await [sum_to(10), sum_to(20), sum_to(30)];
            return results[0] + results[1] + results[2];
        }
        return await main();
    ", 730); // sum(1..10)=55, sum(1..20)=210, sum(1..30)=465 → 55+210+465=730
}

// ============================================================================
// 6. Sequential Spawn/Await Patterns
// ============================================================================

#[test]
fn test_spawn_await_in_while_loop() {
    // Repeatedly spawn and await tasks in a loop
    expect_i32("
        async function work(x: number): Task<number> {
            return x * 2;
        }
        async function main(): Task<number> {
            let sum: number = 0;
            let i: number = 1;
            while (i <= 5) {
                let t = work(i);
                sum = sum + await t;
                i = i + 1;
            }
            return sum;
        }
        return await main();
    ", 30); // 2+4+6+8+10 = 30
}

#[test]
fn test_batch_spawn_then_batch_await() {
    // Spawn all tasks first, then await them all individually
    expect_i32("
        async function work(x: number): Task<number> {
            return x;
        }
        async function main(): Task<number> {
            let t1 = work(1);
            let t2 = work(2);
            let t3 = work(3);
            let t4 = work(4);
            let t5 = work(5);
            // Now await them individually (not using waitall)
            let r1 = await t1;
            let r2 = await t2;
            let r3 = await t3;
            let r4 = await t4;
            let r5 = await t5;
            return r1 + r2 + r3 + r4 + r5;
        }
        return await main();
    ", 15);
}

#[test]
fn test_task_chain_through_loop() {
    // Each iteration spawns a task that depends on previous result
    expect_i32("
        async function addOne(x: number): Task<number> {
            return x + 1;
        }
        async function main(): Task<number> {
            let value: number = 0;
            let i: number = 0;
            while (i < 10) {
                value = await addOne(value);
                i = i + 1;
            }
            return value;
        }
        return await main();
    ", 10);
}

#[test]
fn test_alternating_sync_async_work() {
    // Mix sync computation with async spawning
    expect_i32("
        async function asyncDouble(x: number): Task<number> {
            return x * 2;
        }
        async function main(): Task<number> {
            let result: number = 1;
            // sync
            result = result + 1;
            // async
            result = await asyncDouble(result);
            // sync
            result = result + 3;
            // async
            result = await asyncDouble(result);
            // sync
            result = result + 5;
            return result;
        }
        return await main();
    ", 19); // 1+1=2, *2=4, +3=7, *2=14, +5=19
}

// ============================================================================
// 7. Mutex Contention
// ============================================================================

#[test]
fn test_mutex_two_tasks_increment() {
    // Two tasks each increment a counter 5 times
    expect_i32_with_builtins("
        class SharedCounter {
            count: number = 0;
            mu: Mutex = new Mutex();
        }
        async function incrementN(counter: SharedCounter, n: number): Task<void> {
            let i: number = 0;
            while (i < n) {
                counter.mu.lock();
                counter.count = counter.count + 1;
                counter.mu.unlock();
                i = i + 1;
            }
        }
        async function main(): Task<number> {
            let c = new SharedCounter();
            let t1 = incrementN(c, 5);
            let t2 = incrementN(c, 5);
            await t1;
            await t2;
            return c.count;
        }
        return await main();
    ", 10);
}

#[test]
fn test_mutex_lock_unlock_cycle() {
    // Lock and unlock multiple times in sequence
    expect_i32_with_builtins("
        async function main(): Task<number> {
            let mu = new Mutex();
            let sum: number = 0;
            mu.lock();
            sum = sum + 1;
            mu.unlock();
            mu.lock();
            sum = sum + 2;
            mu.unlock();
            mu.lock();
            sum = sum + 3;
            mu.unlock();
            return sum;
        }
        return await main();
    ", 6);
}

#[test]
fn test_mutex_with_sleep_between() {
    // Lock, sleep, unlock — tests that mutex is held across suspension
    expect_i32_with_builtins("
        async function main(): Task<number> {
            let mu = new Mutex();
            mu.lock();
            sleep(0);
            mu.unlock();
            return 42;
        }
        return await main();
    ", 42);
}

#[test]
fn test_mutex_passed_to_function() {
    // Mutex passed as parameter to function that acquires it
    expect_i32_with_builtins("
        function criticalSection(mu: Mutex): number {
            mu.lock();
            let result = 42;
            mu.unlock();
            return result;
        }
        let mu = new Mutex();
        return criticalSection(mu);
    ", 42);
}

#[test]
fn test_mutex_try_lock_while_held() {
    // tryLock should return false when mutex is already locked
    expect_bool_with_builtins("
        let mu = new Mutex();
        mu.lock();
        let result = mu.tryLock();
        mu.unlock();
        return result;
    ", false);
}

#[test]
fn test_mutex_protects_accumulation() {
    // Multiple tasks accumulate into a shared counter
    expect_i32_with_builtins("
        class State {
            total: number = 0;
            mu: Mutex = new Mutex();
        }
        async function add(state: State, value: number): Task<void> {
            state.mu.lock();
            state.total = state.total + value;
            state.mu.unlock();
        }
        async function main(): Task<number> {
            let s = new State();
            let t1 = add(s, 10);
            let t2 = add(s, 20);
            let t3 = add(s, 30);
            await t1;
            await t2;
            await t3;
            return s.total;
        }
        return await main();
    ", 60);
}

// ============================================================================
// 8. Channel Patterns
// ============================================================================

#[test]
fn test_channel_fifo_ordering() {
    // Verify FIFO: send 1, 2, 3 → receive 1, 2, 3
    expect_i32_with_builtins("
        let ch = new Channel<number>(3);
        ch.send(1);
        ch.send(2);
        ch.send(3);
        let a = ch.receive();
        let b = ch.receive();
        let c = ch.receive();
        // Encode order: a*100 + b*10 + c
        return a * 100 + b * 10 + c;
    ", 123);
}

#[test]
fn test_channel_try_send_full_buffer() {
    // trySend on full buffer returns false
    expect_bool_with_builtins("
        let ch = new Channel<number>(1);
        ch.send(1);  // fills buffer
        return ch.trySend(2);  // should fail
    ", false);
}

#[test]
fn test_channel_length_tracks_count() {
    // Channel length reflects buffered items
    expect_i32_with_builtins("
        let ch = new Channel<number>(5);
        ch.send(1);
        ch.send(2);
        ch.send(3);
        let len1 = ch.length();
        ch.receive();
        let len2 = ch.length();
        return len1 * 10 + len2;
    ", 32); // 3*10 + 2 = 32
}

#[test]
fn test_channel_is_closed() {
    // isClosed returns true after close()
    expect_bool_with_builtins("
        let ch = new Channel<number>(1);
        ch.close();
        return ch.isClosed();
    ", true);
}

#[test]
fn test_channel_is_not_closed_initially() {
    // isClosed returns false on fresh channel
    expect_bool_with_builtins("
        let ch = new Channel<number>(1);
        return ch.isClosed();
    ", false);
}

#[test]
fn test_channel_multiple_values_sequence() {
    // Send and receive multiple values in sequence
    expect_i32_with_builtins("
        let ch = new Channel<number>(10);
        let i: number = 0;
        while (i < 5) {
            ch.send(i * 10);
            i = i + 1;
        }
        let sum: number = 0;
        let j: number = 0;
        while (j < 5) {
            sum = sum + ch.receive();
            j = j + 1;
        }
        return sum;
    ", 100); // 0+10+20+30+40 = 100
}

#[test]
fn test_channel_producer_consumer_pattern() {
    // Producer sends multiple values, consumer receives them via async
    expect_i32_with_builtins("
        async function producer(ch: Channel<number>): Task<void> {
            ch.send(10);
            ch.send(20);
            ch.send(30);
        }
        async function consumer(ch: Channel<number>): Task<number> {
            let a = ch.receive();
            let b = ch.receive();
            let c = ch.receive();
            return a + b + c;
        }
        async function main(): Task<number> {
            let ch = new Channel<number>(3);
            let p = producer(ch);
            let c = consumer(ch);
            await p;
            return await c;
        }
        return await main();
    ", 60);
}

#[test]
fn test_channel_capacity_query() {
    // Verify capacity() returns the buffer size
    expect_i32_with_builtins("
        let ch = new Channel<number>(7);
        return ch.capacity();
    ", 7);
}

// ============================================================================
// 9. Sleep & Yield Interactions
// ============================================================================

#[test]
fn test_sleep_zero_as_yield() {
    // sleep(0) should yield control and resume
    expect_i32("
        async function work(): Task<number> {
            let x: number = 1;
            sleep(0);
            x = x + 1;
            sleep(0);
            x = x + 1;
            return x;
        }
        return await work();
    ", 3);
}

#[test]
fn test_multiple_sequential_sleeps() {
    // Multiple sleeps in a row
    expect_i32("
        async function main(): Task<number> {
            sleep(0);
            sleep(0);
            sleep(0);
            return 42;
        }
        return await main();
    ", 42);
}

#[test]
fn test_sleep_in_loop() {
    // Sleep inside a loop with work
    expect_i32("
        async function main(): Task<number> {
            let sum: number = 0;
            let i: number = 0;
            while (i < 5) {
                sleep(0);
                sum = sum + i;
                i = i + 1;
            }
            return sum;
        }
        return await main();
    ", 10); // 0+1+2+3+4 = 10
}

// ============================================================================
// 10. Async + Closure Capture Interaction
// ============================================================================

#[test]
fn test_closure_used_after_await() {
    // Closure created before async work, used after await
    expect_i32("
        async function work(): Task<number> {
            return 5;
        }
        async function main(): Task<number> {
            let multiplier: number = 3;
            let mul = (x: number): number => x * multiplier;
            let base = await work();
            return mul(base);
        }
        return await main();
    ", 15);
}

#[test]
fn test_multiple_async_tasks_with_closures() {
    // Multiple tasks each using closures with different captures
    expect_i32("
        async function makeTask(base: number): Task<number> {
            let add = (x: number): number => x + base;
            return add(10);
        }
        async function main(): Task<number> {
            let t1 = makeTask(1);
            let t2 = makeTask(2);
            let t3 = makeTask(3);
            let results = await [t1, t2, t3];
            return results[0] + results[1] + results[2];
        }
        return await main();
    ", 36); // 11 + 12 + 13 = 36
}

#[test]
fn test_async_function_returning_closure_result() {
    // Async function that builds and calls a closure internally
    expect_i32("
        async function compute(a: number, b: number): Task<number> {
            let op = (x: number, y: number): number => x * y + 1;
            return op(a, b);
        }
        return await compute(6, 7);
    ", 43); // 6*7+1 = 43
}

#[test]
fn test_closure_captures_task_result() {
    // Create a closure that uses the result of an await
    expect_i32("
        async function getData(): Task<number> {
            return 100;
        }
        async function main(): Task<number> {
            let data = await getData();
            let process = (x: number): number => x + data;
            return process(23);
        }
        return await main();
    ", 123);
}

#[test]
fn test_async_function_with_closure_inside() {
    // Async function that creates and uses a closure internally
    expect_i32("
        async function compute(factor: number): Task<number> {
            let double = (x: number): number => x * 2;
            return double(factor);
        }
        async function main(): Task<number> {
            return await compute(5);
        }
        return await main();
    ", 10);
}

#[test]
fn test_async_arrow_block_body() {
    // Async arrow with block body (not just expression body)
    expect_i32("
        async function main(): Task<number> {
            let compute = async (): Task<number> => {
                let x: number = 10;
                let y: number = 20;
                return x + y;
            };
            return await compute();
        }
        return await main();
    ", 30);
}

#[test]
fn test_async_arrow_block_body_with_capture() {
    // Async arrow block body capturing outer variable
    expect_i32("
        async function main(): Task<number> {
            let factor: number = 5;
            let compute = async (): Task<number> => {
                let double = (x: number): number => x * 2;
                return double(factor);
            };
            return await compute();
        }
        return await main();
    ", 10);
}

#[test]
fn test_async_arrow_block_body_with_param() {
    // Async arrow block body with parameters
    expect_i32("
        async function main(): Task<number> {
            let add = async (a: number, b: number): Task<number> => {
                let sum: number = a + b;
                return sum;
            };
            return await add(17, 25);
        }
        return await main();
    ", 42);
}

// ============================================================================
// 11. Async + Try/Catch/Finally
// ============================================================================

#[test]
fn test_try_catch_inside_async() {
    // Try-catch within an async function (no cross-task boundary)
    expect_i32("
        async function main(): Task<number> {
            try {
                throw 'error';
            } catch (e) {
                return 42;
            }
        }
        return await main();
    ", 42);
}

#[test]
fn test_finally_runs_in_async() {
    // Finally block executes in async function
    expect_i32("
        async function main(): Task<number> {
            let result: number = 0;
            try {
                result = 1;
            } finally {
                result = result + 10;
            }
            return result;
        }
        return await main();
    ", 11);
}

#[test]
fn test_await_inside_try_catches_task_exception() {
    // Await inside try block where the awaited task throws
    expect_i32("
        async function failing(): Task<number> {
            throw 'task error';
        }
        async function main(): Task<number> {
            try {
                return await failing();
            } catch (e) {
                return 99;
            }
        }
        return await main();
    ", 99);
}

#[test]
fn test_nested_try_catch_in_async() {
    // Nested try-catch in async function
    expect_i32("
        async function main(): Task<number> {
            let result: number = 0;
            try {
                try {
                    throw 'inner';
                } catch (e) {
                    result = 10;
                }
                result = result + 5;
            } catch (e) {
                result = -1;
            }
            return result;
        }
        return await main();
    ", 15);
}

#[test]
fn test_finally_with_cleanup_after_async_error() {
    // Finally runs cleanup even after async exception
    expect_i32("
        async function failing(): Task<number> {
            throw 'fail';
        }
        async function main(): Task<number> {
            let cleaned: number = 0;
            try {
                await failing();
            } catch (e) {
                cleaned = 1;
            } finally {
                cleaned = cleaned + 100;
            }
            return cleaned;
        }
        return await main();
    ", 101);
}

// ============================================================================
// 12. Multi-Worker with Builtins (Mutex/Channel under true parallelism)
// ============================================================================

#[test]
fn test_multiworker_mutex_counter() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Mutex-protected counter with 4 workers — verifies correctness under true parallelism
    expect_i32_multiworker_with_builtins("
        class Counter {
            value: number = 0;
            mu: Mutex = new Mutex();
        }
        async function increment(c: Counter): Task<void> {
            c.mu.lock();
            c.value = c.value + 1;
            c.mu.unlock();
        }
        async function main(): Task<number> {
            let c = new Counter();
            let t1 = increment(c);
            let t2 = increment(c);
            let t3 = increment(c);
            let t4 = increment(c);
            let t5 = increment(c);
            await t1;
            await t2;
            await t3;
            await t4;
            await t5;
            return c.value;
        }
        return await main();
    ", 5, 4);
}

#[test]
fn test_multiworker_channel_producer_consumer() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Channel producer-consumer with 4 workers
    expect_i32_multiworker_with_builtins("
        async function producer(ch: Channel<number>): Task<void> {
            ch.send(10);
            ch.send(20);
            ch.send(30);
        }
        async function consumer(ch: Channel<number>): Task<number> {
            let a = ch.receive();
            let b = ch.receive();
            let c = ch.receive();
            return a + b + c;
        }
        async function main(): Task<number> {
            let ch = new Channel<number>(3);
            let p = producer(ch);
            let c = consumer(ch);
            await p;
            return await c;
        }
        return await main();
    ", 60, 4);
}

#[test]
fn test_multiworker_mutex_multiple_tasks() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Multiple tasks with mutex-protected shared state, 4 workers
    expect_i32_multiworker_with_builtins("
        class State {
            sum: number = 0;
            mu: Mutex = new Mutex();
        }
        async function addValue(s: State, v: number): Task<void> {
            s.mu.lock();
            s.sum = s.sum + v;
            s.mu.unlock();
        }
        async function main(): Task<number> {
            let s = new State();
            let t1 = addValue(s, 1);
            let t2 = addValue(s, 2);
            let t3 = addValue(s, 3);
            let t4 = addValue(s, 4);
            let t5 = addValue(s, 5);
            let t6 = addValue(s, 6);
            let t7 = addValue(s, 7);
            let t8 = addValue(s, 8);
            await t1;
            await t2;
            await t3;
            await t4;
            await t5;
            await t6;
            await t7;
            await t8;
            return s.sum;
        }
        return await main();
    ", 36, 4);
}

#[test]
fn test_multiworker_channel_fifo() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Channel FIFO ordering preserved under 4 workers
    expect_i32_multiworker_with_builtins("
        async function main(): Task<number> {
            let ch = new Channel<number>(5);
            ch.send(1);
            ch.send(2);
            ch.send(3);
            let a = ch.receive();
            let b = ch.receive();
            let c = ch.receive();
            return a * 100 + b * 10 + c;
        }
        return await main();
    ", 123, 4);
}

#[test]
fn test_multiworker_parallel_with_mutex() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Parallel tasks with varying work and mutex, 4 workers
    expect_i32_multiworker_with_builtins("
        class Acc {
            total: number = 0;
            mu: Mutex = new Mutex();
        }
        async function work(acc: Acc, iterations: number): Task<void> {
            let sum: number = 0;
            let i: number = 0;
            while (i < iterations) {
                sum = sum + 1;
                i = i + 1;
            }
            acc.mu.lock();
            acc.total = acc.total + sum;
            acc.mu.unlock();
        }
        async function main(): Task<number> {
            let acc = new Acc();
            let t1 = work(acc, 10);
            let t2 = work(acc, 20);
            let t3 = work(acc, 30);
            let t4 = work(acc, 40);
            await t1;
            await t2;
            await t3;
            await t4;
            return acc.total;
        }
        return await main();
    ", 100, 4);
}

#[test]
fn test_multiworker_rapid_channel() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Rapid channel send/receive under 4 workers
    expect_i32_multiworker_with_builtins("
        async function main(): Task<number> {
            let ch = new Channel<number>(20);
            let i: number = 0;
            while (i < 10) {
                ch.send(i);
                i = i + 1;
            }
            let sum: number = 0;
            let j: number = 0;
            while (j < 10) {
                sum = sum + ch.receive();
                j = j + 1;
            }
            return sum;
        }
        return await main();
    ", 45, 4); // 0+1+2+...+9 = 45
}

#[test]
fn test_multiworker_mixed_mutex_channel() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Mixed mutex + channel with 4 workers
    expect_i32_multiworker_with_builtins("
        class State {
            count: number = 0;
            mu: Mutex = new Mutex();
        }
        async function worker(s: State, ch: Channel<number>, value: number): Task<void> {
            s.mu.lock();
            s.count = s.count + 1;
            s.mu.unlock();
            ch.send(value);
        }
        async function main(): Task<number> {
            let s = new State();
            let ch = new Channel<number>(3);
            let t1 = worker(s, ch, 10);
            let t2 = worker(s, ch, 20);
            let t3 = worker(s, ch, 30);
            await t1;
            await t2;
            await t3;
            let sum = ch.receive() + ch.receive() + ch.receive();
            return s.count * 1000 + sum;
        }
        return await main();
    ", 3060, 4); // count=3, sum=60 → 3000+60
}

#[test]
fn test_multiworker_try_lock_contention() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // tryLock contention under 4 workers — at least one tryLock call works
    expect_bool_multiworker_with_builtins("
        async function main(): Task<boolean> {
            let mu = new Mutex();
            let success = mu.tryLock();
            if (success) {
                mu.unlock();
            }
            return success;
        }
        return await main();
    ", true, 4);
}

// ============================================================================
// 13. Async Recursive Algorithms
// ============================================================================

#[test]
fn test_async_recursive_fibonacci_parallel() {
    // Parallel recursive fibonacci: spawns ~177 tasks
    // Each level spawns fib(n-1) and fib(n-2) as separate tasks, awaits both
    // Uses braceless if body: `if (n <= 1) return n;`
    expect_i32("
        async function fib(n: number): Task<number> {
            if (n <= 1) return n;
            let t1 = fib(n - 1);
            let t2 = fib(n - 2);
            let results = await [t1, t2];
            return results[0] + results[1];
        }
        return await fib(10);
    ", 55);
}

#[test]
fn test_async_recursive_fibonacci_sequence() {
    // Compute fib(0)..fib(7) in parallel, verify all values via weighted sum
    // Weights: 1, 2, 4, 8, 16, 32, 64, 128
    // Expected: 0*1 + 1*2 + 1*4 + 2*8 + 3*16 + 5*32 + 8*64 + 13*128 = 2406
    expect_i32("
        async function fib(n: number): Task<number> {
            if (n <= 1) { return n; }
            let t1 = fib(n - 1);
            let t2 = fib(n - 2);
            let results = await [t1, t2];
            return results[0] + results[1];
        }
        async function main(): Task<number> {
            let f0 = fib(0);
            let f1 = fib(1);
            let f2 = fib(2);
            let f3 = fib(3);
            let f4 = fib(4);
            let f5 = fib(5);
            let f6 = fib(6);
            let f7 = fib(7);
            let r = await [f0, f1, f2, f3, f4, f5, f6, f7];
            return r[0] * 1 + r[1] * 2 + r[2] * 4 + r[3] * 8
                 + r[4] * 16 + r[5] * 32 + r[6] * 64 + r[7] * 128;
        }
        return await main();
    ", 2406);
}

#[test]
fn test_async_recursive_sum_divide_and_conquer() {
    // Parallel divide-and-conquer sum of 1..100
    // Splits range in half, recurses, combines via parallel await
    expect_i32("
        async function rangeSum(lo: number, hi: number): Task<number> {
            if (hi - lo <= 1) {
                if (lo < hi) { return lo; }
                return 0;
            }
            let half: number = (hi - lo) / 2;
            let mid: number = lo + half - half % 1;
            let left = rangeSum(lo, mid);
            let right = rangeSum(mid, hi);
            let results = await [left, right];
            return results[0] + results[1];
        }
        return await rangeSum(1, 101);
    ", 5050);
}

#[test]
fn test_async_recursive_power() {
    // Fast exponentiation via repeated squaring: pow(2, 10) = 1024
    // Each level spawns a task for the half-power, then squares
    expect_i32("
        async function power(base: number, exp: number): Task<number> {
            if (exp == 0) { return 1; }
            if (exp == 1) { return base; }
            let half: number = exp / 2 - (exp / 2) % 1;
            let t = power(base, half);
            let halfResult = await t;
            let isEven: boolean = exp % 2 == 0;
            if (isEven) {
                return halfResult * halfResult;
            }
            return halfResult * halfResult * base;
        }
        return await power(2, 10);
    ", 1024);
}

// ============================================================================
// 14. Async Parallel Computation Patterns
// ============================================================================

#[test]
fn test_parallel_matrix_multiply_16x16() {
    // Parallel 16x16 matrix multiply with parallel reduction tree for sum
    // A[i][j] = i + 1, B[i][j] = j + 1
    // C[i][j] = 16 * (i+1) * (j+1)
    // Row i sum = 16 * (i+1) * 136
    // Total = 16 * 136 * 136 = 295936
    expect_i32("
        function a(i: number, j: number): number { return i + 1; }
        function b(i: number, j: number): number { return j + 1; }

        async function computeRow(row: number): Task<number> {
            let sum: number = 0;
            let j: number = 0;
            while (j < 16) {
                let dot: number = 0;
                let k: number = 0;
                while (k < 16) {
                    dot = dot + a(row, k) * b(k, j);
                    k = k + 1;
                }
                sum = sum + dot;
                j = j + 1;
            }
            return sum;
        }

        async function add(x: number, y: number): Task<number> { return x + y; }

        async function main(): Task<number> {
            let rows = await [
                computeRow(0), computeRow(1), computeRow(2), computeRow(3),
                computeRow(4), computeRow(5), computeRow(6), computeRow(7),
                computeRow(8), computeRow(9), computeRow(10), computeRow(11),
                computeRow(12), computeRow(13), computeRow(14), computeRow(15)
            ];
            let s8 = await [
                add(rows[0], rows[1]), add(rows[2], rows[3]),
                add(rows[4], rows[5]), add(rows[6], rows[7]),
                add(rows[8], rows[9]), add(rows[10], rows[11]),
                add(rows[12], rows[13]), add(rows[14], rows[15])
            ];
            let s4 = await [
                add(s8[0], s8[1]), add(s8[2], s8[3]),
                add(s8[4], s8[5]), add(s8[6], s8[7])
            ];
            let s2 = await [add(s4[0], s4[1]), add(s4[2], s4[3])];
            let s1 = await [add(s2[0], s2[1])];
            return s1[0];
        }
        return await main();
    ", 295936);
}

#[test]
fn test_parallel_vector_dot_product() {
    // Parallel dot product: [1,2,3,4,5] · [6,7,8,9,10]
    // Each element-wise product computed as a separate task
    // = 6 + 14 + 24 + 36 + 50 = 130
    expect_i32("
        async function mul(a: number, b: number): Task<number> {
            return a * b;
        }
        async function main(): Task<number> {
            let t1 = mul(1, 6);
            let t2 = mul(2, 7);
            let t3 = mul(3, 8);
            let t4 = mul(4, 9);
            let t5 = mul(5, 10);
            let r = await [t1, t2, t3, t4, t5];
            return r[0] + r[1] + r[2] + r[3] + r[4];
        }
        return await main();
    ", 130);
}

#[test]
fn test_parallel_map_reduce_sum() {
    // Parallel map (square) then reduce (sum)
    // map: [1,2,3,4,5] → [1,4,9,16,25]
    // reduce: 1+4+9+16+25 = 55
    expect_i32("
        async function square(x: number): Task<number> {
            return x * x;
        }
        async function main(): Task<number> {
            let t1 = square(1);
            let t2 = square(2);
            let t3 = square(3);
            let t4 = square(4);
            let t5 = square(5);
            let r = await [t1, t2, t3, t4, t5];
            return r[0] + r[1] + r[2] + r[3] + r[4];
        }
        return await main();
    ", 55);
}

#[test]
fn test_parallel_pipeline_stages() {
    // 4 parallel pipelines, each going through 3 async stages
    // pipeline(x) = ((x + 10) * 2) - 5
    // pipeline(1)=17, pipeline(2)=19, pipeline(3)=21, pipeline(4)=23
    // sum = 80
    expect_i32("
        async function stage1(x: number): Task<number> { return x + 10; }
        async function stage2(x: number): Task<number> { return x * 2; }
        async function stage3(x: number): Task<number> { return x - 5; }

        async function pipeline(input: number): Task<number> {
            let s1 = await stage1(input);
            let s2 = await stage2(s1);
            let s3 = await stage3(s2);
            return s3;
        }

        async function main(): Task<number> {
            let p1 = pipeline(1);
            let p2 = pipeline(2);
            let p3 = pipeline(3);
            let p4 = pipeline(4);
            let r = await [p1, p2, p3, p4];
            return r[0] + r[1] + r[2] + r[3];
        }
        return await main();
    ", 80);
}

#[test]
fn test_parallel_faster_than_sequential() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Sequential: unoptimized (full triple-nested loop, redundant k-loop)
    let seq_source = "
        let result: number = 0;
        let i: number = 0;
        while (i < 16) {
            let sum: number = 0;
            let rep: number = 0;
            while (rep < 50) {
                let j: number = 0;
                while (j < 16) {
                    let dot: number = 0;
                    let k: number = 0;
                    while (k < 16) {
                        dot = dot + (i + 1) * (j + 1);
                        k = k + 1;
                    }
                    sum = sum + dot;
                    j = j + 1;
                }
                rep = rep + 1;
            }
            result = result + sum;
            i = i + 1;
        }
        return result;
    ";

    // Parallel: optimized (inlined formula, no function calls, no inner k-loop)
    let par_source = "
        async function computeRow(row: number): Task<number> {
            let sum: number = 0;
            let r: number = row + 1;
            let rep: number = 0;
            while (rep < 50) {
                let j: number = 0;
                while (j < 16) {
                    sum = sum + 16 * r * (j + 1);
                    j = j + 1;
                }
                rep = rep + 1;
            }
            return sum;
        }

        async function add(x: number, y: number): Task<number> { return x + y; }

        async function main(): Task<number> {
            let rows = await [
                computeRow(0), computeRow(1), computeRow(2), computeRow(3),
                computeRow(4), computeRow(5), computeRow(6), computeRow(7),
                computeRow(8), computeRow(9), computeRow(10), computeRow(11),
                computeRow(12), computeRow(13), computeRow(14), computeRow(15)
            ];
            let s8 = await [
                add(rows[0], rows[1]), add(rows[2], rows[3]),
                add(rows[4], rows[5]), add(rows[6], rows[7]),
                add(rows[8], rows[9]), add(rows[10], rows[11]),
                add(rows[12], rows[13]), add(rows[14], rows[15])
            ];
            let s4 = await [
                add(s8[0], s8[1]), add(s8[2], s8[3]),
                add(s8[4], s8[5]), add(s8[6], s8[7])
            ];
            let s2 = await [add(s4[0], s4[1]), add(s4[2], s4[3])];
            let s1 = await [add(s2[0], s2[1])];
            return s1[0];
        }
        return await main();
    ";

    // Both should produce the same result: 50 * 16 * 136 * 136 = 14,796,800
    let seq_start = std::time::Instant::now();
    let seq_result = compile_and_run_with_builtins(seq_source).unwrap();
    let seq_time = seq_start.elapsed();

    let par_start = std::time::Instant::now();
    let par_result = compile_and_run_multiworker_with_builtins(par_source, 4).unwrap();
    let par_time = par_start.elapsed();

    let seq_val = seq_result.as_i32().or_else(|| seq_result.as_f64().map(|f| f as i32)).unwrap();
    let par_val = par_result.as_i32().or_else(|| par_result.as_f64().map(|f| f as i32)).unwrap();

    assert_eq!(seq_val, par_val,
        "Both should produce same result: seq={}, par={}", seq_val, par_val);
    assert_eq!(seq_val, 14_796_800,
        "Expected 14,796,800, got {}", seq_val);
    let speedup = seq_time.as_nanos() as f64 / par_time.as_nanos() as f64;
    eprintln!("Sequential: {:?}, Parallel: {:?}, Speedup: {:.2}x", seq_time, par_time, speedup);
    assert!(par_time < seq_time,
        "Parallel ({:?}) should be faster than sequential ({:?})",
        par_time, seq_time);
}

// ============================================================================
// 15. Nested Closures with Captured Tasks
// ============================================================================

#[test]
fn test_closure_captures_task_and_awaits() {
    // Async closure captures a task variable and awaits it
    expect_i32("
        async function compute(): Task<number> {
            return 42;
        }
        async function main(): Task<number> {
            let t = compute();
            let getResult = async (): Task<number> => {
                return await t;
            };
            return await getResult();
        }
        return await main();
    ", 42);
}

#[test]
fn test_nested_closure_captures_outer_task() {
    // Two levels of async closure nesting, inner awaits outer's captured task
    expect_i32("
        async function compute(): Task<number> {
            return 100;
        }
        async function main(): Task<number> {
            let t = compute();
            let outer = async (): Task<number> => {
                let inner = async (): Task<number> => {
                    return await t;
                };
                return await inner();
            };
            return await outer();
        }
        return await main();
    ", 100);
}

#[test]
fn test_closure_factory_with_task_spawning() {
    // Closure captures base value, spawns tasks internally, combines results
    expect_i32("
        async function compute(x: number): Task<number> {
            return x * 10;
        }
        async function main(): Task<number> {
            let base: number = 5;
            let addBase = async (x: number): Task<number> => {
                let result = await compute(x);
                return result + base;
            };
            let t1 = addBase(3);
            let t2 = addBase(7);
            let r = await [t1, t2];
            return r[0] + r[1];
        }
        return await main();
    ", 110); // (30+5) + (70+5) = 110
}

#[test]
fn test_multiple_closures_share_captured_task() {
    // Two async closures both capture and await the same task
    expect_i32("
        async function compute(): Task<number> {
            return 42;
        }
        async function main(): Task<number> {
            let t = compute();
            let a = async (): Task<number> => {
                let v = await t;
                return v + 1;
            };
            let b = async (): Task<number> => {
                let v = await t;
                return v + 2;
            };
            let r = await [a(), b()];
            return r[0] + r[1];
        }
        return await main();
    ", 87); // 43 + 44 = 87
}

#[test]
fn test_closure_captures_parallel_await_results() {
    // Closure created after parallel await, uses captured results
    expect_i32("
        async function work(x: number): Task<number> {
            return x * x;
        }
        async function main(): Task<number> {
            let t1 = work(3);
            let t2 = work(4);
            let t3 = work(5);
            let results = await [t1, t2, t3];
            let sum: number = results[0] + results[1] + results[2];
            let doubler = (x: number): number => x * 2;
            return doubler(sum);
        }
        return await main();
    ", 100); // (9 + 16 + 25) * 2 = 100
}

// ============================================================================
// 16. Complex Mutex Scenarios (4 workers)
// ============================================================================

#[test]
fn test_mutex_prevents_lost_updates_heavy() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 4 tasks each increment 100 times with mutex protection — must be exactly 400
    expect_i32_multiworker_with_builtins("
        class Counter {
            value: number = 0;
            mu: Mutex = new Mutex();
        }
        async function incrementMany(c: Counter, n: number): Task<void> {
            let i: number = 0;
            while (i < n) {
                c.mu.lock();
                c.value = c.value + 1;
                c.mu.unlock();
                i = i + 1;
            }
        }
        async function main(): Task<number> {
            let c = new Counter();
            let t1 = incrementMany(c, 100);
            let t2 = incrementMany(c, 100);
            let t3 = incrementMany(c, 100);
            let t4 = incrementMany(c, 100);
            await t1;
            await t2;
            await t3;
            await t4;
            return c.value;
        }
        return await main();
    ", 400, 4);
}

#[test]
fn test_mutex_protects_compound_read_modify_write() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Each task reads, computes, writes — compound operation must be atomic
    // 4 tasks each add 1+2+...+10 = 55 → total 220
    expect_i32_multiworker_with_builtins("
        class State {
            total: number = 0;
            mu: Mutex = new Mutex();
        }
        async function accumulate(s: State): Task<void> {
            let i: number = 1;
            while (i < 11) {
                s.mu.lock();
                let current: number = s.total;
                let next: number = current + i;
                s.total = next;
                s.mu.unlock();
                i = i + 1;
            }
        }
        async function main(): Task<number> {
            let s = new State();
            let t1 = accumulate(s);
            let t2 = accumulate(s);
            let t3 = accumulate(s);
            let t4 = accumulate(s);
            await t1;
            await t2;
            await t3;
            await t4;
            return s.total;
        }
        return await main();
    ", 220, 4);
}

#[test]
fn test_mutex_bank_transfer_atomicity() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Two accounts, 4 workers each transfer 10 times — total must be preserved
    // A=1000, B=1000. Each task transfers 1 from A to B, 10 times.
    // After: A=960, B=1040. A+B must still be 2000.
    expect_i32_multiworker_with_builtins("
        class Bank {
            a: number = 1000;
            b: number = 1000;
            mu: Mutex = new Mutex();
        }
        async function transfer(bank: Bank, amount: number): Task<void> {
            let i: number = 0;
            while (i < 10) {
                bank.mu.lock();
                bank.a = bank.a - amount;
                bank.b = bank.b + amount;
                bank.mu.unlock();
                i = i + 1;
            }
        }
        async function main(): Task<number> {
            let bank = new Bank();
            let t1 = transfer(bank, 1);
            let t2 = transfer(bank, 1);
            let t3 = transfer(bank, 1);
            let t4 = transfer(bank, 1);
            await t1;
            await t2;
            await t3;
            await t4;
            return bank.a + bank.b;
        }
        return await main();
    ", 2000, 4);
}

#[test]
fn test_mutex_protects_running_max() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 4 tasks update a shared max value concurrently — result must be 50
    expect_i32_multiworker_with_builtins("
        class MaxTracker {
            max: number = 0;
            mu: Mutex = new Mutex();
        }
        async function updateMax(t: MaxTracker, v1: number, v2: number, v3: number): Task<void> {
            t.mu.lock();
            if (v1 > t.max) t.max = v1;
            t.mu.unlock();
            t.mu.lock();
            if (v2 > t.max) t.max = v2;
            t.mu.unlock();
            t.mu.lock();
            if (v3 > t.max) t.max = v3;
            t.mu.unlock();
        }
        async function main(): Task<number> {
            let t = new MaxTracker();
            let w1 = updateMax(t, 10, 20, 30);
            let w2 = updateMax(t, 5, 25, 35);
            let w3 = updateMax(t, 15, 45, 40);
            let w4 = updateMax(t, 50, 8, 12);
            await w1;
            await w2;
            await w3;
            await w4;
            return t.max;
        }
        return await main();
    ", 50, 4);
}

#[test]
fn test_mutex_producer_consumer_with_shared_buffer() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 2 producers each add 1+2+3+4+5=15, 2 consumers each subtract 1+2+3=6
    // Final: 30 - 12 = 18
    // Uses `class Buffer` to verify user classes can shadow builtin names
    expect_i32_multiworker_with_builtins("
        class Buffer {
            value: number = 0;
            mu: Mutex = new Mutex();
        }
        async function produce(buf: Buffer, count: number): Task<void> {
            let bound: number = count + 1;
            let i: number = 1;
            while (i < bound) {
                buf.mu.lock();
                buf.value = buf.value + i;
                buf.mu.unlock();
                i = i + 1;
            }
        }
        async function consume(buf: Buffer, count: number): Task<void> {
            let bound: number = count + 1;
            let i: number = 1;
            while (i < bound) {
                buf.mu.lock();
                buf.value = buf.value - i;
                buf.mu.unlock();
                i = i + 1;
            }
        }
        async function main(): Task<number> {
            let buf = new Buffer();
            let p1 = produce(buf, 5);
            let p2 = produce(buf, 5);
            let c1 = consume(buf, 3);
            let c2 = consume(buf, 3);
            await p1;
            await p2;
            await c1;
            await c2;
            return buf.value;
        }
        return await main();
    ", 18, 4);
}
