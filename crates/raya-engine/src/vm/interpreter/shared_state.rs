//! Shared VM state for concurrent task execution
//!
//! This module provides shared state that can be safely accessed by multiple
//! worker threads executing tasks concurrently.

use crate::compiler::Module;
use crate::vm::gc::GarbageCollector;
use crate::vm::interpreter::{ClassRegistry, ModuleRegistry, SafepointCoordinator};
use crate::vm::native_handler::{NativeHandler, NoopNativeHandler};
use crate::vm::native_registry::{NativeFunctionRegistry, ResolvedNatives};
use crate::vm::reflect::{ClassMetadata, ClassMetadataRegistry, MetadataStore};
use crate::vm::scheduler::{IoSubmission, StackPool, Task, TaskId};
use crate::vm::sync::MutexRegistry;
use crate::vm::value::Value;
use crossbeam::channel::Sender;
use crossbeam_deque::Injector;
use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;
#[cfg(feature = "jit")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Promise-related microtasks processed by scheduler checkpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromiseMicrotask {
    /// Report a task rejection if still unhandled at checkpoint drain time.
    ReportUnhandledRejection(TaskId),
}

#[cfg(feature = "jit")]
#[derive(Default)]
pub struct JitTelemetry {
    pub call_samples: AtomicU64,
    pub loop_samples: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub compile_requests_submitted: AtomicU64,
    pub compile_requests_dropped: AtomicU64,
    pub resume_native_ok: AtomicU64,
    pub resume_native_reject: AtomicU64,
    pub resume_preemption_ok: AtomicU64,
    pub resume_preemption_reject: AtomicU64,
}

#[cfg(feature = "jit")]
#[derive(Debug, Clone, Copy, Default)]
pub struct JitTelemetrySnapshot {
    pub call_samples: u64,
    pub loop_samples: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub compile_requests_submitted: u64,
    pub compile_requests_dropped: u64,
    pub resume_native_ok: u64,
    pub resume_native_reject: u64,
    pub resume_preemption_ok: u64,
    pub resume_preemption_reject: u64,
}

#[cfg(feature = "jit")]
impl JitTelemetry {
    pub fn snapshot(&self) -> JitTelemetrySnapshot {
        JitTelemetrySnapshot {
            call_samples: self.call_samples.load(Ordering::Relaxed),
            loop_samples: self.loop_samples.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            compile_requests_submitted: self.compile_requests_submitted.load(Ordering::Relaxed),
            compile_requests_dropped: self.compile_requests_dropped.load(Ordering::Relaxed),
            resume_native_ok: self.resume_native_ok.load(Ordering::Relaxed),
            resume_native_reject: self.resume_native_reject.load(Ordering::Relaxed),
            resume_preemption_ok: self.resume_preemption_ok.load(Ordering::Relaxed),
            resume_preemption_reject: self.resume_preemption_reject.load(Ordering::Relaxed),
        }
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

    /// Promise microtask queue (FIFO), drained at scheduler checkpoints.
    pub promise_microtasks: Mutex<std::collections::VecDeque<PromiseMicrotask>>,

    /// Global task injector for scheduling
    pub injector: Arc<Injector<Arc<Task>>>,

    /// Mutex registry for task synchronization
    pub mutex_registry: MutexRegistry,

    /// Stack pool for reusing Stack allocations across task lifetimes
    pub stack_pool: StackPool,

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

    /// Debug state for debugger coordination (None = no debugger attached)
    pub debug_state: Mutex<Option<Arc<super::debug_state::DebugState>>>,

    /// Maximum consecutive preemptions before killing a task (infinite loop detection).
    /// Default: 1000. Set lower (e.g. 100) for faster infinite loop detection in tests.
    pub max_preemptions: u32,

    /// Preemption threshold in milliseconds (how long a task runs before being preempted).
    /// Default: 10ms.
    pub preempt_threshold_ms: u64,

    /// CPU/wall-clock profiler — shared with interpreter threads for sampling.
    /// Set by `Vm::enable_profiling()`, cloned by worker threads.
    pub profiler: Mutex<Option<Arc<crate::profiler::Profiler>>>,

    /// JIT code cache — shared with interpreter threads for native dispatch.
    /// Set once by `Vm::enable_jit()`, then read by interpreter threads.
    #[cfg(feature = "jit")]
    pub code_cache: Mutex<Option<Arc<crate::jit::runtime::code_cache::CodeCache>>>,

    /// Per-module profiling data for hot function detection.
    /// Keyed by module checksum. Created when a module is first executed with JIT enabled.
    #[cfg(feature = "jit")]
    pub module_profiles:
        RwLock<FxHashMap<[u8; 32], Arc<crate::jit::profiling::counters::ModuleProfile>>>,

    /// Handle to the background JIT compilation thread.
    /// Set by `Vm::enable_jit()`, cloned by worker threads to submit compilation requests.
    #[cfg(feature = "jit")]
    pub background_compiler: Mutex<Option<Arc<crate::jit::profiling::BackgroundCompiler>>>,

    /// Compilation policy thresholds shared with interpreter workers.
    #[cfg(feature = "jit")]
    pub jit_compilation_policy: Mutex<crate::jit::profiling::policy::CompilationPolicy>,

    /// Lightweight counters for JIT activity and dispatch behavior.
    #[cfg(feature = "jit")]
    pub jit_telemetry: Arc<JitTelemetry>,
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
            promise_microtasks: Mutex::new(std::collections::VecDeque::new()),
            injector,
            mutex_registry: MutexRegistry::new(),
            stack_pool: StackPool::new(num_cpus::get() * 2),
            io_submit_tx: Mutex::new(None),
            metadata: Mutex::new(MetadataStore::new()),
            class_metadata: RwLock::new(ClassMetadataRegistry::new()),
            native_handler,
            resolved_natives: RwLock::new(ResolvedNatives::empty()),
            native_registry: RwLock::new(NativeFunctionRegistry::new()),
            module_registry: RwLock::new(ModuleRegistry::new()),
            debug_state: Mutex::new(None),
            max_preemptions: crate::vm::defaults::DEFAULT_MAX_PREEMPTIONS,
            preempt_threshold_ms: crate::vm::defaults::DEFAULT_PREEMPT_THRESHOLD_MS,
            profiler: Mutex::new(None),
            #[cfg(feature = "jit")]
            code_cache: Mutex::new(None),
            #[cfg(feature = "jit")]
            module_profiles: RwLock::new(FxHashMap::default()),
            #[cfg(feature = "jit")]
            background_compiler: Mutex::new(None),
            #[cfg(feature = "jit")]
            jit_compilation_policy: Mutex::new(
                crate::jit::profiling::policy::CompilationPolicy::default(),
            ),
            #[cfg(feature = "jit")]
            jit_telemetry: Arc::new(JitTelemetry::default()),
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
        let mut class_metadata_registry = self.class_metadata.write();
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

            // Pre-size vtable to accommodate all slots (including gaps from abstract methods)
            if let Some(max_slot) = class_def.methods.iter().map(|m| m.slot + 1).max() {
                while class.vtable.methods.len() < max_slot {
                    class.add_method(usize::MAX); // sentinel for abstract/unimplemented slots
                }
            }

            // Place methods at their correct vtable slots
            for method in &class_def.methods {
                class.vtable.methods[method.slot] = method.function_id;
            }

            classes.register_class(class);

            // Populate reflection metadata for runtime field/method lookups.
            // Reflection data is always emitted by codegen for bytecode modules.
            if let Some(class_reflection) =
                module.reflection.as_ref().and_then(|r| r.classes.get(i))
            {
                let mut class_meta = ClassMetadata::new();

                for (field_index, field) in class_reflection.fields.iter().enumerate() {
                    if field.is_static {
                        class_meta.add_static_field(field.name.clone(), field_index);
                    } else {
                        let type_id = reflect_type_name_to_id(&field.type_name);
                        class_meta.add_field_with_type(field.name.clone(), field_index, type_id);
                    }
                }

                for (method_index, method_name) in class_reflection.method_names.iter().enumerate()
                {
                    class_meta.add_method(method_name.clone(), method_index);
                }

                for (static_index, static_name) in
                    class_reflection.static_field_names.iter().enumerate()
                {
                    class_meta.add_static_field(static_name.clone(), static_index);
                }

                class_metadata_registry.register(i, class_meta);
            }
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

/// Convert a `FieldReflectionData.type_name` string back to the u32 compiler TypeId it originated from.
///
/// The mapping mirrors `IrCodeGenerator::get_type_name()`:
/// 0=number, 1=string, 2=boolean, 3=null, 4=void, 5=never, 6=unknown, 16=int.
/// Generic class TypeIds are serialised as "type#N" and parsed back here.
fn reflect_type_name_to_id(type_name: &str) -> u32 {
    match type_name {
        "number" => 0,
        "string" => 1,
        "boolean" => 2,
        "null" => 3,
        "void" => 4,
        "never" => 5,
        "unknown" => 6,
        "int" => 16,
        s if s.starts_with("type#") => s[5..].parse().unwrap_or(0),
        _ => 0,
    }
}
