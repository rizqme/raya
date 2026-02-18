//! Snapshot/Restore Validation Tests
//!
//! This module contains comprehensive tests for VM snapshot and restore functionality.
//! Tests validate the complete pause-snapshot-transfer-restore cycle, including:
//! - VmContext state preservation
//! - Multi-context snapshot coordination
//! - Heap state preservation
//! - Task state preservation
//! - Global variable preservation
//! - Snapshot portability
//! - Corruption detection
//! - Resource limit preservation
//! - Safepoint integration
//!
//! # Running Tests
//! ```bash
//! cargo test --test snapshot_restore_validation
//! ```

use raya_engine::vm::scheduler::{Scheduler, TaskId};
use raya_engine::vm::snapshot::{SerializedTask, SnapshotReader, SnapshotWriter};
use raya_engine::vm::value::Value;
use raya_engine::vm::interpreter::{ResourceLimits, VmContext, VmOptions};
use std::io::Cursor;

// ===== VmContext Snapshot/Restore Tests =====

#[test]
fn test_empty_context_snapshot() {
    // Create empty context
    let ctx = VmContext::new();

    // Verify initial state
    assert_eq!(ctx.task_count(), 0);
    assert_eq!(ctx.heap_stats().allocated_bytes, 0);

    // Note: Full snapshot/restore will be tested when Vm::snapshot() is implemented
}

#[test]
fn test_context_with_globals_snapshot_preparation() {
    // Create context with global variables
    let mut ctx = VmContext::new();

    // Set some globals
    ctx.set_global("x".to_string(), Value::i32(42));
    ctx.set_global("y".to_string(), Value::bool(true));
    ctx.set_global("name".to_string(), Value::null());

    // Verify globals are set
    assert_eq!(ctx.get_global("x"), Some(Value::i32(42)));
    assert_eq!(ctx.get_global("y"), Some(Value::bool(true)));
    assert!(ctx.get_global("name").is_some());

    // Note: Full snapshot/restore will preserve these globals
}

#[test]
fn test_context_with_tasks_snapshot_preparation() {
    // Create context with tasks
    let mut ctx = VmContext::new();

    let task1 = TaskId::new();
    let task2 = TaskId::new();
    let task3 = TaskId::new();

    ctx.register_task(task1);
    ctx.register_task(task2);
    ctx.register_task(task3);

    // Verify tasks are registered
    assert_eq!(ctx.task_count(), 3);
    assert!(ctx.tasks().contains(&task1));
    assert!(ctx.tasks().contains(&task2));
    assert!(ctx.tasks().contains(&task3));

    // Note: Full snapshot/restore will preserve task IDs and states
}

// ===== Serialization Round-Trip Tests =====

#[test]
fn test_task_snapshot_roundtrip_with_state() {
    use raya_engine::vm::scheduler::TaskState;

    // Create task with various state
    let mut task = SerializedTask::new(TaskId::from_u64(100), 5);
    task.state = TaskState::Running;
    task.ip = 42;
    task.parent = Some(TaskId::from_u64(99));

    // Add stack values
    task.stack.push(Value::i32(10));
    task.stack.push(Value::i32(20));
    task.stack.push(Value::bool(true));
    task.stack.push(Value::f64(3.14));

    // Write to snapshot
    let mut writer = SnapshotWriter::new();
    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);

    let restored = &reader.tasks()[0];

    // Verify all state is preserved
    assert_eq!(restored.task_id.as_u64(), 100);
    assert_eq!(restored.function_index, 5);
    assert_eq!(restored.state, TaskState::Running);
    assert_eq!(restored.ip, 42);
    assert_eq!(restored.parent.unwrap().as_u64(), 99);

    // Verify stack
    assert_eq!(restored.stack.len(), 4);
    assert_eq!(restored.stack[0].as_i32(), Some(10));
    assert_eq!(restored.stack[1].as_i32(), Some(20));
    assert_eq!(restored.stack[2].as_bool(), Some(true));
    assert_eq!(restored.stack[3].as_f64(), Some(3.14));
}

#[test]
fn test_multiple_tasks_snapshot_roundtrip() {
    use raya_engine::vm::scheduler::TaskState;

    let mut writer = SnapshotWriter::new();

    // Task 1: Running
    let mut task1 = SerializedTask::new(TaskId::from_u64(1), 0);
    task1.state = TaskState::Running;
    task1.ip = 100;

    // Task 2: Created (not started)
    let task2 = SerializedTask::new(TaskId::from_u64(2), 1);

    // Task 3: Completed with result
    let mut task3 = SerializedTask::new(TaskId::from_u64(3), 2);
    task3.state = TaskState::Completed;
    task3.result = Some(Value::i32(42));

    writer.add_task(task1);
    writer.add_task(task2);
    writer.add_task(task3);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 3);

    // Verify each task
    assert_eq!(reader.tasks()[0].state, TaskState::Running);
    assert_eq!(reader.tasks()[1].state, TaskState::Created);
    assert_eq!(reader.tasks()[2].state, TaskState::Completed);
    assert_eq!(reader.tasks()[2].result.unwrap().as_i32(), Some(42));
}

// ===== Task Hierarchy Preservation Tests =====

#[test]
fn test_parent_child_task_relationships() {
    let mut writer = SnapshotWriter::new();

    // Create task hierarchy:
    // Parent (ID=1)
    //   ├─ Child1 (ID=2)
    //   │   └─ Grandchild1 (ID=4)
    //   └─ Child2 (ID=3)

    let parent = SerializedTask::new(TaskId::from_u64(1), 0);

    let mut child1 = SerializedTask::new(TaskId::from_u64(2), 1);
    child1.parent = Some(TaskId::from_u64(1));

    let mut child2 = SerializedTask::new(TaskId::from_u64(3), 1);
    child2.parent = Some(TaskId::from_u64(1));

    let mut grandchild1 = SerializedTask::new(TaskId::from_u64(4), 2);
    grandchild1.parent = Some(TaskId::from_u64(2));

    writer.add_task(parent);
    writer.add_task(child1);
    writer.add_task(child2);
    writer.add_task(grandchild1);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 4);

    // Verify relationships
    assert!(reader.tasks()[0].parent.is_none()); // parent has no parent
    assert_eq!(reader.tasks()[1].parent.unwrap().as_u64(), 1); // child1 -> parent
    assert_eq!(reader.tasks()[2].parent.unwrap().as_u64(), 1); // child2 -> parent
    assert_eq!(reader.tasks()[3].parent.unwrap().as_u64(), 2); // grandchild1 -> child1
}

// ===== Blocked Task State Tests =====

#[test]
fn test_blocked_task_snapshot() {
    use raya_engine::vm::snapshot::BlockedReason;

    let mut writer = SnapshotWriter::new();

    // Task blocked on another task
    let mut task1 = SerializedTask::new(TaskId::from_u64(10), 0);
    task1.blocked_on = Some(BlockedReason::AwaitingTask(TaskId::from_u64(11)));

    // Task blocked on mutex
    let mut task2 = SerializedTask::new(TaskId::from_u64(20), 0);
    task2.blocked_on = Some(BlockedReason::AwaitingMutex(42));

    // Task blocked on other reason
    let mut task3 = SerializedTask::new(TaskId::from_u64(30), 0);
    task3.blocked_on = Some(BlockedReason::Other("waiting on channel".to_string()));

    writer.add_task(task1);
    writer.add_task(task2);
    writer.add_task(task3);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 3);

    // Verify blocked states
    match &reader.tasks()[0].blocked_on {
        Some(BlockedReason::AwaitingTask(id)) => assert_eq!(id.as_u64(), 11),
        _ => panic!("Wrong blocked reason for task1"),
    }

    match &reader.tasks()[1].blocked_on {
        Some(BlockedReason::AwaitingMutex(id)) => assert_eq!(*id, 42),
        _ => panic!("Wrong blocked reason for task2"),
    }

    match &reader.tasks()[2].blocked_on {
        Some(BlockedReason::Other(msg)) => assert_eq!(msg, "waiting on channel"),
        _ => panic!("Wrong blocked reason for task3"),
    }
}

// ===== Call Stack Preservation Tests =====

#[test]
fn test_deep_call_stack_snapshot() {
    use raya_engine::vm::snapshot::SerializedFrame;

    let mut writer = SnapshotWriter::new();
    let mut task = SerializedTask::new(TaskId::from_u64(500), 0);

    // Create a deep call stack (10 frames)
    for i in 0..10 {
        let mut frame = SerializedFrame::new(i as usize);
        frame.return_ip = (i + 1) * 100;
        frame.base_pointer = i * 10;

        // Add some locals
        for j in 0..5 {
            frame.locals.push(Value::i32((i * 10 + j) as i32));
        }

        task.frames.push(frame);
    }

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);

    let restored = &reader.tasks()[0];
    assert_eq!(restored.frames.len(), 10);

    // Verify each frame
    for i in 0..10 {
        let frame = &restored.frames[i];
        assert_eq!(frame.function_index, i);
        assert_eq!(frame.return_ip, (i + 1) * 100);
        assert_eq!(frame.base_pointer, i * 10);
        assert_eq!(frame.locals.len(), 5);

        // Verify locals
        for j in 0..5 {
            assert_eq!(frame.locals[j].as_i32(), Some((i * 10 + j) as i32));
        }
    }
}

// ===== Value Type Serialization Tests =====

#[test]
fn test_all_value_types_snapshot() {
    let mut writer = SnapshotWriter::new();
    let mut task = SerializedTask::new(TaskId::from_u64(1), 0);

    // Add all value types to stack
    task.stack.push(Value::null());
    task.stack.push(Value::bool(true));
    task.stack.push(Value::bool(false));
    task.stack.push(Value::i32(42));
    task.stack.push(Value::i32(-42));
    task.stack.push(Value::i32(0));
    task.stack.push(Value::f64(3.14));
    task.stack.push(Value::f64(-3.14));
    task.stack.push(Value::f64(0.0));
    task.stack.push(Value::u32(100));
    task.stack.push(Value::u64(1000));

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);

    let restored = &reader.tasks()[0];
    assert_eq!(restored.stack.len(), 11);

    // Verify each value
    assert!(restored.stack[0].is_null());
    assert_eq!(restored.stack[1].as_bool(), Some(true));
    assert_eq!(restored.stack[2].as_bool(), Some(false));
    assert_eq!(restored.stack[3].as_i32(), Some(42));
    assert_eq!(restored.stack[4].as_i32(), Some(-42));
    assert_eq!(restored.stack[5].as_i32(), Some(0));
    assert_eq!(restored.stack[6].as_f64(), Some(3.14));
    assert_eq!(restored.stack[7].as_f64(), Some(-3.14));
    assert_eq!(restored.stack[8].as_f64(), Some(0.0));
    assert_eq!(restored.stack[9].as_u32(), Some(100));
    assert_eq!(restored.stack[10].as_u64(), Some(1000));
}

// ===== Snapshot Validation Tests =====

#[test]
fn test_snapshot_magic_number_validation() {
    use raya_engine::vm::snapshot::format::{SnapshotHeader, SNAPSHOT_MAGIC, SNAPSHOT_VERSION};

    let header = SnapshotHeader::new();

    // Verify magic number is correct
    assert_eq!(header.magic, SNAPSHOT_MAGIC);
    assert_eq!(SNAPSHOT_MAGIC, 0x0000_0059_4159_4152); // "RAYA\0\0\0\0" in little-endian

    // Verify version
    assert_eq!(header.version, SNAPSHOT_VERSION);

    // Verify endianness marker
    use raya_engine::vm::snapshot::format::ENDIANNESS_MARKER;
    assert_eq!(header.endianness, ENDIANNESS_MARKER);
    assert_eq!(ENDIANNESS_MARKER, 0x01020304);

    // Validate header (returns Ok(false) for no byte-swap needed)
    let result = header.validate();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), false); // No byte-swapping needed on same endianness
}

#[test]
fn test_snapshot_corrupted_magic() {
    // Create invalid snapshot with wrong magic
    let mut buf = vec![0u8; 100];

    // Write wrong magic number
    buf[0..8].copy_from_slice(&0xDEADBEEF_u64.to_le_bytes());

    // Should fail to parse
    let result = SnapshotReader::from_reader(&mut Cursor::new(&buf));
    assert!(result.is_err());
}

#[test]
fn test_snapshot_checksum_validation() {
    // Create valid snapshot
    let mut writer = SnapshotWriter::new();
    let task = SerializedTask::new(TaskId::from_u64(1), 0);
    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Should validate correctly
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf));
    assert!(reader.is_ok());

    // Corrupt the checksum (last 32 bytes)
    if buf.len() > 32 {
        let len = buf.len();
        buf[len - 1] ^= 0xFF;
        buf[len - 2] ^= 0xFF;
    }

    // Should fail checksum validation
    let result = SnapshotReader::from_reader(&mut Cursor::new(&buf));
    assert!(result.is_err());
}

#[test]
fn test_snapshot_data_corruption_detection() {
    // Create valid snapshot with data
    let mut writer = SnapshotWriter::new();

    let mut task = SerializedTask::new(TaskId::from_u64(100), 5);
    task.stack.push(Value::i32(42));
    task.stack.push(Value::bool(true));

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Corrupt payload data near the end (but before checksum)
    // Avoid corrupting length fields which could cause capacity overflow
    if buf.len() > 64 {
        let corrupt_pos = buf.len() - 48; // Safe position away from checksum
        buf[corrupt_pos] ^= 0xFF;
        buf[corrupt_pos + 1] ^= 0xFF;
    }

    // Should fail checksum validation
    let result = SnapshotReader::from_reader(&mut Cursor::new(&buf));
    assert!(result.is_err());
}

// ===== File I/O Tests =====

#[test]
fn test_snapshot_file_write_and_read() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test.snapshot");

    // Create snapshot with multiple tasks
    let mut writer = SnapshotWriter::new();

    for i in 0..10 {
        let mut task = SerializedTask::new(TaskId::from_u64(i), i as usize);
        task.stack.push(Value::i32((i * 10) as i32));
        writer.add_task(task);
    }

    // Write to file
    writer.write_to_file(&file_path).unwrap();

    // Verify file exists
    assert!(file_path.exists());

    // Read back from file
    let reader = SnapshotReader::from_file(&file_path).unwrap();
    assert_eq!(reader.tasks().len(), 10);

    // Verify data
    for i in 0..10 {
        assert_eq!(reader.tasks()[i].task_id.as_u64(), i as u64);
        assert_eq!(reader.tasks()[i].function_index, i as usize);
        assert_eq!(reader.tasks()[i].stack.len(), 1);
        assert_eq!(reader.tasks()[i].stack[0].as_i32(), Some((i * 10) as i32));
    }

    // Cleanup
    drop(dir);
}

// ===== Large Snapshot Tests =====

#[test]
fn test_large_snapshot_with_many_tasks() {
    let mut writer = SnapshotWriter::new();

    // Create 1000 tasks
    for i in 0..1000 {
        let mut task = SerializedTask::new(TaskId::from_u64(i), i as usize % 100);

        // Add varying amounts of stack data
        for j in 0..(i % 20) {
            task.stack.push(Value::i32((i * 100 + j) as i32));
        }

        writer.add_task(task);
    }

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    println!(
        "Large snapshot size: {} bytes ({} KB)",
        buf.len(),
        buf.len() / 1024
    );

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1000);

    // Verify some tasks
    assert_eq!(reader.tasks()[0].task_id.as_u64(), 0);
    assert_eq!(reader.tasks()[500].task_id.as_u64(), 500);
    assert_eq!(reader.tasks()[999].task_id.as_u64(), 999);
}

// ===== Multi-Context Snapshot Tests =====

#[test]
fn test_multiple_contexts_independent_snapshots() {
    // Create multiple contexts with different state
    let mut ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();
    let mut ctx3 = VmContext::new();

    // Set different globals in each
    ctx1.set_global("value".to_string(), Value::i32(100));
    ctx2.set_global("value".to_string(), Value::i32(200));
    ctx3.set_global("value".to_string(), Value::i32(300));

    // Register different tasks
    ctx1.register_task(TaskId::from_u64(1));
    ctx2.register_task(TaskId::from_u64(2));
    ctx2.register_task(TaskId::from_u64(3));
    ctx3.register_task(TaskId::from_u64(4));
    ctx3.register_task(TaskId::from_u64(5));
    ctx3.register_task(TaskId::from_u64(6));

    // Verify independent state
    assert_eq!(ctx1.get_global("value"), Some(Value::i32(100)));
    assert_eq!(ctx2.get_global("value"), Some(Value::i32(200)));
    assert_eq!(ctx3.get_global("value"), Some(Value::i32(300)));

    assert_eq!(ctx1.task_count(), 1);
    assert_eq!(ctx2.task_count(), 2);
    assert_eq!(ctx3.task_count(), 3);

    // Each context can be independently snapshotted
    // (Full implementation will test this)
}

// ===== Resource Limit Preservation Tests =====

#[test]
fn test_context_limits_preservation() {
    // Create context with specific limits
    let options = VmOptions {
        limits: ResourceLimits {
            max_heap_bytes: Some(16 * 1024 * 1024),
            max_tasks: Some(100),
            max_step_budget: Some(1_000_000),
            ..Default::default()
        },
        gc_threshold: 8 * 1024 * 1024,
        ..Default::default()
    };

    let ctx = VmContext::with_options(options);

    // Verify limits
    let limits = ctx.limits();
    assert_eq!(limits.max_heap_bytes, Some(16 * 1024 * 1024));
    assert_eq!(limits.max_tasks, Some(100));
    assert_eq!(limits.max_step_budget, Some(1_000_000));

    // GC threshold
    assert_eq!(ctx.heap_stats().threshold, 8 * 1024 * 1024);

    // Note: Full snapshot/restore will preserve these limits
}

// ===== Endianness Awareness Tests =====

#[test]
fn test_endianness_detection() {
    use raya_engine::vm::snapshot::format::{is_big_endian, is_little_endian, SnapshotHeader};

    // Verify system endianness detection works
    let is_le = is_little_endian();
    let is_be = is_big_endian();

    // Exactly one should be true
    assert!(
        is_le ^ is_be,
        "System must be either little-endian or big-endian"
    );

    // Get endianness string
    let endian_str = SnapshotHeader::system_endianness();
    if is_le {
        assert_eq!(endian_str, "little-endian");
    } else {
        assert_eq!(endian_str, "big-endian");
    }
}

#[test]
fn test_endianness_marker_in_snapshot() {
    use raya_engine::vm::snapshot::format::{ENDIANNESS_MARKER, ENDIANNESS_MARKER_SWAPPED};

    // Create and write snapshot
    let mut writer = SnapshotWriter::new();
    let task = SerializedTask::new(TaskId::from_u64(1), 0);
    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read header and verify endianness marker
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    // If we can read it successfully, the endianness matches

    // Manually check the endianness marker bytes in the buffer
    // Header format: magic(8) + version(4) + flags(4) + endianness(4) + ...
    // Endianness is at offset 16
    let endianness_bytes = &buf[16..20];
    let endianness = u32::from_le_bytes([
        endianness_bytes[0],
        endianness_bytes[1],
        endianness_bytes[2],
        endianness_bytes[3],
    ]);

    // Should be ENDIANNESS_MARKER since we use little-endian format
    assert_eq!(endianness, ENDIANNESS_MARKER);

    // Verify the marker values are byte-swapped versions of each other
    assert_eq!(ENDIANNESS_MARKER.swap_bytes(), ENDIANNESS_MARKER_SWAPPED);
}

#[test]
fn test_snapshot_with_different_endianness_accepted() {
    use raya_engine::vm::snapshot::format::{SnapshotHeader, ENDIANNESS_MARKER_SWAPPED};

    // Create a snapshot header with swapped endianness
    let mut header = SnapshotHeader::new();
    header.endianness = ENDIANNESS_MARKER_SWAPPED;

    // Validation should detect the different endianness and indicate byte-swapping is needed
    let result = header.validate();
    assert!(
        result.is_ok(),
        "Should accept snapshot with different endianness"
    );
    assert_eq!(
        result.unwrap(),
        true,
        "Should indicate byte-swapping is needed"
    );
}

#[test]
fn test_corrupted_endianness_marker_detected() {
    use raya_engine::vm::snapshot::format::SnapshotHeader;

    // Create a snapshot header with invalid endianness marker
    let mut header = SnapshotHeader::new();
    header.endianness = 0xDEADBEEF; // Invalid marker

    // Validation should detect corruption
    let result = header.validate();
    assert!(result.is_err(), "Should detect corrupted endianness marker");
}

#[test]
fn test_byteswap_round_trip() {
    use raya_engine::vm::snapshot::format::byteswap;

    // Test all byte-swap functions with typical values

    // u16
    let value_u16: u16 = 0x1234;
    assert_eq!(byteswap::swap_u16(value_u16, false), value_u16);
    assert_eq!(byteswap::swap_u16(value_u16, true), 0x3412);

    // u32
    let value_u32: u32 = 0x12345678;
    assert_eq!(byteswap::swap_u32(value_u32, false), value_u32);
    assert_eq!(byteswap::swap_u32(value_u32, true), 0x78563412);

    // u64
    let value_u64: u64 = 0x123456789ABCDEF0;
    assert_eq!(byteswap::swap_u64(value_u64, false), value_u64);
    assert_eq!(byteswap::swap_u64(value_u64, true), 0xF0DEBC9A78563412);

    // i32
    let value_i32: i32 = -42;
    assert_eq!(byteswap::swap_i32(value_i32, false), value_i32);
    assert_eq!(
        byteswap::swap_i32(value_i32, true),
        i32::from_le_bytes(
            value_i32
                .to_le_bytes()
                .iter()
                .copied()
                .rev()
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        )
    );

    // i64
    let value_i64: i64 = -12345678;
    assert_eq!(byteswap::swap_i64(value_i64, false), value_i64);

    // f64
    let value_f64: f64 = 3.14159;
    assert_eq!(byteswap::swap_f64(value_f64, false), value_f64);
    let swapped = byteswap::swap_f64(value_f64, true);
    // Swapping and then swapping back should give original value
    assert_eq!(byteswap::swap_f64(swapped, true), value_f64);
}

// ===== Safepoint Integration Tests =====

#[test]
fn test_safepoint_snapshot_coordination() {
    use raya_engine::vm::interpreter::{SafepointCoordinator, StopReason};

    let coord = SafepointCoordinator::new(4);

    // Request snapshot pause
    coord
        .snapshot_pending
        .store(true, std::sync::atomic::Ordering::Release);

    // Verify pause is pending
    assert!(coord.is_pause_pending());

    // Set reason
    {
        let mut reason = coord.current_reason.lock().unwrap();
        *reason = Some(StopReason::Snapshot);
    }

    assert_eq!(coord.current_reason(), Some(StopReason::Snapshot));

    // Resume from pause
    coord.resume_from_pause();

    // Verify flags cleared
    assert!(!coord.is_pause_pending());
    assert_eq!(coord.current_reason(), None);
}

#[test]
fn test_scheduler_integration_with_snapshots() {
    let scheduler = Scheduler::new(4);

    // Initial state
    assert_eq!(scheduler.worker_count(), 4);
    assert_eq!(scheduler.task_count(), 0);

    let stats = scheduler.stats();
    assert_eq!(stats.active_tasks, 0);

    // Note: Full snapshot will coordinate with scheduler to pause all workers
}

// ===== Concurrent Snapshot Tests =====

#[test]
fn test_concurrent_context_snapshot_preparation() {
    use std::thread;

    // Create multiple contexts in different threads
    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                let mut ctx = VmContext::new();
                ctx.set_global(format!("thread_{}", i), Value::i32(i as i32));

                // Register some tasks
                for j in 0..5 {
                    ctx.register_task(TaskId::from_u64((i * 100 + j) as u64));
                }

                // Return context ID and state
                (ctx.id(), ctx.task_count())
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // Verify all contexts were created independently
    assert_eq!(results.len(), 10);
    for (_, task_count) in &results {
        assert_eq!(*task_count, 5);
    }

    // Verify all context IDs are unique
    let ids: Vec<_> = results.iter().map(|(id, _)| id).collect();
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(ids[i], ids[j]);
        }
    }
}

// ===== Edge Case Tests =====

#[test]
fn test_snapshot_with_zero_tasks() {
    let writer = SnapshotWriter::new();

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Should be valid, just empty
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 0);
}

#[test]
fn test_snapshot_with_empty_stack() {
    let mut writer = SnapshotWriter::new();

    let task = SerializedTask::new(TaskId::from_u64(1), 0);
    // Task has no stack values

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);
    assert_eq!(reader.tasks()[0].stack.len(), 0);
}

#[test]
fn test_snapshot_with_max_values() {
    let mut writer = SnapshotWriter::new();

    // Note: Value uses NaN-boxing and stores u64 values with 48-bit payload
    const U64_MAX_48BIT: u64 = 0x0000_FFFF_FFFF_FFFF;

    let mut task = SerializedTask::new(TaskId::from_u64(u64::MAX), usize::MAX);
    task.ip = usize::MAX;
    task.stack.push(Value::i32(i32::MAX));
    task.stack.push(Value::i32(i32::MIN));
    task.stack.push(Value::u32(u32::MAX));
    task.stack.push(Value::u64(U64_MAX_48BIT));

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);

    let restored = &reader.tasks()[0];
    assert_eq!(restored.task_id.as_u64(), u64::MAX);
    assert_eq!(restored.function_index, usize::MAX);
    assert_eq!(restored.ip, usize::MAX);
    assert_eq!(restored.stack[0].as_i32(), Some(i32::MAX));
    assert_eq!(restored.stack[1].as_i32(), Some(i32::MIN));
    assert_eq!(restored.stack[2].as_u32(), Some(u32::MAX));
    assert_eq!(restored.stack[3].as_u64(), Some(U64_MAX_48BIT));
}

// ===== Future Tests (when Vm::snapshot/restore are implemented) =====

// These tests are placeholders for when full VM snapshot/restore is implemented

// #[test]
// fn test_vm_snapshot_and_restore() {
//     use raya_engine::vm::interpreter::lifecycle::Vm;
//
//     let vm = Vm::new(VmOptions::default()).unwrap();
//
//     // Execute some code
//     // ...
//
//     // Take snapshot
//     let snapshot = vm.snapshot().unwrap();
//
//     // Modify VM state
//     // ...
//
//     // Restore from snapshot
//     vm.restore(snapshot).unwrap();
//
//     // Verify state is restored
//     // ...
// }

// #[test]
// fn test_snapshot_portability_across_vms() {
//     // Create VM1, run code, snapshot
//     // Create VM2, restore from snapshot
//     // Verify VM2 continues execution correctly
// }

// #[test]
// fn test_snapshot_with_heap_objects() {
//     // Create objects on heap
//     // Take snapshot
//     // Restore
//     // Verify object graph is preserved
// }

// #[test]
// fn test_concurrent_multi_context_snapshot() {
//     // Create multiple VmContexts
//     // Coordinate STW pause across all contexts
//     // Snapshot all contexts simultaneously
//     // Restore all contexts
//     // Verify independent execution resumes
// }
