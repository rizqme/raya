//! Task state serialization for snapshots

use crate::scheduler::{TaskId, TaskState};
use crate::value::Value;
use std::io::{Read, Write};

/// Serialized task state
#[derive(Debug, Clone)]
pub struct SerializedTask {
    /// Task ID
    pub task_id: TaskId,

    /// Current state
    pub state: TaskState,

    /// Function index being executed
    pub function_index: usize,

    /// Instruction pointer
    pub ip: usize,

    /// Call stack frames
    pub frames: Vec<SerializedFrame>,

    /// Operand stack
    pub stack: Vec<Value>,

    /// Result (if completed)
    pub result: Option<Value>,

    /// Parent task ID (if spawned from another task)
    pub parent: Option<TaskId>,

    /// Blocked reason (if suspended)
    pub blocked_on: Option<BlockedReason>,
}

impl SerializedTask {
    /// Create a new serialized task
    pub fn new(task_id: TaskId, function_index: usize) -> Self {
        Self {
            task_id,
            state: TaskState::Created,
            function_index,
            ip: 0,
            frames: Vec::new(),
            stack: Vec::new(),
            result: None,
            parent: None,
            blocked_on: None,
        }
    }

    /// Encode to writer
    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        // Write task ID
        writer.write_all(&self.task_id.as_u64().to_le_bytes())?;

        // Write state
        writer.write_all(&[self.state as u8])?;

        // Write function index
        writer.write_all(&(self.function_index as u64).to_le_bytes())?;

        // Write instruction pointer
        writer.write_all(&(self.ip as u64).to_le_bytes())?;

        // Write frame count
        writer.write_all(&(self.frames.len() as u64).to_le_bytes())?;

        // Write frames
        for frame in &self.frames {
            frame.encode(writer)?;
        }

        // Write stack size
        writer.write_all(&(self.stack.len() as u64).to_le_bytes())?;

        // Write stack values
        for value in &self.stack {
            value.encode(writer)?;
        }

        // Write result
        match &self.result {
            Some(value) => {
                writer.write_all(&[1])?;
                value.encode(writer)?;
            }
            None => {
                writer.write_all(&[0])?;
            }
        }

        // Write parent
        match self.parent {
            Some(parent_id) => {
                writer.write_all(&[1])?;
                writer.write_all(&parent_id.as_u64().to_le_bytes())?;
            }
            None => {
                writer.write_all(&[0])?;
            }
        }

        // Write blocked reason
        match &self.blocked_on {
            Some(reason) => {
                writer.write_all(&[1])?;
                reason.encode(writer)?;
            }
            None => {
                writer.write_all(&[0])?;
            }
        }

        Ok(())
    }

    /// Decode from reader
    pub fn decode(reader: &mut impl Read, needs_byte_swap: bool) -> std::io::Result<Self> {
        use crate::snapshot::format::byteswap;

        let mut buf = [0u8; 8];

        // Read task ID
        reader.read_exact(&mut buf)?;
        let task_id = TaskId::from_u64(byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap));

        // Read state
        let mut state_buf = [0u8; 1];
        reader.read_exact(&mut state_buf)?;
        let state = match state_buf[0] {
            0 => TaskState::Created,
            1 => TaskState::Running,
            2 => TaskState::Suspended,
            3 => TaskState::Resumed,
            4 => TaskState::Completed,
            5 => TaskState::Failed,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid task state",
                ))
            }
        };

        // Read function index
        reader.read_exact(&mut buf)?;
        let function_index = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        // Read instruction pointer
        reader.read_exact(&mut buf)?;
        let ip = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        // Read frame count
        reader.read_exact(&mut buf)?;
        let frame_count = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        // Read frames
        let mut frames = Vec::with_capacity(frame_count);
        for _ in 0..frame_count {
            frames.push(SerializedFrame::decode(reader, needs_byte_swap)?);
        }

        // Read stack size
        reader.read_exact(&mut buf)?;
        let stack_size = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        // Read stack values
        let mut stack = Vec::with_capacity(stack_size);
        for _ in 0..stack_size {
            stack.push(Value::decode_with_byteswap(reader, needs_byte_swap)?);
        }

        // Read result
        reader.read_exact(&mut state_buf)?;
        let result = if state_buf[0] == 1 {
            Some(Value::decode_with_byteswap(reader, needs_byte_swap)?)
        } else {
            None
        };

        // Read parent
        reader.read_exact(&mut state_buf)?;
        let parent = if state_buf[0] == 1 {
            reader.read_exact(&mut buf)?;
            Some(TaskId::from_u64(byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap)))
        } else {
            None
        };

        // Read blocked reason
        reader.read_exact(&mut state_buf)?;
        let blocked_on = if state_buf[0] == 1 {
            Some(BlockedReason::decode(reader, needs_byte_swap)?)
        } else {
            None
        };

        Ok(Self {
            task_id,
            state,
            function_index,
            ip,
            frames,
            stack,
            result,
            parent,
            blocked_on,
        })
    }
}

/// Serialized call frame
#[derive(Debug, Clone)]
pub struct SerializedFrame {
    /// Function being executed
    pub function_index: usize,

    /// Return instruction pointer
    pub return_ip: usize,

    /// Base pointer in stack
    pub base_pointer: usize,

    /// Local variables
    pub locals: Vec<Value>,
}

impl SerializedFrame {
    /// Create a new serialized call frame
    pub fn new(function_index: usize) -> Self {
        Self {
            function_index,
            return_ip: 0,
            base_pointer: 0,
            locals: Vec::new(),
        }
    }

    /// Encode call frame to writer
    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&(self.function_index as u64).to_le_bytes())?;
        writer.write_all(&(self.return_ip as u64).to_le_bytes())?;
        writer.write_all(&(self.base_pointer as u64).to_le_bytes())?;
        writer.write_all(&(self.locals.len() as u64).to_le_bytes())?;

        for local in &self.locals {
            local.encode(writer)?;
        }

        Ok(())
    }

    /// Decode call frame from reader
    pub fn decode(reader: &mut impl Read, needs_byte_swap: bool) -> std::io::Result<Self> {
        use crate::snapshot::format::byteswap;

        let mut buf = [0u8; 8];

        reader.read_exact(&mut buf)?;
        let function_index = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        reader.read_exact(&mut buf)?;
        let return_ip = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        reader.read_exact(&mut buf)?;
        let base_pointer = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        reader.read_exact(&mut buf)?;
        let local_count = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

        let mut locals = Vec::with_capacity(local_count);
        for _ in 0..local_count {
            locals.push(Value::decode_with_byteswap(reader, needs_byte_swap)?);
        }

        Ok(Self {
            function_index,
            return_ip,
            base_pointer,
            locals,
        })
    }
}

/// Reason a task is blocked
#[derive(Debug, Clone)]
pub enum BlockedReason {
    /// Waiting for another task to complete
    AwaitingTask(TaskId),

    /// Waiting on a mutex
    AwaitingMutex(u64), // Mutex ID

    /// Other blocking operations
    Other(String),
}

impl BlockedReason {
    /// Encode blocked reason to writer
    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            BlockedReason::AwaitingTask(task_id) => {
                writer.write_all(&[0])?;
                writer.write_all(&task_id.as_u64().to_le_bytes())?;
            }
            BlockedReason::AwaitingMutex(mutex_id) => {
                writer.write_all(&[1])?;
                writer.write_all(&mutex_id.to_le_bytes())?;
            }
            BlockedReason::Other(s) => {
                writer.write_all(&[2])?;
                let bytes = s.as_bytes();
                writer.write_all(&(bytes.len() as u64).to_le_bytes())?;
                writer.write_all(bytes)?;
            }
        }
        Ok(())
    }

    /// Decode blocked reason from reader
    pub fn decode(reader: &mut impl Read, needs_byte_swap: bool) -> std::io::Result<Self> {
        use crate::snapshot::format::byteswap;

        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf)?;

        match buf[0] {
            0 => {
                let mut buf = [0u8; 8];
                reader.read_exact(&mut buf)?;
                let task_id = TaskId::from_u64(byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap));
                Ok(BlockedReason::AwaitingTask(task_id))
            }
            1 => {
                let mut buf = [0u8; 8];
                reader.read_exact(&mut buf)?;
                let mutex_id = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap);
                Ok(BlockedReason::AwaitingMutex(mutex_id))
            }
            2 => {
                let mut buf = [0u8; 8];
                reader.read_exact(&mut buf)?;
                let len = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;

                let mut bytes = vec![0u8; len];
                reader.read_exact(&mut bytes)?;
                let s = String::from_utf8(bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                Ok(BlockedReason::Other(s))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid blocked reason type",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialized_task_encode_decode() {
        let task_id = TaskId::from_u64(42);
        let mut task = SerializedTask::new(task_id, 10);
        task.state = TaskState::Running;
        task.ip = 100;
        task.stack.push(Value::i32(42));
        task.stack.push(Value::bool(true));

        let mut buf = Vec::new();
        task.encode(&mut buf).unwrap();

        let decoded = SerializedTask::decode(&mut &buf[..], false).unwrap();
        assert_eq!(decoded.task_id.as_u64(), 42);
        assert_eq!(decoded.state, TaskState::Running);
        assert_eq!(decoded.function_index, 10);
        assert_eq!(decoded.ip, 100);
        assert_eq!(decoded.stack.len(), 2);
    }

    #[test]
    fn test_serialized_frame_encode_decode() {
        let mut frame = SerializedFrame::new(5);
        frame.return_ip = 50;
        frame.base_pointer = 10;
        frame.locals.push(Value::i32(100));
        frame.locals.push(Value::null());

        let mut buf = Vec::new();
        frame.encode(&mut buf).unwrap();

        let decoded = SerializedFrame::decode(&mut &buf[..], false).unwrap();
        assert_eq!(decoded.function_index, 5);
        assert_eq!(decoded.return_ip, 50);
        assert_eq!(decoded.base_pointer, 10);
        assert_eq!(decoded.locals.len(), 2);
    }

    #[test]
    fn test_blocked_reason_encode_decode() {
        // Test AwaitingTask
        let reason = BlockedReason::AwaitingTask(TaskId::from_u64(123));
        let mut buf = Vec::new();
        reason.encode(&mut buf).unwrap();
        let decoded = BlockedReason::decode(&mut &buf[..], false).unwrap();
        match decoded {
            BlockedReason::AwaitingTask(id) => assert_eq!(id.as_u64(), 123),
            _ => panic!("Wrong variant"),
        }

        // Test AwaitingMutex
        let reason = BlockedReason::AwaitingMutex(456);
        let mut buf = Vec::new();
        reason.encode(&mut buf).unwrap();
        let decoded = BlockedReason::decode(&mut &buf[..], false).unwrap();
        match decoded {
            BlockedReason::AwaitingMutex(id) => assert_eq!(id, 456),
            _ => panic!("Wrong variant"),
        }

        // Test Other
        let reason = BlockedReason::Other("test reason".to_string());
        let mut buf = Vec::new();
        reason.encode(&mut buf).unwrap();
        let decoded = BlockedReason::decode(&mut &buf[..], false).unwrap();
        match decoded {
            BlockedReason::Other(s) => assert_eq!(s, "test reason"),
            _ => panic!("Wrong variant"),
        }
    }
}
