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
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
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
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
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
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
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
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
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
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
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
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
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
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
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
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
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
