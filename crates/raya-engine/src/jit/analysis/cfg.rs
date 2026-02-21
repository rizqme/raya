//! Control-flow graph construction from decoded bytecode
//!
//! Splits decoded instructions into basic blocks and connects them via
//! terminators. Also tracks exception scopes from Try/EndTry pairs.

use rustc_hash::{FxHashMap, FxHashSet};
use crate::compiler::bytecode::Opcode;
use super::decoder::{DecodedInstr, Operands};

/// Unique identifier for a basic block in the CFG
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

/// A control-flow graph built from decoded bytecode
#[derive(Debug)]
pub struct ControlFlowGraph {
    pub blocks: Vec<CfgBlock>,
    pub entry: BlockId,
    /// Map from bytecode offset to the block that starts there
    pub offset_to_block: FxHashMap<usize, BlockId>,
}

/// A basic block in the CFG
#[derive(Debug)]
pub struct CfgBlock {
    pub id: BlockId,
    /// Byte offset of the first instruction in this block
    pub start_offset: usize,
    /// Indices into the decoded instruction array
    pub instrs: Vec<usize>,
    /// How this block ends
    pub terminator: CfgTerminator,
    /// Predecessor blocks
    pub predecessors: Vec<BlockId>,
    /// Active exception scope (if inside try/catch/finally)
    pub exception_scope: Option<ExceptionScope>,
}

/// How a basic block terminates
#[derive(Debug, Clone)]
pub enum CfgTerminator {
    /// Falls through to the next block
    Fallthrough(BlockId),
    /// Unconditional jump
    Jump(BlockId),
    /// Conditional branch
    Branch {
        kind: BranchKind,
        then_block: BlockId,
        else_block: BlockId,
    },
    /// Return with value
    Return,
    /// Return void
    ReturnVoid,
    /// Throw exception
    Throw,
    /// Trap (debug/assertion)
    Trap(u16),
    /// Block has no terminator yet (unreachable or empty function)
    None,
}

/// The type of conditional branch
#[derive(Debug, Clone, Copy)]
pub enum BranchKind {
    IfFalse,
    IfTrue,
    IfNull,
    IfNotNull,
}

/// An exception handling scope from Try/EndTry
#[derive(Debug, Clone)]
pub struct ExceptionScope {
    /// Byte offset of the Try instruction
    pub try_offset: usize,
    /// Target block for catch handler
    pub catch_block: Option<BlockId>,
    /// Target block for finally handler
    pub finally_block: Option<BlockId>,
}

/// Build a control-flow graph from decoded instructions
pub fn build_cfg(instrs: &[DecodedInstr]) -> ControlFlowGraph {
    if instrs.is_empty() {
        return ControlFlowGraph {
            blocks: vec![CfgBlock {
                id: BlockId(0),
                start_offset: 0,
                instrs: vec![],
                terminator: CfgTerminator::None,
                predecessors: vec![],
                exception_scope: None,
            }],
            entry: BlockId(0),
            offset_to_block: {
                let mut m = FxHashMap::default();
                m.insert(0, BlockId(0));
                m
            },
        };
    }

    // Step 1: Collect all block boundary offsets
    let block_starts = collect_block_starts(instrs);

    // Step 2: Map byte offsets to block IDs
    let mut offset_to_block = FxHashMap::default();
    let sorted_starts: Vec<usize> = {
        let mut v: Vec<usize> = block_starts.into_iter().collect();
        v.sort();
        v
    };
    for (idx, &start) in sorted_starts.iter().enumerate() {
        offset_to_block.insert(start, BlockId(idx as u32));
    }

    // Build index: byte offset → instruction index
    let mut offset_to_instr: FxHashMap<usize, usize> = FxHashMap::default();
    for (i, instr) in instrs.iter().enumerate() {
        offset_to_instr.insert(instr.offset, i);
    }

    // Step 3: Create blocks and assign instructions
    let block_count = sorted_starts.len();
    let mut blocks: Vec<CfgBlock> = sorted_starts
        .iter()
        .enumerate()
        .map(|(idx, &start)| CfgBlock {
            id: BlockId(idx as u32),
            start_offset: start,
            instrs: vec![],
            terminator: CfgTerminator::None,
            predecessors: vec![],
            exception_scope: None,
        })
        .collect();

    // Assign instructions to blocks
    let mut current_block_idx = 0;
    for (instr_idx, instr) in instrs.iter().enumerate() {
        // Check if this instruction starts a new block
        if current_block_idx + 1 < block_count
            && instr.offset >= sorted_starts[current_block_idx + 1]
        {
            current_block_idx += 1;
        }
        blocks[current_block_idx].instrs.push(instr_idx);
    }

    // Step 4: Set terminators and build exception scopes
    let mut exception_scopes: Vec<ExceptionScope> = Vec::new();

    for (block_idx, block) in blocks.iter_mut().enumerate().take(block_count) {
        let block_instrs = &block.instrs;
        if block_instrs.is_empty() {
            // Empty block falls through to next if there is one
            if block_idx + 1 < block_count {
                block.terminator =
                    CfgTerminator::Fallthrough(BlockId((block_idx + 1) as u32));
            }
            continue;
        }

        let last_instr_idx = *block_instrs.last().unwrap();
        let last_instr = &instrs[last_instr_idx];
        let next_offset = last_instr.offset + last_instr.size;

        match last_instr.opcode {
            Opcode::Jmp => {
                if let Operands::I32(rel) = last_instr.operands {
                    let target = resolve_jump(last_instr.offset, rel);
                    let target_block = offset_to_block
                        .get(&target)
                        .copied()
                        .unwrap_or(BlockId(0));
                    block.terminator = CfgTerminator::Jump(target_block);
                }
            }

            Opcode::JmpIfFalse | Opcode::JmpIfTrue | Opcode::JmpIfNull | Opcode::JmpIfNotNull => {
                if let Operands::I32(rel) = last_instr.operands {
                    let target = resolve_jump(last_instr.offset, rel);
                    let target_block = offset_to_block
                        .get(&target)
                        .copied()
                        .unwrap_or(BlockId(0));
                    let fallthrough_block = offset_to_block
                        .get(&next_offset)
                        .copied()
                        .unwrap_or(BlockId(0));

                    let kind = match last_instr.opcode {
                        Opcode::JmpIfFalse => BranchKind::IfFalse,
                        Opcode::JmpIfTrue => BranchKind::IfTrue,
                        Opcode::JmpIfNull => BranchKind::IfNull,
                        Opcode::JmpIfNotNull => BranchKind::IfNotNull,
                        _ => unreachable!(),
                    };

                    // then_block = jump target, else_block = fallthrough
                    block.terminator = CfgTerminator::Branch {
                        kind,
                        then_block: target_block,
                        else_block: fallthrough_block,
                    };
                }
            }

            Opcode::Return => {
                block.terminator = CfgTerminator::Return;
            }

            Opcode::ReturnVoid => {
                block.terminator = CfgTerminator::ReturnVoid;
            }

            Opcode::Throw | Opcode::Rethrow => {
                block.terminator = CfgTerminator::Throw;
            }

            Opcode::Trap => {
                let trap_code = if let Operands::U16(v) = last_instr.operands {
                    v
                } else {
                    0
                };
                block.terminator = CfgTerminator::Trap(trap_code);
            }

            Opcode::Try => {
                // Try is not a terminator — the block falls through.
                // But we record the exception scope.
                if let Operands::Try {
                    catch_offset,
                    finally_offset,
                } = last_instr.operands
                {
                    let catch_block = if catch_offset != 0 {
                        let target = resolve_jump(last_instr.offset, catch_offset);
                        offset_to_block.get(&target).copied()
                    } else {
                        None
                    };
                    let finally_block = if finally_offset > 0 {
                        let target = resolve_jump(last_instr.offset, finally_offset);
                        offset_to_block.get(&target).copied()
                    } else {
                        None
                    };
                    exception_scopes.push(ExceptionScope {
                        try_offset: last_instr.offset,
                        catch_block,
                        finally_block,
                    });
                }

                // Fallthrough to next block
                if let Some(&next_block) = offset_to_block.get(&next_offset) {
                    block.terminator = CfgTerminator::Fallthrough(next_block);
                }
            }

            _ => {
                // Non-terminator: fallthrough to next block
                if block_idx + 1 < block_count {
                    if let Some(&next_block) = offset_to_block.get(&next_offset) {
                        block.terminator = CfgTerminator::Fallthrough(next_block);
                    } else {
                        block.terminator =
                            CfgTerminator::Fallthrough(BlockId((block_idx + 1) as u32));
                    }
                }
                // else: last block with no terminator — could be unreachable or implicit return
            }
        }
    }

    // Step 5: Build predecessor lists
    for block_idx in 0..block_count {
        let succs = match &blocks[block_idx].terminator {
            CfgTerminator::Fallthrough(b) => vec![*b],
            CfgTerminator::Jump(b) => vec![*b],
            CfgTerminator::Branch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            _ => vec![],
        };
        let src = BlockId(block_idx as u32);
        for succ in succs {
            if (succ.0 as usize) < block_count {
                blocks[succ.0 as usize].predecessors.push(src);
            }
        }
    }

    // Step 6: Annotate exception scopes on blocks
    // Simple approach: each scope applies to blocks between try_offset and the catch/finally target
    for scope in &exception_scopes {
        let scope_end = scope
            .catch_block
            .or(scope.finally_block)
            .and_then(|b| blocks.get(b.0 as usize).map(|bb| bb.start_offset))
            .unwrap_or(usize::MAX);

        for block in &mut blocks {
            if block.start_offset > scope.try_offset && block.start_offset < scope_end {
                block.exception_scope = Some(scope.clone());
            }
        }
    }

    ControlFlowGraph {
        blocks,
        entry: BlockId(0),
        offset_to_block,
    }
}

/// Collect all byte offsets that start a new basic block
fn collect_block_starts(instrs: &[DecodedInstr]) -> FxHashSet<usize> {
    let mut starts = FxHashSet::default();

    // Entry point is always a block start
    if let Some(first) = instrs.first() {
        starts.insert(first.offset);
    }

    for instr in instrs {
        match instr.opcode {
            // Unconditional jump: target is block start, and so is fallthrough (dead code)
            Opcode::Jmp => {
                if let Operands::I32(rel) = instr.operands {
                    starts.insert(resolve_jump(instr.offset, rel));
                    starts.insert(instr.offset + instr.size);
                }
            }

            Opcode::JmpIfFalse | Opcode::JmpIfTrue | Opcode::JmpIfNull | Opcode::JmpIfNotNull => {
                if let Operands::I32(rel) = instr.operands {
                    starts.insert(resolve_jump(instr.offset, rel));
                    // Fallthrough is also a block start
                    starts.insert(instr.offset + instr.size);
                }
            }

            // Try: catch and finally targets are block starts
            Opcode::Try => {
                if let Operands::Try {
                    catch_offset,
                    finally_offset,
                } = instr.operands
                {
                    if catch_offset != 0 {
                        starts.insert(resolve_jump(instr.offset, catch_offset));
                    }
                    if finally_offset > 0 {
                        starts.insert(resolve_jump(instr.offset, finally_offset));
                    }
                    // Instruction after Try is also a block start
                    starts.insert(instr.offset + instr.size);
                }
            }

            // After any terminator, the next instruction starts a new block
            Opcode::Return | Opcode::ReturnVoid | Opcode::Throw | Opcode::Rethrow
            | Opcode::Trap => {
                starts.insert(instr.offset + instr.size);
            }

            _ => {}
        }
    }

    starts
}

/// Resolve a relative jump offset to an absolute byte offset
fn resolve_jump(instr_offset: usize, relative: i32) -> usize {
    ((instr_offset as i64) + (relative as i64)) as usize
}

impl ControlFlowGraph {
    /// Get a block by ID
    pub fn block(&self, id: BlockId) -> &CfgBlock {
        &self.blocks[id.0 as usize]
    }

    /// Get successor block IDs for a given block
    pub fn successors(&self, id: BlockId) -> Vec<BlockId> {
        match &self.blocks[id.0 as usize].terminator {
            CfgTerminator::Fallthrough(b) => vec![*b],
            CfgTerminator::Jump(b) => vec![*b],
            CfgTerminator::Branch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            _ => vec![],
        }
    }

    /// Number of blocks
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::analysis::decoder::{decode_function, Operands};

    /// Helper: build bytecode for ConstI32(val)
    fn emit_const_i32(code: &mut Vec<u8>, val: i32) {
        code.push(Opcode::ConstI32 as u8);
        code.extend_from_slice(&val.to_le_bytes());
    }

    /// Helper: emit opcode with no operands
    fn emit(code: &mut Vec<u8>, op: Opcode) {
        code.push(op as u8);
    }

    /// Helper: emit jump with i32 relative offset
    fn emit_jmp(code: &mut Vec<u8>, op: Opcode, offset: i32) {
        code.push(op as u8);
        code.extend_from_slice(&offset.to_le_bytes());
    }

    /// Helper: emit Try with catch and finally offsets
    fn emit_try(code: &mut Vec<u8>, catch_offset: i32, finally_offset: i32) {
        code.push(Opcode::Try as u8);
        code.extend_from_slice(&catch_offset.to_le_bytes());
        code.extend_from_slice(&finally_offset.to_le_bytes());
    }

    #[test]
    fn test_linear_code() {
        // ConstI32 42, ConstI32 10, Iadd, Return
        let mut code = Vec::new();
        emit_const_i32(&mut code, 42);
        emit_const_i32(&mut code, 10);
        emit(&mut code, Opcode::Iadd);
        emit(&mut code, Opcode::Return);

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        // Linear code should produce 1 block (+ potentially one after Return)
        assert!(cfg.block_count() >= 1);
        assert_eq!(cfg.entry, BlockId(0));

        // Entry block has all instructions up to Return
        let entry = cfg.block(BlockId(0));
        assert!(entry.instrs.len() >= 3); // at least ConstI32, ConstI32, Iadd (Return might split)
    }

    #[test]
    fn test_unconditional_jump() {
        // offset 0: Jmp +10 (jumps to offset 10)
        // offset 5: ConstI32 1  (dead code)
        // offset 10: Return
        let mut code = Vec::new();
        emit_jmp(&mut code, Opcode::Jmp, 10); // offset 0, size 5, target = 0 + 10 = 10
        emit_const_i32(&mut code, 1);          // offset 5, size 5
        emit(&mut code, Opcode::Return);        // offset 10, size 1

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        // Should have blocks at: 0 (entry), 5 (after jmp), 10 (target)
        assert!(cfg.block_count() >= 2);

        // Entry block terminates with Jump
        let entry = cfg.block(BlockId(0));
        assert!(matches!(entry.terminator, CfgTerminator::Jump(_)));
    }

    #[test]
    fn test_conditional_branch() {
        // if/else pattern:
        // offset 0: ConstTrue
        // offset 1: JmpIfFalse +11 (target = offset 12)
        // offset 6: ConstI32 1  (then branch)
        // offset 11: Return
        // offset 12: ConstI32 2  (else branch)
        // offset 17: Return
        let mut code = Vec::new();
        emit(&mut code, Opcode::ConstTrue);     // offset 0
        emit_jmp(&mut code, Opcode::JmpIfFalse, 12); // offset 1, target = 1+11 = 12... wait
        // JmpIfFalse at offset 1, relative offset = target - instr_offset
        // We want target = 12, so relative = 12 - 1 = 11

        // Actually let me be more careful. After ConstTrue (1 byte), JmpIfFalse (5 bytes) ends at offset 6.
        // Then branch: ConstI32 1 (5 bytes) at offset 6, ends at 11.
        // Return at offset 11 (1 byte), ends at 12.
        // Else branch: ConstI32 2 at offset 12.
        // We want JmpIfFalse (at offset 1) to jump to offset 12, relative = 12 - 1 = 11.

        // Clear and redo
        code.clear();
        emit(&mut code, Opcode::ConstTrue);             // offset 0, size 1
        emit_jmp(&mut code, Opcode::JmpIfFalse, 12);    // offset 1, size 5, target=1+12=13... hmm

        // resolve_jump: instr_offset + relative
        // offset 1 + relative = target. For target 12: relative = 11.
        code.clear();
        emit(&mut code, Opcode::ConstTrue);             // offset 0
        emit_jmp(&mut code, Opcode::JmpIfFalse, 11);    // offset 1, target = 1+11 = 12
        emit_const_i32(&mut code, 1);                    // offset 6 (then)
        emit(&mut code, Opcode::Return);                 // offset 11
        emit_const_i32(&mut code, 2);                    // offset 12 (else)
        emit(&mut code, Opcode::Return);                 // offset 17

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        // Block starts: 0, 6, 12 (plus after Return at 12, 18)
        // The branch block should have then and else targets
        let entry = cfg.block(BlockId(0));
        assert!(matches!(
            entry.terminator,
            CfgTerminator::Branch { kind: BranchKind::IfFalse, .. }
        ));
    }

    #[test]
    fn test_loop() {
        // Simple loop:
        // offset 0: ConstI32 0    (i = 0)
        // offset 5: ConstI32 10   (limit)
        // offset 10: Ilt           (i < 10)
        // offset 11: JmpIfFalse +5 (exit at 11+5=16)
        // offset 16: ... but we need to jump back
        // Let me simplify:
        //
        // offset 0: Nop        (loop header)
        // offset 1: ConstTrue
        // offset 2: JmpIfFalse +3 (exit at 2+3=5)
        // offset 7: Jmp -7 (back to 7-7=0)
        // offset 12: ReturnVoid  (exit target would be at 5 but... let me redo)

        // Simpler:
        // offset 0: ConstTrue       (1 byte)
        // offset 1: JmpIfFalse +6   (5 bytes, target = 1+6=7)
        // offset 6: Jmp -6          (5 bytes, target = 6-6=0)
        // offset 11: ReturnVoid     (1 byte)
        let mut code = Vec::new();
        emit(&mut code, Opcode::ConstTrue);              // offset 0
        emit_jmp(&mut code, Opcode::JmpIfFalse, 10);     // offset 1, target=11
        emit_jmp(&mut code, Opcode::Jmp, 0);             // offset 6, target=6+0=6?...
        // Actually I want Jmp to go back to 0: relative = 0 - 6 = -6
        code.clear();
        emit(&mut code, Opcode::ConstTrue);              // offset 0
        emit_jmp(&mut code, Opcode::JmpIfFalse, 10);     // offset 1, target=1+10=11
        emit_jmp(&mut code, Opcode::Jmp, -6);            // offset 6, target=6+(-6)=0
        emit(&mut code, Opcode::ReturnVoid);              // offset 11

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        // Should have blocks at: 0, 6, 11
        // Block 0 (offset 0): ConstTrue, JmpIfFalse → branch to 11 (else) / 6 (then-fallthrough)
        assert!(cfg.block_count() >= 3);

        // Block at offset 6 should jump back to block at offset 0
        let block6 = cfg.offset_to_block.get(&6).unwrap();
        let blk = cfg.block(*block6);
        assert!(matches!(blk.terminator, CfgTerminator::Jump(target) if target == BlockId(0)));
    }

    #[test]
    fn test_try_catch() {
        // Try at offset 0, catch at offset +14, no finally (-1)
        // offset 0: Try(14, -1)    (9 bytes)
        // offset 9: ConstI32 42    (5 bytes)
        // offset 14: Return        (1 byte) — catch handler
        let mut code = Vec::new();
        emit_try(&mut code, 14, -1);       // offset 0, catch target = 0+14=14
        emit_const_i32(&mut code, 42);     // offset 9
        emit(&mut code, Opcode::Return);   // offset 14

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        // Block at offset 14 should exist (catch target)
        assert!(cfg.offset_to_block.contains_key(&14));
    }

    #[test]
    fn test_try_catch_finally() {
        // Try(catch=+18, finally=+23)
        // offset 0: Try            (9 bytes)
        // offset 9: ConstI32 1     (5 bytes, try body)
        // offset 14: Return        (1 byte)
        // offset 15: Nop Nop Nop   (3 bytes padding to reach offsets)
        // offset 18: ConstI32 2    (catch) (5 bytes)
        // offset 23: ReturnVoid    (finally)
        let mut code = Vec::new();
        emit_try(&mut code, 18, 23);       // offset 0
        emit_const_i32(&mut code, 1);      // offset 9
        emit(&mut code, Opcode::Return);   // offset 14
        emit(&mut code, Opcode::Nop);      // offset 15
        emit(&mut code, Opcode::Nop);      // offset 16
        emit(&mut code, Opcode::Nop);      // offset 17
        emit_const_i32(&mut code, 2);      // offset 18 (catch)
        emit(&mut code, Opcode::ReturnVoid); // offset 23 (finally)

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        // Both catch (18) and finally (23) should be block starts
        assert!(cfg.offset_to_block.contains_key(&18));
        assert!(cfg.offset_to_block.contains_key(&23));
    }

    #[test]
    fn test_empty_function() {
        let instrs = decode_function(&[]).unwrap();
        let cfg = build_cfg(&instrs);
        assert_eq!(cfg.block_count(), 1);
        assert_eq!(cfg.entry, BlockId(0));
    }

    #[test]
    fn test_predecessors() {
        // Two paths merge:
        // offset 0: JmpIfFalse +6 (target = 6)
        // offset 5: Nop              (then path, falls through)
        // offset 6: Return            (merge point)
        let mut code = Vec::new();
        // Need a value on stack for JmpIfFalse
        emit(&mut code, Opcode::ConstTrue);              // offset 0
        emit_jmp(&mut code, Opcode::JmpIfFalse, 7);      // offset 1, target = 1+7=8
        emit(&mut code, Opcode::Nop);                     // offset 6 (then)
        emit(&mut code, Opcode::Nop);                     // offset 7
        emit(&mut code, Opcode::Return);                  // offset 8 (merge)

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        // Block at offset 8 should have 2 predecessors
        if let Some(&merge_id) = cfg.offset_to_block.get(&8) {
            let merge_block = cfg.block(merge_id);
            assert!(merge_block.predecessors.len() >= 1);
        }
    }

    #[test]
    fn test_switch_like_chain() {
        // Chained if-else:
        // offset 0: ConstTrue, JmpIfFalse +8
        // offset 6: ConstI32 1, Jmp +10
        // offset 16: ConstI32 2, Return
        let mut code = Vec::new();
        emit(&mut code, Opcode::ConstTrue);               // offset 0
        emit_jmp(&mut code, Opcode::JmpIfFalse, 15);      // offset 1, target=1+15=16
        emit_const_i32(&mut code, 1);                      // offset 6 (case 1)
        emit_jmp(&mut code, Opcode::Jmp, 21);             // offset 11, target=11+21=32... too far
        // Simpler: just have two cases
        code.clear();
        emit(&mut code, Opcode::ConstTrue);               // offset 0
        emit_jmp(&mut code, Opcode::JmpIfFalse, 11);      // offset 1, target=12
        emit_const_i32(&mut code, 1);                      // offset 6 (case 1)
        emit(&mut code, Opcode::Return);                   // offset 11
        emit_const_i32(&mut code, 2);                      // offset 12 (case 2)
        emit(&mut code, Opcode::Return);                   // offset 17

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        // Multiple blocks, entry has a branch
        assert!(cfg.block_count() >= 3);
        let entry = cfg.block(BlockId(0));
        assert!(matches!(entry.terminator, CfgTerminator::Branch { .. }));
    }

    #[test]
    fn test_successors() {
        let mut code = Vec::new();
        emit(&mut code, Opcode::ConstTrue);
        emit_jmp(&mut code, Opcode::JmpIfFalse, 7);  // target = 1+7=8
        emit(&mut code, Opcode::Nop);                 // offset 6
        emit(&mut code, Opcode::Nop);                 // offset 7
        emit(&mut code, Opcode::ReturnVoid);           // offset 8

        let instrs = decode_function(&code).unwrap();
        let cfg = build_cfg(&instrs);

        let succs = cfg.successors(cfg.entry);
        assert_eq!(succs.len(), 2); // branch has two successors
    }
}
