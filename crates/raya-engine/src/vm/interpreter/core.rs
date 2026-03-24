//! Task-based interpreter that can suspend and resume
//!
//! This interpreter executes a single task until it completes, suspends, or fails.
//! Unlike the synchronous `Vm`, this interpreter returns control to the scheduler
//! when the task needs to wait for something.

use super::execution::{ExecutionFrame, ExecutionResult, OpcodeResult, ReturnAction};
use super::{ClassRegistry, SafepointCoordinator};
use crate::compiler::{Module, Opcode};
use crate::vm::builtins::handlers::{
    call_runtime_method as runtime_handler, RuntimeHandlerContext,
};
use crate::vm::gc::GarbageCollector;
use crate::vm::native_handler::NativeHandler;
use crate::vm::object::{CallableKind, Class, Object, RayaString};
use crate::vm::scheduler::{SuspendReason, Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::{MutexRegistry, SemaphoreRegistry};
use crate::vm::value::Value;
use crate::vm::VmError;
use crossbeam_deque::Injector;
use parking_lot::RwLock;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::time::Instant;

/// Helper to convert Value to f64, handling both f64 and i32 values
#[inline]
pub(in crate::vm::interpreter) fn value_to_f64(v: Value) -> Result<f64, VmError> {
    if let Some(f) = v.as_f64() {
        Ok(f)
    } else if let Some(i) = v.as_i32() {
        Ok(i as f64)
    } else if let Some(b) = v.as_bool() {
        // ES spec: ToNumber(true) = 1, ToNumber(false) = 0
        Ok(if b { 1.0 } else { 0.0 })
    } else if v.is_null() {
        // ES spec: ToNumber(null) = 0
        Ok(0.0)
    } else if v.is_undefined() {
        // ES spec: ToNumber(undefined) = NaN
        Ok(f64::NAN)
    } else if let Some(s) = super::opcodes::native::checked_string_ptr(v) {
        // ES spec: ToNumber(string) — parse trimmed string as number
        let s = unsafe { &*s.as_ptr() };
        let trimmed = s.data.trim();
        if trimmed.is_empty() {
            Ok(0.0)
        } else if trimmed == "Infinity" || trimmed == "+Infinity" {
            Ok(f64::INFINITY)
        } else if trimmed == "-Infinity" {
            Ok(f64::NEG_INFINITY)
        } else if let Some(hex) = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
        {
            Ok(u64::from_str_radix(hex, 16)
                .map(|n| n as f64)
                .unwrap_or(f64::NAN))
        } else if let Some(bin) = trimmed
            .strip_prefix("0b")
            .or_else(|| trimmed.strip_prefix("0B"))
        {
            Ok(u64::from_str_radix(bin, 2)
                .map(|n| n as f64)
                .unwrap_or(f64::NAN))
        } else if let Some(oct) = trimmed
            .strip_prefix("0o")
            .or_else(|| trimmed.strip_prefix("0O"))
        {
            Ok(u64::from_str_radix(oct, 8)
                .map(|n| n as f64)
                .unwrap_or(f64::NAN))
        } else {
            Ok(trimmed.parse::<f64>().unwrap_or(f64::NAN))
        }
    } else {
        Err(VmError::TypeError("Expected number".to_string()))
    }
}

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

#[cfg(all(test, feature = "jit"))]
mod tests {
    use super::Interpreter;
    use crate::compiler::{Function, Opcode};
    use crate::jit::runtime::trampoline::JitExitInfo;
    use crate::vm::interpreter::JitTelemetry;
    use crate::vm::value::Value;
    use std::sync::Arc;

    fn make_function(code: Vec<u8>) -> Function {
        Function {
            name: "f".to_string(),
            param_count: 0,
            local_count: 0,
            code,
            ..Default::default()
        }
    }

    #[test]
    fn resume_guard_allows_entry_nativecall_zero_args() {
        let mut code = Vec::new();
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(0u8);
        code.push(Opcode::Return as u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 0),
            Some(0)
        );
    }

    #[test]
    fn resume_guard_rejects_non_entry_offset() {
        let mut code = Vec::new();
        code.push(Opcode::ConstNull as u8);
        code.push(Opcode::Pop as u8);
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(0u8);
        code.push(Opcode::Return as u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 2),
            Some(0)
        );
    }

    #[test]
    fn resume_guard_allows_nativecall_with_args_when_stack_empty() {
        let mut code = Vec::new();
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(1u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 0),
            Some(1)
        );
    }

    #[test]
    fn resume_guard_rejects_non_entry_with_non_empty_stack() {
        let mut code = Vec::new();
        code.push(Opcode::ConstNull as u8); // leaves stack depth = 1
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(0u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 1),
            None
        );
    }

    #[test]
    fn resume_guard_allows_non_entry_nativecall_with_args_when_stack_empty() {
        let mut code = Vec::new();
        code.push(Opcode::ConstNull as u8);
        code.push(Opcode::Pop as u8); // stack depth back to 0
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(2u8);
        let func = make_function(code);
        assert_eq!(
            Interpreter::native_resume_boundary_arg_count(&func, 2),
            Some(2)
        );
    }

    #[test]
    fn resume_guard_allows_preemption_on_jmp_with_empty_stack() {
        let mut code = Vec::new();
        code.push(Opcode::Jmp as u8);
        code.extend_from_slice(&0i32.to_le_bytes());
        let func = make_function(code);
        assert!(Interpreter::can_resume_at_preemption_boundary(&func, 0));
    }

    #[test]
    fn resume_guard_rejects_preemption_on_conditional_jump_stack_dep() {
        let mut code = Vec::new();
        code.push(Opcode::ConstTrue as u8);
        code.push(Opcode::JmpIfFalse as u8);
        code.extend_from_slice(&0i32.to_le_bytes());
        let func = make_function(code);
        assert!(!Interpreter::can_resume_at_preemption_boundary(&func, 1));
    }

    #[test]
    fn resume_telemetry_counters_increment() {
        let t = Arc::new(JitTelemetry::default());
        let opt = Some(t.clone());

        Interpreter::record_native_resume_decision(&opt, true);
        Interpreter::record_native_resume_decision(&opt, false);
        Interpreter::record_preemption_resume_decision(&opt, true);
        Interpreter::record_preemption_resume_decision(&opt, false);

        let snap = t.snapshot();
        assert_eq!(snap.resume_native_ok, 1);
        assert_eq!(snap.resume_native_reject, 1);
        assert_eq!(snap.resume_preemption_ok, 1);
        assert_eq!(snap.resume_preemption_reject, 1);
    }

    #[test]
    fn native_resume_materialization_accepts_matching_count() {
        let mut code = Vec::new();
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(2u8);
        let func = make_function(code);

        let mut exit = JitExitInfo {
            bytecode_offset: 0,
            native_arg_count: 2,
            ..Default::default()
        };
        exit.native_args[0] = Value::i32(7).raw();
        exit.native_args[1] = Value::i32(11).raw();

        let vals = Interpreter::materialize_native_resume_operands(&func, &exit)
            .expect("expected materialized operand values");
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0].as_i32(), Some(7));
        assert_eq!(vals[1].as_i32(), Some(11));
    }

    #[test]
    fn native_resume_materialization_rejects_mismatched_count() {
        let mut code = Vec::new();
        code.push(Opcode::NativeCall as u8);
        code.extend_from_slice(&0u16.to_le_bytes());
        code.push(1u8);
        let func = make_function(code);

        let exit = JitExitInfo {
            bytecode_offset: 0,
            native_arg_count: 2,
            ..Default::default()
        };
        assert!(Interpreter::materialize_native_resume_operands(&func, &exit).is_none());
    }

    #[test]
    fn interpreter_boundary_materialization_restores_full_stack() {
        let mut exit = JitExitInfo {
            bytecode_offset: 12,
            native_arg_count: 3,
            ..Default::default()
        };
        exit.native_args[0] = Value::i32(1).raw();
        exit.native_args[1] = Value::i32(2).raw();
        exit.native_args[2] = Value::i32(3).raw();

        let vals = Interpreter::materialize_interpreter_resume_stack(&exit)
            .expect("expected full stack materialization");
        assert_eq!(vals.len(), 3);
        assert_eq!(vals[0].as_i32(), Some(1));
        assert_eq!(vals[1].as_i32(), Some(2));
        assert_eq!(vals[2].as_i32(), Some(3));
    }
}

/// Task interpreter that can suspend and resume
///
/// This struct holds references to shared state and executes a task.
/// The task's execution state (stack, IP, exception handlers, etc.) lives in the Task itself.
pub struct Interpreter<'a> {
    /// Reference to the garbage collector
    pub(in crate::vm::interpreter) gc: &'a parking_lot::Mutex<GarbageCollector>,

    /// Reference to the class registry
    pub(in crate::vm::interpreter) classes: &'a RwLock<ClassRegistry>,

    /// Reference to the physical runtime layout registry.
    pub(in crate::vm::interpreter) layouts:
        &'a RwLock<crate::vm::interpreter::RuntimeLayoutRegistry>,

    /// Reference to the mutex registry
    pub(in crate::vm::interpreter) mutex_registry: &'a MutexRegistry,

    /// Reference to the semaphore registry
    pub(in crate::vm::interpreter) semaphore_registry: &'a SemaphoreRegistry,

    /// Safepoint coordinator for GC
    pub(in crate::vm::interpreter) safepoint: &'a SafepointCoordinator,

    /// Global variables by index
    pub(in crate::vm::interpreter) globals_by_index: &'a RwLock<Vec<Value>>,

    /// Ambient builtin global slot mapping (name -> absolute global slot index).
    pub(in crate::vm::interpreter) builtin_global_slots: &'a RwLock<FxHashMap<String, usize>>,

    /// Shared JS-compatible realm-global bindings keyed by identifier name.
    pub(in crate::vm::interpreter) js_global_bindings:
        &'a RwLock<FxHashMap<String, crate::vm::interpreter::shared_state::JsGlobalBindingRecord>>,

    /// Reverse mapping from absolute global slot to JS-compatible binding name.
    pub(in crate::vm::interpreter) js_global_binding_slots:
        &'a RwLock<FxHashMap<usize, String>>,

    /// VM-local interned constant strings keyed by `(module checksum, constant index)`.
    pub(in crate::vm::interpreter) constant_string_cache:
        &'a RwLock<FxHashMap<(String, usize), Value>>,

    /// Freshly allocated values rooted only until they are published into a
    /// stable root set such as task state or a shared cache.
    pub(in crate::vm::interpreter) ephemeral_gc_roots: &'a RwLock<Vec<Value>>,

    /// Opaque GC-backed handles pinned for VM lifetime.
    pub(in crate::vm::interpreter) pinned_handles: &'a RwLock<FxHashSet<u64>>,

    /// Task registry (for spawn/await)
    pub(in crate::vm::interpreter) tasks: &'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,

    /// Global task injector for scheduling spawned tasks
    pub(in crate::vm::interpreter) injector: &'a Arc<Injector<Arc<Task>>>,

    /// Metadata store for Reflect API
    pub(in crate::vm::interpreter) metadata:
        &'a parking_lot::Mutex<crate::vm::reflect::MetadataStore>,

    /// Class metadata registry for reflection (field/method names)
    pub(in crate::vm::interpreter) class_metadata:
        &'a RwLock<crate::vm::reflect::ClassMetadataRegistry>,

    /// External native call handler (stdlib implementation)
    #[allow(dead_code)]
    pub(in crate::vm::interpreter) native_handler: &'a Arc<dyn NativeHandler>,

    /// Per-module runtime layouts (global slot base, class base, native table, init state).
    pub(in crate::vm::interpreter) module_layouts:
        &'a RwLock<FxHashMap<[u8; 32], crate::vm::interpreter::shared_state::ModuleRuntimeLayout>>,

    /// Loaded modules indexed by name/checksum for lazy builtin initialization.
    pub(in crate::vm::interpreter) module_registry:
        &'a RwLock<crate::vm::interpreter::module_registry::ModuleRegistry>,

    /// Shared structural adapter cache keyed by `(provider_layout, required_shape)`.
    pub(in crate::vm::interpreter) structural_shape_adapters: &'a RwLock<
        FxHashMap<
            crate::vm::interpreter::shared_state::StructuralAdapterKey,
            Arc<crate::vm::interpreter::shared_state::ShapeAdapter>,
        >,
    >,
    /// Canonical member names keyed by structural shape id.
    pub(in crate::vm::interpreter) structural_shape_names:
        &'a RwLock<FxHashMap<crate::vm::object::ShapeId, Vec<String>>>,
    /// Canonical structural shapes for structural object layouts.
    pub(in crate::vm::interpreter) structural_object_shapes:
        &'a RwLock<FxHashMap<crate::vm::object::LayoutId, Vec<String>>>,

    /// Runtime-owned constructor/type handle registry for imported nominal types.
    pub(in crate::vm::interpreter) type_handles:
        &'a RwLock<crate::vm::interpreter::shared_state::RuntimeTypeHandleRegistry>,
    /// Canonical runtime constructor values rooted through `globals_by_index`.
    pub(in crate::vm::interpreter) class_value_slots: &'a RwLock<FxHashMap<usize, usize>>,
    /// Runtime-local property-key interner for dynamic object lanes.
    pub(in crate::vm::interpreter) prop_keys:
        &'a RwLock<crate::vm::interpreter::shared_state::PropertyKeyRegistry>,

    /// Offline AOT profile collector populated from interpreter execution.
    pub(in crate::vm::interpreter) aot_profile: &'a RwLock<crate::aot_profile::AotProfileCollector>,

    /// IO submission sender for NativeCallResult::Suspend (None in tests without reactor)
    pub(in crate::vm::interpreter) io_submit_tx:
        Option<&'a crossbeam::channel::Sender<crate::vm::scheduler::IoSubmission>>,

    /// Maximum consecutive preemptions before killing a task
    pub(in crate::vm::interpreter) max_preemptions: u32,

    /// Stack pool for reusing Stack allocations across spawned tasks
    pub(in crate::vm::interpreter) stack_pool: &'a crate::vm::scheduler::StackPool,

    /// JIT code cache for native dispatch (None when JIT is disabled)
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) code_cache:
        Option<Arc<crate::jit::runtime::code_cache::CodeCache>>,

    /// Per-module profiling counters for on-the-fly JIT compilation
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) module_profile:
        Option<Arc<crate::jit::profiling::counters::ModuleProfile>>,

    /// Global module-profile table used when layout changes invalidate compiled code.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) module_profiles_map: Option<
        &'a RwLock<FxHashMap<[u8; 32], Arc<crate::jit::profiling::counters::ModuleProfile>>>,
    >,

    /// Handle to submit compilation requests to the background JIT thread
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) background_compiler:
        Option<Arc<crate::jit::profiling::BackgroundCompiler>>,

    /// Shared counters for JIT telemetry.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) jit_telemetry: Option<Arc<crate::vm::interpreter::JitTelemetry>>,

    /// Compilation policy for deciding when a function is hot enough
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) compilation_policy:
        crate::jit::profiling::policy::CompilationPolicy,

    /// Current function ID being executed for profiling/bookkeeping.
    pub(in crate::vm::interpreter) current_func_id_for_profiling: usize,

    /// Current module Arc used by loop-based JIT profiling requests.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) current_module_for_profiling: Option<Arc<Module>>,

    /// Current module id in code cache used by loop-based JIT profiling requests.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) current_module_id_for_profiling: Option<u64>,

    /// Debug state for debugger coordination (None = no debugger attached)
    pub(in crate::vm::interpreter) debug_state: Option<Arc<super::debug_state::DebugState>>,

    /// Sampling profiler (None when profiling is disabled).
    pub(in crate::vm::interpreter) profiler: Option<Arc<crate::profiler::Profiler>>,

    /// Current function ID for profiler stack capture.
    pub(in crate::vm::interpreter) profiler_func_id: usize,

    /// Current bytecode offset for offline AOT profile site recording.
    pub(in crate::vm::interpreter) current_bytecode_offset_for_aot_profile: u32,

    /// Current module checksum for offline AOT profile recording.
    pub(in crate::vm::interpreter) current_module_checksum_for_aot_profile: [u8; 32],
}

impl<'a> Interpreter<'a> {
    #[inline]
    pub(in crate::vm::interpreter) fn resolve_global_slot(
        &self,
        module: &Module,
        local_slot: usize,
    ) -> usize {
        self.module_layouts
            .read()
            .get(&module.checksum)
            .map(|layout| layout.global_base + local_slot)
            .unwrap_or(local_slot)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn resolve_nominal_type_id(
        &self,
        module: &Module,
        local_nominal_type_id: usize,
    ) -> Result<usize, VmError> {
        self.module_layouts
            .read()
            .get(&module.checksum)
            .and_then(|layout| {
                (local_nominal_type_id < layout.nominal_type_len)
                    .then_some(layout.nominal_type_base + local_nominal_type_id)
            })
            .ok_or_else(|| {
                VmError::RuntimeError(format!(
                    "Invalid module-local nominal type id {} for module {}",
                    local_nominal_type_id, module.metadata.name
                ))
            })
    }

    #[inline]
    pub(in crate::vm::interpreter) fn module_resolved_natives(
        &self,
        module: &Module,
    ) -> crate::vm::native_registry::ResolvedNatives {
        self.module_layouts
            .read()
            .get(&module.checksum)
            .map(|layout| layout.resolved_natives.clone())
            .unwrap_or_else(crate::vm::native_registry::ResolvedNatives::empty)
    }

    pub(in crate::vm::interpreter) fn register_structural_shape_names(
        &self,
        shape_id: crate::vm::object::ShapeId,
        member_names: &[String],
    ) {
        if member_names.is_empty() {
            return;
        }
        self.structural_shape_names
            .write()
            .entry(shape_id)
            .or_insert_with(|| member_names.to_vec());
    }

    pub(in crate::vm::interpreter) fn register_dynamic_module(
        &self,
        module: Arc<Module>,
    ) -> Result<(), String> {
        if self.module_layouts.read().contains_key(&module.checksum) {
            return Ok(());
        }

        if !module.native_functions.is_empty() {
            return Err(format!(
                "dynamic module '{}' unexpectedly contains unresolved module natives",
                module.metadata.name
            ));
        }

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

        self.module_layouts.write().insert(
            module.checksum,
            crate::vm::interpreter::shared_state::ModuleRuntimeLayout {
                checksum: module.checksum,
                global_base,
                global_len,
                nominal_type_base,
                nominal_type_len,
                resolved_natives: crate::vm::native_registry::ResolvedNatives::empty(),
                initialized: false,
            },
        );

        for binding in &module.metadata.js_global_bindings {
            let absolute_slot = global_base + binding.slot as usize;
            let canonical_existing = if binding.published_to_global_object {
                self.js_global_bindings
                    .read()
                    .get(&binding.name)
                    .copied()
                    .filter(|existing| existing.published_to_global_object)
            } else {
                None
            };
            let initialized = matches!(
                binding.kind,
                crate::compiler::bytecode::module::JsGlobalBindingKind::Var
                    | crate::compiler::bytecode::module::JsGlobalBindingKind::Function
            );
            self.js_global_binding_slots
                .write()
                .insert(absolute_slot, binding.name.clone());
            if canonical_existing.is_none() {
                self.js_global_bindings.write().insert(
                    binding.name.clone(),
                    crate::vm::interpreter::JsGlobalBindingRecord {
                        slot: absolute_slot,
                        kind: binding.kind,
                        published_to_global_object: binding.published_to_global_object,
                        initialized,
                    },
                );
            }
            if std::env::var("RAYA_DEBUG_JS_GLOBAL_BINDINGS").is_ok() {
                eprintln!(
                    "[js-global:register] name={} slot={} kind={:?} published={} initialized={} canonical={}",
                    binding.name,
                    absolute_slot,
                    binding.kind,
                    binding.published_to_global_object,
                    initialized,
                    canonical_existing
                        .map(|existing| existing.slot.to_string())
                        .unwrap_or_else(|| "-".to_string())
                );
            }
        }

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

        // Register structural layouts emitted by the compiler for object literals.
        // The compiler stores layout_id → field_names in structural_layouts;
        // without this registration, for-in and property enumeration cannot
        // discover the field names for structural objects at runtime.
        for layout in &module.metadata.structural_layouts {
            if layout.member_names.is_empty() {
                continue;
            }
            self.register_structural_layout_shape(layout.layout_id, &layout.member_names);
        }

        self.register_dynamic_module_classes(&module, nominal_type_base);
        Ok(())
    }

    fn register_dynamic_module_classes(&self, module: &Arc<Module>, nominal_type_base: usize) {
        let mut classes = self.classes.write();
        let mut class_metadata_registry = self.class_metadata.write();
        for (i, class_def) in module.classes.iter().enumerate() {
            let global_nominal_type_id = nominal_type_base + i;
            let parent_global_id = class_def
                .parent_id
                .map(|parent_id| nominal_type_base + parent_id as usize)
                .or_else(|| {
                    class_def.parent_name.as_deref().and_then(|parent_name| {
                        classes.get_class_by_name(parent_name).map(|class| class.id)
                    })
                });
            let inherited_runtime_parent =
                class_def.parent_id.is_none() && class_def.parent_name.is_some();
            let inherited_field_count = parent_global_id
                .and_then(|parent_id| classes.get_class(parent_id).map(|class| class.field_count))
                .unwrap_or(0);
            let missing_parent_fields =
                inherited_field_count > 0 && class_def.field_count < inherited_field_count;
            let total_field_count = if inherited_runtime_parent || missing_parent_fields {
                inherited_field_count + class_def.field_count
            } else {
                class_def.field_count
            };

            let mut class = if let Some(parent_id) = parent_global_id {
                let mut c = crate::vm::object::Class::with_parent(
                    global_nominal_type_id,
                    class_def.name.clone(),
                    total_field_count,
                    parent_id,
                );
                if let Some(parent) = classes.get_class(parent_id) {
                    for &method_id in &parent.vtable.methods {
                        c.add_method(method_id);
                    }
                }
                c
            } else {
                crate::vm::object::Class::new(
                    global_nominal_type_id,
                    class_def.name.clone(),
                    total_field_count,
                )
            };
            class.module = Some(module.clone());

            if let Some(max_slot) = class_def.methods.iter().map(|m| m.slot + 1).max() {
                while class.vtable.methods.len() < max_slot {
                    class.add_method(usize::MAX);
                }
            }

            for method in &class_def.methods {
                class.vtable.methods[method.slot] = method.function_id;
            }
            class.prototype_members = class_def
                .methods
                .iter()
                .map(|method| crate::vm::object::PrototypeMember {
                    name: method
                        .name
                        .rsplit("::")
                        .next()
                        .unwrap_or(method.name.as_str())
                        .to_string(),
                    function_id: method.function_id,
                    kind: match method.kind {
                        crate::compiler::bytecode::MethodKind::Normal => {
                            crate::vm::object::PrototypeMemberKind::Method
                        }
                        crate::compiler::bytecode::MethodKind::Getter => {
                            crate::vm::object::PrototypeMemberKind::Getter
                        }
                        crate::compiler::bytecode::MethodKind::Setter => {
                            crate::vm::object::PrototypeMemberKind::Setter
                        }
                    },
                })
                .collect();
            class.static_members = class_def
                .static_methods
                .iter()
                .map(|method| crate::vm::object::PrototypeMember {
                    name: method
                        .name
                        .rsplit("::")
                        .next()
                        .unwrap_or(method.name.as_str())
                        .to_string(),
                    function_id: method.function_id,
                    kind: match method.kind {
                        crate::compiler::bytecode::MethodKind::Normal => {
                            crate::vm::object::PrototypeMemberKind::Method
                        }
                        crate::compiler::bytecode::MethodKind::Getter => {
                            crate::vm::object::PrototypeMemberKind::Getter
                        }
                        crate::compiler::bytecode::MethodKind::Setter => {
                            crate::vm::object::PrototypeMemberKind::Setter
                        }
                    },
                })
                .collect();

            let exported_constructor_id = module.exports.iter().find_map(|export| {
                (matches!(export.symbol_type, crate::compiler::SymbolType::Class)
                    && export
                        .nominal_type
                        .is_some_and(|nominal| nominal.local_nominal_type_index as usize == i))
                .then_some(
                    export
                        .nominal_type
                        .and_then(|nominal| nominal.constructor_function_index),
                )
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
            self.register_nominal_layout(
                global_nominal_type_id,
                layout_id,
                total_field_count,
                Some(class_def.name.clone()),
            );

            let mut class_meta = if inherited_runtime_parent || missing_parent_fields {
                parent_global_id
                    .and_then(|parent_id| class_metadata_registry.get(parent_id).cloned())
                    .unwrap_or_default()
            } else {
                crate::vm::reflect::ClassMetadata::new()
            };

            if let Some(class_reflection) =
                module.reflection.as_ref().and_then(|r| r.classes.get(i))
            {
                let field_offset = if inherited_runtime_parent || missing_parent_fields {
                    inherited_field_count
                } else {
                    0
                };
                for (field_index, field) in class_reflection.fields.iter().enumerate() {
                    if field.is_static {
                        class_meta.add_static_field(field.name.clone(), field_index);
                    } else {
                        let type_id = reflect_type_name_to_id(&field.type_name);
                        class_meta.add_field_with_type(
                            field.name.clone(),
                            field_offset + field_index,
                            type_id,
                        );
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

    fn module_global_slot_count(module: &Module) -> usize {
        let function_slots = module
            .functions
            .iter()
            .map(Self::function_global_slot_count)
            .max()
            .unwrap_or(0);
        let exported_constant_slots = module
            .exports
            .iter()
            .filter(|export| matches!(export.symbol_type, crate::compiler::SymbolType::Constant))
            .map(|export| export.index + 1)
            .max()
            .unwrap_or(0);
        let import_slots = module
            .imports
            .iter()
            .filter_map(|import| import.runtime_global_slot.map(|slot| slot as usize + 1))
            .max()
            .unwrap_or(0);
        function_slots
            .max(import_slots)
            .max(exported_constant_slots)
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
                }
                _ => {}
            }
            ip += crate::compiler::bytecode::verify::operand_size(opcode);
        }

        max_slot
    }

    #[inline]
    pub(in crate::vm::interpreter) fn remap_shape_slot_binding(
        &self,
        object: &Object,
        expected_shape: crate::vm::object::ShapeId,
        expected_slot: usize,
    ) -> crate::vm::interpreter::shared_state::StructuralSlotBinding {
        let adapter_key = crate::vm::interpreter::shared_state::StructuralAdapterKey {
            provider_layout: object.layout_id(),
            required_shape: expected_shape,
        };
        if let Some(adapter) = self
            .structural_shape_adapters
            .read()
            .get(&adapter_key)
            .cloned()
        {
            return adapter.binding_for_slot(expected_slot);
        }
        self.ensure_shape_adapter_for_object(object, expected_shape)
            .map(|adapter| adapter.binding_for_slot(expected_slot))
            .unwrap_or(
                crate::vm::interpreter::shared_state::StructuralSlotBinding::Field(expected_slot),
            )
    }

    #[inline]
    pub(in crate::vm::interpreter) fn intern_prop_key(
        &self,
        name: &str,
    ) -> crate::vm::object::PropKeyId {
        self.prop_keys.write().intern(name)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn allocate_pinned_handle<T: 'static>(&self, value: T) -> u64 {
        let gc_ptr = self.gc.lock().allocate(value);
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        // Keep raw-handle-backed GC objects strongly rooted for VM lifetime.
        self.ephemeral_gc_roots.write().push(value);
        let handle = gc_ptr.as_ptr() as u64;
        self.pinned_handles.write().insert(handle);
        handle
    }

    #[inline]
    pub(in crate::vm::interpreter) fn prop_key_name(
        &self,
        key: crate::vm::object::PropKeyId,
    ) -> Option<String> {
        self.prop_keys.read().resolve(key).map(str::to_string)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn register_structural_layout_shape(
        &self,
        layout_id: crate::vm::object::LayoutId,
        member_names: &[String],
    ) {
        if layout_id == 0 {
            return;
        }
        self.structural_object_shapes
            .write()
            .entry(layout_id)
            .or_insert_with(|| member_names.to_vec());
        self.layouts
            .write()
            .register_layout_shape(layout_id, member_names);
        self.invalidate_jit_for_layout(layout_id);
    }

    #[inline]
    pub(in crate::vm::interpreter) fn structural_layout_names(
        &self,
        layout_id: crate::vm::object::LayoutId,
    ) -> Option<Vec<String>> {
        if let Some(names) = self
            .layouts
            .read()
            .layout_field_names(layout_id)
            .map(|names| names.to_vec())
        {
            return Some(names);
        }
        self.structural_object_shapes
            .read()
            .get(&layout_id)
            .cloned()
    }

    pub(in crate::vm::interpreter) fn invoke_callable_sync(
        &mut self,
        callable: Value,
        args: &[Value],
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        self.invoke_callable_sync_with_this_and_new_target(
            callable,
            None,
            None,
            args,
            caller_task,
            caller_module,
        )
    }

    pub(in crate::vm::interpreter) fn invoke_callable_sync_with_this(
        &mut self,
        callable: Value,
        explicit_this: Option<Value>,
        args: &[Value],
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        self.invoke_callable_sync_with_this_and_new_target(
            callable,
            explicit_this,
            None,
            args,
            caller_task,
            caller_module,
        )
    }

    pub(in crate::vm::interpreter) fn invoke_callable_sync_with_this_and_new_target(
        &mut self,
        callable: Value,
        explicit_this: Option<Value>,
        new_target: Option<Value>,
        args: &[Value],
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        let mut stack = Stack::new();
        let scratch_task = Arc::new(Task::new(
            caller_task.current_func_id(),
            caller_task.current_module(),
            Some(caller_task.id()),
        ));
        if let Some(env) = caller_task.current_active_direct_eval_env() {
            scratch_task.push_active_direct_eval_env(
                env,
                caller_task.current_active_direct_eval_is_strict(),
                caller_task.current_active_direct_eval_uses_script_global_bindings(),
                caller_task.current_active_direct_eval_persist_caller_declarations(),
            );
            if let Some(completion) = caller_task.current_active_direct_eval_completion() {
                let _ = scratch_task.set_current_active_direct_eval_completion(completion);
            }
        }
        if let Some(home_object) = caller_task
            .current_active_js_home_object()
            .or_else(|| {
                caller_task.current_closure().and_then(|closure| {
                    let closure_ptr = unsafe { closure.as_ptr::<Object>() }?;
                    let closure_obj = unsafe { &*closure_ptr.as_ptr() };
                    closure_obj.callable_home_object()
                })
            })
        {
            scratch_task.push_active_js_home_object(home_object);
        }
        if let Some(new_target) = caller_task.current_active_js_new_target() {
            scratch_task.push_active_js_new_target(new_target);
        }
        if let Some(new_target) = new_target {
            scratch_task.push_active_js_new_target(new_target);
        }

        let opcode_result = if let Some(raw_ptr) = unsafe { callable.as_ptr::<u8>() } {
            let header = unsafe { &*crate::vm::gc::header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
            if header.type_id() == std::any::TypeId::of::<Object>() {
                let co = unsafe { &*callable.as_ptr::<Object>().unwrap().as_ptr() };
                if let Some(ref callable_data) = co.callable {
                    if let CallableKind::BoundNative {
                        native_id,
                        receiver,
                    } = &callable_data.kind
                    {
                        self.exec_bound_native_method_call(
                            &mut stack,
                            *receiver,
                            *native_id,
                            args.to_vec(),
                            caller_module,
                            &scratch_task,
                        )
                    } else {
                        match self.callable_frame_for_value(
                            callable,
                            &mut stack,
                            args,
                            explicit_this,
                            ReturnAction::PushReturnValue,
                            caller_module,
                            &scratch_task,
                        )? {
                            Some(result) => result,
                            None => {
                                return Err(VmError::TypeError("Value is not callable".to_string()))
                            }
                        }
                    }
                } else {
                    match self.callable_frame_for_value(
                        callable,
                        &mut stack,
                        args,
                        explicit_this,
                        ReturnAction::PushReturnValue,
                        caller_module,
                        &scratch_task,
                    )? {
                        Some(result) => result,
                        None => {
                            return Err(VmError::TypeError("Value is not callable".to_string()))
                        }
                    }
                }
            } else {
                return Err(VmError::TypeError("Value is not callable".to_string()));
            }
        } else {
            return Err(VmError::TypeError("Value is not callable".to_string()));
        };

        match opcode_result {
            OpcodeResult::Continue => {
                if stack.depth() == 0 {
                    Ok(Value::undefined())
                } else {
                    stack.pop()
                }
            }
            OpcodeResult::Return(value) => Ok(value),
            OpcodeResult::Error(error) => {
                if !caller_task.has_exception() {
                    if let Some(exception) = scratch_task.current_exception() {
                        caller_task.set_exception(exception);
                    }
                }
                Err(error)
            }
            OpcodeResult::Suspend(_) => Err(VmError::RuntimeError(
                "Synchronous callable invocation suspended unexpectedly".to_string(),
            )),
            OpcodeResult::PushFrame {
                func_id,
                arg_count,
                is_closure: _,
                closure_val,
                module,
                return_action: _,
            } => {
                let callee_module = module.unwrap_or_else(|| caller_task.current_module());
                let depth = stack.depth();
                let args_start = depth.saturating_sub(arg_count);
                let mut frame_args = Vec::with_capacity(arg_count);
                for offset in 0..arg_count {
                    frame_args.push(stack.peek_at(args_start + offset)?);
                }

                let callee_task = Arc::new(Task::with_args(
                    func_id,
                    callee_module.clone(),
                    Some(caller_task.id()),
                    frame_args,
                ));
                if let Some(closure) = closure_val {
                    callee_task.push_closure(closure);
                }
                if let Some(env) = scratch_task.current_active_direct_eval_env() {
                    callee_task.push_active_direct_eval_env(
                        env,
                        scratch_task.current_active_direct_eval_is_strict(),
                        scratch_task.current_active_direct_eval_uses_script_global_bindings(),
                        scratch_task.current_active_direct_eval_persist_caller_declarations(),
                    );
                    if let Some(completion) = scratch_task.current_active_direct_eval_completion() {
                        let _ = callee_task.set_current_active_direct_eval_completion(completion);
                    }
                }
                if let Some(home_object) = scratch_task.current_active_js_home_object() {
                    callee_task.push_active_js_home_object(home_object);
                }
                if let Some(new_target) = scratch_task.current_active_js_new_target() {
                    callee_task.push_active_js_new_target(new_target);
                }
                self.tasks
                    .write()
                    .insert(callee_task.id(), callee_task.clone());

                match self.run(&callee_task) {
                    ExecutionResult::Completed(value) => {
                        callee_task.complete(value);
                        Ok(value)
                    }
                    ExecutionResult::Suspended(reason) => {
                        callee_task.suspend(reason);
                        Err(VmError::RuntimeError(
                            "Synchronous callable invocation suspended unexpectedly".to_string(),
                        ))
                    }
                    ExecutionResult::Failed(error) => {
                        callee_task.fail();
                        if !caller_task.has_exception() {
                            if let Some(exception) = callee_task.current_exception() {
                                caller_task.set_exception(exception);
                            }
                        }
                        Err(error)
                    }
                }
            }
        }
    }

    #[inline]
    pub(in crate::vm::interpreter) fn layout_field_names_for_object(
        &self,
        object: &crate::vm::object::Object,
    ) -> Option<Vec<String>> {
        if let Some(names) = self.structural_layout_names(object.layout_id()) {
            return Some(names);
        }
        crate::vm::object::global_layout_names(object.layout_id())
    }

    #[inline]
    pub(in crate::vm::interpreter) fn structural_field_slot_index_for_object(
        &self,
        object: &crate::vm::object::Object,
        field_name: &str,
    ) -> Option<usize> {
        let names = self.layout_field_names_for_object(object)?;
        if names.len() != object.field_count() {
            return None;
        }
        names.iter().position(|name| name == field_name)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn structural_field_name_for_object_offset(
        &self,
        object: &crate::vm::object::Object,
        field_offset: usize,
    ) -> Option<String> {
        let names = self.layout_field_names_for_object(object)?;
        if names.len() != object.field_count() {
            return None;
        }
        names.get(field_offset).cloned()
    }

    /// Resolve a property key string to a fixed-slot index via the object's layout.
    /// This is the runtime equivalent of what LoadFieldShape does at compile time.
    pub(in crate::vm::interpreter) fn shape_resolve_key(
        &self,
        layout_id: crate::vm::object::LayoutId,
        key: &str,
    ) -> Option<usize> {
        // Try layout registry (covers both nominal and structural registered layouts)
        {
            let layouts = self.layouts.read();
            if let Some(field_names) = layouts.layout_field_names(layout_id) {
                for (idx, name) in field_names.iter().enumerate() {
                    if name == key {
                        return Some(idx);
                    }
                }
            }
        }

        // Try structural object shapes fallback
        if let Some(names) = self.structural_object_shapes.read().get(&layout_id) {
            for (idx, name) in names.iter().enumerate() {
                if name == key {
                    return Some(idx);
                }
            }
        }

        // Try global layout names fallback
        if let Some(field_names) = crate::vm::object::global_layout_names(layout_id) {
            for (idx, name) in field_names.iter().enumerate() {
                if name == key {
                    return Some(idx);
                }
            }
        }

        None
    }

    #[inline]
    pub(in crate::vm::interpreter) fn nominal_allocation(
        &self,
        nominal_type_id: usize,
    ) -> Option<(crate::vm::object::LayoutId, usize)> {
        self.layouts.read().nominal_allocation(nominal_type_id)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn nominal_layout_id(
        &self,
        nominal_type_id: usize,
    ) -> Option<crate::vm::object::LayoutId> {
        self.layouts.read().nominal_layout_id(nominal_type_id)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn register_nominal_layout(
        &self,
        nominal_type_id: usize,
        layout_id: crate::vm::object::LayoutId,
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

    #[inline]
    pub(in crate::vm::interpreter) fn allocate_nominal_layout_id(
        &self,
    ) -> crate::vm::object::LayoutId {
        self.layouts.write().allocate_nominal_layout_id()
    }

    #[inline]
    pub(in crate::vm::interpreter) fn register_runtime_class(&self, class: Class) -> usize {
        self.register_runtime_class_with_layout_names(class, None::<&[&str]>)
    }

    pub(in crate::vm::interpreter) fn register_runtime_class_with_layout_names(
        &self,
        class: Class,
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

    #[inline]
    pub(in crate::vm::interpreter) fn set_nominal_field_count(
        &self,
        nominal_type_id: usize,
        field_count: usize,
    ) -> bool {
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
    fn invalidate_jit_for_layout(&self, layout_id: crate::vm::object::LayoutId) {
        let Some(cache) = self.code_cache.as_ref() else {
            return;
        };
        let affected = cache.invalidate_layout(layout_id);
        let Some(profiles_map) = self.module_profiles_map else {
            return;
        };
        let profiles = profiles_map.read();
        for (checksum, func_index) in affected {
            if let Some(profile) = profiles.get(&checksum) {
                if let Some(func) = profile.get(func_index as usize) {
                    func.invalidate_compiled_code();
                }
            }
        }
    }

    #[cfg(not(feature = "jit"))]
    fn invalidate_jit_for_layout(&self, _layout_id: crate::vm::object::LayoutId) {}

    #[inline]
    fn format_exception_value(exception: Value) -> String {
        use crate::vm::gc::header_ptr_from_value_ptr;
        if exception.is_null() {
            return "null".to_string();
        }
        if !exception.is_ptr() {
            return format!("{:?}", exception);
        }

        let Some(ptr) = (unsafe { exception.as_ptr::<u8>() }) else {
            return format!("{:?}", exception);
        };
        let header = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<RayaString>() {
            let s = unsafe { &*ptr.cast::<RayaString>().as_ptr() };
            return s.data.clone();
        }
        if header.type_id() == std::any::TypeId::of::<Object>() {
            let obj = unsafe { &*ptr.cast::<Object>().as_ptr() };
            if let Some(msg_val) = obj.get_field(0) {
                if let Some(msg_ptr) = unsafe { msg_val.as_ptr::<u8>() } {
                    let msg_header = unsafe { &*header_ptr_from_value_ptr(msg_ptr.as_ptr()) };
                    if msg_header.type_id() == std::any::TypeId::of::<RayaString>() {
                        let s = unsafe { &*msg_ptr.cast::<RayaString>().as_ptr() };
                        return s.data.clone();
                    }
                }
            }
        }
        format!("{:?}", exception)
    }

    #[cfg(feature = "jit")]
    #[inline]
    fn record_native_resume_decision(
        telemetry: &Option<Arc<crate::vm::interpreter::JitTelemetry>>,
        resumed: bool,
    ) {
        if let Some(t) = telemetry {
            if resumed {
                t.resume_native_ok
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                t.resume_native_reject
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    #[cfg(feature = "jit")]
    #[inline]
    fn record_preemption_resume_decision(
        telemetry: &Option<Arc<crate::vm::interpreter::JitTelemetry>>,
        resumed: bool,
    ) {
        if let Some(t) = telemetry {
            if resumed {
                t.resume_preemption_ok
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                t.resume_preemption_reject
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    #[cfg(feature = "jit")]
    #[inline]
    fn materialize_native_resume_operands(
        func: &crate::compiler::Function,
        exit_info: &crate::jit::runtime::trampoline::JitExitInfo,
    ) -> Option<Vec<Value>> {
        let expected_arg_count =
            Self::native_resume_boundary_arg_count(func, exit_info.bytecode_offset)?;
        let mat_count = exit_info.native_arg_count as usize;
        let max_native_args = crate::jit::runtime::trampoline::JIT_EXIT_MAX_NATIVE_ARGS;
        if mat_count != expected_arg_count as usize || mat_count > max_native_args {
            return None;
        }

        let mut vals = Vec::with_capacity(mat_count);
        for i in 0..mat_count {
            vals.push(unsafe { Value::from_raw(exit_info.native_args[i]) });
        }
        Some(vals)
    }

    #[cfg(feature = "jit")]
    #[inline]
    fn materialize_interpreter_resume_stack(
        exit_info: &crate::jit::runtime::trampoline::JitExitInfo,
    ) -> Option<Vec<Value>> {
        let mat_count = exit_info.native_arg_count as usize;
        let max_native_args = crate::jit::runtime::trampoline::JIT_EXIT_MAX_NATIVE_ARGS;
        if mat_count > max_native_args {
            return None;
        }

        let mut vals = Vec::with_capacity(mat_count);
        for i in 0..mat_count {
            vals.push(unsafe { Value::from_raw(exit_info.native_args[i]) });
        }
        Some(vals)
    }

    #[cfg(feature = "jit")]
    #[inline]
    fn refresh_jit_module_context(&mut self, module: &Arc<Module>) -> Option<u64> {
        let jit_module_id = self
            .code_cache
            .as_ref()
            .and_then(|cache| cache.module_id(&module.checksum));
        self.current_module_for_profiling = Some(module.clone());
        self.current_module_id_for_profiling = jit_module_id;
        jit_module_id
    }

    /// Create a new task interpreter
    #[allow(clippy::too_many_arguments)] // Interpreter borrows many VM subsystems; a config struct would just move the problem.
    pub fn new(
        gc: &'a parking_lot::Mutex<GarbageCollector>,
        classes: &'a RwLock<ClassRegistry>,
        layouts: &'a RwLock<crate::vm::interpreter::RuntimeLayoutRegistry>,
        mutex_registry: &'a MutexRegistry,
        semaphore_registry: &'a SemaphoreRegistry,
        safepoint: &'a SafepointCoordinator,
        globals_by_index: &'a RwLock<Vec<Value>>,
        builtin_global_slots: &'a RwLock<FxHashMap<String, usize>>,
        js_global_bindings: &'a RwLock<
            FxHashMap<String, crate::vm::interpreter::shared_state::JsGlobalBindingRecord>,
        >,
        js_global_binding_slots: &'a RwLock<FxHashMap<usize, String>>,
        constant_string_cache: &'a RwLock<FxHashMap<(String, usize), Value>>,
        ephemeral_gc_roots: &'a RwLock<Vec<Value>>,
        pinned_handles: &'a RwLock<FxHashSet<u64>>,
        tasks: &'a Arc<RwLock<FxHashMap<TaskId, Arc<Task>>>>,
        injector: &'a Arc<Injector<Arc<Task>>>,
        metadata: &'a parking_lot::Mutex<crate::vm::reflect::MetadataStore>,
        class_metadata: &'a RwLock<crate::vm::reflect::ClassMetadataRegistry>,
        native_handler: &'a Arc<dyn NativeHandler>,
        module_layouts: &'a RwLock<
            FxHashMap<[u8; 32], crate::vm::interpreter::shared_state::ModuleRuntimeLayout>,
        >,
        module_registry: &'a RwLock<crate::vm::interpreter::module_registry::ModuleRegistry>,
        structural_shape_adapters: &'a RwLock<
            FxHashMap<
                crate::vm::interpreter::shared_state::StructuralAdapterKey,
                Arc<crate::vm::interpreter::shared_state::ShapeAdapter>,
            >,
        >,
        structural_shape_names: &'a RwLock<FxHashMap<crate::vm::object::ShapeId, Vec<String>>>,
        structural_object_shapes: &'a RwLock<FxHashMap<crate::vm::object::LayoutId, Vec<String>>>,
        type_handles: &'a RwLock<crate::vm::interpreter::shared_state::RuntimeTypeHandleRegistry>,
        class_value_slots: &'a RwLock<FxHashMap<usize, usize>>,
        prop_keys: &'a RwLock<crate::vm::interpreter::shared_state::PropertyKeyRegistry>,
        aot_profile: &'a RwLock<crate::aot_profile::AotProfileCollector>,
        io_submit_tx: Option<&'a crossbeam::channel::Sender<crate::vm::scheduler::IoSubmission>>,
        max_preemptions: u32,
        stack_pool: &'a crate::vm::scheduler::StackPool,
    ) -> Self {
        Self {
            gc,
            classes,
            layouts,
            mutex_registry,
            semaphore_registry,
            safepoint,
            globals_by_index,
            builtin_global_slots,
            js_global_bindings,
            js_global_binding_slots,
            constant_string_cache,
            ephemeral_gc_roots,
            pinned_handles,
            tasks,
            injector,
            metadata,
            class_metadata,
            native_handler,
            module_layouts,
            module_registry,
            structural_shape_adapters,
            structural_shape_names,
            structural_object_shapes,
            type_handles,
            class_value_slots,
            prop_keys,
            aot_profile,
            io_submit_tx,
            max_preemptions,
            stack_pool,
            debug_state: None,
            #[cfg(feature = "jit")]
            code_cache: None,
            #[cfg(feature = "jit")]
            module_profile: None,
            #[cfg(feature = "jit")]
            module_profiles_map: None,
            #[cfg(feature = "jit")]
            background_compiler: None,
            #[cfg(feature = "jit")]
            jit_telemetry: None,
            #[cfg(feature = "jit")]
            compilation_policy: crate::jit::profiling::policy::CompilationPolicy::new(),
            current_func_id_for_profiling: 0,
            #[cfg(feature = "jit")]
            current_module_for_profiling: None,
            #[cfg(feature = "jit")]
            current_module_id_for_profiling: None,
            profiler: None,
            profiler_func_id: 0,
            current_bytecode_offset_for_aot_profile: 0,
            current_module_checksum_for_aot_profile: [0; 32],
        }
    }

    /// Set the debug state for debugger coordination.
    pub fn set_debug_state(&mut self, debug_state: Option<Arc<super::debug_state::DebugState>>) {
        self.debug_state = debug_state;
    }

    /// Set the profiler for sampling.
    pub fn set_profiler(&mut self, profiler: Option<Arc<crate::profiler::Profiler>>) {
        self.profiler = profiler;
    }

    /// Set the JIT code cache for native dispatch.
    ///
    /// Called by the reactor worker after constructing the interpreter.
    #[cfg(feature = "jit")]
    pub fn set_code_cache(
        &mut self,
        cache: Option<Arc<crate::jit::runtime::code_cache::CodeCache>>,
    ) {
        self.code_cache = cache;
    }

    /// Set the module profile for on-the-fly JIT profiling.
    #[cfg(feature = "jit")]
    pub fn set_module_profile(
        &mut self,
        profile: Option<Arc<crate::jit::profiling::counters::ModuleProfile>>,
    ) {
        self.module_profile = profile;
    }

    /// Set the global module profile map so layout invalidation can clear
    /// per-function `jit_available` flags across modules.
    #[cfg(feature = "jit")]
    pub fn set_module_profiles_map(
        &mut self,
        profiles: Option<
            &'a RwLock<FxHashMap<[u8; 32], Arc<crate::jit::profiling::counters::ModuleProfile>>>,
        >,
    ) {
        self.module_profiles_map = profiles;
    }

    /// Set the background compiler handle for submitting compilation requests.
    #[cfg(feature = "jit")]
    pub fn set_background_compiler(
        &mut self,
        compiler: Option<Arc<crate::jit::profiling::BackgroundCompiler>>,
    ) {
        self.background_compiler = compiler;
    }

    /// Set shared JIT telemetry counters.
    #[cfg(feature = "jit")]
    pub fn set_jit_telemetry(
        &mut self,
        telemetry: Option<Arc<crate::vm::interpreter::JitTelemetry>>,
    ) {
        self.jit_telemetry = telemetry;
    }

    /// Set the compilation policy thresholds.
    #[cfg(feature = "jit")]
    pub fn set_compilation_policy(
        &mut self,
        policy: crate::jit::profiling::policy::CompilationPolicy,
    ) {
        self.compilation_policy = policy;
    }

    /// Wake a suspended task by setting its resume value and pushing it to the scheduler.
    #[allow(dead_code)]
    pub(in crate::vm::interpreter) fn wake_task(&self, task_id: u64, resume_value: Value) {
        let tasks = self.tasks.read();
        let target_id = TaskId::from_u64(task_id);
        if let Some(target_task) = tasks.get(&target_id) {
            target_task.set_resume_value(resume_value);
            target_task.set_state(TaskState::Resumed);
            target_task.clear_suspend_reason();
            self.injector.push(target_task.clone());
        }
    }

    /// Execute a task until completion, suspension, or failure
    ///
    /// This is the main entry point for running a task. Uses frame-based execution:
    /// function calls push a CallFrame and continue in the same loop. This allows
    /// suspension (channel operations, await, sleep) to work at any call depth.
    pub fn run(&mut self, task: &Arc<Task>) -> ExecutionResult {
        let mut module = task.current_module();

        // JIT: track module ID and profiling module context for the current frame module.
        #[cfg(feature = "jit")]
        let mut jit_module_id: Option<u64> = self.refresh_jit_module_context(&module);

        // Restore execution state (supports suspend/resume)
        let mut current_func_id = task.current_func_id();
        let mut frames: Vec<ExecutionFrame> = task.take_execution_frames();

        // Track current function for loop profiling
        #[cfg(feature = "jit")]
        {
            self.current_func_id_for_profiling = current_func_id;
        }
        self.profiler_func_id = current_func_id;

        let (entry_local_count, entry_param_count) = match module.functions.get(current_func_id) {
            Some(f) => (f.local_count, f.param_count),
            None => {
                return ExecutionResult::Failed(VmError::RuntimeError(format!(
                    "Function {} not found",
                    current_func_id
                )));
            }
        };

        let mut stack_guard = task.stack().lock().unwrap();
        let mut ip = task.ip();
        let mut code: &[u8] = &module.functions[current_func_id].code;
        let mut locals_base = task.current_locals_base();
        let mut current_arg_count = 0usize; // Track current function's arg count (for rest parameters)

        // Check if we're resuming from suspension.
        //
        // Most await sites expect the resumed task's value to be pushed back
        // onto the operand stack. `WaitAll` is different: it re-executes with
        // its original array operand already on the stack and does not consume
        // the resumed value from a single completed child task.
        if task.has_exception() {
            // Exception resumption path must not also materialize a prior resume value.
            // Mixing both can corrupt operand expectations at catch/unwind boundaries.
            let _ = task.take_resume_value();
        } else if let Some(resume_value) = task.take_resume_value() {
            let next_opcode = code.get(ip).and_then(|b| Opcode::from_u8(*b));
            if !matches!(next_opcode, Some(Opcode::WaitAll)) {
                if let Err(e) = stack_guard.push(resume_value) {
                    return ExecutionResult::Failed(e);
                }
            }
        }

        // Check if there's a pending exception (e.g., from awaited task failure).
        // Use the same frame-aware unwind logic as the main OpcodeResult::Error path.
        if task.has_exception() {
            let exception = task.current_exception().unwrap_or_else(Value::null);
            let mut handled = false;
            'resume_exception_search: loop {
                while let Some(handler) = task.peek_exception_handler() {
                    if handler.frame_count != frames.len() {
                        break;
                    }

                    while stack_guard.depth() > handler.stack_size {
                        let _ = stack_guard.pop();
                    }

                    if handler.catch_offset != -1 {
                        task.pop_exception_handler();
                        task.set_caught_exception(exception);
                        task.clear_exception();
                        let _ = stack_guard.push(exception);
                        ip = handler.catch_offset as usize;
                        handled = true;
                        break 'resume_exception_search;
                    }

                    if handler.finally_offset != -1 {
                        task.pop_exception_handler();
                        ip = handler.finally_offset as usize;
                        handled = true;
                        break 'resume_exception_search;
                    }

                    task.pop_exception_handler();
                }

                if let Some(frame) = frames.pop() {
                    task.clear_activation_direct_eval_env(current_func_id, locals_base);
                    task.pop_call_frame();
                    if frame.is_closure {
                        task.pop_closure();
                    }
                    module = frame.module;
                    task.set_current_module(module.clone());
                    current_func_id = frame.func_id;
                    #[cfg(feature = "jit")]
                    {
                        self.current_func_id_for_profiling = current_func_id;
                        jit_module_id = self.refresh_jit_module_context(&module);
                    }
                    code = &module.functions[frame.func_id].code;
                    ip = frame.ip;
                    locals_base = frame.locals_base;
                    current_arg_count = frame.arg_count;
                    task.set_current_func_id(current_func_id);
                    task.set_current_locals_base(locals_base);
                    task.set_current_arg_count(current_arg_count);
                } else {
                    break;
                }
            }

            if !handled {
                task.set_ip(ip);
                drop(stack_guard);
                return ExecutionResult::Failed(VmError::RuntimeError(format!(
                    "Unhandled exception from awaited task: {}",
                    Self::format_exception_value(exception)
                )));
            }
        }

        // Initialize the task if this is a fresh start
        if ip == 0 && stack_guard.depth() == 0 && frames.is_empty() {
            task.push_call_frame(current_func_id);

            let initial_args = task.take_initial_args();
            current_arg_count = initial_args.len();
            let initial_slot_count = entry_local_count.max(current_arg_count);

            for local_index in 0..initial_slot_count {
                let initial = if local_index < entry_param_count {
                    Value::undefined()
                } else {
                    Value::null()
                };
                if let Err(e) = stack_guard.push(initial) {
                    return ExecutionResult::Failed(e);
                }
            }

            for (i, arg) in initial_args.into_iter().enumerate() {
                if i < initial_slot_count {
                    if let Err(e) = stack_guard.set_at(i, arg) {
                        return ExecutionResult::Failed(e);
                    }
                }
            }
            task.set_current_func_id(current_func_id);
            task.set_current_locals_base(locals_base);
            task.set_current_arg_count(current_arg_count);
        }

        // Macro to save all frame state before leaving run()
        macro_rules! save_frame_state {
            () => {
                task.set_ip(ip);
                task.set_current_func_id(current_func_id);
                task.set_current_locals_base(locals_base);
                task.set_current_arg_count(current_arg_count);
                task.set_current_module(module.clone());
                task.save_execution_frames(frames);
            };
        }

        // Helper: handle return from current function (frame pop)
        // Returns None if frame popped successfully (continue execution),
        // or Some(ExecutionResult) if this was the top-level return.
        macro_rules! handle_frame_return {
            ($return_value:expr) => {{
                let return_value = $return_value;
                // A normal return can bypass bytecode `EndTry` cleanup.
                // Drop any handlers registered by the frame we're leaving so
                // later throws cannot jump into a dead catch/finally block.
                while task
                    .peek_exception_handler()
                    .is_some_and(|handler| handler.frame_count == frames.len())
                {
                    task.pop_exception_handler();
                }
                // Clean up current frame's locals and operand stack
                while stack_guard.depth() > locals_base {
                    let _ = stack_guard.pop();
                }
                task.clear_activation_direct_eval_env(current_func_id, locals_base);

                if let Some(frame) = frames.pop() {
                    task.pop_call_frame();
                    if frame.is_closure {
                        task.pop_closure();
                    }

                    // Restore caller's state
                    module = frame.module;
                    task.set_current_module(module.clone());
                    current_func_id = frame.func_id;
                    #[cfg(feature = "jit")]
                    {
                        self.current_func_id_for_profiling = current_func_id;
                        jit_module_id = self.refresh_jit_module_context(&module);
                    }
                    self.profiler_func_id = current_func_id;
                    code = &module.functions[frame.func_id].code;
                    ip = frame.ip;
                    locals_base = frame.locals_base;
                    current_arg_count = frame.arg_count;

                    // Push appropriate value onto caller's stack
                    if !matches!(frame.return_action, ReturnAction::Discard) {
                        let push_val = match frame.return_action {
                            ReturnAction::PushReturnValue => return_value,
                            ReturnAction::PushConstructResult(receiver) => {
                                self.constructor_result_or_receiver(return_value, receiver)
                            }
                            ReturnAction::Discard => unreachable!(),
                        };
                        match stack_guard.push(push_val) {
                            Ok(()) => None,
                            Err(e) => Some(ExecutionResult::Failed(e)),
                        }
                    } else {
                        None // Discard return value (super() call)
                    }
                } else {
                    // Top-level return - task is complete
                    Some(ExecutionResult::Completed(return_value))
                }
            }};
        }

        // Debug: break at entry point if requested
        if let Some(ref ds) = self.debug_state {
            if ds
                .break_at_entry
                .swap(false, std::sync::atomic::Ordering::AcqRel)
            {
                let bytecode_offset = ip as u32;
                let current_line =
                    self.lookup_line(module.as_ref(), current_func_id, bytecode_offset);
                let info = self.build_pause_info(
                    module.as_ref(),
                    current_func_id,
                    bytecode_offset,
                    current_line,
                    super::debug_state::PauseReason::Entry,
                );
                ds.signal_pause(info);
            }
        }

        // Main execution loop
        loop {
            // Safepoint poll for GC
            self.safepoint.poll();

            // Profiler: sample at preemption points (zero-cost when profiler is None)
            if let Some(ref profiler) = self.profiler {
                profiler.maybe_sample(task, self.profiler_func_id, ip);
            }

            // Check for preemption
            if task.is_preempt_requested() {
                task.clear_preempt();
                let count = task.increment_preempt_count();
                // Infinite loop detection: kill task after max_preemptions consecutive
                // preemptions without voluntary suspension
                if count >= self.max_preemptions {
                    save_frame_state!();
                    drop(stack_guard);
                    return ExecutionResult::Failed(VmError::RuntimeError(format!(
                        "Maximum execution time exceeded (task preempted {} times)",
                        count
                    )));
                }
                save_frame_state!();
                drop(stack_guard);
                return ExecutionResult::Suspended(SuspendReason::Sleep {
                    wake_at: Instant::now(),
                });
            }

            // Check for cancellation
            if task.is_cancelled() {
                save_frame_state!();
                drop(stack_guard);
                // Cancellation is observable to awaiters as a rejected task.
                // Unhandled rejection reporting already suppresses cancelled tasks.
                return ExecutionResult::Failed(VmError::RuntimeError(
                    "Task cancelled".to_string(),
                ));
            }

            // Bounds check - implicit return at end of function
            if ip >= code.len() {
                let local_count = module.functions[current_func_id].local_count;
                let return_value = if stack_guard.depth() > locals_base + local_count {
                    stack_guard.pop().unwrap_or_default()
                } else {
                    Value::null()
                };

                if let Some(result) = handle_frame_return!(return_value) {
                    return result;
                }
                continue;
            }

            // Fetch and decode opcode
            let opcode_byte = code[ip];
            let opcode = match Opcode::from_u8(opcode_byte) {
                Some(op) => op,
                None => {
                    if std::env::var("RAYA_DEBUG_INVALID_OPCODE").is_ok() {
                        let func_name = module
                            .functions
                            .get(current_func_id)
                            .map(|f| f.name.as_str())
                            .unwrap_or("<unknown>");
                        let start = ip.saturating_sub(8);
                        let end = (ip + 9).min(code.len());
                        let window = code[start..end]
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        eprintln!(
                            "[invalid-opcode] module={} func={}#{} ip={} byte=0x{:02X} window[{}..{}]={}",
                            module.metadata.name,
                            func_name,
                            current_func_id,
                            ip,
                            opcode_byte,
                            start,
                            end,
                            window
                        );
                    }
                    return ExecutionResult::Failed(VmError::InvalidOpcode(opcode_byte));
                }
            };

            ip += 1;

            if (std::env::var("RAYA_DEBUG_OPCODE_TRACE_ALL").is_ok()
                || (std::env::var("RAYA_DEBUG_OPCODE_TRACE").is_ok()
                    && matches!(
                        opcode,
                        Opcode::Call
                            | Opcode::DynGetKeyed
                            | Opcode::LoadFieldExact
                            | Opcode::LoadFieldShape
                            | Opcode::CallMethodExact
                            | Opcode::CallMethodShape
                            | Opcode::NativeCall
                    )))
            {
                let func_name = module
                    .functions
                    .get(current_func_id)
                    .map(|f| f.name.as_str())
                    .unwrap_or("<unknown>");
                eprintln!(
                    "[optrace] module={} {}#{} ip={} opcode={:?}",
                    module.metadata.name,
                    func_name,
                    current_func_id,
                    ip - 1,
                    opcode
                );
            }

            self.current_bytecode_offset_for_aot_profile = (ip - 1) as u32;
            self.current_module_checksum_for_aot_profile = module.checksum;

            // Debug check: test breakpoints, step modes, and debugger statements
            // when a debugger is attached. The fast path (no debugger) is a single
            // atomic relaxed load.
            if let Some(ref ds) = self.debug_state {
                if ds.active.load(std::sync::atomic::Ordering::Relaxed) {
                    let bytecode_offset = (ip - 1) as u32;
                    let current_line =
                        self.lookup_line(module.as_ref(), current_func_id, bytecode_offset);

                    // Check for `debugger;` statement first
                    let pause_reason = if opcode == Opcode::Debugger {
                        Some(super::debug_state::PauseReason::DebuggerStatement)
                    } else {
                        ds.should_break(
                            current_func_id,
                            bytecode_offset,
                            frames.len() + 1,
                            current_line,
                        )
                    };

                    if let Some(reason) = pause_reason {
                        if let super::debug_state::PauseReason::Breakpoint(bp_id) = &reason {
                            ds.increment_hit_count(*bp_id);
                        }
                        let info = self.build_pause_info(
                            module.as_ref(),
                            current_func_id,
                            bytecode_offset,
                            current_line,
                            reason,
                        );
                        ds.signal_pause(info);
                    }
                }
            }

            // Execute the opcode
            match self.execute_opcode(
                task,
                &mut stack_guard,
                &mut ip,
                code,
                module.as_ref(),
                opcode,
                locals_base,
                frames.len(),
                current_arg_count,
            ) {
                OpcodeResult::Continue => {
                    // Continue to next instruction
                }
                OpcodeResult::Return(value) => {
                    if let Some(result) = handle_frame_return!(value) {
                        return result;
                    }
                }
                OpcodeResult::Suspend(reason) => {
                    task.reset_preempt_count();
                    save_frame_state!();
                    drop(stack_guard);
                    return ExecutionResult::Suspended(reason);
                }
                OpcodeResult::PushFrame {
                    func_id,
                    arg_count,
                    is_closure,
                    closure_val,
                    module: callee_module,
                    return_action,
                } => {
                    let callee_module = callee_module.unwrap_or_else(|| module.clone());

                    #[cfg(feature = "jit")]
                    let mut forced_callee_ip: Option<usize> = None;
                    #[cfg(feature = "jit")]
                    let mut forced_callee_extra_locals: Option<Vec<u64>> = None;
                    #[cfg(feature = "jit")]
                    let mut forced_callee_operand_values: Option<Vec<Value>> = None;
                    #[cfg(feature = "jit")]
                    let jit_can_use_fast_path = Arc::ptr_eq(&callee_module, &module);

                    // JIT profiling: record call and check if function should be compiled
                    #[cfg(feature = "jit")]
                    if !is_closure && jit_can_use_fast_path {
                        if let Some(ref profile) = self.module_profile {
                            let count = profile.record_call(func_id);
                            self.aot_profile
                                .write()
                                .record_call(module.checksum, func_id as u32);
                            if let Some(ref telemetry) = self.jit_telemetry {
                                telemetry
                                    .call_samples
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                            // Check compilation policy periodically to amortize overhead
                            if count & crate::vm::defaults::JIT_POLICY_CHECK_MASK == 0 {
                                if let Some(mid) = jit_module_id {
                                    self.maybe_request_compilation(func_id, &module, mid);
                                }
                            }
                        }
                    }

                    // JIT fast path: dispatch to native code if available
                    // Only for non-closure, non-constructor calls (pure function calls)
                    #[cfg(feature = "jit")]
                    if !is_closure && jit_can_use_fast_path {
                        if let (Some(cache), Some(mid)) = (&self.code_cache, jit_module_id) {
                            if let Some(jit_fn) = cache.get(mid, func_id as u32) {
                                if let Some(ref telemetry) = self.jit_telemetry {
                                    telemetry
                                        .cache_hits
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                // Collect args from stack as NaN-boxed u64s
                                let args: Vec<u64> = (0..arg_count)
                                    .map(|i| {
                                        stack_guard
                                            .peek_at(stack_guard.depth() - arg_count + i)
                                            .unwrap_or_default()
                                            .raw()
                                    })
                                    .collect();

                                let func = &module.functions[func_id];
                                let local_count = func.local_count;
                                let mut locals_buf = vec![Value::null().raw(); local_count];
                                for (i, raw) in args.iter().copied().enumerate().take(local_count) {
                                    locals_buf[i] = raw;
                                }
                                let mut exit_info =
                                    crate::jit::runtime::trampoline::JitExitInfo::default();
                                let jit_resolved_natives =
                                    parking_lot::RwLock::new(self.module_resolved_natives(&module));
                                let bridge_ctx =
                                    crate::jit::runtime::helpers::build_runtime_bridge_context(
                                        self.safepoint,
                                        task,
                                        self.gc,
                                        self.classes,
                                        self.layouts,
                                        self.mutex_registry,
                                        self.semaphore_registry,
                                        self.globals_by_index,
                                        self.builtin_global_slots,
                                        self.js_global_bindings,
                                        self.js_global_binding_slots,
                                        self.constant_string_cache,
                                        self.ephemeral_gc_roots,
                                        self.pinned_handles,
                                        self.tasks,
                                        self.injector,
                                        self.module_layouts,
                                        self.module_registry,
                                        self.metadata,
                                        self.class_metadata,
                                        self.native_handler,
                                        &jit_resolved_natives,
                                        self.structural_shape_names,
                                        self.structural_object_shapes,
                                        self.structural_shape_adapters,
                                        self.aot_profile,
                                        self.type_handles,
                                        self.class_value_slots,
                                        self.prop_keys,
                                        self.stack_pool,
                                        self.max_preemptions,
                                        frames.len(),
                                        self.io_submit_tx,
                                    );
                                let mut runtime_ctx =
                                    crate::jit::runtime::helpers::build_runtime_context(
                                        &bridge_ctx,
                                        module.as_ref(),
                                    );

                                // Call JIT-compiled function with runtime context so safepoint/preemption
                                // branches inside machine code can hand off to interpreter thread loop.
                                let result = unsafe {
                                    jit_fn(
                                        args.as_ptr(),
                                        arg_count as u32,
                                        locals_buf.as_mut_ptr(),
                                        local_count as u32,
                                        (&mut runtime_ctx as *mut _),
                                        (&mut exit_info as *mut _),
                                    )
                                };

                                // Pop args from stack
                                for _ in 0..arg_count {
                                    let _ = stack_guard.pop();
                                }

                                // Future-proof exit handling for suspension/deopt bridges.
                                if exit_info.kind
                                    != crate::jit::runtime::trampoline::JitExitKind::Completed
                                        as u32
                                {
                                    // Fallback safely to interpreter execution path.
                                    // Leave no JIT-only side effects behind.
                                    if let Some(ref telemetry) = self.jit_telemetry {
                                        telemetry
                                            .cache_misses
                                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    }
                                    // Re-push args so normal interpreter call-frame setup can proceed.
                                    for raw in &args {
                                        if let Err(e) =
                                            stack_guard.push(unsafe { Value::from_raw(*raw) })
                                        {
                                            return ExecutionResult::Failed(e);
                                        }
                                    }

                                    // Conservative continuation: if JIT suspended exactly at a
                                    // zero-arg native boundary, resume interpreter from that
                                    // bytecode offset instead of restarting the whole callee.
                                    if exit_info.kind
                                        == crate::jit::runtime::trampoline::JitExitKind::Suspended
                                            as u32
                                    {
                                        match exit_info.suspend_reason {
                                            x if x
                                                == crate::jit::runtime::trampoline::JitSuspendReason::NativeCallBoundary
                                                    as u32 =>
                                            {
                                                let resumed = if let Some(vals) =
                                                    Self::materialize_native_resume_operands(
                                                        func, &exit_info,
                                                    )
                                                {
                                                    forced_callee_ip =
                                                        Some(exit_info.bytecode_offset as usize);
                                                    forced_callee_extra_locals =
                                                        Some(locals_buf.clone());
                                                    forced_callee_operand_values = Some(vals);
                                                    true
                                                } else {
                                                    false
                                                };
                                                Self::record_native_resume_decision(
                                                    &self.jit_telemetry,
                                                    resumed,
                                                );
                                            }
                                            x if x
                                                == crate::jit::runtime::trampoline::JitSuspendReason::Preemption
                                                    as u32 =>
                                            {
                                                let resumed =
                                                    if Self::can_resume_at_preemption_boundary(
                                                        func,
                                                        exit_info.bytecode_offset,
                                                    ) {
                                                        forced_callee_ip =
                                                            Some(exit_info.bytecode_offset as usize);
                                                        forced_callee_extra_locals =
                                                            Some(locals_buf.clone());
                                                        true
                                                    } else {
                                                        false
                                                    };
                                                Self::record_preemption_resume_decision(
                                                    &self.jit_telemetry,
                                                    resumed,
                                                );
                                            }
                                            x if x
                                                == crate::jit::runtime::trampoline::JitSuspendReason::InterpreterBoundary
                                                    as u32 =>
                                            {
                                                if let Some(vals) =
                                                    Self::materialize_interpreter_resume_stack(
                                                        &exit_info,
                                                    )
                                                {
                                                    forced_callee_ip =
                                                        Some(exit_info.bytecode_offset as usize);
                                                    forced_callee_extra_locals =
                                                        Some(locals_buf.clone());
                                                    forced_callee_operand_values = Some(vals);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    // continue below into bytecode frame setup (no `continue`)
                                } else {
                                    // Push return value (or handle based on return_action)
                                    // Safety: result is a NaN-boxed Value returned by JIT-compiled code
                                    let return_val = unsafe { Value::from_raw(result) };
                                    match return_action {
                                        ReturnAction::PushReturnValue => {
                                            if let Err(e) = stack_guard.push(return_val) {
                                                return ExecutionResult::Failed(e);
                                            }
                                        }
                                        ReturnAction::PushConstructResult(receiver) => {
                                            let construct_value = self
                                                .constructor_result_or_receiver(
                                                    return_val, receiver,
                                                );
                                            if let Err(e) = stack_guard.push(construct_value) {
                                                return ExecutionResult::Failed(e);
                                            }
                                        }
                                        ReturnAction::Discard => {}
                                    }
                                    continue; // skip bytecode frame setup
                                }
                            } else if let Some(ref telemetry) = self.jit_telemetry {
                                telemetry
                                    .cache_misses
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }

                    // Validate function index
                    let new_func = match callee_module.functions.get(func_id) {
                        Some(f) => f,
                        None => {
                            return ExecutionResult::Failed(VmError::RuntimeError(format!(
                                "Invalid function index: {}",
                                func_id
                            )));
                        }
                    };
                    let new_local_count = new_func.local_count;
                    let new_param_count = new_func.param_count;

                    // Save caller's frame
                    frames.push(ExecutionFrame {
                        module: module.clone(),
                        func_id: current_func_id,
                        ip,
                        locals_base,
                        is_closure,
                        return_action,
                        arg_count: current_arg_count, // Save caller's arg count
                    });

                    // Push call frame for stack traces
                    task.push_call_frame(func_id);

                    // Push closure onto closure stack if needed
                    if let Some(cv) = closure_val {
                        task.push_closure(cv);
                    }

                    // Set up callee's frame on the same stack
                    // Args are already on the stack from the caller
                    locals_base = stack_guard.depth() - arg_count;

                    // Allocate remaining slots. Missing parameter slots must materialize as
                    // `undefined` in JS-compatible code; non-parameter locals stay `null`.
                    // Note: If arg_count > new_local_count, we don't discard extras.
                    // This allows rest parameters to access all arguments via LoadArgLocal.
                    for local_index in arg_count..new_local_count {
                        let initial = if local_index < new_param_count {
                            Value::undefined()
                        } else {
                            Value::null()
                        };
                        if let Err(e) = stack_guard.push(initial) {
                            return ExecutionResult::Failed(e);
                        }
                    }

                    // First stack materialization piece:
                    // if we resume interpreter from a JIT suspension, restore the
                    // full boxed locals buffer exactly as JIT saw it. The buffer now
                    // includes argument slots at the front, so restore from locals_base.
                    #[cfg(feature = "jit")]
                    if forced_callee_ip.is_some() {
                        if let Some(extra_locals) = forced_callee_extra_locals.as_ref() {
                            for (i, raw) in extra_locals.iter().enumerate() {
                                let slot = locals_base + i;
                                if slot >= stack_guard.depth() {
                                    break;
                                }
                                if let Err(e) =
                                    stack_guard.set_at(slot, unsafe { Value::from_raw(*raw) })
                                {
                                    return ExecutionResult::Failed(e);
                                }
                            }
                            if let Some(operand_vals) = forced_callee_operand_values.as_ref() {
                                if std::env::var("RAYA_JIT_DEBUG_CALLS").is_ok() {
                                    let rendered = operand_vals
                                        .iter()
                                        .map(|v| format!("{v:?}/0x{:016x}", v.raw()))
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    eprintln!(
                                        "jit interpreter boundary resume: ip={} locals_base={} arg_count={} operands=[{}]",
                                        forced_callee_ip.unwrap_or_default(),
                                        locals_base,
                                        arg_count,
                                        rendered
                                    );
                                }
                                for v in operand_vals {
                                    if let Err(e) = stack_guard.push(*v) {
                                        return ExecutionResult::Failed(e);
                                    }
                                }
                            }
                        }
                    }

                    // Switch to callee's code
                    module = callee_module;
                    task.set_current_module(module.clone());
                    current_func_id = func_id;
                    #[cfg(feature = "jit")]
                    {
                        self.current_func_id_for_profiling = current_func_id;
                        jit_module_id = self.refresh_jit_module_context(&module);
                    }
                    self.profiler_func_id = current_func_id;
                    code = &module.functions[func_id].code;
                    current_arg_count = arg_count; // Set current arg count to callee's arg count
                    task.set_current_func_id(current_func_id);
                    task.set_current_locals_base(locals_base);
                    task.set_current_arg_count(current_arg_count);
                    if std::env::var("RAYA_DEBUG_ARGS_ENTRY").is_ok() {
                        let func_name = module
                            .functions
                            .get(func_id)
                            .map(|f| f.name.as_str())
                            .unwrap_or("<unknown>");
                        if func_name.ends_with("::fill") {
                            eprintln!(
                                "[args-entry] {} arg_count={} locals_base={}",
                                func_name, current_arg_count, locals_base
                            );
                        }
                    }
                    #[cfg(feature = "jit")]
                    {
                        ip = forced_callee_ip.unwrap_or(0);
                    }
                    #[cfg(not(feature = "jit"))]
                    {
                        ip = 0;
                    }
                }
                OpcodeResult::Error(e) => {
                    if matches!(e, VmError::StackUnderflow)
                        && std::env::var("RAYA_DEBUG_STACK_UNDERFLOW").is_ok()
                    {
                        let func_name = module
                            .functions
                            .get(current_func_id)
                            .map(|f| f.name.as_str())
                            .unwrap_or("<unknown>");
                        eprintln!(
                            "[stack-underflow] module={} func={}#{} ip={} opcode={:?} depth={} locals_base={}",
                            module.metadata.name,
                            func_name,
                            current_func_id,
                            ip.saturating_sub(1),
                            opcode,
                            stack_guard.depth(),
                            locals_base
                        );
                    }
                    // Set exception on task if not already set
                    if !task.has_exception() {
                        let exc_val = match &e {
                            VmError::TypeError(message) => {
                                self.alloc_builtin_error_value("TypeError", message)
                            }
                            VmError::SyntaxError(message) => {
                                self.alloc_builtin_error_value("SyntaxError", message)
                            }
                            VmError::RangeError(message) => {
                                self.alloc_builtin_error_value("RangeError", message)
                            }
                            VmError::ReferenceError(message) => {
                                self.alloc_builtin_error_value("ReferenceError", message)
                            }
                            VmError::RuntimeError(message) => {
                                self.alloc_builtin_error_value("Error", message)
                            }
                            _ => {
                                let error_msg = e.to_string();
                                let mut gc = self.gc.lock();
                                let gc_ptr = gc.allocate(RayaString::new(error_msg));
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                }
                            }
                        };
                        self.ephemeral_gc_roots.write().push(exc_val);
                        task.set_exception(exc_val);
                        let mut ephemeral = self.ephemeral_gc_roots.write();
                        if let Some(index) = ephemeral
                            .iter()
                            .rposition(|candidate| *candidate == exc_val)
                        {
                            ephemeral.swap_remove(index);
                        }
                    }

                    let exception = task.current_exception().unwrap_or_else(Value::null);

                    // Frame-aware exception handling: search for handlers,
                    // unwinding frames as needed to find a catch/finally block.
                    let mut handled = false;
                    'exception_search: loop {
                        // Process handlers that belong to the current frame depth
                        while let Some(handler) = task.peek_exception_handler() {
                            if handler.frame_count != frames.len() {
                                // This handler belongs to a different frame, stop
                                break;
                            }

                            // Unwind stack to handler's saved state
                            while stack_guard.depth() > handler.stack_size {
                                let _ = stack_guard.pop();
                            }

                            if handler.catch_offset != -1 {
                                task.pop_exception_handler();
                                task.set_caught_exception(exception);
                                task.clear_exception();
                                let _ = stack_guard.push(exception);
                                ip = handler.catch_offset as usize;
                                handled = true;
                                break 'exception_search;
                            }

                            if handler.finally_offset != -1 {
                                task.pop_exception_handler();
                                ip = handler.finally_offset as usize;
                                handled = true;
                                break 'exception_search;
                            }

                            // No catch or finally, pop and continue
                            task.pop_exception_handler();
                        }

                        // No handler in current frame — pop frame and try parent
                        if let Some(frame) = frames.pop() {
                            task.clear_activation_direct_eval_env(current_func_id, locals_base);
                            task.pop_call_frame();
                            if frame.is_closure {
                                task.pop_closure();
                            }
                            // Restore caller's context — don't clean stack here,
                            // the exception handler's stack_size will handle unwinding
                            module = frame.module;
                            task.set_current_module(module.clone());
                            current_func_id = frame.func_id;
                            #[cfg(feature = "jit")]
                            {
                                self.current_func_id_for_profiling = current_func_id;
                                jit_module_id = self.refresh_jit_module_context(&module);
                            }
                            code = &module.functions[frame.func_id].code;
                            ip = frame.ip;
                            locals_base = frame.locals_base;
                            current_arg_count = frame.arg_count; // Restore caller's arg count
                            task.set_current_func_id(current_func_id);
                            task.set_current_locals_base(locals_base);
                            task.set_current_arg_count(current_arg_count);
                            // Continue searching in parent frame
                        } else {
                            // No more frames — unhandled exception
                            break;
                        }
                    }

                    if !handled {
                        task.set_ip(ip);
                        drop(stack_guard);
                        return ExecutionResult::Failed(e);
                    }
                }
            }
        }
    }

    /// Execute a single opcode
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_opcode(
        &mut self,
        task: &Arc<Task>,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        opcode: Opcode,
        locals_base: usize,
        frame_depth: usize,
        current_arg_count: usize,
    ) -> OpcodeResult {
        match opcode {
            // =========================================================
            // Stack Manipulation
            // =========================================================
            Opcode::Nop | Opcode::Pop | Opcode::Dup | Opcode::Swap => {
                self.exec_stack_ops(stack, opcode)
            }

            // =========================================================
            // Constants
            // =========================================================
            Opcode::ConstNull
            | Opcode::ConstUndefined
            | Opcode::ConstTrue
            | Opcode::ConstFalse
            | Opcode::ConstI32
            | Opcode::ConstF64
            | Opcode::ConstStr => self.exec_constant_ops(stack, ip, code, module, opcode),

            // =========================================================
            // Variables
            // =========================================================
            Opcode::LoadLocal
            | Opcode::StoreLocal
            | Opcode::LoadLocal0
            | Opcode::LoadLocal1
            | Opcode::StoreLocal0
            | Opcode::StoreLocal1
            | Opcode::GetArgCount
            | Opcode::LoadArgLocal
            | Opcode::LoadGlobal
            | Opcode::StoreGlobal => self.exec_variable_ops(
                stack,
                ip,
                code,
                module,
                task,
                locals_base,
                opcode,
                current_arg_count,
            ),

            // =========================================================
            // Integer and Float Arithmetic
            // =========================================================
            Opcode::Iadd
            | Opcode::Isub
            | Opcode::Imul
            | Opcode::Idiv
            | Opcode::Imod
            | Opcode::Ineg
            | Opcode::Ipow
            | Opcode::Ishl
            | Opcode::Ishr
            | Opcode::Iushr
            | Opcode::Iand
            | Opcode::Ior
            | Opcode::Ixor
            | Opcode::Inot
            | Opcode::Fadd
            | Opcode::Fsub
            | Opcode::Fmul
            | Opcode::Fdiv
            | Opcode::Fneg
            | Opcode::Fpow
            | Opcode::Fmod => self.exec_arithmetic_ops(stack, module, task, opcode),

            // =========================================================
            // Comparisons and Logical Operators
            // =========================================================
            Opcode::Ieq
            | Opcode::Ine
            | Opcode::Ilt
            | Opcode::Ile
            | Opcode::Igt
            | Opcode::Ige
            | Opcode::Feq
            | Opcode::Fne
            | Opcode::Flt
            | Opcode::Fle
            | Opcode::Fgt
            | Opcode::Fge
            | Opcode::Not
            | Opcode::And
            | Opcode::Or
            | Opcode::Eq
            | Opcode::Ne
            | Opcode::StrictEq
            | Opcode::StrictNe => self.exec_comparison_ops(stack, module, task, opcode),

            // =========================================================
            // Control Flow
            // =========================================================
            Opcode::Jmp
            | Opcode::JmpIfTrue
            | Opcode::JmpIfFalse
            | Opcode::JmpIfNull
            | Opcode::JmpIfNotNull
            | Opcode::Return
            | Opcode::ReturnVoid => self.exec_control_flow_ops(stack, ip, code, opcode),

            // =========================================================
            // Exception Handling
            // =========================================================
            Opcode::Try | Opcode::EndTry | Opcode::Throw | Opcode::Rethrow => {
                self.exec_exception_ops(stack, ip, code, task, frame_depth, opcode)
            }

            // =========================================================
            // Object Operations
            // =========================================================
            Opcode::NewType
            | Opcode::LoadFieldExact
            | Opcode::LoadFieldShape
            | Opcode::StoreFieldExact
            | Opcode::StoreFieldShape
            | Opcode::OptionalFieldExact
            | Opcode::OptionalFieldShape
            | Opcode::ObjectLiteral
            | Opcode::InitObject
            | Opcode::BindMethod => self.exec_object_ops(stack, ip, code, module, task, opcode),

            // =========================================================
            // Array Operations
            // =========================================================
            Opcode::NewArray
            | Opcode::LoadElem
            | Opcode::StoreElem
            | Opcode::ArrayLen
            | Opcode::ArrayPush
            | Opcode::ArrayPop
            | Opcode::ArrayLiteral
            | Opcode::InitArray => self.exec_array_ops(stack, ip, code, opcode),

            // =========================================================
            // Closure Operations
            // =========================================================
            Opcode::MakeClosure
            | Opcode::LoadCaptured
            | Opcode::StoreCaptured
            | Opcode::SetClosureCapture
            | Opcode::NewRefCell
            | Opcode::LoadRefCell
            | Opcode::StoreRefCell => self.exec_closure_ops(stack, ip, code, module, task, opcode),

            // =========================================================
            // String Operations
            // =========================================================
            Opcode::Sconcat
            | Opcode::Slen
            | Opcode::Seq
            | Opcode::Sne
            | Opcode::Slt
            | Opcode::Sle
            | Opcode::Sgt
            | Opcode::Sge
            | Opcode::ToString => self.exec_string_ops(stack, opcode),

            // =========================================================
            // Concurrency (needs MutexGuard for Await/WaitAll suspension)
            // =========================================================
            Opcode::Spawn
            | Opcode::SpawnClosure
            | Opcode::Await
            | Opcode::WaitAll
            | Opcode::Sleep
            | Opcode::MutexLock
            | Opcode::MutexUnlock
            | Opcode::SemAcquire
            | Opcode::SemRelease
            | Opcode::Yield
            | Opcode::TaskCancel => {
                self.exec_concurrency_ops(stack, ip, code, module, task, opcode)
            }

            // =========================================================
            // Function Calls (needs MutexGuard for frame operations)
            // =========================================================
            Opcode::Call
            | Opcode::CallMethodExact
            | Opcode::OptionalCallMethodExact
            | Opcode::CallMethodShape
            | Opcode::OptionalCallMethodShape
            | Opcode::ConstructType
            | Opcode::CallConstructor
            | Opcode::CallSuper => self.exec_call_ops(stack, ip, code, module, task, opcode),

            // =========================================================
            // Native Calls (needs MutexGuard for suspend/resume)
            // =========================================================
            Opcode::NativeCall | Opcode::ModuleNativeCall => {
                self.exec_native_ops(stack, ip, code, module, task, opcode)
            }

            // =========================================================
            // Type Operations, JSON, Static Fields, Channels, Mutexes
            // =========================================================
            Opcode::IsNominal
            | Opcode::ImplementsShape
            | Opcode::CastShape
            | Opcode::CastTupleLen
            | Opcode::CastObjectMinFields
            | Opcode::CastArrayElemKind
            | Opcode::CastKindMask
            | Opcode::CastNominal
            | Opcode::DynGetKeyed
            | Opcode::DynSetKeyed
            | Opcode::NewMutex
            | Opcode::NewSemaphore
            | Opcode::NewChannel
            | Opcode::LoadStatic
            | Opcode::StoreStatic
            | Opcode::Typeof => self.exec_type_ops(stack, ip, code, module, task, opcode),

            // =========================================================
            // Debugger Statement
            // =========================================================
            Opcode::Debugger => {
                // The actual pause is handled in the main loop via the
                // `debugger_pause` flag. This handler is a no-op.
                OpcodeResult::Continue
            }

            // =========================================================
            // Catch-all for unimplemented opcodes
            // =========================================================
            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Opcode {:?} not yet implemented in Interpreter",
                opcode
            ))),
        }
    }

    // ===== Helper Methods =====

    #[inline]
    pub(in crate::vm::interpreter) fn read_u8(code: &[u8], ip: &mut usize) -> Result<u8, VmError> {
        if *ip >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = code[*ip];
        *ip += 1;
        Ok(value)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn read_u16(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<u16, VmError> {
        if *ip + 1 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = u16::from_le_bytes([code[*ip], code[*ip + 1]]);
        *ip += 2;
        Ok(value)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn read_i16(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<i16, VmError> {
        if *ip + 1 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = i16::from_le_bytes([code[*ip], code[*ip + 1]]);
        *ip += 2;
        Ok(value)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn read_u32(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<u32, VmError> {
        if *ip + 3 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = u32::from_le_bytes([code[*ip], code[*ip + 1], code[*ip + 2], code[*ip + 3]]);
        *ip += 4;
        Ok(value)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn read_u64(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<u64, VmError> {
        if *ip + 7 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = u64::from_le_bytes([
            code[*ip],
            code[*ip + 1],
            code[*ip + 2],
            code[*ip + 3],
            code[*ip + 4],
            code[*ip + 5],
            code[*ip + 6],
            code[*ip + 7],
        ]);
        *ip += 8;
        Ok(value)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn read_i32(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<i32, VmError> {
        if *ip + 3 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
        }
        let value = i32::from_le_bytes([code[*ip], code[*ip + 1], code[*ip + 2], code[*ip + 3]]);
        *ip += 4;
        Ok(value)
    }

    #[inline]
    pub(in crate::vm::interpreter) fn read_f64(
        code: &[u8],
        ip: &mut usize,
    ) -> Result<f64, VmError> {
        if *ip + 7 >= code.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of bytecode".to_string(),
            ));
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

    /// Handle built-in runtime methods (std:runtime)
    ///
    /// Bridge between Interpreter's call convention (pre-popped args Vec)
    /// and the runtime handler's stack-based convention.
    pub(in crate::vm::interpreter) fn call_runtime_method(
        &mut self,
        _task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        args: Vec<Value>,
        _module: &Module,
    ) -> Result<(), VmError> {
        let ctx = RuntimeHandlerContext {
            gc: self.gc,
            classes: self.classes,
            layouts: self.layouts,
        };

        // Push args back onto stack so the handler can pop them
        let arg_count = args.len();
        for arg in args {
            stack.push(arg)?;
        }

        runtime_handler(&ctx, stack, method_id, arg_count)
    }

    /// Check if a function should be compiled on-the-fly and submit a request.
    ///
    /// Called after profiling counters are incremented. Uses the compilation policy
    /// to decide, then CAS-claims the function and sends a request to the background thread.
    #[cfg(feature = "jit")]
    pub(in crate::vm::interpreter) fn maybe_request_compilation(
        &self,
        func_id: usize,
        module: &Arc<Module>,
        module_id: u64,
    ) {
        let Some(ref profile) = self.module_profile else {
            return;
        };
        let Some(func_profile) = profile.get(func_id) else {
            return;
        };

        // Already compiled or in progress
        if func_profile.is_jit_available() {
            return;
        }

        if let Some(func) = module.functions.get(func_id) {
            if !crate::jit::analysis::heuristics::function_supported_for_jit(func) {
                func_profile.finish_compile_failed();
                return;
            }
        } else {
            return;
        }

        let code_size = module
            .functions
            .get(func_id)
            .map(|f| f.code.len())
            .unwrap_or(0);
        if !self
            .compilation_policy
            .should_compile(func_profile, code_size)
        {
            return;
        }

        // CAS to claim this function for compilation (prevents duplicate requests)
        if !func_profile.try_start_compile() {
            return;
        }

        // Submit to background compiler
        if let Some(ref compiler) = self.background_compiler {
            let request = crate::jit::profiling::CompilationRequest {
                module: module.clone(),
                func_index: func_id,
                module_id,
                module_profile: profile.clone(),
            };
            let accepted = compiler.try_submit(request);
            if let Some(ref telemetry) = self.jit_telemetry {
                if accepted {
                    telemetry
                        .compile_requests_submitted
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    telemetry
                        .compile_requests_dropped
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
    }

    /// Conservative check for direct resume into a native-call suspension point.
    ///
    /// Safe only for zero-arg NativeCall/ModuleNativeCall and when the
    /// bytecode prefix guarantees an empty operand stack at resume point.
    #[cfg(feature = "jit")]
    fn native_resume_boundary_arg_count(
        func: &crate::compiler::Function,
        bytecode_offset: u32,
    ) -> Option<u8> {
        let offset = bytecode_offset as usize;
        let code = &func.code;
        if offset >= code.len() {
            return None;
        }
        let op = code[offset];
        if op != Opcode::NativeCall as u8 && op != Opcode::ModuleNativeCall as u8 {
            return None;
        }
        // Encoding: opcode (1) + native_id (2) + arg_count (1)
        if offset + 3 >= code.len() {
            return None;
        }
        let arg_count = code[offset + 3];

        // First materialization phase safety rule:
        // resume only if operand stack depth at target is statically zero.
        if Self::conservative_stack_depth_until(code, offset) != Some(0) {
            return None;
        }
        Some(arg_count)
    }

    /// Conservative check for resume at a preemption boundary.
    ///
    /// Currently limited to unconditional `Jmp` sites with statically-empty
    /// operand stack at the resume offset.
    #[cfg(feature = "jit")]
    fn can_resume_at_preemption_boundary(
        func: &crate::compiler::Function,
        bytecode_offset: u32,
    ) -> bool {
        let offset = bytecode_offset as usize;
        let code = &func.code;
        if offset >= code.len() {
            return false;
        }
        if code[offset] != Opcode::Jmp as u8 {
            return false;
        }
        if Self::conservative_stack_depth_until(code, offset) != Some(0) {
            return false;
        }
        true
    }

    /// Conservative linear stack-depth evaluator for resume safety.
    ///
    /// Returns `None` for unsupported/control-flow opcodes or malformed bytecode.
    #[cfg(feature = "jit")]
    fn conservative_stack_depth_until(code: &[u8], target_offset: usize) -> Option<i32> {
        let mut ip = 0usize;
        let mut depth = 0i32;

        while ip < target_offset {
            let op = Opcode::from_u8(*code.get(ip)?)?;
            ip += 1;

            let (pop, push, imm): (i32, i32, usize) = match op {
                // constants
                Opcode::ConstNull
                | Opcode::ConstUndefined
                | Opcode::ConstTrue
                | Opcode::ConstFalse
                | Opcode::LoadLocal0
                | Opcode::LoadLocal1
                | Opcode::LoadGlobal
                | Opcode::LoadConst => (
                    0,
                    1,
                    match op {
                        Opcode::LoadGlobal | Opcode::LoadConst => 4,
                        _ => 0,
                    },
                ),
                Opcode::ConstI32 => (0, 1, 4),
                Opcode::ConstF64 => (0, 1, 8),
                Opcode::ConstStr => (0, 1, 2),
                Opcode::ConstUndefined => (0, 1, 0),
                Opcode::LoadLocal => (0, 1, 2),

                // stores/stack ops
                Opcode::StoreLocal0 | Opcode::StoreLocal1 | Opcode::Pop => (1, 0, 0),
                Opcode::StoreLocal => (1, 0, 2),
                Opcode::Dup => (1, 2, 0),
                Opcode::Swap => (2, 2, 0),

                // integer arithmetic/comparison
                Opcode::Iadd
                | Opcode::Isub
                | Opcode::Imul
                | Opcode::Idiv
                | Opcode::Imod
                | Opcode::Ieq
                | Opcode::Ine
                | Opcode::Ilt
                | Opcode::Ile
                | Opcode::Igt
                | Opcode::Ige => (2, 1, 0),
                Opcode::Ineg => (1, 1, 0),

                // float arithmetic/comparison
                Opcode::Fadd
                | Opcode::Fsub
                | Opcode::Fmul
                | Opcode::Fdiv
                | Opcode::Feq
                | Opcode::Fne
                | Opcode::Flt
                | Opcode::Fle
                | Opcode::Fgt
                | Opcode::Fge => (2, 1, 0),
                Opcode::Fneg => (1, 1, 0),

                // conservative stop: control flow/calls/exceptions/other complex ops
                _ => return None,
            };

            ip = ip.checked_add(imm)?;
            if ip > target_offset {
                return None;
            }

            depth -= pop;
            if depth < 0 {
                return None;
            }
            depth += push;
        }

        if ip == target_offset {
            Some(depth)
        } else {
            None
        }
    }

    /// Look up the source line for a bytecode offset in a function.
    /// Returns 0 if debug info is unavailable.
    #[inline]
    fn lookup_line(&self, module: &Module, func_id: usize, bytecode_offset: u32) -> u32 {
        module
            .debug_info
            .as_ref()
            .and_then(|di| di.functions.get(func_id))
            .and_then(|fd| fd.lookup_location(bytecode_offset))
            .map(|entry| entry.line)
            .unwrap_or(0)
    }

    /// Build a PauseInfo struct from the current execution state.
    fn build_pause_info(
        &self,
        module: &Module,
        func_id: usize,
        bytecode_offset: u32,
        current_line: u32,
        reason: super::debug_state::PauseReason,
    ) -> super::debug_state::PauseInfo {
        let (source_file, column) = module
            .debug_info
            .as_ref()
            .and_then(|di| {
                let fd = di.functions.get(func_id)?;
                let entry = fd.lookup_location(bytecode_offset)?;
                let file = di
                    .source_files
                    .get(fd.source_file_index as usize)
                    .cloned()
                    .unwrap_or_default();
                Some((file, entry.column))
            })
            .unwrap_or_else(|| (String::new(), 0));

        let function_name = module
            .functions
            .get(func_id)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| format!("<func_{}>", func_id));

        super::debug_state::PauseInfo {
            func_id,
            bytecode_offset,
            source_file,
            line: current_line,
            column,
            reason,
            function_name,
        }
    }

    /// Signal debug completion or failure after a task finishes.
    ///
    /// Called by the reactor after `run()` returns a terminal result (Completed or Failed).
    /// Suspended tasks don't signal — they'll signal on final completion.
    pub fn signal_debug_result(&self, result: &ExecutionResult) {
        if let Some(ref ds) = self.debug_state {
            if ds.active.load(std::sync::atomic::Ordering::Relaxed) {
                match result {
                    ExecutionResult::Completed(value) => {
                        ds.signal_completed(value.raw() as i64);
                    }
                    ExecutionResult::Failed(err) => {
                        ds.signal_failed(err.to_string());
                    }
                    ExecutionResult::Suspended(_) => {
                        // Don't signal on suspend — task will resume later
                    }
                }
            }
        }
    }

    /// Record a backward jump (loop iteration) for profiling.
    #[cfg(feature = "jit")]
    #[inline]
    pub(in crate::vm::interpreter) fn record_loop_for_profiling(&self) {
        if let Some(ref profile) = self.module_profile {
            let count = profile.record_loop(self.current_func_id_for_profiling);
            self.aot_profile.write().record_loop(
                self.current_module_checksum_for_aot_profile,
                self.current_func_id_for_profiling as u32,
            );
            if let Some(ref telemetry) = self.jit_telemetry {
                telemetry
                    .loop_samples
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if count & crate::vm::defaults::JIT_POLICY_CHECK_MASK == 0 {
                if let (Some(module), Some(module_id)) = (
                    self.current_module_for_profiling.as_ref(),
                    self.current_module_id_for_profiling,
                ) {
                    self.maybe_request_compilation(
                        self.current_func_id_for_profiling,
                        module,
                        module_id,
                    );
                }
            }
        }
    }

    #[inline]
    pub(in crate::vm::interpreter) fn record_aot_shape_site(
        &self,
        kind: crate::aot_profile::AotSiteKind,
        layout_id: crate::vm::object::LayoutId,
    ) {
        self.aot_profile.write().record_layout_site(
            self.current_module_checksum_for_aot_profile,
            self.current_func_id_for_profiling as u32,
            self.current_bytecode_offset_for_aot_profile,
            kind,
            layout_id,
        );
    }
}
