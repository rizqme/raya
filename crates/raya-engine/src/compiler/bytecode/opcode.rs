//! Bytecode opcodes for the Raya VM
//!
//! This module defines the complete instruction set for the Raya virtual machine.
//! See design/OPCODE.md for detailed documentation of each instruction.

/// Bytecode opcode enumeration
///
/// All opcodes are single-byte instructions. Some opcodes take additional operands
/// that follow the opcode byte in the bytecode stream.
///
/// Opcodes are organized into categories:
/// - 0x00-0x0F: Stack manipulation & constants
/// - 0x10-0x1F: Local variables
/// - 0x20-0x2F: Integer arithmetic
/// - 0x30-0x3F: Float arithmetic
/// - 0x40-0x4F: Number arithmetic (generic)
/// - 0x50-0x5F: Integer comparison
/// - 0x60-0x6F: Float comparison
/// - 0x70-0x7F: Generic comparison & logical
/// - 0x80-0x8F: String operations
/// - 0x90-0x9F: Control flow
/// - 0xA0-0xAF: Function calls
/// - 0xB0-0xBF: Object operations
/// - 0xC0-0xCF: Array & tuple operations
/// - 0xD0-0xDF: Concurrency & tasks (spawn, mutex, semaphore)
/// - 0xE0-0xEF: JSON operations (complete set)
/// - 0xF0-0xF7: Closures & modules
/// - 0xF8-0xFF: Exception handling & special
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    // ===== Stack Manipulation & Constants (0x00-0x0F) =====
    /// No operation
    Nop = 0x00,
    /// Pop top value from stack
    Pop = 0x01,
    /// Duplicate top stack value
    Dup = 0x02,
    /// Swap top two stack values
    Swap = 0x03,

    /// Push null constant
    ConstNull = 0x04,
    /// Push true constant
    ConstTrue = 0x05,
    /// Push false constant
    ConstFalse = 0x06,
    /// Push 32-bit integer constant (operand: i32)
    ConstI32 = 0x07,
    /// Push 64-bit float constant (operand: f64)
    ConstF64 = 0x08,
    /// Push string constant from pool (operand: u32 index)
    ConstStr = 0x09,
    /// Load constant from constant pool (operand: u32 index)
    LoadConst = 0x0A,

    // ===== Local Variables (0x10-0x1F) =====
    /// Load local variable onto stack (operand: u16 index)
    LoadLocal = 0x10,
    /// Store top of stack to local variable (operand: u16 index)
    StoreLocal = 0x11,
    /// Load local variable 0 (optimized, no operand)
    LoadLocal0 = 0x12,
    /// Load local variable 1 (optimized, no operand)
    LoadLocal1 = 0x13,
    /// Store to local variable 0 (optimized, no operand)
    StoreLocal0 = 0x14,
    /// Store to local variable 1 (optimized, no operand)
    StoreLocal1 = 0x15,

    // ===== Integer Arithmetic (0x20-0x2F) =====
    /// Integer addition: pop b, pop a, push a + b
    Iadd = 0x20,
    /// Integer subtraction: pop b, pop a, push a - b
    Isub = 0x21,
    /// Integer multiplication: pop b, pop a, push a * b
    Imul = 0x22,
    /// Integer division: pop b, pop a, push a / b
    Idiv = 0x23,
    /// Integer modulo: pop b, pop a, push a % b
    Imod = 0x24,
    /// Integer negation: pop a, push -a
    Ineg = 0x25,
    /// Integer power: pop b, pop a, push a ** b
    Ipow = 0x26,
    /// Integer shift left: pop b, pop a, push a << b
    Ishl = 0x27,
    /// Integer shift right (signed): pop b, pop a, push a >> b
    Ishr = 0x28,
    /// Integer shift right (unsigned): pop b, pop a, push a >>> b
    Iushr = 0x29,
    /// Integer bitwise AND: pop b, pop a, push a & b
    Iand = 0x2A,
    /// Integer bitwise OR: pop b, pop a, push a | b
    Ior = 0x2B,
    /// Integer bitwise XOR: pop b, pop a, push a ^ b
    Ixor = 0x2C,
    /// Integer bitwise NOT: pop a, push ~a
    Inot = 0x2D,

    // ===== Float Arithmetic (0x30-0x3F) =====
    /// Float addition: pop b, pop a, push a + b
    Fadd = 0x30,
    /// Float subtraction: pop b, pop a, push a - b
    Fsub = 0x31,
    /// Float multiplication: pop b, pop a, push a * b
    Fmul = 0x32,
    /// Float division: pop b, pop a, push a / b
    Fdiv = 0x33,
    /// Float negation: pop a, push -a
    Fneg = 0x34,
    /// Float power: pop b, pop a, push a ** b
    Fpow = 0x35,
    /// Float modulo: pop b, pop a, push a % b
    Fmod = 0x36,

    // ===== Debug (0x40-0x4F) =====
    /// Debugger breakpoint: pause execution if a debugger is attached
    Debugger = 0x40,

    // ===== Integer Comparison (0x50-0x5F) =====
    /// Integer equality: pop b, pop a, push a == b
    Ieq = 0x50,
    /// Integer inequality: pop b, pop a, push a != b
    Ine = 0x51,
    /// Integer less than: pop b, pop a, push a < b
    Ilt = 0x52,
    /// Integer less or equal: pop b, pop a, push a <= b
    Ile = 0x53,
    /// Integer greater than: pop b, pop a, push a > b
    Igt = 0x54,
    /// Integer greater or equal: pop b, pop a, push a >= b
    Ige = 0x55,

    // ===== Float Comparison (0x60-0x6F) =====
    /// Float equality: pop b, pop a, push a == b
    Feq = 0x60,
    /// Float inequality: pop b, pop a, push a != b
    Fne = 0x61,
    /// Float less than: pop b, pop a, push a < b
    Flt = 0x62,
    /// Float less or equal: pop b, pop a, push a <= b
    Fle = 0x63,
    /// Float greater than: pop b, pop a, push a > b
    Fgt = 0x64,
    /// Float greater or equal: pop b, pop a, push a >= b
    Fge = 0x65,

    // ===== Generic Comparison & Logical (0x70-0x7F) =====
    /// Generic equality: pop b, pop a, push a == b (structural)
    Eq = 0x70,
    /// Generic inequality: pop b, pop a, push a != b
    Ne = 0x71,
    /// Strict equality: pop b, pop a, push a === b
    StrictEq = 0x72,
    /// Strict inequality: pop b, pop a, push a !== b
    StrictNe = 0x73,
    /// Logical NOT: pop a, push !a
    Not = 0x74,
    /// Logical AND: pop b, pop a, push a && b
    And = 0x75,
    /// Logical OR: pop b, pop a, push a || b
    Or = 0x76,
    /// Typeof: pop a, push typeof(a) as string
    Typeof = 0x77,

    // ===== String Operations (0x80-0x8F) =====
    /// String concatenation: pop b, pop a, push a + b
    Sconcat = 0x80,
    /// String length: pop a, push a.length
    Slen = 0x81,
    /// String equality: pop b, pop a, push a == b
    Seq = 0x82,
    /// String inequality: pop b, pop a, push a != b
    Sne = 0x83,
    /// String less than: pop b, pop a, push a < b (lexicographic)
    Slt = 0x84,
    /// String less or equal: pop b, pop a, push a <= b
    Sle = 0x85,
    /// String greater than: pop b, pop a, push a > b
    Sgt = 0x86,
    /// String greater or equal: pop b, pop a, push a >= b
    Sge = 0x87,
    /// Convert value to string: pop a, push toString(a)
    ToString = 0x88,

    // ===== Control Flow (0x90-0x9F) =====
    /// Unconditional jump (operand: i32 offset)
    Jmp = 0x90,
    /// Jump if false: pop a, if !a jump (operand: i32 offset)
    JmpIfFalse = 0x91,
    /// Jump if true: pop a, if a jump (operand: i32 offset)
    JmpIfTrue = 0x92,
    /// Jump if null: pop a, if a == null jump (operand: i32 offset)
    JmpIfNull = 0x93,
    /// Jump if not null: pop a, if a != null jump (operand: i32 offset)
    JmpIfNotNull = 0x94,

    // ===== Function Calls (0xA0-0xAF) =====
    /// Call function (operands: u32 funcIndex, u16 argCount)
    Call = 0xA0,
    /// Call method on object (operands: u32 methodIndex, u16 argCount)
    CallMethod = 0xA1,
    /// Return from function (pop return value)
    Return = 0xA2,
    /// Return from void function
    ReturnVoid = 0xA3,
    /// Call constructor (operands: u32 ctorIndex, u16 argCount)
    CallConstructor = 0xA4,
    /// Call parent class constructor (operands: u32 superCtorIndex, u16 argCount)
    CallSuper = 0xA5,
    /// Call static method (operands: u32 methodIndex, u16 argCount)
    CallStatic = 0xA6,

    // ===== Object Operations (0xB0-0xBF) =====
    /// Allocate new object (operand: u32 classIndex)
    New = 0xB0,
    /// Load object field: pop object, push field (operand: u16 fieldOffset)
    LoadField = 0xB1,
    /// Store object field: pop value, pop object (operand: u16 fieldOffset)
    StoreField = 0xB2,
    /// Load field at known offset (optimized) (operand: u16 offset)
    LoadFieldFast = 0xB3,
    /// Store field at known offset (optimized) (operand: u16 offset)
    StoreFieldFast = 0xB4,
    /// Create object literal (operands: u32 typeIndex, u16 fieldCount)
    ObjectLiteral = 0xB5,
    /// Initialize object fields: pop N values (operand: u16 count)
    InitObject = 0xB6,
    /// Optional chaining field access (operand: u16 offset)
    OptionalField = 0xB7,
    /// Load static field (operand: u32 staticIndex)
    LoadStatic = 0xB8,
    /// Store static field (operand: u32 staticIndex)
    StoreStatic = 0xB9,

    // ===== Array Operations (0xC0-0xCF) =====
    /// Create new array: pop length (operand: u32 typeIndex)
    NewArray = 0xC0,
    /// Load array element: pop index, pop array, push element
    LoadElem = 0xC1,
    /// Store array element: pop value, pop index, pop array
    StoreElem = 0xC2,
    /// Get array length: pop array, push length
    ArrayLen = 0xC3,
    /// Create array literal (operands: u32 typeIndex, u32 length)
    ArrayLiteral = 0xC4,
    /// Initialize array: pop N values (operand: u16 count)
    InitArray = 0xC5,

    // ===== Tuple Operations (0xC6-0xC9) =====
    /// Create tuple literal (operands: u32 typeIndex, u16 length)
    TupleLiteral = 0xC6,
    /// Initialize tuple: pop N values (operand: u16 count)
    InitTuple = 0xC7,
    /// Get tuple element: pop index, pop tuple, push element
    TupleGet = 0xC8,
    /// Push element to array: pop value, pop array
    ArrayPush = 0xC9,
    /// Pop element from array: pop array, push popped element
    ArrayPop = 0xCA,
    /// Create new channel with capacity: pop capacity, push Channel reference
    /// Stack: [capacity] -> [channel]
    NewChannel = 0xCB,

    // ===== Concurrency & Tasks (0xD0-0xDB) =====
    /// Spawn new task (operands: u16 funcIndex, u16 argCount)
    Spawn = 0xD0,
    /// Await task completion: pop TaskHandle, push result
    Await = 0xD1,
    /// Voluntary yield to scheduler
    Yield = 0xD2,
    /// Register continuation on task (operand: u32 funcIndex)
    TaskThen = 0xD3,
    /// Create new mutex: push Mutex reference
    NewMutex = 0xD4,
    /// Acquire mutex: pop mutex (may block)
    MutexLock = 0xD5,
    /// Release mutex: pop mutex
    MutexUnlock = 0xD6,
    /// Create new semaphore: pop initial permit count, push Semaphore reference
    NewSemaphore = 0xD7,
    /// Acquire semaphore permits: pop permit count, pop semaphore (may block)
    SemAcquire = 0xD8,
    /// Release semaphore permits: pop permit count, pop semaphore
    SemRelease = 0xD9,
    /// Wait for all tasks to complete: pop task array, push result array
    /// Use for: await [task1, task2, ...] syntax
    WaitAll = 0xDA,
    /// Spawn new task from closure (operand: u16 argCount)
    /// Stack: [args..., closure] -> TaskHandle
    SpawnClosure = 0xDB,
    /// Sleep for given milliseconds: pop ms
    /// Stack: [duration_ms] -> []
    Sleep = 0xDF,

    // ===== RefCell Operations (0xDC-0xDE) =====
    /// Allocate a new RefCell with initial value from stack
    /// Stack: [value] -> [refcell_ptr]
    NewRefCell = 0xDC,
    /// Load value from RefCell
    /// Stack: [refcell_ptr] -> [value]
    LoadRefCell = 0xDD,
    /// Store value to RefCell
    /// Stack: [refcell_ptr, value] -> []
    StoreRefCell = 0xDE,

    // ===== JSON Operations (0xE0-0xEF) =====
    // Note: JsonParse and JsonStringify use NativeCall (0x0C00, 0x0C01) instead of opcodes
    /// JSON property access: pop json object, push property value (operand: u32 propertyIndex)
    JsonGet = 0xE2,
    /// Set JSON object property: pop value, pop object (operand: u32 propertyIndex)
    JsonSet = 0xE3,
    /// Delete property from JSON object: pop object (operand: u32 propertyIndex)
    JsonDelete = 0xE4,
    /// JSON array indexing: pop index, pop json array, push element
    JsonIndex = 0xE5,
    /// Set JSON array element: pop value, pop index, pop array
    JsonIndexSet = 0xE6,
    /// Push element to JSON array: pop value, pop array
    JsonPush = 0xE7,
    /// Pop element from JSON array: pop array, push popped element
    JsonPop = 0xE8,
    /// Create new empty JSON object: push json object
    JsonNewObject = 0xE9,
    /// Create new empty JSON array: push json array
    JsonNewArray = 0xEA,
    /// Get object keys as array: pop json object, push string array
    JsonKeys = 0xEB,
    /// Get object/array length: pop json, push length
    JsonLength = 0xEC,

    // ===== Task Control & Type Operations (0xED-0xEF) =====
    /// Cancel a task: pop TaskHandle, task is marked for cancellation
    TaskCancel = 0xED,
    /// Check if object is instance of class: pop class_id, pop obj, push bool
    InstanceOf = 0xEE,
    /// Cast object to type: pop class_id, pop obj, push obj (throws TypeError if invalid)
    Cast = 0xEF,

    // ===== Closures & Modules (0xF0-0xF7) =====
    /// Create closure object (operands: u32 funcIndex, u16 captureCount)
    MakeClosure = 0xF0,
    /// Capture local variable (operand: u16 localIndex)
    CloseVar = 0xF1,
    /// Load captured variable (operand: u16 index)
    LoadCaptured = 0xF2,
    /// Store to captured variable (operand: u16 index)
    StoreCaptured = 0xF3,
    /// Load module namespace object (operand: u32 moduleIndex)
    LoadModule = 0xF4,
    /// Load global variable (operand: u32 index)
    LoadGlobal = 0xF5,
    /// Store global variable (operand: u32 index)
    StoreGlobal = 0xF6,
    /// Set a closure's capture (operand: u16 captureIndex)
    /// Stack: [closure, value] -> [closure]
    /// Used for recursive closures to patch the capture after creation
    SetClosureCapture = 0xF7,

    // ===== Exception Handling & Special (0xF8-0xFF) =====
    /// Throw exception: pop error value
    Throw = 0xF8,
    /// Begin try block (operands: i32 catchOffset, i32 finallyOffset)
    /// Pushes exception handler onto handler stack
    /// catchOffset: -1 if no catch, finallyOffset: -1 if no finally
    Try = 0xF9,
    /// End try-catch-finally block (no operands)
    /// Pops exception handler from stack
    EndTry = 0xFA,
    /// Rethrow current exception (no operands)
    /// Used in catch blocks to re-raise the exception
    Rethrow = 0xFB,
    /// Trap with error code (operand: u16 errorCode)
    Trap = 0xFC,
    /// Call native function by ID (operand: u16 nativeId, u8 argCount)
    /// Stack: [args...] -> [result]
    NativeCall = 0xFD,
    /// Call module-local native function (operand: u16 localIdx, u8 argCount)
    /// The localIdx indexes into the module's native_functions table,
    /// which is resolved to a handler at load time.
    /// Stack: [args...] -> [result]
    ModuleNativeCall = 0xFE,
}

impl Opcode {
    /// Convert byte to opcode
    ///
    /// Returns None if the byte does not correspond to a valid opcode.
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            // Stack manipulation & constants
            0x00 => Some(Self::Nop),
            0x01 => Some(Self::Pop),
            0x02 => Some(Self::Dup),
            0x03 => Some(Self::Swap),
            0x04 => Some(Self::ConstNull),
            0x05 => Some(Self::ConstTrue),
            0x06 => Some(Self::ConstFalse),
            0x07 => Some(Self::ConstI32),
            0x08 => Some(Self::ConstF64),
            0x09 => Some(Self::ConstStr),
            0x0A => Some(Self::LoadConst),

            // Local variables
            0x10 => Some(Self::LoadLocal),
            0x11 => Some(Self::StoreLocal),
            0x12 => Some(Self::LoadLocal0),
            0x13 => Some(Self::LoadLocal1),
            0x14 => Some(Self::StoreLocal0),
            0x15 => Some(Self::StoreLocal1),

            // Integer arithmetic
            0x20 => Some(Self::Iadd),
            0x21 => Some(Self::Isub),
            0x22 => Some(Self::Imul),
            0x23 => Some(Self::Idiv),
            0x24 => Some(Self::Imod),
            0x25 => Some(Self::Ineg),
            0x26 => Some(Self::Ipow),
            0x27 => Some(Self::Ishl),
            0x28 => Some(Self::Ishr),
            0x29 => Some(Self::Iushr),
            0x2A => Some(Self::Iand),
            0x2B => Some(Self::Ior),
            0x2C => Some(Self::Ixor),
            0x2D => Some(Self::Inot),

            // Float arithmetic
            0x30 => Some(Self::Fadd),
            0x31 => Some(Self::Fsub),
            0x32 => Some(Self::Fmul),
            0x33 => Some(Self::Fdiv),
            0x34 => Some(Self::Fneg),
            0x35 => Some(Self::Fpow),
            0x36 => Some(Self::Fmod),

            // Debug
            0x40 => Some(Self::Debugger),

            // Integer comparison
            0x50 => Some(Self::Ieq),
            0x51 => Some(Self::Ine),
            0x52 => Some(Self::Ilt),
            0x53 => Some(Self::Ile),
            0x54 => Some(Self::Igt),
            0x55 => Some(Self::Ige),

            // Float comparison
            0x60 => Some(Self::Feq),
            0x61 => Some(Self::Fne),
            0x62 => Some(Self::Flt),
            0x63 => Some(Self::Fle),
            0x64 => Some(Self::Fgt),
            0x65 => Some(Self::Fge),

            // Generic comparison & logical
            0x70 => Some(Self::Eq),
            0x71 => Some(Self::Ne),
            0x72 => Some(Self::StrictEq),
            0x73 => Some(Self::StrictNe),
            0x74 => Some(Self::Not),
            0x75 => Some(Self::And),
            0x76 => Some(Self::Or),
            0x77 => Some(Self::Typeof),

            // String operations
            0x80 => Some(Self::Sconcat),
            0x81 => Some(Self::Slen),
            0x82 => Some(Self::Seq),
            0x83 => Some(Self::Sne),
            0x84 => Some(Self::Slt),
            0x85 => Some(Self::Sle),
            0x86 => Some(Self::Sgt),
            0x87 => Some(Self::Sge),
            0x88 => Some(Self::ToString),

            // Control flow
            0x90 => Some(Self::Jmp),
            0x91 => Some(Self::JmpIfFalse),
            0x92 => Some(Self::JmpIfTrue),
            0x93 => Some(Self::JmpIfNull),
            0x94 => Some(Self::JmpIfNotNull),

            // Function calls
            0xA0 => Some(Self::Call),
            0xA1 => Some(Self::CallMethod),
            0xA2 => Some(Self::Return),
            0xA3 => Some(Self::ReturnVoid),
            0xA4 => Some(Self::CallConstructor),
            0xA5 => Some(Self::CallSuper),
            0xA6 => Some(Self::CallStatic),

            // Object operations
            0xB0 => Some(Self::New),
            0xB1 => Some(Self::LoadField),
            0xB2 => Some(Self::StoreField),
            0xB3 => Some(Self::LoadFieldFast),
            0xB4 => Some(Self::StoreFieldFast),
            0xB5 => Some(Self::ObjectLiteral),
            0xB6 => Some(Self::InitObject),
            0xB7 => Some(Self::OptionalField),
            0xB8 => Some(Self::LoadStatic),
            0xB9 => Some(Self::StoreStatic),

            // Array operations
            0xC0 => Some(Self::NewArray),
            0xC1 => Some(Self::LoadElem),
            0xC2 => Some(Self::StoreElem),
            0xC3 => Some(Self::ArrayLen),
            0xC4 => Some(Self::ArrayLiteral),
            0xC5 => Some(Self::InitArray),

            // Tuple operations
            0xC6 => Some(Self::TupleLiteral),
            0xC7 => Some(Self::InitTuple),
            0xC8 => Some(Self::TupleGet),
            0xC9 => Some(Self::ArrayPush),
            0xCA => Some(Self::ArrayPop),
            0xCB => Some(Self::NewChannel),

            // Concurrency & tasks
            0xD0 => Some(Self::Spawn),
            0xD1 => Some(Self::Await),
            0xD2 => Some(Self::Yield),
            0xD3 => Some(Self::TaskThen),
            0xD4 => Some(Self::NewMutex),
            0xD5 => Some(Self::MutexLock),
            0xD6 => Some(Self::MutexUnlock),
            0xD7 => Some(Self::NewSemaphore),
            0xD8 => Some(Self::SemAcquire),
            0xD9 => Some(Self::SemRelease),
            0xDA => Some(Self::WaitAll),
            0xDB => Some(Self::SpawnClosure),
            0xDF => Some(Self::Sleep),

            // RefCell operations
            0xDC => Some(Self::NewRefCell),
            0xDD => Some(Self::LoadRefCell),
            0xDE => Some(Self::StoreRefCell),

            // JSON operations (0xE0, 0xE1 reserved for NativeCall parse/stringify)
            0xE2 => Some(Self::JsonGet),
            0xE3 => Some(Self::JsonSet),
            0xE4 => Some(Self::JsonDelete),
            0xE5 => Some(Self::JsonIndex),
            0xE6 => Some(Self::JsonIndexSet),
            0xE7 => Some(Self::JsonPush),
            0xE8 => Some(Self::JsonPop),
            0xE9 => Some(Self::JsonNewObject),
            0xEA => Some(Self::JsonNewArray),
            0xEB => Some(Self::JsonKeys),
            0xEC => Some(Self::JsonLength),

            // Task control & type operations
            0xED => Some(Self::TaskCancel),
            0xEE => Some(Self::InstanceOf),
            0xEF => Some(Self::Cast),

            // Closures & modules
            0xF0 => Some(Self::MakeClosure),
            0xF1 => Some(Self::CloseVar),
            0xF2 => Some(Self::LoadCaptured),
            0xF3 => Some(Self::StoreCaptured),
            0xF4 => Some(Self::LoadModule),
            0xF5 => Some(Self::LoadGlobal),
            0xF6 => Some(Self::StoreGlobal),
            0xF7 => Some(Self::SetClosureCapture),

            // Exception handling & special
            0xF8 => Some(Self::Throw),
            0xF9 => Some(Self::Try),
            0xFA => Some(Self::EndTry),
            0xFB => Some(Self::Rethrow),
            0xFC => Some(Self::Trap),
            0xFD => Some(Self::NativeCall),
            0xFE => Some(Self::ModuleNativeCall),

            // Invalid opcodes
            _ => None,
        }
    }

    /// Convert opcode to byte
    #[inline]
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Get the human-readable name of the opcode
    pub fn name(self) -> &'static str {
        match self {
            Self::Nop => "NOP",
            Self::Pop => "POP",
            Self::Dup => "DUP",
            Self::Swap => "SWAP",
            Self::ConstNull => "CONST_NULL",
            Self::ConstTrue => "CONST_TRUE",
            Self::ConstFalse => "CONST_FALSE",
            Self::ConstI32 => "CONST_I32",
            Self::ConstF64 => "CONST_F64",
            Self::ConstStr => "CONST_STR",
            Self::LoadConst => "LOAD_CONST",
            Self::LoadLocal => "LOAD_LOCAL",
            Self::StoreLocal => "STORE_LOCAL",
            Self::LoadLocal0 => "LOAD_LOCAL_0",
            Self::LoadLocal1 => "LOAD_LOCAL_1",
            Self::StoreLocal0 => "STORE_LOCAL_0",
            Self::StoreLocal1 => "STORE_LOCAL_1",
            Self::Iadd => "IADD",
            Self::Isub => "ISUB",
            Self::Imul => "IMUL",
            Self::Idiv => "IDIV",
            Self::Imod => "IMOD",
            Self::Ineg => "INEG",
            Self::Ipow => "IPOW",
            Self::Ishl => "ISHL",
            Self::Ishr => "ISHR",
            Self::Iushr => "IUSHR",
            Self::Iand => "IAND",
            Self::Ior => "IOR",
            Self::Ixor => "IXOR",
            Self::Inot => "INOT",
            Self::Fadd => "FADD",
            Self::Fsub => "FSUB",
            Self::Fmul => "FMUL",
            Self::Fdiv => "FDIV",
            Self::Fneg => "FNEG",
            Self::Fpow => "FPOW",
            Self::Fmod => "FMOD",
            Self::Debugger => "DEBUGGER",
            Self::Ieq => "IEQ",
            Self::Ine => "INE",
            Self::Ilt => "ILT",
            Self::Ile => "ILE",
            Self::Igt => "IGT",
            Self::Ige => "IGE",
            Self::Feq => "FEQ",
            Self::Fne => "FNE",
            Self::Flt => "FLT",
            Self::Fle => "FLE",
            Self::Fgt => "FGT",
            Self::Fge => "FGE",
            Self::Eq => "EQ",
            Self::Ne => "NE",
            Self::StrictEq => "STRICT_EQ",
            Self::StrictNe => "STRICT_NE",
            Self::Not => "NOT",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Typeof => "TYPEOF",
            Self::Sconcat => "SCONCAT",
            Self::Slen => "SLEN",
            Self::Seq => "SEQ",
            Self::Sne => "SNE",
            Self::Slt => "SLT",
            Self::Sle => "SLE",
            Self::Sgt => "SGT",
            Self::Sge => "SGE",
            Self::ToString => "TO_STRING",
            Self::Jmp => "JMP",
            Self::JmpIfFalse => "JMP_IF_FALSE",
            Self::JmpIfTrue => "JMP_IF_TRUE",
            Self::JmpIfNull => "JMP_IF_NULL",
            Self::JmpIfNotNull => "JMP_IF_NOT_NULL",
            Self::Call => "CALL",
            Self::CallMethod => "CALL_METHOD",
            Self::Return => "RETURN",
            Self::ReturnVoid => "RETURN_VOID",
            Self::CallConstructor => "CALL_CONSTRUCTOR",
            Self::CallSuper => "CALL_SUPER",
            Self::CallStatic => "CALL_STATIC",
            Self::New => "NEW",
            Self::LoadField => "LOAD_FIELD",
            Self::StoreField => "STORE_FIELD",
            Self::LoadFieldFast => "LOAD_FIELD_FAST",
            Self::StoreFieldFast => "STORE_FIELD_FAST",
            Self::ObjectLiteral => "OBJECT_LITERAL",
            Self::InitObject => "INIT_OBJECT",
            Self::OptionalField => "OPTIONAL_FIELD",
            Self::LoadStatic => "LOAD_STATIC",
            Self::StoreStatic => "STORE_STATIC",
            Self::NewArray => "NEW_ARRAY",
            Self::LoadElem => "LOAD_ELEM",
            Self::StoreElem => "STORE_ELEM",
            Self::ArrayLen => "ARRAY_LEN",
            Self::ArrayLiteral => "ARRAY_LITERAL",
            Self::InitArray => "INIT_ARRAY",
            Self::TupleLiteral => "TUPLE_LITERAL",
            Self::InitTuple => "INIT_TUPLE",
            Self::TupleGet => "TUPLE_GET",
            Self::ArrayPush => "ARRAY_PUSH",
            Self::ArrayPop => "ARRAY_POP",
            Self::NewChannel => "NEW_CHANNEL",
            Self::Spawn => "SPAWN",
            Self::Await => "AWAIT",
            Self::Yield => "YIELD",
            Self::TaskThen => "TASK_THEN",
            Self::NewMutex => "NEW_MUTEX",
            Self::MutexLock => "MUTEX_LOCK",
            Self::MutexUnlock => "MUTEX_UNLOCK",
            Self::NewSemaphore => "NEW_SEMAPHORE",
            Self::SemAcquire => "SEM_ACQUIRE",
            Self::SemRelease => "SEM_RELEASE",
            Self::WaitAll => "WAIT_ALL",
            Self::SpawnClosure => "SPAWN_CLOSURE",
            Self::Sleep => "SLEEP",
            Self::JsonGet => "JSON_GET",
            Self::JsonSet => "JSON_SET",
            Self::JsonDelete => "JSON_DELETE",
            Self::JsonIndex => "JSON_INDEX",
            Self::JsonIndexSet => "JSON_INDEX_SET",
            Self::JsonPush => "JSON_PUSH",
            Self::JsonPop => "JSON_POP",
            Self::JsonNewObject => "JSON_NEW_OBJECT",
            Self::JsonNewArray => "JSON_NEW_ARRAY",
            Self::JsonKeys => "JSON_KEYS",
            Self::JsonLength => "JSON_LENGTH",
            Self::TaskCancel => "TASK_CANCEL",
            Self::InstanceOf => "INSTANCE_OF",
            Self::Cast => "CAST",
            Self::MakeClosure => "MAKE_CLOSURE",
            Self::CloseVar => "CLOSE_VAR",
            Self::LoadCaptured => "LOAD_CAPTURED",
            Self::StoreCaptured => "STORE_CAPTURED",
            Self::LoadModule => "LOAD_MODULE",
            Self::LoadGlobal => "LOAD_GLOBAL",
            Self::StoreGlobal => "STORE_GLOBAL",
            Self::SetClosureCapture => "SET_CLOSURE_CAPTURE",
            Self::NewRefCell => "NEW_REFCELL",
            Self::LoadRefCell => "LOAD_REFCELL",
            Self::StoreRefCell => "STORE_REFCELL",
            Self::Throw => "THROW",
            Self::Try => "TRY",
            Self::EndTry => "END_TRY",
            Self::Rethrow => "RETHROW",
            Self::Trap => "TRAP",
            Self::NativeCall => "NATIVE_CALL",
            Self::ModuleNativeCall => "MODULE_NATIVE_CALL",
        }
    }

    /// Check if this opcode is a jump instruction
    pub fn is_jump(self) -> bool {
        matches!(
            self,
            Self::Jmp | Self::JmpIfFalse | Self::JmpIfTrue | Self::JmpIfNull | Self::JmpIfNotNull
        )
    }

    /// Check if this opcode is a call instruction
    pub fn is_call(self) -> bool {
        matches!(
            self,
            Self::Call
                | Self::CallMethod
                | Self::CallConstructor
                | Self::CallSuper
                | Self::CallStatic
        )
    }

    /// Check if this opcode is a return instruction
    pub fn is_return(self) -> bool {
        matches!(self, Self::Return | Self::ReturnVoid)
    }

    /// Check if this opcode terminates a basic block
    pub fn is_terminator(self) -> bool {
        self.is_jump() || self.is_return() || matches!(self, Self::Throw | Self::Trap)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcode_roundtrip() {
        // Test all valid opcodes
        let opcodes = [
            Opcode::Nop,
            Opcode::Pop,
            Opcode::Dup,
            Opcode::Swap,
            Opcode::ConstNull,
            Opcode::ConstTrue,
            Opcode::ConstFalse,
            Opcode::ConstI32,
            Opcode::ConstF64,
            Opcode::ConstStr,
            Opcode::LoadConst,
            Opcode::LoadLocal,
            Opcode::StoreLocal,
            Opcode::LoadLocal0,
            Opcode::LoadLocal1,
            Opcode::StoreLocal0,
            Opcode::StoreLocal1,
            Opcode::Iadd,
            Opcode::Isub,
            Opcode::Imul,
            Opcode::Idiv,
            Opcode::Imod,
            Opcode::Ineg,
            Opcode::Fadd,
            Opcode::Fsub,
            Opcode::Fmul,
            Opcode::Fdiv,
            Opcode::Fneg,
            Opcode::Spawn,
            Opcode::Await,
            Opcode::Yield,
            Opcode::Debugger,
            Opcode::Return,
            Opcode::Call,
        ];

        for opcode in &opcodes {
            let byte = opcode.to_u8();
            let decoded = Opcode::from_u8(byte);
            assert_eq!(decoded, Some(*opcode), "Failed roundtrip for {:?}", opcode);
        }
    }

    #[test]
    fn test_invalid_opcode() {
        // Test that unassigned opcodes return None
        assert_eq!(Opcode::from_u8(0xFE), Some(Opcode::ModuleNativeCall));
        assert_eq!(Opcode::from_u8(0xFF), None); // Unassigned
        // Test that assigned opcodes return correct values
        assert_eq!(Opcode::from_u8(Opcode::NewChannel.to_u8()), Some(Opcode::NewChannel));
        assert_eq!(Opcode::from_u8(Opcode::SetClosureCapture.to_u8()), Some(Opcode::SetClosureCapture));
        assert_eq!(Opcode::from_u8(Opcode::SpawnClosure.to_u8()), Some(Opcode::SpawnClosure));
        assert_eq!(Opcode::from_u8(Opcode::NewRefCell.to_u8()), Some(Opcode::NewRefCell));
        assert_eq!(Opcode::from_u8(Opcode::LoadRefCell.to_u8()), Some(Opcode::LoadRefCell));
        assert_eq!(Opcode::from_u8(Opcode::StoreRefCell.to_u8()), Some(Opcode::StoreRefCell));
        assert_eq!(Opcode::from_u8(Opcode::Sleep.to_u8()), Some(Opcode::Sleep));
        assert_eq!(Opcode::from_u8(Opcode::TaskCancel.to_u8()), Some(Opcode::TaskCancel));
        assert_eq!(Opcode::from_u8(Opcode::InstanceOf.to_u8()), Some(Opcode::InstanceOf));
        assert_eq!(Opcode::from_u8(Opcode::Cast.to_u8()), Some(Opcode::Cast));
        assert_eq!(Opcode::from_u8(Opcode::NativeCall.to_u8()), Some(Opcode::NativeCall));
        assert_eq!(Opcode::from_u8(Opcode::ArrayPush.to_u8()), Some(Opcode::ArrayPush));
        assert_eq!(Opcode::from_u8(Opcode::ArrayPop.to_u8()), Some(Opcode::ArrayPop));
    }

    #[test]
    fn test_opcode_names() {
        assert_eq!(Opcode::Nop.name(), "NOP");
        assert_eq!(Opcode::Iadd.name(), "IADD");
        assert_eq!(Opcode::Spawn.name(), "SPAWN");
        assert_eq!(Opcode::Return.name(), "RETURN");
        assert_eq!(Opcode::MakeClosure.name(), "MAKE_CLOSURE");
        assert_eq!(Opcode::Debugger.name(), "DEBUGGER");
    }

    #[test]
    fn test_jump_detection() {
        assert!(Opcode::Jmp.is_jump());
        assert!(Opcode::JmpIfFalse.is_jump());
        assert!(Opcode::JmpIfTrue.is_jump());
        assert!(Opcode::JmpIfNull.is_jump());
        assert!(Opcode::JmpIfNotNull.is_jump());
        assert!(!Opcode::Call.is_jump());
        assert!(!Opcode::Return.is_jump());
    }

    #[test]
    fn test_call_detection() {
        assert!(Opcode::Call.is_call());
        assert!(Opcode::CallMethod.is_call());
        assert!(Opcode::CallConstructor.is_call());
        assert!(Opcode::CallSuper.is_call());
        assert!(Opcode::CallStatic.is_call());
        assert!(!Opcode::Spawn.is_call()); // Spawn is not a call
        assert!(!Opcode::Return.is_call());
    }

    #[test]
    fn test_return_detection() {
        assert!(Opcode::Return.is_return());
        assert!(Opcode::ReturnVoid.is_return());
        assert!(!Opcode::Call.is_return());
        assert!(!Opcode::Jmp.is_return());
    }

    #[test]
    fn test_terminator_detection() {
        assert!(Opcode::Return.is_terminator());
        assert!(Opcode::ReturnVoid.is_terminator());
        assert!(Opcode::Jmp.is_terminator());
        assert!(Opcode::JmpIfFalse.is_terminator());
        assert!(Opcode::Throw.is_terminator());
        assert!(Opcode::Trap.is_terminator());
        assert!(!Opcode::Call.is_terminator());
        assert!(!Opcode::Iadd.is_terminator());
    }


    #[test]
    fn test_opcode_roundtrip_all() {
        // Test that all opcodes can be converted to u8 and back
        let all_opcodes = [
            Opcode::Nop, Opcode::Pop, Opcode::Dup, Opcode::Swap,
            Opcode::ConstNull, Opcode::ConstTrue, Opcode::ConstFalse,
            Opcode::ConstI32, Opcode::ConstF64, Opcode::ConstStr, Opcode::LoadConst,
            Opcode::LoadLocal, Opcode::StoreLocal, Opcode::LoadLocal0,
            Opcode::LoadLocal1, Opcode::StoreLocal0, Opcode::StoreLocal1,
            Opcode::Iadd, Opcode::Isub, Opcode::Imul, Opcode::Idiv, Opcode::Imod,
            Opcode::Ineg, Opcode::Ipow, Opcode::Ishl, Opcode::Ishr, Opcode::Iushr,
            Opcode::Iand, Opcode::Ior, Opcode::Ixor, Opcode::Inot,
            Opcode::Fadd, Opcode::Fsub, Opcode::Fmul, Opcode::Fdiv, Opcode::Fneg, Opcode::Fpow,
            Opcode::Ieq, Opcode::Ine, Opcode::Ilt, Opcode::Ile, Opcode::Igt, Opcode::Ige,
            Opcode::Feq, Opcode::Fne, Opcode::Flt, Opcode::Fle, Opcode::Fgt, Opcode::Fge,
            Opcode::Eq, Opcode::Ne, Opcode::StrictEq, Opcode::StrictNe, Opcode::Not, Opcode::And, Opcode::Or, Opcode::Typeof,
            Opcode::Sconcat, Opcode::Slen, Opcode::Seq, Opcode::Sne, Opcode::Slt, Opcode::Sle, Opcode::Sgt, Opcode::Sge, Opcode::ToString,
            Opcode::Jmp, Opcode::JmpIfFalse, Opcode::JmpIfTrue, Opcode::JmpIfNull, Opcode::JmpIfNotNull,
            Opcode::Call, Opcode::CallMethod, Opcode::Return, Opcode::ReturnVoid,
            Opcode::CallConstructor, Opcode::CallSuper, Opcode::CallStatic,
            Opcode::New, Opcode::LoadField, Opcode::StoreField, Opcode::LoadFieldFast,
            Opcode::StoreFieldFast, Opcode::ObjectLiteral, Opcode::InitObject,
            Opcode::OptionalField, Opcode::LoadStatic, Opcode::StoreStatic,
            Opcode::NewArray, Opcode::LoadElem, Opcode::StoreElem, Opcode::ArrayLen,
            Opcode::ArrayLiteral, Opcode::InitArray,
            Opcode::TupleLiteral, Opcode::InitTuple, Opcode::TupleGet,
            Opcode::ArrayPush, Opcode::ArrayPop,
            Opcode::Spawn, Opcode::Await, Opcode::Yield, Opcode::TaskThen,
            Opcode::NewMutex, Opcode::MutexLock, Opcode::MutexUnlock,
            Opcode::NewSemaphore, Opcode::SemAcquire, Opcode::SemRelease,
            Opcode::WaitAll, Opcode::SpawnClosure, Opcode::Sleep,
            Opcode::NewRefCell, Opcode::LoadRefCell, Opcode::StoreRefCell,
            Opcode::JsonGet, Opcode::JsonSet, Opcode::JsonDelete, Opcode::JsonIndex, Opcode::JsonIndexSet, Opcode::JsonPush,
            Opcode::JsonPop, Opcode::JsonNewObject, Opcode::JsonNewArray, Opcode::JsonKeys, Opcode::JsonLength,
            Opcode::TaskCancel, Opcode::InstanceOf, Opcode::Cast,
            Opcode::MakeClosure, Opcode::CloseVar, Opcode::LoadCaptured, Opcode::StoreCaptured,
            Opcode::LoadModule, Opcode::LoadGlobal, Opcode::StoreGlobal, Opcode::SetClosureCapture,
            Opcode::Throw, Opcode::Try, Opcode::EndTry, Opcode::Rethrow, Opcode::Trap, Opcode::NativeCall,
            Opcode::ModuleNativeCall,
            Opcode::Debugger,
        ];

        for opcode in &all_opcodes {
            let byte = opcode.to_u8();
            let decoded = Opcode::from_u8(byte);
            assert_eq!(decoded, Some(*opcode), "Failed roundtrip for {:?} (byte: 0x{:02X})", opcode, byte);
        }
    }
}
