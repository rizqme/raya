//! Register-based call opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::execution::ReturnAction;
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Closure, Object};
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;
use crate::vm::scheduler::Task;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_call_ops(
        &mut self,
        task: &Arc<Task>,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
        extra: u32,
    ) -> RegOpcodeResult {
        let opcode = match instr.opcode() {
            Some(op) => op,
            None => return RegOpcodeResult::error(VmError::InvalidOpcode(instr.opcode_byte())),
        };

        match opcode {
            RegOpcode::Call => {
                // rA = func(rB, rB+1, ..., rB+C-1); extra = func_id
                let func_id = extra as usize;
                let dest_reg = instr.a();
                let arg_base = instr.b();
                let arg_count = instr.c();

                if func_id == 0xFFFFFFFF {
                    // Closure call via Call opcode (legacy path)
                    // Closure is at rB, args at rB+1..rB+C-1
                    let closure_val = match regs.get_reg(reg_base, arg_base) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };

                    if !closure_val.is_ptr() {
                        return RegOpcodeResult::runtime_error("Expected closure");
                    }
                    let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                    let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                    let closure_func_id = closure.func_id();

                    RegOpcodeResult::PushFrame {
                        func_id: closure_func_id,
                        arg_base: arg_base.wrapping_add(1), // skip closure register
                        arg_count: arg_count.saturating_sub(1),
                        dest_reg,
                        is_closure: true,
                        closure_val: Some(closure_val),
                        return_action: ReturnAction::PushReturnValue,
                    }
                } else {
                    // Regular function call
                    RegOpcodeResult::PushFrame {
                        func_id,
                        arg_base,
                        arg_count,
                        dest_reg,
                        is_closure: false,
                        closure_val: None,
                        return_action: ReturnAction::PushReturnValue,
                    }
                }
            }

            RegOpcode::CallClosure => {
                // rA = rB(rB+1, ..., rB+C-1) — closure call (ABC format, no extra)
                let dest_reg = instr.a();
                let closure_reg = instr.b();
                let arg_count = instr.c();

                let closure_val = match regs.get_reg(reg_base, closure_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !closure_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected closure for CallClosure");
                }
                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let closure_func_id = closure.func_id();

                // Args start at rB+1
                RegOpcodeResult::PushFrame {
                    func_id: closure_func_id,
                    arg_base: closure_reg.wrapping_add(1),
                    arg_count,
                    dest_reg,
                    is_closure: true,
                    closure_val: Some(closure_val),
                    return_action: ReturnAction::PushReturnValue,
                }
            }

            RegOpcode::CallMethod => {
                // rA = rB.method(rB+1, ..., rB+C-1); extra = method_idx
                let dest_reg = instr.a();
                let arg_base = instr.b(); // rB = receiver
                let arg_count = instr.c(); // includes receiver
                let method_index = extra as usize;

                // For now, handle user-defined methods via vtable dispatch
                // Built-in methods (array, string, etc.) will be added in later phases

                let receiver_val = match regs.get_reg(reg_base, arg_base) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !receiver_val.is_ptr() {
                    return RegOpcodeResult::runtime_error(
                        "Expected object for method call",
                    );
                }

                let obj_ptr = match unsafe { receiver_val.as_ptr::<Object>() } {
                    Some(p) => p,
                    None => {
                        return RegOpcodeResult::runtime_error(
                            "Expected object for method call",
                        );
                    }
                };
                let obj = unsafe { &*obj_ptr.as_ptr() };

                let classes = self.classes.read();
                let class = match classes.get_class(obj.class_id) {
                    Some(c) => c,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid class ID: {}",
                            obj.class_id
                        ));
                    }
                };

                let function_id = match class.vtable.get_method(method_index) {
                    Some(id) => id,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Method index {} not found in vtable for class '{}' (id={}, vtable_size={})",
                            method_index, class.name, obj.class_id, class.vtable.method_count()
                        ));
                    }
                };
                drop(classes);

                // receiver + args are at rB..rB+C-1
                RegOpcodeResult::PushFrame {
                    func_id: function_id,
                    arg_base,
                    arg_count, // includes receiver as first arg (this)
                    dest_reg,
                    is_closure: false,
                    closure_val: None,
                    return_action: ReturnAction::PushReturnValue,
                }
            }

            RegOpcode::CallConstructor => {
                // rA = new Class(rB, ..., rB+C-1); extra = class_id
                let dest_reg = instr.a();
                let arg_base = instr.b();
                let arg_count = instr.c();
                let class_id = extra as usize;

                // Look up class
                let classes = self.classes.read();
                let class = match classes.get_class(class_id) {
                    Some(c) => c,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid class index: {}",
                            class_id
                        ));
                    }
                };
                let field_count = class.field_count;
                let constructor_id = class.get_constructor();
                drop(classes);

                // Create the object
                let obj = Object::new(class_id, field_count);
                let gc_ptr = self.gc.lock().allocate(obj);
                let obj_val = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                // If no constructor, just store the object
                let constructor_id = match constructor_id {
                    Some(id) => id,
                    None => {
                        if let Err(e) = regs.set_reg(reg_base, dest_reg, obj_val) {
                            return RegOpcodeResult::Error(e);
                        }
                        return RegOpcodeResult::Continue;
                    }
                };

                // Store obj in rA temporarily so it can be passed as first arg (this)
                // The caller must have arranged registers so that rA holds the object
                // and rB..rB+C-1 holds the constructor args.
                // For the callee, r0 = this (obj), r1..rN = args from rB..rB+C-1
                //
                // We store obj_val into a temp location. The PushFrame handler will
                // copy args from caller to callee. We need obj as the first arg.
                // Strategy: store obj in dest_reg, then the call reads (dest_reg, arg_base..arg_base+arg_count-1)
                if let Err(e) = regs.set_reg(reg_base, dest_reg, obj_val) {
                    return RegOpcodeResult::Error(e);
                }

                // The PushFrame handler copies args from caller to callee.
                // For constructors, we need: callee.r0 = obj, callee.r1..rN = caller.rB..rB+C-1
                // We use dest_reg as arg_base for the object, then arg_base for the rest.
                // But PushFrame only supports a contiguous range...
                //
                // Simplest approach: use a special convention where we set obj at dest_reg,
                // and expect the dispatch loop to handle constructor calls specially.
                //
                // Actually, let's just return PushFrame with special handling:
                // We'll copy the object + args in the dispatch loop.
                RegOpcodeResult::PushFrame {
                    func_id: constructor_id,
                    arg_base,        // constructor args start here
                    arg_count,       // number of constructor args (NOT including this)
                    dest_reg,        // rA holds the object (also result destination)
                    is_closure: false,
                    closure_val: None,
                    return_action: ReturnAction::PushObject(obj_val),
                }
            }

            RegOpcode::CallSuper => {
                // super(rB, ..., rB+C-1); extra = class_id (current class)
                let arg_base = instr.b();
                let arg_count = instr.c();
                let class_id = extra as usize;

                let classes = self.classes.read();
                let class = match classes.get_class(class_id) {
                    Some(c) => c,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid class index: {}",
                            class_id
                        ));
                    }
                };

                let parent_id = match class.parent_id {
                    Some(id) => id,
                    None => {
                        return RegOpcodeResult::runtime_error("Class has no parent");
                    }
                };

                let parent_class = match classes.get_class(parent_id) {
                    Some(c) => c,
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid parent class ID: {}",
                            parent_id
                        ));
                    }
                };

                let constructor_id = match parent_class.get_constructor() {
                    Some(id) => id,
                    None => {
                        drop(classes);
                        return RegOpcodeResult::Continue;
                    }
                };
                drop(classes);

                // For super(): receiver (this) is at rB, args at rB+1..rB+C-1
                RegOpcodeResult::PushFrame {
                    func_id: constructor_id,
                    arg_base,
                    arg_count, // includes receiver (this)
                    dest_reg: 0,  // super call discards return value
                    is_closure: false,
                    closure_val: None,
                    return_action: ReturnAction::Discard,
                }
            }

            RegOpcode::CallStatic => {
                // rA = static_method(rB, ..., rB+C-1); extra = func_id
                // Static methods are dispatched directly by function ID
                let dest_reg = instr.a();
                let arg_base = instr.b();
                let arg_count = instr.c();
                let func_id = extra as usize;

                RegOpcodeResult::PushFrame {
                    func_id,
                    arg_base,
                    arg_count,
                    dest_reg,
                    is_closure: false,
                    closure_val: None,
                    return_action: ReturnAction::PushReturnValue,
                }
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not a call opcode: {:?}",
                opcode
            )),
        }
    }
}
