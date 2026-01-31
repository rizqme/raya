//! Shared VM state for concurrent task execution
//!
//! This module provides shared state that can be safely accessed by multiple
//! worker threads executing tasks concurrently.

use crate::vm::gc::GarbageCollector;
use crate::vm::object::{Array, Closure, Object, RayaString};
use crate::vm::scheduler::{ExceptionHandler, Task, TaskId, TaskState, TimerThread};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexRegistry;
use crate::vm::value::Value;
use crate::vm::vm::{ClassRegistry, SafepointCoordinator};
use crate::vm::{VmError, VmResult};
use crossbeam_deque::Injector;
use parking_lot::{Mutex, RwLock};
use crate::compiler::{Module, Opcode};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::time::Duration;

/// Helper to convert Value to f64, handling both f64 and i32 values
#[inline]
fn value_to_f64(v: Value) -> VmResult<f64> {
    if let Some(f) = v.as_f64() {
        Ok(f)
    } else if let Some(i) = v.as_i32() {
        Ok(i as f64)
    } else {
        Err(VmError::TypeError("Expected number".to_string()))
    }
}

/// Shared VM state accessible by all worker threads
///
/// This struct contains all the state that needs to be shared across
/// concurrent task execution. Each field is wrapped in appropriate
/// synchronization primitives for safe concurrent access.
pub struct SharedVmState {
    /// Garbage collector (needs exclusive access for allocation/collection)
    pub gc: Mutex<GarbageCollector>,

    /// Class registry (mostly read, occasionally written during class registration)
    pub classes: RwLock<ClassRegistry>,

    /// Global variables by name
    pub globals: RwLock<FxHashMap<String, Value>>,

    /// Global variables by index (for static fields)
    pub globals_by_index: RwLock<Vec<Value>>,

    /// Safepoint coordinator
    pub safepoint: Arc<SafepointCoordinator>,

    /// Task registry
    pub tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,

    /// Global task injector for scheduling
    pub injector: Arc<Injector<Arc<Task>>>,

    /// Mutex registry for task synchronization
    pub mutex_registry: MutexRegistry,

    /// Timer thread for efficient sleep handling
    pub timer: Arc<TimerThread>,
}

impl SharedVmState {
    /// Create new shared VM state
    pub fn new(
        safepoint: Arc<SafepointCoordinator>,
        tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        injector: Arc<Injector<Arc<Task>>>,
    ) -> Self {
        let timer = TimerThread::new();
        // Start timer thread immediately
        timer.start(injector.clone());

        Self {
            gc: Mutex::new(GarbageCollector::default()),
            classes: RwLock::new(ClassRegistry::new()),
            globals: RwLock::new(FxHashMap::default()),
            globals_by_index: RwLock::new(Vec::new()),
            safepoint,
            tasks,
            injector,
            mutex_registry: MutexRegistry::new(),
            timer,
        }
    }

    /// Register classes from a module
    pub fn register_classes(&self, module: &Module) {
        let mut classes = self.classes.write();
        for (i, class_def) in module.classes.iter().enumerate() {
            let class = if let Some(parent_id) = class_def.parent_id {
                crate::vm::object::Class::with_parent(
                    i,
                    class_def.name.clone(),
                    class_def.field_count,
                    parent_id as usize,
                )
            } else {
                crate::vm::object::Class::new(i, class_def.name.clone(), class_def.field_count)
            };
            classes.register_class(class);
        }
    }

    /// Copy classes from a ClassRegistry (for VM-level class registration)
    pub fn copy_classes_from(&self, source: &ClassRegistry) {
        let mut classes = self.classes.write();
        for (id, class) in source.iter() {
            if classes.get(id).is_none() {
                classes.register_class(class.clone());
            }
        }
    }
}

/// Task executor - executes bytecode for a task using shared VM state
///
/// This is the single execution engine used by all workers. It implements
/// all opcodes and uses the shared VM state for heap allocation, class
/// lookup, and global variable access.
pub struct TaskExecutor<'a> {
    /// Shared VM state
    state: &'a SharedVmState,

    /// The task being executed
    task: &'a Task,

    /// Module containing the bytecode
    module: &'a Module,

    /// Stack of currently executing closures (for LoadCaptured)
    closure_stack: Vec<Value>,
}

impl<'a> TaskExecutor<'a> {
    /// Create a new task executor
    pub fn new(state: &'a SharedVmState, task: &'a Task, module: &'a Module) -> Self {
        Self {
            state,
            task,
            module,
            closure_stack: Vec::new(),
        }
    }

    /// Execute the task until completion or suspension
    pub fn execute(&mut self) -> VmResult<Value> {
        let func_index = self.task.function_id();

        if func_index >= self.module.functions.len() {
            return Err(VmError::RuntimeError(format!(
                "Invalid function index: {}",
                func_index
            )));
        }

        let function = &self.module.functions[func_index];
        let code = &function.code;

        // Get task's stack
        let stack = self.task.stack();
        let mut stack_guard = stack.lock().unwrap();

        // Get/set instruction pointer
        let mut ip = self.task.ip();

        // Only allocate locals on fresh start (ip == 0)
        if ip == 0 {
            // Allocate space for local variables
            for _ in 0..function.local_count {
                stack_guard.push(Value::null())?;
            }

            // Set initial arguments as the first N locals
            let initial_args = self.task.take_initial_args();
            for (i, arg) in initial_args.into_iter().enumerate() {
                if i < function.local_count as usize {
                    stack_guard.set_at(i, arg)?;
                }
            }
        }

        let locals_base = 0;

        // Exception handler stack
        let mut exception_handlers: Vec<ExceptionHandler> = Vec::new();
        let mut current_exception: Option<Value> = None;
        // Caught exception (preserved for Rethrow even after catch entry clears current_exception)
        let mut caught_exception: Option<Value> = None;

        loop {
            // Poll safepoint
            self.state.safepoint.poll();

            // Check for preemption
            if self.task.is_preempt_requested() {
                self.task.clear_preempt();
                self.task.set_ip(ip);
                drop(stack_guard);
                return Err(VmError::TaskPreempted);
            }

            if ip >= code.len() {
                break;
            }

            let opcode_byte = code[ip];
            ip += 1;

            let opcode = Opcode::from_u8(opcode_byte).ok_or(VmError::InvalidOpcode(opcode_byte))?;

            match opcode {
                // ===== Stack Manipulation =====
                Opcode::Nop => {}
                Opcode::Pop => {
                    stack_guard.pop()?;
                }
                Opcode::Dup => {
                    let value = stack_guard.peek()?;
                    stack_guard.push(value)?;
                }
                Opcode::Swap => {
                    let a = stack_guard.pop()?;
                    let b = stack_guard.pop()?;
                    stack_guard.push(a)?;
                    stack_guard.push(b)?;
                }

                // ===== Constants =====
                Opcode::ConstNull => {
                    stack_guard.push(Value::null())?;
                }
                Opcode::ConstTrue => {
                    stack_guard.push(Value::bool(true))?;
                }
                Opcode::ConstFalse => {
                    stack_guard.push(Value::bool(false))?;
                }
                Opcode::ConstI32 => {
                    let value = Self::read_i32(code, &mut ip)?;
                    stack_guard.push(Value::i32(value))?;
                }
                Opcode::ConstF64 => {
                    let value = Self::read_f64(code, &mut ip)?;
                    stack_guard.push(Value::f64(value))?;
                }
                Opcode::ConstStr => {
                    let index = Self::read_u16(code, &mut ip)? as usize;
                    let s = self.module.constants.strings.get(index).ok_or_else(|| {
                        VmError::RuntimeError(format!("Invalid string constant index: {}", index))
                    })?;
                    // Allocate string on GC heap
                    let raya_string = RayaString::new(s.clone());
                    let gc_ptr = self.state.gc.lock().allocate(raya_string);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }

                // ===== Local Variables =====
                Opcode::LoadLocal => {
                    let index = Self::read_u16(code, &mut ip)? as usize;
                    let value = stack_guard.peek_at(locals_base + index)?;
                    stack_guard.push(value)?;
                }
                Opcode::StoreLocal => {
                    let index = Self::read_u16(code, &mut ip)? as usize;
                    let value = stack_guard.pop()?;
                    stack_guard.set_at(locals_base + index, value)?;
                }
                Opcode::LoadLocal0 => {
                    let value = stack_guard.peek_at(locals_base)?;
                    stack_guard.push(value)?;
                }
                Opcode::LoadLocal1 => {
                    let value = stack_guard.peek_at(locals_base + 1)?;
                    stack_guard.push(value)?;
                }
                Opcode::StoreLocal0 => {
                    let value = stack_guard.pop()?;
                    stack_guard.set_at(locals_base, value)?;
                }
                Opcode::StoreLocal1 => {
                    let value = stack_guard.pop()?;
                    stack_guard.set_at(locals_base + 1, value)?;
                }

                // ===== Global Variables =====
                Opcode::LoadGlobal => {
                    let index = Self::read_u32(code, &mut ip)? as usize;
                    let globals = self.state.globals_by_index.read();
                    let value = globals.get(index).copied().unwrap_or(Value::null());
                    stack_guard.push(value)?;
                }
                Opcode::StoreGlobal => {
                    let index = Self::read_u32(code, &mut ip)? as usize;
                    let value = stack_guard.pop()?;
                    let mut globals = self.state.globals_by_index.write();
                    if index >= globals.len() {
                        globals.resize(index + 1, Value::null());
                    }
                    globals[index] = value;
                }

                // ===== Integer Arithmetic =====
                Opcode::Iadd => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a.wrapping_add(b)))?;
                }
                Opcode::Isub => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a.wrapping_sub(b)))?;
                }
                Opcode::Imul => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a.wrapping_mul(b)))?;
                }
                Opcode::Idiv => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    if b == 0 {
                        return Err(VmError::RuntimeError("Division by zero".to_string()));
                    }
                    stack_guard.push(Value::i32(a / b))?;
                }
                Opcode::Imod => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    if b == 0 {
                        return Err(VmError::RuntimeError("Division by zero".to_string()));
                    }
                    stack_guard.push(Value::i32(a % b))?;
                }
                Opcode::Ineg => {
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(-a))?;
                }
                Opcode::Ipow => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a.pow(b as u32)))?;
                }

                // ===== Integer Bitwise =====
                Opcode::Ishl => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a << (b & 31)))?;
                }
                Opcode::Ishr => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a >> (b & 31)))?;
                }
                Opcode::Iushr => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(((a as u32) >> (b & 31)) as i32))?;
                }
                Opcode::Iand => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a & b))?;
                }
                Opcode::Ior => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a | b))?;
                }
                Opcode::Ixor => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(a ^ b))?;
                }
                Opcode::Inot => {
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::i32(!a))?;
                }

                // ===== Integer Comparisons =====
                Opcode::Ieq => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::bool(a == b))?;
                }
                Opcode::Ine => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::bool(a != b))?;
                }
                Opcode::Ilt => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::bool(a < b))?;
                }
                Opcode::Ile => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::bool(a <= b))?;
                }
                Opcode::Igt => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::bool(a > b))?;
                }
                Opcode::Ige => {
                    let b = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    let a = stack_guard.pop()?.as_i32().ok_or_else(|| {
                        VmError::TypeError("Expected i32".to_string())
                    })?;
                    stack_guard.push(Value::bool(a >= b))?;
                }

                // ===== Float Arithmetic =====
                Opcode::Fadd => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::f64(a + b))?;
                }
                Opcode::Fsub => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::f64(a - b))?;
                }
                Opcode::Fmul => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::f64(a * b))?;
                }
                Opcode::Fdiv => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::f64(a / b))?;
                }
                Opcode::Fneg => {
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::f64(-a))?;
                }
                Opcode::Fpow => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::f64(a.powf(b)))?;
                }

                // ===== Float Comparisons =====
                Opcode::Feq => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::bool(a == b))?;
                }
                Opcode::Fne => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::bool(a != b))?;
                }
                Opcode::Flt => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::bool(a < b))?;
                }
                Opcode::Fle => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::bool(a <= b))?;
                }
                Opcode::Fgt => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::bool(a > b))?;
                }
                Opcode::Fge => {
                    let b = value_to_f64(stack_guard.pop()?)?;
                    let a = value_to_f64(stack_guard.pop()?)?;
                    stack_guard.push(Value::bool(a >= b))?;
                }

                // ===== Generic Number Operations =====
                Opcode::Nadd | Opcode::Nsub | Opcode::Nmul | Opcode::Ndiv | Opcode::Nmod | Opcode::Nneg | Opcode::Npow => {
                    // For simplicity, treat as i32 for now (proper implementation would check types)
                    match opcode {
                        Opcode::Nadd => {
                            let b = stack_guard.pop()?.as_i32().unwrap_or(0);
                            let a = stack_guard.pop()?.as_i32().unwrap_or(0);
                            stack_guard.push(Value::i32(a.wrapping_add(b)))?;
                        }
                        Opcode::Nsub => {
                            let b = stack_guard.pop()?.as_i32().unwrap_or(0);
                            let a = stack_guard.pop()?.as_i32().unwrap_or(0);
                            stack_guard.push(Value::i32(a.wrapping_sub(b)))?;
                        }
                        Opcode::Nmul => {
                            let b = stack_guard.pop()?.as_i32().unwrap_or(0);
                            let a = stack_guard.pop()?.as_i32().unwrap_or(0);
                            stack_guard.push(Value::i32(a.wrapping_mul(b)))?;
                        }
                        Opcode::Ndiv => {
                            let b = stack_guard.pop()?.as_i32().unwrap_or(1);
                            let a = stack_guard.pop()?.as_i32().unwrap_or(0);
                            stack_guard.push(Value::i32(if b != 0 { a / b } else { 0 }))?;
                        }
                        Opcode::Nmod => {
                            let b = stack_guard.pop()?.as_i32().unwrap_or(1);
                            let a = stack_guard.pop()?.as_i32().unwrap_or(0);
                            stack_guard.push(Value::i32(if b != 0 { a % b } else { 0 }))?;
                        }
                        Opcode::Nneg => {
                            let a = stack_guard.pop()?.as_i32().unwrap_or(0);
                            stack_guard.push(Value::i32(-a))?;
                        }
                        Opcode::Npow => {
                            let b = stack_guard.pop()?.as_i32().unwrap_or(0);
                            let a = stack_guard.pop()?.as_i32().unwrap_or(0);
                            stack_guard.push(Value::i32(a.pow(b as u32)))?;
                        }
                        _ => unreachable!(),
                    }
                }

                // ===== Boolean Operations =====
                Opcode::Not => {
                    let a = stack_guard.pop()?;
                    stack_guard.push(Value::bool(!a.is_truthy()))?;
                }
                Opcode::And => {
                    let b = stack_guard.pop()?;
                    let a = stack_guard.pop()?;
                    stack_guard.push(Value::bool(a.is_truthy() && b.is_truthy()))?;
                }
                Opcode::Or => {
                    let b = stack_guard.pop()?;
                    let a = stack_guard.pop()?;
                    stack_guard.push(Value::bool(a.is_truthy() || b.is_truthy()))?;
                }

                // ===== Generic Equality =====
                Opcode::Eq | Opcode::StrictEq => {
                    let b = stack_guard.pop()?;
                    let a = stack_guard.pop()?;
                    stack_guard.push(Value::bool(a == b))?;
                }
                Opcode::Ne | Opcode::StrictNe => {
                    let b = stack_guard.pop()?;
                    let a = stack_guard.pop()?;
                    stack_guard.push(Value::bool(a != b))?;
                }

                // ===== Control Flow =====
                Opcode::Jmp => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    if offset < 0 {
                        self.state.safepoint.poll();
                    }
                    ip = (ip as isize + offset as isize) as usize;
                }
                Opcode::JmpIfTrue => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    let cond = stack_guard.pop()?;
                    if cond.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfFalse => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    let cond = stack_guard.pop()?;
                    if !cond.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfNull => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    let value = stack_guard.pop()?;
                    if value.is_null() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfNotNull => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    let value = stack_guard.pop()?;
                    if !value.is_null() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }

                // ===== Exception Handling =====
                Opcode::Try => {
                    // Read relative offsets and convert to absolute positions
                    // -1 means "no catch/finally block"
                    let catch_rel = Self::read_i32(code, &mut ip)?;
                    let catch_abs = if catch_rel >= 0 {
                        (ip as i32 + catch_rel) as i32
                    } else {
                        -1 // No catch block
                    };

                    let finally_rel = Self::read_i32(code, &mut ip)?;
                    let finally_abs = if finally_rel > 0 {
                        (ip as i32 + finally_rel) as i32
                    } else {
                        -1 // No finally block (0 or negative)
                    };

                    let handler = ExceptionHandler {
                        catch_offset: catch_abs,
                        finally_offset: finally_abs,
                        stack_size: stack_guard.depth(),
                        frame_count: 0, // TaskExecutor doesn't track frames the same way
                        mutex_count: 0,
                    };
                    exception_handlers.push(handler);
                }
                Opcode::EndTry => {
                    exception_handlers.pop();
                }
                Opcode::Throw => {
                    let exception = stack_guard.pop()?;
                    current_exception = Some(exception);

                    // Begin exception unwinding
                    loop {
                        if let Some(handler) = exception_handlers.last().cloned() {
                            // Unwind stack to handler's saved state
                            while stack_guard.depth() > handler.stack_size {
                                stack_guard.pop()?;
                            }

                            // Jump to catch block if present
                            if handler.catch_offset != -1 {
                                exception_handlers.pop();
                                // IMPORTANT: Use take() to clear current_exception, otherwise
                                // the exception will be detected again after any function call
                                // Also save to caught_exception for Rethrow
                                let exc = current_exception.take().unwrap();
                                caught_exception = Some(exc);
                                stack_guard.push(exc)?;
                                ip = handler.catch_offset as usize;
                                break;
                            }

                            // No catch block, execute finally block if present
                            if handler.finally_offset != -1 {
                                exception_handlers.pop();
                                ip = handler.finally_offset as usize;
                                break;
                            }

                            // No catch or finally, remove handler and continue unwinding
                            exception_handlers.pop();
                        } else {
                            // No handler found - store exception in task and propagate error
                            if let Some(exc) = current_exception.take() {
                                self.task.set_exception(exc);
                            }
                            return Err(VmError::RuntimeError(
                                "Uncaught exception".to_string(),
                            ));
                        }
                    }
                }
                Opcode::Rethrow => {
                    // Re-raise the caught exception (stored in caught_exception for Rethrow)
                    if let Some(exception) = caught_exception.clone() {
                        // Set current_exception for propagation tracking
                        current_exception = Some(exception.clone());

                        loop {
                            if let Some(handler) = exception_handlers.last().cloned() {
                                while stack_guard.depth() > handler.stack_size {
                                    stack_guard.pop()?;
                                }

                                if handler.finally_offset != -1 {
                                    exception_handlers.pop();
                                    ip = handler.finally_offset as usize;
                                    break;
                                }

                                if handler.catch_offset != -1 {
                                    exception_handlers.pop();
                                    // Save for potential nested Rethrow and clear current_exception
                                    let exc = current_exception.take().unwrap();
                                    caught_exception = Some(exc);
                                    stack_guard.push(exc)?;
                                    ip = handler.catch_offset as usize;
                                    break;
                                }

                                exception_handlers.pop();
                            } else {
                                // No handler found - store exception in task and propagate error
                                if let Some(exc) = current_exception.take() {
                                    self.task.set_exception(exc);
                                }
                                return Err(VmError::RuntimeError(
                                    "Uncaught exception".to_string(),
                                ));
                            }
                        }
                    } else {
                        return Err(VmError::RuntimeError(
                            "RETHROW with no active exception".to_string(),
                        ));
                    }
                }

                // ===== Function Calls =====
                Opcode::Call => {
                    self.state.safepoint.poll();
                    let func_index = Self::read_u32(code, &mut ip)? as usize;
                    let arg_count = Self::read_u16(code, &mut ip)? as usize;

                    if func_index == 0xFFFFFFFF {
                        // Closure call
                        let result = self.call_closure(&mut stack_guard, arg_count)?;
                        stack_guard.push(result)?;
                    } else {
                        // Regular function call - execute recursively
                        // Pop arguments
                        let mut args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            args.push(stack_guard.pop()?);
                        }
                        args.reverse();

                        // Save current IP
                        self.task.set_ip(ip);
                        drop(stack_guard);

                        // Create child task for the call
                        let result = self.call_function(func_index, args)?;

                        // Restore stack and push result
                        stack_guard = self.task.stack().lock().unwrap();
                        stack_guard.push(result)?;
                    }
                }
                Opcode::Return => {
                    let return_value = if stack_guard.depth() > 0 {
                        stack_guard.pop()?
                    } else {
                        Value::null()
                    };
                    self.task.set_ip(ip);
                    return Ok(return_value);
                }
                Opcode::ReturnVoid => {
                    self.task.set_ip(ip);
                    return Ok(Value::null());
                }

                // ===== Object Operations =====
                Opcode::New => {
                    self.state.safepoint.poll();
                    let class_index = Self::read_u16(code, &mut ip)? as usize;

                    let classes = self.state.classes.read();
                    let class = classes.get_class(class_index).ok_or_else(|| {
                        VmError::RuntimeError(format!("Invalid class index: {}", class_index))
                    })?;
                    let field_count = class.field_count;
                    drop(classes);

                    let obj = Object::new(class_index, field_count);
                    let gc_ptr = self.state.gc.lock().allocate(obj);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }
                Opcode::LoadField => {
                    let field_offset = Self::read_u16(code, &mut ip)? as usize;
                    let obj_val = stack_guard.pop()?;

                    if !obj_val.is_ptr() {
                        return Err(VmError::TypeError(
                            "Expected object for field access".to_string(),
                        ));
                    }

                    let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                    let value = obj.get_field(field_offset).ok_or_else(|| {
                        VmError::RuntimeError(format!("Field offset {} out of bounds", field_offset))
                    })?;
                    stack_guard.push(value)?;
                }
                Opcode::StoreField => {
                    let field_offset = Self::read_u16(code, &mut ip)? as usize;
                    let value = stack_guard.pop()?;
                    let obj_val = stack_guard.pop()?;

                    if !obj_val.is_ptr() {
                        return Err(VmError::TypeError(
                            "Expected object for field access".to_string(),
                        ));
                    }

                    let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                    let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                    obj.set_field(field_offset, value)
                        .map_err(|e| VmError::RuntimeError(e))?;
                }
                Opcode::LoadFieldFast => {
                    let field_offset = Self::read_u8(code, &mut ip)? as usize;
                    let obj_val = stack_guard.pop()?;

                    if !obj_val.is_ptr() {
                        return Err(VmError::TypeError(
                            "Expected object for field access".to_string(),
                        ));
                    }

                    let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                    let obj = unsafe { &*obj_ptr.unwrap().as_ptr() };
                    let value = obj.get_field(field_offset).ok_or_else(|| {
                        VmError::RuntimeError(format!("Field offset {} out of bounds", field_offset))
                    })?;
                    stack_guard.push(value)?;
                }
                Opcode::StoreFieldFast => {
                    let field_offset = Self::read_u8(code, &mut ip)? as usize;
                    let value = stack_guard.pop()?;
                    let obj_val = stack_guard.pop()?;

                    if !obj_val.is_ptr() {
                        return Err(VmError::TypeError(
                            "Expected object for field access".to_string(),
                        ));
                    }

                    let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                    let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                    obj.set_field(field_offset, value)
                        .map_err(|e| VmError::RuntimeError(e))?;
                }
                Opcode::ObjectLiteral => {
                    self.state.safepoint.poll();
                    let class_index = Self::read_u16(code, &mut ip)? as usize;
                    let field_count = Self::read_u16(code, &mut ip)? as usize;

                    let obj = Object::new(class_index, field_count);
                    let gc_ptr = self.state.gc.lock().allocate(obj);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }
                Opcode::InitObject => {
                    let field_offset = Self::read_u16(code, &mut ip)? as usize;
                    let value = stack_guard.pop()?;
                    let obj_val = stack_guard.peek()?;

                    if !obj_val.is_ptr() {
                        return Err(VmError::TypeError(
                            "Expected object for field initialization".to_string(),
                        ));
                    }

                    let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
                    let obj = unsafe { &mut *obj_ptr.unwrap().as_ptr() };
                    obj.set_field(field_offset, value)
                        .map_err(|e| VmError::RuntimeError(e))?;
                }

                // ===== Array Operations =====
                Opcode::NewArray => {
                    self.state.safepoint.poll();
                    let type_index = Self::read_u16(code, &mut ip)? as usize;
                    let len = stack_guard.pop()?.as_i32().unwrap_or(0) as usize;

                    let arr = Array::new(type_index, len);
                    let gc_ptr = self.state.gc.lock().allocate(arr);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }
                Opcode::LoadElem => {
                    let index = stack_guard.pop()?.as_i32().unwrap_or(0) as usize;
                    let arr_val = stack_guard.pop()?;

                    if !arr_val.is_ptr() {
                        return Err(VmError::TypeError("Expected array".to_string()));
                    }

                    let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                    let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                    let value = arr.get(index).ok_or_else(|| {
                        VmError::RuntimeError(format!("Array index {} out of bounds", index))
                    })?;
                    stack_guard.push(value)?;
                }
                Opcode::StoreElem => {
                    let value = stack_guard.pop()?;
                    let index = stack_guard.pop()?.as_i32().unwrap_or(0) as usize;
                    let arr_val = stack_guard.pop()?;

                    if !arr_val.is_ptr() {
                        return Err(VmError::TypeError("Expected array".to_string()));
                    }

                    let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                    let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                    arr.set(index, value)
                        .map_err(|e| VmError::RuntimeError(e))?;
                }
                Opcode::ArrayLen => {
                    let arr_val = stack_guard.pop()?;

                    if !arr_val.is_ptr() {
                        return Err(VmError::TypeError("Expected array".to_string()));
                    }

                    let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                    let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                    stack_guard.push(Value::i32(arr.len() as i32))?;
                }
                Opcode::ArrayLiteral => {
                    self.state.safepoint.poll();
                    let type_index = Self::read_u32(code, &mut ip)? as usize;
                    let length = Self::read_u32(code, &mut ip)? as usize;

                    let arr = Array::new(type_index, length);
                    let gc_ptr = self.state.gc.lock().allocate(arr);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }
                Opcode::InitArray => {
                    let index = Self::read_u32(code, &mut ip)? as usize;
                    let value = stack_guard.pop()?;
                    let arr_val = stack_guard.peek()?;

                    if !arr_val.is_ptr() {
                        return Err(VmError::TypeError("Expected array".to_string()));
                    }

                    let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                    let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                    arr.set(index, value)
                        .map_err(|e| VmError::RuntimeError(e))?;
                }

                // ===== Closure Operations =====
                Opcode::MakeClosure => {
                    self.state.safepoint.poll();
                    let func_index = Self::read_u32(code, &mut ip)? as usize;
                    let capture_count = Self::read_u16(code, &mut ip)? as usize;

                    let mut captures = Vec::with_capacity(capture_count);
                    for _ in 0..capture_count {
                        captures.push(stack_guard.pop()?);
                    }
                    captures.reverse();

                    let closure = Closure::new(func_index, captures);
                    let gc_ptr = self.state.gc.lock().allocate(closure);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }
                Opcode::LoadCaptured => {
                    let capture_index = Self::read_u16(code, &mut ip)? as usize;

                    let closure_val = self.closure_stack.last().ok_or_else(|| {
                        VmError::RuntimeError("LoadCaptured without active closure".to_string())
                    })?;

                    let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                    let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                    let value = closure.get_captured(capture_index).ok_or_else(|| {
                        VmError::RuntimeError(format!(
                            "Capture index {} out of bounds",
                            capture_index
                        ))
                    })?;
                    stack_guard.push(value)?;
                }
                Opcode::StoreCaptured => {
                    let capture_index = Self::read_u16(code, &mut ip)? as usize;
                    let value = stack_guard.pop()?;

                    let closure_val = self.closure_stack.last().ok_or_else(|| {
                        VmError::RuntimeError("StoreCaptured without active closure".to_string())
                    })?;

                    let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                    let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                    closure
                        .set_captured(capture_index, value)
                        .map_err(|e| VmError::RuntimeError(e))?;
                }
                Opcode::SetClosureCapture => {
                    let capture_index = Self::read_u16(code, &mut ip)? as usize;
                    let value = stack_guard.pop()?;
                    let closure_val = stack_guard.pop()?;

                    if !closure_val.is_ptr() {
                        return Err(VmError::TypeError("Expected closure".to_string()));
                    }

                    let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                    let closure = unsafe { &mut *closure_ptr.unwrap().as_ptr() };
                    closure
                        .set_captured(capture_index, value)
                        .map_err(|e| VmError::RuntimeError(e))?;
                    stack_guard.push(closure_val)?;
                }

                // ===== RefCell Operations =====
                Opcode::NewRefCell => {
                    use crate::vm::object::RefCell;
                    let initial_value = stack_guard.pop()?;
                    let refcell = RefCell::new(initial_value);
                    let gc_ptr = self.state.gc.lock().allocate(refcell);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }
                Opcode::LoadRefCell => {
                    use crate::vm::object::RefCell;
                    let refcell_val = stack_guard.pop()?;

                    if !refcell_val.is_ptr() {
                        return Err(VmError::TypeError("Expected RefCell".to_string()));
                    }

                    let refcell_ptr = unsafe { refcell_val.as_ptr::<RefCell>() };
                    let refcell = unsafe { &*refcell_ptr.unwrap().as_ptr() };
                    stack_guard.push(refcell.get())?;
                }
                Opcode::StoreRefCell => {
                    use crate::vm::object::RefCell;
                    let value = stack_guard.pop()?;
                    let refcell_val = stack_guard.pop()?;

                    if !refcell_val.is_ptr() {
                        return Err(VmError::TypeError("Expected RefCell".to_string()));
                    }

                    let refcell_ptr = unsafe { refcell_val.as_ptr::<RefCell>() };
                    let refcell = unsafe { &mut *refcell_ptr.unwrap().as_ptr() };
                    refcell.set(value);
                }

                // ===== Concurrency Operations =====
                Opcode::Spawn => {
                    let func_index = Self::read_u16(code, &mut ip)? as usize;
                    let arg_count = Self::read_u16(code, &mut ip)? as usize;

                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(stack_guard.pop()?);
                    }
                    // Args are already in correct order: codegen pushed in reverse, pop gives correct order

                    let new_task = Arc::new(Task::with_args(
                        func_index,
                        self.task.module().clone(),
                        Some(self.task.id()),
                        args,
                    ));

                    let task_id = new_task.id();
                    self.state.tasks.write().insert(task_id, new_task.clone());
                    self.state.injector.push(new_task);

                    stack_guard.push(Value::u64(task_id.as_u64()))?;
                }
                Opcode::SpawnClosure => {
                    let arg_count = Self::read_u16(code, &mut ip)? as usize;

                    // Pop closure from stack
                    let closure_val = stack_guard.pop()?;
                    if !closure_val.is_ptr() {
                        return Err(VmError::TypeError("Expected closure".to_string()));
                    }

                    let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                    let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };

                    // Pop arguments (already in correct order after pop)
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(stack_guard.pop()?);
                    }

                    // Prepend captures to args - captures become first locals in the spawned task
                    let mut task_args = closure.captures.clone();
                    task_args.extend(args);

                    let new_task = Arc::new(Task::with_args(
                        closure.func_id,
                        self.task.module().clone(),
                        Some(self.task.id()),
                        task_args,
                    ));

                    let task_id = new_task.id();
                    self.state.tasks.write().insert(task_id, new_task.clone());
                    self.state.injector.push(new_task);

                    stack_guard.push(Value::u64(task_id.as_u64()))?;
                }
                Opcode::Await => {
                    let task_id_val = stack_guard.pop()?;
                    let task_id_u64 = task_id_val.as_u64().ok_or_else(|| {
                        VmError::TypeError("Expected TaskId".to_string())
                    })?;

                    let awaited_task_id = TaskId::from_u64(task_id_u64);

                    let awaited_task = self
                        .state
                        .tasks
                        .read()
                        .get(&awaited_task_id)
                        .cloned()
                        .ok_or_else(|| {
                            VmError::RuntimeError(format!(
                                "Task {:?} not found",
                                awaited_task_id
                            ))
                        })?;

                    match awaited_task.state() {
                        TaskState::Completed => {
                            let result = awaited_task.result().unwrap_or(Value::null());
                            stack_guard.push(result)?;
                        }
                        TaskState::Failed => {
                            // Get exception from the awaited task
                            if let Some(exc) = awaited_task.current_exception() {
                                // Re-throw the exception in current context
                                current_exception = Some(exc.clone());

                                // Begin exception unwinding
                                loop {
                                    if let Some(handler) = exception_handlers.last().cloned() {
                                        while stack_guard.depth() > handler.stack_size {
                                            stack_guard.pop()?;
                                        }

                                        if handler.catch_offset != -1 {
                                            exception_handlers.pop();
                                            stack_guard.push(exc.clone())?;
                                            ip = handler.catch_offset as usize;
                                            break;
                                        }

                                        if handler.finally_offset != -1 {
                                            exception_handlers.pop();
                                            ip = handler.finally_offset as usize;
                                            break;
                                        }

                                        exception_handlers.pop();
                                    } else {
                                        // No handler found - store and propagate
                                        self.task.set_exception(exc);
                                        return Err(VmError::RuntimeError(
                                            "Uncaught exception".to_string(),
                                        ));
                                    }
                                }
                            } else {
                                return Err(VmError::RuntimeError(format!(
                                    "Awaited task {:?} failed",
                                    awaited_task_id
                                )));
                            }
                        }
                        _ => {
                            // Task not done yet - yield instead of busy-wait
                            // Push task_id back on stack so Await can re-execute on resume
                            stack_guard.push(task_id_val)?;

                            // Register as waiter on the awaited task
                            awaited_task.add_waiter(self.task.id());

                            // Save IP pointing to the Await opcode (ip-1 since we already incremented)
                            self.task.set_ip(ip - 1);

                            // Mark what task we're waiting for
                            self.task.set_awaiting(awaited_task_id);

                            // Set task state to Suspended
                            self.task.set_state(TaskState::Suspended);

                            // Release the stack lock and return Suspended
                            drop(stack_guard);
                            return Err(VmError::Suspended);
                        }
                    }
                }
                Opcode::WaitAll => {
                    let arr_val = stack_guard.pop()?;

                    if !arr_val.is_ptr() {
                        return Err(VmError::TypeError("Expected array of tasks".to_string()));
                    }

                    let arr_ptr = unsafe { arr_val.as_ptr::<Array>() };
                    let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                    // Collect all task IDs
                    let mut task_ids = Vec::with_capacity(arr.len());
                    for i in 0..arr.len() {
                        let task_id_val = arr.get(i).unwrap_or(Value::null());
                        let task_id_u64 = task_id_val.as_u64().ok_or_else(|| {
                            VmError::TypeError("Expected TaskId in array".to_string())
                        })?;
                        task_ids.push(TaskId::from_u64(task_id_u64));
                    }

                    // Wait for all tasks
                    let mut results = vec![Value::null(); task_ids.len()];
                    'wait_all_outer: loop {
                        let mut all_done = true;

                        for (i, task_id) in task_ids.iter().enumerate() {
                            let task = self
                                .state
                                .tasks
                                .read()
                                .get(task_id)
                                .cloned()
                                .ok_or_else(|| {
                                    VmError::RuntimeError(format!("Task {:?} not found", task_id))
                                })?;

                            match task.state() {
                                TaskState::Completed => {
                                    results[i] = task.result().unwrap_or(Value::null());
                                }
                                TaskState::Failed => {
                                    // Handle failed task - re-throw exception
                                    if let Some(exc) = task.current_exception() {
                                        current_exception = Some(exc.clone());

                                        loop {
                                            if let Some(handler) = exception_handlers.last().cloned() {
                                                while stack_guard.depth() > handler.stack_size {
                                                    stack_guard.pop()?;
                                                }

                                                if handler.catch_offset != -1 {
                                                    exception_handlers.pop();
                                                    stack_guard.push(exc)?;
                                                    ip = handler.catch_offset as usize;
                                                    break 'wait_all_outer;
                                                }

                                                if handler.finally_offset != -1 {
                                                    exception_handlers.pop();
                                                    ip = handler.finally_offset as usize;
                                                    break 'wait_all_outer;
                                                }

                                                exception_handlers.pop();
                                            } else {
                                                return Err(VmError::RuntimeError(format!(
                                                    "Uncaught exception from task {:?} in WAIT_ALL",
                                                    task_id
                                                )));
                                            }
                                        }
                                    } else {
                                        return Err(VmError::RuntimeError(format!(
                                            "Task {:?} failed",
                                            task_id
                                        )));
                                    }
                                }
                                _ => {
                                    all_done = false;
                                }
                            }
                        }

                        if all_done {
                            // Create result array
                            let result_arr = Array::new(0, results.len());
                            let gc_ptr = self.state.gc.lock().allocate(result_arr);
                            let arr_value =
                                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };

                            // Initialize array elements
                            let result_arr_ptr = unsafe { arr_value.as_ptr::<Array>() };
                            let result_arr = unsafe { &mut *result_arr_ptr.unwrap().as_ptr() };
                            for (i, result) in results.into_iter().enumerate() {
                                result_arr.set(i, result).map_err(|e| VmError::RuntimeError(e))?;
                            }

                            stack_guard.push(arr_value)?;
                            break 'wait_all_outer;
                        }

                        drop(stack_guard);
                        self.state.safepoint.poll();
                        std::thread::sleep(Duration::from_micros(100));
                        stack_guard = self.task.stack().lock().unwrap();
                    }
                }

                Opcode::Sleep => {
                    // Pop duration (milliseconds) from stack
                    let duration_val = stack_guard.pop()?;
                    let ms = duration_val.as_i64().unwrap_or(0) as u64;

                    // Sleep for the duration
                    if ms > 0 {
                        drop(stack_guard);
                        std::thread::sleep(Duration::from_millis(ms));
                        stack_guard = self.task.stack().lock().unwrap();
                    }
                    // For ms == 0, just yield
                    self.state.safepoint.poll();
                }

                Opcode::Yield => {
                    // Voluntary yield
                    drop(stack_guard);
                    self.state.safepoint.poll();
                    std::thread::yield_now();
                    stack_guard = self.task.stack().lock().unwrap();
                }

                // ===== String Operations =====
                Opcode::Sconcat => {
                    let b_val = stack_guard.pop()?;
                    let a_val = stack_guard.pop()?;

                    // Get string data
                    let a_str = if a_val.is_ptr() {
                        let ptr = unsafe { a_val.as_ptr::<RayaString>() };
                        unsafe { &*ptr.unwrap().as_ptr() }.data.clone()
                    } else {
                        format!("{:?}", a_val)
                    };

                    let b_str = if b_val.is_ptr() {
                        let ptr = unsafe { b_val.as_ptr::<RayaString>() };
                        unsafe { &*ptr.unwrap().as_ptr() }.data.clone()
                    } else {
                        format!("{:?}", b_val)
                    };

                    let result = RayaString::new(format!("{}{}", a_str, b_str));
                    let gc_ptr = self.state.gc.lock().allocate(result);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }
                Opcode::Slen => {
                    let s_val = stack_guard.pop()?;

                    if !s_val.is_ptr() {
                        return Err(VmError::TypeError("Expected string".to_string()));
                    }

                    let str_ptr = unsafe { s_val.as_ptr::<RayaString>() };
                    let s = unsafe { &*str_ptr.unwrap().as_ptr() };
                    stack_guard.push(Value::i32(s.len() as i32))?;
                }
                Opcode::Seq => {
                    let b_val = stack_guard.pop()?;
                    let a_val = stack_guard.pop()?;

                    let result = if a_val.is_ptr() && b_val.is_ptr() {
                        let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                        let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                        let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                        let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                        a.data == b.data
                    } else {
                        false
                    };
                    stack_guard.push(Value::bool(result))?;
                }
                Opcode::Sne => {
                    let b_val = stack_guard.pop()?;
                    let a_val = stack_guard.pop()?;

                    let result = if a_val.is_ptr() && b_val.is_ptr() {
                        let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                        let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                        let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                        let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                        a.data != b.data
                    } else {
                        true
                    };
                    stack_guard.push(Value::bool(result))?;
                }
                Opcode::Slt | Opcode::Sle | Opcode::Sgt | Opcode::Sge => {
                    let b_val = stack_guard.pop()?;
                    let a_val = stack_guard.pop()?;

                    let result = if a_val.is_ptr() && b_val.is_ptr() {
                        let a_ptr = unsafe { a_val.as_ptr::<RayaString>() };
                        let b_ptr = unsafe { b_val.as_ptr::<RayaString>() };
                        let a = unsafe { &*a_ptr.unwrap().as_ptr() };
                        let b = unsafe { &*b_ptr.unwrap().as_ptr() };
                        match opcode {
                            Opcode::Slt => a.data < b.data,
                            Opcode::Sle => a.data <= b.data,
                            Opcode::Sgt => a.data > b.data,
                            Opcode::Sge => a.data >= b.data,
                            _ => unreachable!(),
                        }
                    } else {
                        false
                    };
                    stack_guard.push(Value::bool(result))?;
                }
                Opcode::ToString => {
                    let val = stack_guard.pop()?;
                    let s = format!("{:?}", val);
                    let result = RayaString::new(s);
                    let gc_ptr = self.state.gc.lock().allocate(result);
                    let value =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    stack_guard.push(value)?;
                }

                // ===== Other Operations =====
                _ => {
                    return Err(VmError::RuntimeError(format!(
                        "Opcode {:?} not implemented in TaskExecutor",
                        opcode
                    )));
                }
            }
        }

        self.task.set_ip(ip);
        Ok(Value::null())
    }

    /// Call a closure
    fn call_closure(
        &mut self,
        stack: &mut std::sync::MutexGuard<Stack>,
        arg_count: usize,
    ) -> VmResult<Value> {
        // Pop arguments
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(stack.pop()?);
        }
        args.reverse();

        // Pop closure
        let closure_val = stack.pop()?;
        if !closure_val.is_ptr() {
            return Err(VmError::TypeError("Expected closure".to_string()));
        }

        let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
        let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
        let func_index = closure.func_id();

        // Push closure onto closure stack for LoadCaptured
        self.closure_stack.push(closure_val);

        // Execute the closure's function
        let result = self.call_function(func_index, args)?;

        // Pop closure from closure stack
        self.closure_stack.pop();

        Ok(result)
    }

    /// Call a function by index
    fn call_function(&mut self, func_index: usize, args: Vec<Value>) -> VmResult<Value> {
        if func_index >= self.module.functions.len() {
            return Err(VmError::RuntimeError(format!(
                "Invalid function index: {}",
                func_index
            )));
        }

        let function = &self.module.functions[func_index];
        let code = &function.code;

        // Create a new stack for this call
        let mut call_stack = Stack::new();

        // Allocate locals
        for _ in 0..function.local_count {
            call_stack.push(Value::null())?;
        }

        // Set arguments
        for (i, arg) in args.into_iter().enumerate() {
            if i < function.local_count as usize {
                call_stack.set_at(i, arg)?;
            }
        }

        let locals_base = 0;
        let mut ip = 0;

        loop {
            self.state.safepoint.poll();

            if ip >= code.len() {
                break;
            }

            let opcode_byte = code[ip];
            ip += 1;

            let opcode = Opcode::from_u8(opcode_byte).ok_or(VmError::InvalidOpcode(opcode_byte))?;

            // Execute opcode (simplified - only essential ones for nested calls)
            match opcode {
                Opcode::Return => {
                    let return_value = if call_stack.depth() > 0 {
                        call_stack.pop()?
                    } else {
                        Value::null()
                    };
                    return Ok(return_value);
                }
                Opcode::ReturnVoid => {
                    return Ok(Value::null());
                }
                Opcode::ConstNull => call_stack.push(Value::null())?,
                Opcode::ConstTrue => call_stack.push(Value::bool(true))?,
                Opcode::ConstFalse => call_stack.push(Value::bool(false))?,
                Opcode::ConstI32 => {
                    let value = Self::read_i32(code, &mut ip)?;
                    call_stack.push(Value::i32(value))?;
                }
                Opcode::LoadLocal => {
                    let index = Self::read_u16(code, &mut ip)? as usize;
                    let value = call_stack.peek_at(locals_base + index)?;
                    call_stack.push(value)?;
                }
                Opcode::StoreLocal => {
                    let index = Self::read_u16(code, &mut ip)? as usize;
                    let value = call_stack.pop()?;
                    call_stack.set_at(locals_base + index, value)?;
                }
                Opcode::LoadLocal0 => {
                    let value = call_stack.peek_at(locals_base)?;
                    call_stack.push(value)?;
                }
                Opcode::LoadLocal1 => {
                    let value = call_stack.peek_at(locals_base + 1)?;
                    call_stack.push(value)?;
                }
                Opcode::Iadd => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_add(b)))?;
                }
                Opcode::Isub => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_sub(b)))?;
                }
                Opcode::Imul => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::i32(a.wrapping_mul(b)))?;
                }
                Opcode::Ile => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::bool(a <= b))?;
                }
                Opcode::Ilt => {
                    let b = call_stack.pop()?.as_i32().unwrap_or(0);
                    let a = call_stack.pop()?.as_i32().unwrap_or(0);
                    call_stack.push(Value::bool(a < b))?;
                }
                Opcode::Jmp => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    ip = (ip as isize + offset as isize) as usize;
                }
                Opcode::JmpIfFalse => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    let cond = call_stack.pop()?;
                    if !cond.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                Opcode::JmpIfTrue => {
                    let offset = Self::read_i16(code, &mut ip)?;
                    let cond = call_stack.pop()?;
                    if cond.is_truthy() {
                        ip = (ip as isize + offset as isize) as usize;
                    }
                }
                _ => {
                    return Err(VmError::RuntimeError(format!(
                        "Opcode {:?} not implemented in nested call",
                        opcode
                    )));
                }
            }
        }

        Ok(Value::null())
    }

    // ===== Helper Methods =====

    #[inline]
    fn read_u8(code: &[u8], ip: &mut usize) -> VmResult<u8> {
        if *ip >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = code[*ip];
        *ip += 1;
        Ok(value)
    }

    #[inline]
    fn read_u16(code: &[u8], ip: &mut usize) -> VmResult<u16> {
        if *ip + 1 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = u16::from_le_bytes([code[*ip], code[*ip + 1]]);
        *ip += 2;
        Ok(value)
    }

    #[inline]
    fn read_i16(code: &[u8], ip: &mut usize) -> VmResult<i16> {
        if *ip + 1 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = i16::from_le_bytes([code[*ip], code[*ip + 1]]);
        *ip += 2;
        Ok(value)
    }

    #[inline]
    fn read_u32(code: &[u8], ip: &mut usize) -> VmResult<u32> {
        if *ip + 3 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = u32::from_le_bytes([code[*ip], code[*ip + 1], code[*ip + 2], code[*ip + 3]]);
        *ip += 4;
        Ok(value)
    }

    #[inline]
    fn read_i32(code: &[u8], ip: &mut usize) -> VmResult<i32> {
        if *ip + 3 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let value = i32::from_le_bytes([code[*ip], code[*ip + 1], code[*ip + 2], code[*ip + 3]]);
        *ip += 4;
        Ok(value)
    }

    #[inline]
    fn read_f64(code: &[u8], ip: &mut usize) -> VmResult<f64> {
        if *ip + 7 >= code.len() {
            return Err(VmError::RuntimeError("Unexpected end of bytecode".to_string()));
        }
        let bytes = [
            code[*ip],
            code[*ip + 1],
            code[*ip + 2],
            code[*ip + 3],
            code[*ip + 4],
            code[*ip + 5],
            code[*ip + 6],
            code[*ip + 7],
        ];
        let value = f64::from_le_bytes(bytes);
        *ip += 8;
        Ok(value)
    }
}
