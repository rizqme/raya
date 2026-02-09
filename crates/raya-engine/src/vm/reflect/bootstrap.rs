//! Bootstrap Context for Dynamic VM
//!
//! Provides minimal runtime environment for dynamic code execution.
//! Part of Phase 17: Dynamic VM Bootstrap.
//!
//! ## Native Call IDs (0x0E20-0x0E2F)
//!
//! | ID     | Method             | Description                    |
//! |--------|--------------------|--------------------------------|
//! | 0x0E20 | bootstrap          | Initialize runtime             |
//! | 0x0E21 | getObjectClass     | Get core Object class ID       |
//! | 0x0E22 | getArrayClass      | Get core Array class ID        |
//! | 0x0E23 | getStringClass     | Get core String class ID       |
//! | 0x0E24 | getTaskClass       | Get core Task class ID         |
//! | 0x0E25 | dynamicPrint       | Print to console               |
//! | 0x0E26 | createDynamicArray | Create array from values       |
//! | 0x0E27 | createDynamicString| Create string value            |
//! | 0x0E28 | isBootstrapped     | Check if context exists        |

use std::sync::atomic::{AtomicBool, Ordering};

use crate::vm::VmError;

/// Well-known class IDs for core types
/// These should match the class IDs in the VM's ClassRegistry
pub mod core_class_ids {
    /// Object class ID (base class)
    pub const OBJECT: usize = 0;
    /// Array class ID (built-in)
    pub const ARRAY: usize = 1;
    /// String class ID (built-in)
    pub const STRING: usize = 2;
    /// Task class ID (built-in)
    pub const TASK: usize = 3;
    /// Map class ID (built-in)
    pub const MAP: usize = 4;
    /// Closure class ID (built-in)
    pub const CLOSURE: usize = 5;
}

/// Native call IDs for bootstrap functions
pub mod bootstrap_native_ids {
    /// Print to console
    pub const PRINT: u16 = 0x0A00;
    /// Logger.info (std:logger)
    pub const LOGGER_INFO: u16 = 0x1001;
}

/// Bootstrap context for dynamic code execution
#[derive(Debug, Clone)]
pub struct BootstrapContext {
    /// Whether the context has been initialized
    pub initialized: bool,

    /// Object class ID
    pub object_class_id: usize,
    /// Array class ID
    pub array_class_id: usize,
    /// String class ID
    pub string_class_id: usize,
    /// Task class ID
    pub task_class_id: usize,
    /// Map class ID
    pub map_class_id: usize,

    /// Print native call ID
    pub print_native_id: u16,
    /// Logger.info native call ID (std:logger)
    pub logger_info_native_id: u16,
}

impl Default for BootstrapContext {
    fn default() -> Self {
        Self::new()
    }
}

impl BootstrapContext {
    /// Create a new bootstrap context with default core class IDs
    pub fn new() -> Self {
        Self {
            initialized: false,
            object_class_id: core_class_ids::OBJECT,
            array_class_id: core_class_ids::ARRAY,
            string_class_id: core_class_ids::STRING,
            task_class_id: core_class_ids::TASK,
            map_class_id: core_class_ids::MAP,
            print_native_id: bootstrap_native_ids::PRINT,
            logger_info_native_id: bootstrap_native_ids::LOGGER_INFO,
        }
    }

    /// Initialize the bootstrap context
    pub fn initialize(&mut self) -> Result<(), VmError> {
        if self.initialized {
            return Err(VmError::RuntimeError(
                "Bootstrap context already initialized".to_string(),
            ));
        }
        self.initialized = true;
        Ok(())
    }

    /// Check if the context is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get information about the bootstrap context
    pub fn get_info(&self) -> BootstrapInfo {
        BootstrapInfo {
            initialized: self.initialized,
            object_class_id: self.object_class_id,
            array_class_id: self.array_class_id,
            string_class_id: self.string_class_id,
            task_class_id: self.task_class_id,
            print_native_id: self.print_native_id,
        }
    }
}

/// Bootstrap information for introspection
#[derive(Debug, Clone)]
pub struct BootstrapInfo {
    pub initialized: bool,
    pub object_class_id: usize,
    pub array_class_id: usize,
    pub string_class_id: usize,
    pub task_class_id: usize,
    pub print_native_id: u16,
}

/// Global flag indicating if bootstrap has been called
static BOOTSTRAP_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Check if bootstrap has been initialized globally
pub fn is_bootstrapped() -> bool {
    BOOTSTRAP_INITIALIZED.load(Ordering::Relaxed)
}

/// Mark bootstrap as initialized
pub fn mark_bootstrapped() {
    BOOTSTRAP_INITIALIZED.store(true, Ordering::Relaxed);
}

/// Reset bootstrap state (for testing)
#[cfg(test)]
pub fn reset_bootstrap() {
    BOOTSTRAP_INITIALIZED.store(false, Ordering::Relaxed);
}

/// Execution options for dynamic code
#[derive(Debug, Clone, Default)]
pub struct ExecutionOptions {
    /// Maximum stack depth
    pub max_stack_depth: Option<usize>,
    /// Maximum instruction count (for timeout)
    pub max_instructions: Option<usize>,
    /// Whether to allow native calls
    pub allow_native_calls: bool,
    /// Whether to allow spawning tasks
    pub allow_spawn: bool,
}

impl ExecutionOptions {
    /// Create default execution options
    pub fn new() -> Self {
        Self {
            max_stack_depth: Some(1024),
            max_instructions: None,
            allow_native_calls: true,
            allow_spawn: true,
        }
    }

    /// Create restricted options for sandboxed execution
    pub fn sandboxed() -> Self {
        Self {
            max_stack_depth: Some(256),
            max_instructions: Some(100_000),
            allow_native_calls: false,
            allow_spawn: false,
        }
    }

    /// Allow all operations
    pub fn unrestricted() -> Self {
        Self {
            max_stack_depth: None,
            max_instructions: None,
            allow_native_calls: true,
            allow_spawn: true,
        }
    }
}

/// Result of dynamic execution
#[derive(Debug)]
pub enum ExecutionResult {
    /// Execution completed successfully with a value
    Success(crate::vm::value::Value),
    /// Execution yielded (async function)
    Yielded(usize), // Task ID
    /// Execution failed with an error
    Error(VmError),
}

impl ExecutionResult {
    /// Check if execution was successful
    pub fn is_success(&self) -> bool {
        matches!(self, ExecutionResult::Success(_))
    }

    /// Get the value if successful
    pub fn value(self) -> Result<crate::vm::value::Value, VmError> {
        match self {
            ExecutionResult::Success(v) => Ok(v),
            ExecutionResult::Yielded(_) => Err(VmError::RuntimeError(
                "Unexpected yield in synchronous execution".to_string(),
            )),
            ExecutionResult::Error(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_context_new() {
        let ctx = BootstrapContext::new();
        assert!(!ctx.initialized);
        assert_eq!(ctx.object_class_id, core_class_ids::OBJECT);
        assert_eq!(ctx.array_class_id, core_class_ids::ARRAY);
    }

    #[test]
    fn test_bootstrap_context_initialize() {
        let mut ctx = BootstrapContext::new();
        assert!(ctx.initialize().is_ok());
        assert!(ctx.is_initialized());

        // Cannot initialize twice
        assert!(ctx.initialize().is_err());
    }

    #[test]
    fn test_bootstrap_info() {
        let mut ctx = BootstrapContext::new();
        ctx.initialize().unwrap();

        let info = ctx.get_info();
        assert!(info.initialized);
        assert_eq!(info.object_class_id, core_class_ids::OBJECT);
    }

    #[test]
    fn test_execution_options_default() {
        let opts = ExecutionOptions::new();
        assert_eq!(opts.max_stack_depth, Some(1024));
        assert!(opts.allow_native_calls);
        assert!(opts.allow_spawn);
    }

    #[test]
    fn test_execution_options_sandboxed() {
        let opts = ExecutionOptions::sandboxed();
        assert_eq!(opts.max_stack_depth, Some(256));
        assert_eq!(opts.max_instructions, Some(100_000));
        assert!(!opts.allow_native_calls);
        assert!(!opts.allow_spawn);
    }

    #[test]
    fn test_global_bootstrap_flag() {
        reset_bootstrap();
        assert!(!is_bootstrapped());

        mark_bootstrapped();
        assert!(is_bootstrapped());

        reset_bootstrap();
        assert!(!is_bootstrapped());
    }

    #[test]
    fn test_execution_result() {
        let success = ExecutionResult::Success(crate::vm::value::Value::i32(42));
        assert!(success.is_success());
        assert_eq!(success.value().unwrap().as_i32(), Some(42));

        let error = ExecutionResult::Error(VmError::RuntimeError("test".to_string()));
        assert!(!error.is_success());
        assert!(error.value().is_err());
    }
}
