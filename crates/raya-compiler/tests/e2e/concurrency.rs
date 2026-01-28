//! Phase 13: Concurrency tests
//!
//! Tests for concurrent execution patterns.
//! In Raya, Tasks are green threads that can run in parallel across CPU cores.
//! Use `await [task1, task2, ...]` to wait for multiple tasks.

use super::harness::*;

// ============================================================================
// Task Cancellation
// ============================================================================

#[test]
#[ignore = "Task.cancel() requires runtime Task object creation with handle"]
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
#[ignore = "Mutex VM implementation not yet complete"]
fn test_concurrent_counter_with_mutex() {
    expect_i32(
        "class Counter {
             value: number = 0;
             mutex: Mutex = new Mutex();

             async increment(): Task<void> {
                 this.mutex.lock();  // Blocking, no await needed
                 this.value = this.value + 1;
                 this.mutex.unlock();
             }
         }

         async function main(): Task<number> {
             let counter = new Counter();
             let tasks: Task<void>[] = [];
             for (let i = 0; i < 10; i = i + 1) {
                 tasks.push(counter.increment());
             }
             await tasks;
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
#[ignore = "Channels not yet implemented"]
fn test_channel_send_receive() {
    expect_i32(
        "async function producer(ch: Channel<number>): Task<void> {
             ch.send(42);
         }

         async function consumer(ch: Channel<number>): Task<number> {
             return await ch.receive();
         }

         async function main(): Task<number> {
             let ch = new Channel<number>();
             let t1 = producer(ch);
             let t2 = consumer(ch);
             await t1;
             return await t2;
         }
         return await main();",
        42,
    );
}
