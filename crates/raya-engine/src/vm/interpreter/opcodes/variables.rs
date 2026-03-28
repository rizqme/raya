use crate::compiler::Module;
use crate::compiler::Opcode;
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Object, RayaString, TypeHandle};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::any::TypeId;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_variable_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        task: &Arc<Task>,
        locals_base: usize,
        opcode: Opcode,
        current_arg_count: usize,
        current_arg_values: &[Value],
    ) -> OpcodeResult {
        match opcode {
            Opcode::LoadLocal => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.peek_at(locals_base + index) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base + index, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadLocal0 => {
                let value = match stack.peek_at(locals_base) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadLocal1 => {
                let value = match stack.peek_at(locals_base + 1) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal0 => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreLocal1 => {
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.set_at(locals_base + 1, value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::GetArgCount => {
                // Push the current function's arg_count onto the stack
                if let Err(e) = stack.push(Value::i32(current_arg_count as i32)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadArgLocal => {
                // Pop the index from the stack
                let index_value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let index = match index_value.as_i32() {
                    Some(i) if i >= 0 => i as usize,
                    _ => {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "Invalid local index".to_string(),
                        ))
                    }
                };
                let value = if index < current_arg_count {
                    current_arg_values
                        .get(index)
                        .copied()
                        .unwrap_or(Value::undefined())
                } else {
                    Value::undefined()
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::LoadGlobal => {
                let local_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let index = self.resolve_global_slot(module, local_index);
                if let Some(name) = self.js_global_binding_slots.read().get(&index).cloned() {
                    if let Some(binding) = self.js_global_bindings.read().get(&name).copied() {
                        if !binding.initialized {
                            return OpcodeResult::Error(VmError::ReferenceError(format!(
                                "{} is not defined",
                                name
                            )));
                        }
                    }
                }
                let globals = self.globals_by_index.read();
                let value = globals.get(index).copied().unwrap_or(Value::null());
                drop(globals);
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::StoreGlobal => {
                let local_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let index = self.resolve_global_slot(module, local_index);
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if std::env::var("RAYA_DEBUG_STORE_GLOBALS").is_ok()
                    && module.metadata.name.contains("node_compat/globals.raya")
                {
                    let kind = if value.is_ptr() && !value.is_null() {
                        let raw_ptr = unsafe { value.as_ptr::<u8>() }.map(|ptr| ptr.as_ptr());
                        if let Some(raw_ptr) = raw_ptr {
                            let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr) };
                            if header.type_id() == TypeId::of::<Object>() {
                                "Object"
                            } else if header.type_id() == TypeId::of::<TypeHandle>() {
                                "TypeHandle"
                            } else if header.type_id() == TypeId::of::<RayaString>() {
                                "String"
                            } else {
                                "OtherPtr"
                            }
                        } else {
                            "Ptr?"
                        }
                    } else if value.as_i32().is_some() {
                        "I32"
                    } else if value.as_f64().is_some() {
                        "F64"
                    } else if value.is_null() {
                        "Null"
                    } else if value.is_undefined() {
                        "Undefined"
                    } else {
                        "Other"
                    };
                    eprintln!(
                        "[store-global] module={} local={} absolute={} kind={} ptr={} i32={:?} f64={:?} null={} undef={}",
                        module.metadata.name,
                        local_index,
                        index,
                        kind,
                        value.is_ptr(),
                        value.as_i32(),
                        value.as_f64(),
                        value.is_null(),
                        value.is_undefined(),
                    );
                }
                let mut globals = self.globals_by_index.write();
                if index >= globals.len() {
                    globals.resize(index + 1, Value::null());
                }
                if let Some(name) = self.js_global_binding_slots.read().get(&index).cloned() {
                    if let Some(binding) = self.js_global_bindings.read().get(&name).copied() {
                        if binding.slot != index {
                            let current_value = globals[index];
                            if binding.slot >= globals.len() {
                                globals.resize(binding.slot + 1, Value::null());
                            }
                            if current_value.is_null() && value.is_undefined() {
                                globals[index] = globals[binding.slot];
                            } else {
                                globals[index] = value;
                                globals[binding.slot] = value;
                            }
                        } else {
                            globals[index] = value;
                        }
                    } else {
                        globals[index] = value;
                    }
                } else {
                    globals[index] = value;
                }
                drop(globals);
                if let Some(name) = self.js_global_binding_slots.read().get(&index).cloned() {
                    if std::env::var("RAYA_DEBUG_JS_GLOBAL_BINDINGS").is_ok() {
                        eprintln!(
                            "[js-global:store] name={} slot={} value={:#x}",
                            name,
                            index,
                            value.raw()
                        );
                    }
                    let mut published_to_global_object = false;
                    if let Some(binding) = self.js_global_bindings.write().get_mut(&name) {
                        binding.initialized = true;
                        published_to_global_object = binding.published_to_global_object;
                    }
                    if published_to_global_object {
                        if let Err(error) =
                            self.sync_existing_script_global_property(&name, value, task, module)
                        {
                            return OpcodeResult::Error(error);
                        }
                    }
                }
                OpcodeResult::Continue
            }

            _ => unreachable!("Not a variable opcode: {:?}", opcode),
        }
    }
}
