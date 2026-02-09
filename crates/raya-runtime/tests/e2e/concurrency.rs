//! Phase 13: Concurrency tests
//!
//! Tests for concurrent execution patterns.
//! In Raya, Tasks are green threads that can run in parallel across CPU cores.
//! Use `await [task1, task2, ...]` to wait for multiple tasks.

use super::harness::*;
use std::sync::Mutex;

// Mutex to serialize multi-worker tests to avoid resource contention
static MULTIWORKER_LOCK: Mutex<()> = Mutex::new(());

// ============================================================================
// Task Cancellation
// ============================================================================

#[test]
fn test_task_cancel() {
    expect_i32_with_builtins(
        "async function longRunning(): Task<number> {
             // Would run for a long time
             return 0;
         }

         async function main(): Task<number> {
             let task = longRunning();
             task.cancel();
             return 42;
         }
         return await main();",
        42,
    );
}

// ============================================================================
// Task with Shared State
// ============================================================================

#[test]
fn test_concurrent_counter_with_mutex() {
    expect_i32_with_builtins(
        "class Counter {
             value: number = 0;
             mutex: Mutex = new Mutex();

             async increment(): Task<void> {
                 this.mutex.lock();
                 this.value = this.value + 1;
                 this.mutex.unlock();
             }
         }

         async function main(): Task<number> {
             let counter = new Counter();
             // Spawn 10 increment tasks
             let t1 = counter.increment();
             let t2 = counter.increment();
             let t3 = counter.increment();
             let t4 = counter.increment();
             let t5 = counter.increment();
             let t6 = counter.increment();
             let t7 = counter.increment();
             let t8 = counter.increment();
             let t9 = counter.increment();
             let t10 = counter.increment();
             // Await all tasks
             await t1;
             await t2;
             await t3;
             await t4;
             await t5;
             await t6;
             await t7;
             await t8;
             await t9;
             await t10;
             return counter.value;
         }
         return await main();",
        10,
    );
}

// ============================================================================
// Mutex Operations (Basic)
// ============================================================================

#[test]
fn test_mutex_basic_lock_unlock() {
    // Test basic mutex lock/unlock without concurrency
    // Mutex only has lock/unlock - no get/set methods
    expect_i32_with_builtins(
        "let mutex = new Mutex();
         mutex.lock();
         let value = 42;
         mutex.unlock();
         return value;",
        42,
    );
}

#[test]
fn test_mutex_try_lock_unlocked() {
    // tryLock on unlocked mutex should succeed
    expect_bool_with_builtins(
        "let mutex = new Mutex();
         return mutex.tryLock();",
        true,
    );
}

#[test]
fn test_mutex_try_lock_after_unlock() {
    // tryLock should succeed after unlock
    expect_bool_with_builtins(
        "let mutex = new Mutex();
         mutex.lock();
         mutex.unlock();
         return mutex.tryLock();",
        true,
    );
}

#[test]
fn test_mutex_is_locked_false() {
    // isLocked on fresh mutex should be false
    expect_bool_with_builtins(
        "let mutex = new Mutex();
         return mutex.isLocked();",
        false,
    );
}

#[test]
fn test_mutex_is_locked_true() {
    // isLocked after lock should be true
    expect_bool_with_builtins(
        "let mutex = new Mutex();
         mutex.lock();
         return mutex.isLocked();",
        true,
    );
}

#[test]
fn test_mutex_is_locked_after_unlock() {
    // isLocked after unlock should be false
    expect_bool_with_builtins(
        "let mutex = new Mutex();
         mutex.lock();
         mutex.unlock();
         return mutex.isLocked();",
        false,
    );
}

// ============================================================================
// Sleep and Timing
// ============================================================================

#[test]
fn test_sleep_basic() {
    expect_i32(
        "async function main(): Task<number> {
             sleep(0); // sleep for 0ms (yield)
             return 42;
         }
         return await main();",
        42,
    );
}

#[test]
fn test_sleep_with_value() {
    // Test sleep with a non-zero duration
    expect_i32(
        "async function compute(): Task<number> {
             sleep(1);
             return 42;
         }
         return await compute();",
        42,
    );
}

// ============================================================================
// Error Handling in Concurrent Tasks
// ============================================================================

#[test]
fn test_await_array_with_failure() {
    // If one task fails, await [...] should propagate the error
    expect_i32(
        "async function succeed(): Task<number> {
             return 42;
         }

         async function fail(): Task<number> {
             throw 'error';
         }

         async function main(): Task<number> {
             try {
                 await [succeed(), fail()];
                 return 0;
             } catch (e) {
                 return 42; // Caught the error
             }
         }
         return await main();",
        42,
    );
}

// ============================================================================
// Channel Communication (if supported)
// ============================================================================

#[test]
fn test_channel_send_receive() {
    // Simple channel test - send and receive in same task
    expect_i32_with_builtins(
        "let ch = new Channel<number>(1);
         ch.send(42);
         return ch.receive();",
        42,
    );
}

#[test]
fn test_channel_buffered() {
    // Buffered channel can hold multiple values
    expect_i32_with_builtins(
        "let ch = new Channel<number>(3);
         ch.send(1);
         ch.send(2);
         ch.send(3);
         let a = ch.receive();
         let b = ch.receive();
         let c = ch.receive();
         return a + b + c;",
        6,
    );
}

#[test]
fn test_channel_try_send_receive() {
    // trySend and tryReceive for non-blocking operations
    expect_bool_with_builtins(
        "let ch = new Channel<number>(1);
         let sent = ch.trySend(42);
         return sent;",
        true,
    );
}

#[test]
fn test_channel_try_receive_empty() {
    // tryReceive on empty channel returns null
    expect_bool_with_builtins(
        "let ch = new Channel<number>(1);
         return ch.tryReceive() == null;",
        true,
    );
}

#[test]
fn test_channel_async_send_then_receive() {
    // Async send in one task, receive after awaiting send
    expect_i32_with_builtins(
        "async function main(): Task<number> {
             let ch = new Channel<number>(1);
             ch.send(42);
             return ch.receive();
         }
         return await main();",
        42,
    );
}

#[test]
fn test_channel_async_producer_consumer() {
    // Async producer/consumer pattern - ignored until generic parameter passing is fixed
    expect_i32_with_builtins(
        "async function producer(ch: Channel<number>): Task<void> {
             ch.send(42);
         }

         async function consumer(ch: Channel<number>): Task<number> {
             return ch.receive();
         }

         async function main(): Task<number> {
             let ch = new Channel<number>(1);
             let t1 = producer(ch);
             let t2 = consumer(ch);
             await t1;
             return await t2;
         }
         return await main();",
        42,
    );
}

// ============================================================================
// Parallel Async Stress Tests - Deadlock & Scheduling Detection
// ============================================================================

#[test]
fn test_many_parallel_tasks() {
    // Stress test: spawn many tasks in parallel
    // Tests scheduler's ability to handle many concurrent tasks
    expect_i32(
        "async function work(x: number): Task<number> {
             return x * 2;
         }

         async function main(): Task<number> {
             let t1 = work(1);
             let t2 = work(2);
             let t3 = work(3);
             let t4 = work(4);
             let t5 = work(5);
             let t6 = work(6);
             let t7 = work(7);
             let t8 = work(8);
             let t9 = work(9);
             let t10 = work(10);

             let results = await [t1, t2, t3, t4, t5, t6, t7, t8, t9, t10];
             let sum = 0;
             let i = 0;
             while (i < 10) {
                 sum = sum + results[i];
                 i = i + 1;
             }
             return sum;
         }
         return await main();",
        110, // 2+4+6+8+10+12+14+16+18+20 = 110
    );
}

#[test]
fn test_chained_task_dependencies() {
    // Test sequential task dependencies: A -> B -> C -> D
    // Each task waits for the previous one - potential deadlock if scheduler is broken
    expect_i32(
        "async function taskA(): Task<number> {
             return 1;
         }

         async function taskB(): Task<number> {
             let a = await taskA();
             return a + 2;
         }

         async function taskC(): Task<number> {
             let b = await taskB();
             return b + 3;
         }

         async function taskD(): Task<number> {
             let c = await taskC();
             return c + 4;
         }

         return await taskD();",
        10, // 1+2+3+4 = 10
    );
}

#[test]
fn test_diamond_task_dependency() {
    // Diamond dependency pattern:
    //       A
    //      / \\
    //     B   C
    //      \\ /
    //       D
    // D waits for both B and C, which both wait for A
    // Tests that shared dependencies don't cause issues
    expect_i32(
        "async function taskA(): Task<number> {
             return 10;
         }

         async function taskB(): Task<number> {
             let a = await taskA();
             return a + 1;
         }

         async function taskC(): Task<number> {
             let a = await taskA();
             return a + 2;
         }

         async function taskD(): Task<number> {
             let b = taskB();
             let c = taskC();
             let results = await [b, c];
             return results[0] + results[1];
         }

         return await taskD();",
        23, // (10+1) + (10+2) = 23
    );
}

#[test]
fn test_nested_parallel_await() {
    // Nested parallel awaits - tests scheduler depth handling
    expect_i32(
        "async function leaf(x: number): Task<number> {
             return x;
         }

         async function branch(x: number): Task<number> {
             let t1 = leaf(x);
             let t2 = leaf(x + 1);
             let results = await [t1, t2];
             return results[0] + results[1];
         }

         async function root(): Task<number> {
             let b1 = branch(1);
             let b2 = branch(10);
             let results = await [b1, b2];
             return results[0] + results[1];
         }

         return await root();",
        24, // (1+2) + (10+11) = 3 + 21 = 24
    );
}

#[test]
fn test_task_spawning_tasks() {
    // Tasks that spawn more tasks - tests dynamic task creation
    expect_i32(
        "async function worker(depth: number): Task<number> {
             if (depth <= 0) {
                 return 1;
             }
             let child = worker(depth - 1);
             let result = await child;
             return result + 1;
         }

         return await worker(5);",
        6, // depth 5 -> 4 -> 3 -> 2 -> 1 -> 0, returns 1+1+1+1+1+1 = 6
    );
}

#[test]
fn test_multiple_waves_of_tasks() {
    // Multiple sequential waves of parallel tasks
    // Tests scheduler handling repeated parallel batches
    expect_i32(
        "async function compute(x: number): Task<number> {
             return x * 2;
         }

         async function main(): Task<number> {
             // Wave 1
             let wave1 = await [compute(1), compute(2), compute(3)];
             let sum1 = wave1[0] + wave1[1] + wave1[2];

             // Wave 2
             let wave2 = await [compute(4), compute(5), compute(6)];
             let sum2 = wave2[0] + wave2[1] + wave2[2];

             // Wave 3
             let wave3 = await [compute(7), compute(8), compute(9)];
             let sum3 = wave3[0] + wave3[1] + wave3[2];

             return sum1 + sum2 + sum3;
         }
         return await main();",
        90, // (2+4+6) + (8+10+12) + (14+16+18) = 12 + 30 + 48 = 90
    );
}

#[test]
fn test_interleaved_await_and_spawn() {
    // Interleave spawning and awaiting - tests scheduler fairness
    expect_i32(
        "async function work(x: number): Task<number> {
             return x;
         }

         async function main(): Task<number> {
             let t1 = work(1);
             let r1 = await t1;

             let t2 = work(2);
             let t3 = work(3);
             let r23 = await [t2, t3];

             let t4 = work(4);
             let r4 = await t4;

             return r1 + r23[0] + r23[1] + r4;
         }
         return await main();",
        10, // 1+2+3+4 = 10
    );
}

#[test]
fn test_deep_async_call_stack() {
    // Deep async call stack - tests stack handling under async
    expect_i32(
        "async function level5(): Task<number> { return 5; }
         async function level4(): Task<number> { return await level5() + 4; }
         async function level3(): Task<number> { return await level4() + 3; }
         async function level2(): Task<number> { return await level3() + 2; }
         async function level1(): Task<number> { return await level2() + 1; }
         async function level0(): Task<number> { return await level1() + 0; }

         return await level0();",
        15, // 5+4+3+2+1+0 = 15
    );
}

#[test]
fn test_parallel_with_different_completion_times() {
    // Tasks with varying "work" - tests scheduler doesn't assume equal completion
    expect_i32(
        "async function quick(): Task<number> {
             return 1;
         }

         async function medium(): Task<number> {
             let x = 0;
             let i = 0;
             while (i < 10) {
                 x = x + 1;
                 i = i + 1;
             }
             return x;
         }

         async function slow(): Task<number> {
             let x = 0;
             let i = 0;
             while (i < 100) {
                 x = x + 1;
                 i = i + 1;
             }
             return x;
         }

         async function main(): Task<number> {
             let results = await [quick(), medium(), slow()];
             return results[0] + results[1] + results[2];
         }
         return await main();",
        111, // 1 + 10 + 100 = 111
    );
}

#[test]
fn test_rapid_spawn_and_complete() {
    // Rapidly spawn and await many small tasks
    // Tests scheduler overhead and task recycling
    expect_i32(
        "async function tiny(x: number): Task<number> {
             return x;
         }

         async function main(): Task<number> {
             let sum = 0;
             let i = 0;
             while (i < 20) {
                 let t = tiny(i);
                 sum = sum + await t;
                 i = i + 1;
             }
             return sum;
         }
         return await main();",
        190, // 0+1+2+...+19 = 190
    );
}

// ============================================================================
// Multi-Worker Parallel Tests - True Concurrent Execution
// These tests use multiple worker threads to stress-test actual parallelism
// ============================================================================

#[test]
fn test_multiworker_many_parallel_tasks() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
    // Same as test_many_parallel_tasks but with 4 workers
    // Tests true parallel execution with work stealing
    expect_i32_multiworker(
        "async function work(x: number): Task<number> {
             return x * 2;
         }

         async function main(): Task<number> {
             let t1 = work(1);
             let t2 = work(2);
             let t3 = work(3);
             let t4 = work(4);
             let t5 = work(5);
             let t6 = work(6);
             let t7 = work(7);
             let t8 = work(8);
             let t9 = work(9);
             let t10 = work(10);

             let results = await [t1, t2, t3, t4, t5, t6, t7, t8, t9, t10];
             let sum = 0;
             let i = 0;
             while (i < 10) {
                 sum = sum + results[i];
                 i = i + 1;
             }
             return sum;
         }
         return await main();",
        110,
        4, // 4 worker threads
    );
}

#[test]
fn test_multiworker_diamond_dependency() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
    // Diamond pattern with 4 workers - tests shared dependency handling
    // under true parallel execution
    expect_i32_multiworker(
        "async function taskA(): Task<number> {
             return 10;
         }

         async function taskB(): Task<number> {
             let a = await taskA();
             return a + 1;
         }

         async function taskC(): Task<number> {
             let a = await taskA();
             return a + 2;
         }

         async function taskD(): Task<number> {
             let b = taskB();
             let c = taskC();
             let results = await [b, c];
             return results[0] + results[1];
         }

         return await taskD();",
        23,
        4,
    );
}

#[test]
fn test_multiworker_nested_parallel() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
    // Nested parallel awaits with 4 workers
    // Tests scheduler depth handling under parallel execution
    expect_i32_multiworker(
        "async function leaf(x: number): Task<number> {
             return x;
         }

         async function branch(x: number): Task<number> {
             let t1 = leaf(x);
             let t2 = leaf(x + 1);
             let results = await [t1, t2];
             return results[0] + results[1];
         }

         async function root(): Task<number> {
             let b1 = branch(1);
             let b2 = branch(10);
             let results = await [b1, b2];
             return results[0] + results[1];
         }

         return await root();",
        24,
        4,
    );
}

#[test]
fn test_multiworker_task_chain() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
    // Chain of tasks with 4 workers
    // Tests that sequential dependencies work under parallel scheduler
    expect_i32_multiworker(
        "async function taskA(): Task<number> { return 1; }
         async function taskB(): Task<number> { return await taskA() + 2; }
         async function taskC(): Task<number> { return await taskB() + 3; }
         async function taskD(): Task<number> { return await taskC() + 4; }

         return await taskD();",
        10,
        4,
    );
}

#[test]
fn test_multiworker_waves() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
    // Multiple waves with 4 workers
    // Tests repeated parallel batches under true concurrency
    expect_i32_multiworker(
        "async function compute(x: number): Task<number> {
             return x * 2;
         }

         async function main(): Task<number> {
             let wave1 = await [compute(1), compute(2), compute(3)];
             let sum1 = wave1[0] + wave1[1] + wave1[2];

             let wave2 = await [compute(4), compute(5), compute(6)];
             let sum2 = wave2[0] + wave2[1] + wave2[2];

             let wave3 = await [compute(7), compute(8), compute(9)];
             let sum3 = wave3[0] + wave3[1] + wave3[2];

             return sum1 + sum2 + sum3;
         }
         return await main();",
        90,
        4,
    );
}

#[test]
fn test_multiworker_work_stealing_stress() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
    // Stress test for work stealing - many tasks with varying work
    // With 4 workers, work should be distributed
    expect_i32_multiworker(
        "async function heavy(x: number): Task<number> {
             let sum = 0;
             let i = 0;
             while (i < x) {
                 sum = sum + 1;
                 i = i + 1;
             }
             return sum;
         }

         async function main(): Task<number> {
             // Create tasks with different amounts of work
             let t1 = heavy(10);
             let t2 = heavy(20);
             let t3 = heavy(30);
             let t4 = heavy(40);
             let t5 = heavy(50);
             let t6 = heavy(60);
             let t7 = heavy(70);
             let t8 = heavy(80);

             let results = await [t1, t2, t3, t4, t5, t6, t7, t8];
             let sum = 0;
             let i = 0;
             while (i < 8) {
                 sum = sum + results[i];
                 i = i + 1;
             }
             return sum;
         }
         return await main();",
        360, // 10+20+30+40+50+60+70+80 = 360
        4,
    );
}

#[test]
fn test_multiworker_rapid_spawn() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
    // Rapid spawn/complete with 4 workers
    // Tests scheduler overhead under parallel execution
    expect_i32_multiworker(
        "async function tiny(x: number): Task<number> {
             return x;
         }

         async function main(): Task<number> {
             let sum = 0;
             let i = 0;
             while (i < 20) {
                 let t = tiny(i);
                 sum = sum + await t;
                 i = i + 1;
             }
             return sum;
         }
         return await main();",
        190,
        4,
    );
}

#[test]
fn test_multiworker_recursive_spawn() {
    let _guard = MULTIWORKER_LOCK.lock().unwrap();
    // Recursive task spawning with 4 workers
    // Tests dynamic task creation under parallel execution
    expect_i32_multiworker(
        "async function worker(depth: number): Task<number> {
             if (depth <= 0) {
                 return 1;
             }
             let child = worker(depth - 1);
             let result = await child;
             return result + 1;
         }

         return await worker(10);",
        11, // depth 10 -> 0, returns 11
        4,
    );
}
