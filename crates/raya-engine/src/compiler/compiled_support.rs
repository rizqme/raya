//! Shared compiled-backend support classification.
//!
//! This module is the single authority for whether a bytecode operation is
//! supported exactly in compiled backends and whether a function is sync-safe.

use crate::compiler::bytecode::module::Module;
use crate::compiler::bytecode::opcode::Opcode;

/// Compiled backend kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompiledBackendKind {
    Aot,
    Jit,
}

/// Exact shared numeric helper operations used by compiled backends.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompiledNumericIntrinsicOp {
    I32Pow = 1,
    F64Pow = 2,
    F64Mod = 3,
}

impl CompiledNumericIntrinsicOp {
    pub const fn from_u16(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::I32Pow),
            2 => Some(Self::F64Pow),
            3 => Some(Self::F64Mod),
            _ => None,
        }
    }
}

/// Exact shared unary value operations used by compiled backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueUnaryOp {
    ToString,
    Typeof,
}

/// Exact shared boxed-value binary operations used by compiled backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueBinaryOp {
    Equal,
    NotEqual,
    StrictEqual,
    StrictNotEqual,
}

/// Exact shared string operations used by compiled backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StringOp {
    Concat,
    CompareEq,
    CompareNe,
    CompareLt,
    CompareLe,
    CompareGt,
    CompareGe,
}

/// Exact shared member operations used by compiled backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberOp {
    BindMethod,
}

/// Exact shared argument-frame operations used by compiled backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgFrameOp {
    GetArgCount,
    LoadArgLocal,
}

/// Typed helper families available to compiled backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompiledAbiOp {
    KernelCall,
    CallFunction,
    CallStatic,
    CallMethodExact,
    CallMethodShape,
    CallConstructor,
    CallSuper,
    ConstructType,
    SpawnFunction,
    SpawnClosure,
    AwaitTask,
    WaitAll,
    Sleep,
    YieldTask,
    NumericIntrinsic(CompiledNumericIntrinsicOp),
    ValueUnary(ValueUnaryOp),
    ValueBinary(ValueBinaryOp),
    StringOp(StringOp),
    MemberOp(MemberOp),
    ArgFrameOp(ArgFrameOp),
}

/// Shared decision for compiled-backend support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompiledSupportDecision {
    NativeLowering,
    AbiHelper(CompiledAbiOp),
    Unsupported { reason: &'static str },
}

impl CompiledSupportDecision {
    pub const fn is_supported(self) -> bool {
        !matches!(self, Self::Unsupported { .. })
    }

    pub const fn may_suspend(self) -> bool {
        matches!(
            self,
            Self::AbiHelper(
                CompiledAbiOp::KernelCall
                    | CompiledAbiOp::CallFunction
                    | CompiledAbiOp::CallStatic
                    | CompiledAbiOp::CallMethodExact
                    | CompiledAbiOp::CallMethodShape
                    | CompiledAbiOp::CallConstructor
                    | CompiledAbiOp::CallSuper
                    | CompiledAbiOp::ConstructType
                    | CompiledAbiOp::SpawnFunction
                    | CompiledAbiOp::SpawnClosure
                    | CompiledAbiOp::AwaitTask
                    | CompiledAbiOp::WaitAll
                    | CompiledAbiOp::Sleep
                    | CompiledAbiOp::YieldTask
            )
        )
    }
}

/// Summary of compiled support for a whole function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompiledFunctionSupport {
    pub is_supported: bool,
    pub is_sync_safe: bool,
    pub may_suspend: bool,
}

impl CompiledFunctionSupport {
    pub const fn supported(sync_safe: bool, may_suspend: bool) -> Self {
        Self {
            is_supported: true,
            is_sync_safe: sync_safe,
            may_suspend,
        }
    }

    pub const fn unsupported(may_suspend: bool) -> Self {
        Self {
            is_supported: false,
            is_sync_safe: false,
            may_suspend,
        }
    }
}

#[cfg(feature = "jit")]
use crate::jit::analysis::decoder::{decode_function, Operands};
#[cfg(feature = "jit")]
use rustc_hash::FxHashSet;

/// Shared bytecode opcode classifier for compiled backends.
#[cfg(feature = "jit")]
pub fn bytecode_instruction_support(
    _backend: CompiledBackendKind,
    opcode: Opcode,
    operands: &Operands,
) -> CompiledSupportDecision {
    match opcode {
        Opcode::Ipow => CompiledSupportDecision::AbiHelper(CompiledAbiOp::NumericIntrinsic(
            CompiledNumericIntrinsicOp::I32Pow,
        )),
        Opcode::Fpow => CompiledSupportDecision::AbiHelper(CompiledAbiOp::NumericIntrinsic(
            CompiledNumericIntrinsicOp::F64Pow,
        )),
        Opcode::Fmod => CompiledSupportDecision::AbiHelper(CompiledAbiOp::NumericIntrinsic(
            CompiledNumericIntrinsicOp::F64Mod,
        )),
        Opcode::KernelCall => CompiledSupportDecision::AbiHelper(CompiledAbiOp::KernelCall),
        Opcode::Await => CompiledSupportDecision::AbiHelper(CompiledAbiOp::AwaitTask),
        Opcode::WaitAll => CompiledSupportDecision::AbiHelper(CompiledAbiOp::WaitAll),
        Opcode::Sleep => CompiledSupportDecision::AbiHelper(CompiledAbiOp::Sleep),
        Opcode::Yield => CompiledSupportDecision::AbiHelper(CompiledAbiOp::YieldTask),
        Opcode::Spawn => CompiledSupportDecision::AbiHelper(CompiledAbiOp::SpawnFunction),
        Opcode::SpawnClosure => CompiledSupportDecision::AbiHelper(CompiledAbiOp::SpawnClosure),
        Opcode::Call => match operands {
            Operands::Call {
                func_index: 0xFFFF_FFFF,
                ..
            } => CompiledSupportDecision::Unsupported {
                reason: "dynamic Call is unsupported in compiled backends",
            },
            Operands::Call { .. } => {
                CompiledSupportDecision::AbiHelper(CompiledAbiOp::CallFunction)
            }
            _ => CompiledSupportDecision::Unsupported {
                reason: "malformed Call operands",
            },
        },
        Opcode::CallStatic => match operands {
            Operands::Call { .. } => CompiledSupportDecision::AbiHelper(CompiledAbiOp::CallStatic),
            _ => CompiledSupportDecision::Unsupported {
                reason: "malformed CallStatic operands",
            },
        },
        Opcode::CallMethodExact | Opcode::OptionalCallMethodExact => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::CallMethodExact)
        }
        Opcode::CallMethodShape | Opcode::OptionalCallMethodShape => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::CallMethodShape)
        }
        Opcode::CallConstructor => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::CallConstructor)
        }
        Opcode::ConstructType => CompiledSupportDecision::AbiHelper(CompiledAbiOp::ConstructType),
        Opcode::CallSuper => CompiledSupportDecision::AbiHelper(CompiledAbiOp::CallSuper),
        Opcode::GetArgCount => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ArgFrameOp(ArgFrameOp::GetArgCount))
        }
        Opcode::LoadArgLocal => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ArgFrameOp(ArgFrameOp::LoadArgLocal))
        }
        Opcode::BindMethod => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::MemberOp(MemberOp::BindMethod))
        }
        Opcode::Eq => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ValueBinary(ValueBinaryOp::Equal))
        }
        Opcode::Ne => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ValueBinary(ValueBinaryOp::NotEqual))
        }
        Opcode::StrictEq => CompiledSupportDecision::AbiHelper(CompiledAbiOp::ValueBinary(
            ValueBinaryOp::StrictEqual,
        )),
        Opcode::StrictNe => CompiledSupportDecision::AbiHelper(CompiledAbiOp::ValueBinary(
            ValueBinaryOp::StrictNotEqual,
        )),
        Opcode::Typeof => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ValueUnary(ValueUnaryOp::Typeof))
        }
        Opcode::Sconcat => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::StringOp(StringOp::Concat))
        }
        Opcode::Seq => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::StringOp(StringOp::CompareEq))
        }
        Opcode::Sne => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::StringOp(StringOp::CompareNe))
        }
        Opcode::Slt => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::StringOp(StringOp::CompareLt))
        }
        Opcode::Sle => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::StringOp(StringOp::CompareLe))
        }
        Opcode::Sgt => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::StringOp(StringOp::CompareGt))
        }
        Opcode::Sge => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::StringOp(StringOp::CompareGe))
        }
        Opcode::ToString => {
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ValueUnary(ValueUnaryOp::ToString))
        }
        _ => CompiledSupportDecision::NativeLowering,
    }
}

/// Shared function-level compiled support and sync-safety classifier.
#[cfg(feature = "jit")]
pub fn bytecode_function_support(
    backend: CompiledBackendKind,
    module: &Module,
    func_id: usize,
) -> CompiledFunctionSupport {
    bytecode_function_support_inner(backend, module, func_id, &mut FxHashSet::default())
}

#[cfg(feature = "jit")]
fn bytecode_function_support_inner(
    backend: CompiledBackendKind,
    module: &Module,
    func_id: usize,
    visiting: &mut FxHashSet<usize>,
) -> CompiledFunctionSupport {
    if !visiting.insert(func_id) {
        return CompiledFunctionSupport::supported(true, false);
    }
    let Some(func) = module.functions.get(func_id) else {
        return CompiledFunctionSupport::unsupported(false);
    };
    let Ok(instrs) = decode_function(&func.code) else {
        return CompiledFunctionSupport::unsupported(false);
    };

    let mut support = CompiledFunctionSupport::supported(true, false);
    for instr in instrs {
        match instr.opcode {
            Opcode::Call | Opcode::CallStatic => match instr.operands {
                Operands::Call {
                    func_index: 0xFFFF_FFFF,
                    ..
                } => return CompiledFunctionSupport::unsupported(true),
                Operands::Call { func_index, .. } => {
                    let callee = bytecode_function_support_inner(
                        backend,
                        module,
                        func_index as usize,
                        visiting,
                    );
                    if !callee.is_supported {
                        return CompiledFunctionSupport::unsupported(
                            support.may_suspend || callee.may_suspend,
                        );
                    }
                    support.may_suspend |= callee.may_suspend;
                    support.is_sync_safe &= callee.is_sync_safe;
                }
                _ => return CompiledFunctionSupport::unsupported(false),
            },
            _ => {
                let decision = bytecode_instruction_support(backend, instr.opcode, &instr.operands);
                if !decision.is_supported() {
                    return CompiledFunctionSupport::unsupported(
                        support.may_suspend || decision.may_suspend(),
                    );
                }
                if decision.may_suspend() {
                    support.is_sync_safe = false;
                    support.may_suspend = true;
                }
            }
        }
    }

    support
}

#[cfg(all(test, feature = "jit"))]
mod tests {
    use super::*;
    use crate::compiler::bytecode::Function;

    fn test_function(name: &str, code: Vec<u8>) -> Function {
        Function {
            name: name.to_string(),
            param_count: 0,
            uses_js_this_slot: false,
            is_constructible: false,
            is_async: false,
            is_generator: false,
            visible_length: 0,
            is_strict_js: true,
            uses_js_runtime_semantics: false,
            uses_builtin_this_coercion: false,
            js_arguments_mapping: Vec::new(),
            local_count: 0,
            code,
        }
    }

    #[test]
    fn classifies_numeric_intrinsics_as_exact_helpers() {
        assert_eq!(
            bytecode_instruction_support(CompiledBackendKind::Aot, Opcode::Ipow, &Operands::None),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::NumericIntrinsic(
                CompiledNumericIntrinsicOp::I32Pow
            ))
        );
        assert_eq!(
            bytecode_instruction_support(CompiledBackendKind::Jit, Opcode::Fpow, &Operands::None),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::NumericIntrinsic(
                CompiledNumericIntrinsicOp::F64Pow
            ))
        );
        assert_eq!(
            bytecode_instruction_support(CompiledBackendKind::Jit, Opcode::Fmod, &Operands::None),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::NumericIntrinsic(
                CompiledNumericIntrinsicOp::F64Mod
            ))
        );
    }

    #[test]
    fn classifies_argument_and_string_helper_ops_as_exact_helpers() {
        assert_eq!(
            bytecode_instruction_support(
                CompiledBackendKind::Aot,
                Opcode::LoadArgLocal,
                &Operands::None
            ),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ArgFrameOp(ArgFrameOp::LoadArgLocal))
        );
        assert_eq!(
            bytecode_instruction_support(
                CompiledBackendKind::Jit,
                Opcode::GetArgCount,
                &Operands::None
            ),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ArgFrameOp(ArgFrameOp::GetArgCount))
        );
        assert_eq!(
            bytecode_instruction_support(CompiledBackendKind::Jit, Opcode::Typeof, &Operands::None),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ValueUnary(ValueUnaryOp::Typeof))
        );
        assert_eq!(
            bytecode_instruction_support(
                CompiledBackendKind::Aot,
                Opcode::ToString,
                &Operands::None
            ),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::ValueUnary(ValueUnaryOp::ToString))
        );
        assert_eq!(
            bytecode_instruction_support(
                CompiledBackendKind::Aot,
                Opcode::Sconcat,
                &Operands::None
            ),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::StringOp(StringOp::Concat))
        );
        assert_eq!(
            bytecode_instruction_support(
                CompiledBackendKind::Jit,
                Opcode::BindMethod,
                &Operands::None
            ),
            CompiledSupportDecision::AbiHelper(CompiledAbiOp::MemberOp(MemberOp::BindMethod))
        );
    }

    #[test]
    fn computes_recursive_sync_safe_support_once() {
        let mut module = Module::new("compiled-support".to_string());
        module.functions.push(test_function(
            "callee",
            vec![Opcode::Iadd as u8, Opcode::Return as u8],
        ));
        module.functions.push(test_function(
            "caller",
            vec![
                Opcode::CallStatic as u8,
                0,
                0,
                0,
                0,
                0,
                0,
                Opcode::Return as u8,
            ],
        ));

        let support = bytecode_function_support(CompiledBackendKind::Aot, &module, 1);
        assert!(support.is_supported);
        assert!(support.is_sync_safe);
        assert!(!support.may_suspend);
    }
}
