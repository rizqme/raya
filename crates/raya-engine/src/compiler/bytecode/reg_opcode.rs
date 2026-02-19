//! Register-based bytecode opcodes for the Raya VM
//!
//! 32-bit fixed-width instructions with three formats:
//! - ABC:  [opcode:8][A:8][B:8][C:8]       — binary ops, field access
//! - ABx:  [opcode:8][A:8][Bx:16]          — constants, globals, jumps
//! - AsBx: [opcode:8][A:8][sBx:16 signed]  — conditional jumps, small ints
//!
//! Extended instructions use two consecutive u32 words:
//! - ABCx: [opcode:8][A:8][B:8][C:8] + [extra:32] — calls, spawn, new

/// Register-based opcode enumeration
///
/// Each instruction is a 32-bit word. Register operands are 8-bit indices (0-255).
/// Some instructions use an additional 32-bit extension word for wide operands.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegOpcode {
    // ===== Constants & Moves (0x00-0x0F) =====
    /// No operation
    Nop = 0x00,
    /// rA = rB (register move)
    Move = 0x01,
    /// rA = null
    LoadNil = 0x02,
    /// rA = true
    LoadTrue = 0x03,
    /// rA = false
    LoadFalse = 0x04,
    /// rA = sBx (signed 16-bit immediate integer)
    LoadInt = 0x05,
    /// rA = constants[Bx] (from constant pool: i32, f64, string)
    LoadConst = 0x06,
    /// rA = globals[Bx]
    LoadGlobal = 0x07,
    /// globals[Bx] = rA
    StoreGlobal = 0x08,

    // ===== Integer Arithmetic (0x10-0x1F) =====
    /// rA = rB + rC (i32, wrapping)
    Iadd = 0x10,
    /// rA = rB - rC
    Isub = 0x11,
    /// rA = rB * rC
    Imul = 0x12,
    /// rA = rB / rC
    Idiv = 0x13,
    /// rA = rB % rC
    Imod = 0x14,
    /// rA = -rB (C unused)
    Ineg = 0x15,
    /// rA = rB ** rC
    Ipow = 0x16,
    /// rA = rB << rC
    Ishl = 0x17,
    /// rA = rB >> rC (signed)
    Ishr = 0x18,
    /// rA = rB >>> rC (unsigned)
    Iushr = 0x19,
    /// rA = rB & rC
    Iand = 0x1A,
    /// rA = rB | rC
    Ior = 0x1B,
    /// rA = rB ^ rC
    Ixor = 0x1C,
    /// rA = ~rB (C unused)
    Inot = 0x1D,

    // ===== Float Arithmetic (0x20-0x2F) =====
    /// rA = rB + rC (f64)
    Fadd = 0x20,
    /// rA = rB - rC
    Fsub = 0x21,
    /// rA = rB * rC
    Fmul = 0x22,
    /// rA = rB / rC
    Fdiv = 0x23,
    /// rA = -rB (C unused)
    Fneg = 0x24,
    /// rA = rB ** rC
    Fpow = 0x25,
    /// rA = rB % rC
    Fmod = 0x26,

    // ===== Integer Comparison (0x30-0x3F) =====
    /// rA = (rB == rC) as bool (i32)
    Ieq = 0x30,
    /// rA = (rB != rC) as bool
    Ine = 0x31,
    /// rA = (rB < rC) as bool
    Ilt = 0x32,
    /// rA = (rB <= rC) as bool
    Ile = 0x33,
    /// rA = (rB > rC) as bool
    Igt = 0x34,
    /// rA = (rB >= rC) as bool
    Ige = 0x35,

    // ===== Float Comparison (0x38-0x3F) =====
    /// rA = (rB == rC) as bool (f64)
    Feq = 0x38,
    /// rA = (rB != rC) as bool
    Fne = 0x39,
    /// rA = (rB < rC) as bool
    Flt = 0x3A,
    /// rA = (rB <= rC) as bool
    Fle = 0x3B,
    /// rA = (rB > rC) as bool
    Fgt = 0x3C,
    /// rA = (rB >= rC) as bool
    Fge = 0x3D,

    // ===== Generic Comparison & Logical (0x40-0x4F) =====
    /// rA = (rB == rC) structural equality
    Eq = 0x40,
    /// rA = (rB != rC)
    Ne = 0x41,
    /// rA = (rB === rC) strict equality
    StrictEq = 0x42,
    /// rA = (rB !== rC)
    StrictNe = 0x43,
    /// rA = !rB (C unused)
    Not = 0x44,
    /// rA = rB && rC
    And = 0x45,
    /// rA = rB || rC
    Or = 0x46,
    /// rA = typeof rB (C unused)
    Typeof = 0x47,

    // ===== String Operations (0x48-0x4F) =====
    /// rA = rB ++ rC (string concatenation)
    Sconcat = 0x48,
    /// rA = rB.length (string length, C unused)
    Slen = 0x49,
    /// rA = (rB == rC) string equality
    Seq = 0x4A,
    /// rA = (rB != rC) string inequality
    Sne = 0x4B,
    /// rA = (rB < rC) string less than
    Slt = 0x4C,
    /// rA = (rB <= rC) string less or equal
    Sle = 0x4D,
    /// rA = (rB > rC) string greater than
    Sgt = 0x4E,
    /// rA = (rB >= rC) string greater or equal
    Sge = 0x4F,
    /// rA = String(rB) (C unused)
    ToString = 0x50,

    // ===== Control Flow (0x58-0x5F) =====
    /// PC += sBx unconditional jump (A unused)
    Jmp = 0x58,
    /// if rA then PC += sBx
    JmpIf = 0x59,
    /// if !rA then PC += sBx
    JmpIfNot = 0x5A,
    /// if rA == null then PC += sBx
    JmpIfNull = 0x5B,
    /// if rA != null then PC += sBx
    JmpIfNotNull = 0x5C,

    // ===== Function Calls (0x60-0x6F) — extended format =====
    /// rA = func(rB, rB+1, ..., rB+C-1); extra = func_id
    Call = 0x60,
    /// rA = rB.method(rB+1, ..., rB+C-1); extra = method_idx
    CallMethod = 0x61,
    /// return rA (B, C unused)
    Return = 0x62,
    /// return null
    ReturnVoid = 0x63,
    /// rA = new Class(rB, ..., rB+C-1); extra = func_id
    CallConstructor = 0x64,
    /// super(rB, ..., rB+C-1); extra = func_id
    CallSuper = 0x65,
    /// rA = rB(rB+1, ..., rB+C-1) — closure call
    CallClosure = 0x66,
    /// rA = static_method(rB, ..., rB+C-1); extra = method_idx
    CallStatic = 0x67,

    // ===== Object Operations (0x70-0x7F) =====
    /// rA = new Class; extra = class_id (extended)
    New = 0x70,
    /// rA = rB.field[C]
    LoadField = 0x71,
    /// rA.field[B] = rC
    StoreField = 0x72,
    /// rA = { fields from rB..rB+C-1 }; extra = class_id (extended)
    ObjectLiteral = 0x73,
    /// rA = static[Bx] (ABx format)
    LoadStatic = 0x74,
    /// static[Bx] = rA (ABx format)
    StoreStatic = 0x75,
    /// rA = rB instanceof class; extra = class_id (extended)
    InstanceOf = 0x76,
    /// rA = rB as class; extra = class_id (extended)
    Cast = 0x77,
    /// rA = rB?.field[C] (optional chaining)
    OptionalField = 0x78,

    // ===== Array & Tuple Operations (0x80-0x8F) =====
    /// rA = new Array(rB); extra = type_id (extended)
    NewArray = 0x80,
    /// rA = rB[rC]
    LoadElem = 0x81,
    /// rA[rB] = rC
    StoreElem = 0x82,
    /// rA = rB.length (C unused)
    ArrayLen = 0x83,
    /// rA = [rB, rB+1, ..., rB+C-1]; extra = type_id (extended)
    ArrayLiteral = 0x84,
    /// rA.push(rB) (C unused)
    ArrayPush = 0x85,
    /// rA = rB.pop() (C unused)
    ArrayPop = 0x86,
    /// rA = (rB, rB+1, ..., rB+C-1); extra = type_id (extended)
    TupleLiteral = 0x87,
    /// rA = rB[C] (constant tuple index)
    TupleGet = 0x88,

    // ===== Concurrency (0x90-0x9F) =====
    /// rA = spawn func(rB..rB+C-1); extra = func_id (extended)
    Spawn = 0x90,
    /// rA = spawn rB(rB+1..rB+C-1) (closure spawn)
    SpawnClosure = 0x91,
    /// rA = await rB (C unused)
    Await = 0x92,
    /// rA = await_all rB (C unused, rB is array of tasks)
    AwaitAll = 0x93,
    /// sleep rA ms (B, C unused)
    Sleep = 0x94,
    /// yield to scheduler
    Yield = 0x95,
    /// rA = new Mutex (B, C unused)
    NewMutex = 0x96,
    /// lock rA (B, C unused)
    MutexLock = 0x97,
    /// unlock rA (B, C unused)
    MutexUnlock = 0x98,
    /// rA = new Channel(rB) (C unused)
    NewChannel = 0x99,
    /// cancel rA (B, C unused)
    TaskCancel = 0x9A,
    /// rA.then(rB); extra = func_id (extended)
    TaskThen = 0x9B,

    // ===== Closures & Captures (0xA0-0xAF) =====
    /// rA = closure(func_id, captures from rB..rB+C-1); extra = func_id (extended)
    MakeClosure = 0xA0,
    /// rA = captured[Bx] (ABx format)
    LoadCaptured = 0xA1,
    /// captured[Bx] = rA (ABx format)
    StoreCaptured = 0xA2,
    /// rA.captures[B] = rC
    SetClosureCapture = 0xA3,
    /// rA = RefCell(rB) (C unused)
    NewRefCell = 0xA4,
    /// rA = rB.value (load RefCell, C unused)
    LoadRefCell = 0xA5,
    /// rA.value = rB (store RefCell, C unused)
    StoreRefCell = 0xA6,
    /// rA = module[Bx] (ABx format)
    LoadModule = 0xA7,

    // ===== Exception Handling (0xB0-0xB7) =====
    /// setup try handler; sBx = catch_offset; extra = finally_offset (extended)
    Try = 0xB0,
    /// remove exception handler
    EndTry = 0xB1,
    /// throw rA (B, C unused)
    Throw = 0xB2,
    /// rethrow current exception
    Rethrow = 0xB3,

    // ===== JSON Operations (0xC0-0xCF) =====
    /// rA = rB[prop]; extra = const_pool_idx (extended)
    JsonGet = 0xC0,
    /// rA[prop] = rB; extra = const_pool_idx (extended)
    JsonSet = 0xC1,
    /// delete rA[prop]; extra = const_pool_idx (extended)
    JsonDelete = 0xC2,
    /// rA = rB[rC] (dynamic JSON index)
    JsonIndex = 0xC3,
    /// rA[rB] = rC (dynamic JSON index set)
    JsonIndexSet = 0xC4,
    /// rA.push(rB) (JSON array push, C unused)
    JsonPush = 0xC5,
    /// rA = rB.pop() (JSON array pop, C unused)
    JsonPop = 0xC6,
    /// rA = {} (new JSON object, B, C unused)
    JsonNewObject = 0xC7,
    /// rA = [] (new JSON array, B, C unused)
    JsonNewArray = 0xC8,
    /// rA = keys(rB) (C unused)
    JsonKeys = 0xC9,
    /// rA = rB.length (JSON length, C unused)
    JsonLength = 0xCA,

    // ===== Native Calls (0xD0-0xD3) — extended format =====
    /// rA = native_id(rB..rB+C-1); extra = native_id (extended)
    NativeCall = 0xD0,
    /// rA = module_native(rB..rB+C-1); extra = local_idx (extended)
    ModuleNativeCall = 0xD1,
    /// Trap with error code; Bx = error_code (ABx format)
    Trap = 0xD2,
}

/// Instruction format types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrFormat {
    /// [opcode:8][A:8][B:8][C:8] — binary ops, field access, etc.
    ABC,
    /// [opcode:8][A:8][Bx:16] — constants, globals
    ABx,
    /// [opcode:8][A:8][sBx:16 signed] — jumps, small ints
    AsBx,
    /// [opcode:8][A:8][B:8][C:8] + [extra:32] — calls, new, spawn
    ABCx,
}

impl RegOpcode {
    /// Convert byte to register opcode
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            // Constants & Moves
            0x00 => Some(Self::Nop),
            0x01 => Some(Self::Move),
            0x02 => Some(Self::LoadNil),
            0x03 => Some(Self::LoadTrue),
            0x04 => Some(Self::LoadFalse),
            0x05 => Some(Self::LoadInt),
            0x06 => Some(Self::LoadConst),
            0x07 => Some(Self::LoadGlobal),
            0x08 => Some(Self::StoreGlobal),

            // Integer arithmetic
            0x10 => Some(Self::Iadd),
            0x11 => Some(Self::Isub),
            0x12 => Some(Self::Imul),
            0x13 => Some(Self::Idiv),
            0x14 => Some(Self::Imod),
            0x15 => Some(Self::Ineg),
            0x16 => Some(Self::Ipow),
            0x17 => Some(Self::Ishl),
            0x18 => Some(Self::Ishr),
            0x19 => Some(Self::Iushr),
            0x1A => Some(Self::Iand),
            0x1B => Some(Self::Ior),
            0x1C => Some(Self::Ixor),
            0x1D => Some(Self::Inot),

            // Float arithmetic
            0x20 => Some(Self::Fadd),
            0x21 => Some(Self::Fsub),
            0x22 => Some(Self::Fmul),
            0x23 => Some(Self::Fdiv),
            0x24 => Some(Self::Fneg),
            0x25 => Some(Self::Fpow),
            0x26 => Some(Self::Fmod),

            // Integer comparison
            0x30 => Some(Self::Ieq),
            0x31 => Some(Self::Ine),
            0x32 => Some(Self::Ilt),
            0x33 => Some(Self::Ile),
            0x34 => Some(Self::Igt),
            0x35 => Some(Self::Ige),

            // Float comparison
            0x38 => Some(Self::Feq),
            0x39 => Some(Self::Fne),
            0x3A => Some(Self::Flt),
            0x3B => Some(Self::Fle),
            0x3C => Some(Self::Fgt),
            0x3D => Some(Self::Fge),

            // Generic comparison & logical
            0x40 => Some(Self::Eq),
            0x41 => Some(Self::Ne),
            0x42 => Some(Self::StrictEq),
            0x43 => Some(Self::StrictNe),
            0x44 => Some(Self::Not),
            0x45 => Some(Self::And),
            0x46 => Some(Self::Or),
            0x47 => Some(Self::Typeof),

            // String operations
            0x48 => Some(Self::Sconcat),
            0x49 => Some(Self::Slen),
            0x4A => Some(Self::Seq),
            0x4B => Some(Self::Sne),
            0x4C => Some(Self::Slt),
            0x4D => Some(Self::Sle),
            0x4E => Some(Self::Sgt),
            0x4F => Some(Self::Sge),
            0x50 => Some(Self::ToString),

            // Control flow
            0x58 => Some(Self::Jmp),
            0x59 => Some(Self::JmpIf),
            0x5A => Some(Self::JmpIfNot),
            0x5B => Some(Self::JmpIfNull),
            0x5C => Some(Self::JmpIfNotNull),

            // Function calls
            0x60 => Some(Self::Call),
            0x61 => Some(Self::CallMethod),
            0x62 => Some(Self::Return),
            0x63 => Some(Self::ReturnVoid),
            0x64 => Some(Self::CallConstructor),
            0x65 => Some(Self::CallSuper),
            0x66 => Some(Self::CallClosure),
            0x67 => Some(Self::CallStatic),

            // Object operations
            0x70 => Some(Self::New),
            0x71 => Some(Self::LoadField),
            0x72 => Some(Self::StoreField),
            0x73 => Some(Self::ObjectLiteral),
            0x74 => Some(Self::LoadStatic),
            0x75 => Some(Self::StoreStatic),
            0x76 => Some(Self::InstanceOf),
            0x77 => Some(Self::Cast),
            0x78 => Some(Self::OptionalField),

            // Array & tuple operations
            0x80 => Some(Self::NewArray),
            0x81 => Some(Self::LoadElem),
            0x82 => Some(Self::StoreElem),
            0x83 => Some(Self::ArrayLen),
            0x84 => Some(Self::ArrayLiteral),
            0x85 => Some(Self::ArrayPush),
            0x86 => Some(Self::ArrayPop),
            0x87 => Some(Self::TupleLiteral),
            0x88 => Some(Self::TupleGet),

            // Concurrency
            0x90 => Some(Self::Spawn),
            0x91 => Some(Self::SpawnClosure),
            0x92 => Some(Self::Await),
            0x93 => Some(Self::AwaitAll),
            0x94 => Some(Self::Sleep),
            0x95 => Some(Self::Yield),
            0x96 => Some(Self::NewMutex),
            0x97 => Some(Self::MutexLock),
            0x98 => Some(Self::MutexUnlock),
            0x99 => Some(Self::NewChannel),
            0x9A => Some(Self::TaskCancel),
            0x9B => Some(Self::TaskThen),

            // Closures & captures
            0xA0 => Some(Self::MakeClosure),
            0xA1 => Some(Self::LoadCaptured),
            0xA2 => Some(Self::StoreCaptured),
            0xA3 => Some(Self::SetClosureCapture),
            0xA4 => Some(Self::NewRefCell),
            0xA5 => Some(Self::LoadRefCell),
            0xA6 => Some(Self::StoreRefCell),
            0xA7 => Some(Self::LoadModule),

            // Exception handling
            0xB0 => Some(Self::Try),
            0xB1 => Some(Self::EndTry),
            0xB2 => Some(Self::Throw),
            0xB3 => Some(Self::Rethrow),

            // JSON operations
            0xC0 => Some(Self::JsonGet),
            0xC1 => Some(Self::JsonSet),
            0xC2 => Some(Self::JsonDelete),
            0xC3 => Some(Self::JsonIndex),
            0xC4 => Some(Self::JsonIndexSet),
            0xC5 => Some(Self::JsonPush),
            0xC6 => Some(Self::JsonPop),
            0xC7 => Some(Self::JsonNewObject),
            0xC8 => Some(Self::JsonNewArray),
            0xC9 => Some(Self::JsonKeys),
            0xCA => Some(Self::JsonLength),

            // Native calls
            0xD0 => Some(Self::NativeCall),
            0xD1 => Some(Self::ModuleNativeCall),
            0xD2 => Some(Self::Trap),

            _ => None,
        }
    }

    /// Convert opcode to byte
    #[inline]
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Get the instruction format for this opcode
    pub fn format(self) -> InstrFormat {
        match self {
            // ABx format: constants, globals, captured, static, module
            Self::LoadInt | Self::LoadConst | Self::LoadGlobal | Self::StoreGlobal
            | Self::LoadStatic | Self::StoreStatic
            | Self::LoadCaptured | Self::StoreCaptured | Self::LoadModule
            | Self::Trap => InstrFormat::ABx,

            // AsBx format: jumps
            Self::Jmp | Self::JmpIf | Self::JmpIfNot
            | Self::JmpIfNull | Self::JmpIfNotNull => InstrFormat::AsBx,

            // ABCx format (extended): calls, new, spawn, closures, etc.
            Self::Call | Self::CallMethod | Self::CallConstructor | Self::CallSuper
            | Self::CallStatic
            | Self::New | Self::ObjectLiteral | Self::InstanceOf | Self::Cast
            | Self::NewArray | Self::ArrayLiteral | Self::TupleLiteral
            | Self::Spawn | Self::TaskThen | Self::MakeClosure
            | Self::Try
            | Self::JsonGet | Self::JsonSet | Self::JsonDelete
            | Self::NativeCall | Self::ModuleNativeCall => InstrFormat::ABCx,

            // ABC format: everything else
            _ => InstrFormat::ABC,
        }
    }

    /// Returns true if this instruction uses an extra u32 word
    #[inline]
    pub fn is_extended(self) -> bool {
        self.format() == InstrFormat::ABCx
    }

    /// Get the human-readable name of the opcode
    pub fn name(self) -> &'static str {
        match self {
            Self::Nop => "NOP",
            Self::Move => "MOVE",
            Self::LoadNil => "LOAD_NIL",
            Self::LoadTrue => "LOAD_TRUE",
            Self::LoadFalse => "LOAD_FALSE",
            Self::LoadInt => "LOAD_INT",
            Self::LoadConst => "LOAD_CONST",
            Self::LoadGlobal => "LOAD_GLOBAL",
            Self::StoreGlobal => "STORE_GLOBAL",
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
            Self::JmpIf => "JMP_IF",
            Self::JmpIfNot => "JMP_IF_NOT",
            Self::JmpIfNull => "JMP_IF_NULL",
            Self::JmpIfNotNull => "JMP_IF_NOT_NULL",
            Self::Call => "CALL",
            Self::CallMethod => "CALL_METHOD",
            Self::Return => "RETURN",
            Self::ReturnVoid => "RETURN_VOID",
            Self::CallConstructor => "CALL_CONSTRUCTOR",
            Self::CallSuper => "CALL_SUPER",
            Self::CallClosure => "CALL_CLOSURE",
            Self::CallStatic => "CALL_STATIC",
            Self::New => "NEW",
            Self::LoadField => "LOAD_FIELD",
            Self::StoreField => "STORE_FIELD",
            Self::ObjectLiteral => "OBJECT_LITERAL",
            Self::LoadStatic => "LOAD_STATIC",
            Self::StoreStatic => "STORE_STATIC",
            Self::InstanceOf => "INSTANCE_OF",
            Self::Cast => "CAST",
            Self::OptionalField => "OPTIONAL_FIELD",
            Self::NewArray => "NEW_ARRAY",
            Self::LoadElem => "LOAD_ELEM",
            Self::StoreElem => "STORE_ELEM",
            Self::ArrayLen => "ARRAY_LEN",
            Self::ArrayLiteral => "ARRAY_LITERAL",
            Self::ArrayPush => "ARRAY_PUSH",
            Self::ArrayPop => "ARRAY_POP",
            Self::TupleLiteral => "TUPLE_LITERAL",
            Self::TupleGet => "TUPLE_GET",
            Self::Spawn => "SPAWN",
            Self::SpawnClosure => "SPAWN_CLOSURE",
            Self::Await => "AWAIT",
            Self::AwaitAll => "AWAIT_ALL",
            Self::Sleep => "SLEEP",
            Self::Yield => "YIELD",
            Self::NewMutex => "NEW_MUTEX",
            Self::MutexLock => "MUTEX_LOCK",
            Self::MutexUnlock => "MUTEX_UNLOCK",
            Self::NewChannel => "NEW_CHANNEL",
            Self::TaskCancel => "TASK_CANCEL",
            Self::TaskThen => "TASK_THEN",
            Self::MakeClosure => "MAKE_CLOSURE",
            Self::LoadCaptured => "LOAD_CAPTURED",
            Self::StoreCaptured => "STORE_CAPTURED",
            Self::SetClosureCapture => "SET_CLOSURE_CAPTURE",
            Self::NewRefCell => "NEW_REFCELL",
            Self::LoadRefCell => "LOAD_REFCELL",
            Self::StoreRefCell => "STORE_REFCELL",
            Self::LoadModule => "LOAD_MODULE",
            Self::Try => "TRY",
            Self::EndTry => "END_TRY",
            Self::Throw => "THROW",
            Self::Rethrow => "RETHROW",
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
            Self::NativeCall => "NATIVE_CALL",
            Self::ModuleNativeCall => "MODULE_NATIVE_CALL",
            Self::Trap => "TRAP",
        }
    }

    /// Check if this opcode is a jump instruction
    pub fn is_jump(self) -> bool {
        matches!(
            self,
            Self::Jmp | Self::JmpIf | Self::JmpIfNot | Self::JmpIfNull | Self::JmpIfNotNull
        )
    }

    /// Check if this opcode is a call instruction
    pub fn is_call(self) -> bool {
        matches!(
            self,
            Self::Call | Self::CallMethod | Self::CallConstructor
            | Self::CallSuper | Self::CallStatic | Self::CallClosure
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

// ============================================================================
// Instruction encoding/decoding
// ============================================================================

/// A 32-bit register-based instruction word
///
/// Layout: [opcode:8][A:8][B:8][C:8] or [opcode:8][A:8][Bx:16]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegInstr(pub u32);

impl RegInstr {
    // ===== Constructors =====

    /// Encode ABC format: [opcode][A][B][C]
    #[inline]
    pub fn abc(op: RegOpcode, a: u8, b: u8, c: u8) -> Self {
        Self(
            (op as u32) << 24
            | (a as u32) << 16
            | (b as u32) << 8
            | (c as u32)
        )
    }

    /// Encode ABx format: [opcode][A][Bx:16]
    #[inline]
    pub fn abx(op: RegOpcode, a: u8, bx: u16) -> Self {
        Self(
            (op as u32) << 24
            | (a as u32) << 16
            | (bx as u32)
        )
    }

    /// Encode AsBx format: [opcode][A][sBx:16 signed]
    #[inline]
    pub fn asbx(op: RegOpcode, a: u8, sbx: i16) -> Self {
        Self(
            (op as u32) << 24
            | (a as u32) << 16
            | (sbx as u16 as u32)
        )
    }

    // ===== Decoders =====

    /// Extract opcode byte (bits 31-24)
    #[inline]
    pub fn opcode_byte(self) -> u8 {
        (self.0 >> 24) as u8
    }

    /// Extract opcode enum
    #[inline]
    pub fn opcode(self) -> Option<RegOpcode> {
        RegOpcode::from_u8(self.opcode_byte())
    }

    /// Extract A field (bits 23-16)
    #[inline]
    pub fn a(self) -> u8 {
        (self.0 >> 16) as u8
    }

    /// Extract B field (bits 15-8) — only valid for ABC format
    #[inline]
    pub fn b(self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// Extract C field (bits 7-0) — only valid for ABC format
    #[inline]
    pub fn c(self) -> u8 {
        self.0 as u8
    }

    /// Extract Bx field (bits 15-0) — only valid for ABx format
    #[inline]
    pub fn bx(self) -> u16 {
        self.0 as u16
    }

    /// Extract sBx field (bits 15-0 as signed) — only valid for AsBx format
    #[inline]
    pub fn sbx(self) -> i16 {
        self.0 as u16 as i16
    }

    /// Get the raw u32 value
    #[inline]
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Create from raw u32
    #[inline]
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl std::fmt::Display for RegInstr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op = match self.opcode() {
            Some(op) => op,
            None => return write!(f, "UNKNOWN(0x{:02X})", self.opcode_byte()),
        };
        match op.format() {
            InstrFormat::ABC | InstrFormat::ABCx => {
                write!(f, "{} r{}, r{}, r{}", op.name(), self.a(), self.b(), self.c())
            }
            InstrFormat::ABx => {
                write!(f, "{} r{}, {}", op.name(), self.a(), self.bx())
            }
            InstrFormat::AsBx => {
                write!(f, "{} r{}, {}", op.name(), self.a(), self.sbx())
            }
        }
    }
}

// ============================================================================
// RegBytecodeWriter — emits Vec<u32> register instructions
// ============================================================================

/// Writer for register-based bytecode (Vec<u32> instructions)
pub struct RegBytecodeWriter {
    code: Vec<u32>,
}

impl RegBytecodeWriter {
    /// Create a new writer
    pub fn new() -> Self {
        Self { code: Vec::new() }
    }

    /// Emit an ABC-format instruction
    #[inline]
    pub fn emit_abc(&mut self, op: RegOpcode, a: u8, b: u8, c: u8) -> usize {
        let pos = self.code.len();
        self.code.push(RegInstr::abc(op, a, b, c).raw());
        pos
    }

    /// Emit an ABx-format instruction
    #[inline]
    pub fn emit_abx(&mut self, op: RegOpcode, a: u8, bx: u16) -> usize {
        let pos = self.code.len();
        self.code.push(RegInstr::abx(op, a, bx).raw());
        pos
    }

    /// Emit an AsBx-format instruction
    #[inline]
    pub fn emit_asbx(&mut self, op: RegOpcode, a: u8, sbx: i16) -> usize {
        let pos = self.code.len();
        self.code.push(RegInstr::asbx(op, a, sbx).raw());
        pos
    }

    /// Emit an extended (ABCx) instruction: ABC word + extra u32
    #[inline]
    pub fn emit_abcx(&mut self, op: RegOpcode, a: u8, b: u8, c: u8, extra: u32) -> usize {
        let pos = self.code.len();
        self.code.push(RegInstr::abc(op, a, b, c).raw());
        self.code.push(extra);
        pos
    }

    /// Emit a raw u32 word (for extension words)
    #[inline]
    pub fn emit_raw(&mut self, word: u32) {
        self.code.push(word);
    }

    /// Current position (instruction index)
    #[inline]
    pub fn position(&self) -> usize {
        self.code.len()
    }

    /// Patch an instruction at a given position
    #[inline]
    pub fn patch(&mut self, pos: usize, instr: u32) {
        self.code[pos] = instr;
    }

    /// Patch the sBx field of an AsBx instruction at a given position
    pub fn patch_sbx(&mut self, pos: usize, sbx: i16) {
        let existing = self.code[pos];
        // Keep opcode and A, replace lower 16 bits
        self.code[pos] = (existing & 0xFFFF_0000) | (sbx as u16 as u32);
    }

    /// Patch the extra word of an extended instruction
    pub fn patch_extra(&mut self, pos: usize, extra: u32) {
        self.code[pos + 1] = extra;
    }

    /// Consume the writer and return the instruction vector
    pub fn finish(self) -> Vec<u32> {
        self.code
    }

    /// Get a reference to the current code
    pub fn code(&self) -> &[u32] {
        &self.code
    }

    /// Get a mutable reference to the current code
    pub fn code_mut(&mut self) -> &mut Vec<u32> {
        &mut self.code
    }
}

impl Default for RegBytecodeWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abc_encode_decode() {
        let instr = RegInstr::abc(RegOpcode::Iadd, 2, 0, 1);
        assert_eq!(instr.opcode(), Some(RegOpcode::Iadd));
        assert_eq!(instr.a(), 2);
        assert_eq!(instr.b(), 0);
        assert_eq!(instr.c(), 1);
    }

    #[test]
    fn test_abx_encode_decode() {
        let instr = RegInstr::abx(RegOpcode::LoadConst, 5, 1234);
        assert_eq!(instr.opcode(), Some(RegOpcode::LoadConst));
        assert_eq!(instr.a(), 5);
        assert_eq!(instr.bx(), 1234);
    }

    #[test]
    fn test_asbx_encode_decode() {
        // Positive offset
        let instr = RegInstr::asbx(RegOpcode::Jmp, 0, 42);
        assert_eq!(instr.opcode(), Some(RegOpcode::Jmp));
        assert_eq!(instr.a(), 0);
        assert_eq!(instr.sbx(), 42);

        // Negative offset
        let instr = RegInstr::asbx(RegOpcode::JmpIf, 3, -10);
        assert_eq!(instr.opcode(), Some(RegOpcode::JmpIf));
        assert_eq!(instr.a(), 3);
        assert_eq!(instr.sbx(), -10);
    }

    #[test]
    fn test_asbx_edge_values() {
        // Max positive
        let instr = RegInstr::asbx(RegOpcode::Jmp, 0, i16::MAX);
        assert_eq!(instr.sbx(), i16::MAX);

        // Max negative
        let instr = RegInstr::asbx(RegOpcode::Jmp, 0, i16::MIN);
        assert_eq!(instr.sbx(), i16::MIN);

        // Zero
        let instr = RegInstr::asbx(RegOpcode::Jmp, 0, 0);
        assert_eq!(instr.sbx(), 0);
    }

    #[test]
    fn test_abc_max_values() {
        let instr = RegInstr::abc(RegOpcode::Iadd, 255, 255, 255);
        assert_eq!(instr.a(), 255);
        assert_eq!(instr.b(), 255);
        assert_eq!(instr.c(), 255);
    }

    #[test]
    fn test_abx_max_value() {
        let instr = RegInstr::abx(RegOpcode::LoadConst, 255, u16::MAX);
        assert_eq!(instr.a(), 255);
        assert_eq!(instr.bx(), u16::MAX);
    }

    #[test]
    fn test_opcode_roundtrip_all() {
        let all_opcodes = [
            RegOpcode::Nop, RegOpcode::Move, RegOpcode::LoadNil,
            RegOpcode::LoadTrue, RegOpcode::LoadFalse, RegOpcode::LoadInt,
            RegOpcode::LoadConst, RegOpcode::LoadGlobal, RegOpcode::StoreGlobal,
            RegOpcode::Iadd, RegOpcode::Isub, RegOpcode::Imul, RegOpcode::Idiv,
            RegOpcode::Imod, RegOpcode::Ineg, RegOpcode::Ipow, RegOpcode::Ishl,
            RegOpcode::Ishr, RegOpcode::Iushr, RegOpcode::Iand, RegOpcode::Ior,
            RegOpcode::Ixor, RegOpcode::Inot,
            RegOpcode::Fadd, RegOpcode::Fsub, RegOpcode::Fmul, RegOpcode::Fdiv,
            RegOpcode::Fneg, RegOpcode::Fpow, RegOpcode::Fmod,
            RegOpcode::Ieq, RegOpcode::Ine, RegOpcode::Ilt, RegOpcode::Ile,
            RegOpcode::Igt, RegOpcode::Ige,
            RegOpcode::Feq, RegOpcode::Fne, RegOpcode::Flt, RegOpcode::Fle,
            RegOpcode::Fgt, RegOpcode::Fge,
            RegOpcode::Eq, RegOpcode::Ne, RegOpcode::StrictEq, RegOpcode::StrictNe,
            RegOpcode::Not, RegOpcode::And, RegOpcode::Or, RegOpcode::Typeof,
            RegOpcode::Sconcat, RegOpcode::Slen, RegOpcode::Seq, RegOpcode::Sne,
            RegOpcode::Slt, RegOpcode::Sle, RegOpcode::Sgt, RegOpcode::Sge,
            RegOpcode::ToString,
            RegOpcode::Jmp, RegOpcode::JmpIf, RegOpcode::JmpIfNot,
            RegOpcode::JmpIfNull, RegOpcode::JmpIfNotNull,
            RegOpcode::Call, RegOpcode::CallMethod, RegOpcode::Return,
            RegOpcode::ReturnVoid, RegOpcode::CallConstructor, RegOpcode::CallSuper,
            RegOpcode::CallClosure, RegOpcode::CallStatic,
            RegOpcode::New, RegOpcode::LoadField, RegOpcode::StoreField,
            RegOpcode::ObjectLiteral, RegOpcode::LoadStatic, RegOpcode::StoreStatic,
            RegOpcode::InstanceOf, RegOpcode::Cast, RegOpcode::OptionalField,
            RegOpcode::NewArray, RegOpcode::LoadElem, RegOpcode::StoreElem,
            RegOpcode::ArrayLen, RegOpcode::ArrayLiteral, RegOpcode::ArrayPush,
            RegOpcode::ArrayPop, RegOpcode::TupleLiteral, RegOpcode::TupleGet,
            RegOpcode::Spawn, RegOpcode::SpawnClosure, RegOpcode::Await,
            RegOpcode::AwaitAll, RegOpcode::Sleep, RegOpcode::Yield,
            RegOpcode::NewMutex, RegOpcode::MutexLock, RegOpcode::MutexUnlock,
            RegOpcode::NewChannel, RegOpcode::TaskCancel, RegOpcode::TaskThen,
            RegOpcode::MakeClosure, RegOpcode::LoadCaptured, RegOpcode::StoreCaptured,
            RegOpcode::SetClosureCapture, RegOpcode::NewRefCell, RegOpcode::LoadRefCell,
            RegOpcode::StoreRefCell, RegOpcode::LoadModule,
            RegOpcode::Try, RegOpcode::EndTry, RegOpcode::Throw, RegOpcode::Rethrow,
            RegOpcode::JsonGet, RegOpcode::JsonSet, RegOpcode::JsonDelete,
            RegOpcode::JsonIndex, RegOpcode::JsonIndexSet, RegOpcode::JsonPush,
            RegOpcode::JsonPop, RegOpcode::JsonNewObject, RegOpcode::JsonNewArray,
            RegOpcode::JsonKeys, RegOpcode::JsonLength,
            RegOpcode::NativeCall, RegOpcode::ModuleNativeCall, RegOpcode::Trap,
        ];

        for opcode in &all_opcodes {
            let byte = opcode.to_u8();
            let decoded = RegOpcode::from_u8(byte);
            assert_eq!(
                decoded,
                Some(*opcode),
                "Failed roundtrip for {:?} (byte: 0x{:02X})",
                opcode,
                byte
            );
        }
    }

    #[test]
    fn test_opcode_names() {
        assert_eq!(RegOpcode::Nop.name(), "NOP");
        assert_eq!(RegOpcode::Move.name(), "MOVE");
        assert_eq!(RegOpcode::Iadd.name(), "IADD");
        assert_eq!(RegOpcode::Jmp.name(), "JMP");
        assert_eq!(RegOpcode::Call.name(), "CALL");
        assert_eq!(RegOpcode::Return.name(), "RETURN");
        assert_eq!(RegOpcode::NativeCall.name(), "NATIVE_CALL");
    }

    #[test]
    fn test_opcode_format() {
        // ABC format
        assert_eq!(RegOpcode::Iadd.format(), InstrFormat::ABC);
        assert_eq!(RegOpcode::Move.format(), InstrFormat::ABC);
        assert_eq!(RegOpcode::LoadField.format(), InstrFormat::ABC);

        // ABx format
        assert_eq!(RegOpcode::LoadConst.format(), InstrFormat::ABx);
        assert_eq!(RegOpcode::LoadGlobal.format(), InstrFormat::ABx);
        assert_eq!(RegOpcode::LoadCaptured.format(), InstrFormat::ABx);

        // AsBx format
        assert_eq!(RegOpcode::Jmp.format(), InstrFormat::AsBx);
        assert_eq!(RegOpcode::JmpIf.format(), InstrFormat::AsBx);

        // ABCx format (extended)
        assert_eq!(RegOpcode::Call.format(), InstrFormat::ABCx);
        assert_eq!(RegOpcode::New.format(), InstrFormat::ABCx);
        assert_eq!(RegOpcode::Spawn.format(), InstrFormat::ABCx);
        assert_eq!(RegOpcode::NativeCall.format(), InstrFormat::ABCx);
    }

    #[test]
    fn test_is_extended() {
        assert!(RegOpcode::Call.is_extended());
        assert!(RegOpcode::NativeCall.is_extended());
        assert!(RegOpcode::New.is_extended());
        assert!(!RegOpcode::Iadd.is_extended());
        assert!(!RegOpcode::Jmp.is_extended());
        assert!(!RegOpcode::LoadConst.is_extended());
    }

    #[test]
    fn test_jump_detection() {
        assert!(RegOpcode::Jmp.is_jump());
        assert!(RegOpcode::JmpIf.is_jump());
        assert!(RegOpcode::JmpIfNot.is_jump());
        assert!(RegOpcode::JmpIfNull.is_jump());
        assert!(RegOpcode::JmpIfNotNull.is_jump());
        assert!(!RegOpcode::Call.is_jump());
    }

    #[test]
    fn test_call_detection() {
        assert!(RegOpcode::Call.is_call());
        assert!(RegOpcode::CallMethod.is_call());
        assert!(RegOpcode::CallConstructor.is_call());
        assert!(RegOpcode::CallClosure.is_call());
        assert!(!RegOpcode::Spawn.is_call());
        assert!(!RegOpcode::Return.is_call());
    }

    #[test]
    fn test_return_detection() {
        assert!(RegOpcode::Return.is_return());
        assert!(RegOpcode::ReturnVoid.is_return());
        assert!(!RegOpcode::Call.is_return());
    }

    #[test]
    fn test_terminator_detection() {
        assert!(RegOpcode::Return.is_terminator());
        assert!(RegOpcode::ReturnVoid.is_terminator());
        assert!(RegOpcode::Jmp.is_terminator());
        assert!(RegOpcode::Throw.is_terminator());
        assert!(RegOpcode::Trap.is_terminator());
        assert!(!RegOpcode::Iadd.is_terminator());
        assert!(!RegOpcode::Call.is_terminator());
    }

    #[test]
    fn test_bytecode_writer() {
        let mut writer = RegBytecodeWriter::new();

        // LOAD_INT r0, 42
        writer.emit_asbx(RegOpcode::LoadInt, 0, 42);
        // LOAD_INT r1, 10
        writer.emit_asbx(RegOpcode::LoadInt, 1, 10);
        // IADD r2, r0, r1
        writer.emit_abc(RegOpcode::Iadd, 2, 0, 1);
        // RETURN r2
        writer.emit_abc(RegOpcode::Return, 2, 0, 0);

        let code = writer.finish();
        assert_eq!(code.len(), 4);

        // Verify first instruction
        let instr = RegInstr::from_raw(code[0]);
        assert_eq!(instr.opcode(), Some(RegOpcode::LoadInt));
        assert_eq!(instr.a(), 0);
        assert_eq!(instr.sbx(), 42);

        // Verify IADD
        let instr = RegInstr::from_raw(code[2]);
        assert_eq!(instr.opcode(), Some(RegOpcode::Iadd));
        assert_eq!(instr.a(), 2);
        assert_eq!(instr.b(), 0);
        assert_eq!(instr.c(), 1);
    }

    #[test]
    fn test_bytecode_writer_extended() {
        let mut writer = RegBytecodeWriter::new();

        // CALL r5, r10, 3 + func_id=42
        let pos = writer.emit_abcx(RegOpcode::Call, 5, 10, 3, 42);
        assert_eq!(pos, 0);

        let code = writer.finish();
        assert_eq!(code.len(), 2); // ABC word + extra word

        let instr = RegInstr::from_raw(code[0]);
        assert_eq!(instr.opcode(), Some(RegOpcode::Call));
        assert_eq!(instr.a(), 5);
        assert_eq!(instr.b(), 10);
        assert_eq!(instr.c(), 3);
        assert_eq!(code[1], 42); // extra word = func_id
    }

    #[test]
    fn test_bytecode_writer_patch() {
        let mut writer = RegBytecodeWriter::new();

        // Emit a placeholder jump
        let jmp_pos = writer.emit_asbx(RegOpcode::Jmp, 0, 0);

        // Emit some instructions
        writer.emit_abc(RegOpcode::Iadd, 2, 0, 1);
        writer.emit_abc(RegOpcode::Return, 2, 0, 0);

        // Patch the jump to skip to after IADD
        let target = writer.position();
        writer.patch_sbx(jmp_pos, (target as i16) - (jmp_pos as i16) - 1);

        let code = writer.finish();
        let jmp = RegInstr::from_raw(code[0]);
        assert_eq!(jmp.sbx(), 2); // jump over 2 instructions (IADD + RETURN)
    }

    #[test]
    fn test_instr_display() {
        let instr = RegInstr::abc(RegOpcode::Iadd, 2, 0, 1);
        assert_eq!(format!("{}", instr), "IADD r2, r0, r1");

        let instr = RegInstr::abx(RegOpcode::LoadConst, 5, 100);
        assert_eq!(format!("{}", instr), "LOAD_CONST r5, 100");

        let instr = RegInstr::asbx(RegOpcode::Jmp, 0, -5);
        assert_eq!(format!("{}", instr), "JMP r0, -5");
    }
}
