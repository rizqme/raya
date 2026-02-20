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
use crate::compiler::ir::value::{IrConstant, IrValue};
use crate::parser::TypeId;

use super::analysis::{SuspensionAnalysis, SuspensionKind, SuspensionPoint};
use super::statemachine::*;
use super::traits::AotCompilable;

// Well-known TypeIds (from TypeContext::new() interning order)
const NUMBER_TYPE_ID: u32 = 0; // f64
#[allow(dead_code)]
const STRING_TYPE_ID: u32 = 1;
const BOOLEAN_TYPE_ID: u32 = 2;
#[allow(dead_code)]
const NULL_TYPE_ID: u32 = 3;
const INT_TYPE_ID: u32 = 16; // i32

/// Adapter that wraps an `IrFunction` to implement `AotCompilable`.
pub struct IrFunctionAdapter<'a> {
    func: &'a IrFunction,
}

impl<'a> IrFunctionAdapter<'a> {
    /// Create a new adapter for the given IR function.
    pub fn new(func: &'a IrFunction) -> Self {
        Self { func }
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

    /// Translate a single IrInstr to SmInstr(s).
    fn translate_instr(instr: &IrInstr, out: &mut Vec<SmInstr>) {
        match instr {
            // === Assignment (constant or register copy) ===
            IrInstr::Assign { dest, value } => {
                match value {
                    IrValue::Constant(c) => match c {
                        IrConstant::I32(v) => out.push(SmInstr::ConstI32 { dest: Self::reg(dest), value: *v }),
                        IrConstant::F64(v) => out.push(SmInstr::ConstF64 { dest: Self::reg(dest), bits: v.to_bits() }),
                        IrConstant::Boolean(v) => out.push(SmInstr::ConstBool { dest: Self::reg(dest), value: *v }),
                        IrConstant::Null => out.push(SmInstr::ConstNull { dest: Self::reg(dest) }),
                        IrConstant::String(_) => {
                            // String constants go through the helper table
                            // TODO: Map string to constant pool index
                            out.push(SmInstr::CallHelper {
                                dest: Some(Self::reg(dest)),
                                helper: HelperCall::LoadStringConstant,
                                args: vec![0], // placeholder constant index
                            });
                        }
                    },
                    IrValue::Register(src) => {
                        out.push(SmInstr::Move { dest: Self::reg(dest), src: Self::reg(src) });
                    }
                }
            }

            // === Binary Operations (type-dispatched) ===
            IrInstr::BinaryOp { dest, op, left, right } => {
                let d = Self::reg(dest);
                let l = Self::reg(left);
                let r = Self::reg(right);

                // Use type information for typed dispatch
                if Self::is_int(left.ty) && Self::is_int(right.ty) {
                    match op {
                        // Arithmetic
                        BinaryOp::Add => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Add, left: l, right: r }),
                        BinaryOp::Sub => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Sub, left: l, right: r }),
                        BinaryOp::Mul => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Mul, left: l, right: r }),
                        BinaryOp::Div => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Div, left: l, right: r }),
                        BinaryOp::Mod => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Mod, left: l, right: r }),
                        BinaryOp::Pow => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Pow, left: l, right: r }),
                        // Comparison
                        BinaryOp::Equal => out.push(SmInstr::I32Cmp { dest: d, op: SmCmpOp::Eq, left: l, right: r }),
                        BinaryOp::NotEqual => out.push(SmInstr::I32Cmp { dest: d, op: SmCmpOp::Ne, left: l, right: r }),
                        BinaryOp::Less => out.push(SmInstr::I32Cmp { dest: d, op: SmCmpOp::Lt, left: l, right: r }),
                        BinaryOp::LessEqual => out.push(SmInstr::I32Cmp { dest: d, op: SmCmpOp::Le, left: l, right: r }),
                        BinaryOp::Greater => out.push(SmInstr::I32Cmp { dest: d, op: SmCmpOp::Gt, left: l, right: r }),
                        BinaryOp::GreaterEqual => out.push(SmInstr::I32Cmp { dest: d, op: SmCmpOp::Ge, left: l, right: r }),
                        // Bitwise
                        BinaryOp::BitAnd => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::And, left: l, right: r }),
                        BinaryOp::BitOr => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Or, left: l, right: r }),
                        BinaryOp::BitXor => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Xor, left: l, right: r }),
                        BinaryOp::ShiftLeft => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Shl, left: l, right: r }),
                        BinaryOp::ShiftRight => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Shr, left: l, right: r }),
                        BinaryOp::UnsignedShiftRight => out.push(SmInstr::I32BinOp { dest: d, op: SmI32BinOp::Ushr, left: l, right: r }),
                        // Logical (should be on booleans, but emit generic)
                        BinaryOp::And | BinaryOp::Or | BinaryOp::Concat => {
                            Self::emit_generic_binop(d, *op, l, r, out);
                        }
                    }
                } else if Self::is_number(left.ty) && Self::is_number(right.ty) {
                    match op {
                        BinaryOp::Add => out.push(SmInstr::F64BinOp { dest: d, op: SmF64BinOp::Add, left: l, right: r }),
                        BinaryOp::Sub => out.push(SmInstr::F64BinOp { dest: d, op: SmF64BinOp::Sub, left: l, right: r }),
                        BinaryOp::Mul => out.push(SmInstr::F64BinOp { dest: d, op: SmF64BinOp::Mul, left: l, right: r }),
                        BinaryOp::Div => out.push(SmInstr::F64BinOp { dest: d, op: SmF64BinOp::Div, left: l, right: r }),
                        BinaryOp::Mod => out.push(SmInstr::F64BinOp { dest: d, op: SmF64BinOp::Mod, left: l, right: r }),
                        BinaryOp::Pow => out.push(SmInstr::F64BinOp { dest: d, op: SmF64BinOp::Pow, left: l, right: r }),
                        BinaryOp::Equal => out.push(SmInstr::F64Cmp { dest: d, op: SmCmpOp::Eq, left: l, right: r }),
                        BinaryOp::NotEqual => out.push(SmInstr::F64Cmp { dest: d, op: SmCmpOp::Ne, left: l, right: r }),
                        BinaryOp::Less => out.push(SmInstr::F64Cmp { dest: d, op: SmCmpOp::Lt, left: l, right: r }),
                        BinaryOp::LessEqual => out.push(SmInstr::F64Cmp { dest: d, op: SmCmpOp::Le, left: l, right: r }),
                        BinaryOp::Greater => out.push(SmInstr::F64Cmp { dest: d, op: SmCmpOp::Gt, left: l, right: r }),
                        BinaryOp::GreaterEqual => out.push(SmInstr::F64Cmp { dest: d, op: SmCmpOp::Ge, left: l, right: r }),
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
                        out.push(SmInstr::CallHelper { dest: Some(d), helper: HelperCall::GenericNeg, args: vec![s] });
                    }
                    UnaryOp::Not => {
                        out.push(SmInstr::CallHelper { dest: Some(d), helper: HelperCall::GenericNot, args: vec![s] });
                    }
                    UnaryOp::BitNot => {
                        // Bitwise NOT on non-int → generic
                        out.push(SmInstr::CallHelper { dest: Some(d), helper: HelperCall::GenericNot, args: vec![s] });
                    }
                }
            }

            // === Local Variable Access ===
            IrInstr::LoadLocal { dest, index } => {
                out.push(SmInstr::LoadLocal { dest: Self::reg(dest), index: *index as u32 });
            }
            IrInstr::StoreLocal { index, value } => {
                out.push(SmInstr::StoreLocal { index: *index as u32, src: Self::reg(value) });
            }
            IrInstr::PopToLocal { index } => {
                // PopToLocal is for catch parameters — load resume value
                out.push(SmInstr::LoadResumeValue { dest: *index as u32 });
            }

            // === Global Variable Access ===
            IrInstr::LoadGlobal { dest, index } => {
                out.push(SmInstr::LoadGlobal { dest: Self::reg(dest), index: *index as u32 });
            }
            IrInstr::StoreGlobal { index, value } => {
                out.push(SmInstr::StoreGlobal { index: *index as u32, src: Self::reg(value) });
            }

            // === Object Field Access ===
            IrInstr::LoadField { dest, object, field } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ObjectGetField,
                    args: vec![Self::reg(object), *field as u32],
                });
            }
            IrInstr::StoreField { object, field, value } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::ObjectSetField,
                    args: vec![Self::reg(object), *field as u32, Self::reg(value)],
                });
            }

            // === JSON Property Access ===
            IrInstr::JsonLoadProperty { dest, object, .. } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::JsonLoadProperty,
                    args: vec![Self::reg(object)], // TODO: property name index
                });
            }
            IrInstr::JsonStoreProperty { object, value, .. } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::JsonStoreProperty,
                    args: vec![Self::reg(object), Self::reg(value)],
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
            IrInstr::StoreElement { array, index, value } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::ArraySet,
                    args: vec![Self::reg(array), Self::reg(index), Self::reg(value)],
                });
            }

            // === Object/Array Creation ===
            IrInstr::NewObject { dest, class } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::AllocObject,
                    args: vec![class.as_u32()],
                });
            }
            IrInstr::NewArray { dest, len, .. } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::AllocArray,
                    args: vec![Self::reg(len)],
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
            IrInstr::ObjectLiteral { dest, class, fields } => {
                let mut args = vec![class.as_u32()];
                for (field_idx, reg) in fields {
                    args.push(*field_idx as u32);
                    args.push(Self::reg(reg));
                }
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::ObjectLiteral,
                    args,
                });
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
            IrInstr::StringCompare { dest, left, right, .. } => {
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
            IrInstr::CallMethod { dest, object, method, args } => {
                let mut call_args = vec![Self::reg(object), *method as u32];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::NativeCall,
                    args: call_args,
                });
            }
            IrInstr::NativeCall { dest, native_id, args } => {
                let mut call_args = vec![*native_id as u32];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::NativeCall,
                    args: call_args,
                });
            }
            IrInstr::ModuleNativeCall { dest, local_idx, args } => {
                let mut call_args = vec![*local_idx as u32];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::ModuleNativeCall,
                    args: call_args,
                });
            }
            IrInstr::CallClosure { dest, closure, args } => {
                let mut call_args = vec![Self::reg(closure)];
                call_args.extend(args.iter().map(Self::reg));
                out.push(SmInstr::CallHelper {
                    dest: dest.as_ref().map(Self::reg),
                    helper: HelperCall::CallClosure,
                    args: call_args,
                });
            }

            // === Closures ===
            IrInstr::MakeClosure { dest, func, captures } => {
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
            IrInstr::SetClosureCapture { closure, index, value } => {
                out.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::StoreCaptured,
                    args: vec![Self::reg(closure), *index as u32, Self::reg(value)],
                });
            }

            // === RefCells ===
            IrInstr::NewRefCell { dest, initial_value } => {
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
            IrInstr::InstanceOf { dest, object, class_id } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::InstanceOf,
                    args: vec![Self::reg(object), class_id.as_u32()],
                });
            }
            IrInstr::Cast { dest, object, class_id } => {
                out.push(SmInstr::CallHelper {
                    dest: Some(Self::reg(dest)),
                    helper: HelperCall::Cast,
                    args: vec![Self::reg(object), class_id.as_u32()],
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
            IrInstr::SpawnClosure { dest, closure, args } => {
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
                out.push(SmInstr::Phi { dest: Self::reg(dest), sources: sm_sources });
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
        }
    }

    /// Translate an IR Terminator to an SmTerminator.
    fn translate_terminator(term: &Terminator) -> SmTerminator {
        match term {
            Terminator::Jump(target) => SmTerminator::Jump(Self::block_id(*target)),
            Terminator::Branch { cond, then_block, else_block } => {
                SmTerminator::Branch {
                    cond: Self::reg(cond),
                    then_block: Self::block_id(*then_block),
                    else_block: Self::block_id(*else_block),
                }
            }
            Terminator::BranchIfNull { value, null_block, not_null_block } => {
                SmTerminator::BranchNull {
                    value: Self::reg(value),
                    null_block: Self::block_id(*null_block),
                    not_null_block: Self::block_id(*not_null_block),
                }
            }
            Terminator::Return(Some(reg)) => {
                SmTerminator::Return { value: Self::reg(reg) }
            }
            Terminator::Return(None) => {
                // void return → return null
                SmTerminator::Return { value: u32::MAX } // sentinel for void
            }
            Terminator::Switch { value, cases, default } => {
                SmTerminator::BrTable {
                    index: Self::reg(value),
                    default: Self::block_id(*default),
                    targets: cases.iter().map(|(_, bb)| Self::block_id(*bb)).collect(),
                }
            }
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
            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
            | BinaryOp::ShiftLeft | BinaryOp::ShiftRight | BinaryOp::UnsignedShiftRight => {
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
            IrInstr::CallMethod { .. } => Some(SuspensionKind::NativeCall),
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

        for block in &self.func.blocks {
            let mut instructions = Vec::new();

            // Handle Throw terminator: emit the throw call as an instruction
            if let Terminator::Throw(reg) = &block.terminator {
                for instr in &block.instructions {
                    Self::translate_instr(instr, &mut instructions);
                }
                instructions.push(SmInstr::CallHelper {
                    dest: None,
                    helper: HelperCall::ThrowException,
                    args: vec![Self::reg(reg)],
                });
            } else {
                for instr in &block.instructions {
                    Self::translate_instr(instr, &mut instructions);
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
        self.func.locals.len() as u32
    }

    fn name(&self) -> Option<&str> {
        Some(&self.func.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::block::{BasicBlock, BasicBlockId};
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
        let mut func = IrFunction::new("test", vec![make_int_reg(0), make_int_reg(1)], TypeId::new(INT_TYPE_ID));

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
            SmInstr::I32BinOp { dest, op, left, right } => {
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
        let mut func = IrFunction::new("test", vec![make_number_reg(0), make_number_reg(1)], TypeId::new(NUMBER_TYPE_ID));

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
        assert!(matches!(&blocks[0].instructions[0], SmInstr::ConstI32 { value: 42, .. }));
        assert!(matches!(&blocks[0].instructions[1], SmInstr::ConstF64 { .. }));
        assert!(matches!(&blocks[0].instructions[2], SmInstr::ConstBool { value: true, .. }));
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
        let await_points: Vec<_> = analysis.points.iter()
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
}
