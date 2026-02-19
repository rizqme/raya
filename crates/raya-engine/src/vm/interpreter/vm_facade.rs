//! Synchronous VM facade for testing and simple execution


use super::SafepointCoordinator;
use crate::vm::{
    object::{Object, RayaString},
    scheduler::{Scheduler, Task, TaskState},
    snapshot::{SnapshotReader, SnapshotWriter},
    value::Value,
    VmError, VmResult,
};
use crate::compiler::{Module, Opcode};
use std::path::Path;
use std::sync::Arc;

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

/// Raya virtual machine
pub struct Vm {
    /// Task scheduler (owns SharedVmState — the canonical runtime state)
    scheduler: Scheduler,
    /// JIT engine for pre-warming and native code compilation
    #[cfg(feature = "jit")]
    jit_engine: Option<crate::jit::JitEngine>,
}

impl Vm {
    /// Create a new VM with default worker count
    pub fn new() -> Self {
        let worker_count = num_cpus::get();
        Self::with_worker_count(worker_count)
    }

    /// Create a new VM with specified worker count
    pub fn with_worker_count(worker_count: usize) -> Self {
        let mut scheduler = Scheduler::new(worker_count);
        scheduler.start();

        Self {
            scheduler,
            #[cfg(feature = "jit")]
            jit_engine: None,
        }
    }

    /// Create a new VM with specified worker count and native handler
    pub fn with_native_handler(worker_count: usize, native_handler: std::sync::Arc<dyn crate::vm::NativeHandler>) -> Self {
        let mut scheduler = Scheduler::with_native_handler(worker_count, native_handler);
        scheduler.start();

        Self {
            scheduler,
            #[cfg(feature = "jit")]
            jit_engine: None,
        }
    }

    /// Create a new VM with specified scheduler limits
    pub fn with_scheduler_limits(worker_count: usize, limits: crate::vm::scheduler::SchedulerLimits) -> Self {
        let mut scheduler = Scheduler::with_limits(worker_count, limits);
        scheduler.start();

        Self {
            scheduler,
            #[cfg(feature = "jit")]
            jit_engine: None,
        }
    }

    /// Create a new VM from VmOptions (resource limits, capabilities, etc.)
    pub fn with_options(options: super::VmOptions) -> Self {
        let limits = crate::vm::scheduler::SchedulerLimits {
            max_heap_size: options.limits.max_heap_bytes,
            max_concurrent_tasks: options.limits.max_tasks,
            max_preemptions: options.limits.max_preemptions,
            preempt_threshold_ms: options.limits.preempt_threshold_ms,
            ..Default::default()
        };
        Self::with_scheduler_limits(1, limits)
    }

    /// Get the scheduler
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Get mutable scheduler
    pub fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    /// Get the native function registry for registering native handlers
    pub fn native_registry(&self) -> &parking_lot::RwLock<crate::vm::NativeFunctionRegistry> {
        &self.scheduler.shared_state().native_registry
    }

    /// Get the safepoint coordinator
    pub fn safepoint(&self) -> &Arc<SafepointCoordinator> {
        self.scheduler.safepoint()
    }

    /// Get the shared VM state
    pub fn shared_state(&self) -> &super::SharedVmState {
        self.scheduler.shared_state()
    }

    /// Load a .ryb file into this VM
    ///
    /// Reads the file and delegates to `load_rbin_bytes`.
    pub fn load_rbin(&mut self, path: &Path) -> VmResult<()> {
        let bytes = std::fs::read(path)
            .map_err(|e| VmError::IoError(format!("{}: {}", path.display(), e)))?;
        self.load_rbin_bytes(&bytes)
    }

    /// Load a .ryb module from bytes
    ///
    /// Decodes the binary module (verifying magic, version, checksums),
    /// then registers it in the shared module registry along with its
    /// classes and native function table.
    pub fn load_rbin_bytes(&mut self, bytes: &[u8]) -> VmResult<()> {
        let module = Module::decode(bytes)
            .map_err(|e| VmError::InvalidBinaryFormat(format!("{}", e)))?;

        self.scheduler
            .shared_state()
            .register_module(Arc::new(module))
            .map_err(|e| VmError::RuntimeError(e))?;

        Ok(())
    }

    /// Get statistics for this VM
    pub fn get_stats(&self) -> VmStats {
        let gc = self.scheduler.shared_state().gc.lock();
        let heap_stats = gc.heap_stats();
        let task_count = self.scheduler.shared_state().tasks.read().len();
        drop(gc);

        VmStats {
            heap_bytes_used: heap_stats.allocated_bytes,
            max_heap_bytes: 0, // Limit is per-context, not directly accessible here
            tasks: task_count,
            max_tasks: 0,
            steps_executed: 0,
        }
    }

    /// Terminate this VM and shut down the scheduler
    pub fn terminate(&mut self) {
        self.scheduler.shutdown();
    }

    /// Register a class with the VM's shared class registry
    pub fn register_class(&self, class: crate::vm::object::Class) {
        self.scheduler.shared_state().classes.write().register_class(class);
    }

    /// Trigger garbage collection on the shared GC
    pub fn collect_garbage(&mut self) {
        let mut gc = self.scheduler.shared_state().gc.lock();
        gc.collect();
    }

    /// Enable JIT compilation with default configuration.
    ///
    /// When enabled, `execute()` will pre-warm CPU-intensive functions at module load time
    /// and the interpreter will dispatch to native code for compiled functions.
    #[cfg(feature = "jit")]
    pub fn enable_jit(&mut self) -> Result<(), String> {
        let engine = crate::jit::JitEngine::new()
            .map_err(|e| format!("Failed to initialize JIT: {}", e))?;
        *self.scheduler.shared_state().code_cache.lock() = Some(engine.code_cache().clone());
        self.jit_engine = Some(engine);
        Ok(())
    }

    /// Enable JIT compilation with custom configuration.
    #[cfg(feature = "jit")]
    pub fn enable_jit_with_config(&mut self, config: crate::jit::JitConfig) -> Result<(), String> {
        let engine = crate::jit::JitEngine::with_config(config)
            .map_err(|e| format!("Failed to initialize JIT: {}", e))?;
        *self.scheduler.shared_state().code_cache.lock() = Some(engine.code_cache().clone());
        self.jit_engine = Some(engine);
        Ok(())
    }

    /// Execute a module using the task scheduler
    ///
    /// This method runs the main function as a task, enabling full cooperative
    /// scheduling with proper suspension for await, sleep, mutex, and channel operations.
    pub fn execute(&mut self, module: &Module) -> VmResult<Value> {
        // Validate module
        module.validate().map_err(|e| VmError::RuntimeError(e))?;

        // Register module: classes, native linkage, and module registry
        self.scheduler.shared_state().register_module(Arc::new(module.clone()))
            .map_err(|e| VmError::RuntimeError(e))?;

        // JIT pre-warming: analyze, compile, and cache CPU-intensive functions
        #[cfg(feature = "jit")]
        if let Some(ref mut jit_engine) = self.jit_engine {
            let _summary = jit_engine.prewarm(module);
        }

        // Find main function
        let main_fn_id = module
            .functions
            .iter()
            .position(|f| f.name == "main")
            .ok_or_else(|| VmError::RuntimeError("No main function".to_string()))?;

        // Create main task
        let main_task = Arc::new(Task::new(main_fn_id, Arc::new(module.clone()), None));
        let _task_id = main_task.id();

        // Spawn main task
        if self.scheduler.spawn(main_task.clone()).is_none() {
            return Err(VmError::RuntimeError("Failed to spawn main task".to_string()));
        }

        // Block until main task completes using condvar (no busy-waiting)
        let final_state = main_task.wait_completion();

        match final_state {
            TaskState::Completed => {
                Ok(main_task.result().unwrap_or(Value::null()))
            }
            TaskState::Failed => {
                let msg = Self::extract_exception_message(&main_task);
                Err(VmError::RuntimeError(msg))
            }
            other => {
                Err(VmError::RuntimeError(format!(
                    "Main task ended in unexpected state: {:?}", other
                )))
            }
        }
    }

    /// Extract a human-readable error message from a failed task's exception
    fn extract_exception_message(task: &Task) -> String {
        let Some(exc) = task.current_exception() else {
            return "Main task failed".to_string();
        };

        if exc.is_null() {
            return "Main task failed".to_string();
        }

        if !exc.is_ptr() {
            return format!("Main task failed: {:?}", exc);
        }

        // Try string
        if let Some(s) = unsafe { exc.as_ptr::<RayaString>() } {
            return format!("Main task failed: {}", unsafe { &*s.as_ptr() }.data);
        }

        // Try Error object (message is field 0)
        if let Some(obj) = unsafe { exc.as_ptr::<Object>() } {
            if let Some(msg_val) = unsafe { &*obj.as_ptr() }.get_field(0) {
                if msg_val.is_ptr() {
                    if let Some(s) = unsafe { msg_val.as_ptr::<RayaString>() } {
                        return format!("Main task failed: {}", unsafe { &*s.as_ptr() }.data);
                    }
                }
            }
        }

        "Main task failed".to_string()
    }

    // =========================================================================
    // Snapshot / Restore
    // =========================================================================

    /// Capture a snapshot of the VM state and write it to a file.
    ///
    /// Must be called when no tasks are actively executing (e.g., before `execute()`
    /// or after it returns). All registered tasks are serialized along with the heap.
    pub fn snapshot_to_file(&self, path: &Path) -> VmResult<()> {
        let mut writer = self.build_snapshot()?;
        writer
            .write_to_file(path)
            .map_err(|e| VmError::IoError(format!("{}", e)))?;
        Ok(())
    }

    /// Capture a snapshot of the VM state and write it to a byte buffer.
    pub fn snapshot_to_bytes(&self) -> VmResult<Vec<u8>> {
        let writer = self.build_snapshot()?;
        let mut buf = Vec::new();
        writer
            .write_snapshot(&mut buf)
            .map_err(|e| VmError::IoError(format!("{}", e)))?;
        Ok(buf)
    }

    /// Build a SnapshotWriter from the current VM state.
    fn build_snapshot(&self) -> VmResult<SnapshotWriter> {
        let mut writer = SnapshotWriter::new();

        // Serialize all tasks
        let tasks = self.scheduler.shared_state().tasks.read();
        for task in tasks.values() {
            writer.add_task(task.to_serialized());
        }

        // Heap snapshot (placeholder — full heap serialization is future work)

        Ok(writer)
    }

    /// Restore VM state from a snapshot file.
    ///
    /// The modules referenced by snapshot tasks must already be loaded
    /// (via `load_rbin` / `load_rbin_bytes`) before calling restore.
    /// Tasks are reconstructed and re-inserted into the scheduler's task map.
    pub fn restore_from_file(&mut self, path: &Path) -> VmResult<()> {
        let reader = SnapshotReader::from_file(path)
            .map_err(|e| VmError::IoError(format!("{}", e)))?;
        self.apply_snapshot(reader)
    }

    /// Restore VM state from snapshot bytes.
    pub fn restore_from_bytes(&mut self, bytes: &[u8]) -> VmResult<()> {
        let reader = SnapshotReader::from_reader(&mut &bytes[..])
            .map_err(|e| VmError::IoError(format!("{}", e)))?;
        self.apply_snapshot(reader)
    }

    /// Apply a parsed snapshot to this VM.
    fn apply_snapshot(&mut self, reader: SnapshotReader) -> VmResult<()> {
        let shared = self.scheduler.shared_state();

        // Restore tasks: look up each task's module from the registry
        let serialized_tasks = reader.tasks();
        let mut tasks_map = shared.tasks.write();

        for stask in serialized_tasks {
            // Resolve the module for this task. The module must have been loaded
            // beforehand. We use function_index == 0 heuristic: use first registered module.
            // TODO: when snapshot includes module name/checksum per task, look up precisely.
            let module = shared
                .module_registry
                .read()
                .all_modules()
                .first()
                .cloned()
                .ok_or_else(|| {
                    VmError::RuntimeError(
                        "No modules loaded — load modules before restoring snapshot".to_string(),
                    )
                })?;

            let task = Arc::new(Task::from_serialized(stask.clone(), module));
            tasks_map.insert(task.id(), task);
        }

        // Heap restoration is future work — heap objects are not yet serialized

        Ok(())
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Function;

    #[test]
    fn test_vm_creation() {
        let _vm = Vm::new();
    }

    #[test]
    fn test_const_null() {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::null());
    }

    #[test]
    fn test_const_true() {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstTrue as u8, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_const_false() {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstFalse as u8, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(false));
    }

    #[test]
    fn test_const_i32() {
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_simple_arithmetic() {
        // 10 + 20 = 30
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                20,
                0,
                0,
                0,
                Opcode::Iadd as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(30));
    }

    #[test]
    fn test_arithmetic_subtraction() {
        // 100 - 25 = 75
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                100,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                25,
                0,
                0,
                0,
                Opcode::Isub as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(75));
    }

    #[test]
    fn test_arithmetic_multiplication() {
        // 6 * 7 = 42
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                6,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                7,
                0,
                0,
                0,
                Opcode::Imul as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(42));
    }

    #[test]
    fn test_arithmetic_division() {
        // 100 / 5 = 20
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                100,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                5,
                0,
                0,
                0,
                Opcode::Idiv as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(20));
    }

    #[test]
    fn test_division_by_zero() {
        // 10 / 0 should error
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                0,
                0,
                0,
                0,
                Opcode::Idiv as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VmError::RuntimeError(_)));
    }

    #[test]
    fn test_stack_operations() {
        // Test DUP: push 42, dup, add
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::Dup as u8,
                Opcode::Iadd as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(84));
    }

    #[test]
    fn test_local_variables() {
        // local x = 42
        // local y = 10
        // return x + y
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 2,
            code: vec![
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::StoreLocal as u8,
                0, 0, // u16 index 0
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::StoreLocal as u8,
                1, 0, // u16 index 1
                Opcode::LoadLocal as u8,
                0, 0, // u16 index 0
                Opcode::LoadLocal as u8,
                1, 0, // u16 index 1
                Opcode::Iadd as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(52));
    }

    #[test]
    fn test_comparison_equal() {
        // 42 == 42
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::Ieq as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_comparison_not_equal() {
        // 42 != 10
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::Ine as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_comparison_less_than() {
        // 5 < 10
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                5,
                0,
                0,
                0,
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0,
                Opcode::Ilt as u8,
                Opcode::Return as u8,
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::bool(true));
    }

    #[test]
    fn test_conditional_branch() {
        // if (10 > 5) { return 1 } else { return 0 }
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::ConstI32 as u8,
                10,
                0,
                0,
                0, // offset 0-4
                Opcode::ConstI32 as u8,
                5,
                0,
                0,
                0,                 // offset 5-9
                Opcode::Igt as u8, // offset 10
                Opcode::JmpIfFalse as u8,
                8,
                0, // offset 11-13, jump +8 to offset 21
                Opcode::ConstI32 as u8,
                1,
                0,
                0,
                0,                    // offset 14-18 (then branch)
                Opcode::Return as u8, // offset 19
                // else branch starts at offset 20
                Opcode::ConstI32 as u8,
                0,
                0,
                0,
                0,                    // offset 20-24
                Opcode::Return as u8, // offset 25
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(1));
    }

    #[test]
    fn test_unconditional_jump() {
        // Jump over some code
        // After JMP instruction (offset 0), IP is at 1
        // After reading i16 offset (2 bytes), IP is at 3
        // Jump offset of +5 makes IP = 3 + 5 = 8 (start of second CONST_I32)
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![
                Opcode::Jmp as u8,
                5,
                0, // offset 0-2, jump +5 to offset 8
                Opcode::ConstI32 as u8,
                99,
                0,
                0,
                0, // offset 3-7 (skipped)
                Opcode::ConstI32 as u8,
                42,
                0,
                0,
                0,                    // offset 8-12
                Opcode::Return as u8, // offset 13
            ],
        });

        let mut vm = Vm::new();
        let result = vm.execute(&module).unwrap();
        assert_eq!(result, Value::i32(42));
    }

    // =========================================================================
    // Snapshot / Restore tests
    // =========================================================================

    #[test]
    fn test_snapshot_empty_vm() {
        let vm = Vm::new();
        let bytes = vm.snapshot_to_bytes().unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_snapshot_after_execution() {
        // Execute a module, then snapshot — completed tasks should be captured
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8],
        });

        let mut vm = Vm::new();
        let _result = vm.execute(&module).unwrap();

        // Snapshot should succeed even after execution
        let bytes = vm.snapshot_to_bytes().unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_snapshot_round_trip_bytes() {
        use crate::vm::scheduler::{Task, TaskId};

        // Create a VM and manually insert a task
        let vm = Vm::new();

        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "test_fn".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::Return as u8],
        });
        let module = Arc::new(module);

        // Register the module
        vm.shared_state()
            .register_module(module.clone())
            .unwrap();

        // Manually add a task to the task registry
        let task = Arc::new(Task::new(0, module.clone(), None));
        let task_id = task.id();
        task.set_ip(42);
        task.stack().lock().unwrap().push(Value::i32(100)).unwrap();
        vm.shared_state().tasks.write().insert(task_id, task);

        // Snapshot
        let bytes = vm.snapshot_to_bytes().unwrap();

        // Restore into a fresh VM
        let mut vm2 = Vm::new();
        vm2.shared_state()
            .register_module(module.clone())
            .unwrap();
        vm2.restore_from_bytes(&bytes).unwrap();

        // Verify the task was restored
        let tasks = vm2.shared_state().tasks.read();
        assert_eq!(tasks.len(), 1);
        let restored = tasks.values().next().unwrap();
        assert_eq!(restored.id().as_u64(), task_id.as_u64());
        assert_eq!(restored.ip(), 42);
        assert_eq!(
            restored.stack().lock().unwrap().as_slice(),
            &[Value::i32(100)]
        );
    }

    #[test]
    fn test_snapshot_round_trip_file() {
        use crate::vm::scheduler::Task;

        let vm = Vm::new();

        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "test_fn".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::Return as u8],
        });
        let module = Arc::new(module);

        vm.shared_state()
            .register_module(module.clone())
            .unwrap();

        let task = Arc::new(Task::new(0, module.clone(), None));
        let task_id = task.id();
        vm.shared_state().tasks.write().insert(task_id, task);

        // Snapshot to temp file
        let dir = std::env::temp_dir();
        let path = dir.join("raya_test_snapshot.snap");

        vm.snapshot_to_file(&path).unwrap();

        // Restore
        let mut vm2 = Vm::new();
        vm2.shared_state()
            .register_module(module.clone())
            .unwrap();
        vm2.restore_from_file(&path).unwrap();

        let tasks = vm2.shared_state().tasks.read();
        assert_eq!(tasks.len(), 1);

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_restore_requires_modules() {
        // Snapshot an empty VM
        let vm = Vm::new();
        let bytes = vm.snapshot_to_bytes().unwrap();

        // Now create a snapshot with tasks by manually creating one
        use crate::vm::scheduler::Task;

        let vm_with_task = Vm::new();
        let mut module = Module::new("test".to_string());
        module.functions.push(Function {
            name: "test_fn".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::Return as u8],
        });
        let module = Arc::new(module);
        vm_with_task
            .shared_state()
            .register_module(module.clone())
            .unwrap();
        let task = Arc::new(Task::new(0, module.clone(), None));
        vm_with_task
            .shared_state()
            .tasks
            .write()
            .insert(task.id(), task);

        let bytes_with_task = vm_with_task.snapshot_to_bytes().unwrap();

        // Try restoring tasks without loading modules → should fail
        let mut vm_empty = Vm::new();
        let result = vm_empty.restore_from_bytes(&bytes_with_task);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No modules loaded"));

        // Empty snapshot restore should succeed (no tasks to resolve)
        let mut vm_empty2 = Vm::new();
        let result = vm_empty2.restore_from_bytes(&bytes);
        assert!(result.is_ok());
    }
}
