//! Synchronous VM facade for testing and simple execution


use super::{ClassRegistry, SafepointCoordinator};
use crate::vm::{
    builtin,
    gc::GarbageCollector,
    object::{Array, Closure, Object, RayaString},
    scheduler::{ExceptionHandler, Scheduler, Task, TaskId, TaskState},
    stack::Stack,
    sync::MutexRegistry,
    value::Value,
    VmError, VmResult,
};
use crate::compiler::{Module, Opcode};
use std::sync::Arc;

/// Raya virtual machine
pub struct Vm {
    /// Garbage collector
    gc: GarbageCollector,
    /// Operand stack
    stack: Stack,
    /// Global variables (string-keyed)
    globals: rustc_hash::FxHashMap<String, Value>,
    /// Global variables (index-based, for static fields)
    globals_by_index: Vec<Value>,
    /// Class registry
    pub classes: ClassRegistry,
    /// Task scheduler
    scheduler: Scheduler,
    /// Stack of currently executing closures (for LoadCaptured access)
    closure_stack: Vec<Value>,
    /// Exception handler stack (shared across all function calls)
    exception_handlers: Vec<ExceptionHandler>,
    /// Current exception being processed (for propagation detection)
    current_exception: Option<Value>,
    /// Caught exception (for Rethrow - preserved even after catch entry clears current_exception)
    caught_exception: Option<Value>,
    /// Held mutexes for exception unwinding
    held_mutexes: Vec<crate::vm::sync::MutexId>,
    /// Mutex registry for managing all mutexes
    mutex_registry: MutexRegistry,
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
            gc: GarbageCollector::default(),
            stack: Stack::new(),
            globals: rustc_hash::FxHashMap::default(),
            globals_by_index: Vec::new(),
            classes: ClassRegistry::new(),
            scheduler,
            closure_stack: Vec::new(),
            exception_handlers: Vec::new(),
            current_exception: None,
            caught_exception: None,
            held_mutexes: Vec::new(),
            mutex_registry: MutexRegistry::new(),
        }
    }

    /// Create a new VM with specified worker count and native handler
    pub fn with_native_handler(worker_count: usize, native_handler: std::sync::Arc<dyn crate::vm::NativeHandler>) -> Self {
        let mut scheduler = Scheduler::with_native_handler(worker_count, native_handler);
        scheduler.start();

        Self {
            gc: GarbageCollector::default(),
            stack: Stack::new(),
            globals: rustc_hash::FxHashMap::default(),
            globals_by_index: Vec::new(),
            classes: ClassRegistry::new(),
            scheduler,
            closure_stack: Vec::new(),
            exception_handlers: Vec::new(),
            current_exception: None,
            caught_exception: None,
            held_mutexes: Vec::new(),
            mutex_registry: MutexRegistry::new(),
        }
    }

    /// Get the scheduler
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Get mutable scheduler
    pub fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    /// Get the safepoint coordinator
    pub fn safepoint(&self) -> &Arc<SafepointCoordinator> {
        self.scheduler.safepoint()
    }

    /// Collect GC roots from the stack
    fn collect_roots(&mut self) {
        self.gc.clear_stack_roots();

        // Add all values from the operand stack
        for i in 0..self.stack.depth() {
            if let Ok(value) = self.stack.peek_at(i) {
                if value.is_heap_allocated() {
                    self.gc.add_root(value);
                }
            }
        }

        // Add values from all call frames' local variables
        for frame in self.stack.frames() {
            let locals_start = frame.locals_start();
            let locals_count = frame.locals_count();

            for i in 0..locals_count {
                if let Ok(value) = self.stack.peek_at(locals_start + i) {
                    if value.is_heap_allocated() {
                        self.gc.add_root(value);
                    }
                }
            }
        }

        // Add global variables as roots
        for value in self.globals.values() {
            if value.is_heap_allocated() {
                self.gc.add_root(*value);
            }
        }
    }

    /// Trigger garbage collection
    pub fn collect_garbage(&mut self) {
        self.collect_roots();
        self.gc.collect();
    }

    /// Execute a module using the task scheduler
    ///
    /// This method runs the main function as a task, enabling full cooperative
    /// scheduling with proper suspension for await, sleep, mutex, and channel operations.
    pub fn execute(&mut self, module: &Module) -> VmResult<Value> {
        // Validate module
        module.validate().map_err(|e| VmError::RuntimeError(e))?;

        // Copy classes from VM's class registry to shared state (for tests that register classes directly)
        self.scheduler.shared_state().copy_classes_from(&self.classes);

        // Register classes with the shared VM state (from module)
        self.scheduler.shared_state().register_classes(module);

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

        // Wait for main task to complete (with a long timeout)
        let timeout = std::time::Duration::from_secs(3600); // 1 hour timeout
        let start = std::time::Instant::now();

        loop {
            // Check task state
            match main_task.state() {
                TaskState::Completed => {
                    // Return the result
                    return Ok(main_task.result().unwrap_or(Value::null()));
                }
                TaskState::Failed => {
                    // Get the actual exception message if available
                    let msg = if let Some(exc) = main_task.current_exception() {
                        if exc.is_ptr() {
                            // Try to get string content from exception
                            if let Some(s) = unsafe { exc.as_ptr::<RayaString>() } {
                                format!("Main task failed: {}", unsafe { &*s.as_ptr() }.data)
                            } else if let Some(obj) = unsafe { exc.as_ptr::<Object>() } {
                                // Check if it's an Error object with a message field
                                if let Some(msg_val) = unsafe { &*obj.as_ptr() }.get_field(0) {
                                    if msg_val.is_ptr() {
                                        if let Some(s) = unsafe { msg_val.as_ptr::<RayaString>() } {
                                            format!("Main task failed: {}", unsafe { &*s.as_ptr() }.data)
                                        } else {
                                            "Main task failed".to_string()
                                        }
                                    } else {
                                        "Main task failed".to_string()
                                    }
                                } else {
                                    "Main task failed".to_string()
                                }
                            } else {
                                "Main task failed".to_string()
                            }
                        } else if exc.is_null() {
                            "Main task failed".to_string()
                        } else {
                            format!("Main task failed: {:?}", exc)
                        }
                    } else {
                        "Main task failed".to_string()
                    };
                    return Err(VmError::RuntimeError(msg));
                }
                _ => {
                    // Still running, check timeout
                    if start.elapsed() > timeout {
                        return Err(VmError::RuntimeError("Main task timed out".to_string()));
                    }
                    // Brief sleep to avoid busy waiting
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
            }
        }
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

}
