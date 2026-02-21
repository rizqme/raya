//! Profiling infrastructure for hot function detection

pub mod counters;
pub mod policy;

use std::sync::Arc;
use crate::compiler::bytecode::Module;

/// A request to compile a function on the background JIT thread.
pub struct CompilationRequest {
    /// The module containing the function to compile
    pub module: Arc<Module>,
    /// Index of the function within the module
    pub func_index: usize,
    /// Pre-resolved module ID in the code cache
    pub module_id: u64,
    /// Profile for the module (to call `finish_compile` on completion)
    pub module_profile: Arc<counters::ModuleProfile>,
}

/// Handle to the background JIT compilation thread.
///
/// Dropping this handle closes the channel, causing the background thread to exit.
pub struct BackgroundCompiler {
    /// Sender for submitting compilation requests
    tx: crossbeam::channel::Sender<CompilationRequest>,
}

impl BackgroundCompiler {
    /// Submit a compilation request. Returns false if the channel is full or closed.
    pub fn try_submit(&self, request: CompilationRequest) -> bool {
        self.tx.try_send(request).is_ok()
    }

    /// Create a BackgroundCompiler from a sender (used internally by JitEngine).
    pub(crate) fn new(tx: crossbeam::channel::Sender<CompilationRequest>) -> Self {
        Self { tx }
    }
}
