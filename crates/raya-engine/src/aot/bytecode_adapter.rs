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
use rustc_hash::FxHashMap;

use super::analysis::{SuspensionAnalysis, SuspensionKind, SuspensionPoint};
use super::statemachine::{
    SmBlock, SmBlockId, SmBlockKind, SmCmpOp, SmF64BinOp, SmI32BinOp, SmInstr, SmTerminator,
    HelperCall,
};
use super::traits::AotCompilable;

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
    let optimizer = JitOptimizer::new();
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
            jit_func,
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
#[derive(Debug)]
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

    /// The lifted JIT IR (only available when both aot and jit features are enabled).
    #[cfg(all(feature = "aot", feature = "jit"))]
    pub jit_func: JitFunction,
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
        JitInstr::CallNative { .. } => Some(SuspensionKind::NativeCall),

        // May suspend - AOT function call
        JitInstr::Call { .. } => Some(SuspensionKind::AotCall),

        // May suspend - mutex lock
        JitInstr::MutexLock { .. } => Some(SuspensionKind::MutexLock),

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
        self.nominal_layouts.get(&nominal_type_id).map(Vec::as_slice)
    }

    fn exact_layout_field_names(&self, exact: ExactLayout) -> Option<Vec<String>> {
        match exact {
            ExactLayout::Structural(layout_id) => {
                self.structural_layout_names(layout_id).map(|names| names.to_vec())
            }
            ExactLayout::Nominal(nominal_type_id) => {
                self.nominal_layout_names(nominal_type_id).map(|names| names.to_vec())
            }
        }
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
        let reg_layout = |reg: Reg, reg_state: &FxHashMap<Reg, ExactLayout>| reg_state.get(&reg).copied();
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
            JitInstr::ObjectLiteral { dest, type_index, .. } => {
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

    fn emit_instrs_for_block(
        &self,
        out: &mut Vec<SmInstr>,
        instr: &JitInstr,
        reg_state: &mut FxHashMap<Reg, ExactLayout>,
        local_state: &mut LocalLayoutState,
        global_state: &mut FxHashMap<u32, ExactLayout>,
    ) {
        let reg_layout = |reg: Reg, reg_state: &FxHashMap<Reg, ExactLayout>| reg_state.get(&reg).copied();
        match instr {
            JitInstr::NewObject { dest, nominal_type_id, .. } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::AllocObject,
                args: vec![*nominal_type_id],
            }),
            JitInstr::LoadFieldExact { dest, object, offset } => out.push(SmInstr::CallHelper {
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
                ..
            } => {
                if let Some(ShapeFieldSpecialization::ExactField(field)) =
                    reg_layout(*object, reg_state)
                        .and_then(|layout| self.specialize_shape_field_access(layout, *shape_id, *offset))
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
            JitInstr::ImplementsShape { dest, object, shape_id } => {
                if let Some(layout) = reg_layout(*object, reg_state) {
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
            JitInstr::CastShape { dest, object, shape_id, .. } => {
                if let Some(layout) = reg_layout(*object, reg_state)
                    .filter(|layout| self.exact_layout_satisfies_shape_by_fields(*layout, *shape_id))
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
            JitInstr::StoreFieldExact { object, offset, value } => out.push(SmInstr::CallHelper {
                dest: None,
                helper: HelperCall::ObjectSetField,
                args: vec![object.0, *offset as u32, value.0],
            }),
            JitInstr::StoreFieldShape {
                object,
                shape_id,
                offset,
                value,
                ..
            } => {
                if let Some(ShapeFieldSpecialization::ExactField(field)) =
                    reg_layout(*object, reg_state)
                        .and_then(|layout| self.specialize_shape_field_access(layout, *shape_id, *offset))
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
            JitInstr::InstanceOf { dest, object, nominal_type_id } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::InstanceOf,
                args: vec![object.0, *nominal_type_id],
            }),
            JitInstr::Cast { dest, object, nominal_type_id, .. } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::Cast,
                args: vec![object.0, *nominal_type_id],
            }),
            JitInstr::Typeof { dest, operand } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::Typeof,
                args: vec![operand.0],
            }),
            JitInstr::ObjectLiteral { dest, type_index, fields } => {
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
            JitInstr::DynGetKeyed { dest, object, index } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::DynGetProp,
                args: vec![object.0, index.0],
            }),
            JitInstr::DynSetKeyed { object, index, value } => out.push(SmInstr::CallHelper {
                dest: None,
                helper: HelperCall::DynSetProp,
                args: vec![object.0, index.0, value.0],
            }),
            JitInstr::DynNewObject { dest } => out.push(SmInstr::CallHelper {
                dest: Some(dest.0),
                helper: HelperCall::AllocStructuralObject,
                args: vec![crate::vm::object::layout_id_from_ordered_names(&[]), 0],
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
        let mut points = Vec::new();
        let mut index = 0u32;
        let mut loop_headers = std::collections::HashSet::new();

        // Identify loop headers (blocks with back-edges from later blocks)
        for (block_idx, block) in self.jit_func.blocks.iter().enumerate() {
            for pred in &block.predecessors {
                if pred.0 > block_idx as u32 {
                    loop_headers.insert(block_idx as u32);
                }
            }
        }

        // Walk all blocks and instructions to find suspension points
        for (block_idx, block) in self.jit_func.blocks.iter().enumerate() {
            for (instr_idx, instr) in block.instrs.iter().enumerate() {
                if let Some(kind) = classify_suspension(instr) {
                    points.push(SuspensionPoint {
                        index,
                        block_id: block_idx as u32,
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
                self.func_index,
                self.name,
                block_entry_states
            );
        }
        self.jit_func
            .blocks
            .iter()
            .enumerate()
            .map(|(idx, jit_block)| {
                let mut instructions = Vec::new();
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
                jit_func,
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
}
