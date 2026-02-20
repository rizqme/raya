//! IR Functions
//!
//! Functions in the IR contain parameters, local variables, and basic blocks.

use super::block::{BasicBlock, BasicBlockId};
use super::value::Register;
use crate::parser::token::Span;
use crate::parser::TypeId;
use rustc_hash::FxHashMap;

/// An IR function
#[derive(Debug, Clone)]
pub struct IrFunction {
    /// Function name
    pub name: String,
    /// Parameter registers (with types)
    pub params: Vec<Register>,
    /// Return type
    pub return_ty: TypeId,
    /// Local variable registers (with types)
    pub locals: Vec<Register>,
    /// Basic blocks (in order)
    pub blocks: Vec<BasicBlock>,
    /// Entry block ID
    pub entry_block: BasicBlockId,
    /// Block lookup map for fast access
    block_map: FxHashMap<BasicBlockId, usize>,
    /// Source span covering the function definition (default when sourcemap disabled)
    pub source_span: Span,
}

impl IrFunction {
    /// Create a new function
    pub fn new(name: impl Into<String>, params: Vec<Register>, return_ty: TypeId) -> Self {
        Self {
            name: name.into(),
            params,
            return_ty,
            locals: Vec::new(),
            blocks: Vec::new(),
            entry_block: BasicBlockId(0),
            block_map: FxHashMap::default(),
            source_span: Span::default(),
        }
    }

    /// Add a local variable
    pub fn add_local(&mut self, local: Register) {
        self.locals.push(local);
    }

    /// Add a basic block and return its ID
    pub fn add_block(&mut self, block: BasicBlock) -> BasicBlockId {
        let id = block.id;
        let index = self.blocks.len();
        self.block_map.insert(id, index);
        self.blocks.push(block);
        id
    }

    /// Create and add a new empty block
    pub fn create_block(&mut self, id: BasicBlockId) -> BasicBlockId {
        let block = BasicBlock::new(id);
        self.add_block(block)
    }

    /// Get a block by ID
    pub fn get_block(&self, id: BasicBlockId) -> Option<&BasicBlock> {
        self.block_map.get(&id).map(|&idx| &self.blocks[idx])
    }

    /// Get a mutable block by ID
    pub fn get_block_mut(&mut self, id: BasicBlockId) -> Option<&mut BasicBlock> {
        self.block_map
            .get(&id)
            .copied()
            .map(|idx| &mut self.blocks[idx])
    }

    /// Get the entry block
    pub fn entry(&self) -> Option<&BasicBlock> {
        self.get_block(self.entry_block)
    }

    /// Get the entry block mutably
    pub fn entry_mut(&mut self) -> Option<&mut BasicBlock> {
        self.get_block_mut(self.entry_block)
    }

    /// Get all block IDs in order
    pub fn block_ids(&self) -> impl Iterator<Item = BasicBlockId> + '_ {
        self.blocks.iter().map(|b| b.id)
    }

    /// Get the number of blocks
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Get the number of parameters
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// Get the number of locals (including parameters)
    pub fn local_count(&self) -> usize {
        self.locals.len()
    }

    /// Check if this function has any blocks
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Iterate over all blocks
    pub fn blocks(&self) -> impl Iterator<Item = &BasicBlock> {
        self.blocks.iter()
    }

    /// Iterate over all blocks mutably
    pub fn blocks_mut(&mut self) -> impl Iterator<Item = &mut BasicBlock> {
        self.blocks.iter_mut()
    }

    /// Compute the total number of instructions across all blocks
    pub fn instruction_count(&self) -> usize {
        self.blocks.iter().map(|b| b.len()).sum()
    }

    /// Validate the function structure
    pub fn validate(&self) -> Result<(), String> {
        if self.blocks.is_empty() {
            return Err("Function has no blocks".to_string());
        }

        // Check that entry block exists
        if self.get_block(self.entry_block).is_none() {
            return Err(format!(
                "Entry block {} does not exist",
                self.entry_block
            ));
        }

        // Check that all blocks are properly terminated
        for block in &self.blocks {
            if !block.is_terminated() {
                return Err(format!("Block {} is not terminated", block.id));
            }

            // Check that all successor blocks exist
            for succ in block.successors() {
                if self.get_block(succ).is_none() {
                    return Err(format!(
                        "Block {} references non-existent successor {}",
                        block.id, succ
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::block::Terminator;
    use crate::compiler::ir::value::RegisterId;

    fn make_reg(id: u32, ty_id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(ty_id))
    }

    #[test]
    fn test_function_new() {
        let func = IrFunction::new("test", vec![], TypeId::new(0));
        assert_eq!(func.name, "test");
        assert!(func.params.is_empty());
        assert!(func.blocks.is_empty());
    }

    #[test]
    fn test_function_add_block() {
        let mut func = IrFunction::new("test", vec![], TypeId::new(0));
        let mut block = BasicBlock::new(BasicBlockId(0));
        block.set_terminator(Terminator::Return(None));
        func.add_block(block);

        assert_eq!(func.block_count(), 1);
        assert!(func.get_block(BasicBlockId(0)).is_some());
    }

    #[test]
    fn test_function_with_params() {
        let params = vec![make_reg(0, 1), make_reg(1, 1)];
        let func = IrFunction::new("add", params, TypeId::new(1));

        assert_eq!(func.param_count(), 2);
    }

    #[test]
    fn test_function_validate() {
        let mut func = IrFunction::new("test", vec![], TypeId::new(0));

        // Empty function is invalid
        assert!(func.validate().is_err());

        // Add a properly terminated block
        let mut block = BasicBlock::new(BasicBlockId(0));
        block.set_terminator(Terminator::Return(None));
        func.add_block(block);

        // Now it should be valid
        assert!(func.validate().is_ok());
    }

    #[test]
    fn test_function_validate_missing_successor() {
        let mut func = IrFunction::new("test", vec![], TypeId::new(0));

        let mut block = BasicBlock::new(BasicBlockId(0));
        // Jump to non-existent block
        block.set_terminator(Terminator::Jump(BasicBlockId(999)));
        func.add_block(block);

        assert!(func.validate().is_err());
    }
}
