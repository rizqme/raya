//! Shared VM state for concurrent task execution
//!
//! This module provides shared state that can be safely accessed by multiple
//! worker threads executing tasks concurrently.

use crate::compiler::Module;
use crate::compiler::Opcode;
use crate::vm::gc::GarbageCollector;
use crate::vm::interpreter::{
    ClassRegistry, ModuleRegistry, RuntimeLayoutRegistry, SafepointCoordinator,
};
use crate::vm::native_handler::{NativeHandler, NoopNativeHandler};
use crate::vm::native_registry::{NativeFunctionRegistry, ResolvedNatives};
use crate::vm::reflect::{ClassMetadata, ClassMetadataRegistry, MetadataStore};
use crate::vm::scheduler::{IoSubmission, StackPool, Task, TaskId};
use crate::vm::sync::{MutexRegistry, SemaphoreRegistry};
use crate::vm::value::Value;
use crossbeam::channel::Sender;
use crossbeam_deque::Injector;
use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Promise-related microtasks processed by scheduler checkpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromiseMicrotask {
    /// Report a task rejection if still unhandled at checkpoint drain time.
    ///
    /// The countdown provides a grace turn so user code can attach an awaiter/
    /// handler after the task fails but before we surface it as unhandled.
    ReportUnhandledRejection(TaskId, u8),
}

/// Runtime layout assigned to a registered module.
#[derive(Debug, Clone)]
pub struct ModuleRuntimeLayout {
    /// Module identity checksum.
    pub checksum: [u8; 32],
    /// Module-local global slots are rebased to this absolute start index.
    pub global_base: usize,
    /// Number of module-local global slots reserved.
    pub global_len: usize,
    /// Module-local nominal type IDs are rebased by this absolute base.
    pub nominal_type_base: usize,
    /// Number of nominal types registered from this module.
    pub nominal_type_len: usize,
    /// Resolved native function dispatch table for this module.
    pub resolved_natives: ResolvedNatives,
    /// Whether module-level init has been executed in this VM.
    pub initialized: bool,
}

/// Structural slot binding for cross-type field/method access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralSlotBinding {
    /// Expected slot maps to a concrete object field slot.
    Field(usize),
    /// Expected slot maps to a class vtable method slot.
    Method(usize),
    /// Expected slot maps to a dynamic property key on the object's dyn lane.
    Dynamic(PropKeyId),
    /// Expected slot is not present on the provider type.
    Missing,
}

/// Cached structural adapter from a provider layout to a required shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeAdapter {
    pub provider_layout: LayoutId,
    pub required_shape: ShapeId,
    pub field_map: Vec<Option<usize>>,
    pub method_map: Vec<Option<usize>>,
    pub dynamic_key_map: Vec<Option<PropKeyId>>,
    pub epoch: u32,
}

impl ShapeAdapter {
    pub fn from_slot_map(
        provider_layout: LayoutId,
        required_shape: ShapeId,
        slot_map: &[StructuralSlotBinding],
        epoch: u32,
    ) -> Self {
        let mut field_map = Vec::with_capacity(slot_map.len());
        let mut method_map = Vec::with_capacity(slot_map.len());
        let mut dynamic_key_map = Vec::with_capacity(slot_map.len());
        for binding in slot_map {
            match binding {
                StructuralSlotBinding::Field(slot) => {
                    field_map.push(Some(*slot));
                    method_map.push(None);
                    dynamic_key_map.push(None);
                }
                StructuralSlotBinding::Method(slot) => {
                    field_map.push(None);
                    method_map.push(Some(*slot));
                    dynamic_key_map.push(None);
                }
                StructuralSlotBinding::Dynamic(key) => {
                    field_map.push(None);
                    method_map.push(None);
                    dynamic_key_map.push(Some(*key));
                }
                StructuralSlotBinding::Missing => {
                    field_map.push(None);
                    method_map.push(None);
                    dynamic_key_map.push(None);
                }
            }
        }
        Self {
            provider_layout,
            required_shape,
            field_map,
            method_map,
            dynamic_key_map,
            epoch,
        }
    }

    pub fn binding_for_slot(&self, expected_slot: usize) -> StructuralSlotBinding {
        if let Some(Some(slot)) = self.field_map.get(expected_slot) {
            return StructuralSlotBinding::Field(*slot);
        }
        if let Some(Some(slot)) = self.method_map.get(expected_slot) {
            return StructuralSlotBinding::Method(*slot);
        }
        if let Some(Some(key)) = self.dynamic_key_map.get(expected_slot) {
            return StructuralSlotBinding::Dynamic(*key);
        }
        StructuralSlotBinding::Missing
    }

    pub fn len(&self) -> usize {
        self.field_map
            .len()
            .max(self.method_map.len())
            .max(self.dynamic_key_map.len())
    }

    pub fn is_identity_field_projection(&self) -> bool {
        self.field_map
            .iter()
            .enumerate()
            .all(|(expected, binding)| binding == &Some(expected))
            && self.method_map.iter().all(|binding| binding.is_none())
            && self.dynamic_key_map.iter().all(|binding| binding.is_none())
    }
}

/// Stable layout identity used by structural adapter cache.
pub type LayoutId = crate::vm::object::LayoutId;
/// Stable structural shape identity.
pub type ShapeId = crate::vm::object::ShapeId;
/// Stable runtime type-handle identity.
pub type TypeHandleId = crate::vm::object::TypeHandleId;
/// Stable nominal runtime type identity.
pub type NominalTypeId = crate::vm::object::NominalTypeId;
/// Stable interned property-key identity.
pub type PropKeyId = crate::vm::object::PropKeyId;

/// Runtime-local property key interner for `Object::dyn_map`.
#[derive(Debug, Default)]
pub struct PropertyKeyRegistry {
    next_id: PropKeyId,
    by_name: FxHashMap<String, PropKeyId>,
    by_id: FxHashMap<PropKeyId, String>,
}

impl PropertyKeyRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            by_name: FxHashMap::default(),
            by_id: FxHashMap::default(),
        }
    }

    pub fn intern(&mut self, name: &str) -> PropKeyId {
        if let Some(&id) = self.by_name.get(name) {
            return id;
        }
        let id = self.next_id.max(1);
        self.next_id = self.next_id.saturating_add(1).max(1);
        let owned = name.to_string();
        self.by_name.insert(owned.clone(), id);
        self.by_id.insert(id, owned);
        id
    }

    pub fn resolve(&self, id: PropKeyId) -> Option<&str> {
        self.by_id.get(&id).map(String::as_str)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TypeHandleKey {
    nominal_type_id: NominalTypeId,
    layout_id: LayoutId,
    shape_id: Option<ShapeId>,
}

/// Runtime-owned entry for imported/exported constructor handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeHandleEntry {
    pub handle_id: TypeHandleId,
    pub nominal_type_id: NominalTypeId,
    pub layout_id: LayoutId,
    pub shape_id: Option<ShapeId>,
}

#[derive(Debug, Default)]
pub struct RuntimeTypeHandleRegistry {
    next_id: TypeHandleId,
    entries: FxHashMap<TypeHandleId, TypeHandleEntry>,
    reverse: FxHashMap<TypeHandleKey, TypeHandleId>,
}

impl RuntimeTypeHandleRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entries: FxHashMap::default(),
            reverse: FxHashMap::default(),
        }
    }

    pub fn register(
        &mut self,
        nominal_type_id: NominalTypeId,
        layout_id: LayoutId,
        shape_id: Option<ShapeId>,
    ) -> TypeHandleId {
        let key = TypeHandleKey {
            nominal_type_id,
            layout_id,
            shape_id,
        };
        if let Some(&existing) = self.reverse.get(&key) {
            return existing;
        }

        let handle_id = self.next_id.max(1);
        self.next_id = self.next_id.saturating_add(1).max(1);
        let entry = TypeHandleEntry {
            handle_id,
            nominal_type_id,
            layout_id,
            shape_id,
        };
        self.entries.insert(handle_id, entry);
        self.reverse.insert(key, handle_id);
        handle_id
    }

    pub fn get(&self, handle_id: TypeHandleId) -> Option<TypeHandleEntry> {
        self.entries.get(&handle_id).copied()
    }
}

/// Structural adapter cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructuralAdapterKey {
    pub provider_layout: LayoutId,
    pub required_shape: ShapeId,
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

    /// Physical runtime layout registry for nominal and structural object storage.
    pub layouts: RwLock<RuntimeLayoutRegistry>,

    /// Global variables by name
    pub globals: RwLock<FxHashMap<String, Value>>,

    /// Global variables by index (for static fields)
    pub globals_by_index: RwLock<Vec<Value>>,

    /// Named ambient builtin globals stored as roots in `globals_by_index`.
    /// Maps builtin name -> global slot index.
    pub builtin_global_slots: RwLock<FxHashMap<String, usize>>,

    /// VM-local interned string constants keyed by module checksum and constant index.
    pub constant_string_cache: RwLock<FxHashMap<([u8; 32], usize), Value>>,

    /// Freshly allocated values rooted only until they are published into a
    /// stable root set such as task state or a shared cache.
    pub ephemeral_gc_roots: RwLock<Vec<Value>>,

    /// Safepoint coordinator
    pub safepoint: Arc<SafepointCoordinator>,

    /// Task registry
    pub tasks: Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,

    /// Promise microtask queue (FIFO), drained at scheduler checkpoints.
    pub promise_microtasks: Mutex<std::collections::VecDeque<PromiseMicrotask>>,

    /// Whether the reactor has reached a quiescent checkpoint with no
    /// internally queued work.
    pub reactor_is_quiescent: AtomicBool,

    /// Bumped whenever the reactor quiescence state changes. This lets callers
    /// wait for a stable quiescent window instead of observing a transient
    /// quiescent bool between local queue transitions.
    pub reactor_quiescent_epoch: AtomicU64,

    /// Global task injector for scheduling
    pub injector: Arc<Injector<Arc<Task>>>,

    /// Mutex registry for task synchronization
    pub mutex_registry: MutexRegistry,

    /// Semaphore registry for task synchronization
    pub semaphore_registry: SemaphoreRegistry,

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

    /// Per-module runtime layouts (globals/classes/natives/init state).
    pub module_layouts: RwLock<FxHashMap<[u8; 32], ModuleRuntimeLayout>>,

    /// Shared adapter cache keyed by provider layout + required structural shape.
    /// Value: cached adapter with split field/method maps.
    pub structural_shape_adapters: RwLock<FxHashMap<StructuralAdapterKey, Arc<ShapeAdapter>>>,

    /// Canonical member names keyed by structural shape id.
    pub structural_shape_names: RwLock<FxHashMap<ShapeId, Vec<String>>>,

    /// Structural layout shapes keyed by physical layout ID.
    /// Stores canonical slot names for structural object carriers so expected
    /// structural views can be remapped by name across call boundaries.
    pub structural_layout_shapes: RwLock<FxHashMap<LayoutId, Vec<String>>>,

    /// Runtime-owned constructor/type handles used for imported/exported nominal types.
    pub type_handles: RwLock<RuntimeTypeHandleRegistry>,

    /// Runtime-local property key interner for dynamic object lanes.
    pub prop_keys: RwLock<PropertyKeyRegistry>,

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

    /// Offline AOT profile collector populated from interpreter execution.
    pub aot_profile: RwLock<crate::aot_profile::AotProfileCollector>,

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
            layouts: RwLock::new(RuntimeLayoutRegistry::new()),
            globals: RwLock::new(FxHashMap::default()),
            globals_by_index: RwLock::new(Vec::new()),
            builtin_global_slots: RwLock::new(FxHashMap::default()),
            constant_string_cache: RwLock::new(FxHashMap::default()),
            ephemeral_gc_roots: RwLock::new(Vec::new()),
            safepoint,
            tasks,
            promise_microtasks: Mutex::new(std::collections::VecDeque::new()),
            reactor_is_quiescent: AtomicBool::new(false),
            reactor_quiescent_epoch: AtomicU64::new(0),
            injector,
            mutex_registry: MutexRegistry::new(),
            semaphore_registry: SemaphoreRegistry::new(),
            stack_pool: StackPool::new(num_cpus::get() * 2),
            io_submit_tx: Mutex::new(None),
            metadata: Mutex::new(MetadataStore::new()),
            class_metadata: RwLock::new(ClassMetadataRegistry::new()),
            native_handler,
            resolved_natives: RwLock::new(ResolvedNatives::empty()),
            native_registry: RwLock::new(NativeFunctionRegistry::new()),
            module_registry: RwLock::new(ModuleRegistry::new()),
            module_layouts: RwLock::new(FxHashMap::default()),
            structural_shape_adapters: RwLock::new(FxHashMap::default()),
            structural_shape_names: RwLock::new(FxHashMap::default()),
            structural_layout_shapes: RwLock::new(FxHashMap::default()),
            type_handles: RwLock::new(RuntimeTypeHandleRegistry::new()),
            prop_keys: RwLock::new(PropertyKeyRegistry::new()),
            debug_state: Mutex::new(None),
            max_preemptions: crate::vm::defaults::DEFAULT_MAX_PREEMPTIONS,
            preempt_threshold_ms: crate::vm::defaults::DEFAULT_PREEMPT_THRESHOLD_MS,
            profiler: Mutex::new(None),
            aot_profile: RwLock::new(crate::aot_profile::AotProfileCollector::default()),
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

    pub fn set_reactor_quiescent(&self, quiescent: bool) {
        let previous = self.reactor_is_quiescent.swap(quiescent, Ordering::AcqRel);
        if previous != quiescent {
            self.reactor_quiescent_epoch.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Snapshot heap roots reachable from globals and live tasks.
    pub fn collect_gc_roots(&self) -> crate::vm::gc::ExternalRootSnapshot {
        let mut roots = Vec::new();
        let mut complete = true;

        {
            let globals = self.globals.read();
            roots.extend(globals.values().copied().filter(|value| value.is_heap_allocated()));
        }

        {
            let globals = self.globals_by_index.read();
            roots.extend(globals.iter().copied().filter(|value| value.is_heap_allocated()));
        }

        {
            let cached = self.constant_string_cache.read();
            roots.extend(cached.values().copied().filter(|value| value.is_heap_allocated()));
        }

        {
            let ephemeral = self.ephemeral_gc_roots.read();
            roots.extend(ephemeral.iter().copied().filter(|value| value.is_heap_allocated()));
        }

        {
            let tasks = self.tasks.read();
            for task in tasks.values() {
                let (task_roots, task_complete) = task.gc_roots();
                roots.extend(task_roots);
                complete &= task_complete;
            }
        }

        crate::vm::gc::ExternalRootSnapshot { roots, complete }
    }

    /// Intern a bytecode string constant once per VM and keep it rooted in shared state.
    pub fn intern_constant_string(&self, module: &Module, index: usize, value: &str) -> Value {
        let key = (module.checksum, index);
        let cached = {
            let cache = self.constant_string_cache.read();
            cache.get(&key).copied()
        };
        if let Some(cached) = cached {
            return cached;
        }

        let interned = self.allocate_ephemerally_rooted_string(value.to_owned());

        let mut cache = self.constant_string_cache.write();
        let published = *cache.entry(key).or_insert(interned);
        self.release_ephemeral_gc_root(interned);
        published
    }

    pub fn allocate_ephemerally_rooted_string(&self, value: String) -> Value {
        let mut gc = self.gc.lock();
        let gc_ptr = gc.allocate(crate::vm::object::RayaString::new(value));
        let rooted = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        self.ephemeral_gc_roots.write().push(rooted);
        rooted
    }

    pub fn release_ephemeral_gc_root(&self, value: Value) {
        let mut ephemeral = self.ephemeral_gc_roots.write();
        if let Some(index) = ephemeral.iter().rposition(|candidate| *candidate == value) {
            ephemeral.swap_remove(index);
        }
    }

    /// Link a module's native function table against the registry.
    /// Must be called before executing a module that uses ModuleNativeCall.
    pub fn link_module_natives(&self, module: &Module) -> Result<(), String> {
        let resolved = self.resolve_module_natives(module)?;
        if let Some(layout) = self.module_layouts.write().get_mut(&module.checksum) {
            layout.resolved_natives = resolved;
        }
        Ok(())
    }

    fn resolve_module_natives(&self, module: &Module) -> Result<ResolvedNatives, String> {
        if module.native_functions.is_empty() {
            return Ok(ResolvedNatives::empty());
        }
        let registry = self.native_registry.read();
        ResolvedNatives::link(&module.native_functions, &registry)
    }

    fn module_global_slot_count(module: &Module) -> usize {
        let function_slots = module
            .functions
            .iter()
            .map(Self::function_global_slot_count)
            .max()
            .unwrap_or(0);
        let import_slots = module
            .imports
            .iter()
            .filter_map(|import| import.runtime_global_slot.map(|slot| slot as usize + 1))
            .max()
            .unwrap_or(0);
        function_slots.max(import_slots)
    }

    fn function_global_slot_count(function: &crate::compiler::Function) -> usize {
        let code = &function.code;
        let mut ip = 0usize;
        let mut max_slot = 0usize;

        while ip < code.len() {
            let op = code[ip];
            ip += 1;
            let Some(opcode) = Opcode::from_u8(op) else {
                continue;
            };
            match opcode {
                Opcode::LoadGlobal | Opcode::StoreGlobal => {
                    if ip + 4 <= code.len() {
                        let slot = u32::from_le_bytes([
                            code[ip],
                            code[ip + 1],
                            code[ip + 2],
                            code[ip + 3],
                        ]) as usize;
                        max_slot = max_slot.max(slot + 1);
                    }
                    ip += 4.min(code.len().saturating_sub(ip));
                }
                _ => {
                    let operand_len =
                        crate::compiler::codegen::emit::opcode_size(opcode).saturating_sub(1);
                    ip += operand_len.min(code.len().saturating_sub(ip));
                }
            }
        }

        max_slot
    }

    /// Resolve the absolute global slot for a module-local global index.
    pub fn resolve_global_slot(&self, module: &Module, local_slot: usize) -> usize {
        self.module_layouts
            .read()
            .get(&module.checksum)
            .map(|layout| layout.global_base + local_slot)
            .unwrap_or(local_slot)
    }

    /// Store or update an ambient builtin global value.
    /// Values are rooted through `globals_by_index` to keep GC visibility.
    pub fn set_builtin_global(&self, name: impl Into<String>, value: Value) {
        let name = name.into();
        let mut slots = self.builtin_global_slots.write();
        let mut globals = self.globals_by_index.write();
        if let Some(&slot) = slots.get(&name) {
            if slot < globals.len() {
                globals[slot] = value;
            } else {
                globals.resize(slot + 1, Value::null());
                globals[slot] = value;
            }
            return;
        }
        let slot = globals.len();
        globals.push(value);
        slots.insert(name, slot);
    }

    /// Load an ambient builtin global value by name.
    pub fn get_builtin_global(&self, name: &str) -> Option<Value> {
        let slot = self.builtin_global_slots.read().get(name).copied()?;
        self.globals_by_index.read().get(slot).copied()
    }

    /// Resolve the absolute nominal type ID for a module-local nominal type ID.
    pub fn resolve_nominal_type_id(
        &self,
        module: &Module,
        local_nominal_type_id: usize,
    ) -> Option<usize> {
        let layout = self.module_layouts.read().get(&module.checksum)?.clone();
        (local_nominal_type_id < layout.nominal_type_len)
            .then_some(layout.nominal_type_base + local_nominal_type_id)
    }

    /// Fetch resolved native table for a module checksum.
    pub fn resolved_natives_for_module(&self, module: &Module) -> ResolvedNatives {
        self.module_layouts
            .read()
            .get(&module.checksum)
            .map(|layout| layout.resolved_natives.clone())
            .unwrap_or_else(ResolvedNatives::empty)
    }

    /// Register a runtime-owned constructor/type handle.
    pub fn register_type_handle(
        &self,
        nominal_type_id: NominalTypeId,
        layout_id: LayoutId,
        shape_id: Option<ShapeId>,
    ) -> TypeHandleId {
        self.type_handles
            .write()
            .register(nominal_type_id, layout_id, shape_id)
    }

    /// Resolve a runtime-owned constructor/type handle.
    pub fn resolve_type_handle(&self, handle_id: TypeHandleId) -> Option<TypeHandleEntry> {
        self.type_handles.read().get(handle_id)
    }

    /// Intern a dynamic property name.
    pub fn intern_prop_key(&self, name: &str) -> PropKeyId {
        self.prop_keys.write().intern(name)
    }

    /// Resolve an interned property key back to its string name.
    pub fn prop_key_name(&self, key: PropKeyId) -> Option<String> {
        self.prop_keys.read().resolve(key).map(str::to_string)
    }

    /// Register a structural slot view for object access in `module`.
    /// The map translates consumer slot indices into provider slot indices.
    pub fn register_structural_shape_adapter(
        &self,
        provider_layout: LayoutId,
        required_shape: ShapeId,
        slot_map: Vec<StructuralSlotBinding>,
    ) {
        if slot_map.is_empty() {
            return;
        }
        let adapter_key = StructuralAdapterKey {
            provider_layout,
            required_shape,
        };
        let adapter = Arc::new(ShapeAdapter::from_slot_map(
            provider_layout,
            required_shape,
            &slot_map,
            0,
        ));
        self.structural_shape_adapters
            .write()
            .insert(adapter_key, adapter);
    }

    /// Register canonical member names for a structural shape id.
    pub fn register_structural_shape_names(&self, shape_id: ShapeId, member_names: &[String]) {
        if member_names.is_empty() {
            return;
        }
        self.structural_shape_names
            .write()
            .entry(shape_id)
            .or_insert_with(|| member_names.to_vec());
    }

    /// Register canonical member names for a physical structural layout.
    ///
    /// This keeps the dedicated structural-layout cache and the runtime layout
    /// registry in sync so later layout-based queries do not need to infer
    /// structure through nominal class metadata.
    pub fn register_structural_layout_shape(&self, layout_id: LayoutId, member_names: &[String]) {
        if layout_id == 0 {
            return;
        }
        self.structural_layout_shapes
            .write()
            .entry(layout_id)
            .or_insert_with(|| member_names.to_vec());
        self.layouts
            .write()
            .register_layout_shape(layout_id, member_names);
        self.invalidate_jit_for_layout(layout_id);
    }

    /// Resolve canonical member names for a physical structural layout.
    pub fn structural_layout_names(&self, layout_id: LayoutId) -> Option<Vec<String>> {
        if let Some(names) = self
            .layouts
            .read()
            .layout_field_names(layout_id)
            .map(|names| names.to_vec())
        {
            return Some(names);
        }
        self.structural_layout_shapes
            .read()
            .get(&layout_id)
            .cloned()
    }

    /// Resolve canonical layout member names for an object, lazily seeding
    /// builtin nominal layouts into the layout registry when needed.
    pub fn layout_field_names_for_object(
        &self,
        object: &crate::vm::object::Object,
    ) -> Option<Vec<String>> {
        if let Some(names) = self.structural_layout_names(object.layout_id()) {
            return Some(names);
        }
        crate::vm::object::global_layout_names(object.layout_id())
    }

    /// Record physical layout metadata for a nominal runtime type.
    pub fn register_nominal_layout(
        &self,
        nominal_type_id: usize,
        layout_id: LayoutId,
        field_count: usize,
        name: impl Into<Option<String>>,
    ) {
        self.layouts
            .write()
            .register_nominal_layout(nominal_type_id, layout_id, field_count, name);
        if layout_id != 0 {
            self.invalidate_jit_for_layout(layout_id);
        }
    }

    /// Resolve the physical layout ID for a nominal runtime type.
    pub fn nominal_layout_id(&self, nominal_type_id: usize) -> Option<LayoutId> {
        self.layouts.read().nominal_layout_id(nominal_type_id)
    }

    /// Allocate one fresh nominal object layout ID.
    pub fn allocate_nominal_layout_id(&self) -> LayoutId {
        self.layouts.write().allocate_nominal_layout_id()
    }

    /// Register a runtime class after ensuring it has an assigned nominal layout.
    pub fn register_runtime_class(&self, class: crate::vm::object::Class) -> usize {
        self.register_runtime_class_with_layout_names(class, None::<&[&str]>)
    }

    pub fn register_runtime_class_with_layout_names(
        &self,
        class: crate::vm::object::Class,
        layout_names: impl Into<Option<&'static [&'static str]>>,
    ) -> usize {
        let layout_id = self.allocate_nominal_layout_id();
        let field_count = class.field_count;
        let class_name = class.name.clone();
        let id = self.classes.write().register_class(class);
        self.register_nominal_layout(id, layout_id, field_count, Some(class_name));
        if let Some(layout_names) = layout_names.into() {
            let owned_names = layout_names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>();
            self.register_structural_layout_shape(layout_id, &owned_names);
        }
        id
    }

    /// Resolve the physical allocation metadata for a nominal runtime type.
    pub fn nominal_allocation(&self, nominal_type_id: usize) -> Option<(LayoutId, usize)> {
        self.layouts.read().nominal_allocation(nominal_type_id)
    }

    /// Update instance field count metadata for a nominal runtime type.
    pub fn set_nominal_field_count(&self, nominal_type_id: usize, field_count: usize) -> bool {
        let layout_id = self.nominal_layout_id(nominal_type_id);
        let updated_layouts = self
            .layouts
            .write()
            .set_nominal_field_count(nominal_type_id, field_count);
        let updated_classes = self
            .classes
            .write()
            .set_nominal_field_count(nominal_type_id, field_count);
        if (updated_layouts || updated_classes) && layout_id.is_some() {
            self.invalidate_jit_for_layout(layout_id.unwrap());
        }
        updated_layouts || updated_classes
    }

    #[cfg(feature = "jit")]
    fn invalidate_jit_for_layout(&self, layout_id: LayoutId) {
        let affected = self
            .code_cache
            .lock()
            .as_ref()
            .map(|cache| cache.invalidate_layout(layout_id))
            .unwrap_or_default();
        if affected.is_empty() {
            return;
        }
        let profiles = self.module_profiles.read();
        for (checksum, func_index) in affected {
            if let Some(profile) = profiles.get(&checksum) {
                if let Some(func) = profile.get(func_index as usize) {
                    func.invalidate_compiled_code();
                }
            }
        }
    }

    #[cfg(not(feature = "jit"))]
    fn invalidate_jit_for_layout(&self, _layout_id: LayoutId) {}

    pub fn record_aot_call(&self, checksum: [u8; 32], func_index: u32) {
        self.aot_profile.write().record_call(checksum, func_index);
    }

    pub fn record_aot_loop(&self, checksum: [u8; 32], func_index: u32) {
        self.aot_profile.write().record_loop(checksum, func_index);
    }

    pub fn record_aot_layout_site(
        &self,
        checksum: [u8; 32],
        func_index: u32,
        bytecode_offset: u32,
        kind: crate::aot_profile::AotSiteKind,
        layout_id: LayoutId,
    ) {
        self.aot_profile.write().record_layout_site(
            checksum,
            func_index,
            bytecode_offset,
            kind,
            layout_id,
        );
    }

    pub fn snapshot_aot_profile(&self) -> crate::aot_profile::AotProfileData {
        self.aot_profile.read().snapshot()
    }

    /// Resolve canonical member names for a structural shape id.
    pub fn structural_shape_names(&self, shape_id: ShapeId) -> Option<Vec<String>> {
        self.structural_shape_names.read().get(&shape_id).cloned()
    }

    /// Mark a module as initialized.
    pub fn mark_module_initialized(&self, module: &Module) {
        if let Some(layout) = self.module_layouts.write().get_mut(&module.checksum) {
            layout.initialized = true;
        }
    }

    /// Check whether module top-level init has executed.
    pub fn is_module_initialized(&self, module: &Module) -> bool {
        self.module_layouts
            .read()
            .get(&module.checksum)
            .map(|layout| layout.initialized)
            .unwrap_or(false)
    }

    /// Register classes from a module
    pub fn register_classes(&self, module: &Arc<Module>, nominal_type_base: usize) {
        let mut classes = self.classes.write();
        let mut class_metadata_registry = self.class_metadata.write();
        for (i, class_def) in module.classes.iter().enumerate() {
            let global_nominal_type_id = nominal_type_base + i;
            let mut class = if let Some(parent_id) = class_def.parent_id {
                let mut c = crate::vm::object::Class::with_parent(
                    global_nominal_type_id,
                    class_def.name.clone(),
                    class_def.field_count,
                    nominal_type_base + parent_id as usize,
                );
                // Inherit parent vtable entries
                if let Some(parent) = classes.get_class(nominal_type_base + parent_id as usize) {
                    for &method_id in &parent.vtable.methods {
                        c.add_method(method_id);
                    }
                }
                c
            } else {
                crate::vm::object::Class::new(
                    global_nominal_type_id,
                    class_def.name.clone(),
                    class_def.field_count,
                )
            };
            class.module = Some(module.clone());

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

            // Constructors are lowered as dedicated functions named
            // "<ClassName>::constructor" (not regular vtable methods).
            // Prefer explicit bytecode export metadata when available so runtime
            // class registration does not depend on synthesized-name lookup.
            let exported_constructor_id = module.exports.iter().find_map(|export| {
                (matches!(export.symbol_type, crate::compiler::SymbolType::Class)
                    && export
                        .nominal_type
                        .is_some_and(|nominal| nominal.local_nominal_type_index as usize == i))
                .then_some(export.nominal_type.and_then(|nominal| nominal.constructor_function_index))
                .flatten()
                .map(|idx| idx as usize)
            });
            if let Some(constructor_id) = exported_constructor_id.or_else(|| {
                let constructor_name = format!("{}::constructor", class_def.name);
                module
                    .functions
                    .iter()
                    .position(|function| function.name == constructor_name)
            }) {
                class.set_constructor(constructor_id);
            }

            let layout_id = self.allocate_nominal_layout_id();
            classes.register_class(class);
            self.layouts.write().register_nominal_layout(
                global_nominal_type_id,
                layout_id,
                class_def.field_count,
                Some(class_def.name.clone()),
            );

            // Populate runtime metadata for dynamic property lookups.
            // Prefer rich reflection data when present, and always seed method slot names
            // from class defs so imported-class dynamic member calls remain callable.
            let mut class_meta = ClassMetadata::new();

            if let Some(class_reflection) =
                module.reflection.as_ref().and_then(|r| r.classes.get(i))
            {
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
            }

            for method in &class_def.methods {
                let plain_name = method
                    .name
                    .rsplit("::")
                    .next()
                    .unwrap_or(method.name.as_str())
                    .to_string();
                if !class_meta.has_method(&plain_name) {
                    class_meta.add_method(plain_name.clone(), method.slot);
                }
                if plain_name != method.name && !class_meta.has_method(&method.name) {
                    class_meta.add_method(method.name.clone(), method.slot);
                }
            }

            if !class_meta.method_names.is_empty()
                || !class_meta.field_names.is_empty()
                || !class_meta.static_field_names.is_empty()
            {
                class_metadata_registry.register(global_nominal_type_id, class_meta);
            }
        }
    }

    /// Register a module: add to module registry, register classes, link natives.
    ///
    /// This is the canonical way to make a module available for execution.
    pub fn register_module(&self, module: Arc<Module>) -> Result<(), String> {
        if self.module_layouts.read().contains_key(&module.checksum) {
            // Already registered in this VM.
            return Ok(());
        }

        // Register in module registry (deduplicates by checksum)
        self.module_registry.write().register(module.clone())?;

        // Allocate globals/class ID ranges and resolve module-native table.
        let global_len = Self::module_global_slot_count(&module);
        let global_base = {
            let mut globals = self.globals_by_index.write();
            let base = globals.len();
            if global_len > 0 {
                globals.resize(base + global_len, Value::null());
            }
            base
        };
        let nominal_type_len = module.classes.len();
        let nominal_type_base = self
            .classes
            .write()
            .reserve_nominal_type_range(nominal_type_len);
        let resolved_natives = self.resolve_module_natives(&module)?;

        self.module_layouts.write().insert(
            module.checksum,
            ModuleRuntimeLayout {
                checksum: module.checksum,
                global_base,
                global_len,
                nominal_type_base,
                nominal_type_len,
                resolved_natives: resolved_natives.clone(),
                initialized: false,
            },
        );

        for shape in &module.metadata.structural_shapes {
            if shape.member_names.is_empty() {
                continue;
            }
            let names = &shape.member_names;
            let shape_id = crate::vm::object::shape_id_from_member_names(names);
            self.register_structural_shape_names(shape_id, names);
            let layout_id = crate::vm::object::layout_id_from_ordered_names(names);
            self.register_structural_layout_shape(layout_id, names);
        }

        // Register classes from the module (rebased to global class IDs).
        self.register_classes(&module, nominal_type_base);

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

#[cfg(test)]
mod tests {
    use super::{
        PropertyKeyRegistry, RuntimeTypeHandleRegistry, ShapeAdapter, ShapeId,
        StructuralSlotBinding,
    };

    #[test]
    fn type_handle_registry_dedupes_equivalent_entries() {
        let mut registry = RuntimeTypeHandleRegistry::new();
        let shape_id: ShapeId = 0xDEADBEEF;
        let first = registry.register(11, 11, Some(shape_id));
        let second = registry.register(11, 11, Some(shape_id));

        assert_eq!(first, second);
        let entry = registry.get(first).expect("type handle entry");
        assert_eq!(entry.nominal_type_id, 11);
        assert_eq!(entry.layout_id, 11);
        assert_eq!(entry.shape_id, Some(shape_id));
    }

    #[test]
    fn type_handle_registry_distinguishes_shape_contracts() {
        let mut registry = RuntimeTypeHandleRegistry::new();
        let a = registry.register(7, 7, Some(1));
        let b = registry.register(7, 7, Some(2));

        assert_ne!(a, b);
        assert_eq!(registry.get(a).map(|entry| entry.shape_id), Some(Some(1)));
        assert_eq!(registry.get(b).map(|entry| entry.shape_id), Some(Some(2)));
    }

    #[test]
    fn shape_adapter_splits_field_and_method_maps() {
        let adapter = ShapeAdapter::from_slot_map(
            11,
            22,
            &[
                StructuralSlotBinding::Field(3),
                StructuralSlotBinding::Method(5),
                StructuralSlotBinding::Dynamic(9),
                StructuralSlotBinding::Missing,
            ],
            0,
        );

        assert_eq!(adapter.binding_for_slot(0), StructuralSlotBinding::Field(3));
        assert_eq!(
            adapter.binding_for_slot(1),
            StructuralSlotBinding::Method(5)
        );
        assert_eq!(
            adapter.binding_for_slot(2),
            StructuralSlotBinding::Dynamic(9)
        );
        assert_eq!(adapter.binding_for_slot(3), StructuralSlotBinding::Missing);
        assert_eq!(adapter.len(), 4);
        assert_eq!(adapter.epoch, 0);
    }

    #[test]
    fn shape_adapter_detects_identity_projection() {
        let adapter = ShapeAdapter::from_slot_map(
            9,
            10,
            &[
                StructuralSlotBinding::Field(0),
                StructuralSlotBinding::Field(1),
            ],
            0,
        );
        assert!(adapter.is_identity_field_projection());

        let non_identity = ShapeAdapter::from_slot_map(
            9,
            10,
            &[
                StructuralSlotBinding::Field(1),
                StructuralSlotBinding::Field(0),
            ],
            0,
        );
        assert!(!non_identity.is_identity_field_projection());
    }

    #[test]
    fn property_key_registry_dedupes_names() {
        let mut registry = PropertyKeyRegistry::new();
        let first = registry.intern("name");
        let second = registry.intern("name");
        let third = registry.intern("other");

        assert_eq!(first, second);
        assert_ne!(first, third);
        assert_eq!(registry.resolve(first), Some("name"));
        assert_eq!(registry.resolve(third), Some("other"));
    }
}
