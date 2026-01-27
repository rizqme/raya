//! Type Substitution
//!
//! Provides mechanisms for substituting type parameters with concrete types
//! during monomorphization.

use crate::ir::block::{BasicBlock, Terminator};
use crate::ir::function::IrFunction;
use crate::ir::instr::{IrInstr, StringCompareMode};
use crate::ir::value::{IrValue, Register, RegisterId};
use raya_parser::{TypeContext, TypeId};
use rustc_hash::FxHashMap;

/// Represents a type parameter that can be substituted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeParamId(pub u32);

impl TypeParamId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Substitutes type parameters with concrete types
///
/// This is used during monomorphization to replace generic type parameters
/// with their actual concrete types for a specific instantiation.
#[derive(Debug, Clone)]
pub struct TypeSubstitution {
    /// Maps type parameter IDs to concrete types
    mappings: FxHashMap<TypeId, TypeId>,
}

impl TypeSubstitution {
    /// Create a new empty substitution
    pub fn new() -> Self {
        Self {
            mappings: FxHashMap::default(),
        }
    }

    /// Create a substitution from parallel lists of parameters and arguments
    ///
    /// # Arguments
    /// * `params` - Type parameter IDs (from the generic definition)
    /// * `args` - Concrete type arguments (from the instantiation)
    pub fn from_params_and_args(params: &[TypeId], args: &[TypeId]) -> Self {
        let mut mappings = FxHashMap::default();
        for (param, arg) in params.iter().zip(args.iter()) {
            mappings.insert(*param, *arg);
        }
        Self { mappings }
    }

    /// Add a single substitution mapping
    pub fn add(&mut self, param: TypeId, concrete: TypeId) {
        self.mappings.insert(param, concrete);
    }

    /// Apply substitution to a type
    ///
    /// Returns the substituted type, or the original if no substitution applies.
    pub fn apply(&self, ty: TypeId) -> TypeId {
        self.mappings.get(&ty).copied().unwrap_or(ty)
    }

    /// Apply substitution to a register, returning a new register with substituted type
    pub fn apply_register(&self, reg: &Register) -> Register {
        Register::new(reg.id, self.apply(reg.ty))
    }

    /// Apply substitution to an IR value
    pub fn apply_value(&self, value: &IrValue) -> IrValue {
        match value {
            IrValue::Register(reg) => IrValue::Register(self.apply_register(reg)),
            IrValue::Constant(c) => IrValue::Constant(c.clone()),
        }
    }

    /// Apply substitution to an IR instruction
    pub fn apply_instr(&self, instr: &IrInstr) -> IrInstr {
        match instr {
            IrInstr::Assign { dest, value } => IrInstr::Assign {
                dest: self.apply_register(dest),
                value: self.apply_value(value),
            },
            IrInstr::BinaryOp {
                dest,
                op,
                left,
                right,
            } => IrInstr::BinaryOp {
                dest: self.apply_register(dest),
                op: *op,
                left: self.apply_register(left),
                right: self.apply_register(right),
            },
            IrInstr::UnaryOp { dest, op, operand } => IrInstr::UnaryOp {
                dest: self.apply_register(dest),
                op: *op,
                operand: self.apply_register(operand),
            },
            IrInstr::Call { dest, func, args } => IrInstr::Call {
                dest: dest.as_ref().map(|d| self.apply_register(d)),
                func: *func,
                args: args.iter().map(|a| self.apply_register(a)).collect(),
            },
            IrInstr::CallMethod {
                dest,
                object,
                method,
                args,
            } => IrInstr::CallMethod {
                dest: dest.as_ref().map(|d| self.apply_register(d)),
                object: self.apply_register(object),
                method: *method,
                args: args.iter().map(|a| self.apply_register(a)).collect(),
            },
            IrInstr::LoadLocal { dest, index } => IrInstr::LoadLocal {
                dest: self.apply_register(dest),
                index: *index,
            },
            IrInstr::StoreLocal { index, value } => IrInstr::StoreLocal {
                index: *index,
                value: self.apply_register(value),
            },
            IrInstr::LoadGlobal { dest, index } => IrInstr::LoadGlobal {
                dest: self.apply_register(dest),
                index: *index,
            },
            IrInstr::StoreGlobal { index, value } => IrInstr::StoreGlobal {
                index: *index,
                value: self.apply_register(value),
            },
            IrInstr::LoadField {
                dest,
                object,
                field,
            } => IrInstr::LoadField {
                dest: self.apply_register(dest),
                object: self.apply_register(object),
                field: *field,
            },
            IrInstr::StoreField {
                object,
                field,
                value,
            } => IrInstr::StoreField {
                object: self.apply_register(object),
                field: *field,
                value: self.apply_register(value),
            },
            IrInstr::LoadElement { dest, array, index } => IrInstr::LoadElement {
                dest: self.apply_register(dest),
                array: self.apply_register(array),
                index: self.apply_register(index),
            },
            IrInstr::StoreElement {
                array,
                index,
                value,
            } => IrInstr::StoreElement {
                array: self.apply_register(array),
                index: self.apply_register(index),
                value: self.apply_register(value),
            },
            IrInstr::NewObject { dest, class } => IrInstr::NewObject {
                dest: self.apply_register(dest),
                class: *class,
            },
            IrInstr::NewArray { dest, len, elem_ty } => IrInstr::NewArray {
                dest: self.apply_register(dest),
                len: self.apply_register(len),
                elem_ty: self.apply(*elem_ty),
            },
            IrInstr::ArrayLiteral {
                dest,
                elements,
                elem_ty,
            } => IrInstr::ArrayLiteral {
                dest: self.apply_register(dest),
                elements: elements.iter().map(|e| self.apply_register(e)).collect(),
                elem_ty: self.apply(*elem_ty),
            },
            IrInstr::ObjectLiteral {
                dest,
                class,
                fields,
            } => IrInstr::ObjectLiteral {
                dest: self.apply_register(dest),
                class: *class,
                fields: fields
                    .iter()
                    .map(|(idx, val)| (*idx, self.apply_register(val)))
                    .collect(),
            },
            IrInstr::ArrayLen { dest, array } => IrInstr::ArrayLen {
                dest: self.apply_register(dest),
                array: self.apply_register(array),
            },
            IrInstr::StringLen { dest, string } => IrInstr::StringLen {
                dest: self.apply_register(dest),
                string: self.apply_register(string),
            },
            IrInstr::Typeof { dest, operand } => IrInstr::Typeof {
                dest: self.apply_register(dest),
                operand: self.apply_register(operand),
            },
            IrInstr::Phi { dest, sources } => IrInstr::Phi {
                dest: self.apply_register(dest),
                sources: sources
                    .iter()
                    .map(|(block, reg)| (*block, self.apply_register(reg)))
                    .collect(),
            },
            IrInstr::MakeClosure {
                dest,
                func,
                captures,
            } => IrInstr::MakeClosure {
                dest: self.apply_register(dest),
                func: *func,
                captures: captures.iter().map(|c| self.apply_register(c)).collect(),
            },
            IrInstr::LoadCaptured { dest, index } => IrInstr::LoadCaptured {
                dest: self.apply_register(dest),
                index: *index,
            },
            IrInstr::StoreCaptured { index, value } => IrInstr::StoreCaptured {
                index: *index,
                value: self.apply_register(value),
            },
            IrInstr::SetClosureCapture { closure, index, value } => IrInstr::SetClosureCapture {
                closure: self.apply_register(closure),
                index: *index,
                value: self.apply_register(value),
            },
            IrInstr::NewRefCell { dest, initial_value } => IrInstr::NewRefCell {
                dest: self.apply_register(dest),
                initial_value: self.apply_register(initial_value),
            },
            IrInstr::LoadRefCell { dest, refcell } => IrInstr::LoadRefCell {
                dest: self.apply_register(dest),
                refcell: self.apply_register(refcell),
            },
            IrInstr::StoreRefCell { refcell, value } => IrInstr::StoreRefCell {
                refcell: self.apply_register(refcell),
                value: self.apply_register(value),
            },
            IrInstr::CallClosure {
                dest,
                closure,
                args,
            } => IrInstr::CallClosure {
                dest: dest.as_ref().map(|d| self.apply_register(d)),
                closure: self.apply_register(closure),
                args: args.iter().map(|a| self.apply_register(a)).collect(),
            },
            IrInstr::StringCompare {
                dest,
                left,
                right,
                mode,
                negate,
            } => IrInstr::StringCompare {
                dest: self.apply_register(dest),
                left: self.apply_register(left),
                right: self.apply_register(right),
                mode: *mode,
                negate: *negate,
            },
            IrInstr::ToString { dest, operand } => IrInstr::ToString {
                dest: self.apply_register(dest),
                operand: self.apply_register(operand),
            },
            IrInstr::Spawn { dest, func, args } => IrInstr::Spawn {
                dest: self.apply_register(dest),
                func: *func,
                args: args.iter().map(|a| self.apply_register(a)).collect(),
            },
            IrInstr::SpawnClosure { dest, closure, args } => IrInstr::SpawnClosure {
                dest: self.apply_register(dest),
                closure: self.apply_register(closure),
                args: args.iter().map(|a| self.apply_register(a)).collect(),
            },
            IrInstr::Await { dest, task } => IrInstr::Await {
                dest: self.apply_register(dest),
                task: self.apply_register(task),
            },
            IrInstr::AwaitAll { dest, tasks } => IrInstr::AwaitAll {
                dest: self.apply_register(dest),
                tasks: self.apply_register(tasks),
            },
            IrInstr::Sleep { duration_ms } => IrInstr::Sleep {
                duration_ms: self.apply_register(duration_ms),
            },
            IrInstr::Yield => IrInstr::Yield,
            IrInstr::SetupTry { catch_block, finally_block } => IrInstr::SetupTry {
                catch_block: *catch_block,
                finally_block: *finally_block,
            },
            IrInstr::EndTry => IrInstr::EndTry,
            IrInstr::PopToLocal { index } => IrInstr::PopToLocal { index: *index },
        }
    }

    /// Apply substitution to a terminator
    pub fn apply_terminator(&self, term: &Terminator) -> Terminator {
        match term {
            Terminator::Return(val) => {
                Terminator::Return(val.as_ref().map(|v| self.apply_register(v)))
            }
            Terminator::Jump(target) => Terminator::Jump(*target),
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => Terminator::Branch {
                cond: self.apply_register(cond),
                then_block: *then_block,
                else_block: *else_block,
            },
            Terminator::BranchIfNull {
                value,
                null_block,
                not_null_block,
            } => Terminator::BranchIfNull {
                value: self.apply_register(value),
                null_block: *null_block,
                not_null_block: *not_null_block,
            },
            Terminator::Switch {
                value,
                cases,
                default,
            } => Terminator::Switch {
                value: self.apply_register(value),
                cases: cases.clone(),
                default: *default,
            },
            Terminator::Throw(val) => Terminator::Throw(self.apply_register(val)),
            Terminator::Unreachable => Terminator::Unreachable,
        }
    }

    /// Apply substitution to a basic block
    pub fn apply_block(&self, block: &BasicBlock) -> BasicBlock {
        let mut new_block = BasicBlock::new(block.id);
        if let Some(label) = &block.label {
            new_block.label = Some(label.clone());
        }

        // Apply to all instructions
        for instr in &block.instructions {
            new_block.add_instr(self.apply_instr(instr));
        }

        // Apply to terminator
        new_block.set_terminator(self.apply_terminator(&block.terminator));

        new_block
    }

    /// Apply substitution to an entire function
    ///
    /// This creates a new function with all types substituted.
    pub fn apply_function(&self, func: &IrFunction) -> IrFunction {
        let new_params: Vec<Register> = func.params.iter().map(|p| self.apply_register(p)).collect();

        let mut new_func = IrFunction::new(func.name.clone(), new_params, self.apply(func.return_ty));

        // Apply to locals
        for local in &func.locals {
            new_func.add_local(self.apply_register(local));
        }

        // Apply to all blocks
        for block in &func.blocks {
            new_func.add_block(self.apply_block(block));
        }

        new_func.entry_block = func.entry_block;

        new_func
    }

    /// Check if this substitution has any mappings
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// Get the number of mappings
    pub fn len(&self) -> usize {
        self.mappings.len()
    }
}

impl Default for TypeSubstitution {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::block::BasicBlockId;
    use crate::ir::instr::BinaryOp;
    use crate::ir::value::IrConstant;

    fn make_reg(id: u32, ty: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(ty))
    }

    #[test]
    fn test_substitution_apply_type() {
        let mut sub = TypeSubstitution::new();
        sub.add(TypeId::new(100), TypeId::new(1)); // T -> number

        assert_eq!(sub.apply(TypeId::new(100)), TypeId::new(1));
        assert_eq!(sub.apply(TypeId::new(2)), TypeId::new(2)); // Unchanged
    }

    #[test]
    fn test_substitution_apply_register() {
        let mut sub = TypeSubstitution::new();
        sub.add(TypeId::new(100), TypeId::new(1)); // T -> number

        let reg = make_reg(0, 100); // r0 with type T
        let result = sub.apply_register(&reg);

        assert_eq!(result.ty, TypeId::new(1)); // Now has type number
        assert_eq!(result.id, RegisterId::new(0)); // Same register ID
    }

    #[test]
    fn test_substitution_apply_instr() {
        let mut sub = TypeSubstitution::new();
        sub.add(TypeId::new(100), TypeId::new(1));

        let instr = IrInstr::BinaryOp {
            dest: make_reg(2, 100),
            op: BinaryOp::Add,
            left: make_reg(0, 100),
            right: make_reg(1, 100),
        };

        let result = sub.apply_instr(&instr);

        if let IrInstr::BinaryOp { dest, left, right, .. } = result {
            assert_eq!(dest.ty, TypeId::new(1));
            assert_eq!(left.ty, TypeId::new(1));
            assert_eq!(right.ty, TypeId::new(1));
        } else {
            panic!("Expected BinaryOp");
        }
    }

    #[test]
    fn test_substitution_apply_array_literal() {
        let mut sub = TypeSubstitution::new();
        sub.add(TypeId::new(100), TypeId::new(1)); // T -> number

        let instr = IrInstr::ArrayLiteral {
            dest: make_reg(0, 100),
            elements: vec![make_reg(1, 100), make_reg(2, 100)],
            elem_ty: TypeId::new(100),
        };

        let result = sub.apply_instr(&instr);

        if let IrInstr::ArrayLiteral { dest, elements, elem_ty } = result {
            assert_eq!(dest.ty, TypeId::new(1));
            assert_eq!(elements[0].ty, TypeId::new(1));
            assert_eq!(elements[1].ty, TypeId::new(1));
            assert_eq!(elem_ty, TypeId::new(1));
        } else {
            panic!("Expected ArrayLiteral");
        }
    }

    #[test]
    fn test_substitution_from_params_and_args() {
        let params = vec![TypeId::new(100), TypeId::new(101)];
        let args = vec![TypeId::new(1), TypeId::new(3)]; // number, string

        let sub = TypeSubstitution::from_params_and_args(&params, &args);

        assert_eq!(sub.apply(TypeId::new(100)), TypeId::new(1));
        assert_eq!(sub.apply(TypeId::new(101)), TypeId::new(3));
    }

    #[test]
    fn test_substitution_apply_function() {
        let mut sub = TypeSubstitution::new();
        sub.add(TypeId::new(100), TypeId::new(1)); // T -> number

        // Create a simple function: fn identity(x: T) -> T
        let mut func = IrFunction::new(
            "identity",
            vec![make_reg(0, 100)],
            TypeId::new(100),
        );

        let mut block = crate::ir::block::BasicBlock::new(BasicBlockId(0));
        block.set_terminator(Terminator::Return(Some(make_reg(0, 100))));
        func.add_block(block);

        let result = sub.apply_function(&func);

        assert_eq!(result.params[0].ty, TypeId::new(1));
        assert_eq!(result.return_ty, TypeId::new(1));
    }
}
