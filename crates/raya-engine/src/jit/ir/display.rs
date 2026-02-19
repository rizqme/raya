//! Pretty-printing for JIT IR
//!
//! Display implementations for debugging and dump output.

use std::fmt;
use super::instr::{JitBlock, JitFunction, JitInstr, JitTerminator, Reg};

impl fmt::Display for JitFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "function @{} [{}] (params: {}, locals: {}) {{",
            self.name, self.func_index, self.param_count, self.local_count)?;

        for block in &self.blocks {
            write!(f, "{}", block)?;
        }

        writeln!(f, "}}")
    }
}

impl fmt::Display for JitBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  {}:", self.id)?;
        if !self.predecessors.is_empty() {
            write!(f, "    ; preds:")?;
            for pred in &self.predecessors {
                write!(f, " {}", pred)?;
            }
            writeln!(f)?;
        }

        for instr in &self.instrs {
            writeln!(f, "    {}", instr)?;
        }

        writeln!(f, "    {}", self.terminator)
    }
}

impl fmt::Display for JitInstr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Constants
            JitInstr::ConstI32 { dest, value } => write!(f, "{} = const.i32 {}", dest, value),
            JitInstr::ConstF64 { dest, value } => write!(f, "{} = const.f64 {}", dest, value),
            JitInstr::ConstBool { dest, value } => write!(f, "{} = const.bool {}", dest, value),
            JitInstr::ConstNull { dest } => write!(f, "{} = const.null", dest),
            JitInstr::ConstString { dest, pool_index } => write!(f, "{} = const.string @{}", dest, pool_index),
            JitInstr::ConstStr { dest, str_index } => write!(f, "{} = const.str #{}", dest, str_index),

            // Int arithmetic
            JitInstr::IAdd { dest, left, right } => write!(f, "{} = iadd {}, {}", dest, left, right),
            JitInstr::ISub { dest, left, right } => write!(f, "{} = isub {}, {}", dest, left, right),
            JitInstr::IMul { dest, left, right } => write!(f, "{} = imul {}, {}", dest, left, right),
            JitInstr::IDiv { dest, left, right } => write!(f, "{} = idiv {}, {}", dest, left, right),
            JitInstr::IMod { dest, left, right } => write!(f, "{} = imod {}, {}", dest, left, right),
            JitInstr::INeg { dest, operand } => write!(f, "{} = ineg {}", dest, operand),
            JitInstr::IPow { dest, left, right } => write!(f, "{} = ipow {}, {}", dest, left, right),
            JitInstr::IShl { dest, left, right } => write!(f, "{} = ishl {}, {}", dest, left, right),
            JitInstr::IShr { dest, left, right } => write!(f, "{} = ishr {}, {}", dest, left, right),
            JitInstr::IUshr { dest, left, right } => write!(f, "{} = iushr {}, {}", dest, left, right),
            JitInstr::IAnd { dest, left, right } => write!(f, "{} = iand {}, {}", dest, left, right),
            JitInstr::IOr { dest, left, right } => write!(f, "{} = ior {}, {}", dest, left, right),
            JitInstr::IXor { dest, left, right } => write!(f, "{} = ixor {}, {}", dest, left, right),
            JitInstr::INot { dest, operand } => write!(f, "{} = inot {}", dest, operand),

            // Float arithmetic
            JitInstr::FAdd { dest, left, right } => write!(f, "{} = fadd {}, {}", dest, left, right),
            JitInstr::FSub { dest, left, right } => write!(f, "{} = fsub {}, {}", dest, left, right),
            JitInstr::FMul { dest, left, right } => write!(f, "{} = fmul {}, {}", dest, left, right),
            JitInstr::FDiv { dest, left, right } => write!(f, "{} = fdiv {}, {}", dest, left, right),
            JitInstr::FNeg { dest, operand } => write!(f, "{} = fneg {}", dest, operand),
            JitInstr::FPow { dest, left, right } => write!(f, "{} = fpow {}, {}", dest, left, right),
            JitInstr::FMod { dest, left, right } => write!(f, "{} = fmod {}, {}", dest, left, right),

            // Int comparison
            JitInstr::ICmpEq { dest, left, right } => write!(f, "{} = icmp.eq {}, {}", dest, left, right),
            JitInstr::ICmpNe { dest, left, right } => write!(f, "{} = icmp.ne {}, {}", dest, left, right),
            JitInstr::ICmpLt { dest, left, right } => write!(f, "{} = icmp.lt {}, {}", dest, left, right),
            JitInstr::ICmpLe { dest, left, right } => write!(f, "{} = icmp.le {}, {}", dest, left, right),
            JitInstr::ICmpGt { dest, left, right } => write!(f, "{} = icmp.gt {}, {}", dest, left, right),
            JitInstr::ICmpGe { dest, left, right } => write!(f, "{} = icmp.ge {}, {}", dest, left, right),

            // Float comparison
            JitInstr::FCmpEq { dest, left, right } => write!(f, "{} = fcmp.eq {}, {}", dest, left, right),
            JitInstr::FCmpNe { dest, left, right } => write!(f, "{} = fcmp.ne {}, {}", dest, left, right),
            JitInstr::FCmpLt { dest, left, right } => write!(f, "{} = fcmp.lt {}, {}", dest, left, right),
            JitInstr::FCmpLe { dest, left, right } => write!(f, "{} = fcmp.le {}, {}", dest, left, right),
            JitInstr::FCmpGt { dest, left, right } => write!(f, "{} = fcmp.gt {}, {}", dest, left, right),
            JitInstr::FCmpGe { dest, left, right } => write!(f, "{} = fcmp.ge {}, {}", dest, left, right),

            // String comparison
            JitInstr::SCmpEq { dest, left, right } => write!(f, "{} = scmp.eq {}, {}", dest, left, right),
            JitInstr::SCmpNe { dest, left, right } => write!(f, "{} = scmp.ne {}, {}", dest, left, right),
            JitInstr::SCmpLt { dest, left, right } => write!(f, "{} = scmp.lt {}, {}", dest, left, right),
            JitInstr::SCmpLe { dest, left, right } => write!(f, "{} = scmp.le {}, {}", dest, left, right),
            JitInstr::SCmpGt { dest, left, right } => write!(f, "{} = scmp.gt {}, {}", dest, left, right),
            JitInstr::SCmpGe { dest, left, right } => write!(f, "{} = scmp.ge {}, {}", dest, left, right),

            // Generic comparison
            JitInstr::Eq { dest, left, right } => write!(f, "{} = eq {}, {}", dest, left, right),
            JitInstr::Ne { dest, left, right } => write!(f, "{} = ne {}, {}", dest, left, right),
            JitInstr::StrictEq { dest, left, right } => write!(f, "{} = strict_eq {}, {}", dest, left, right),
            JitInstr::StrictNe { dest, left, right } => write!(f, "{} = strict_ne {}, {}", dest, left, right),

            // Logical
            JitInstr::Not { dest, operand } => write!(f, "{} = not {}", dest, operand),
            JitInstr::And { dest, left, right } => write!(f, "{} = and {}, {}", dest, left, right),
            JitInstr::Or { dest, left, right } => write!(f, "{} = or {}, {}", dest, left, right),

            // Boxing
            JitInstr::BoxI32 { dest, src } => write!(f, "{} = box.i32 {}", dest, src),
            JitInstr::BoxF64 { dest, src } => write!(f, "{} = box.f64 {}", dest, src),
            JitInstr::BoxBool { dest, src } => write!(f, "{} = box.bool {}", dest, src),
            JitInstr::BoxPtr { dest, src } => write!(f, "{} = box.ptr {}", dest, src),
            JitInstr::UnboxI32 { dest, src } => write!(f, "{} = unbox.i32 {}", dest, src),
            JitInstr::UnboxF64 { dest, src } => write!(f, "{} = unbox.f64 {}", dest, src),
            JitInstr::UnboxBool { dest, src } => write!(f, "{} = unbox.bool {}", dest, src),
            JitInstr::UnboxPtr { dest, src } => write!(f, "{} = unbox.ptr {}", dest, src),

            // Memory
            JitInstr::LoadLocal { dest, index } => write!(f, "{} = load.local {}", dest, index),
            JitInstr::StoreLocal { index, value } => write!(f, "store.local {}, {}", index, value),
            JitInstr::LoadGlobal { dest, index } => write!(f, "{} = load.global {}", dest, index),
            JitInstr::StoreGlobal { index, value } => write!(f, "store.global {}, {}", index, value),
            JitInstr::LoadStatic { dest, index } => write!(f, "{} = load.static {}", dest, index),
            JitInstr::StoreStatic { index, value } => write!(f, "store.static {}, {}", index, value),

            // Object
            JitInstr::NewObject { dest, class_id } => write!(f, "{} = new @{}", dest, class_id),
            JitInstr::LoadField { dest, object, offset } => write!(f, "{} = load.field {}.{}", dest, object, offset),
            JitInstr::StoreField { object, offset, value } => write!(f, "store.field {}.{}, {}", object, offset, value),
            JitInstr::LoadFieldFast { dest, object, offset } => write!(f, "{} = load.field.fast {}.{}", dest, object, offset),
            JitInstr::StoreFieldFast { object, offset, value } => write!(f, "store.field.fast {}.{}, {}", object, offset, value),
            JitInstr::InstanceOf { dest, object, class_id } => write!(f, "{} = instanceof {}, @{}", dest, object, class_id),
            JitInstr::Cast { dest, object, class_id } => write!(f, "{} = cast {}, @{}", dest, object, class_id),
            JitInstr::Typeof { dest, operand } => write!(f, "{} = typeof {}", dest, operand),
            JitInstr::OptionalField { dest, object, offset } => write!(f, "{} = optional.field {}.{}", dest, object, offset),

            // Array
            JitInstr::NewArray { dest, type_index } => write!(f, "{} = newarray @{}", dest, type_index),
            JitInstr::LoadElem { dest, array, index } => write!(f, "{} = load.elem {}[{}]", dest, array, index),
            JitInstr::StoreElem { array, index, value } => write!(f, "store.elem {}[{}], {}", array, index, value),
            JitInstr::ArrayLen { dest, array } => write!(f, "{} = array.len {}", dest, array),
            JitInstr::ArrayPush { array, value } => write!(f, "array.push {}, {}", array, value),
            JitInstr::ArrayPop { dest, array } => write!(f, "{} = array.pop {}", dest, array),
            JitInstr::ArrayLiteral { dest, type_index, elements } => {
                write!(f, "{} = array.literal @{} [", dest, type_index)?;
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", e)?;
                }
                write!(f, "]")
            }
            JitInstr::InitArray { dest, count, elements } => {
                write!(f, "{} = init.array {} [", dest, count)?;
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", e)?;
                }
                write!(f, "]")
            }

            // String
            JitInstr::SConcat { dest, left, right } => write!(f, "{} = sconcat {}, {}", dest, left, right),
            JitInstr::SLen { dest, string } => write!(f, "{} = slen {}", dest, string),
            JitInstr::ToString { dest, value } => write!(f, "{} = tostring {}", dest, value),

            // Calls
            JitInstr::Call { dest, func_index, args } => {
                if let Some(d) = dest { write!(f, "{} = ", d)?; }
                write!(f, "call @{} (", func_index)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }
            JitInstr::CallMethod { dest, method_index, receiver, args } => {
                if let Some(d) = dest { write!(f, "{} = ", d)?; }
                write!(f, "call.method {}.@{} (", receiver, method_index)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }
            JitInstr::CallConstructor { dest, class_id, args } => {
                write!(f, "{} = call.constructor @{} (", dest, class_id)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }
            JitInstr::CallSuper { dest, method_index, args } => {
                if let Some(d) = dest { write!(f, "{} = ", d)?; }
                write!(f, "call.super @{} (", method_index)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }
            JitInstr::CallStatic { dest, func_index, args } => {
                if let Some(d) = dest { write!(f, "{} = ", d)?; }
                write!(f, "call.static @{} (", func_index)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }
            JitInstr::CallNative { dest, native_id, args } => {
                if let Some(d) = dest { write!(f, "{} = ", d)?; }
                write!(f, "call.native #{:#06x} (", native_id)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }
            JitInstr::CallClosure { dest, closure, args } => {
                if let Some(d) = dest { write!(f, "{} = ", d)?; }
                write!(f, "call.closure {} (", closure)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }

            // Closures
            JitInstr::MakeClosure { dest, func_index, captures } => {
                write!(f, "{} = make.closure @{} [", dest, func_index)?;
                format_args_list(f, captures)?;
                write!(f, "]")
            }
            JitInstr::LoadCaptured { dest, index } => write!(f, "{} = load.captured {}", dest, index),
            JitInstr::StoreCaptured { index, value } => write!(f, "store.captured {}, {}", index, value),
            JitInstr::SetClosureCapture { closure, index, value } => write!(f, "set.capture {}.{}, {}", closure, index, value),
            JitInstr::CloseVar { index } => write!(f, "close.var {}", index),

            // RefCell
            JitInstr::NewRefCell { dest, value } => write!(f, "{} = new.refcell {}", dest, value),
            JitInstr::LoadRefCell { dest, cell } => write!(f, "{} = load.refcell {}", dest, cell),
            JitInstr::StoreRefCell { cell, value } => write!(f, "store.refcell {}, {}", cell, value),

            // Concurrency
            JitInstr::Spawn { dest, func_index, args } => {
                write!(f, "{} = spawn @{} (", dest, func_index)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }
            JitInstr::SpawnClosure { dest, closure, args } => {
                write!(f, "{} = spawn.closure {} (", dest, closure)?;
                format_args_list(f, args)?;
                write!(f, ")")
            }
            JitInstr::Await { dest, task } => write!(f, "{} = await {}", dest, task),
            JitInstr::Yield => write!(f, "yield"),
            JitInstr::Sleep { duration } => write!(f, "sleep {}", duration),
            JitInstr::NewMutex { dest } => write!(f, "{} = new.mutex", dest),
            JitInstr::MutexLock { mutex } => write!(f, "mutex.lock {}", mutex),
            JitInstr::MutexUnlock { mutex } => write!(f, "mutex.unlock {}", mutex),
            JitInstr::NewChannel { dest } => write!(f, "{} = new.channel", dest),
            JitInstr::NewSemaphore { dest } => write!(f, "{} = new.semaphore", dest),
            JitInstr::SemAcquire { sem } => write!(f, "sem.acquire {}", sem),
            JitInstr::SemRelease { sem } => write!(f, "sem.release {}", sem),
            JitInstr::WaitAll { dest, tasks } => write!(f, "{} = wait.all {}", dest, tasks),
            JitInstr::TaskCancel { task } => write!(f, "task.cancel {}", task),
            JitInstr::TaskThen { task, callback_index } => write!(f, "task.then {}, @{}", task, callback_index),

            // Object/Tuple literals
            JitInstr::ObjectLiteral { dest, type_index, fields } => {
                write!(f, "{} = object.literal @{} {{", dest, type_index)?;
                format_args_list(f, fields)?;
                write!(f, "}}")
            }
            JitInstr::TupleLiteral { dest, type_index, elements } => {
                write!(f, "{} = tuple.literal @{} (", dest, type_index)?;
                format_args_list(f, elements)?;
                write!(f, ")")
            }
            JitInstr::TupleGet { dest, tuple } => write!(f, "{} = tuple.get {}", dest, tuple),
            JitInstr::InitObject { dest, count, fields } => {
                write!(f, "{} = init.object {} {{", dest, count)?;
                format_args_list(f, fields)?;
                write!(f, "}}")
            }
            JitInstr::InitTuple { dest, count, elements } => {
                write!(f, "{} = init.tuple {} (", dest, count)?;
                format_args_list(f, elements)?;
                write!(f, ")")
            }

            // Module
            JitInstr::LoadModule { dest, module_index } => write!(f, "{} = load.module @{}", dest, module_index),
            JitInstr::LoadConst { dest, const_index } => write!(f, "{} = load.const @{}", dest, const_index),

            // JSON
            JitInstr::JsonGet { dest, object, key_index } => write!(f, "{} = json.get {}.@{}", dest, object, key_index),
            JitInstr::JsonSet { object, key_index, value } => write!(f, "json.set {}.@{}, {}", object, key_index, value),
            JitInstr::JsonDelete { object, key_index } => write!(f, "json.delete {}.@{}", object, key_index),
            JitInstr::JsonIndex { dest, object, index } => write!(f, "{} = json.index {}[{}]", dest, object, index),
            JitInstr::JsonIndexSet { object, index, value } => write!(f, "json.index_set {}[{}], {}", object, index, value),
            JitInstr::JsonPush { array, value } => write!(f, "json.push {}, {}", array, value),
            JitInstr::JsonPop { dest, array } => write!(f, "{} = json.pop {}", dest, array),
            JitInstr::JsonNewObject { dest } => write!(f, "{} = json.new_object", dest),
            JitInstr::JsonNewArray { dest } => write!(f, "{} = json.new_array", dest),
            JitInstr::JsonKeys { dest, object } => write!(f, "{} = json.keys {}", dest, object),
            JitInstr::JsonLength { dest, object } => write!(f, "{} = json.length {}", dest, object),

            // Runtime
            JitInstr::GcSafepoint => write!(f, "gc.safepoint"),
            JitInstr::CheckPreemption => write!(f, "check.preemption"),

            // SSA
            JitInstr::Phi { dest, sources } => {
                write!(f, "{} = phi", dest)?;
                for (i, (block, reg)) in sources.iter().enumerate() {
                    if i > 0 { write!(f, ",")?; }
                    write!(f, " [{}:{}]", block, reg)?;
                }
                Ok(())
            }
            JitInstr::Move { dest, src } => write!(f, "{} = move {}", dest, src),

            // Exception handling
            JitInstr::SetupTry { catch_block, finally_block } => {
                write!(f, "setup.try catch={}", catch_block)?;
                if let Some(fb) = finally_block {
                    write!(f, " finally={}", fb)?;
                }
                Ok(())
            }
            JitInstr::EndTry => write!(f, "end.try"),
            JitInstr::Throw { value } => write!(f, "throw {}", value),
            JitInstr::Rethrow => write!(f, "rethrow"),
        }
    }
}

impl fmt::Display for JitTerminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JitTerminator::Jump(target) => write!(f, "jmp {}", target),
            JitTerminator::Branch { cond, then_block, else_block } =>
                write!(f, "br {}, {}, {}", cond, then_block, else_block),
            JitTerminator::BranchNull { value, null_block, not_null_block } =>
                write!(f, "br.null {}, {}, {}", value, null_block, not_null_block),
            JitTerminator::Return(Some(reg)) => write!(f, "ret {}", reg),
            JitTerminator::Return(None) => write!(f, "ret void"),
            JitTerminator::Throw(reg) => write!(f, "throw {}", reg),
            JitTerminator::Unreachable => write!(f, "unreachable"),
            JitTerminator::Deoptimize { reason, state } =>
                write!(f, "deoptimize {:?} @{}", reason, state.bytecode_offset),
            JitTerminator::None => write!(f, "<no terminator>"),
        }
    }
}

fn format_args_list(f: &mut fmt::Formatter<'_>, args: &[Reg]) -> fmt::Result {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 { write!(f, ", ")?; }
        write!(f, "{}", arg)?;
    }
    Ok(())
}
