//! Shared VM state for concurrent task execution
//!
//! This module provides shared state that can be safely accessed by multiple
//! worker threads executing tasks concurrently.

use crate::vm::gc::GarbageCollector;
use crate::vm::native_handler::{NativeHandler, NoopNativeHandler};
use crate::vm::native_registry::{NativeFunctionRegistry, ResolvedNatives};
use crate::vm::reflect::{ClassMetadataRegistry, MetadataStore};
use crate::vm::scheduler::{IoSubmission, Task, TaskId};
use crate::vm::sync::MutexRegistry;
use crate::vm::value::Value;
use crate::vm::interpreter::{ClassRegistry, ModuleRegistry, SafepointCoordinator};
use crossbeam::channel::Sender;
use crossbeam_deque::Injector;
use parking_lot::{Mutex, RwLock};
use crate::compiler::Module;
use rustc_hash::FxHashMap;
use std::sync::Arc;

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

    /// IO submission sender (set by reactor on start, used by Interpreter for NativeCallResult::Suspend)
    pub io_submit_tx: Mutex<Option<Sender<IoSubmission>>>,

    /// Metadata store for Reflect API (WeakMap-style storage)
    pub metadata: Mutex<MetadataStore>,

    /// Class metadata registry for reflection (field names, method names)
    /// Populated when --emit-reflection is used
    pub class_metadata: RwLock<ClassMetadataRegistry>,

    /// External native call handler (stdlib implementation)
    pub native_handler: Arc<dyn NativeHandler>,

    /// Resolved native functions for ModuleNativeCall dispatch
    pub resolved_natives: RwLock<ResolvedNatives>,

    /// Native function registry for linking module native calls at load time
    pub native_registry: RwLock<NativeFunctionRegistry>,

    /// Module registry for loaded bytecode modules
    pub module_registry: RwLock<ModuleRegistry>,

    /// Maximum consecutive preemptions before killing a task (infinite loop detection).
    /// Default: 1000. Set lower (e.g. 100) for faster infinite loop detection in tests.
    pub max_preemptions: u32,

    /// Preemption threshold in milliseconds (how long a task runs before being preempted).
    /// Default: 10ms.
    pub preempt_threshold_ms: u64,
}

impl SharedVmState {
    /// Create new shared VM state with default (no-op) native handler
    pub fn new(
        safepoint: Arc<SafepointCoordinator>,
        tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        injector: Arc<Injector<Arc<Task>>>,
    ) -> Self {
        Self::with_native_handler(safepoint, tasks, injector, Arc::new(NoopNativeHandler))
    }

    /// Create new shared VM state with a custom native handler
    pub fn with_native_handler(
        safepoint: Arc<SafepointCoordinator>,
        tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        injector: Arc<Injector<Arc<Task>>>,
        native_handler: Arc<dyn NativeHandler>,
    ) -> Self {
        Self {
            gc: Mutex::new(GarbageCollector::default()),
            classes: RwLock::new(ClassRegistry::new()),
            globals: RwLock::new(FxHashMap::default()),
            globals_by_index: RwLock::new(Vec::new()),
            safepoint,
            tasks,
            injector,
            mutex_registry: MutexRegistry::new(),
            io_submit_tx: Mutex::new(None),
            metadata: Mutex::new(MetadataStore::new()),
            class_metadata: RwLock::new(ClassMetadataRegistry::new()),
            native_handler,
            resolved_natives: RwLock::new(ResolvedNatives::empty()),
            native_registry: RwLock::new(NativeFunctionRegistry::new()),
            module_registry: RwLock::new(ModuleRegistry::new()),
            max_preemptions: 1000,
            preempt_threshold_ms: 10,
        }
    }

    /// Link a module's native function table against the registry.
    /// Must be called before executing a module that uses ModuleNativeCall.
    pub fn link_module_natives(&self, module: &Module) -> Result<(), String> {
        if module.native_functions.is_empty() {
            return Ok(());
        }
        let registry = self.native_registry.read();
        let resolved = ResolvedNatives::link(&module.native_functions, &registry)?;
        *self.resolved_natives.write() = resolved;
        Ok(())
    }

    /// Register classes from a module
    pub fn register_classes(&self, module: &Module) {
        let mut classes = self.classes.write();
        for (i, class_def) in module.classes.iter().enumerate() {
            let mut class = if let Some(parent_id) = class_def.parent_id {
                let mut c = crate::vm::object::Class::with_parent(
                    i,
                    class_def.name.clone(),
                    class_def.field_count,
                    parent_id as usize,
                );
                // Inherit parent vtable entries
                if let Some(parent) = classes.get_class(parent_id as usize) {
                    for &method_id in &parent.vtable.methods {
                        c.add_method(method_id);
                    }
                }
                c
            } else {
                crate::vm::object::Class::new(i, class_def.name.clone(), class_def.field_count)
            };

            // Add/override methods by vtable slot index
            for method in &class_def.methods {
                if method.slot < class.vtable.methods.len() {
                    // Override inherited method at same slot
                    class.vtable.methods[method.slot] = method.function_id;
                } else {
                    // New method, append to vtable
                    class.add_method(method.function_id);
                }
            }

            classes.register_class(class);
        }
    }

    /// Register a module: add to module registry, register classes, link natives.
    ///
    /// This is the canonical way to make a module available for execution.
    pub fn register_module(&self, module: Arc<Module>) -> Result<(), String> {
        // Register in module registry (deduplicates by checksum)
        self.module_registry.write().register(module.clone())?;

        // Register classes from the module
        self.register_classes(&module);

        // Link native function table
        self.link_module_natives(&module)?;

        Ok(())
    }
}
