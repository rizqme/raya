//! Shared suspend/resume model across interpreter, scheduler, AOT, and JIT.

use crate::vm::sync::{MutexId, SemaphoreId};
use crate::vm::value::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Unique identifier for a task.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    /// Generate a new unique task id.
    pub fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the numeric id value.
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Create a task id from a raw integer.
    pub fn from_u64(id: u64) -> Self {
        TaskId(id)
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile-time/backend suspension classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionSuspendKind {
    AwaitTask,
    YieldNow,
    Sleep,
    IoWait,
    ChannelReceive,
    ChannelSend,
    MutexAcquire,
    SemaphoreAcquire,
    KernelBoundary,
    InterpreterBoundary,
    AotCall,
    Preemption,
    GeneratorYield,
    GeneratorInit,
}

impl ExecutionSuspendKind {
    pub fn always_suspends(self) -> bool {
        matches!(
            self,
            ExecutionSuspendKind::AwaitTask
                | ExecutionSuspendKind::YieldNow
                | ExecutionSuspendKind::Sleep
                | ExecutionSuspendKind::IoWait
                | ExecutionSuspendKind::ChannelReceive
                | ExecutionSuspendKind::ChannelSend
                | ExecutionSuspendKind::MutexAcquire
                | ExecutionSuspendKind::SemaphoreAcquire
                | ExecutionSuspendKind::GeneratorYield
                | ExecutionSuspendKind::GeneratorInit
        )
    }

    pub fn has_child_frame(self) -> bool {
        matches!(self, ExecutionSuspendKind::AotCall)
    }
}

/// How a blocked operation should resume once ownership is transferred.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResumePolicy {
    Reexecute = 0,
    ReturnNull = 1,
    UseResumeValue = 2,
}

impl ResumePolicy {
    pub fn from_u64(raw: u64) -> Self {
        match raw as u32 {
            1 => ResumePolicy::ReturnNull,
            2 => ResumePolicy::UseResumeValue,
            _ => ResumePolicy::Reexecute,
        }
    }

    pub fn as_u64(self) -> u64 {
        self as u32 as u64
    }
}

/// Canonical runtime/scheduler-facing suspend reason.
#[derive(Debug, Clone)]
pub enum SuspendReason {
    AwaitTask(TaskId),
    YieldNow,
    Preemption,
    KernelBoundary,
    Sleep { wake_at: Instant },
    IoWait,
    ChannelReceive { channel_id: u64 },
    ChannelSend { channel_id: u64, value: Value },
    MutexAcquire {
        mutex_id: MutexId,
        resume_policy: ResumePolicy,
    },
    SemaphoreAcquire { semaphore_id: SemaphoreId },
    JsGeneratorYield { value: Value },
    JsGeneratorInit,
}

/// ABI tag shared by AOT and JIT suspend transport.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuspendTag {
    #[default]
    None = 0,
    AwaitTask = 1,
    YieldNow = 2,
    Sleep = 3,
    IoWait = 4,
    ChannelReceive = 5,
    ChannelSend = 6,
    MutexAcquire = 7,
    SemaphoreAcquire = 8,
    KernelBoundary = 9,
    Preemption = 10,
    JsGeneratorYield = 11,
    JsGeneratorInit = 12,
    InterpreterBoundary = 13,
}

impl SuspendTag {
    pub fn from_reason(reason: &SuspendReason) -> Self {
        match reason {
            SuspendReason::AwaitTask(_) => SuspendTag::AwaitTask,
            SuspendReason::YieldNow => SuspendTag::YieldNow,
            SuspendReason::Preemption => SuspendTag::Preemption,
            SuspendReason::KernelBoundary => SuspendTag::KernelBoundary,
            SuspendReason::Sleep { .. } => SuspendTag::Sleep,
            SuspendReason::IoWait => SuspendTag::IoWait,
            SuspendReason::ChannelReceive { .. } => SuspendTag::ChannelReceive,
            SuspendReason::ChannelSend { .. } => SuspendTag::ChannelSend,
            SuspendReason::MutexAcquire { .. } => SuspendTag::MutexAcquire,
            SuspendReason::SemaphoreAcquire { .. } => SuspendTag::SemaphoreAcquire,
            SuspendReason::JsGeneratorYield { .. } => SuspendTag::JsGeneratorYield,
            SuspendReason::JsGeneratorInit => SuspendTag::JsGeneratorInit,
        }
    }
}

/// Shared ABI transport for suspend reasons/payloads.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SuspendRecord {
    pub tag: SuspendTag,
    pub _reserved: u32,
    pub word0: u64,
    pub word1: u64,
}

impl SuspendRecord {
    pub const fn none() -> Self {
        Self {
            tag: SuspendTag::None,
            _reserved: 0,
            word0: 0,
            word1: 0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::none();
    }

    pub fn set_tag(&mut self, tag: SuspendTag) {
        self.tag = tag;
        self.word0 = 0;
        self.word1 = 0;
    }

    pub fn set_reason(&mut self, reason: &SuspendReason) {
        self.tag = SuspendTag::from_reason(reason);
        self.word0 = 0;
        self.word1 = 0;
        match reason {
            SuspendReason::AwaitTask(task_id) => {
                self.word0 = task_id.as_u64();
            }
            SuspendReason::YieldNow
            | SuspendReason::Preemption
            | SuspendReason::KernelBoundary
            | SuspendReason::IoWait
            | SuspendReason::JsGeneratorInit => {}
            SuspendReason::Sleep { wake_at } => {
                let millis = wake_at
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(u64::MAX as u128) as u64;
                self.word0 = millis;
            }
            SuspendReason::ChannelReceive { channel_id } => {
                self.word0 = *channel_id;
            }
            SuspendReason::ChannelSend { channel_id, value } => {
                self.word0 = *channel_id;
                self.word1 = value.raw();
            }
            SuspendReason::MutexAcquire {
                mutex_id,
                resume_policy,
            } => {
                self.word0 = mutex_id.as_u64();
                self.word1 = resume_policy.as_u64();
            }
            SuspendReason::SemaphoreAcquire { semaphore_id } => {
                self.word0 = semaphore_id.as_u64();
            }
            SuspendReason::JsGeneratorYield { value } => {
                self.word0 = value.raw();
            }
        }
    }

    pub fn to_runtime_reason(&self) -> Option<SuspendReason> {
        match self.tag {
            SuspendTag::None => None,
            SuspendTag::AwaitTask => Some(SuspendReason::AwaitTask(TaskId::from_u64(self.word0))),
            SuspendTag::YieldNow => Some(SuspendReason::YieldNow),
            SuspendTag::Preemption => Some(SuspendReason::Preemption),
            SuspendTag::KernelBoundary => Some(SuspendReason::KernelBoundary),
            SuspendTag::Sleep => Some(SuspendReason::Sleep {
                wake_at: Instant::now() + Duration::from_millis(self.word0),
            }),
            SuspendTag::IoWait => Some(SuspendReason::IoWait),
            SuspendTag::ChannelReceive => Some(SuspendReason::ChannelReceive {
                channel_id: self.word0,
            }),
            SuspendTag::ChannelSend => Some(SuspendReason::ChannelSend {
                channel_id: self.word0,
                value: unsafe { Value::from_raw(self.word1) },
            }),
            SuspendTag::MutexAcquire => Some(SuspendReason::MutexAcquire {
                mutex_id: MutexId::from_u64(self.word0),
                resume_policy: ResumePolicy::from_u64(self.word1),
            }),
            SuspendTag::SemaphoreAcquire => Some(SuspendReason::SemaphoreAcquire {
                semaphore_id: SemaphoreId::from_u64(self.word0),
            }),
            SuspendTag::JsGeneratorYield => Some(SuspendReason::JsGeneratorYield {
                value: unsafe { Value::from_raw(self.word0) },
            }),
            SuspendTag::JsGeneratorInit => Some(SuspendReason::JsGeneratorInit),
            SuspendTag::InterpreterBoundary => None,
        }
    }
}
