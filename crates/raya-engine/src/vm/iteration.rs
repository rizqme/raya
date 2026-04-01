use crate::vm::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IteratorProtocolKind {
    Sync,
    Async,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResumeCompletionKind {
    Next = 0,
    Return = 1,
    Throw = 2,
}

impl ResumeCompletionKind {
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeCompletion {
    pub kind: ResumeCompletionKind,
    pub value: Value,
}
