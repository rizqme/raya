#![allow(missing_docs)]
//! State machine transform
//!
//! Transforms a function with suspension points into a resumable state machine.
//! For each function:
//!
//! 1. Number suspension points (0 = entry, 1..N = continuations)
//! 2. Compute live variables at each suspension point (liveness analysis)
//! 3. Generate:
//!    - Dispatch block (br_table on `resume_point`)
//!    - Child re-entry block (for AotCall suspensions)
//!    - For each suspension point: save-to-frame + resume-from-frame blocks
//! 4. Output: transformed function with explicit frame loads/stores and state dispatch

use super::analysis::SuspensionAnalysis;

/// A transformed function ready for Cranelift lowering.
///
/// Contains the original function logic plus the state machine scaffolding:
/// dispatch block, frame save/restore, child re-entry.
#[derive(Debug)]
pub struct StateMachineFunction {
    /// Original function identifier.
    pub function_id: u32,

    /// Number of locals needed in the AotFrame.
    pub local_count: u32,

    /// Number of parameters.
    pub param_count: u32,

    /// Function name (for debug info).
    pub name: Option<String>,

    /// The suspension analysis that drove this transform.
    pub analysis: SuspensionAnalysis,

    /// State machine blocks, in order.
    pub blocks: Vec<SmBlock>,
}

/// A basic block in the state machine.
#[derive(Debug, Clone)]
pub struct SmBlock {
    /// Block identifier.
    pub id: SmBlockId,

    /// What kind of block this is.
    pub kind: SmBlockKind,

    /// Instructions in this block.
    pub instructions: Vec<SmInstr>,

    /// How this block terminates.
    pub terminator: SmTerminator,
}

/// Block identifier in the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmBlockId(pub u32);

/// Classification of state machine blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmBlockKind {
    /// The dispatch block: switches on `frame.resume_point`.
    Dispatch,

    /// Re-entry block: re-enters a suspended child frame.
    ChildReentry,

    /// Block that propagates a child's suspend upward.
    PropagateSuspend,

    /// Original function logic (possibly split at suspension points).
    Body,

    /// Save state to frame before suspending.
    SaveState {
        /// Which suspension point this save is for.
        suspension_index: u32,
    },

    /// Restore state from frame after resuming.
    RestoreState {
        /// Which suspension point this restore is for.
        suspension_index: u32,
    },
}

// =============================================================================
// Instruction set
// =============================================================================

/// A state machine instruction.
///
/// These are higher-level than Cranelift IR — they'll be lowered to Cranelift
/// instructions in the lowering pass. All operations are explicit: typed
/// arithmetic, NaN-boxing, memory access, and helper calls. There is no opaque
/// "original instruction" variant.
#[derive(Debug, Clone)]
pub enum SmInstr {
    // ===== Frame / State (state machine transform inserts these) =====

    /// Load a local from the frame: dest = frame.locals[index]
    LoadLocal { dest: u32, index: u32 },

    /// Store a value to the frame: frame.locals[index] = src
    StoreLocal { index: u32, src: u32 },

    /// Load the resume_point field from the frame.
    LoadResumePoint { dest: u32 },

    /// Store the resume_point field to the frame.
    StoreResumePoint { value: u32 },

    /// Load the child_frame pointer from the frame.
    LoadChildFrame { dest: u32 },

    /// Store the child_frame pointer to the frame.
    StoreChildFrame { src: u32 },

    /// Store the suspend_reason on the task context.
    StoreSuspendReason { reason: u32 },

    /// Store the suspend_payload on the task context.
    StoreSuspendPayload { src: u32 },

    /// Load the resume_value from the task context.
    LoadResumeValue { dest: u32 },

    // ===== Constants =====

    /// Load an i32 immediate.
    ConstI32 { dest: u32, value: i32 },

    /// Load an f64 immediate (stored as raw bits for exact representation).
    ConstF64 { dest: u32, bits: u64 },

    /// Load a boolean immediate.
    ConstBool { dest: u32, value: bool },

    /// Load null.
    ConstNull { dest: u32 },

    // ===== Typed Integer Arithmetic (unboxed i32) =====

    /// dest = left `op` right (i32)
    I32BinOp { dest: u32, op: SmI32BinOp, left: u32, right: u32 },

    /// dest = -src (i32)
    I32Neg { dest: u32, src: u32 },

    /// dest = ~src (i32 bitwise NOT)
    I32BitNot { dest: u32, src: u32 },

    // ===== Typed Float Arithmetic (unboxed f64) =====

    /// dest = left `op` right (f64)
    F64BinOp { dest: u32, op: SmF64BinOp, left: u32, right: u32 },

    /// dest = -src (f64)
    F64Neg { dest: u32, src: u32 },

    // ===== Typed Comparison (unboxed → bool) =====

    /// dest = left `op` right (i32 compare → bool)
    I32Cmp { dest: u32, op: SmCmpOp, left: u32, right: u32 },

    /// dest = left `op` right (f64 compare → bool)
    F64Cmp { dest: u32, op: SmCmpOp, left: u32, right: u32 },

    // ===== Boolean Logic =====

    /// dest = !src (boolean NOT)
    BoolNot { dest: u32, src: u32 },

    /// dest = left && right (boolean AND — not short-circuit, that uses branches)
    BoolAnd { dest: u32, left: u32, right: u32 },

    /// dest = left || right (boolean OR — not short-circuit)
    BoolOr { dest: u32, left: u32, right: u32 },

    // ===== NaN-boxing Conversion =====

    /// dest = box_i32(src)
    BoxI32 { dest: u32, src: u32 },

    /// dest = unbox_i32(src)
    UnboxI32 { dest: u32, src: u32 },

    /// dest = box_f64(src)
    BoxF64 { dest: u32, src: u32 },

    /// dest = unbox_f64(src)
    UnboxF64 { dest: u32, src: u32 },

    /// dest = box_bool(src)
    BoxBool { dest: u32, src: u32 },

    /// dest = unbox_bool(src)
    UnboxBool { dest: u32, src: u32 },

    // ===== Memory Access =====

    /// dest = globals[index]
    LoadGlobal { dest: u32, index: u32 },

    /// globals[index] = src
    StoreGlobal { index: u32, src: u32 },

    // ===== Helper Calls (runtime-assisted operations) =====

    /// Call a helper function through the AotHelperTable.
    /// Used for all operations that need runtime support: allocation,
    /// string/array/object operations, native calls, concurrency, etc.
    CallHelper {
        dest: Option<u32>,
        helper: HelperCall,
        args: Vec<u32>,
    },

    // ===== AOT Function Call (may suspend) =====

    /// Call another AOT function. The callee may suspend, in which case
    /// the state machine must check for AOT_SUSPEND and propagate.
    CallAot {
        dest: u32,
        func_id: u32,
        callee_frame: u32,
    },

    // ===== Suspension =====

    /// Check if a returned value is the AOT_SUSPEND sentinel.
    IsSuspend { dest: u32, value: u32 },

    /// Return a value from this function (or AOT_SUSPEND).
    ReturnValue { value: u32 },

    // ===== SSA =====

    /// Phi node: dest = phi([(block_a, val_a), (block_b, val_b), ...])
    Phi { dest: u32, sources: Vec<(SmBlockId, u32)> },

    /// Register-to-register copy.
    Move { dest: u32, src: u32 },
}

// =============================================================================
// Operation enums
// =============================================================================

/// Integer (i32) binary operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmI32BinOp {
    Add, Sub, Mul, Div, Mod, Pow,
    Shl, Shr, Ushr, And, Or, Xor,
}

/// Float (f64) binary operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmF64BinOp {
    Add, Sub, Mul, Div, Mod, Pow,
}

/// Comparison operators (works for both i32 and f64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmCmpOp {
    Eq, Ne, Lt, Le, Gt, Ge,
}

// =============================================================================
// Helper calls
// =============================================================================

/// Identifies which helper to call through AotHelperTable, or which
/// runtime-assisted operation the lowering should emit.
///
/// The first group maps 1:1 to `AotHelperTable` function pointers.
/// The second group maps to specific code patterns the lowering emits
/// (possibly using multiple helper table entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperCall {
    // ===== AotHelperTable entries (direct) =====
    AllocFrame,
    FreeFrame,
    SafepointPoll,
    AllocObject,
    AllocArray,
    AllocString,
    StringConcat,
    StringLen,
    ArrayLen,
    ArrayGet,
    ArraySet,
    ArrayPush,
    GenericEquals,
    GenericLessThan,
    ObjectGetField,
    ObjectSetField,
    NativeCall,
    IsNativeSuspend,
    Spawn,
    CheckPreemption,
    ThrowException,
    GetAotFuncPtr,
    LoadStringConstant,
    LoadI32Constant,
    LoadF64Constant,

    // ===== Compound operations (lowering emits specific code) =====

    /// Generic polymorphic add (box+unbox pattern based on runtime tags).
    GenericAdd,
    GenericSub,
    GenericMul,
    GenericDiv,
    GenericMod,
    GenericNeg,
    GenericNot,
    GenericNotEqual,
    GenericLessEqual,
    GenericGreater,
    GenericGreaterEqual,
    GenericConcat,
    ToString,

    // Object / Array / Element operations
    NewObject,
    ObjectLiteral,
    ArrayLiteral,
    ArrayPop,
    LoadElement,
    StoreElement,
    LoadField,
    StoreField,

    // Native/module call dispatch
    ModuleNativeCall,

    // Closures
    MakeClosure,
    LoadCaptured,
    StoreCaptured,
    CallClosure,

    // RefCells
    NewRefCell,
    LoadRefCell,
    StoreRefCell,

    // Type operations
    InstanceOf,
    Cast,
    Typeof,

    // JSON operations
    JsonLoadProperty,
    JsonStoreProperty,

    // String comparison
    StringCompare,

    // Concurrency (suspension-causing)
    AwaitTask,
    AwaitAll,
    YieldTask,
    SleepTask,
    SpawnClosure,
    NewMutex,
    MutexLock,
    MutexUnlock,
    NewChannel,
    TaskCancel,

    // Exception handling
    SetupTry,
    EndTry,
}

impl HelperCall {
    /// Whether this helper call is a suspension point.
    ///
    /// Suspension-causing helpers require the state machine transform
    /// to insert save/restore/dispatch machinery around them.
    pub fn is_suspension_point(&self) -> bool {
        matches!(
            self,
            HelperCall::AwaitTask
                | HelperCall::AwaitAll
                | HelperCall::YieldTask
                | HelperCall::SleepTask
                | HelperCall::MutexLock
                | HelperCall::NativeCall
                | HelperCall::ModuleNativeCall
        )
    }
}

// =============================================================================
// Terminator
// =============================================================================

/// Terminator for a state machine block.
#[derive(Debug, Clone)]
pub enum SmTerminator {
    /// Unconditional jump to a block.
    Jump(SmBlockId),

    /// Conditional branch: if cond != 0, goto then_block, else goto else_block.
    Branch {
        cond: u32,
        then_block: SmBlockId,
        else_block: SmBlockId,
    },

    /// Branch on null: if value is NaN-boxed null, goto null_block.
    BranchNull {
        value: u32,
        null_block: SmBlockId,
        not_null_block: SmBlockId,
    },

    /// Switch on resume_point: dispatch to the appropriate continuation.
    BrTable {
        index: u32,
        default: SmBlockId,
        targets: Vec<SmBlockId>,
    },

    /// Return a value from the function.
    Return { value: u32 },

    /// Unreachable (after throw, etc.)
    Unreachable,
}

// =============================================================================
// State machine transform
// =============================================================================

/// Transform pre-transform SM blocks into a full state machine.
///
/// Takes the original function blocks (with suspension points identified)
/// and wraps them with the dispatch/save/restore machinery.
///
/// The generated structure:
/// ```text
/// [Dispatch]  ─── resume_point=0 ──→ [Entry body]
///     │
///     ├── resume_point=1 ──→ [Restore_1] → [Continuation_1]
///     ├── resume_point=2 ──→ [Restore_2] → [Continuation_2]
///     └── ...
///
/// Each suspension point splits a body block into:
///   [Pre-suspend body] → [Save_N] → return AOT_SUSPEND
///                                     [Restore_N] → [Post-suspend body]
/// ```
pub fn transform_to_state_machine(
    function_id: u32,
    blocks: Vec<SmBlock>,
    analysis: SuspensionAnalysis,
    param_count: u32,
    local_count: u32,
    name: Option<String>,
) -> StateMachineFunction {
    if !analysis.has_suspensions {
        // No suspension points — return blocks as-is (no state machine needed).
        return StateMachineFunction {
            function_id,
            local_count,
            param_count,
            name,
            analysis,
            blocks,
        };
    }

    let mut transformer = StateMachineTransformer::new(
        function_id, local_count, param_count, &analysis, blocks,
    );
    let sm_blocks = transformer.transform();

    StateMachineFunction {
        function_id,
        local_count,
        param_count,
        name,
        analysis,
        blocks: sm_blocks,
    }
}

/// Internal state for the state machine transformation.
struct StateMachineTransformer<'a> {
    _function_id: u32,
    _local_count: u32,
    #[allow(dead_code)]
    param_count: u32,
    analysis: &'a SuspensionAnalysis,
    input_blocks: Vec<SmBlock>,
    output_blocks: Vec<SmBlock>,
    next_block_id: u32,
    /// Temporary register allocator (starts after local_count * 2 to avoid collisions).
    next_temp_reg: u32,
}

impl<'a> StateMachineTransformer<'a> {
    fn new(
        function_id: u32,
        local_count: u32,
        param_count: u32,
        analysis: &'a SuspensionAnalysis,
        blocks: Vec<SmBlock>,
    ) -> Self {
        // Find max block ID from input blocks
        let max_block = blocks.iter().map(|b| b.id.0).max().unwrap_or(0);

        Self {
            _function_id: function_id,
            _local_count: local_count,
            param_count,
            analysis,
            input_blocks: blocks,
            output_blocks: Vec::new(),
            next_block_id: max_block + 1,
            next_temp_reg: (local_count + 1) * 2 + 100,
        }
    }

    fn alloc_block_id(&mut self) -> SmBlockId {
        let id = SmBlockId(self.next_block_id);
        self.next_block_id += 1;
        id
    }

    fn alloc_temp(&mut self) -> u32 {
        let r = self.next_temp_reg;
        self.next_temp_reg += 1;
        r
    }

    fn transform(&mut self) -> Vec<SmBlock> {
        // 1. Build suspension point index: (block_id, instr_index) → suspension_index
        let mut suspend_map: std::collections::HashMap<(u32, u32), usize> = std::collections::HashMap::new();
        for (idx, point) in self.analysis.points.iter().enumerate() {
            suspend_map.insert((point.block_id, point.instr_index), idx);
        }

        // 2. Allocate restore block IDs for each suspension point
        let restore_block_ids: Vec<SmBlockId> = (0..self.analysis.points.len())
            .map(|_| self.alloc_block_id())
            .collect();

        // 3. Create dispatch block
        let dispatch_id = self.alloc_block_id();
        let resume_point_reg = self.alloc_temp();
        let entry_block_id = if self.input_blocks.is_empty() {
            SmBlockId(0)
        } else {
            self.input_blocks[0].id
        };

        let mut dispatch_targets = vec![entry_block_id]; // resume_point=0 → entry
        for restore_id in &restore_block_ids {
            dispatch_targets.push(*restore_id);
        }

        let dispatch_block = SmBlock {
            id: dispatch_id,
            kind: SmBlockKind::Dispatch,
            instructions: vec![
                SmInstr::LoadResumePoint { dest: resume_point_reg },
            ],
            terminator: SmTerminator::BrTable {
                index: resume_point_reg,
                default: SmBlockId(u32::MAX), // unreachable
                targets: dispatch_targets,
            },
        };
        self.output_blocks.push(dispatch_block);

        // 4. Process each input block: split at suspension points
        let input_blocks = std::mem::take(&mut self.input_blocks);
        for block in &input_blocks {
            // Find suspension points within this block
            let block_suspensions: Vec<(u32, usize)> = suspend_map.iter()
                .filter(|((bid, _), _)| *bid == block.id.0)
                .map(|((_, iidx), &sidx)| (*iidx, sidx))
                .collect::<Vec<_>>();

            if block_suspensions.is_empty() {
                // No suspension in this block — pass through unchanged
                self.output_blocks.push(SmBlock {
                    id: block.id,
                    kind: block.kind,
                    instructions: block.instructions.clone(),
                    terminator: block.terminator.clone(),
                });
            } else {
                // Split block at each suspension point
                self.split_block_at_suspensions(block, &block_suspensions, &restore_block_ids);
            }
        }

        // 5. Create restore blocks for each suspension point
        for (idx, point) in self.analysis.points.iter().enumerate() {
            let restore_id = restore_block_ids[idx];

            // Find the continuation block for this suspension point.
            // The continuation is the block that was split after the suspension.
            let continuation_id = SmBlockId(point.block_id * 1000 + point.instr_index + 1);

            let mut restore_instrs = Vec::new();

            // Load live locals from frame
            for &local_idx in &point.live_locals {
                restore_instrs.push(SmInstr::LoadLocal {
                    dest: local_idx,
                    index: local_idx,
                });
            }

            // Load resume value if this suspension produces a result
            if matches!(point.kind, super::analysis::SuspensionKind::Await) {
                let resume_val = self.alloc_temp();
                restore_instrs.push(SmInstr::LoadResumeValue { dest: resume_val });
            }

            self.output_blocks.push(SmBlock {
                id: restore_id,
                kind: SmBlockKind::RestoreState { suspension_index: idx as u32 },
                instructions: restore_instrs,
                terminator: SmTerminator::Jump(continuation_id),
            });
        }

        std::mem::take(&mut self.output_blocks)
    }

    /// Split a block at its suspension points.
    ///
    /// For a block with suspension at instruction index `i`:
    /// - Emit instructions [0..i) as the pre-suspend body
    /// - Emit save block: store live vars, set resume_point, return SUSPEND
    /// - Emit continuation block with instructions [i+1..end) and original terminator
    fn split_block_at_suspensions(
        &mut self,
        block: &SmBlock,
        suspensions: &[(u32, usize)],
        _restore_block_ids: &[SmBlockId],
    ) {
        // Sort by instruction index
        let mut sorted = suspensions.to_vec();
        sorted.sort_by_key(|(iidx, _)| *iidx);

        let mut current_start = 0u32;
        let mut current_block_id = block.id;

        for (i, &(instr_idx, suspend_idx)) in sorted.iter().enumerate() {
            let _is_last = i == sorted.len() - 1;

            // Create pre-suspend block with instructions up to the suspension point
            let save_block_id = self.alloc_block_id();
            let continuation_id = SmBlockId(block.id.0 * 1000 + instr_idx + 1);

            let pre_instrs: Vec<SmInstr> = block.instructions[current_start as usize..instr_idx as usize]
                .to_vec();

            // Include the suspension instruction itself in pre-suspend
            let suspend_instr = if (instr_idx as usize) < block.instructions.len() {
                Some(block.instructions[instr_idx as usize].clone())
            } else {
                None
            };

            let mut all_pre_instrs = pre_instrs;
            if let Some(si) = suspend_instr {
                all_pre_instrs.push(si);
            }

            // For CallAot suspensions, add IsSuspend check
            let point = &self.analysis.points[suspend_idx];
            if matches!(point.kind, super::analysis::SuspensionKind::AotCall) {
                let check_reg = self.alloc_temp();
                // The last instruction's dest should be the call result
                let call_result = all_pre_instrs.last().and_then(|i| match i {
                    SmInstr::CallHelper { dest: Some(d), .. } => Some(*d),
                    SmInstr::CallAot { dest, .. } => Some(*dest),
                    _ => None,
                }).unwrap_or(0);

                all_pre_instrs.push(SmInstr::IsSuspend { dest: check_reg, value: call_result });

                // Branch: if suspended → save, else → continuation
                self.output_blocks.push(SmBlock {
                    id: current_block_id,
                    kind: SmBlockKind::Body,
                    instructions: all_pre_instrs,
                    terminator: SmTerminator::Branch {
                        cond: check_reg,
                        then_block: save_block_id,
                        else_block: continuation_id,
                    },
                });
            } else {
                // Non-call suspension (Await, Yield, Sleep): always suspends
                self.output_blocks.push(SmBlock {
                    id: current_block_id,
                    kind: SmBlockKind::Body,
                    instructions: all_pre_instrs,
                    terminator: SmTerminator::Jump(save_block_id),
                });
            }

            // Create save block
            let mut save_instrs = Vec::new();

            // Store live locals to frame
            for &local_idx in &point.live_locals {
                save_instrs.push(SmInstr::StoreLocal {
                    index: local_idx,
                    src: local_idx,
                });
            }

            // Set resume_point = suspension_index + 1 (0 = entry)
            let resume_const = self.alloc_temp();
            save_instrs.push(SmInstr::ConstI32 { dest: resume_const, value: (suspend_idx as i32) + 1 });
            save_instrs.push(SmInstr::StoreResumePoint { value: resume_const });

            // Set suspend reason
            let reason_const = self.alloc_temp();
            let reason_value = match point.kind {
                super::analysis::SuspensionKind::Await => 1, // AwaitTask
                super::analysis::SuspensionKind::Yield => 4, // Yielded
                super::analysis::SuspensionKind::Sleep => 5, // Sleep
                super::analysis::SuspensionKind::NativeCall => 2, // IoWait
                super::analysis::SuspensionKind::AotCall => 1, // AwaitTask
                super::analysis::SuspensionKind::PreemptionCheck => 3, // Preempted
                super::analysis::SuspensionKind::MutexLock => 8, // MutexLock
                super::analysis::SuspensionKind::ChannelRecv => 6,
                super::analysis::SuspensionKind::ChannelSend => 7,
            };
            save_instrs.push(SmInstr::ConstI32 { dest: reason_const, value: reason_value });
            save_instrs.push(SmInstr::StoreSuspendReason { reason: reason_const });

            // Return AOT_SUSPEND
            let suspend_reg = self.alloc_temp();
            save_instrs.push(SmInstr::ConstF64 {
                dest: suspend_reg,
                bits: super::frame::AOT_SUSPEND,
            });

            self.output_blocks.push(SmBlock {
                id: save_block_id,
                kind: SmBlockKind::SaveState { suspension_index: suspend_idx as u32 },
                instructions: save_instrs,
                terminator: SmTerminator::Return { value: suspend_reg },
            });

            // Update for next segment
            current_start = instr_idx + 1;
            current_block_id = continuation_id;
        }

        // Emit final continuation block (instructions after last suspension to end)
        let final_instrs: Vec<SmInstr> = block.instructions[current_start as usize..].to_vec();
        self.output_blocks.push(SmBlock {
            id: current_block_id,
            kind: SmBlockKind::Body,
            instructions: final_instrs,
            terminator: block.terminator.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sm_block_id() {
        let id = SmBlockId(42);
        assert_eq!(id.0, 42);
    }

    #[test]
    fn test_helper_call_suspension() {
        assert!(HelperCall::AwaitTask.is_suspension_point());
        assert!(HelperCall::YieldTask.is_suspension_point());
        assert!(HelperCall::SleepTask.is_suspension_point());
        assert!(HelperCall::MutexLock.is_suspension_point());
        assert!(HelperCall::NativeCall.is_suspension_point());

        assert!(!HelperCall::AllocObject.is_suspension_point());
        assert!(!HelperCall::StringConcat.is_suspension_point());
        assert!(!HelperCall::GenericAdd.is_suspension_point());
    }

    #[test]
    fn test_transform_no_suspensions() {
        let blocks = vec![SmBlock {
            id: SmBlockId(0),
            kind: SmBlockKind::Body,
            instructions: vec![
                SmInstr::ConstI32 { dest: 0, value: 42 },
                SmInstr::BoxI32 { dest: 1, src: 0 },
            ],
            terminator: SmTerminator::Return { value: 1 },
        }];

        let analysis = SuspensionAnalysis::none();
        let sm = transform_to_state_machine(0, blocks, analysis, 0, 1, None);

        assert_eq!(sm.blocks.len(), 1);
        assert_eq!(sm.function_id, 0);
        assert!(!sm.analysis.has_suspensions);
    }

    #[test]
    fn test_transform_with_await() {
        use super::super::analysis::{SuspensionPoint, SuspensionKind};
        use std::collections::HashSet;

        // Simulate a function that spawns a task and awaits it:
        //   r0 = spawn(func_1)
        //   r1 = await(r0)   ← suspension point
        //   return r1
        let blocks = vec![SmBlock {
            id: SmBlockId(0),
            kind: SmBlockKind::Body,
            instructions: vec![
                SmInstr::CallHelper {
                    dest: Some(0),
                    helper: HelperCall::Spawn,
                    args: vec![1],
                },
                SmInstr::CallHelper {
                    dest: Some(1),
                    helper: HelperCall::AwaitTask,
                    args: vec![0],
                },
                SmInstr::BoxI32 { dest: 2, src: 1 },
            ],
            terminator: SmTerminator::Return { value: 2 },
        }];

        let analysis = SuspensionAnalysis {
            points: vec![SuspensionPoint {
                index: 0,
                block_id: 0,
                instr_index: 1, // await is at index 1
                kind: SuspensionKind::Await,
                live_locals: HashSet::new(),
            }],
            has_suspensions: true,
            loop_headers: HashSet::new(),
        };

        let sm = transform_to_state_machine(0, blocks, analysis, 0, 2, Some("test_await".to_string()));

        assert!(sm.analysis.has_suspensions);
        assert_eq!(sm.name.as_deref(), Some("test_await"));

        // Should have:
        // 1. Dispatch block
        // 2. Pre-suspend body (spawn + await)
        // 3. Save block
        // 4. Continuation block (BoxI32 + return)
        // 5. Restore block
        assert!(sm.blocks.len() >= 4, "Expected at least 4 blocks, got {}", sm.blocks.len());

        // Verify dispatch block exists
        let dispatch = sm.blocks.iter().find(|b| b.kind == SmBlockKind::Dispatch);
        assert!(dispatch.is_some(), "Should have a dispatch block");

        // Verify save block exists
        let save = sm.blocks.iter().find(|b| matches!(b.kind, SmBlockKind::SaveState { .. }));
        assert!(save.is_some(), "Should have a save block");

        // Verify restore block exists
        let restore = sm.blocks.iter().find(|b| matches!(b.kind, SmBlockKind::RestoreState { .. }));
        assert!(restore.is_some(), "Should have a restore block");

        // Save block should return AOT_SUSPEND
        let save_block = save.unwrap();
        assert!(matches!(save_block.terminator, SmTerminator::Return { .. }));
    }
}
