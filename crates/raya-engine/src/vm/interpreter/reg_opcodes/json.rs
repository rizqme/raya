//! Register-based JSON opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::compiler::Module;
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::json::{self, JsonValue};
use crate::vm::object::RayaString;
use crate::vm::register_file::RegisterFile;
use crate::vm::value::Value;
use crate::vm::VmError;
use rustc_hash::FxHashMap;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_json_ops(
        &mut self,
        regs: &mut RegisterFile,
        reg_base: usize,
        instr: RegInstr,
        extra: u32,
        module: &Module,
    ) -> RegOpcodeResult {
        let opcode = match instr.opcode() {
            Some(op) => op,
            None => return RegOpcodeResult::error(VmError::InvalidOpcode(instr.opcode_byte())),
        };

        match opcode {
            RegOpcode::JsonGet => {
                // rA = rB[prop]; extra = const_pool_idx (extended)
                let dest_reg = instr.a();
                let obj_reg = instr.b();
                let prop_index = extra;

                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid constant index {} for JSON property",
                            prop_index
                        ));
                    }
                };

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let result = if obj_val.is_null() {
                    Value::null()
                } else if obj_val.is_ptr() {
                    let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        let prop_val = json_val.get_property(&prop_name);
                        json::json_to_value(&prop_val, &mut self.gc.lock())
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, result) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::JsonSet => {
                // rA[prop] = rB; extra = const_pool_idx (extended)
                let obj_reg = instr.a();
                let value_reg = instr.b();
                let prop_index = extra;

                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid constant index {} for JSON property",
                            prop_index
                        ));
                    }
                };

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let value = match regs.get_reg(reg_base, value_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !obj_val.is_ptr() {
                    return RegOpcodeResult::runtime_error(
                        "Expected JSON object for property assignment",
                    );
                }

                let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                if let Some(json_ptr) = ptr {
                    let json_val = unsafe { &*json_ptr.as_ptr() };
                    if let Some(obj_ptr) = json_val.as_object() {
                        let map = unsafe { &mut *obj_ptr.as_ptr() };
                        let new_json_val = json::value_to_json(value, &mut self.gc.lock());
                        map.insert(prop_name, new_json_val);
                    } else {
                        return RegOpcodeResult::runtime_error(
                            "Expected JSON object for property assignment",
                        );
                    }
                } else {
                    return RegOpcodeResult::runtime_error(
                        "Expected JSON object for property assignment",
                    );
                }

                RegOpcodeResult::Continue
            }

            RegOpcode::JsonDelete => {
                // delete rA[prop]; extra = const_pool_idx (extended)
                let obj_reg = instr.a();
                let prop_index = extra;

                let prop_name = match module.constants.get_string(prop_index) {
                    Some(s) => s.to_string(),
                    None => {
                        return RegOpcodeResult::runtime_error(format!(
                            "Invalid constant index {} for JSON property",
                            prop_index
                        ));
                    }
                };

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if obj_val.is_ptr() {
                    let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        if let Some(obj_ptr) = json_val.as_object() {
                            let map = unsafe { &mut *obj_ptr.as_ptr() };
                            map.remove(&prop_name);
                        }
                    }
                }

                RegOpcodeResult::Continue
            }

            RegOpcode::JsonIndex => {
                // rA = rB[rC] (dynamic JSON index)
                let dest_reg = instr.a();
                let obj_reg = instr.b();
                let idx_reg = instr.c();

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let idx_val = match regs.get_reg(reg_base, idx_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let result = if obj_val.is_null() {
                    Value::null()
                } else if obj_val.is_ptr() {
                    let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        // Try numeric index first (for arrays)
                        if let Some(i) = idx_val.as_i32() {
                            let elem = json_val.get_index(i as usize);
                            json::json_to_value(&elem, &mut self.gc.lock())
                        } else if let Some(str_ptr) = unsafe { idx_val.as_ptr::<RayaString>() } {
                            // String key
                            let key = unsafe { &*str_ptr.as_ptr() }.data.as_str();
                            let prop_val = json_val.get_property(key);
                            json::json_to_value(&prop_val, &mut self.gc.lock())
                        } else {
                            Value::null()
                        }
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, result) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::JsonIndexSet => {
                // rA[rB] = rC (dynamic JSON index set)
                let obj_reg = instr.a();
                let idx_reg = instr.b();
                let value_reg = instr.c();

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let idx_val = match regs.get_reg(reg_base, idx_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let value = match regs.get_reg(reg_base, value_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if obj_val.is_ptr() {
                    let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        let new_json_val = json::value_to_json(value, &mut self.gc.lock());

                        if let Some(i) = idx_val.as_i32() {
                            // Array index set
                            if let Some(arr_ptr) = json_val.as_array() {
                                let arr = unsafe { &mut *arr_ptr.as_ptr() };
                                let idx = i as usize;
                                if idx < arr.len() {
                                    arr[idx] = new_json_val;
                                } else {
                                    // Extend with Null values up to the index
                                    while arr.len() <= idx {
                                        arr.push(JsonValue::Null);
                                    }
                                    arr[idx] = new_json_val;
                                }
                            }
                        } else if let Some(str_ptr) = unsafe { idx_val.as_ptr::<RayaString>() } {
                            // Object property set
                            let key = unsafe { &*str_ptr.as_ptr() }.data.clone();
                            if let Some(obj_ptr) = json_val.as_object() {
                                let map = unsafe { &mut *obj_ptr.as_ptr() };
                                map.insert(key, new_json_val);
                            }
                        }
                    }
                }

                RegOpcodeResult::Continue
            }

            RegOpcode::JsonPush => {
                // rA.push(rB) (JSON array push, C unused)
                let arr_reg = instr.a();
                let elem_reg = instr.b();

                let arr_val = match regs.get_reg(reg_base, arr_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let element = match regs.get_reg(reg_base, elem_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if arr_val.is_ptr() {
                    let ptr = unsafe { arr_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        if let Some(arr_ptr) = json_val.as_array() {
                            let arr = unsafe { &mut *arr_ptr.as_ptr() };
                            let json_elem = json::value_to_json(element, &mut self.gc.lock());
                            arr.push(json_elem);
                        }
                    }
                }

                RegOpcodeResult::Continue
            }

            RegOpcode::JsonPop => {
                // rA = rB.pop() (JSON array pop, C unused)
                let dest_reg = instr.a();
                let arr_reg = instr.b();

                let arr_val = match regs.get_reg(reg_base, arr_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let result = if arr_val.is_ptr() {
                    let ptr = unsafe { arr_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        if let Some(arr_ptr) = json_val.as_array() {
                            let arr = unsafe { &mut *arr_ptr.as_ptr() };
                            match arr.pop() {
                                Some(v) => json::json_to_value(&v, &mut self.gc.lock()),
                                None => Value::null(),
                            }
                        } else {
                            Value::null()
                        }
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, result) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::JsonNewObject => {
                // rA = {} (new JSON object, B, C unused)
                let dest_reg = instr.a();

                let map: FxHashMap<String, JsonValue> = FxHashMap::default();
                let map_ptr = self.gc.lock().allocate(map);
                let json_obj = JsonValue::Object(map_ptr);
                let gc_ptr = self.gc.lock().allocate(json_obj);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::JsonNewArray => {
                // rA = [] (new JSON array, B, C unused)
                let dest_reg = instr.a();

                let arr: Vec<JsonValue> = Vec::new();
                let arr_ptr = self.gc.lock().allocate(arr);
                let json_arr = JsonValue::Array(arr_ptr);
                let gc_ptr = self.gc.lock().allocate(json_arr);
                let value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, value) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::JsonKeys => {
                // rA = keys(rB) (C unused)
                let dest_reg = instr.a();
                let obj_reg = instr.b();

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let result = if obj_val.is_ptr() {
                    let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        if let Some(obj_ptr) = json_val.as_object() {
                            let map = unsafe { &*obj_ptr.as_ptr() };
                            // Create an array of key strings
                            let keys: Vec<JsonValue> = map
                                .keys()
                                .map(|k| {
                                    let s = RayaString::new(k.clone());
                                    let gc_ptr = self.gc.lock().allocate(s);
                                    JsonValue::String(gc_ptr)
                                })
                                .collect();
                            let gc_ptr = self.gc.lock().allocate(keys);
                            let arr = JsonValue::Array(gc_ptr);
                            let arr_gc = self.gc.lock().allocate(arr);
                            unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap(),
                                )
                            }
                        } else {
                            Value::null()
                        }
                    } else {
                        Value::null()
                    }
                } else {
                    Value::null()
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, result) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::JsonLength => {
                // rA = rB.length (JSON length, C unused)
                let dest_reg = instr.a();
                let obj_reg = instr.b();

                let obj_val = match regs.get_reg(reg_base, obj_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let length = if obj_val.is_ptr() {
                    let ptr = unsafe { obj_val.as_ptr::<JsonValue>() };
                    if let Some(json_ptr) = ptr {
                        let json_val = unsafe { &*json_ptr.as_ptr() };
                        if let Some(arr_ptr) = json_val.as_array() {
                            let arr = unsafe { &*arr_ptr.as_ptr() };
                            arr.len() as i32
                        } else if let Some(obj_ptr) = json_val.as_object() {
                            let map = unsafe { &*obj_ptr.as_ptr() };
                            map.len() as i32
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };

                if let Err(e) = regs.set_reg(reg_base, dest_reg, Value::i32(length)) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not a JSON opcode: {:?}",
                opcode
            )),
        }
    }
}
