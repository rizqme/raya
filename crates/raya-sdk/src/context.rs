//! NativeContext trait — abstract VM operations
//!
//! Defines the interface that the Raya engine implements. Native modules
//! (including stdlib) program against this trait without depending on
//! engine internals.

use crate::error::AbiResult;
use crate::value::NativeValue;

/// Information about a class in the VM
#[derive(Debug, Clone)]
pub struct ClassInfo {
    /// Class ID in the registry
    pub class_id: usize,
    /// Number of instance fields
    pub field_count: usize,
    /// Class name
    pub name: String,
    /// Parent class ID (if any)
    pub parent_id: Option<usize>,
    /// Constructor function ID (if any)
    pub constructor_id: Option<usize>,
    /// Number of methods in vtable
    pub method_count: usize,
}

/// Abstract VM context for native handlers.
///
/// This trait is the single entry point for all ABI operations. The Raya engine
/// provides the concrete implementation (`EngineContext`). Native modules only
/// see this trait — they never depend on engine internals.
///
/// # Performance
///
/// Dynamic dispatch (`&dyn NativeContext`) adds ~1-2ns per call. This is
/// negligible because every method does substantial work (GC allocation,
/// registry lookup, mutex/condvar waits, etc.).
pub trait NativeContext {
    // ========================================================================
    // Value Creation
    // ========================================================================

    /// Allocate a new string on the GC heap
    fn create_string(&self, s: &str) -> NativeValue;

    /// Allocate a new buffer on the GC heap
    fn create_buffer(&self, data: &[u8]) -> NativeValue;

    /// Allocate a new array on the GC heap
    fn create_array(&self, items: &[NativeValue]) -> NativeValue;

    /// Allocate a new object instance by class ID
    fn create_object_by_id(&self, class_id: usize) -> AbiResult<NativeValue>;

    // ========================================================================
    // Value Reading
    // ========================================================================

    /// Read string data from a string value
    fn read_string(&self, val: NativeValue) -> AbiResult<String>;

    /// Read bytes from a buffer value
    fn read_buffer(&self, val: NativeValue) -> AbiResult<Vec<u8>>;

    // ========================================================================
    // Array Operations
    // ========================================================================

    /// Get array length
    fn array_len(&self, val: NativeValue) -> AbiResult<usize>;

    /// Get array element at index
    fn array_get(&self, val: NativeValue, index: usize) -> AbiResult<NativeValue>;

    // ========================================================================
    // Object Operations
    // ========================================================================

    /// Get object field by index
    fn object_get_field(&self, val: NativeValue, index: usize) -> AbiResult<NativeValue>;

    /// Set object field by index
    fn object_set_field(
        &self,
        val: NativeValue,
        index: usize,
        value: NativeValue,
    ) -> AbiResult<()>;

    /// Get object's class ID
    fn object_class_id(&self, val: NativeValue) -> AbiResult<usize>;

    // ========================================================================
    // Class Operations
    // ========================================================================

    /// Get class info by ID
    fn class_info(&self, class_id: usize) -> AbiResult<ClassInfo>;

    /// Find class by name (searches exported classes)
    fn class_by_name(&self, name: &str) -> AbiResult<ClassInfo>;

    /// Get class field names and their indices
    fn class_field_names(&self, class_id: usize) -> AbiResult<Vec<(String, usize)>>;

    /// Get class method names and their vtable indices
    fn class_method_entries(&self, class_id: usize) -> AbiResult<Vec<(String, usize)>>;

    // ========================================================================
    // Task Operations
    // ========================================================================

    /// Get current task ID
    fn current_task_id(&self) -> u64;

    /// Spawn a new task running the given function
    fn spawn_function(&self, func_id: usize, args: &[NativeValue]) -> AbiResult<u64>;

    /// Block until task completes and return its result
    fn await_task(&self, task_id: u64) -> AbiResult<NativeValue>;

    /// Check if a task is done (non-blocking)
    fn task_is_done(&self, task_id: u64) -> bool;

    /// Cancel a task
    fn task_cancel(&self, task_id: u64);

    // ========================================================================
    // Function Execution
    // ========================================================================

    /// Call a function by ID (synchronous — blocks until complete)
    fn call_function(&self, func_id: usize, args: &[NativeValue]) -> AbiResult<NativeValue>;

    /// Call a method on an object (synchronous)
    fn call_method(
        &self,
        receiver: NativeValue,
        class_id: usize,
        method_name: &str,
        args: &[NativeValue],
    ) -> AbiResult<NativeValue>;

    // ========================================================================
    // Channel Operations
    // ========================================================================

    /// Send a value to a channel (blocking). Returns false if channel is closed.
    fn channel_send(&self, channel: NativeValue, value: NativeValue) -> AbiResult<bool>;

    /// Receive a value from a channel (blocking). Returns None if channel is closed.
    fn channel_receive(&self, channel: NativeValue) -> AbiResult<Option<NativeValue>>;

    /// Try to receive a value from a channel (non-blocking)
    fn channel_try_receive(&self, channel: NativeValue) -> Option<NativeValue>;

    /// Try to send a value to a channel (non-blocking)
    fn channel_try_send(&self, channel: NativeValue, value: NativeValue) -> bool;

    /// Close a channel
    fn channel_close(&self, channel: NativeValue);

    /// Check if a channel is closed
    fn channel_is_closed(&self, channel: NativeValue) -> bool;
}
