//! VM Lifecycle & Control API
//!
//! High-level API for creating, managing, and controlling isolated VmContexts.
//! This module provides the public-facing API for Inner VMs.

use crate::scheduler::TaskId;
use crate::value::Value;
use crate::vm::{VmContext, VmContextId, VmOptions};
use parking_lot::RwLock;
use raya_compiler::Module;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during VM operations
#[derive(Debug, Error)]
pub enum VmError {
    /// IO error (file not found, permission denied, etc.)
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid binary format
    #[error("Invalid binary format: {0}")]
    InvalidBinaryFormat(String),

    /// Checksum mismatch (module integrity verification failed)
    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    /// Context not found
    #[error("Context not found: {0:?}")]
    ContextNotFound(VmContextId),

    /// Entry point not found
    #[error("Entry point not found: {0}")]
    EntryPointNotFound(String),

    /// Execution error
    #[error("Execution error: {0}")]
    ExecutionError(String),

    /// Resource limit exceeded
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),

    /// Task creation failed
    #[error("Task creation failed: {0}")]
    TaskCreationFailed(String),
}

/// Statistics for a running VM
#[derive(Debug, Clone)]
pub struct VmStats {
    /// Current heap usage in bytes
    pub heap_bytes_used: usize,

    /// Maximum heap size limit (0 = unlimited)
    pub max_heap_bytes: usize,

    /// Current number of active tasks
    pub tasks: usize,

    /// Maximum task limit (0 = unlimited)
    pub max_tasks: usize,

    /// Total CPU steps executed
    pub steps_executed: u64,
}

/// High-level VM handle
///
/// This is the main public API for working with isolated VmContexts.
/// It owns a VmContext and provides convenient methods for:
/// - Loading bytecode (.rbin files)
/// - Executing code
/// - Managing lifecycle
/// - Observing stats
/// - Snapshotting state
pub struct Vm {
    /// The owned VmContext (wrapped in Arc<RwLock> for interior mutability)
    context: Arc<RwLock<VmContext>>,
}

impl Vm {
    /// Create a new isolated VmContext
    ///
    /// # Arguments
    /// * `options` - Configuration options for the VM
    ///
    /// # Returns
    /// * `Ok(Vm)` - Successfully created VM
    /// * `Err(VmError)` - Failed to create VM
    ///
    /// # Example
    /// ```
    /// use raya_core::vm::{InnerVm, VmOptions, ResourceLimits};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let vm = InnerVm::new(VmOptions {
    ///     limits: ResourceLimits {
    ///         max_heap_bytes: Some(16 * 1024 * 1024),
    ///         ..Default::default()
    ///     },
    ///     ..Default::default()
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(options: VmOptions) -> Result<Self, VmError> {
        let context = VmContext::with_options(options);
        Ok(Self {
            context: Arc::new(RwLock::new(context)),
        })
    }

    /// Create a VM from a snapshot
    ///
    /// # Arguments
    /// * `_snapshot` - The snapshot to restore from
    /// * `_options` - Optional new resource limits (can update limits on restore)
    ///
    /// # Returns
    /// * `Ok(Vm)` - Successfully restored VM
    /// * `Err(VmError)` - Failed to restore
    ///
    /// # Example
    /// ```no_run
    /// # use raya_core::vm::{InnerVm, VmSnapshot, VmOptions};
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let vm1 = InnerVm::new(VmOptions::default())?;
    /// let snapshot = vm1.snapshot()?;
    /// let vm2 = InnerVm::from_snapshot(snapshot, None)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_snapshot(
        _snapshot: VmSnapshot,
        _options: Option<VmOptions>,
    ) -> Result<Self, VmError> {
        // TODO: Implement snapshot restoration
        Err(VmError::ExecutionError(
            "Snapshot restore not yet implemented".to_string(),
        ))
    }

    /// Load a .rbin file into this VM
    ///
    /// # Arguments
    /// * `path` - Path to the .rbin file
    ///
    /// # Returns
    /// * `Ok(())` - Successfully loaded
    /// * `Err(VmError)` - Failed to load
    ///
    /// # Example
    /// ```no_run
    /// # use raya_core::vm::{InnerVm, VmOptions};
    /// # use std::path::Path;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let vm = InnerVm::new(VmOptions::default())?;
    /// vm.load_rbin(Path::new("./mymodule.rbin"))?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_rbin(&self, path: &Path) -> Result<(), VmError> {
        let bytes = std::fs::read(path)?;
        self.load_rbin_bytes(&bytes)
    }

    /// Load a .rbin from bytes
    ///
    /// # Arguments
    /// * `bytes` - Raw .rbin file contents
    ///
    /// # Returns
    /// * `Ok(())` - Successfully loaded
    /// * `Err(VmError)` - Failed to load
    ///
    /// # Example
    /// ```no_run
    /// # use raya_core::vm::{InnerVm, VmOptions};
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let vm = InnerVm::new(VmOptions::default())?;
    /// let bytes = std::fs::read("./mymodule.rbin")?;
    /// vm.load_rbin_bytes(&bytes)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_rbin_bytes(&self, bytes: &[u8]) -> Result<(), VmError> {
        use sha2::{Digest, Sha256};

        // Parse the .rbin format
        let module = Module::decode(bytes)
            .map_err(|e| VmError::InvalidBinaryFormat(format!("Failed to parse .rbin: {:?}", e)))?;

        // Verify magic number
        if &module.magic != b"RAYA" {
            return Err(VmError::InvalidBinaryFormat(
                "Invalid magic number (expected 'RAYA')".to_string(),
            ));
        }

        // Compute checksum of the payload (excluding header)
        // The checksum in the module was computed during encoding
        // We need to verify it matches
        let payload_start = 48; // Header size: magic(4) + version(4) + flags(4) + crc32(4) + sha256(32)
        if bytes.len() < payload_start {
            return Err(VmError::InvalidBinaryFormat(
                "File too small to contain valid header".to_string(),
            ));
        }

        let payload = &bytes[payload_start..];
        let hash = Sha256::digest(payload);
        let computed_checksum: [u8; 32] = hash.into();

        // Verify checksum
        if module.checksum != computed_checksum {
            return Err(VmError::ChecksumMismatch {
                expected: hex::encode(module.checksum),
                actual: hex::encode(computed_checksum),
            });
        }

        // Get write access to the context
        let mut context = self.context.write();

        // Register the module
        context
            .register_module(Arc::new(module))
            .map_err(|e| VmError::ExecutionError(format!("Failed to register module: {}", e)))?;

        Ok(())
    }

    /// Load raw bytecode (legacy support)
    pub fn load_bytecode(&self, bytecode: &[u8]) -> Result<(), VmError> {
        self.load_rbin_bytes(bytecode)
    }

    /// Run an entry point function
    ///
    /// # Arguments
    /// * `_name` - Name of the function to execute (e.g., "main")
    /// * `_args` - Arguments to pass to the function
    ///
    /// # Returns
    /// * `Ok(TaskId)` - Task ID for the spawned execution
    /// * `Err(VmError)` - Failed to start execution
    ///
    /// # Example
    /// ```no_run
    /// # use raya_core::vm::{InnerVm, VmOptions};
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let vm = InnerVm::new(VmOptions::default())?;
    /// let task_id = vm.run_entry("main", vec![])?;
    /// // Wait for task to complete...
    /// # Ok(())
    /// # }
    /// ```
    pub fn run_entry(&self, _name: &str, _args: Vec<Value>) -> Result<TaskId, VmError> {
        // TODO: Implement entry point execution
        // - Look up function in function table
        // - Create a new task
        // - Register task with context
        // - Spawn task on scheduler
        // - Return task ID

        Err(VmError::ExecutionError(
            "Entry point execution not yet implemented".to_string(),
        ))
    }

    /// Terminate this VM and clean up resources
    ///
    /// This:
    /// - Terminates all running tasks
    /// - Releases heap memory
    /// - Unregisters the context
    ///
    /// # Returns
    /// * `Ok(())` - Successfully terminated
    /// * `Err(VmError)` - Failed to terminate
    ///
    /// # Example
    /// ```
    /// # use raya_core::vm::{InnerVm, VmOptions};
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let vm = InnerVm::new(VmOptions::default())?;
    /// vm.terminate()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn terminate(&self) -> Result<(), VmError> {
        // Get write access to terminate tasks
        let mut _context = self.context.write();

        // TODO: Terminate all tasks owned by this context
        // TODO: Trigger garbage collection to free memory

        Ok(())
    }

    /// Get statistics for this VM
    ///
    /// # Returns
    /// * `Ok(VmStats)` - Current statistics
    /// * `Err(VmError)` - Failed to get stats
    ///
    /// # Example
    /// ```
    /// # use raya_core::vm::{InnerVm, VmOptions};
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let vm = InnerVm::new(VmOptions::default())?;
    /// let stats = vm.get_stats()?;
    /// println!("Heap: {} bytes", stats.heap_bytes_used);
    /// # Ok(())
    /// # }
    /// ```
    pub fn get_stats(&self) -> Result<VmStats, VmError> {
        let context = self.context.read();
        let limits = context.limits();
        let counters = context.counters();
        let heap_stats = context.heap_stats();

        Ok(VmStats {
            heap_bytes_used: heap_stats.allocated_bytes,
            max_heap_bytes: limits.max_heap_bytes.unwrap_or(0),
            tasks: counters.active_tasks(),
            max_tasks: limits.max_tasks.unwrap_or(0),
            steps_executed: counters.total_steps(),
        })
    }

    /// Snapshot this VM's complete state
    ///
    /// # Returns
    /// * `Ok(VmSnapshot)` - Snapshot of current state
    /// * `Err(VmError)` - Failed to snapshot
    ///
    /// # Example
    /// ```no_run
    /// # use raya_core::vm::{InnerVm, VmOptions};
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let vm = InnerVm::new(VmOptions::default())?;
    /// let snapshot = vm.snapshot()?;
    /// // Later... restore from snapshot
    /// # Ok(())
    /// # }
    /// ```
    pub fn snapshot(&self) -> Result<VmSnapshot, VmError> {
        // TODO: Implement VM snapshotting
        // - Pause all tasks
        // - Snapshot heap
        // - Snapshot task states
        // - Snapshot globals

        Err(VmError::ExecutionError(
            "Snapshot not yet implemented".to_string(),
        ))
    }

    /// Restore VM state from a snapshot
    ///
    /// This replaces the current state with the snapshotted state.
    ///
    /// # Arguments
    /// * `_snapshot` - The snapshot to restore
    ///
    /// # Returns
    /// * `Ok(())` - Successfully restored
    /// * `Err(VmError)` - Failed to restore
    ///
    /// # Example
    /// ```no_run
    /// # use raya_core::vm::{InnerVm, VmOptions};
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let mut vm = InnerVm::new(VmOptions::default())?;
    /// let snapshot = vm.snapshot()?;
    /// // ... later ...
    /// vm.restore(snapshot)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn restore(&mut self, _snapshot: VmSnapshot) -> Result<(), VmError> {
        // TODO: Implement VM restoration
        Err(VmError::ExecutionError(
            "Restore not yet implemented".to_string(),
        ))
    }

    /// Get the context ID
    pub fn context_id(&self) -> VmContextId {
        self.context.read().id()
    }
}

/// VM snapshot containing complete VM state
///
/// Snapshots can be used to:
/// - Save/restore VM state
/// - Migrate VMs across hosts
/// - Create checkpoints
/// - Implement time-travel debugging
#[derive(Debug, Clone)]
pub struct VmSnapshot {
    /// Snapshot of the VmContext
    context: ContextSnapshot,
}

/// Snapshot of a VmContext
#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    /// Context ID
    pub id: VmContextId,

    /// Serialized heap data
    pub heap_data: Vec<u8>,

    /// Global variables
    pub globals: Vec<(String, Value)>,

    /// Task states
    pub tasks: Vec<TaskSnapshot>,
}

/// Snapshot of a single task
#[derive(Debug, Clone)]
pub struct TaskSnapshot {
    /// Task ID
    pub id: TaskId,

    /// Stack frames
    pub frames: Vec<FrameSnapshot>,
}

/// Snapshot of a stack frame
#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    /// Function ID
    pub function_id: u32,

    /// Program counter
    pub pc: usize,

    /// Local variables
    pub locals: Vec<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::ResourceLimits;

    #[test]
    fn test_vm_creation() {
        let _vm = Vm::new(VmOptions::default()).unwrap();
        // VM owns its context directly
    }

    #[test]
    fn test_vm_with_limits() {
        let options = VmOptions {
            limits: ResourceLimits::with_heap_limit(1024 * 1024),
            ..Default::default()
        };

        let vm = Vm::new(options).unwrap();
        let stats = vm.get_stats().unwrap();

        assert_eq!(stats.max_heap_bytes, 1024 * 1024);
    }

    #[test]
    fn test_vm_get_stats() {
        let vm = Vm::new(VmOptions::default()).unwrap();
        let stats = vm.get_stats().unwrap();

        assert_eq!(stats.heap_bytes_used, 0);
        assert_eq!(stats.tasks, 0);
        assert_eq!(stats.steps_executed, 0);
    }

    #[test]
    fn test_vm_terminate() {
        let vm = Vm::new(VmOptions::default()).unwrap();
        let _context_id = vm.context_id();

        vm.terminate().unwrap();

        // VM owns its context directly, no registry to check
        // Verify terminate succeeds without errors
    }

    #[test]
    fn test_load_rbin_invalid_bytes() {
        let vm = Vm::new(VmOptions::default()).unwrap();
        let result = vm.load_rbin_bytes(&[0, 1, 2, 3]);

        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_vms() {
        let vm1 = Vm::new(VmOptions::default()).unwrap();
        let vm2 = Vm::new(VmOptions::default()).unwrap();

        assert_ne!(vm1.context_id(), vm2.context_id());

        let stats1 = vm1.get_stats().unwrap();
        let stats2 = vm2.get_stats().unwrap();

        assert_eq!(stats1.heap_bytes_used, 0);
        assert_eq!(stats2.heap_bytes_used, 0);
    }
}
