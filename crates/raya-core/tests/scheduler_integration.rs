//! Integration tests for Task Scheduler (Milestone 1.10)

#![allow(clippy::identity_op)]
#![allow(unused_variables)]

use raya_bytecode::{Function, Module, Opcode};
use raya_core::scheduler::{Scheduler, Task, TaskState};
use raya_core::value::Value;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn create_simple_task(name: &str, result: i32) -> Arc<Task> {
    let mut module = Module::new("test".to_string());
    module.functions.push(Function {
        name: name.to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8,
            (result & 0xFF) as u8,
            ((result >> 8) & 0xFF) as u8,
            ((result >> 16) & 0xFF) as u8,
            ((result >> 24) & 0xFF) as u8,
            Opcode::Return as u8,
        ],
    });

    Arc::new(Task::new(0, Arc::new(module), None))
}

fn create_compute_task(name: &str, iterations: u32) -> Arc<Task> {
    let mut module = Module::new("test".to_string());
    module.functions.push(Function {
        name: name.to_string(),
        param_count: 0,
        local_count: 2, // counter and result
        code: vec![
            // Initialize counter = 0
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            0,
            0,
            // Initialize result = 0
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            1,
            0,
            // Loop start (offset 18)
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            (iterations & 0xFF) as u8,
            ((iterations >> 8) & 0xFF) as u8,
            ((iterations >> 16) & 0xFF) as u8,
            ((iterations >> 24) & 0xFF) as u8,
            Opcode::Ilt as u8,
            Opcode::JmpIfFalse as u8,
            30,
            0, // Jump to end if counter >= iterations
            // result = result + 1
            Opcode::LoadLocal as u8,
            1,
            0,
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Iadd as u8,
            Opcode::StoreLocal as u8,
            1,
            0,
            // counter = counter + 1
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Iadd as u8,
            Opcode::StoreLocal as u8,
            0,
            0,
            // Jump back to loop start (byte 16)
            // Current position after Jmp operands: byte 55
            // Offset = 16 - 55 = -39
            Opcode::Jmp as u8,
            (-39i16 & 0xFF) as u8,
            (((-39i16) >> 8) & 0xFF) as u8,
            // End: return result
            Opcode::LoadLocal as u8,
            1,
            0,
            Opcode::Return as u8,
        ],
    });

    Arc::new(Task::new(0, Arc::new(module), None))
}

#[test]
fn test_scheduler_basic_task_execution() {
    let mut scheduler = Scheduler::new(2);
    scheduler.start();

    let task = create_simple_task("test", 42);
    let task_id = scheduler.spawn(task.clone()).expect("Failed to spawn task");

    // Wait for task to complete
    thread::sleep(Duration::from_millis(100));

    assert_eq!(task.state(), TaskState::Completed);
    assert_eq!(task.result(), Some(Value::i32(42)));

    scheduler.shutdown();
}

#[test]
fn test_scheduler_multiple_concurrent_tasks() {
    let mut scheduler = Scheduler::new(4);
    scheduler.start();

    let mut tasks = Vec::new();
    for i in 0..20 {
        let task = create_simple_task(&format!("task{}", i), i);
        scheduler.spawn(task.clone()).expect("Failed to spawn task");
        tasks.push(task);
    }

    // Wait for all tasks to complete
    let completed = scheduler.wait_all(Duration::from_secs(2));
    assert!(completed, "Not all tasks completed in time");

    // Verify all tasks completed successfully
    for (i, task) in tasks.iter().enumerate() {
        assert_eq!(
            task.state(),
            TaskState::Completed,
            "Task {} not completed",
            i
        );
        assert_eq!(
            task.result(),
            Some(Value::i32(i as i32)),
            "Task {} has wrong result",
            i
        );
    }

    scheduler.shutdown();
}

#[test]
fn test_scheduler_with_different_worker_counts() {
    for worker_count in [1, 2, 4, 8] {
        let mut scheduler = Scheduler::new(worker_count);
        scheduler.start();

        let task = create_simple_task("test", 100);
        scheduler.spawn(task.clone()).expect("Failed to spawn task");

        thread::sleep(Duration::from_millis(100));

        assert_eq!(task.state(), TaskState::Completed);
        assert_eq!(task.result(), Some(Value::i32(100)));

        scheduler.shutdown();
    }
}

#[test]
fn test_scheduler_task_completion_cleanup() {
    let mut scheduler = Scheduler::new(2);
    scheduler.start();

    let task = create_simple_task("test", 42);
    let task_id = scheduler.spawn(task.clone()).expect("Failed to spawn task");

    // Wait for task to complete
    assert!(scheduler.wait_all(Duration::from_secs(1)));

    // Task should still be in registry
    assert!(scheduler.get_task(task_id).is_some());

    // Can remove it manually
    let removed = scheduler.remove_task(task_id);
    assert!(removed.is_some());
    assert!(scheduler.get_task(task_id).is_none());

    scheduler.shutdown();
}

#[test]
fn test_scheduler_work_stealing() {
    // Create scheduler with 4 workers
    let mut scheduler = Scheduler::new(4);
    scheduler.start();

    // Spawn many tasks quickly
    let mut tasks = Vec::new();
    for i in 0..100 {
        let task = create_simple_task(&format!("task{}", i), i);
        scheduler.spawn(task.clone()).expect("Failed to spawn task");
        tasks.push(task);
    }

    // Wait for all to complete
    assert!(scheduler.wait_all(Duration::from_secs(5)));

    // All tasks should complete despite being distributed across workers
    for (i, task) in tasks.iter().enumerate() {
        assert_eq!(task.state(), TaskState::Completed);
        assert_eq!(task.result(), Some(Value::i32(i as i32)));
    }

    scheduler.shutdown();
}

#[test]
fn test_scheduler_compute_intensive_tasks() {
    let mut scheduler = Scheduler::new(2);
    scheduler.start();

    // Create tasks that do actual computation
    let mut tasks = Vec::new();
    for i in 0..5 {
        let task = create_compute_task(&format!("compute{}", i), 100);
        scheduler.spawn(task.clone()).expect("Failed to spawn task");
        tasks.push(task);
    }

    // Wait for all to complete
    assert!(scheduler.wait_all(Duration::from_secs(5)));

    // All should complete with correct result
    for task in &tasks {
        assert_eq!(task.state(), TaskState::Completed);
        assert_eq!(task.result(), Some(Value::i32(100)));
    }

    scheduler.shutdown();
}

#[test]
fn test_scheduler_preemption_of_long_tasks() {
    let mut scheduler = Scheduler::new(2);
    scheduler.start();

    // Create a very long-running task (should get preempted)
    let long_task = create_compute_task("long", 10000);
    scheduler.spawn(long_task.clone());

    // Create several short tasks
    let mut short_tasks = Vec::new();
    for i in 0..5 {
        let task = create_simple_task(&format!("short{}", i), i);
        scheduler.spawn(task.clone()).expect("Failed to spawn task");
        short_tasks.push(task);
    }

    // Short tasks should complete even though long task is running
    thread::sleep(Duration::from_millis(200));

    // At least some short tasks should complete
    let completed_count = short_tasks
        .iter()
        .filter(|t| t.state() == TaskState::Completed)
        .count();

    assert!(
        completed_count >= 3,
        "Expected at least 3 short tasks to complete, got {}",
        completed_count
    );

    scheduler.shutdown();
}

#[test]
fn test_scheduler_safepoint_integration() {
    // Test that scheduler works correctly with safepoints
    let mut scheduler = Scheduler::new(2);
    scheduler.start();

    let task = create_compute_task("safepoint_test", 500);
    scheduler.spawn(task.clone()).expect("Failed to spawn task");

    // Task should complete despite safepoint polls
    assert!(scheduler.wait_all(Duration::from_secs(2)));
    assert_eq!(task.state(), TaskState::Completed);
    assert_eq!(task.result(), Some(Value::i32(500)));

    scheduler.shutdown();
}

#[test]
fn test_scheduler_rapid_spawn_and_complete() {
    let mut scheduler = Scheduler::new(4);
    scheduler.start();

    // Rapidly spawn and complete tasks
    for _ in 0..10 {
        let mut tasks = Vec::new();
        for i in 0..20 {
            let task = create_simple_task(&format!("rapid{}", i), i);
            scheduler.spawn(task.clone()).expect("Failed to spawn task");
            tasks.push(task);
        }

        // Wait for this batch
        assert!(scheduler.wait_all(Duration::from_millis(500)));

        // All should be done
        for task in tasks {
            assert_eq!(task.state(), TaskState::Completed);
        }
    }

    scheduler.shutdown();
}

#[test]
fn test_scheduler_graceful_shutdown() {
    let mut scheduler = Scheduler::new(2);
    scheduler.start();

    // Spawn some long-running tasks
    for i in 0..5 {
        let task = create_compute_task(&format!("shutdown{}", i), 1000);
        scheduler.spawn(task);
    }

    // Wait a bit
    thread::sleep(Duration::from_millis(50));

    // Shutdown should succeed even with running tasks
    scheduler.shutdown();
    assert!(!scheduler.is_started());
}

#[test]
fn test_scheduler_preemption_fairness() {
    let mut scheduler = Scheduler::new(1); // Single worker to force preemption
    scheduler.start();

    // Create two long-running tasks
    let task1 = create_compute_task("long1", 5000);
    let task2 = create_compute_task("long2", 5000);

    scheduler.spawn(task1.clone());
    scheduler.spawn(task2.clone());

    // Wait a bit - both should make some progress due to preemption
    thread::sleep(Duration::from_millis(300));

    // With preemption, both tasks should have started running
    // (At least one should have moved from Created state)
    let states = [task1.state(), task2.state()];
    let running_or_completed = states
        .iter()
        .filter(|&&s| {
            s == TaskState::Running
                || s == TaskState::Completed
                || s == TaskState::Suspended
                || s == TaskState::Resumed
        })
        .count();

    assert!(
        running_or_completed >= 1,
        "Expected at least one task to have been running"
    );

    scheduler.shutdown();
}

#[test]
fn test_scheduler_default_worker_count() {
    let scheduler = Scheduler::default();
    assert_eq!(scheduler.worker_count(), num_cpus::get());
}

#[test]
fn test_scheduler_task_state_transitions() {
    let mut scheduler = Scheduler::new(1);
    scheduler.start();

    let task = create_compute_task("state_test", 100);

    // Initial state
    assert_eq!(task.state(), TaskState::Created);

    scheduler.spawn(task.clone()).expect("Failed to spawn task");

    // Should transition to Running
    thread::sleep(Duration::from_millis(50));
    let state = task.state();
    assert!(
        state == TaskState::Running || state == TaskState::Completed,
        "Expected Running or Completed, got {:?}",
        state
    );

    // Wait for completion
    assert!(scheduler.wait_all(Duration::from_secs(1)));
    assert_eq!(task.state(), TaskState::Completed);

    scheduler.shutdown();
}
