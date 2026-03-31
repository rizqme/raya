#![allow(missing_docs)]
//! Bytecode adapter (Path B)
//!
//! Lifts `.ryb` bytecode modules through the JIT pipeline to produce
//! functions that can be fed into the AOT state machine transform.
//!
//! This reuses the existing JIT lifter (stack→SSA) at build time rather
//! than runtime. The lifted functions implement the same `AotCompilable`
//! trait as Path A (source IR) functions.

use crate::compiler::bytecode::module::Module;
use crate::compiler::bytecode::opcode::Opcode;
use rustc_hash::FxHashMap;

use super::analysis::{SuspensionAnalysis, SuspensionKind, SuspensionPoint};
use super::profile::{AotFunctionProfile, AotSiteKind};
use super::statemachine::{
    HelperCall, SmBlock, SmBlockId, SmBlockKind, SmCmpOp, SmF64BinOp, SmI32BinOp, SmInstr,
    SmTerminator,
};
use super::traits::{AotCompilable, AotProfileVariant, AotVariantGuard, AotVariantKind};

#[cfg(all(feature = "aot", feature = "jit"))]
use crate::jit::analysis::decoder::Operands;
#[cfg(all(feature = "aot", feature = "jit"))]
use crate::jit::ir::instr::{JitFunction, JitInstr, JitTerminator, Reg};
#[cfg(all(feature = "aot", feature = "jit"))]
use crate::jit::pipeline::{lifter, optimize::JitOptimizer};

#[cfg(all(feature = "aot", feature = "jit"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExactLayout {
    Structural(u32),
    Nominal(u32),
}

#[cfg(all(feature = "aot", feature = "jit"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShapeFieldSpecialization {
    ExactField(u16),
}

#[cfg(all(feature = "aot", feature = "jit"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryOrigin {
    Param(u32),
}

#[cfg(all(feature = "aot", feature = "jit"))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LocalLayoutState {
    slots: Vec<Option<ExactLayout>>,
}

#[cfg(all(feature = "aot", feature = "jit"))]
impl LocalLayoutState {
    fn new(slot_count: usize) -> Self {
        Self {
            slots: vec![None; slot_count],
        }
    }

    fn get(&self, index: u16) -> Option<ExactLayout> {
        self.slots.get(index as usize).and_then(|slot| *slot)
    }

    fn set(&mut self, index: u16, layout: Option<ExactLayout>) {
        if let Some(slot) = self.slots.get_mut(index as usize) {
            *slot = layout;
        }
    }

    fn merge_from_predecessors(states: &[&LocalLayoutState]) -> Self {
        let slot_count = states.first().map(|state| state.slots.len()).unwrap_or(0);
        if states.is_empty() {
            return Self::new(slot_count);
        }
        let mut merged = vec![None; slot_count];
        for slot_idx in 0..slot_count {
            let first = states[0].slots[slot_idx];
            if states[1..]
                .iter()
                .all(|state| state.slots[slot_idx] == first)
            {
                merged[slot_idx] = first;
            }
        }
        Self { slots: merged }
    }
}

#[cfg(all(feature = "aot", feature = "jit"))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LocalOriginState {
    slots: Vec<Option<EntryOrigin>>,
}

#[cfg(all(feature = "aot", feature = "jit"))]
impl LocalOriginState {
    fn new(slot_count: usize) -> Self {
        Self {
            slots: vec![None; slot_count],
        }
    }

    fn with_params(slot_count: usize, param_count: u32) -> Self {
        let mut state = Self::new(slot_count);
        for param_index in 0..param_count as usize {
            if let Some(slot) = state.slots.get_mut(param_index) {
                *slot = Some(EntryOrigin::Param(param_index as u32));
            }
        }
        state
    }

    fn get(&self, index: u16) -> Option<EntryOrigin> {
        self.slots.get(index as usize).and_then(|slot| *slot)
    }

    fn set(&mut self, index: u16, origin: Option<EntryOrigin>) {
        if let Some(slot) = self.slots.get_mut(index as usize) {
            *slot = origin;
        }
    }

    fn merge_from_predecessors(states: &[&LocalOriginState]) -> Self {
        let slot_count = states.first().map(|state| state.slots.len()).unwrap_or(0);
        if states.is_empty() {
            return Self::new(slot_count);
        }
        let mut merged = vec![None; slot_count];
        for slot_idx in 0..slot_count {
            let first = states[0].slots[slot_idx];
            if states[1..]
                .iter()
                .all(|state| state.slots[slot_idx] == first)
            {
                merged[slot_idx] = first;
            }
        }
        Self { slots: merged }
    }
}

/// Errors that can occur during bytecode lifting.
#[derive(Debug)]
pub enum BytecodeAdapterError {
    /// Failed to decode a function's bytecode.
    DecodeFailed { func_index: usize, message: String },

    /// Failed to lift bytecode to SSA form.
    LiftFailed { func_index: usize, message: String },
}

impl std::fmt::Display for BytecodeAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytecodeAdapterError::DecodeFailed {
                func_index,
                message,
            } => {
                write!(f, "Failed to decode function {}: {}", func_index, message)
            }
            BytecodeAdapterError::LiftFailed {
                func_index,
                message,
            } => {
                write!(f, "Failed to lift function {}: {}", func_index, message)
            }
        }
    }
}

impl std::error::Error for BytecodeAdapterError {}

#[cfg(all(feature = "aot", feature = "jit"))]
fn bytecode_function_is_sync_safe(
    module: &Module,
    func_id: usize,
    visiting: &mut std::collections::HashSet<usize>,
) -> bool {
    if !visiting.insert(func_id) {
        return true;
    }
    let Some(func) = module.functions.get(func_id) else {
        return false;
    };
    let Ok(instrs) = crate::jit::analysis::decoder::decode_function(&func.code) else {
        return false;
    };
    for instr in instrs {
        match instr.opcode {
            Opcode::Await
            | Opcode::WaitAll
            | Opcode::Sleep
            | Opcode::Yield
            | Opcode::KernelCall
            | Opcode::Spawn
            | Opcode::SpawnClosure
            | Opcode::CallMethodExact
            | Opcode::OptionalCallMethodExact
            | Opcode::CallMethodShape
            | Opcode::OptionalCallMethodShape
            | Opcode::CallConstructor
            | Opcode::ConstructType
            | Opcode::CallSuper => return false,
            Opcode::Call | Opcode::CallStatic => match instr.operands {
                Operands::Call {
                    func_index: 0xFFFF_FFFF,
                    ..
                } => return false,
                Operands::Call { func_index, .. } => {
                    if !bytecode_function_is_sync_safe(module, func_index as usize, visiting) {
                        return false;
                    }
                }
                _ => return false,
            },
            _ => {}
        }
    }
    true
}

/// Lift all functions in a bytecode module through the JIT pipeline.
///
/// For each function in the module:
/// 1. Decode bytecode to instruction stream
/// 2. Build CFG from jump targets
/// 3. RPO traversal + loop header detection
/// 4. Stack simulation → register assignment
/// 5. Phi node insertion at merge points
///
/// Returns the lifted functions ready for the AOT state machine transform.
///
/// This is the exact same lifting pipeline the JIT uses at runtime,
/// but run at build time.
#[cfg(all(feature = "aot", feature = "jit"))]
pub fn lift_bytecode_module(module: &Module) -> Result<Vec<LiftedFunction>, BytecodeAdapterError> {
    // The runtime JIT optimizer is still tuned for hot-path machine-code generation and
    // can over-prune stack/local traffic that the bytecode-lifted AOT adapter still needs
    // for structurally-typed global/local flows. Keep the lifted form unoptimized here and
    // let the AOT specialization passes operate on the safer pre-optimization IR.
    let optimizer = JitOptimizer::empty();
    let mut lifted = Vec::new();
    let structural_shapes = module
        .metadata
        .structural_shapes
        .iter()
        .map(|shape| {
            (
                crate::vm::object::shape_id_from_member_names(&shape.member_names),
                shape.member_names.clone(),
            )
        })
        .collect::<FxHashMap<_, _>>();
    let structural_layouts = module
        .metadata
        .structural_layouts
        .iter()
        .map(|layout| (layout.layout_id, layout.member_names.clone()))
        .collect::<FxHashMap<_, _>>();
    let nominal_layouts = module
        .reflection
        .as_ref()
        .map(|reflection| {
            reflection
                .classes
                .iter()
                .enumerate()
                .map(|(nominal_type_id, class)| {
                    (
                        nominal_type_id as u32,
                        class
                            .fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<FxHashMap<_, _>>()
        })
        .unwrap_or_default();
    let function_local_counts = module
        .functions
        .iter()
        .map(|func| func.local_count as u32)
        .collect::<Vec<_>>();
    let sync_safe_functions = module
        .functions
        .iter()
        .enumerate()
        .map(|(idx, _)| {
            let mut visiting = std::collections::HashSet::new();
            (
                idx as u32,
                bytecode_function_is_sync_safe(module, idx, &mut visiting),
            )
        })
        .collect::<FxHashMap<_, _>>();

    for (idx, func) in module.functions.iter().enumerate() {
        let mut jit_func = lifter::lift_function(func, module, idx as u32).map_err(|e| {
            BytecodeAdapterError::LiftFailed {
                func_index: idx,
                message: e.to_string(),
            }
        })?;

        optimizer.optimize(&mut jit_func);

        let name = Some(func.name.clone());

        lifted.push(LiftedFunction {
            func_index: idx as u32,
            param_count: func.param_count as u32,
            local_count: func.local_count as u32,
            name,
            structural_shapes: structural_shapes.clone(),
            structural_layouts: structural_layouts.clone(),
            nominal_layouts: nominal_layouts.clone(),
            function_local_counts: function_local_counts.clone(),
            sync_safe_functions: sync_safe_functions.clone(),
            constant_strings: module.constants.strings.clone(),
            jit_func,
            profile_assumptions: FxHashMap::default(),
        });
    }

    Ok(lifted)
}

/// Stub version when JIT feature is not enabled.
#[cfg(not(all(feature = "aot", feature = "jit")))]
pub fn lift_bytecode_module(_module: &Module) -> Result<Vec<LiftedFunction>, BytecodeAdapterError> {
    Ok(Vec::new())
}

/// A function lifted from bytecode, ready for AOT compilation.
#[derive(Debug, Clone)]
pub struct LiftedFunction {
    /// Index within the source module.
    pub func_index: u32,

    /// Number of parameters.
    pub param_count: u32,

    /// Number of locals.
    pub local_count: u32,

    /// Function name (from module metadata, if available).
    pub name: Option<String>,

    /// Canonical structural shape metadata for this module.
    pub structural_shapes: FxHashMap<u64, Vec<String>>,

    /// Physical structural layout metadata for this module.
    pub structural_layouts: FxHashMap<u32, Vec<String>>,

    /// Nominal type field layout metadata for this module.
    pub nominal_layouts: FxHashMap<u32, Vec<String>>,

    /// Local counts for same-module direct call lowering.
    pub function_local_counts: Vec<u32>,

    /// Functions proven sync-safe for helper-backed direct AOT calls.
    pub sync_safe_functions: FxHashMap<u32, bool>,

    /// Bytecode string constants used by lifted constant-string instructions.
    pub constant_strings: Vec<String>,

    /// The lifted JIT IR (only available when both aot and jit features are enabled).
    #[cfg(all(feature = "aot", feature = "jit"))]
    pub jit_func: JitFunction,

    /// Clone-specific shape specialization assumptions keyed by profiled site.
    pub profile_assumptions: FxHashMap<(u32, AotSiteKind), u32>,
}

/// Helper function to map JitInstr to SuspensionKind (when JIT feature is enabled)
#[cfg(all(feature = "aot", feature = "jit"))]
fn classify_suspension(instr: &JitInstr) -> Option<SuspensionKind> {
    match instr {
        // Always suspends
        JitInstr::Await { .. } => Some(SuspensionKind::Await),
        JitInstr::Yield => Some(SuspensionKind::Yield),
        JitInstr::Sleep { .. } => Some(SuspensionKind::Sleep),

        // May suspend - native call
        JitInstr::CallKernel { .. } => Some(SuspensionKind::NativeCall),

        // May suspend - AOT function call
        JitInstr::Call { .. } => Some(SuspensionKind::AotCall),

        // Preemption check
        JitInstr::CheckPreemption { .. } => Some(SuspensionKind::PreemptionCheck),

        // Channel operations (if implemented)
        // JitInstr::ChannelRecv { .. } => Some(SuspensionKind::ChannelRecv),
        // JitInstr::ChannelSend { .. } => Some(SuspensionKind::ChannelSend),
        _ => None,
    }
}

#[cfg(all(feature = "aot", feature = "jit"))]
impl LiftedFunction {
    fn local_slot_count(&self) -> usize {
        self.local_count as usize
    }

    fn structural_shape_names(&self, shape_id: u64) -> Option<&[String]> {
        self.structural_shapes.get(&shape_id).map(Vec::as_slice)
    }

    fn structural_layout_names(&self, layout_id: u32) -> Option<&[String]> {
        self.structural_layouts.get(&layout_id).map(Vec::as_slice)
    }

    fn nominal_layout_names(&self, nominal_type_id: u32) -> Option<&[String]> {
        self.nominal_layouts
            .get(&nominal_type_id)
            .map(Vec::as_slice)
    }

    fn exact_layout_field_names(&self, exact: ExactLayout) -> Option<Vec<String>> {
        match exact {
            ExactLayout::Structural(layout_id) => self
                .structural_layout_names(layout_id)
                .map(|names| names.to_vec()),
            ExactLayout::Nominal(nominal_type_id) => self
                .nominal_layout_names(nominal_type_id)
                .map(|names| names.to_vec()),
        }
    }

    fn profiled_exact_layout(
        &self,
        bytecode_offset: u32,
        kind: AotSiteKind,
    ) -> Option<ExactLayout> {
        let layout_id = *self.profile_assumptions.get(&(bytecode_offset, kind))?;
        if self.structural_layouts.contains_key(&layout_id) {
            return Some(ExactLayout::Structural(layout_id));
        }
        None
    }

    fn clone_for_profile_site(
        &self,
        bytecode_offset: u32,
        kind: AotSiteKind,
        layout_id: u32,
    ) -> Option<Self> {
        self.profiled_exact_layout(bytecode_offset, kind)
            .or_else(|| {
                if self.structural_layouts.contains_key(&layout_id) {
                    Some(ExactLayout::Structural(layout_id))
                } else {
                    None
                }
            })?;
        let mut clone = self.clone();
        clone
            .profile_assumptions
            .insert((bytecode_offset, kind), layout_id);
        if let Some(name) = clone.name.as_mut() {
            *name = format!("{}$pgo_{:?}_{}_{}", name, kind, bytecode_offset, layout_id);
        }
        Some(clone)
    }

    fn profiled_variants(&self, profile: &AotFunctionProfile) -> Vec<AotProfileVariant> {
        let mut variants = Vec::new();
        let mut emitted = 0usize;
        let guard_sources = self.analyze_site_guard_args();
        for site in &profile.sites {
            if !matches!(
                site.kind,
                AotSiteKind::LoadFieldShape
                    | AotSiteKind::StoreFieldShape
                    | AotSiteKind::CastShape
                    | AotSiteKind::ImplementsShape
            ) {
                continue;
            }
            let Some(hot_layout) = site.layouts.first() else {
                continue;
            };
            if hot_layout.hits < 4 {
                continue;
            }
            let Some(clone) =
                self.clone_for_profile_site(site.bytecode_offset, site.kind, hot_layout.layout_id)
            else {
                continue;
            };
            let Some(guard_arg_index) = guard_sources
                .get(&(site.bytecode_offset, site.kind))
                .copied()
                .flatten()
            else {
                continue;
            };
            variants.push(AotProfileVariant {
                func: Box::new(clone),
                name_suffix: format!(
                    "$pgo_{:?}_{}_{}",
                    site.kind, site.bytecode_offset, hot_layout.layout_id
                ),
                kind: AotVariantKind::ProfileClone,
                guard: Some(AotVariantGuard {
                    bytecode_offset: site.bytecode_offset,
                    layout_id: hot_layout.layout_id,
                    guard_arg_index: Some(guard_arg_index),
                }),
            });
            emitted += 1;
            if emitted >= 4 {
                break;
            }
        }
        variants
    }

    fn specialize_shape_field_access(
        &self,
        exact: ExactLayout,
        shape_id: u64,
        required_slot: u16,
    ) -> Option<ShapeFieldSpecialization> {
        let required_names = self.structural_shape_names(shape_id)?;
        let required_name = required_names.get(required_slot as usize)?;
        let provider_names = self.exact_layout_field_names(exact)?;
        let provider_slot = provider_names
            .iter()
            .position(|name| name == required_name)?;
        let provider_slot = u16::try_from(provider_slot).ok()?;
        Some(ShapeFieldSpecialization::ExactField(provider_slot))
    }

    fn exact_layout_satisfies_shape_by_fields(&self, exact: ExactLayout, shape_id: u64) -> bool {
        let Some(required_names) = self.structural_shape_names(shape_id) else {
            return false;
        };
        let Some(provider_names) = self.exact_layout_field_names(exact) else {
            return false;
        };
        required_names
            .iter()
            .all(|required| provider_names.iter().any(|provider| provider == required))
    }

    fn update_layout_tracking(
        &self,
        reg_state: &mut FxHashMap<Reg, ExactLayout>,
        local_state: &mut LocalLayoutState,
        global_state: &mut FxHashMap<u32, ExactLayout>,
        instr: &JitInstr,
    ) {
        let reg_layout =
            |reg: Reg, reg_state: &FxHashMap<Reg, ExactLayout>| reg_state.get(&reg).copied();
        match instr {
            JitInstr::NewObject {
                dest,
                nominal_type_id,
                ..
            }
            | JitInstr::ConstructType {
                dest,
                nominal_type_id,
                ..
            } => {
                reg_state.insert(*dest, ExactLayout::Nominal(*nominal_type_id));
            }
            JitInstr::ObjectLiteral {
                dest, type_index, ..
            } => {
                reg_state.insert(*dest, ExactLayout::Structural(*type_index));
            }
            JitInstr::DynNewObject { dest } => {
                reg_state.insert(
                    *dest,
                    ExactLayout::Structural(crate::vm::object::layout_id_from_ordered_names(&[])),
                );
            }
            JitInstr::CastShape { dest, object, .. }
            | JitInstr::Move { dest, src: object }
            | JitInstr::Cast { dest, object, .. } => {
                if let Some(layout) = reg_layout(*object, reg_state) {
                    reg_state.insert(*dest, layout);
                }
            }
            JitInstr::LoadLocal { dest, index } => {
                if let Some(layout) = local_state.get(*index) {
                    reg_state.insert(*dest, layout);
                }
            }
            JitInstr::LoadGlobal { dest, index } => {
                if let Some(layout) = global_state.get(index).copied() {
                    reg_state.insert(*dest, layout);
                }
            }
            JitInstr::StoreLocal { index, value } => {
                local_state.set(*index, reg_layout(*value, reg_state));
            }
            JitInstr::StoreGlobal { index, value } => {
                if let Some(layout) = reg_layout(*value, reg_state) {
                    global_state.insert(*index, layout);
                }
            }
            JitInstr::Phi { dest, sources } => {
                let first = sources
                    .first()
                    .and_then(|(_, reg)| reg_layout(*reg, reg_state));
                if let Some(layout) = first.filter(|layout| {
                    sources
                        .iter()
                        .all(|(_, reg)| reg_layout(*reg, reg_state) == Some(*layout))
                }) {
                    reg_state.insert(*dest, layout);
                }
            }
            _ => {}
        }
    }

    fn analyze_block_local_layouts(&self) -> Vec<LocalLayoutState> {
        let block_count = self.jit_func.blocks.len();
        let slot_count = self.local_slot_count();
        let mut entries = vec![LocalLayoutState::new(slot_count); block_count];
        let mut exits = vec![LocalLayoutState::new(slot_count); block_count];
        let mut changed = true;
        while changed {
            changed = false;
            for (block_idx, block) in self.jit_func.blocks.iter().enumerate() {
                let merged = if block_idx == self.jit_func.entry.0 as usize {
                    entries[block_idx].clone()
                } else if block.predecessors.is_empty() {
                    LocalLayoutState::new(slot_count)
                } else {
                    let pred_states = block
                        .predecessors
                        .iter()
                        .map(|pred| &exits[pred.0 as usize])
                        .collect::<Vec<_>>();
                    LocalLayoutState::merge_from_predecessors(&pred_states)
                };
                if merged != entries[block_idx] {
                    entries[block_idx] = merged.clone();
                    changed = true;
                }
                let mut reg_state = FxHashMap::default();
                let mut local_state = merged;
                let mut global_state = FxHashMap::default();
                for instr in &block.instrs {
                    self.update_layout_tracking(
                        &mut reg_state,
                        &mut local_state,
                        &mut global_state,
                        instr,
                    );
                }
                if exits[block_idx] != local_state {
                    exits[block_idx] = local_state;
                    changed = true;
                }
            }
        }
        entries
    }

    fn update_origin_tracking(
        &self,
        reg_state: &mut FxHashMap<Reg, EntryOrigin>,
        local_state: &mut LocalOriginState,
        global_state: &mut FxHashMap<u32, EntryOrigin>,
        instr: &JitInstr,
    ) {
        let reg_origin =
            |reg: Reg, reg_state: &FxHashMap<Reg, EntryOrigin>| reg_state.get(&reg).copied();
        match instr {
            JitInstr::CastShape { dest, object, .. }
            | JitInstr::Move { dest, src: object }
            | JitInstr::Cast { dest, object, .. } => {
                if let Some(origin) = reg_origin(*object, reg_state) {
                    reg_state.insert(*dest, origin);
                } else {
                    reg_state.remove(dest);
                }
            }
            JitInstr::LoadLocal { dest, index } => {
                if let Some(origin) = local_state.get(*index) {
                    reg_state.insert(*dest, origin);
                } else {
                    reg_state.remove(dest);
                }
            }
            JitInstr::LoadGlobal { dest, index } => {
                if let Some(origin) = global_state.get(index).copied() {
                    reg_state.insert(*dest, origin);
                } else {
                    reg_state.remove(dest);
                }
            }
            JitInstr::StoreLocal { index, value } => {
                local_state.set(*index, reg_origin(*value, reg_state));
            }
            JitInstr::StoreGlobal { index, value } => {
                if let Some(origin) = reg_origin(*value, reg_state) {
                    global_state.insert(*index, origin);
                } else {
                    global_state.remove(index);
                }
            }
            JitInstr::Phi { dest, sources } => {
                let first = sources
                    .first()
                    .and_then(|(_, reg)| reg_origin(*reg, reg_state));
                if let Some(origin) = first.filter(|origin| {
                    sources
                        .iter()
                        .all(|(_, reg)| reg_origin(*reg, reg_state) == Some(*origin))
                }) {
                    reg_state.insert(*dest, origin);
                } else {
                    reg_state.remove(dest);
                }
            }
            JitInstr::NewObject { dest, .. }
            | JitInstr::ConstructType { dest, .. }
            | JitInstr::ObjectLiteral { dest, .. }
            | JitInstr::DynNewObject { dest } => {
                reg_state.remove(dest);
            }
            _ => {}
        }
    }

    fn analyze_block_local_origins(&self) -> Vec<LocalOriginState> {
        let block_count = self.jit_func.blocks.len();
        let slot_count = self.local_slot_count();
        let mut entries = vec![LocalOriginState::new(slot_count); block_count];
        let mut exits = vec![LocalOriginState::new(slot_count); block_count];
        let mut changed = true;
        while changed {
            changed = false;
            for (block_idx, block) in self.jit_func.blocks.iter().enumerate() {
                let merged = if block_idx == self.jit_func.entry.0 as usize {
                    LocalOriginState::with_params(slot_count, self.param_count)
                } else if block.predecessors.is_empty() {
                    LocalOriginState::new(slot_count)
                } else {
                    let pred_states = block
                        .predecessors
                        .iter()
                        .map(|pred| &exits[pred.0 as usize])
                        .collect::<Vec<_>>();
                    LocalOriginState::merge_from_predecessors(&pred_states)
                };
                if merged != entries[block_idx] {
                    entries[block_idx] = merged.clone();
                    changed = true;
                }
                let mut reg_state = FxHashMap::default();
                let mut local_state = merged;
                let mut global_state = FxHashMap::default();
                for instr in &block.instrs {
                    self.update_origin_tracking(
                        &mut reg_state,
                        &mut local_state,
                        &mut global_state,
                        instr,
                    );
                }
                if exits[block_idx] != local_state {
                    exits[block_idx] = local_state;
                    changed = true;
                }
            }
        }
        entries
    }

    fn analyze_site_guard_args(&self) -> FxHashMap<(u32, AotSiteKind), Option<u32>> {
        let block_entry_states = self.analyze_block_local_origins();
        let mut guard_sources: FxHashMap<(u32, AotSiteKind), Option<u32>> = FxHashMap::default();
        for (idx, jit_block) in self.jit_func.blocks.iter().enumerate() {
            let mut reg_state = FxHashMap::default();
            let mut local_state = block_entry_states
                .get(idx)
                .cloned()
                .unwrap_or_else(|| LocalOriginState::new(self.local_slot_count()));
            let mut global_state = FxHashMap::default();
            for instr in &jit_block.instrs {
                let site_guard = match instr {
                    JitInstr::LoadFieldShape {
                        object,
                        bytecode_offset,
                        ..
                    } => Some((
                        *bytecode_offset,
                        AotSiteKind::LoadFieldShape,
                        reg_state.get(object).copied(),
                    )),
                    JitInstr::StoreFieldShape {
                        object,
                        bytecode_offset,
                        ..
                    } => Some((
                        *bytecode_offset,
                        AotSiteKind::StoreFieldShape,
                        reg_state.get(object).copied(),
                    )),
                    JitInstr::ImplementsShape {
                        object,
                        bytecode_offset,
                        ..
                    } => Some((
                        *bytecode_offset,
                        AotSiteKind::ImplementsShape,
                        reg_state.get(object).copied(),
                    )),
                    JitInstr::CastShape {
                        object,
                        bytecode_offset,
                        ..
                    } => Some((
                        *bytecode_offset,
                        AotSiteKind::CastShape,
                        reg_state.get(object).copied(),
                    )),
                    _ => None,
                };
                if let Some((bytecode_offset, kind, origin)) = site_guard {
                    let next = origin.map(|origin| match origin {
                        EntryOrigin::Param(index) => index,
                    });
                    match guard_sources.get_mut(&(bytecode_offset, kind)) {
                        Some(existing) if *existing != next => *existing = None,
                        Some(_) => {}
                        None => {
                            guard_sources.insert((bytecode_offset, kind), next);
                        }
                    }
                }
                self.update_origin_tracking(
                    &mut reg_state,
                    &mut local_state,
                    &mut global_state,
                    instr,
                );
            }
        }
        guard_sources
    }

    fn emit_instrs_for_block(
        &self,
        out: &mut Vec<SmInstr>,
        instr: &JitInstr,
        next_temp: &mut u32,
        reg_state: &mut FxHashMap<Reg, ExactLayout>,
        local_state: &mut LocalLayoutState,
        global_state: &mut FxHashMap<u32, ExactLayout>,
    ) {
        let reg_layout =
            |reg: Reg, reg_state: &FxHashMap<Reg, ExactLayout>| reg_state.get(&reg).copied();
        match instr {
            JitInstr::NewObject {
                dest,
                nominal_type_id,
                ..
            } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::AllocObject,
                args: vec![*nominal_type_id],
            }),
            JitInstr::LoadGlobal { dest, index } => {
                out.push(SmInstr::LoadGlobal {
                    dest: dest.0,
                    index: *index,
                });
            }
            JitInstr::StoreGlobal { index, value } => {
                out.push(SmInstr::StoreGlobal {
                    index: *index,
                    src: value.0,
                });
            }
            JitInstr::LoadFieldExact {
                dest,
                object,
                offset,
            } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::ObjectGetField,
                args: vec![object.0, *offset as u32],
            }),
            JitInstr::LoadFieldShape {
                dest,
                object,
                shape_id,
                offset,
                optional: _,
                bytecode_offset,
                ..
            } => {
                if let Some(ShapeFieldSpecialization::ExactField(field)) =
                    reg_layout(*object, reg_state)
                        .or_else(|| {
                            self.profiled_exact_layout(
                                *bytecode_offset,
                                AotSiteKind::LoadFieldShape,
                            )
                        })
                        .and_then(|layout| {
                            self.specialize_shape_field_access(layout, *shape_id, *offset)
                        })
                {
                    out.push(SmInstr::CallHelper {
                        dest: Some(dest.0),
                        helper: HelperCall::ObjectGetField,
                        args: vec![object.0, field as u32],
                    });
                } else {
                    out.push(SmInstr::CallHelper {
                        dest: Some(dest.0),
                        helper: HelperCall::LoadFieldShape,
                        args: vec![
                            object.0,
                            (*shape_id & 0xFFFF_FFFF) as u32,
                            (*shape_id >> 32) as u32,
                            *offset as u32,
                        ],
                    });
                }
            }
            JitInstr::ImplementsShape {
                dest,
                object,
                shape_id,
                bytecode_offset,
            } => {
                if let Some(layout) = reg_layout(*object, reg_state).or_else(|| {
                    self.profiled_exact_layout(*bytecode_offset, AotSiteKind::ImplementsShape)
                }) {
                    out.push(SmInstr::ConstBool {
                        dest: dest.0,
                        value: self.exact_layout_satisfies_shape_by_fields(layout, *shape_id),
                    });
                } else {
                    out.push(SmInstr::CallHelper {
                        dest: Some(dest.0),
                        helper: HelperCall::ImplementsShape,
                        args: vec![
                            object.0,
                            (*shape_id & 0xFFFF_FFFF) as u32,
                            (*shape_id >> 32) as u32,
                        ],
                    });
                }
            }
            JitInstr::CastShape {
                dest,
                object,
                shape_id,
                bytecode_offset,
            } => {
                if let Some(layout) = reg_layout(*object, reg_state)
                    .or_else(|| {
                        self.profiled_exact_layout(*bytecode_offset, AotSiteKind::CastShape)
                    })
                    .filter(|layout| {
                        self.exact_layout_satisfies_shape_by_fields(*layout, *shape_id)
                    })
                {
                    let _ = layout;
                    out.push(SmInstr::Move {
                        dest: dest.0,
                        src: object.0,
                    });
                } else {
                    out.push(SmInstr::CallHelper {
                        dest: Some(dest.0),
                        helper: HelperCall::CastShape,
                        args: vec![
                            object.0,
                            (*shape_id & 0xFFFF_FFFF) as u32,
                            (*shape_id >> 32) as u32,
                        ],
                    });
                }
            }
            JitInstr::StoreFieldExact {
                object,
                offset,
                value,
            } => out.push(SmInstr::CallHelper {
                dest: None,
                helper: HelperCall::ObjectSetField,
                args: vec![object.0, *offset as u32, value.0],
            }),
            JitInstr::StoreFieldShape {
                object,
                shape_id,
                offset,
                value,
                bytecode_offset,
                ..
            } => {
                if let Some(ShapeFieldSpecialization::ExactField(field)) =
                    reg_layout(*object, reg_state)
                        .or_else(|| {
                            self.profiled_exact_layout(
                                *bytecode_offset,
                                AotSiteKind::StoreFieldShape,
                            )
                        })
                        .and_then(|layout| {
                            self.specialize_shape_field_access(layout, *shape_id, *offset)
                        })
                {
                    out.push(SmInstr::CallHelper {
                        dest: None,
                        helper: HelperCall::ObjectSetField,
                        args: vec![object.0, field as u32, value.0],
                    });
                } else {
                    out.push(SmInstr::CallHelper {
                        dest: None,
                        helper: HelperCall::StoreFieldShape,
                        args: vec![
                            object.0,
                            (*shape_id & 0xFFFF_FFFF) as u32,
                            (*shape_id >> 32) as u32,
                            *offset as u32,
                            value.0,
                        ],
                    });
                }
            }
            JitInstr::InstanceOf {
                dest,
                object,
                nominal_type_id,
            } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::InstanceOf,
                args: vec![object.0, *nominal_type_id],
            }),
            JitInstr::Cast {
                dest,
                object,
                nominal_type_id,
                ..
            } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::Cast,
                args: vec![object.0, *nominal_type_id],
            }),
            JitInstr::Typeof { dest, operand } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::Typeof,
                args: vec![operand.0],
            }),
            JitInstr::ObjectLiteral {
                dest,
                type_index,
                fields,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(dest.0),
                    helper: HelperCall::AllocStructuralObject,
                    args: vec![*type_index, fields.len() as u32],
                });
                for (field_index, value) in fields.iter().enumerate() {
                    out.push(SmInstr::CallHelper {
                        dest: None,
                        helper: HelperCall::ObjectSetField,
                        args: vec![dest.0, field_index as u32, value.0],
                    });
                }
            }
            JitInstr::DynGetKeyed {
                dest,
                object,
                index,
            } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::DynGetProp,
                args: vec![object.0, index.0],
            }),
            JitInstr::DynSetKeyed {
                object,
                index,
                value,
            } => out.push(SmInstr::CallHelper {
                dest: None,
                helper: HelperCall::DynSetProp,
                args: vec![object.0, index.0, value.0],
            }),
            JitInstr::CallKernel {
                dest,
                kernel_op_id,
                args,
                ..
            } => {
                let mut call_args = vec![*kernel_op_id as u32];
                call_args.extend(args.iter().map(|reg| reg.0));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(|reg| reg.0),
                    helper: HelperCall::KernelCall,
                    args: call_args,
                });
            }
            JitInstr::Call {
                dest,
                func_index,
                closure: None,
                args,
                ..
            } if self
                .sync_safe_functions
                .get(func_index)
                .copied()
                .unwrap_or(false) =>
            {
                let mut call_args = vec![
                    *func_index,
                    self.function_local_counts
                        .get(*func_index as usize)
                        .copied()
                        .unwrap_or(args.len() as u32),
                ];
                call_args.extend(args.iter().map(|reg| reg.0));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(|reg| reg.0),
                    helper: HelperCall::RunSyncAotCall,
                    args: call_args,
                });
            }
            JitInstr::Call {
                dest,
                func_index,
                closure: None,
                args,
                ..
            } => {
                let frame_reg = {
                    let reg = *next_temp;
                    *next_temp += 1;
                    reg
                };
                let result_reg = dest.map(|reg| reg.0).unwrap_or_else(|| {
                    let reg = *next_temp;
                    *next_temp += 1;
                    reg
                });
                let mut call_args = vec![
                    *func_index,
                    self.function_local_counts
                        .get(*func_index as usize)
                        .copied()
                        .unwrap_or(args.len() as u32),
                ];
                call_args.extend(args.iter().map(|reg| reg.0));
                out.push(SmInstr::CallHelper {
                    dest: Some(frame_reg),
                    helper: HelperCall::PrepareAotCallFrame,
                    args: call_args,
                });
                out.push(SmInstr::StoreChildFrame { src: frame_reg });
                out.push(SmInstr::CallAot {
                    dest: result_reg,
                    func_id: *func_index,
                    callee_frame: frame_reg,
                });
            }
            JitInstr::CallStatic {
                dest,
                func_index,
                args,
                ..
            } if self
                .sync_safe_functions
                .get(func_index)
                .copied()
                .unwrap_or(false) =>
            {
                let mut call_args = vec![
                    *func_index,
                    self.function_local_counts
                        .get(*func_index as usize)
                        .copied()
                        .unwrap_or(args.len() as u32),
                ];
                call_args.extend(args.iter().map(|reg| reg.0));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(|reg| reg.0),
                    helper: HelperCall::RunSyncAotCall,
                    args: call_args,
                });
            }
            JitInstr::CallStatic {
                dest,
                func_index,
                args,
                ..
            } => {
                let frame_reg = {
                    let reg = *next_temp;
                    *next_temp += 1;
                    reg
                };
                let result_reg = dest.map(|reg| reg.0).unwrap_or_else(|| {
                    let reg = *next_temp;
                    *next_temp += 1;
                    reg
                });
                let mut call_args = vec![
                    *func_index,
                    self.function_local_counts
                        .get(*func_index as usize)
                        .copied()
                        .unwrap_or(args.len() as u32),
                ];
                call_args.extend(args.iter().map(|reg| reg.0));
                out.push(SmInstr::CallHelper {
                    dest: Some(frame_reg),
                    helper: HelperCall::PrepareAotCallFrame,
                    args: call_args,
                });
                out.push(SmInstr::StoreChildFrame { src: frame_reg });
                out.push(SmInstr::CallAot {
                    dest: result_reg,
                    func_id: *func_index,
                    callee_frame: frame_reg,
                });
            }
            JitInstr::DynNewObject { dest } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::AllocStructuralObject,
                args: vec![crate::vm::object::layout_id_from_ordered_names(&[]), 0],
            }),
            JitInstr::ConstStr { dest, str_index } => out.push(SmInstr::ConstString {
                dest: dest.0,
                value: self
                    .constant_strings
                    .get(*str_index as usize)
                    .cloned()
                    .unwrap_or_default(),
            }),
            JitInstr::ConstString { dest, pool_index } => out.push(SmInstr::ConstString {
                dest: dest.0,
                value: self
                    .constant_strings
                    .get(*pool_index as usize)
                    .cloned()
                    .unwrap_or_default(),
            }),
            _ => {
                if let Some(sm_instr) = map_jit_instr_to_sm(instr) {
                    out.push(sm_instr);
                }
            }
        }
        self.update_layout_tracking(reg_state, local_state, global_state, instr);
    }
}

impl AotCompilable for LiftedFunction {
    #[cfg(all(feature = "aot", feature = "jit"))]
    fn analyze(&self) -> SuspensionAnalysis {
        let sm_blocks = self.emit_blocks();
        let mut points = Vec::new();
        let mut index = 0u32;
        let mut loop_headers = std::collections::HashSet::new();

        for block in &sm_blocks {
            for succ in sm_block_successors(block) {
                if succ.0 <= block.id.0 {
                    loop_headers.insert(succ.0);
                }
            }
        }

        for block in &sm_blocks {
            for (instr_idx, instr) in block.instructions.iter().enumerate() {
                if let Some(kind) = classify_sm_suspension(instr) {
                    points.push(SuspensionPoint {
                        index,
                        block_id: block.id.0,
                        instr_index: instr_idx as u32,
                        kind,
                        live_locals: std::collections::HashSet::new(), // TODO: liveness analysis
                    });
                    index += 1;
                }
            }
        }

        let has_suspensions = !points.is_empty();

        SuspensionAnalysis {
            points,
            has_suspensions,
            loop_headers,
        }
    }

    #[cfg(not(all(feature = "aot", feature = "jit")))]
    fn analyze(&self) -> SuspensionAnalysis {
        SuspensionAnalysis::none()
    }

    #[cfg(all(feature = "aot", feature = "jit"))]
    fn emit_blocks(&self) -> Vec<SmBlock> {
        let debug = std::env::var_os("RAYA_DEBUG_AOT_DUMP").is_some();
        if debug {
            eprintln!(
                "\n=== AOT LIFTED JIT fn={} name={:?} entry={} blocks={} ===\n{:#?}",
                self.func_index,
                self.name,
                self.jit_func.entry.0,
                self.jit_func.blocks.len(),
                self.jit_func
            );
        }
        let block_entry_states = self.analyze_block_local_layouts();
        if debug {
            eprintln!(
                "\n=== AOT BLOCK ENTRY STATES fn={} name={:?} ===\n{:#?}",
                self.func_index, self.name, block_entry_states
            );
        }
        self.jit_func
            .blocks
            .iter()
            .enumerate()
            .map(|(idx, jit_block)| {
                let mut instructions = Vec::new();
                let mut next_temp = self.jit_func.next_reg + 100 + (idx as u32 * 32);
                let mut reg_state = FxHashMap::default();
                let mut local_state = block_entry_states
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| LocalLayoutState::new(self.local_slot_count()));
                let mut global_state = FxHashMap::default();

                for instr in &jit_block.instrs {
                    self.emit_instrs_for_block(
                        &mut instructions,
                        instr,
                        &mut next_temp,
                        &mut reg_state,
                        &mut local_state,
                        &mut global_state,
                    );
                }

                let terminator = map_jit_terminator(&jit_block.terminator);

                SmBlock {
                    id: SmBlockId(idx as u32),
                    kind: SmBlockKind::Body,
                    instructions,
                    terminator,
                }
            })
            .collect()
    }

    #[cfg(not(all(feature = "aot", feature = "jit")))]
    fn emit_blocks(&self) -> Vec<SmBlock> {
        vec![SmBlock {
            id: SmBlockId(0),
            kind: SmBlockKind::Body,
            instructions: vec![SmInstr::ConstNull { dest: 0 }],
            terminator: SmTerminator::Return { value: 0 },
        }]
    }

    fn param_count(&self) -> u32 {
        self.param_count
    }

    fn local_count(&self) -> u32 {
        self.local_count
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn profile_variants(&self, profile: Option<&AotFunctionProfile>) -> Vec<AotProfileVariant> {
        profile
            .map(|profile| self.profiled_variants(profile))
            .unwrap_or_default()
    }
}

#[cfg(all(feature = "aot", feature = "jit"))]
fn classify_sm_suspension(instr: &SmInstr) -> Option<SuspensionKind> {
    match instr {
        SmInstr::CallAot { .. } => Some(SuspensionKind::AotCall),
        SmInstr::CallHelper { helper, .. } => match helper {
            HelperCall::KernelCall => Some(SuspensionKind::NativeCall),
            HelperCall::AwaitTask | HelperCall::AwaitAll => Some(SuspensionKind::Await),
            HelperCall::YieldTask => Some(SuspensionKind::Yield),
            HelperCall::SleepTask => Some(SuspensionKind::Sleep),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(all(feature = "aot", feature = "jit"))]
fn sm_block_successors(block: &SmBlock) -> Vec<SmBlockId> {
    match &block.terminator {
        SmTerminator::Jump(target) => vec![*target],
        SmTerminator::Branch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        SmTerminator::BranchNull {
            null_block,
            not_null_block,
            ..
        } => vec![*null_block, *not_null_block],
        SmTerminator::BrTable {
            default, targets, ..
        } => {
            let mut out = Vec::with_capacity(targets.len() + 1);
            out.push(*default);
            out.extend(targets.iter().copied());
            out
        }
        SmTerminator::Return { .. } | SmTerminator::Unreachable => Vec::new(),
    }
}

/// Map a JitInstr to SmInstr (when JIT feature is enabled)
#[cfg(all(feature = "aot", feature = "jit"))]
fn map_jit_instr_to_sm(instr: &JitInstr) -> Option<SmInstr> {
    Some(match instr {
        // Constants
        JitInstr::ConstI32 { dest, value } => SmInstr::ConstI32 {
            dest: dest.0,
            value: *value,
        },
        JitInstr::ConstF64 { dest, value } => SmInstr::ConstF64 {
            dest: dest.0,
            bits: value.to_bits(),
        },
        JitInstr::ConstBool { dest, value } => SmInstr::ConstBool {
            dest: dest.0,
            value: *value,
        },
        JitInstr::ConstNull { dest } => SmInstr::ConstNull { dest: dest.0 },

        // Integer arithmetic
        JitInstr::IAdd { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Add,
            left: left.0,
            right: right.0,
        },
        JitInstr::ISub { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Sub,
            left: left.0,
            right: right.0,
        },
        JitInstr::IMul { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Mul,
            left: left.0,
            right: right.0,
        },
        JitInstr::IDiv { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Div,
            left: left.0,
            right: right.0,
        },
        JitInstr::IMod { dest, left, right } => SmInstr::I32BinOp {
            dest: dest.0,
            op: SmI32BinOp::Mod,
            left: left.0,
            right: right.0,
        },

        // Float arithmetic
        JitInstr::FAdd { dest, left, right } => SmInstr::F64BinOp {
            dest: dest.0,
            op: SmF64BinOp::Add,
            left: left.0,
            right: right.0,
        },
        JitInstr::FSub { dest, left, right } => SmInstr::F64BinOp {
            dest: dest.0,
            op: SmF64BinOp::Sub,
            left: left.0,
            right: right.0,
        },
        JitInstr::FMul { dest, left, right } => SmInstr::F64BinOp {
            dest: dest.0,
            op: SmF64BinOp::Mul,
            left: left.0,
            right: right.0,
        },
        JitInstr::FDiv { dest, left, right } => SmInstr::F64BinOp {
            dest: dest.0,
            op: SmF64BinOp::Div,
            left: left.0,
            right: right.0,
        },

        // Comparisons
        JitInstr::ICmpEq { dest, left, right } => SmInstr::I32Cmp {
            dest: dest.0,
            op: SmCmpOp::Eq,
            left: left.0,
            right: right.0,
        },
        JitInstr::ICmpLt { dest, left, right } => SmInstr::I32Cmp {
            dest: dest.0,
            op: SmCmpOp::Lt,
            left: left.0,
            right: right.0,
        },
        JitInstr::FCmpEq { dest, left, right } => SmInstr::F64Cmp {
            dest: dest.0,
            op: SmCmpOp::Eq,
            left: left.0,
            right: right.0,
        },
        JitInstr::FCmpLt { dest, left, right } => SmInstr::F64Cmp {
            dest: dest.0,
            op: SmCmpOp::Lt,
            left: left.0,
            right: right.0,
        },

        // NaN-boxing
        JitInstr::BoxI32 { dest, src } => SmInstr::BoxI32 {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::UnboxI32 { dest, src } => SmInstr::UnboxI32 {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::BoxF64 { dest, src } => SmInstr::BoxF64 {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::UnboxF64 { dest, src } => SmInstr::UnboxF64 {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::BoxBool { dest, src } => SmInstr::BoxBool {
            dest: dest.0,
            src: src.0,
        },
        JitInstr::UnboxBool { dest, src } => SmInstr::UnboxBool {
            dest: dest.0,
            src: src.0,
        },

        // Local variables
        JitInstr::LoadLocal { dest, index } => SmInstr::LoadLocal {
            dest: dest.0,
            index: *index as u32,
        },
        JitInstr::StoreLocal { index, value } => SmInstr::StoreLocal {
            index: *index as u32,
            src: value.0,
        },

        // Phi and Move
        JitInstr::Phi { dest, .. } => SmInstr::Phi {
            dest: dest.0,
            sources: Vec::new(), // TODO: map sources
        },
        JitInstr::Move { dest, src } => SmInstr::Move {
            dest: dest.0,
            src: src.0,
        },

        // For other instructions, use helper calls or stub with Unreachable
        _ => return None, // Skip unsupported instructions for now
    })
}

/// Map a JitTerminator to SmTerminator (when JIT feature is enabled)
#[cfg(all(feature = "aot", feature = "jit"))]
fn map_jit_terminator(terminator: &JitTerminator) -> SmTerminator {
    match terminator {
        JitTerminator::Jump(target) => SmTerminator::Jump(SmBlockId(target.0)),
        JitTerminator::Branch {
            cond,
            then_block,
            else_block,
        } => SmTerminator::Branch {
            cond: cond.0,
            then_block: SmBlockId(then_block.0),
            else_block: SmBlockId(else_block.0),
        },
        JitTerminator::Return(Some(value)) => SmTerminator::Return { value: value.0 },
        JitTerminator::Return(None) => SmTerminator::Return { value: 0 }, // Return null
        JitTerminator::Unreachable => SmTerminator::Return { value: 0 },  // Fallback
        _ => SmTerminator::Return { value: 0 }, // Fallback for other terminators
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aot::profile::{AotFunctionProfile, AotHotLayout, AotSiteKind, AotSiteProfile};

    #[test]
    fn test_lifted_function_compilable() {
        // Note: When JIT feature is not enabled, we can't create a full LiftedFunction
        // This test just verifies the AotCompilable trait is implemented
        #[cfg(all(feature = "aot", feature = "jit"))]
        {
            use crate::jit::ir::instr::JitFunction;
            use crate::jit::ir::types::JitType;

            let jit_func = JitFunction::new(0, "test".to_string(), 2, 4);

            let func = LiftedFunction {
                func_index: 0,
                param_count: 2,
                local_count: 4,
                name: Some("add".to_string()),
                structural_shapes: FxHashMap::default(),
                structural_layouts: FxHashMap::default(),
                nominal_layouts: FxHashMap::default(),
                function_local_counts: vec![4],
                sync_safe_functions: FxHashMap::from_iter([(0, true)]),
                constant_strings: Vec::new(),
                jit_func,
                profile_assumptions: FxHashMap::default(),
            };

            assert_eq!(func.param_count(), 2);
            assert_eq!(func.local_count(), 4);
            assert_eq!(func.name(), Some("add"));

            let analysis = func.analyze();
            assert!(!analysis.has_suspensions);

            let blocks = func.emit_blocks();
            // Empty JIT function has no blocks
            assert_eq!(blocks.len(), 0);
        }

        #[cfg(not(all(feature = "aot", feature = "jit")))]
        {
            // When JIT is not enabled, LiftedFunction can't be created from bytecode
            // This test just verifies the module compiles
        }
    }

    #[cfg(all(feature = "aot", feature = "jit"))]
    #[test]
    fn test_profile_variant_specializes_shape_load_to_exact_field() {
        use crate::jit::ir::instr::{
            JitBlock, JitBlockId, JitFunction, JitInstr, JitTerminator, Reg,
        };
        use crate::jit::ir::types::JitType;

        let shape_id = crate::vm::object::shape_id_from_member_names(&["a".to_string()]);
        let mut jit_func = JitFunction::new(0, "shape_load".to_string(), 1, 1);
        jit_func.entry = JitBlockId(0);
        jit_func.reg_types.insert(Reg(0), JitType::Value);
        jit_func.reg_types.insert(Reg(1), JitType::Value);
        jit_func.blocks.push(JitBlock {
            id: JitBlockId(0),
            instrs: vec![
                JitInstr::LoadLocal {
                    dest: Reg(0),
                    index: 0,
                },
                JitInstr::LoadFieldShape {
                    dest: Reg(1),
                    object: Reg(0),
                    shape_id,
                    offset: 0,
                    optional: false,
                    stack: Vec::new(),
                    bytecode_offset: 12,
                },
            ],
            terminator: JitTerminator::Return(Some(Reg(1))),
            predecessors: Vec::new(),
        });

        let func = LiftedFunction {
            func_index: 0,
            param_count: 1,
            local_count: 1,
            name: Some("shape_load".to_string()),
            structural_shapes: FxHashMap::from_iter([(shape_id, vec!["a".to_string()])]),
            structural_layouts: FxHashMap::from_iter([(123, vec!["a".to_string()])]),
            nominal_layouts: FxHashMap::default(),
            function_local_counts: vec![1],
            sync_safe_functions: FxHashMap::from_iter([(0, true)]),
            constant_strings: Vec::new(),
            jit_func,
            profile_assumptions: FxHashMap::default(),
        };
        let profile = AotFunctionProfile {
            func_index: 0,
            call_count: 10,
            loop_count: 0,
            sites: vec![AotSiteProfile {
                bytecode_offset: 12,
                kind: AotSiteKind::LoadFieldShape,
                layouts: vec![AotHotLayout {
                    layout_id: 123,
                    hits: 10,
                }],
            }],
        };

        let variants = func.profile_variants(Some(&profile));
        assert_eq!(variants.len(), 1);
        assert_eq!(
            variants[0].guard,
            Some(AotVariantGuard {
                bytecode_offset: 12,
                layout_id: 123,
                guard_arg_index: Some(0),
            })
        );
        let blocks = variants[0].func.emit_blocks();
        assert!(matches!(
            &blocks[0].instructions[1],
            SmInstr::CallHelper {
                helper: HelperCall::ObjectGetField,
                ..
            }
        ));
    }
}
