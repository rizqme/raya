//! Integration tests for VM Snapshotting (Milestone 1.11)

#![allow(clippy::identity_op)]

use raya_core::scheduler::TaskId;
use raya_core::snapshot::{SerializedTask, SnapshotReader, SnapshotWriter};
use raya_core::value::Value;
use std::io::Cursor;

#[test]
fn test_empty_snapshot() {
    let writer = SnapshotWriter::new();
    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Should be able to read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 0);
}

#[test]
fn test_snapshot_with_single_task() {
    let mut writer = SnapshotWriter::new();

    let task = SerializedTask::new(TaskId::from_u64(42), 10);
    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);
    assert_eq!(reader.tasks()[0].task_id.as_u64(), 42);
    assert_eq!(reader.tasks()[0].function_index, 10);
}

#[test]
fn test_snapshot_with_multiple_tasks() {
    let mut writer = SnapshotWriter::new();

    for i in 0..10 {
        let task = SerializedTask::new(TaskId::from_u64(i), i as usize);
        writer.add_task(task);
    }

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 10);

    for (i, task) in reader.tasks().iter().enumerate() {
        assert_eq!(task.task_id.as_u64(), i as u64);
        assert_eq!(task.function_index, i);
    }
}

#[test]
fn test_snapshot_with_task_state() {
    use raya_core::scheduler::TaskState;

    let mut writer = SnapshotWriter::new();

    let mut task = SerializedTask::new(TaskId::from_u64(100), 5);
    task.state = TaskState::Running;
    task.ip = 42;
    task.stack.push(Value::i32(10));
    task.stack.push(Value::i32(20));
    task.stack.push(Value::bool(true));

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);

    let restored = &reader.tasks()[0];
    assert_eq!(restored.state, TaskState::Running);
    assert_eq!(restored.ip, 42);
    assert_eq!(restored.stack.len(), 3);
    assert_eq!(restored.stack[0].as_i32(), Some(10));
    assert_eq!(restored.stack[1].as_i32(), Some(20));
    assert_eq!(restored.stack[2].as_bool(), Some(true));
}

#[test]
fn test_snapshot_with_completed_task() {
    use raya_core::scheduler::TaskState;

    let mut writer = SnapshotWriter::new();

    let mut task = SerializedTask::new(TaskId::from_u64(200), 0);
    task.state = TaskState::Completed;
    task.result = Some(Value::i32(42));

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);

    let restored = &reader.tasks()[0];
    assert_eq!(restored.state, TaskState::Completed);
    assert!(restored.result.is_some());
    assert_eq!(restored.result.unwrap().as_i32(), Some(42));
}

#[test]
fn test_snapshot_with_parent_child_tasks() {
    let mut writer = SnapshotWriter::new();

    let parent = SerializedTask::new(TaskId::from_u64(1), 0);

    let mut child1 = SerializedTask::new(TaskId::from_u64(2), 1);
    child1.parent = Some(TaskId::from_u64(1));

    let mut child2 = SerializedTask::new(TaskId::from_u64(3), 1);
    child2.parent = Some(TaskId::from_u64(1));

    writer.add_task(parent);
    writer.add_task(child1);
    writer.add_task(child2);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 3);

    assert_eq!(reader.tasks()[0].task_id.as_u64(), 1);
    assert!(reader.tasks()[0].parent.is_none());

    assert_eq!(reader.tasks()[1].task_id.as_u64(), 2);
    assert_eq!(reader.tasks()[1].parent.unwrap().as_u64(), 1);

    assert_eq!(reader.tasks()[2].task_id.as_u64(), 3);
    assert_eq!(reader.tasks()[2].parent.unwrap().as_u64(), 1);
}

#[test]
fn test_snapshot_with_blocked_task() {
    use raya_core::snapshot::BlockedReason;

    let mut writer = SnapshotWriter::new();

    let mut task = SerializedTask::new(TaskId::from_u64(50), 0);
    task.blocked_on = Some(BlockedReason::AwaitingTask(TaskId::from_u64(51)));

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);

    let restored = &reader.tasks()[0];
    assert!(restored.blocked_on.is_some());

    match &restored.blocked_on {
        Some(BlockedReason::AwaitingTask(id)) => {
            assert_eq!(id.as_u64(), 51);
        }
        _ => panic!("Wrong blocked reason"),
    }
}

#[test]
fn test_snapshot_header_validation() {
    use raya_core::snapshot::format::{SnapshotHeader, SNAPSHOT_MAGIC, SNAPSHOT_VERSION};

    let header = SnapshotHeader::new();
    assert_eq!(header.magic, SNAPSHOT_MAGIC);
    assert_eq!(header.version, SNAPSHOT_VERSION);
    assert_eq!(header.endianness, 0x01020304);
    assert!(header.validate().is_ok());
}

#[test]
fn test_snapshot_invalid_magic() {
    let mut buf = vec![0u8; 100];
    // Write invalid magic
    buf[0..8].copy_from_slice(&0u64.to_le_bytes());

    let result = SnapshotReader::from_reader(&mut Cursor::new(&buf));
    assert!(result.is_err());
}

#[test]
fn test_snapshot_checksum_validation() {
    // Create a valid snapshot with a task
    let mut writer = SnapshotWriter::new();
    let task = SerializedTask::new(TaskId::from_u64(1), 0);
    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Should validate correctly
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf));
    assert!(reader.is_ok());

    // Corrupt the checksum itself (last 32 bytes)
    if buf.len() > 32 {
        let len = buf.len();
        buf[len - 1] ^= 0xFF; // Flip bits in the checksum
    }

    // Should fail checksum validation
    let result = SnapshotReader::from_reader(&mut Cursor::new(&buf));
    assert!(result.is_err());
}

#[test]
fn test_snapshot_file_round_trip() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test_snapshot.raya");

    // Create and write snapshot
    let mut writer = SnapshotWriter::new();
    let task1 = SerializedTask::new(TaskId::from_u64(100), 5);
    let task2 = SerializedTask::new(TaskId::from_u64(200), 10);
    writer.add_task(task1);
    writer.add_task(task2);

    writer.write_to_file(&file_path).unwrap();

    // Read it back
    let reader = SnapshotReader::from_file(&file_path).unwrap();
    assert_eq!(reader.tasks().len(), 2);
    assert_eq!(reader.tasks()[0].task_id.as_u64(), 100);
    assert_eq!(reader.tasks()[1].task_id.as_u64(), 200);

    // Cleanup
    drop(dir);
}

#[test]
fn test_value_serialization() {
    // Test various value types
    let values = vec![
        Value::null(),
        Value::bool(true),
        Value::bool(false),
        Value::i32(42),
        Value::i32(-42),
        Value::i32(0),
        Value::f64(3.14),
        Value::f64(-3.14),
        Value::f64(0.0),
        Value::u32(100),
        Value::u64(1000),
    ];

    for original in values {
        let mut buf = Vec::new();
        original.encode(&mut buf).unwrap();

        let decoded = Value::decode(&mut Cursor::new(&buf)).unwrap();

        // Compare based on type
        if original.is_null() {
            assert!(decoded.is_null());
        } else if original.is_bool() {
            assert_eq!(original.as_bool(), decoded.as_bool());
        } else if original.is_i32() {
            assert_eq!(original.as_i32(), decoded.as_i32());
        } else if original.is_f64() {
            assert_eq!(original.as_f64(), decoded.as_f64());
        } else if original.is_u32() {
            assert_eq!(original.as_u32(), decoded.as_u32());
        } else if original.is_u64() {
            assert_eq!(original.as_u64(), decoded.as_u64());
        }
    }
}

#[test]
fn test_large_snapshot() {
    let mut writer = SnapshotWriter::new();

    // Create many tasks
    for i in 0..1000 {
        let mut task = SerializedTask::new(TaskId::from_u64(i), i as usize % 100);

        // Add some stack values
        for j in 0..10 {
            task.stack.push(Value::i32((i * 10 + j) as i32));
        }

        writer.add_task(task);
    }

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    println!("Large snapshot size: {} bytes", buf.len());

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1000);

    // Verify some tasks
    assert_eq!(reader.tasks()[0].task_id.as_u64(), 0);
    assert_eq!(reader.tasks()[0].stack.len(), 10);
    assert_eq!(reader.tasks()[999].task_id.as_u64(), 999);
    assert_eq!(reader.tasks()[999].stack.len(), 10);
}

#[test]
fn test_snapshot_with_call_frames() {
    use raya_core::snapshot::SerializedFrame;

    let mut writer = SnapshotWriter::new();

    let mut task = SerializedTask::new(TaskId::from_u64(300), 0);

    // Add call frames (simulating a call stack)
    let mut frame1 = SerializedFrame::new(0);
    frame1.return_ip = 100;
    frame1.base_pointer = 0;
    frame1.locals.push(Value::i32(10));
    frame1.locals.push(Value::i32(20));

    let mut frame2 = SerializedFrame::new(1);
    frame2.return_ip = 200;
    frame2.base_pointer = 5;
    frame2.locals.push(Value::bool(true));

    task.frames.push(frame1);
    task.frames.push(frame2);

    writer.add_task(task);

    let mut buf = Vec::new();
    writer.write_snapshot(&mut buf).unwrap();

    // Read it back
    let reader = SnapshotReader::from_reader(&mut Cursor::new(&buf)).unwrap();
    assert_eq!(reader.tasks().len(), 1);

    let restored = &reader.tasks()[0];
    assert_eq!(restored.frames.len(), 2);

    assert_eq!(restored.frames[0].function_index, 0);
    assert_eq!(restored.frames[0].return_ip, 100);
    assert_eq!(restored.frames[0].base_pointer, 0);
    assert_eq!(restored.frames[0].locals.len(), 2);

    assert_eq!(restored.frames[1].function_index, 1);
    assert_eq!(restored.frames[1].return_ip, 200);
    assert_eq!(restored.frames[1].base_pointer, 5);
    assert_eq!(restored.frames[1].locals.len(), 1);
}
