//! JIT IR instructions, blocks, and functions
//!
//! Defines the SSA-form intermediate representation used by the JIT compiler.
//! Instructions operate on virtual registers (Reg) and are grouped into basic
//! blocks with explicit terminators.

use rustc_hash::FxHashMap;
use super::types::JitType;

/// Virtual register in the JIT IR (SSA form)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg(pub u32);

impl std::fmt::Display for Reg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// Basic block identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JitBlockId(pub u32);

impl std::fmt::Display for JitBlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

/// Mapping from a JIT register to a local variable slot (for deoptimization)
#[derive(Debug, Clone, Copy)]
pub struct LocalSlot(pub u16);

/// Why we need to deoptimize back to the interpreter
#[derive(Debug, Clone)]
pub enum DeoptReason {
    /// Unsupported opcode encountered during lifting
    UnsupportedOpcode(u8),
    /// Type assumption violated
    TypeGuardFailed,
    /// Concurrency operation (must exit to scheduler)
    ConcurrencyOp,
}

/// State needed to resume interpretation after deoptimization
#[derive(Debug, Clone)]
pub struct DeoptState {
    /// Bytecode offset to resume at
    pub bytecode_offset: usize,
    /// Map JIT registers back to local variable slots
    pub register_map: Vec<(Reg, LocalSlot)>,
}

/// A JIT IR instruction
#[derive(Debug, Clone)]
pub enum JitInstr {
    // ===== Constants =====
    ConstI32 { dest: Reg, value: i32 },
    ConstF64 { dest: Reg, value: f64 },
    ConstBool { dest: Reg, value: bool },
    ConstNull { dest: Reg },
    ConstString { dest: Reg, pool_index: u32 },

    // ===== Integer Arithmetic (unboxed i32) =====
    IAdd { dest: Reg, left: Reg, right: Reg },
    ISub { dest: Reg, left: Reg, right: Reg },
    IMul { dest: Reg, left: Reg, right: Reg },
    IDiv { dest: Reg, left: Reg, right: Reg },
    IMod { dest: Reg, left: Reg, right: Reg },
    INeg { dest: Reg, operand: Reg },
    IPow { dest: Reg, left: Reg, right: Reg },

    // ===== Integer Bitwise (unboxed i32) =====
    IShl { dest: Reg, left: Reg, right: Reg },
    IShr { dest: Reg, left: Reg, right: Reg },
    IUshr { dest: Reg, left: Reg, right: Reg },
    IAnd { dest: Reg, left: Reg, right: Reg },
    IOr { dest: Reg, left: Reg, right: Reg },
    IXor { dest: Reg, left: Reg, right: Reg },
    INot { dest: Reg, operand: Reg },

    // ===== Float Arithmetic (unboxed f64) =====
    FAdd { dest: Reg, left: Reg, right: Reg },
    FSub { dest: Reg, left: Reg, right: Reg },
    FMul { dest: Reg, left: Reg, right: Reg },
    FDiv { dest: Reg, left: Reg, right: Reg },
    FNeg { dest: Reg, operand: Reg },
    FPow { dest: Reg, left: Reg, right: Reg },
    FMod { dest: Reg, left: Reg, right: Reg },

    // ===== Integer Comparison (unboxed) =====
    ICmpEq { dest: Reg, left: Reg, right: Reg },
    ICmpNe { dest: Reg, left: Reg, right: Reg },
    ICmpLt { dest: Reg, left: Reg, right: Reg },
    ICmpLe { dest: Reg, left: Reg, right: Reg },
    ICmpGt { dest: Reg, left: Reg, right: Reg },
    ICmpGe { dest: Reg, left: Reg, right: Reg },

    // ===== Float Comparison (unboxed) =====
    FCmpEq { dest: Reg, left: Reg, right: Reg },
    FCmpNe { dest: Reg, left: Reg, right: Reg },
    FCmpLt { dest: Reg, left: Reg, right: Reg },
    FCmpLe { dest: Reg, left: Reg, right: Reg },
    FCmpGt { dest: Reg, left: Reg, right: Reg },
    FCmpGe { dest: Reg, left: Reg, right: Reg },

    // ===== String Comparison (boxed, calls runtime) =====
    SCmpEq { dest: Reg, left: Reg, right: Reg },
    SCmpNe { dest: Reg, left: Reg, right: Reg },
    SCmpLt { dest: Reg, left: Reg, right: Reg },
    SCmpLe { dest: Reg, left: Reg, right: Reg },
    SCmpGt { dest: Reg, left: Reg, right: Reg },
    SCmpGe { dest: Reg, left: Reg, right: Reg },

    // ===== Generic Comparison (boxed Value) =====
    Eq { dest: Reg, left: Reg, right: Reg },
    Ne { dest: Reg, left: Reg, right: Reg },
    StrictEq { dest: Reg, left: Reg, right: Reg },
    StrictNe { dest: Reg, left: Reg, right: Reg },

    // ===== Logical =====
    Not { dest: Reg, operand: Reg },
    And { dest: Reg, left: Reg, right: Reg },
    Or { dest: Reg, left: Reg, right: Reg },

    // ===== NaN-box Conversion (explicit in IR → optimizable) =====
    BoxI32 { dest: Reg, src: Reg },
    BoxF64 { dest: Reg, src: Reg },
    BoxBool { dest: Reg, src: Reg },
    BoxPtr { dest: Reg, src: Reg },
    UnboxI32 { dest: Reg, src: Reg },
    UnboxF64 { dest: Reg, src: Reg },
    UnboxBool { dest: Reg, src: Reg },
    UnboxPtr { dest: Reg, src: Reg },

    // ===== Local Variable Access =====
    LoadLocal { dest: Reg, index: u16 },
    StoreLocal { index: u16, value: Reg },

    // ===== Global Variable Access =====
    LoadGlobal { dest: Reg, index: u32 },
    StoreGlobal { index: u32, value: Reg },

    // ===== Static Field Access =====
    LoadStatic { dest: Reg, index: u32 },
    StoreStatic { index: u32, value: Reg },

    // ===== Object Operations =====
    NewObject { dest: Reg, class_id: u32 },
    LoadField { dest: Reg, object: Reg, offset: u16 },
    StoreField { object: Reg, offset: u16, value: Reg },
    LoadFieldFast { dest: Reg, object: Reg, offset: u16 },
    StoreFieldFast { object: Reg, offset: u16, value: Reg },
    InstanceOf { dest: Reg, object: Reg, class_id: u32 },
    Cast { dest: Reg, object: Reg, class_id: u32 },
    Typeof { dest: Reg, operand: Reg },

    // ===== Array Operations =====
    NewArray { dest: Reg, type_index: u32 },
    LoadElem { dest: Reg, array: Reg, index: Reg },
    StoreElem { array: Reg, index: Reg, value: Reg },
    ArrayLen { dest: Reg, array: Reg },
    ArrayPush { array: Reg, value: Reg },
    ArrayPop { dest: Reg, array: Reg },
    ArrayLiteral { dest: Reg, type_index: u32, elements: Vec<Reg> },
    InitArray { dest: Reg, count: u16, elements: Vec<Reg> },

    // ===== String Operations =====
    SConcat { dest: Reg, left: Reg, right: Reg },
    SLen { dest: Reg, string: Reg },
    ToString { dest: Reg, value: Reg },

    // ===== Function Calls =====
    Call { dest: Option<Reg>, func_index: u32, args: Vec<Reg> },
    CallMethod { dest: Option<Reg>, method_index: u32, receiver: Reg, args: Vec<Reg> },
    CallConstructor { dest: Reg, class_id: u32, args: Vec<Reg> },
    CallSuper { dest: Option<Reg>, method_index: u32, args: Vec<Reg> },
    CallStatic { dest: Option<Reg>, func_index: u32, args: Vec<Reg> },
    CallNative { dest: Option<Reg>, native_id: u16, args: Vec<Reg> },
    CallClosure { dest: Option<Reg>, closure: Reg, args: Vec<Reg> },

    // ===== Closures =====
    MakeClosure { dest: Reg, func_index: u32, captures: Vec<Reg> },
    LoadCaptured { dest: Reg, index: u16 },
    StoreCaptured { index: u16, value: Reg },
    SetClosureCapture { closure: Reg, index: u16, value: Reg },
    CloseVar { index: u16 },

    // ===== RefCell (closure-captured mutable variables) =====
    NewRefCell { dest: Reg, value: Reg },
    LoadRefCell { dest: Reg, cell: Reg },
    StoreRefCell { cell: Reg, value: Reg },

    // ===== Concurrency (always exit to runtime) =====
    Spawn { dest: Reg, func_index: u16, args: Vec<Reg> },
    SpawnClosure { dest: Reg, closure: Reg, args: Vec<Reg> },
    Await { dest: Reg, task: Reg },
    Yield,
    Sleep { duration: Reg },
    NewMutex { dest: Reg },
    MutexLock { mutex: Reg },
    MutexUnlock { mutex: Reg },
    NewChannel { dest: Reg },
    NewSemaphore { dest: Reg },
    SemAcquire { sem: Reg },
    SemRelease { sem: Reg },
    WaitAll { dest: Reg, tasks: Reg },
    TaskCancel { task: Reg },
    TaskThen { task: Reg, callback_index: u32 },

    // ===== Object/Tuple Literal =====
    ObjectLiteral { dest: Reg, type_index: u32, fields: Vec<Reg> },
    TupleLiteral { dest: Reg, type_index: u32, elements: Vec<Reg> },
    TupleGet { dest: Reg, tuple: Reg },
    InitObject { dest: Reg, count: u16, fields: Vec<Reg> },
    InitTuple { dest: Reg, count: u16, elements: Vec<Reg> },

    // ===== Module =====
    LoadModule { dest: Reg, module_index: u32 },
    LoadConst { dest: Reg, const_index: u32 },

    // ===== JSON Operations =====
    JsonGet { dest: Reg, object: Reg, key_index: u32 },
    JsonSet { object: Reg, key_index: u32, value: Reg },
    JsonDelete { object: Reg, key_index: u32 },
    JsonIndex { dest: Reg, object: Reg, index: Reg },
    JsonIndexSet { object: Reg, index: Reg, value: Reg },
    JsonPush { array: Reg, value: Reg },
    JsonPop { dest: Reg, array: Reg },
    JsonNewObject { dest: Reg },
    JsonNewArray { dest: Reg },
    JsonKeys { dest: Reg, object: Reg },
    JsonLength { dest: Reg, object: Reg },

    // ===== Runtime Integration =====
    GcSafepoint,
    CheckPreemption,

    // ===== SSA =====
    Phi { dest: Reg, sources: Vec<(JitBlockId, Reg)> },
    Move { dest: Reg, src: Reg },

    // ===== Exception Handling =====
    SetupTry { catch_block: JitBlockId, finally_block: Option<JitBlockId> },
    EndTry,
    Throw { value: Reg },
    Rethrow,

    // ===== Optional Field =====
    OptionalField { dest: Reg, object: Reg, offset: u16 },

    // ===== ConstStr (string from constant pool by u16 index) =====
    ConstStr { dest: Reg, str_index: u16 },
}

impl JitInstr {
    /// Get the destination register if this instruction produces a value
    pub fn dest(&self) -> Option<Reg> {
        match self {
            // Constants
            JitInstr::ConstI32 { dest, .. }
            | JitInstr::ConstF64 { dest, .. }
            | JitInstr::ConstBool { dest, .. }
            | JitInstr::ConstNull { dest }
            | JitInstr::ConstString { dest, .. }
            | JitInstr::ConstStr { dest, .. } => Some(*dest),

            // Arithmetic
            JitInstr::IAdd { dest, .. }
            | JitInstr::ISub { dest, .. }
            | JitInstr::IMul { dest, .. }
            | JitInstr::IDiv { dest, .. }
            | JitInstr::IMod { dest, .. }
            | JitInstr::INeg { dest, .. }
            | JitInstr::IPow { dest, .. }
            | JitInstr::IShl { dest, .. }
            | JitInstr::IShr { dest, .. }
            | JitInstr::IUshr { dest, .. }
            | JitInstr::IAnd { dest, .. }
            | JitInstr::IOr { dest, .. }
            | JitInstr::IXor { dest, .. }
            | JitInstr::INot { dest, .. }
            | JitInstr::FAdd { dest, .. }
            | JitInstr::FSub { dest, .. }
            | JitInstr::FMul { dest, .. }
            | JitInstr::FDiv { dest, .. }
            | JitInstr::FNeg { dest, .. }
            | JitInstr::FPow { dest, .. }
            | JitInstr::FMod { dest, .. } => Some(*dest),

            // Comparison
            JitInstr::ICmpEq { dest, .. }
            | JitInstr::ICmpNe { dest, .. }
            | JitInstr::ICmpLt { dest, .. }
            | JitInstr::ICmpLe { dest, .. }
            | JitInstr::ICmpGt { dest, .. }
            | JitInstr::ICmpGe { dest, .. }
            | JitInstr::FCmpEq { dest, .. }
            | JitInstr::FCmpNe { dest, .. }
            | JitInstr::FCmpLt { dest, .. }
            | JitInstr::FCmpLe { dest, .. }
            | JitInstr::FCmpGt { dest, .. }
            | JitInstr::FCmpGe { dest, .. }
            | JitInstr::SCmpEq { dest, .. }
            | JitInstr::SCmpNe { dest, .. }
            | JitInstr::SCmpLt { dest, .. }
            | JitInstr::SCmpLe { dest, .. }
            | JitInstr::SCmpGt { dest, .. }
            | JitInstr::SCmpGe { dest, .. } => Some(*dest),

            JitInstr::Eq { dest, .. }
            | JitInstr::Ne { dest, .. }
            | JitInstr::StrictEq { dest, .. }
            | JitInstr::StrictNe { dest, .. } => Some(*dest),

            // Logical
            JitInstr::Not { dest, .. }
            | JitInstr::And { dest, .. }
            | JitInstr::Or { dest, .. } => Some(*dest),

            // Boxing
            JitInstr::BoxI32 { dest, .. }
            | JitInstr::BoxF64 { dest, .. }
            | JitInstr::BoxBool { dest, .. }
            | JitInstr::BoxPtr { dest, .. }
            | JitInstr::UnboxI32 { dest, .. }
            | JitInstr::UnboxF64 { dest, .. }
            | JitInstr::UnboxBool { dest, .. }
            | JitInstr::UnboxPtr { dest, .. } => Some(*dest),

            // Memory
            JitInstr::LoadLocal { dest, .. }
            | JitInstr::LoadGlobal { dest, .. }
            | JitInstr::LoadStatic { dest, .. }
            | JitInstr::NewObject { dest, .. }
            | JitInstr::LoadField { dest, .. }
            | JitInstr::LoadFieldFast { dest, .. }
            | JitInstr::InstanceOf { dest, .. }
            | JitInstr::Cast { dest, .. }
            | JitInstr::Typeof { dest, .. }
            | JitInstr::NewArray { dest, .. }
            | JitInstr::LoadElem { dest, .. }
            | JitInstr::ArrayLen { dest, .. }
            | JitInstr::ArrayPop { dest, .. }
            | JitInstr::ArrayLiteral { dest, .. }
            | JitInstr::InitArray { dest, .. }
            | JitInstr::OptionalField { dest, .. } => Some(*dest),

            // String
            JitInstr::SConcat { dest, .. }
            | JitInstr::SLen { dest, .. }
            | JitInstr::ToString { dest, .. } => Some(*dest),

            // Calls
            JitInstr::Call { dest, .. }
            | JitInstr::CallMethod { dest, .. }
            | JitInstr::CallSuper { dest, .. }
            | JitInstr::CallStatic { dest, .. }
            | JitInstr::CallNative { dest, .. }
            | JitInstr::CallClosure { dest, .. } => *dest,
            JitInstr::CallConstructor { dest, .. } => Some(*dest),

            // Closures
            JitInstr::MakeClosure { dest, .. }
            | JitInstr::LoadCaptured { dest, .. } => Some(*dest),

            // RefCell
            JitInstr::NewRefCell { dest, .. }
            | JitInstr::LoadRefCell { dest, .. } => Some(*dest),

            // Concurrency
            JitInstr::Spawn { dest, .. }
            | JitInstr::SpawnClosure { dest, .. }
            | JitInstr::Await { dest, .. }
            | JitInstr::NewMutex { dest }
            | JitInstr::NewChannel { dest }
            | JitInstr::NewSemaphore { dest }
            | JitInstr::WaitAll { dest, .. } => Some(*dest),

            // Literals
            JitInstr::ObjectLiteral { dest, .. }
            | JitInstr::TupleLiteral { dest, .. }
            | JitInstr::TupleGet { dest, .. }
            | JitInstr::InitObject { dest, .. }
            | JitInstr::InitTuple { dest, .. } => Some(*dest),

            // Module
            JitInstr::LoadModule { dest, .. }
            | JitInstr::LoadConst { dest, .. } => Some(*dest),

            // JSON
            JitInstr::JsonGet { dest, .. }
            | JitInstr::JsonIndex { dest, .. }
            | JitInstr::JsonPop { dest, .. }
            | JitInstr::JsonNewObject { dest }
            | JitInstr::JsonNewArray { dest }
            | JitInstr::JsonKeys { dest, .. }
            | JitInstr::JsonLength { dest, .. } => Some(*dest),

            // SSA
            JitInstr::Phi { dest, .. }
            | JitInstr::Move { dest, .. } => Some(*dest),

            // No destination
            JitInstr::StoreLocal { .. }
            | JitInstr::StoreGlobal { .. }
            | JitInstr::StoreStatic { .. }
            | JitInstr::StoreField { .. }
            | JitInstr::StoreFieldFast { .. }
            | JitInstr::StoreElem { .. }
            | JitInstr::ArrayPush { .. }
            | JitInstr::StoreCaptured { .. }
            | JitInstr::SetClosureCapture { .. }
            | JitInstr::CloseVar { .. }
            | JitInstr::StoreRefCell { .. }
            | JitInstr::Yield
            | JitInstr::Sleep { .. }
            | JitInstr::MutexLock { .. }
            | JitInstr::MutexUnlock { .. }
            | JitInstr::SemAcquire { .. }
            | JitInstr::SemRelease { .. }
            | JitInstr::TaskCancel { .. }
            | JitInstr::TaskThen { .. }
            | JitInstr::GcSafepoint
            | JitInstr::CheckPreemption
            | JitInstr::SetupTry { .. }
            | JitInstr::EndTry
            | JitInstr::Throw { .. }
            | JitInstr::Rethrow
            | JitInstr::JsonSet { .. }
            | JitInstr::JsonDelete { .. }
            | JitInstr::JsonIndexSet { .. }
            | JitInstr::JsonPush { .. } => None,
        }
    }

    /// Whether this instruction has side effects (can't be dead-code eliminated)
    pub fn has_side_effects(&self) -> bool {
        match self {
            // Pure: constants, arithmetic, comparison, boxing, loads, phi, move
            JitInstr::ConstI32 { .. }
            | JitInstr::ConstF64 { .. }
            | JitInstr::ConstBool { .. }
            | JitInstr::ConstNull { .. }
            | JitInstr::ConstString { .. }
            | JitInstr::ConstStr { .. }
            | JitInstr::IAdd { .. } | JitInstr::ISub { .. } | JitInstr::IMul { .. }
            | JitInstr::IDiv { .. } | JitInstr::IMod { .. } | JitInstr::INeg { .. }
            | JitInstr::IPow { .. }
            | JitInstr::IShl { .. } | JitInstr::IShr { .. } | JitInstr::IUshr { .. }
            | JitInstr::IAnd { .. } | JitInstr::IOr { .. } | JitInstr::IXor { .. }
            | JitInstr::INot { .. }
            | JitInstr::FAdd { .. } | JitInstr::FSub { .. } | JitInstr::FMul { .. }
            | JitInstr::FDiv { .. } | JitInstr::FNeg { .. } | JitInstr::FPow { .. }
            | JitInstr::FMod { .. }
            | JitInstr::ICmpEq { .. } | JitInstr::ICmpNe { .. }
            | JitInstr::ICmpLt { .. } | JitInstr::ICmpLe { .. }
            | JitInstr::ICmpGt { .. } | JitInstr::ICmpGe { .. }
            | JitInstr::FCmpEq { .. } | JitInstr::FCmpNe { .. }
            | JitInstr::FCmpLt { .. } | JitInstr::FCmpLe { .. }
            | JitInstr::FCmpGt { .. } | JitInstr::FCmpGe { .. }
            | JitInstr::SCmpEq { .. } | JitInstr::SCmpNe { .. }
            | JitInstr::SCmpLt { .. } | JitInstr::SCmpLe { .. }
            | JitInstr::SCmpGt { .. } | JitInstr::SCmpGe { .. }
            | JitInstr::Eq { .. } | JitInstr::Ne { .. }
            | JitInstr::StrictEq { .. } | JitInstr::StrictNe { .. }
            | JitInstr::Not { .. } | JitInstr::And { .. } | JitInstr::Or { .. }
            | JitInstr::BoxI32 { .. } | JitInstr::BoxF64 { .. }
            | JitInstr::BoxBool { .. } | JitInstr::BoxPtr { .. }
            | JitInstr::UnboxI32 { .. } | JitInstr::UnboxF64 { .. }
            | JitInstr::UnboxBool { .. } | JitInstr::UnboxPtr { .. }
            | JitInstr::LoadLocal { .. } | JitInstr::LoadGlobal { .. }
            | JitInstr::LoadStatic { .. }
            | JitInstr::LoadField { .. } | JitInstr::LoadFieldFast { .. }
            | JitInstr::LoadElem { .. } | JitInstr::ArrayLen { .. }
            | JitInstr::LoadCaptured { .. } | JitInstr::LoadRefCell { .. }
            | JitInstr::SLen { .. }
            | JitInstr::InstanceOf { .. } | JitInstr::Typeof { .. }
            | JitInstr::OptionalField { .. }
            | JitInstr::LoadModule { .. } | JitInstr::LoadConst { .. }
            | JitInstr::TupleGet { .. }
            | JitInstr::Phi { .. } | JitInstr::Move { .. } => false,

            // Everything else has side effects
            _ => true,
        }
    }
}

/// A basic block in the JIT IR
#[derive(Debug, Clone)]
pub struct JitBlock {
    pub id: JitBlockId,
    pub instrs: Vec<JitInstr>,
    pub terminator: JitTerminator,
    pub predecessors: Vec<JitBlockId>,
}

/// How a JIT IR block terminates
#[derive(Debug, Clone)]
pub enum JitTerminator {
    /// Unconditional jump to target block
    Jump(JitBlockId),
    /// Conditional branch on a boolean register
    Branch {
        cond: Reg,
        then_block: JitBlockId,
        else_block: JitBlockId,
    },
    /// Branch on null/not-null
    BranchNull {
        value: Reg,
        null_block: JitBlockId,
        not_null_block: JitBlockId,
    },
    /// Return with a value
    Return(Option<Reg>),
    /// Throw an exception
    Throw(Reg),
    /// Unreachable code (after trap, etc.)
    Unreachable,
    /// Exit to interpreter for unsupported operations
    Deoptimize {
        reason: DeoptReason,
        state: DeoptState,
    },
    /// Placeholder terminator (not yet assigned)
    None,
}

/// A complete JIT IR function
#[derive(Debug)]
pub struct JitFunction {
    /// Index in the module's function table
    pub func_index: u32,
    /// Function name (for debugging)
    pub name: String,
    /// Number of parameters
    pub param_count: usize,
    /// Number of local variables
    pub local_count: usize,
    /// Basic blocks
    pub blocks: Vec<JitBlock>,
    /// Entry block
    pub entry: JitBlockId,
    /// Next available register number
    pub next_reg: u32,
    /// Type of each register (filled during lifting)
    pub reg_types: FxHashMap<Reg, JitType>,
}

impl JitFunction {
    /// Create a new empty function
    pub fn new(func_index: u32, name: String, param_count: usize, local_count: usize) -> Self {
        JitFunction {
            func_index,
            name,
            param_count,
            local_count,
            blocks: vec![],
            entry: JitBlockId(0),
            next_reg: 0,
            reg_types: FxHashMap::default(),
        }
    }

    /// Allocate a fresh virtual register with a given type
    pub fn alloc_reg(&mut self, ty: JitType) -> Reg {
        let reg = Reg(self.next_reg);
        self.next_reg += 1;
        self.reg_types.insert(reg, ty);
        reg
    }

    /// Get the type of a register
    pub fn reg_type(&self, reg: Reg) -> JitType {
        self.reg_types.get(&reg).copied().unwrap_or(JitType::Value)
    }

    /// Get a block by ID
    pub fn block(&self, id: JitBlockId) -> &JitBlock {
        &self.blocks[id.0 as usize]
    }

    /// Get a mutable block by ID
    pub fn block_mut(&mut self, id: JitBlockId) -> &mut JitBlock {
        &mut self.blocks[id.0 as usize]
    }

    /// Add a new block and return its ID
    pub fn add_block(&mut self) -> JitBlockId {
        let id = JitBlockId(self.blocks.len() as u32);
        self.blocks.push(JitBlock {
            id,
            instrs: vec![],
            terminator: JitTerminator::None,
            predecessors: vec![],
        });
        id
    }

    /// Total number of instructions across all blocks
    pub fn instr_count(&self) -> usize {
        self.blocks.iter().map(|b| b.instrs.len()).sum()
    }
}
