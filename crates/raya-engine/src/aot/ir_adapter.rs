//! IR adapter (Path A)
//!
//! Adapts `IrFunction` (compiler IR) to the `AotCompilable` trait for
//! AOT compilation of source code.
//!
//! The adapter translates `IrInstr` → `SmInstr` and `Terminator` → `SmTerminator`,
//! using type information from the IR registers to emit typed operations where
//! possible (i32 arithmetic, f64 arithmetic) and falling back to generic helpers
//! for polymorphic or complex operations.

use std::collections::HashSet;

use crate::compiler::ir::block::Terminator;
use crate::compiler::ir::function::IrFunction;
use crate::compiler::ir::instr::{BinaryOp, IrInstr, UnaryOp};
use crate::compiler::ir::module::{IrClass, IrModule};
use crate::compiler::ir::value::{IrConstant, IrValue};
use crate::parser::TypeId;
use rustc_hash::FxHashMap;

use super::analysis::{SuspensionAnalysis, SuspensionKind, SuspensionPoint};
use super::statemachine::*;
use super::traits::AotCompilable;

use crate::parser::TypeContext;

// Well-known TypeIds (from TypeContext)
const NUMBER_TYPE_ID: u32 = TypeContext::NUMBER_TYPE_ID;
#[allow(dead_code)]
const STRING_TYPE_ID: u32 = TypeContext::STRING_TYPE_ID;
const BOOLEAN_TYPE_ID: u32 = TypeContext::BOOLEAN_TYPE_ID;
#[allow(dead_code)]
const CAST_KIND_MASK_FLAG: u32 = 0x8000;
const CAST_TUPLE_LEN_FLAG: u32 = 0x4000;
const CAST_OBJECT_MIN_FIELDS_FLAG: u32 = 0x2000;
const CAST_ARRAY_ELEM_KIND_FLAG: u32 = 0x1000;

#[allow(dead_code)]
const NULL_TYPE_ID: u32 = TypeContext::NULL_TYPE_ID;
const INT_TYPE_ID: u32 = TypeContext::INT_TYPE_ID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExactLayout {
    Structural(u32),
    Nominal(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShapeFieldSpecialization {
    ExactField(u16),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LocalLayoutState {
    slots: Vec<Option<ExactLayout>>,
}

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

/// Adapter that wraps an `IrFunction` to implement `AotCompilable`.
pub struct IrFunctionAdapter<'a> {
    func: &'a IrFunction,
    module: Option<&'a IrModule>,
}

impl<'a> IrFunctionAdapter<'a> {
    /// Create a new adapter for the given IR function.
    pub fn new(func: &'a IrFunction) -> Self {
        Self { func, module: None }
    }

    /// Create a new adapter for the given IR function with module metadata.
    pub fn with_module(module: &'a IrModule, func: &'a IrFunction) -> Self {
        Self {
            func,
            module: Some(module),
        }
    }

    /// Check if a TypeId is the i32 integer type.
    fn is_int(ty: TypeId) -> bool {
        ty.as_u32() == INT_TYPE_ID
    }

    /// Check if a TypeId is the f64 number type.
    fn is_number(ty: TypeId) -> bool {
        ty.as_u32() == NUMBER_TYPE_ID
    }

    /// Check if a TypeId is the boolean type.
    fn is_bool(ty: TypeId) -> bool {
        ty.as_u32() == BOOLEAN_TYPE_ID
    }

    /// Map a Register's id to a u32 for SmInstr.
    fn reg(r: &crate::compiler::ir::value::Register) -> u32 {
        r.id.as_u32()
    }

    /// Map a BasicBlockId to an SmBlockId.
    fn block_id(id: crate::compiler::ir::block::BasicBlockId) -> SmBlockId {
        SmBlockId(id.as_u32())
    }

    fn base_reg_capacity(&self) -> u32 {
        self.func
            .params
            .iter()
            .chain(self.func.locals.iter())
            .map(|reg| reg.id.as_u32() + 1)
            .max()
            .unwrap_or(0)
    }

    fn sm_local_count(&self) -> u32 {
        let mut next_temp = self.base_reg_capacity();
        for block in &self.func.blocks {
            for instr in &block.instructions {
                if matches!(instr, IrInstr::DynGetProp { .. } | IrInstr::DynSetProp { .. }) {
                    next_temp = next_temp.saturating_add(1);
                }
            }
        }
        next_temp.max(self.func.locals.len() as u32)
    }

    fn local_slot_count(&self) -> usize {
        let mut max_slot = self
            .func
            .params
            .len()
            .max(self.func.locals.len())
            .saturating_sub(1);
        for block in &self.func.blocks {
            for instr in &block.instructions {
                match instr {
                    IrInstr::LoadLocal { index, .. } | IrInstr::StoreLocal { index, .. } => {
                        max_slot = max_slot.max(*index as usize);
                    }
                    _ => {}
                }
            }
        }
        max_slot.saturating_add(1)
    }

    fn structural_shape_names(&self, shape_id: u64) -> Option<&[String]> {
        self.module
            .and_then(|module| module.structural_shapes.get(&shape_id))
            .map(Vec::as_slice)
    }

    fn structural_layout_names(&self, layout_id: u32) -> Option<&[String]> {
        self.module
            .and_then(|module| module.structural_layouts.get(&layout_id))
            .map(Vec::as_slice)
    }

    fn nominal_class(&self, nominal_type_id: u32) -> Option<&IrClass> {
        self.module
            .and_then(|module| module.classes.get(nominal_type_id as usize))
    }

    fn exact_layout_field_names(&self, exact: ExactLayout) -> Option<Vec<String>> {
        match exact {
            ExactLayout::Structural(layout_id) => {
                self.structural_layout_names(layout_id).map(|names| names.to_vec())
            }
            ExactLayout::Nominal(nominal_type_id) => {
                let class = self.nominal_class(nominal_type_id)?;
                let mut names = vec![String::new(); class.fields.len()];
                for field in &class.fields {
                    let index = field.index as usize;
                    if index >= names.len() {
                        return None;
                    }
                    names[index] = field.name.clone();
                }
                if names.iter().any(|name| name.is_empty()) {
                    return None;
                }
                Some(names)
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
            .all(|required| provider_names.iter().any(|name| name == required))
    }

    fn analyze_block_local_layouts(&self) -> Vec<LocalLayoutState> {
        let block_count = self.func.blocks.len();
        let slot_count = self.local_slot_count();
        let mut preds: Vec<Vec<usize>> = vec![Vec::new(); block_count];
        let mut block_index_by_id = FxHashMap::default();
        for (idx, block) in self.func.blocks.iter().enumerate() {
            block_index_by_id.insert(block.id, idx);
        }
        for (idx, block) in self.func.blocks.iter().enumerate() {
            for succ in block.successors() {
                if let Some(&succ_idx) = block_index_by_id.get(&succ) {
                    preds[succ_idx].push(idx);
                }
            }
        }

        let mut entry_states = vec![LocalLayoutState::new(slot_count); block_count];
        let mut exit_states = vec![LocalLayoutState::new(slot_count); block_count];
        let mut changed = true;
        while changed {
            changed = false;
            for (block_idx, block) in self.func.blocks.iter().enumerate() {
                let incoming = if preds[block_idx].is_empty() {
                    LocalLayoutState::new(slot_count)
                } else {
                    let pred_states = preds[block_idx]
                        .iter()
                        .map(|pred| &exit_states[*pred])
                        .collect::<Vec<_>>();
                    LocalLayoutState::merge_from_predecessors(&pred_states)
                };
                if entry_states[block_idx] != incoming {
                    entry_states[block_idx] = incoming.clone();
                    changed = true;
                }

                let mut local_state = incoming;
                let mut reg_state: FxHashMap<u32, ExactLayout> = FxHashMap::default();
                for instr in &block.instructions {
                    self.update_layout_tracking(instr, &mut reg_state, &mut local_state);
                }
                if exit_states[block_idx] != local_state {
                    exit_states[block_idx] = local_state;
                    changed = true;
                }
            }
        }
        entry_states
    }

    fn update_layout_tracking(
        &self,
        instr: &IrInstr,
        reg_state: &mut FxHashMap<u32, ExactLayout>,
        local_state: &mut LocalLayoutState,
    ) {
        fn set_reg_layout(
            reg_state: &mut FxHashMap<u32, ExactLayout>,
            reg: &crate::compiler::ir::value::Register,
            layout: Option<ExactLayout>,
        ) {
            if let Some(layout) = layout {
                reg_state.insert(reg.id.as_u32(), layout);
            } else {
                reg_state.remove(&reg.id.as_u32());
            }
        }

        fn reg_layout(
            reg_state: &FxHashMap<u32, ExactLayout>,
            reg: &crate::compiler::ir::value::Register,
        ) -> Option<ExactLayout> {
            reg_state.get(&reg.id.as_u32()).copied()
        }

        match instr {
            IrInstr::Assign { dest, value } => {
                let layout = match value {
                    IrValue::Register(src) => reg_layout(reg_state, src),
                    IrValue::Constant(_) => None,
                };
                set_reg_layout(reg_state, dest, layout);
            }
            IrInstr::LoadLocal { dest, index } => {
                set_reg_layout(reg_state, dest, local_state.get(*index));
            }
            IrInstr::StoreLocal { index, value } => {
                local_state.set(*index, reg_layout(reg_state, value));
            }
            IrInstr::NewType {
                dest,
                nominal_type_id,
            } => {
                set_reg_layout(
                    reg_state,
                    dest,
                    Some(ExactLayout::Nominal(nominal_type_id.as_u32())),
                );
            }
            IrInstr::ConstructType { dest, object, .. } => {
                set_reg_layout(reg_state, dest, reg_layout(reg_state, object));
            }
            IrInstr::ObjectLiteral {
                dest, type_index, ..
            } => {
                set_reg_layout(reg_state, dest, Some(ExactLayout::Structural(*type_index)));
            }
            IrInstr::CastShape { dest, object, .. }
            | IrInstr::CastNominal { dest, object, .. } => {
                set_reg_layout(reg_state, dest, reg_layout(reg_state, object));
            }
            IrInstr::Phi { dest, .. }
            | IrInstr::BinaryOp { dest, .. }
            | IrInstr::UnaryOp { dest, .. }
            | IrInstr::Call { dest: Some(dest), .. }
            | IrInstr::CallMethodExact { dest: Some(dest), .. }
            | IrInstr::CallMethodShape { dest: Some(dest), .. }
            | IrInstr::BindMethod { dest, .. }
            | IrInstr::NativeCall { dest: Some(dest), .. }
            | IrInstr::ModuleNativeCall { dest: Some(dest), .. }
            | IrInstr::IsNominal { dest, .. }
            | IrInstr::ImplementsShape { dest, .. }
            | IrInstr::CastTupleLen { dest, .. }
            | IrInstr::CastObjectMinFields { dest, .. }
            | IrInstr::CastArrayElemKind { dest, .. }
            | IrInstr::CastKindMask { dest, .. }
            | IrInstr::LoadArgCount { dest }
            | IrInstr::LoadArgLocal { dest, .. }
            | IrInstr::LoadGlobal { dest, .. }
            | IrInstr::LoadFieldExact { dest, .. }
            | IrInstr::LoadFieldShape { dest, .. }
            | IrInstr::LoadElement { dest, .. }
            | IrInstr::ArrayLiteral { dest, .. }
            | IrInstr::NewArray { dest, .. }
            | IrInstr::ArrayLen { dest, .. }
            | IrInstr::ArrayPop { dest, .. }
            | IrInstr::StringLen { dest, .. }
            | IrInstr::Typeof { dest, .. }
            | IrInstr::MakeClosure { dest, .. }
            | IrInstr::NewRefCell { dest, .. }
            | IrInstr::NewMutex { dest }
            | IrInstr::NewChannel { dest, .. }
            | IrInstr::Spawn { dest, .. }
            | IrInstr::SpawnClosure { dest, .. }
            | IrInstr::Await { dest, .. }
            | IrInstr::AwaitAll { dest, .. } => {
                set_reg_layout(reg_state, dest, None);
            }
            IrInstr::Yield
            | IrInstr::Call { dest: None, .. }
            | IrInstr::CallMethodExact { dest: None, .. }
            | IrInstr::CallMethodShape { dest: None, .. }
            | IrInstr::NativeCall { dest: None, .. }
            | IrInstr::ModuleNativeCall { dest: None, .. }
            | IrInstr::StoreGlobal { .. }
            | IrInstr::StoreFieldExact { .. }
            | IrInstr::StoreFieldShape { .. }
            | IrInstr::StoreElement { .. }
            | IrInstr::ArrayPush { .. }
            | IrInstr::StoreCaptured { .. }
            | IrInstr::SetClosureCapture { .. }
            | IrInstr::PopToLocal { .. }
            | IrInstr::MutexUnlock { .. }
            | IrInstr::MutexLock { .. }
            | IrInstr::Sleep { .. }
            | IrInstr::TaskCancel { .. } => {}
            IrInstr::LoadCaptured { dest, .. } => {
                set_reg_layout(reg_state, dest, None);
            }
            _ => {}
        }
    }

    /// Translate a single IrInstr to SmInstr(s).
    fn translate_instr(
        &self,
        instr: &IrInstr,
        out: &mut Vec<SmInstr>,
        next_temp: &mut u32,
        reg_state: &mut FxHashMap<u32, ExactLayout>,
        local_state: &mut LocalLayoutState,
    ) {
        let reg_layout =
            |reg: &crate::compiler::ir::value::Register,
             reg_state: &FxHashMap<u32, ExactLayout>|
             -> Option<ExactLayout> { reg_state.get(&reg.id.as_u32()).copied() };
        match instr {
            // === Assignment (constant or register copy) ===
            IrInstr::Assign { dest, value } => {
                match value {
                    IrValue::Constant(c) => match c {
                        IrConstant::I32(v) => out.push(SmInstr::ConstI32 {
                            dest: Self::reg(dest),
                            value: *v,
                        }),
                        IrConstant::F64(v) => out.push(SmInstr::ConstF64 {
                            dest: Self::reg(dest),
                            bits: v.to_bits(),
                        }),
                        IrConstant::Boolean(v) => out.push(SmInstr::ConstBool {
                            dest: Self::reg(dest),
                            value: *v,
                        }),
                        IrConstant::Null => out.push(SmInstr::ConstNull {
                            dest: Self::reg(dest),
                        }),
                        IrConstant::String(value) => {
                            out.push(SmInstr::ConstString {
                                dest: Self::reg(dest),
                                value: value.clone(),
                            });
                        }
                    },
                    IrValue::Register(src) => {
                        out.push(SmInstr::Move {
                            dest: Self::reg(dest),
                            src: Self::reg(src),
                        });
                    }
                }
            }

            // === Binary Operations (type-dispatched) ===
            IrInstr::BinaryOp {
                dest,
                op,
                left,
                right,
            } => {
                let d = Self::reg(dest);
                let l = Self::reg(left);
                let r = Self::reg(right);

                // Use type information for typed dispatch
                if Self::is_int(left.ty) && Self::is_int(right.ty) {
                    match op {
                        // Arithmetic
                        BinaryOp::Add => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Add,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Sub => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Sub,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Mul => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Mul,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Div => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Div,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Mod => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Mod,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Pow => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Pow,
                            left: l,
                            right: r,
                        }),
                        // Comparison
                        BinaryOp::Equal => out.push(SmInstr::I32Cmp {
                            dest: d,
                            op: SmCmpOp::Eq,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::NotEqual => out.push(SmInstr::I32Cmp {
                            dest: d,
                            op: SmCmpOp::Ne,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Less => out.push(SmInstr::I32Cmp {
                            dest: d,
                            op: SmCmpOp::Lt,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::LessEqual => out.push(SmInstr::I32Cmp {
                            dest: d,
                            op: SmCmpOp::Le,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Greater => out.push(SmInstr::I32Cmp {
                            dest: d,
                            op: SmCmpOp::Gt,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::GreaterEqual => out.push(SmInstr::I32Cmp {
                            dest: d,
                            op: SmCmpOp::Ge,
                            left: l,
                            right: r,
                        }),
                        // Bitwise
                        BinaryOp::BitAnd => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::And,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::BitOr => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Or,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::BitXor => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Xor,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::ShiftLeft => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Shl,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::ShiftRight => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Shr,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::UnsignedShiftRight => out.push(SmInstr::I32BinOp {
                            dest: d,
                            op: SmI32BinOp::Ushr,
                            left: l,
                            right: r,
                        }),
                        // Logical (should be on booleans, but emit generic)
                        BinaryOp::And | BinaryOp::Or | BinaryOp::Concat => {
                            Self::emit_generic_binop(d, *op, l, r, out);
                        }
                    }
                } else if Self::is_number(left.ty) && Self::is_number(right.ty) {
                    match op {
                        BinaryOp::Add => out.push(SmInstr::F64BinOp {
                            dest: d,
                            op: SmF64BinOp::Add,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Sub => out.push(SmInstr::F64BinOp {
                            dest: d,
                            op: SmF64BinOp::Sub,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Mul => out.push(SmInstr::F64BinOp {
                            dest: d,
                            op: SmF64BinOp::Mul,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Div => out.push(SmInstr::F64BinOp {
                            dest: d,
                            op: SmF64BinOp::Div,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Mod => out.push(SmInstr::F64BinOp {
                            dest: d,
                            op: SmF64BinOp::Mod,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Pow => out.push(SmInstr::F64BinOp {
                            dest: d,
                            op: SmF64BinOp::Pow,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Equal => out.push(SmInstr::F64Cmp {
                            dest: d,
                            op: SmCmpOp::Eq,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::NotEqual => out.push(SmInstr::F64Cmp {
                            dest: d,
                            op: SmCmpOp::Ne,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Less => out.push(SmInstr::F64Cmp {
                            dest: d,
                            op: SmCmpOp::Lt,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::LessEqual => out.push(SmInstr::F64Cmp {
                            dest: d,
                            op: SmCmpOp::Le,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::Greater => out.push(SmInstr::F64Cmp {
                            dest: d,
                            op: SmCmpOp::Gt,
                            left: l,
                            right: r,
                        }),
                        BinaryOp::GreaterEqual => out.push(SmInstr::F64Cmp {
                            dest: d,
                            op: SmCmpOp::Ge,
                            left: l,
                            right: r,
                        }),
                        _ => Self::emit_generic_binop(d, *op, l, r, out),
                    }
                } else {
                    // Fall back to generic helpers
                    Self::emit_generic_binop(d, *op, l, r, out);
                }
            }

            // === Unary Operations ===
            IrInstr::UnaryOp { dest, op, operand } => {
                let d = Self::reg(dest);
                let s = Self::reg(operand);

                match op {
                    UnaryOp::Neg if Self::is_int(operand.ty) => {
                        out.push(SmInstr::I32Neg { dest: d, src: s });
                    }
                    UnaryOp::Neg if Self::is_number(operand.ty) => {
                        out.push(SmInstr::F64Neg { dest: d, src: s });
                    }
                    UnaryOp::Not if Self::is_bool(operand.ty) => {
                        out.push(SmInstr::BoolNot { dest: d, src: s });
                    }
                    UnaryOp::BitNot if Self::is_int(operand.ty) => {
                        out.push(SmInstr::I32BitNot { dest: d, src: s });
                    }
                    UnaryOp::Neg => {
                        out.push(SmInstr::CallHelper {
                            dest: Some(d),
                            helper: HelperCall::GenericNeg,
                            args: vec![s],
                        });
                    }
                    UnaryOp::Not => {
                        out.push(SmInstr::CallHelper {
                            dest: Some(d),
                            helper: HelperCall::GenericNot,
                            args: vec![s],
                        });
                    }
                    UnaryOp::BitNot => {
                        // Bitwise NOT on non-int → generic
                        out.push(SmInstr::CallHelper {
                            dest: Some(d),
                            helper: HelperCall::GenericNot,
                            args: vec![s],
                        });
                    }
                }
            }

            // === Local Variable Access ===
            IrInstr::LoadLocal { dest, index } => {
                out.push(SmInstr::LoadLocal {
                    dest: Self::reg(dest),
                    index: *index as u32,
                });
            }
            IrInstr::StoreLocal { index, value } => {
                out.push(SmInstr::StoreLocal {
                    index: *index as u32,
                    src: Self::reg(value),
                });
            }
            IrInstr::LoadArgCount { dest } => {
                // LoadArgCount reads the argument count from the call frame
                // For AOT, this needs to call a helper to read from the frame
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::GetArgCount,
                    args: vec![],
                });
            }
            IrInstr::LoadArgLocal { dest, index } => {
                // LoadArgLocal loads from a dynamic local index
                // For AOT, this needs to call a helper
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::LoadArgLocal,
                    args: vec![Self::reg(index)],
                });
            }
            IrInstr::PopToLocal { index } => {
                // PopToLocal is for catch parameters — load resume value
                out.push(SmInstr::LoadResumeValue {
                    dest: *index as u32,
                });
            }

            // === Global Variable Access ===
            IrInstr::LoadGlobal { dest, index } => {
                out.push(SmInstr::LoadGlobal {
                    dest: Self::reg(dest),
                    index: *index as u32,
                });
            }
            IrInstr::StoreGlobal { index, value } => {
                out.push(SmInstr::StoreGlobal {
                    index: *index as u32,
                    src: Self::reg(value),
                });
            }

            // === Object Field Access ===
            IrInstr::LoadFieldExact {
                dest,
                object,
                field,
                optional: _,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ObjectGetField,
                    args: vec![Self::reg(object), *field as u32],
                });
            }
            IrInstr::LoadFieldShape {
                dest,
                object,
                shape_id,
                field,
                optional: _,
            } => {
                if let Some(exact) = reg_layout(object, reg_state).and_then(|layout| {
                    self.specialize_shape_field_access(layout, *shape_id, *field)
                }) {
                    let ShapeFieldSpecialization::ExactField(provider_slot) = exact;
                    out.push(SmInstr::CallHelper {
                        dest: Some(Self::reg(dest)),
                        helper: HelperCall::ObjectGetField,
                        args: vec![Self::reg(object), provider_slot as u32],
                    });
                } else {
                    out.push(SmInstr::CallHelper {
                        dest: Some(Self::reg(dest)),
                        helper: HelperCall::LoadFieldShape,
                        args: vec![
                            Self::reg(object),
                            (*shape_id & 0xFFFF_FFFF) as u32,
                            (*shape_id >> 32) as u32,
                            *field as u32,
                        ],
                    });
                }
            }
            IrInstr::StoreFieldExact {
                object,
                field,
                value,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::ObjectSetField,
                    args: vec![Self::reg(object), *field as u32, Self::reg(value)],
                });
            }
            IrInstr::StoreFieldShape {
                object,
                shape_id,
                field,
                value,
            } => {
                if let Some(exact) = reg_layout(object, reg_state).and_then(|layout| {
                    self.specialize_shape_field_access(layout, *shape_id, *field)
                }) {
                    let ShapeFieldSpecialization::ExactField(provider_slot) = exact;
                    out.push(SmInstr::CallHelper {
                        dest: None,
                        helper: HelperCall::ObjectSetField,
                        args: vec![Self::reg(object), provider_slot as u32, Self::reg(value)],
                    });
                } else {
                    out.push(SmInstr::CallHelper {
                        dest: None,
                        helper: HelperCall::StoreFieldShape,
                        args: vec![
                            Self::reg(object),
                            (*shape_id & 0xFFFF_FFFF) as u32,
                            (*shape_id >> 32) as u32,
                            *field as u32,
                            Self::reg(value),
                        ],
                    });
                }
            }

            // === JSON Property Access ===
            IrInstr::DynGetProp {
                dest,
                object,
                property,
            } => {
                let key_reg = *next_temp;
                *next_temp = next_temp.saturating_add(1);
                out.push(SmInstr::ConstString {
                    dest: key_reg,
                    value: property.clone(),
                });
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::DynGetProp,
                    args: vec![Self::reg(object), key_reg],
                });
            }
            IrInstr::DynSetProp {
                object,
                property,
                value,
            } => {
                let key_reg = *next_temp;
                *next_temp = next_temp.saturating_add(1);
                out.push(SmInstr::ConstString {
                    dest: key_reg,
                    value: property.clone(),
                });
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::DynSetProp,
                    args: vec![Self::reg(object), key_reg, Self::reg(value)],
                });
            }
            IrInstr::DynGetKeyed { dest, object, key } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::DynGetProp,
                    args: vec![Self::reg(object), Self::reg(key)],
                });
            }
            IrInstr::DynSetKeyed { object, key, value } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::DynSetProp,
                    args: vec![Self::reg(object), Self::reg(key), Self::reg(value)],
                });
            }

            // === Array/Element Access ===
            IrInstr::LoadElement { dest, array, index } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ArrayGet,
                    args: vec![Self::reg(array), Self::reg(index)],
                });
            }
            IrInstr::StoreElement {
                array,
                index,
                value,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::ArraySet,
                    args: vec![Self::reg(array), Self::reg(index), Self::reg(value)],
                });
            }

            // === Object/Array Creation ===
            IrInstr::NewType {
                dest,
                nominal_type_id,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::AllocObject,
                    args: vec![nominal_type_id.as_u32()],
                });
            }
            IrInstr::NewArray { dest, len, .. } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::AllocArray,
                    args: vec![0, Self::reg(len)],
                });
            }
            IrInstr::ArrayLiteral { dest, elements, .. } => {
                let mut args: Vec<u32> = elements.iter().map(Self::reg).collect();
                args.insert(0, elements.len() as u32); // count as first arg
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ArrayLiteral,
                    args,
                });
            }
            IrInstr::ObjectLiteral {
                dest,
                type_index,
                fields,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::AllocStructuralObject,
                    args: vec![*type_index, fields.len() as u32],
                });
                for (field_idx, reg) in fields {
                    out.push(SmInstr::CallHelper {
                        dest: None,
                        helper: HelperCall::ObjectSetField,
                        args: vec![Self::reg(dest), *field_idx as u32, Self::reg(reg)],
                    });
                }
            }

            // === Array Operations ===
            IrInstr::ArrayLen { dest, array } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ArrayLen,
                    args: vec![Self::reg(array)],
                });
            }
            IrInstr::ArrayPush { array, element } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::ArrayPush,
                    args: vec![Self::reg(array), Self::reg(element)],
                });
            }
            IrInstr::ArrayPop { dest, array } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ArrayPop,
                    args: vec![Self::reg(array)],
                });
            }

            // === String Operations ===
            IrInstr::StringLen { dest, string } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::StringLen,
                    args: vec![Self::reg(string)],
                });
            }
            IrInstr::StringCompare {
                dest, left, right, ..
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::StringCompare,
                    args: vec![Self::reg(left), Self::reg(right)],
                });
            }
            IrInstr::ToString { dest, operand } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ToString,
                    args: vec![Self::reg(operand)],
                });
            }
            IrInstr::Typeof { dest, operand } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::Typeof,
                    args: vec![Self::reg(operand)],
                });
            }

            // === Function Calls ===
            IrInstr::Call { dest, func, args } => {
                let mut call_args: Vec<u32> = args.iter().map(Self::reg).collect();
                call_args.insert(0, func.as_u32()); // function ID as first arg
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::NativeCall, // Will be resolved to CallAot later
                    args: call_args,
                });
            }
            IrInstr::ConstructType {
                dest,
                object,
                nominal_type_id,
                args,
            } => {
                let mut call_args = vec![Self::reg(object), nominal_type_id.as_u32()];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ConstructType,
                    args: call_args,
                });
            }
            IrInstr::CallMethodExact {
                dest,
                object,
                method,
                args,
                optional: _,
            } => {
                let mut call_args = vec![Self::reg(object), *method as u32];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::NativeCall,
                    args: call_args,
                });
            }
            IrInstr::CallMethodShape {
                dest,
                object,
                shape_id: _,
                method,
                args,
                optional: _,
            } => {
                let mut call_args = vec![Self::reg(object), *method as u32];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::NativeCall,
                    args: call_args,
                });
            }
            IrInstr::NativeCall {
                dest,
                native_id,
                args,
            } => {
                let mut call_args = vec![*native_id as u32];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::NativeCall,
                    args: call_args,
                });
            }
            IrInstr::ModuleNativeCall {
                dest,
                local_idx,
                args,
            } => {
                let mut call_args = vec![*local_idx as u32];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::ModuleNativeCall,
                    args: call_args,
                });
            }
            IrInstr::CallClosure {
                dest,
                closure,
                args,
            } => {
                let mut call_args = vec![Self::reg(closure)];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::CallClosure,
                    args: call_args,
                });
            }

            // === Closures ===
            IrInstr::MakeClosure {
                dest,
                func,
                captures,
            } => {
                let mut args = vec![func.as_u32()];
                args.extend(captures.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::MakeClosure,
                    args,
                });
            }
            IrInstr::LoadCaptured { dest, index } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::LoadCaptured,
                    args: vec![*index as u32],
                });
            }
            IrInstr::StoreCaptured { index, value } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::StoreCaptured,
                    args: vec![*index as u32, Self::reg(value)],
                });
            }
            IrInstr::SetClosureCapture {
                closure,
                index,
                value,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::StoreCaptured,
                    args: vec![Self::reg(closure), *index as u32, Self::reg(value)],
                });
            }

            // === RefCells ===
            IrInstr::NewRefCell {
                dest,
                initial_value,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::NewRefCell,
                    args: vec![Self::reg(initial_value)],
                });
            }
            IrInstr::LoadRefCell { dest, refcell } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::LoadRefCell,
                    args: vec![Self::reg(refcell)],
                });
            }
            IrInstr::StoreRefCell { refcell, value } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::StoreRefCell,
                    args: vec![Self::reg(refcell), Self::reg(value)],
                });
            }

            // === Type Operations ===
            IrInstr::IsNominal {
                dest,
                object,
                nominal_type_id,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::InstanceOf,
                    args: vec![Self::reg(object), nominal_type_id.as_u32()],
                });
            }
            IrInstr::ImplementsShape {
                dest,
                object,
                shape_id,
            } => {
                if let Some(layout) = reg_layout(object, reg_state) {
                    out.push(SmInstr::ConstBool {
                        dest: Self::reg(dest),
                        value: self.exact_layout_satisfies_shape_by_fields(layout, *shape_id),
                    });
                } else {
                    out.push(SmInstr::CallHelper {
                        dest: Some(Self::reg(dest)),
                        helper: HelperCall::ImplementsShape,
                        args: vec![
                            Self::reg(object),
                            (*shape_id & 0xFFFF_FFFF) as u32,
                            (*shape_id >> 32) as u32,
                        ],
                    });
                }
            }
            IrInstr::CastNominal {
                dest,
                object,
                nominal_type_id,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::Cast,
                    args: vec![Self::reg(object), nominal_type_id.as_u32()],
                });
            }
            IrInstr::CastShape {
                dest,
                object,
                shape_id,
            } => {
                if let Some(layout) = reg_layout(object, reg_state)
                    .filter(|layout| self.exact_layout_satisfies_shape_by_fields(*layout, *shape_id))
                {
                    let _ = layout;
                    out.push(SmInstr::Move {
                        dest: Self::reg(dest),
                        src: Self::reg(object),
                    });
                } else {
                    out.push(SmInstr::CallHelper {
                        dest: Some(Self::reg(dest)),
                        helper: HelperCall::CastShape,
                        args: vec![
                            Self::reg(object),
                            (*shape_id & 0xFFFF_FFFF) as u32,
                            (*shape_id >> 32) as u32,
                        ],
                    });
                }
            }
            IrInstr::CastTupleLen {
                dest,
                object,
                expected_len,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::Cast,
                    args: vec![
                        Self::reg(object),
                        CAST_KIND_MASK_FLAG | CAST_TUPLE_LEN_FLAG | (*expected_len as u32),
                    ],
                });
            }
            IrInstr::CastObjectMinFields {
                dest,
                object,
                required_fields,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::Cast,
                    args: vec![
                        Self::reg(object),
                        CAST_KIND_MASK_FLAG | CAST_OBJECT_MIN_FIELDS_FLAG | (*required_fields as u32),
                    ],
                });
            }
            IrInstr::CastArrayElemKind {
                dest,
                object,
                expected_elem_mask,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::Cast,
                    args: vec![
                        Self::reg(object),
                        CAST_KIND_MASK_FLAG | CAST_ARRAY_ELEM_KIND_FLAG | (*expected_elem_mask as u32),
                    ],
                });
            }
            IrInstr::CastKindMask {
                dest,
                object,
                expected_kind_mask,
            } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::Cast,
                    args: vec![
                        Self::reg(object),
                        CAST_KIND_MASK_FLAG | (*expected_kind_mask as u32),
                    ],
                });
            }

            // === Concurrency (suspension points) ===
            IrInstr::Spawn { dest, func, args } => {
                let mut call_args = vec![func.as_u32()];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::Spawn,
                    args: call_args,
                });
            }
            IrInstr::SpawnClosure {
                dest,
                closure,
                args,
            } => {
                let mut call_args = vec![Self::reg(closure)];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::SpawnClosure,
                    args: call_args,
                });
            }
            IrInstr::Await { dest, task } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::AwaitTask,
                    args: vec![Self::reg(task)],
                });
            }
            IrInstr::AwaitAll { dest, tasks } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::AwaitAll,
                    args: vec![Self::reg(tasks)],
                });
            }
            IrInstr::Sleep { duration_ms } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::SleepTask,
                    args: vec![Self::reg(duration_ms)],
                });
            }
            IrInstr::Yield => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::YieldTask,
                    args: vec![],
                });
            }
            IrInstr::NewMutex { dest } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::NewMutex,
                    args: vec![],
                });
            }
            IrInstr::MutexLock { mutex } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::MutexLock,
                    args: vec![Self::reg(mutex)],
                });
            }
            IrInstr::MutexUnlock { mutex } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::MutexUnlock,
                    args: vec![Self::reg(mutex)],
                });
            }
            IrInstr::NewChannel { dest, capacity } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::NewChannel,
                    args: vec![Self::reg(capacity)],
                });
            }
            IrInstr::TaskCancel { task } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::TaskCancel,
                    args: vec![Self::reg(task)],
                });
            }

            // === SSA ===
            IrInstr::Phi { dest, sources } => {
                let sm_sources: Vec<(SmBlockId, u32)> = sources
                    .iter()
                    .map(|(bb, reg)| (Self::block_id(*bb), Self::reg(reg)))
                    .collect();
                out.push(SmInstr::Phi {
                    dest: Self::reg(dest),
                    sources: sm_sources,
                });
            }

            // === Exception Handling ===
            IrInstr::SetupTry { .. } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::SetupTry,
                    args: vec![],
                });
            }
            IrInstr::EndTry => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::EndTry,
                    args: vec![],
                });
            }

            // === Late-bound member (should be resolved before AOT) ===
            IrInstr::LateBoundMember { dest, object, .. } => {
                // LateBoundMember should be resolved by monomorphization before reaching AOT.
                // Emit a generic field load as fallback.
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ObjectGetField,
                    args: vec![Self::reg(object), 0],
                });
            }

            // === Debug ===
            IrInstr::Debugger => {
                // No-op in AOT — debugger breakpoints are not supported in compiled code
            }

            IrInstr::BindMethod {
                dest,
                object,
                method,
            } => {
                // Bound method creation requires GC allocation — stub as field load fallback
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ObjectGetField,
                    args: vec![Self::reg(object), *method as u32],
                });
            }
        }
    }

    /// Translate an IR Terminator to an SmTerminator.
    fn translate_terminator(term: &Terminator) -> SmTerminator {
        match term {
            Terminator::Jump(target) => SmTerminator::Jump(Self::block_id(*target)),
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => SmTerminator::Branch {
                cond: Self::reg(cond),
                then_block: Self::block_id(*then_block),
                else_block: Self::block_id(*else_block),
            },
            Terminator::BranchIfNull {
                value,
                null_block,
                not_null_block,
            } => SmTerminator::BranchNull {
                value: Self::reg(value),
                null_block: Self::block_id(*null_block),
                not_null_block: Self::block_id(*not_null_block),
            },
            Terminator::Return(Some(reg)) => SmTerminator::Return {
                value: Self::reg(reg),
            },
            Terminator::Return(None) => {
                // void return → return null
                SmTerminator::Return { value: u32::MAX } // sentinel for void
            }
            Terminator::Switch {
                value,
                cases,
                default,
            } => SmTerminator::BrTable {
                index: Self::reg(value),
                default: Self::block_id(*default),
                targets: cases.iter().map(|(_, bb)| Self::block_id(*bb)).collect(),
            },
            Terminator::Unreachable => SmTerminator::Unreachable,
            Terminator::Throw(_reg) => {
                // Throw → call helper + unreachable
                // Note: The throw instruction is modeled as an unreachable
                // terminator. The actual throw call is emitted as the last
                // instruction in the block.
                SmTerminator::Unreachable
            }
        }
    }

    /// Emit a generic (polymorphic) binary operation via helper call.
    fn emit_generic_binop(dest: u32, op: BinaryOp, left: u32, right: u32, out: &mut Vec<SmInstr>) {
        let helper = match op {
            BinaryOp::Add => HelperCall::GenericAdd,
            BinaryOp::Sub => HelperCall::GenericSub,
            BinaryOp::Mul => HelperCall::GenericMul,
            BinaryOp::Div => HelperCall::GenericDiv,
            BinaryOp::Mod => HelperCall::GenericMod,
            BinaryOp::Pow => HelperCall::GenericMul, // TODO: GenericPow
            BinaryOp::Equal => HelperCall::GenericEquals,
            BinaryOp::NotEqual => HelperCall::GenericNotEqual,
            BinaryOp::Less => HelperCall::GenericLessThan,
            BinaryOp::LessEqual => HelperCall::GenericLessEqual,
            BinaryOp::Greater => HelperCall::GenericGreater,
            BinaryOp::GreaterEqual => HelperCall::GenericGreaterEqual,
            BinaryOp::And => HelperCall::GenericEquals, // TODO: Logical AND
            BinaryOp::Or => HelperCall::GenericEquals,  // TODO: Logical OR
            BinaryOp::Concat => HelperCall::GenericConcat,
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::ShiftLeft
            | BinaryOp::ShiftRight
            | BinaryOp::UnsignedShiftRight => {
                HelperCall::GenericAdd // TODO: generic bitwise
            }
        };
        out.push(SmInstr::CallHelper {
            dest: Some(dest),
            helper,
            args: vec![left, right],
        });
    }

    /// Analyze a single IR instruction for suspension classification.
    fn classify_instr(instr: &IrInstr) -> Option<SuspensionKind> {
        match instr {
            IrInstr::Await { .. } => Some(SuspensionKind::Await),
            IrInstr::AwaitAll { .. } => Some(SuspensionKind::Await),
            IrInstr::Yield => Some(SuspensionKind::Yield),
            IrInstr::Sleep { .. } => Some(SuspensionKind::Sleep),
            IrInstr::NativeCall { .. } => Some(SuspensionKind::NativeCall),
            IrInstr::ModuleNativeCall { .. } => Some(SuspensionKind::NativeCall),
            IrInstr::Call { .. } => Some(SuspensionKind::AotCall),
            IrInstr::CallMethodExact { .. } | IrInstr::CallMethodShape { .. } => {
                Some(SuspensionKind::NativeCall)
            }
            IrInstr::CallClosure { .. } => Some(SuspensionKind::AotCall),
            IrInstr::MutexLock { .. } => Some(SuspensionKind::MutexLock),
            _ => None,
        }
    }
}

impl AotCompilable for IrFunctionAdapter<'_> {
    fn analyze(&self) -> SuspensionAnalysis {
        let mut points = Vec::new();
        let mut index = 0u32;
        let mut loop_headers = HashSet::new();

        for block in &self.func.blocks {
            for (instr_idx, instr) in block.instructions.iter().enumerate() {
                if let Some(kind) = Self::classify_instr(instr) {
                    points.push(SuspensionPoint {
                        index,
                        block_id: block.id.as_u32(),
                        instr_index: instr_idx as u32,
                        kind,
                        live_locals: HashSet::new(), // TODO: liveness analysis
                    });
                    index += 1;
                }
            }

            // Check for back-edges (loop headers) by looking at jump targets
            // that precede the current block (simple heuristic).
            for succ in block.successors() {
                if succ.as_u32() <= block.id.as_u32() {
                    loop_headers.insert(succ.as_u32());
                    // Add a preemption check at the back-edge
                    points.push(SuspensionPoint {
                        index,
                        block_id: block.id.as_u32(),
                        instr_index: block.instructions.len() as u32,
                        kind: SuspensionKind::PreemptionCheck,
                        live_locals: HashSet::new(),
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

    fn emit_blocks(&self) -> Vec<SmBlock> {
        let mut sm_blocks = Vec::with_capacity(self.func.blocks.len());
        let mut next_temp = self.base_reg_capacity();
        let entry_local_states = self.analyze_block_local_layouts();

        for (block_idx, block) in self.func.blocks.iter().enumerate() {
            let mut instructions = Vec::new();
            let mut reg_state: FxHashMap<u32, ExactLayout> = FxHashMap::default();
            let mut local_state = entry_local_states
                .get(block_idx)
                .cloned()
                .unwrap_or_else(|| LocalLayoutState::new(self.local_slot_count()));

            // Handle Throw terminator: emit the throw call as an instruction
            if let Terminator::Throw(reg) = &block.terminator {
                for instr in &block.instructions {
                    self.translate_instr(
                        instr,
                        &mut instructions,
                        &mut next_temp,
                        &mut reg_state,
                        &mut local_state,
                    );
                    self.update_layout_tracking(instr, &mut reg_state, &mut local_state);
                }
                instructions.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::ThrowException,
                    args: vec![Self::reg(reg)],
                });
            } else {
                for instr in &block.instructions {
                    self.translate_instr(
                        instr,
                        &mut instructions,
                        &mut next_temp,
                        &mut reg_state,
                        &mut local_state,
                    );
                    self.update_layout_tracking(instr, &mut reg_state, &mut local_state);
                }
            }

            sm_blocks.push(SmBlock {
                id: Self::block_id(block.id),
                kind: SmBlockKind::Body,
                instructions,
                terminator: Self::translate_terminator(&block.terminator),
            });
        }

        sm_blocks
    }

    fn param_count(&self) -> u32 {
        self.func.params.len() as u32
    }

    fn local_count(&self) -> u32 {
        self.sm_local_count()
    }

    fn name(&self) -> Option<&str> {
        Some(&self.func.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::block::{BasicBlock, BasicBlockId};
    use crate::compiler::ir::module::IrModule;
    use crate::compiler::ir::value::{IrConstant, IrValue, Register, RegisterId};
    use crate::parser::TypeId;

    fn make_int_reg(id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(INT_TYPE_ID))
    }

    fn make_number_reg(id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(NUMBER_TYPE_ID))
    }

    fn make_bool_reg(id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(BOOLEAN_TYPE_ID))
    }

    #[test]
    fn test_translate_i32_add() {
        let mut func = IrFunction::new(
            "test",
            vec![make_int_reg(0), make_int_reg(1)],
            TypeId::new(INT_TYPE_ID),
        );

        let mut block = BasicBlock::new(BasicBlockId(0));
        block.add_instr(IrInstr::BinaryOp {
            dest: make_int_reg(2),
            op: BinaryOp::Add,
            left: make_int_reg(0),
            right: make_int_reg(1),
        });
        block.set_terminator(Terminator::Return(Some(make_int_reg(2))));
        func.add_block(block);

        let adapter = IrFunctionAdapter::new(&func);
        let blocks = adapter.emit_blocks();

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].instructions.len(), 1);

        match &blocks[0].instructions[0] {
            SmInstr::I32BinOp {
                dest,
                op,
                left,
                right,
            } => {
                assert_eq!(*dest, 2);
                assert_eq!(*op, SmI32BinOp::Add);
                assert_eq!(*left, 0);
                assert_eq!(*right, 1);
            }
            other => panic!("Expected I32BinOp, got {:?}", other),
        }
    }

    #[test]
    fn test_translate_f64_mul() {
        let mut func = IrFunction::new(
            "test",
            vec![make_number_reg(0), make_number_reg(1)],
            TypeId::new(NUMBER_TYPE_ID),
        );

        let mut block = BasicBlock::new(BasicBlockId(0));
        block.add_instr(IrInstr::BinaryOp {
            dest: make_number_reg(2),
            op: BinaryOp::Mul,
            left: make_number_reg(0),
            right: make_number_reg(1),
        });
        block.set_terminator(Terminator::Return(Some(make_number_reg(2))));
        func.add_block(block);

        let adapter = IrFunctionAdapter::new(&func);
        let blocks = adapter.emit_blocks();

        match &blocks[0].instructions[0] {
            SmInstr::F64BinOp { dest, op, .. } => {
                assert_eq!(*dest, 2);
                assert_eq!(*op, SmF64BinOp::Mul);
            }
            other => panic!("Expected F64BinOp, got {:?}", other),
        }
    }

    #[test]
    fn test_translate_constants() {
        let mut func = IrFunction::new("test", vec![], TypeId::new(INT_TYPE_ID));

        let mut block = BasicBlock::new(BasicBlockId(0));
        block.add_instr(IrInstr::Assign {
            dest: make_int_reg(0),
            value: IrValue::Constant(IrConstant::I32(42)),
        });
        block.add_instr(IrInstr::Assign {
            dest: make_number_reg(1),
            value: IrValue::Constant(IrConstant::F64(3.14)),
        });
        block.add_instr(IrInstr::Assign {
            dest: make_bool_reg(2),
            value: IrValue::Constant(IrConstant::Boolean(true)),
        });
        block.set_terminator(Terminator::Return(Some(make_int_reg(0))));
        func.add_block(block);

        let adapter = IrFunctionAdapter::new(&func);
        let blocks = adapter.emit_blocks();

        assert_eq!(blocks[0].instructions.len(), 3);
        assert!(matches!(
            &blocks[0].instructions[0],
            SmInstr::ConstI32 { value: 42, .. }
        ));
        assert!(matches!(
            &blocks[0].instructions[1],
            SmInstr::ConstF64 { .. }
        ));
        assert!(matches!(
            &blocks[0].instructions[2],
            SmInstr::ConstBool { value: true, .. }
        ));
    }

    #[test]
    fn test_suspension_analysis() {
        let mut func = IrFunction::new("test", vec![], TypeId::new(0));

        let mut block = BasicBlock::new(BasicBlockId(0));
        block.add_instr(IrInstr::Spawn {
            dest: make_int_reg(0),
            func: crate::compiler::ir::instr::FunctionId::new(1),
            args: vec![],
        });
        block.add_instr(IrInstr::Await {
            dest: make_int_reg(1),
            task: make_int_reg(0),
        });
        block.set_terminator(Terminator::Return(Some(make_int_reg(1))));
        func.add_block(block);

        let adapter = IrFunctionAdapter::new(&func);
        let analysis = adapter.analyze();

        assert!(analysis.has_suspensions);
        // Spawn doesn't suspend, but Await does
        // Also Call (Spawn maps to Call internally) might be counted
        let await_points: Vec<_> = analysis
            .points
            .iter()
            .filter(|p| p.kind == SuspensionKind::Await)
            .collect();
        assert!(!await_points.is_empty());
    }

    #[test]
    fn test_adapter_metadata() {
        let func = IrFunction::new(
            "add",
            vec![make_int_reg(0), make_int_reg(1)],
            TypeId::new(INT_TYPE_ID),
        );

        let adapter = IrFunctionAdapter::new(&func);

        assert_eq!(adapter.param_count(), 2);
        assert_eq!(adapter.name(), Some("add"));
    }

    #[test]
    fn test_shape_field_load_specializes_to_exact_field_for_structural_layout() {
        let mut module = IrModule::new("test");
        let layout_id = 0xABCD_u32;
        let shape_id = 0x1234_u64;
        module
            .structural_layouts
            .insert(layout_id, vec!["b".to_string(), "a".to_string()]);
        module
            .structural_shapes
            .insert(shape_id, vec!["a".to_string()]);

        let mut func = IrFunction::new("test", vec![], TypeId::new(INT_TYPE_ID));
        let mut block = BasicBlock::new(BasicBlockId(0));
        block.add_instr(IrInstr::ObjectLiteral {
            dest: make_int_reg(0),
            type_index: layout_id,
            fields: vec![],
        });
        block.add_instr(IrInstr::LoadFieldShape {
            dest: make_int_reg(1),
            object: make_int_reg(0),
            shape_id,
            field: 0,
            optional: false,
        });
        block.set_terminator(Terminator::Return(Some(make_int_reg(1))));
        func.add_block(block);

        let adapter = IrFunctionAdapter::with_module(&module, &func);
        let blocks = adapter.emit_blocks();

        assert!(matches!(
            &blocks[0].instructions[1],
            SmInstr::CallHelper {
                helper: HelperCall::ObjectGetField,
                args,
                ..
            } if args == &vec![0, 1]
        ));
    }

    #[test]
    fn test_shape_cast_specializes_to_move_for_exact_structural_layout() {
        let mut module = IrModule::new("test");
        let layout_id = 0xBEEF_u32;
        let shape_id = 0x5678_u64;
        module
            .structural_layouts
            .insert(layout_id, vec!["a".to_string(), "b".to_string()]);
        module
            .structural_shapes
            .insert(shape_id, vec!["a".to_string()]);

        let mut func = IrFunction::new("test", vec![], TypeId::new(INT_TYPE_ID));
        let mut block = BasicBlock::new(BasicBlockId(0));
        block.add_instr(IrInstr::ObjectLiteral {
            dest: make_int_reg(0),
            type_index: layout_id,
            fields: vec![],
        });
        block.add_instr(IrInstr::CastShape {
            dest: make_int_reg(1),
            object: make_int_reg(0),
            shape_id,
        });
        block.set_terminator(Terminator::Return(Some(make_int_reg(1))));
        func.add_block(block);

        let adapter = IrFunctionAdapter::with_module(&module, &func);
        let blocks = adapter.emit_blocks();

        assert!(matches!(
            &blocks[0].instructions[1],
            SmInstr::Move { dest: 1, src: 0 }
        ));
    }
}
