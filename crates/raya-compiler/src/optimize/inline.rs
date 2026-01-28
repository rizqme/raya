//! Function Inlining Optimization
//!
//! Automatically inlines small functions (1-2 instructions) at call sites.
//! This eliminates function call overhead for trivial wrapper methods like `mutex.lock()`.

use crate::ir::block::Terminator;
use crate::ir::instr::{FunctionId, IrInstr};
use crate::ir::value::{IrValue, Register, RegisterId};
use crate::ir::{IrFunction, IrModule};
use rustc_hash::{FxHashMap, FxHashSet};

/// Maximum number of instructions (excluding terminator) for a function to be inlinable
const MAX_INLINE_INSTRUCTIONS: usize = 2;

/// Function inliner
pub struct Inliner;

impl Inliner {
    /// Create a new inliner
    pub fn new() -> Self {
        Self
    }

    /// Run inlining on an entire module
    pub fn inline(&self, module: &mut IrModule) {
        // Phase 1: Find all inlinable functions and cache their bodies
        let inlinable = self.find_inlinable_functions(module);

        if inlinable.is_empty() {
            return;
        }

        // Phase 2: Inline calls in all functions
        // We need to iterate by index to allow mutable access
        for func_idx in 0..module.functions.len() {
            self.inline_calls_in_function(func_idx, &inlinable, module);
        }
    }

    /// Find all functions that are candidates for inlining
    fn find_inlinable_functions(&self, module: &IrModule) -> FxHashMap<FunctionId, InlinableBody> {
        let mut inlinable = FxHashMap::default();

        for (idx, func) in module.functions.iter().enumerate() {
            let func_id = FunctionId::new(idx as u32);
            if let Some(body) = self.extract_inlinable_body(func, func_id) {
                inlinable.insert(func_id, body);
            }
        }

        inlinable
    }

    /// Check if a function is inlinable and extract its body if so
    fn extract_inlinable_body(&self, func: &IrFunction, func_id: FunctionId) -> Option<InlinableBody> {
        // Must have exactly one basic block
        if func.blocks.len() != 1 {
            return None;
        }

        let block = &func.blocks[0];

        // Must have at most MAX_INLINE_INSTRUCTIONS instructions
        if block.instructions.len() > MAX_INLINE_INSTRUCTIONS {
            return None;
        }

        // Must have a simple return terminator
        let return_value = match &block.terminator {
            Terminator::Return(ret) => ret.clone(),
            _ => return None,
        };

        // Check that instructions don't contain problematic patterns
        let param_count = func.params.len();
        for instr in &block.instructions {
            if !self.is_inlinable_instruction(instr, func_id, param_count) {
                return None;
            }
        }

        Some(InlinableBody {
            params: func.params.clone(),
            param_count: func.params.len(),
            instructions: block.instructions.clone(),
            return_value,
        })
    }

    /// Check if an instruction can be inlined
    fn is_inlinable_instruction(&self, instr: &IrInstr, self_func_id: FunctionId, param_count: usize) -> bool {
        match instr {
            // Recursive calls cannot be inlined
            IrInstr::Call { func, .. } if *func == self_func_id => false,
            // Async operations cannot be inlined (they create tasks)
            IrInstr::Spawn { .. } | IrInstr::SpawnClosure { .. } | IrInstr::Await { .. } | IrInstr::AwaitAll { .. } => {
                false
            }
            // Try/catch blocks cannot be inlined
            IrInstr::SetupTry { .. } | IrInstr::EndTry => false,
            // Closures with captures are complex
            IrInstr::MakeClosure { captures, .. } if !captures.is_empty() => false,
            // StoreLocal cannot be inlined (would store to caller's local slots)
            IrInstr::StoreLocal { .. } => false,
            // LoadLocal for non-parameter indices cannot be inlined
            IrInstr::LoadLocal { index, .. } if (*index as usize) >= param_count => false,
            // Everything else is fine
            _ => true,
        }
    }

    /// Inline calls in a single function
    fn inline_calls_in_function(
        &self,
        func_idx: usize,
        inlinable: &FxHashMap<FunctionId, InlinableBody>,
        module: &mut IrModule,
    ) {
        // Find the maximum register ID used in this function to allocate fresh registers
        let mut max_reg_id = self.find_max_register_id(&module.functions[func_idx]);

        // Process each block
        for block_idx in 0..module.functions[func_idx].blocks.len() {
            let mut new_instructions = Vec::new();
            let mut i = 0;

            while i < module.functions[func_idx].blocks[block_idx].instructions.len() {
                let instr = &module.functions[func_idx].blocks[block_idx].instructions[i];

                // Check if this is a call to an inlinable function
                if let IrInstr::Call { dest, func, args } = instr {
                    if let Some(body) = inlinable.get(func) {
                        // Inline this call
                        let inlined = self.inline_call(dest.clone(), args, body, &mut max_reg_id);
                        new_instructions.extend(inlined);
                        i += 1;
                        continue;
                    }
                }

                // Keep the original instruction
                new_instructions.push(module.functions[func_idx].blocks[block_idx].instructions[i].clone());
                i += 1;
            }

            // Replace the block's instructions
            module.functions[func_idx].blocks[block_idx].instructions = new_instructions;
        }
    }

    /// Find the maximum register ID used in a function
    fn find_max_register_id(&self, func: &IrFunction) -> u32 {
        let mut max_id = 0u32;

        // Check params
        for param in &func.params {
            max_id = max_id.max(param.id.as_u32());
        }

        // Check locals
        for local in &func.locals {
            max_id = max_id.max(local.id.as_u32());
        }

        // Check all instructions
        for block in &func.blocks {
            for instr in &block.instructions {
                if let Some(dest) = instr.dest() {
                    max_id = max_id.max(dest.id.as_u32());
                }
                self.collect_register_ids(instr, &mut |id| {
                    max_id = max_id.max(id);
                });
            }
        }

        max_id
    }

    /// Collect all register IDs used in an instruction
    fn collect_register_ids<F>(&self, instr: &IrInstr, f: &mut F)
    where
        F: FnMut(u32),
    {
        match instr {
            IrInstr::Assign { dest, value } => {
                f(dest.id.as_u32());
                if let IrValue::Register(reg) = value {
                    f(reg.id.as_u32());
                }
            }
            IrInstr::BinaryOp { dest, left, right, .. } => {
                f(dest.id.as_u32());
                f(left.id.as_u32());
                f(right.id.as_u32());
            }
            IrInstr::UnaryOp { dest, operand, .. } => {
                f(dest.id.as_u32());
                f(operand.id.as_u32());
            }
            IrInstr::Call { dest, args, .. } => {
                if let Some(d) = dest {
                    f(d.id.as_u32());
                }
                for arg in args {
                    f(arg.id.as_u32());
                }
            }
            IrInstr::LoadLocal { dest, .. } => f(dest.id.as_u32()),
            IrInstr::StoreLocal { value, .. } => f(value.id.as_u32()),
            IrInstr::LoadField { dest, object, .. } => {
                f(dest.id.as_u32());
                f(object.id.as_u32());
            }
            IrInstr::StoreField { object, value, .. } => {
                f(object.id.as_u32());
                f(value.id.as_u32());
            }
            IrInstr::MutexLock { mutex } => f(mutex.id.as_u32()),
            IrInstr::MutexUnlock { mutex } => f(mutex.id.as_u32()),
            IrInstr::NewMutex { dest } => f(dest.id.as_u32()),
            IrInstr::NewChannel { dest, capacity } => {
                f(dest.id.as_u32());
                f(capacity.id.as_u32());
            }
            IrInstr::Sleep { duration_ms } => f(duration_ms.id.as_u32()),
            IrInstr::TaskCancel { task } => f(task.id.as_u32()),
            IrInstr::ArrayLen { dest, array } => {
                f(dest.id.as_u32());
                f(array.id.as_u32());
            }
            IrInstr::ArrayPush { array, element } => {
                f(array.id.as_u32());
                f(element.id.as_u32());
            }
            IrInstr::ArrayPop { dest, array } => {
                f(dest.id.as_u32());
                f(array.id.as_u32());
            }
            IrInstr::NativeCall { dest, args, .. } => {
                if let Some(d) = dest {
                    f(d.id.as_u32());
                }
                for arg in args {
                    f(arg.id.as_u32());
                }
            }
            // Add other cases as needed; these are the main ones for small inlinable functions
            _ => {}
        }
    }

    /// Inline a single call, returning the replacement instructions
    fn inline_call(
        &self,
        call_dest: Option<Register>,
        args: &[Register],
        body: &InlinableBody,
        max_reg_id: &mut u32,
    ) -> Vec<IrInstr> {
        // Build parameter -> argument mapping (by register ID)
        let mut reg_map: FxHashMap<RegisterId, Register> = FxHashMap::default();
        for (param, arg) in body.params.iter().zip(args.iter()) {
            reg_map.insert(param.id, arg.clone());
        }

        // Track which registers we've already allocated replacements for
        let mut allocated: FxHashSet<RegisterId> = FxHashSet::default();
        for param in &body.params {
            allocated.insert(param.id);
        }

        let mut result = Vec::new();

        // Clone and rename each instruction
        for instr in &body.instructions {
            let renamed =
                self.rename_instruction(instr, args, body.param_count, &mut reg_map, &mut allocated, max_reg_id);
            // rename_instruction may return None if the instruction was elided (e.g., LoadLocal for param)
            if let Some(renamed_instr) = renamed {
                result.push(renamed_instr);
            }
        }

        // Handle return value: if the callee returns a value and the caller expects one,
        // we need to assign the return register to the call's destination
        if let (Some(ret_reg), Some(call_dest)) = (&body.return_value, &call_dest) {
            let renamed_ret = self.rename_register(ret_reg, &reg_map);
            // Add assignment: call_dest = renamed_ret
            result.push(IrInstr::Assign {
                dest: call_dest.clone(),
                value: IrValue::Register(renamed_ret),
            });
        }

        result
    }

    /// Rename registers in an instruction
    /// Returns None if the instruction should be elided (e.g., LoadLocal for a parameter)
    fn rename_instruction(
        &self,
        instr: &IrInstr,
        args: &[Register],
        param_count: usize,
        reg_map: &mut FxHashMap<RegisterId, Register>,
        allocated: &mut FxHashSet<RegisterId>,
        max_reg_id: &mut u32,
    ) -> Option<IrInstr> {
        match instr {
            IrInstr::Assign { dest, value } => Some(IrInstr::Assign {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                value: match value {
                    IrValue::Register(reg) => IrValue::Register(self.rename_register(reg, reg_map)),
                    IrValue::Constant(c) => IrValue::Constant(c.clone()),
                },
            }),
            IrInstr::BinaryOp { dest, op, left, right } => Some(IrInstr::BinaryOp {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                op: *op,
                left: self.rename_register(left, reg_map),
                right: self.rename_register(right, reg_map),
            }),
            IrInstr::UnaryOp { dest, op, operand } => Some(IrInstr::UnaryOp {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                op: *op,
                operand: self.rename_register(operand, reg_map),
            }),
            IrInstr::LoadLocal { dest, index } => {
                // If this is loading a parameter, map the dest directly to the argument
                // and elide the LoadLocal instruction
                if (*index as usize) < param_count {
                    // Map dest register to the corresponding argument
                    let arg = &args[*index as usize];
                    reg_map.insert(dest.id, arg.clone());
                    allocated.insert(dest.id);
                    None // Elide this instruction
                } else {
                    // Non-parameter local - keep the LoadLocal but this is risky
                    // (would load from caller's local slot) - for safety, we shouldn't
                    // inline functions that access non-param locals
                    Some(IrInstr::LoadLocal {
                        dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                        index: *index,
                    })
                }
            }
            IrInstr::StoreLocal { index, value } => {
                // If storing to a parameter slot, this is unusual but could happen
                // For safety, don't inline functions that store to locals
                if (*index as usize) < param_count {
                    // Storing to a parameter - this is a reassignment of the param
                    // We can't easily handle this, so keep it (will be wrong)
                    // TODO: For now, we should filter out functions that store to params
                    Some(IrInstr::StoreLocal {
                        index: *index,
                        value: self.rename_register(value, reg_map),
                    })
                } else {
                    Some(IrInstr::StoreLocal {
                        index: *index,
                        value: self.rename_register(value, reg_map),
                    })
                }
            }
            IrInstr::LoadField { dest, object, field } => Some(IrInstr::LoadField {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                object: self.rename_register(object, reg_map),
                field: *field,
            }),
            IrInstr::StoreField { object, field, value } => Some(IrInstr::StoreField {
                object: self.rename_register(object, reg_map),
                field: *field,
                value: self.rename_register(value, reg_map),
            }),
            IrInstr::Call { dest, func, args: call_args } => Some(IrInstr::Call {
                dest: dest.as_ref().map(|d| self.rename_or_allocate(d, reg_map, allocated, max_reg_id)),
                func: *func,
                args: call_args.iter().map(|a| self.rename_register(a, reg_map)).collect(),
            }),
            IrInstr::NativeCall { dest, native_id, args: native_args } => Some(IrInstr::NativeCall {
                dest: dest.as_ref().map(|d| self.rename_or_allocate(d, reg_map, allocated, max_reg_id)),
                native_id: *native_id,
                args: native_args.iter().map(|a| self.rename_register(a, reg_map)).collect(),
            }),
            IrInstr::MutexLock { mutex } => Some(IrInstr::MutexLock {
                mutex: self.rename_register(mutex, reg_map),
            }),
            IrInstr::MutexUnlock { mutex } => Some(IrInstr::MutexUnlock {
                mutex: self.rename_register(mutex, reg_map),
            }),
            IrInstr::NewMutex { dest } => Some(IrInstr::NewMutex {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
            }),
            IrInstr::NewChannel { dest, capacity } => Some(IrInstr::NewChannel {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                capacity: self.rename_register(capacity, reg_map),
            }),
            IrInstr::Sleep { duration_ms } => Some(IrInstr::Sleep {
                duration_ms: self.rename_register(duration_ms, reg_map),
            }),
            IrInstr::Yield => Some(IrInstr::Yield),
            IrInstr::TaskCancel { task } => Some(IrInstr::TaskCancel {
                task: self.rename_register(task, reg_map),
            }),
            IrInstr::ArrayLen { dest, array } => Some(IrInstr::ArrayLen {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                array: self.rename_register(array, reg_map),
            }),
            IrInstr::ArrayPush { array, element } => Some(IrInstr::ArrayPush {
                array: self.rename_register(array, reg_map),
                element: self.rename_register(element, reg_map),
            }),
            IrInstr::ArrayPop { dest, array } => Some(IrInstr::ArrayPop {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                array: self.rename_register(array, reg_map),
            }),
            IrInstr::NewObject { dest, class } => Some(IrInstr::NewObject {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                class: *class,
            }),
            IrInstr::InstanceOf { dest, object, class_id } => Some(IrInstr::InstanceOf {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                object: self.rename_register(object, reg_map),
                class_id: *class_id,
            }),
            IrInstr::Cast { dest, object, class_id } => Some(IrInstr::Cast {
                dest: self.rename_or_allocate(dest, reg_map, allocated, max_reg_id),
                object: self.rename_register(object, reg_map),
                class_id: *class_id,
            }),
            // For instructions we don't expect in inlinable functions,
            // clone them as-is (they should have been filtered out)
            other => Some(other.clone()),
        }
    }

    /// Rename a register using the mapping, or return as-is if not mapped
    fn rename_register(&self, reg: &Register, reg_map: &FxHashMap<RegisterId, Register>) -> Register {
        reg_map.get(&reg.id).cloned().unwrap_or_else(|| reg.clone())
    }

    /// Rename a destination register: allocate a fresh one if not already mapped
    fn rename_or_allocate(
        &self,
        reg: &Register,
        reg_map: &mut FxHashMap<RegisterId, Register>,
        allocated: &mut FxHashSet<RegisterId>,
        max_reg_id: &mut u32,
    ) -> Register {
        if let Some(mapped) = reg_map.get(&reg.id) {
            return mapped.clone();
        }

        if allocated.contains(&reg.id) {
            // Already allocated, use the mapping
            return reg.clone();
        }

        // Allocate a fresh register
        *max_reg_id += 1;
        let fresh = Register::new(RegisterId::new(*max_reg_id), reg.ty);
        reg_map.insert(reg.id, fresh.clone());
        allocated.insert(reg.id);
        fresh
    }
}

impl Default for Inliner {
    fn default() -> Self {
        Self::new()
    }
}

/// Cached body of an inlinable function
#[derive(Debug, Clone)]
struct InlinableBody {
    /// Parameter registers
    params: Vec<Register>,
    /// Number of parameters (for detecting LoadLocal of params)
    param_count: usize,
    /// Instructions to inline
    instructions: Vec<IrInstr>,
    /// Return value register (if any)
    return_value: Option<Register>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::block::{BasicBlock, BasicBlockId, Terminator};
    use crate::ir::value::IrConstant;
    use crate::ir::IrModule;
    use raya_parser::TypeId;

    fn make_reg(id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(0))
    }

    fn make_simple_function(name: &str, instrs: Vec<IrInstr>, ret: Option<Register>) -> IrFunction {
        let mut func = IrFunction::new(name, vec![], TypeId::new(0));
        let mut block = BasicBlock::new(BasicBlockId(0));
        for instr in instrs {
            block.add_instr(instr);
        }
        block.set_terminator(Terminator::Return(ret));
        func.add_block(block);
        func
    }

    #[test]
    fn test_is_inlinable_empty_function() {
        let inliner = Inliner::new();
        let func = make_simple_function("empty", vec![], None);
        let body = inliner.extract_inlinable_body(&func, FunctionId::new(0));
        assert!(body.is_some());
    }

    #[test]
    fn test_is_inlinable_single_instruction() {
        let inliner = Inliner::new();
        let instrs = vec![IrInstr::Assign {
            dest: make_reg(0),
            value: IrValue::Constant(IrConstant::I32(42)),
        }];
        let func = make_simple_function("single", instrs, Some(make_reg(0)));
        let body = inliner.extract_inlinable_body(&func, FunctionId::new(0));
        assert!(body.is_some());
    }

    #[test]
    fn test_is_inlinable_two_instructions() {
        let inliner = Inliner::new();
        let instrs = vec![
            IrInstr::LoadField {
                dest: make_reg(0),
                object: make_reg(1),
                field: 0,
            },
            IrInstr::MutexLock { mutex: make_reg(0) },
        ];
        let func = make_simple_function("lock", instrs, None);
        let body = inliner.extract_inlinable_body(&func, FunctionId::new(0));
        assert!(body.is_some());
    }

    #[test]
    fn test_not_inlinable_too_many_instructions() {
        let inliner = Inliner::new();
        let instrs = vec![
            IrInstr::Assign {
                dest: make_reg(0),
                value: IrValue::Constant(IrConstant::I32(1)),
            },
            IrInstr::Assign {
                dest: make_reg(1),
                value: IrValue::Constant(IrConstant::I32(2)),
            },
            IrInstr::Assign {
                dest: make_reg(2),
                value: IrValue::Constant(IrConstant::I32(3)),
            },
        ];
        let func = make_simple_function("many", instrs, None);
        let body = inliner.extract_inlinable_body(&func, FunctionId::new(0));
        assert!(body.is_none());
    }

    #[test]
    fn test_not_inlinable_multiple_blocks() {
        let inliner = Inliner::new();
        let mut func = IrFunction::new("multi", vec![], TypeId::new(0));

        let mut block1 = BasicBlock::new(BasicBlockId(0));
        block1.set_terminator(Terminator::Jump(BasicBlockId(1)));
        func.add_block(block1);

        let mut block2 = BasicBlock::new(BasicBlockId(1));
        block2.set_terminator(Terminator::Return(None));
        func.add_block(block2);

        let body = inliner.extract_inlinable_body(&func, FunctionId::new(0));
        assert!(body.is_none());
    }

    #[test]
    fn test_inline_simple_call() {
        let inliner = Inliner::new();

        // Create inlinable function: fn get42() { return 42; }
        let callee_instrs = vec![IrInstr::Assign {
            dest: make_reg(0),
            value: IrValue::Constant(IrConstant::I32(42)),
        }];
        let callee = make_simple_function("get42", callee_instrs, Some(make_reg(0)));

        // Create caller: fn main() { let x = get42(); }
        let caller_instrs = vec![IrInstr::Call {
            dest: Some(make_reg(0)),
            func: FunctionId::new(0),
            args: vec![],
        }];
        let caller = make_simple_function("main", caller_instrs, None);

        let mut module = IrModule::new("test");
        module.add_function(callee);
        module.add_function(caller);

        inliner.inline(&mut module);

        // The call in main should be replaced with inlined instructions
        let main = module.get_function(FunctionId::new(1)).unwrap();
        let block = main.get_block(BasicBlockId(0)).unwrap();

        // Should have 2 instructions: assign temp = 42, then assign dest = temp
        assert_eq!(block.instructions.len(), 2);
    }
}
