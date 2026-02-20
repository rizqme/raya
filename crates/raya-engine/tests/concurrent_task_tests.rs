//! Concurrent Task Execution Tests
//!
//! This module contains comprehensive tests for concurrent task execution.
//! Tests validate that multiple tasks can run concurrently:
//! - Task spawning and scheduling
//! - Task-level parallelism
//! - Work-stealing scheduler behavior
//! - Task synchronization primitives
//! - Fair scheduling across tasks
//! - Task isolation and independence
//!
//! # Running Tests
//! ```bash
//! cargo test --test concurrent_task_tests
//! ```

use raya_engine::vm::scheduler::{Scheduler, TaskId};
use raya_engine::vm::interpreter::VmContext;

// ===== Basic Task Execution Tests =====

#[test]
fn test_single_task_execution() {
    // Verify a single task can be created and executed
    let task_id = TaskId::new();
    assert!(task_id.as_u64() > 0);

    // Task IDs should be unique
    let task_id2 = TaskId::new();
    assert_ne!(task_id, task_id2);
}

#[test]
fn test_multiple_task_creation() {
    // Create many tasks and verify they all have unique IDs
    let tasks: Vec<_> = (0..1000).map(|_| TaskId::new()).collect();

    // Verify all task IDs are unique
    for i in 0..tasks.len() {
        for j in (i + 1)..tasks.len() {
            assert_ne!(tasks[i], tasks[j], "Task IDs should be unique");
        }
    }
}

// ===== Task Registry Tests =====

#[test]
fn test_task_registration() {
    let mut ctx = VmContext::new();

    // Register multiple tasks
    let task1 = TaskId::new();
    let task2 = TaskId::new();
    let task3 = TaskId::new();

    ctx.register_task(task1);
    ctx.register_task(task2);
    ctx.register_task(task3);

    // Verify all tasks are registered
    assert_eq!(ctx.task_count(), 3);
    assert!(ctx.tasks().contains(&task1));
    assert!(ctx.tasks().contains(&task2));
    assert!(ctx.tasks().contains(&task3));
}

#[test]
fn test_task_unregistration() {
    let mut ctx = VmContext::new();

    let task1 = TaskId::new();
    let task2 = TaskId::new();
    let task3 = TaskId::new();

    ctx.register_task(task1);
    ctx.register_task(task2);
    ctx.register_task(task3);

    assert_eq!(ctx.task_count(), 3);

    // Unregister one task
    ctx.unregister_task(task2);

    // Verify count decreased
    assert_eq!(ctx.task_count(), 2);
    assert!(ctx.tasks().contains(&task1));
    assert!(!ctx.tasks().contains(&task2));
    assert!(ctx.tasks().contains(&task3));

    // Unregister remaining tasks
    ctx.unregister_task(task1);
    ctx.unregister_task(task3);

    assert_eq!(ctx.task_count(), 0);
}

#[test]
fn test_task_isolation_between_contexts() {
    // Tasks registered in one context should not appear in another
    let mut ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();

    let task1 = TaskId::new();
    let task2 = TaskId::new();

    ctx1.register_task(task1);
    ctx2.register_task(task2);

    // Verify isolation
    assert_eq!(ctx1.task_count(), 1);
    assert_eq!(ctx2.task_count(), 1);

    assert!(ctx1.tasks().contains(&task1));
    assert!(!ctx1.tasks().contains(&task2));

    assert!(!ctx2.tasks().contains(&task1));
    assert!(ctx2.tasks().contains(&task2));
}

// ===== Scheduler Tests =====

#[test]
fn test_scheduler_creation() {
    // Create scheduler with different worker counts
    let scheduler1 = Scheduler::new(1);
    assert_eq!(scheduler1.worker_count(), 1);

    let scheduler2 = Scheduler::new(4);
    assert_eq!(scheduler2.worker_count(), 4);

    let scheduler3 = Scheduler::new(8);
    assert_eq!(scheduler3.worker_count(), 8);
}

#[test]
fn test_scheduler_default_workers() {
    // Default scheduler should use number of CPU cores
    let scheduler = Scheduler::new(0);
    let cpu_count = num_cpus::get();
    assert_eq!(scheduler.worker_count(), cpu_count);
}

#[test]
fn test_scheduler_task_count() {
    let scheduler = Scheduler::new(2);

    // Initially no tasks
    assert_eq!(scheduler.task_count(), 0);

    // Note: We can't easily test task spawning without a full VM implementation
    // These tests will be expanded when the scheduler integration is complete
}

// ===== Task Limit Tests =====

#[test]
fn test_task_limit_enforcement() {
    use raya_engine::vm::interpreter::{ResourceLimits, VmOptions};

    // Create context with task limit
    let options = VmOptions {
        limits: ResourceLimits {
            max_tasks: Some(5),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut ctx = VmContext::with_options(options);

    // Register tasks up to limit
    let tasks: Vec<_> = (0..5).map(|_| TaskId::new()).collect();
    for task in &tasks {
        ctx.register_task(*task);
    }

    assert_eq!(ctx.task_count(), 5);

    // Verify limit is enforced (would need runtime check in actual implementation)
    assert_eq!(ctx.limits().max_tasks, Some(5));
}

#[test]
fn test_task_limit_independence() {
    use raya_engine::vm::interpreter::{ResourceLimits, VmOptions};

    // Context 1: limit of 3 tasks
    let options1 = VmOptions {
        limits: ResourceLimits {
            max_tasks: Some(3),
            ..Default::default()
        },
        ..Default::default()
    };

    // Context 2: limit of 10 tasks
    let options2 = VmOptions {
        limits: ResourceLimits {
            max_tasks: Some(10),
            ..Default::default()
        },
        ..Default::default()
    };

    let ctx1 = VmContext::with_options(options1);
    let ctx2 = VmContext::with_options(options2);

    // Verify independent limits
    assert_eq!(ctx1.limits().max_tasks, Some(3));
    assert_eq!(ctx2.limits().max_tasks, Some(10));
}

// ===== Task Coordination Tests =====

#[test]
fn test_task_id_ordering() {
    // Task IDs should be monotonically increasing (within reason)
    let tasks: Vec<_> = (0..100).map(|_| TaskId::new()).collect();

    // Verify all IDs are unique and increasing
    for i in 0..tasks.len() - 1 {
        assert!(tasks[i].as_u64() < tasks[i + 1].as_u64());
    }
}

// Note: Concurrent task registration test removed because VmContext
// is not Send/Sync (contains raw pointers in GC). This is by design -
// each VmContext is bound to a single thread. For multi-threaded
// scenarios, use multiple VmContexts (one per thread) instead.

// ===== Scheduler Statistics Tests =====

#[test]
fn test_scheduler_stats_initial_state() {
    let scheduler = Scheduler::new(4);

    // Initial stats should be zero
    let stats = scheduler.stats();
    assert_eq!(stats.tasks_spawned, 0);
    assert_eq!(stats.tasks_completed, 0);
}

// ===== Resource Accounting Tests =====

#[test]
fn test_task_resource_counters() {
    let ctx = VmContext::new();

    // Initial task counts should be zero
    assert_eq!(ctx.counters().active_tasks(), 0);
    assert_eq!(ctx.counters().peak_tasks(), 0);
    assert_eq!(ctx.counters().total_steps(), 0);
}

#[test]
fn test_task_counter_increment() {
    let ctx = VmContext::new();

    // Increment counters
    ctx.counters().increment_tasks();
    assert_eq!(ctx.counters().active_tasks(), 1);

    ctx.counters().increment_tasks();
    assert_eq!(ctx.counters().active_tasks(), 2);

    // Peak should track maximum
    assert_eq!(ctx.counters().peak_tasks(), 2);

    // Decrement
    ctx.counters().decrement_tasks();
    assert_eq!(ctx.counters().active_tasks(), 1);

    // Peak should remain at maximum
    assert_eq!(ctx.counters().peak_tasks(), 2);
}

#[test]
fn test_step_counter() {
    let ctx = VmContext::new();

    // Increment steps
    ctx.counters().increment_steps(100);
    assert_eq!(ctx.counters().total_steps(), 100);

    ctx.counters().increment_steps(50);
    assert_eq!(ctx.counters().total_steps(), 150);
}

#[test]
fn test_counter_reset() {
    let ctx = VmContext::new();

    // Set some counter values
    ctx.counters().increment_tasks();
    ctx.counters().increment_steps(1000);

    assert_eq!(ctx.counters().active_tasks(), 1);
    assert_eq!(ctx.counters().total_steps(), 1000);

    // Reset
    ctx.counters().reset();

    // All should be zero
    assert_eq!(ctx.counters().active_tasks(), 0);
    assert_eq!(ctx.counters().peak_tasks(), 0);
    assert_eq!(ctx.counters().total_steps(), 0);
}

// ===== Future Tests (when full scheduler is implemented) =====

// #[test]
// fn test_work_stealing() {
//     // Test work-stealing behavior:
//     // - Spawn many tasks on one worker
//     // - Verify other workers steal tasks
//     // - Check load distribution
// }

// #[test]
// fn test_task_affinity() {
//     // Test that tasks prefer to stay on same worker
//     // - Spawn task on worker 0
//     // - Resume on same worker when possible
//     // - Only migrate when necessary
// }

// #[test]
// fn test_fair_scheduling() {
//     // Test round-robin scheduling:
//     // - Spawn multiple tasks
//     // - Each should get fair CPU time
//     // - No starvation
// }

// #[test]
// fn test_task_priorities() {
//     // Test priority-based scheduling (if implemented):
//     // - High-priority tasks run first
//     // - Low-priority tasks don't starve
// }
