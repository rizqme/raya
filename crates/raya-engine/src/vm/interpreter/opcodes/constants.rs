use crate::compiler::{Module, Opcode};
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_constant_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::ConstNull => {
                if let Err(e) = stack.push(Value::null()) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstUndefined => {
                if let Err(e) = stack.push(Value::undefined()) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstTrue => {
                if let Err(e) = stack.push(Value::bool(true)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstFalse => {
                if let Err(e) = stack.push(Value::bool(false)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstI32 => {
                let value = match Self::read_i32(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::i32(value)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstF64 => {
                let value = match Self::read_f64(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                if let Err(e) = stack.push(Value::f64(value)) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::ConstStr => {
                let index = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let s = match module.constants.strings.get(index) {
                    Some(s) => s,
                    None => {
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Invalid string constant index: {}",
                            index
                        )));
                    }
                };
                let value = {
                    let key = (module.checksum, index);
                    let debug_const = std::env::var("RAYA_DEBUG_CONSTSTR").is_ok();
                    if debug_const {
                        eprintln!(
                            "[conststr] lookup module={} index={} value={:?}",
                            module.metadata.name, index, s
                        );
                    }
                    let cached = {
                        let cache = self.constant_string_cache.read();
                        cache.get(&key).copied()
                    };
                    if let Some(cached) = cached {
                        if debug_const {
                            eprintln!("[conststr] cache hit");
                        }
                        cached
                    } else {
                        if debug_const {
                            eprintln!("[conststr] cache miss; allocate");
                        }
                        let interned = {
                            let mut gc = self.gc.lock();
                            if debug_const {
                                eprintln!("[conststr] gc locked");
                            }
                            let gc_ptr = gc.allocate(crate::vm::object::RayaString::new(s.clone()));
                            if debug_const {
                                eprintln!("[conststr] allocated");
                            }
                            let value = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            if debug_const {
                                eprintln!("[conststr] rooting ephemeral");
                            }
                            self.ephemeral_gc_roots.write().push(value);
                            if debug_const {
                                eprintln!("[conststr] rooted ephemeral");
                            }
                            value
                        };
                        if debug_const {
                            eprintln!("[conststr] writing cache");
                        }
                        let mut cache = self.constant_string_cache.write();
                        let published = *cache.entry(key).or_insert(interned);
                        if debug_const {
                            eprintln!("[conststr] cache published");
                        }
                        let mut ephemeral = self.ephemeral_gc_roots.write();
                        if let Some(index) =
                            ephemeral.iter().rposition(|candidate| *candidate == interned)
                        {
                            ephemeral.swap_remove(index);
                        }
                        if debug_const {
                            eprintln!("[conststr] ephemeral released");
                        }
                        published
                    }
                };
                if let Err(e) = stack.push(value) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            _ => unreachable!("Not a constant opcode: {:?}", opcode),
        }
    }
}
