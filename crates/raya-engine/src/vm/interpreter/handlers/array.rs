//! Array built-in method handlers

use crate::compiler::Module;
use crate::vm::interpreter::core::value_to_f64;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{layout_id_from_ordered_names, Array, Object, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::ptr::NonNull;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    fn array_release_ephemeral_root(&self, value: Value) {
        let mut roots = self.ephemeral_gc_roots.write();
        if let Some(index) = roots.iter().rposition(|candidate| *candidate == value) {
            roots.swap_remove(index);
        }
    }

    fn array_callback_this_arg(optional_this: Option<Value>) -> Value {
        optional_this.unwrap_or(Value::undefined())
    }

    fn array_integer_argument(&self, value: Value) -> Result<i64, VmError> {
        let number = if value.is_undefined() || value.is_null() {
            0.0
        } else if let Some(boolean) = value.as_bool() {
            if boolean {
                1.0
            } else {
                0.0
            }
        } else {
            value_to_f64(value)?
        };
        if !number.is_finite() || number == 0.0 {
            return Ok(0);
        }
        let truncated = if number.is_sign_negative() {
            number.ceil()
        } else {
            number.floor()
        };
        if truncated <= i64::MIN as f64 {
            Ok(i64::MIN)
        } else if truncated >= i64::MAX as f64 {
            Ok(i64::MAX)
        } else {
            Ok(truncated as i64)
        }
    }

    fn array_sort_compare_default(&self, left: Value, right: Value) -> i32 {
        let left_string = self.value_to_js_string(left);
        let right_string = self.value_to_js_string(right);
        match left_string.cmp(&right_string) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    fn value_to_js_string(&self, value: Value) -> String {
        if let Some(ptr) = crate::vm::interpreter::opcodes::native::checked_string_ptr(value) {
            return unsafe { &*ptr.as_ptr() }.data.clone();
        }
        if let Some(i) = value.as_i32() {
            return i.to_string();
        }
        if let Some(f) = value.as_f64() {
            return f.to_string();
        }
        if let Some(b) = value.as_bool() {
            return b.to_string();
        }
        if value.is_undefined() {
            return "undefined".to_string();
        }
        if value.is_null() {
            return "null".to_string();
        }
        String::new()
    }

    fn array_search_values_strict_equal(&self, left: Value, right: Value) -> bool {
        if left == right {
            return true;
        }
        let left_string = crate::vm::interpreter::opcodes::native::checked_string_ptr(left);
        let right_string = crate::vm::interpreter::opcodes::native::checked_string_ptr(right);
        if let (Some(left_ptr), Some(right_ptr)) = (left_string, right_string) {
            let left_string = unsafe { &*left_ptr.as_ptr() };
            let right_string = unsafe { &*right_ptr.as_ptr() };
            return left_string.data == right_string.data;
        }
        false
    }

    fn array_search_values_same_value_zero(&self, left: Value, right: Value) -> bool {
        if self.array_search_values_strict_equal(left, right) {
            return true;
        }
        let left_number = value_to_f64(left).ok();
        let right_number = value_to_f64(right).ok();
        matches!((left_number, right_number), (Some(a), Some(b)) if a.is_nan() && b.is_nan())
    }

    fn array_like_string_primitive(&self, value: Value) -> Option<Value> {
        if crate::vm::interpreter::opcodes::native::checked_string_ptr(value).is_some() {
            return Some(value);
        }
        self.boxed_primitive_internal_value(value, "String")
    }

    fn array_like_length_with_context(
        &mut self,
        value: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<usize, VmError> {
        if value.is_null() || value.is_undefined() {
            return Err(VmError::TypeError(
                "Array method called on null or undefined".to_string(),
            ));
        }
        if let Some(array_ptr) = crate::vm::interpreter::opcodes::native::checked_array_ptr(value) {
            let array = unsafe { &*array_ptr.as_ptr() };
            return Ok(array.len());
        }
        if let Some(string_value) = self.array_like_string_primitive(value) {
            let string_ptr =
                crate::vm::interpreter::opcodes::native::checked_string_ptr(string_value)
                    .expect("string primitive");
            let string = unsafe { &*string_ptr.as_ptr() };
            return Ok(string.data.chars().count());
        }

        let length_value = self
            .get_property_value_via_js_semantics_with_context(value, "length", task, module)?
            .unwrap_or(Value::undefined());
        if length_value.is_undefined() || length_value.is_null() {
            return Ok(0);
        }
        let numeric = self.js_to_number_with_context(length_value, task, module)?;
        if !numeric.is_finite() || numeric <= 0.0 {
            return Ok(0);
        }
        Ok(numeric.floor().min(u32::MAX as f64) as usize)
    }

    fn array_like_has_index_with_context(
        &self,
        value: Value,
        index: usize,
    ) -> bool {
        let key = index.to_string();
        if self.has_property_via_js_semantics(value, &key) {
            return true;
        }
        if let Some(array_ptr) = crate::vm::interpreter::opcodes::native::checked_array_ptr(value) {
            let array = unsafe { &*array_ptr.as_ptr() };
            return array.get(index).is_some();
        }
        if let Some(string_value) = self.array_like_string_primitive(value) {
            let string_ptr =
                crate::vm::interpreter::opcodes::native::checked_string_ptr(string_value)
                    .expect("string primitive");
            let string = unsafe { &*string_ptr.as_ptr() };
            return string.data.chars().nth(index).is_some();
        }
        false
    }

    fn array_like_index_value_with_context(
        &mut self,
        value: Value,
        index: usize,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        if let Some(property_value) =
            self.get_property_value_via_js_semantics_with_context(
                value,
                &index.to_string(),
                task,
                module,
            )?
        {
            return Ok(property_value);
        }
        if let Some(array_ptr) = crate::vm::interpreter::opcodes::native::checked_array_ptr(value) {
            let array = unsafe { &*array_ptr.as_ptr() };
            return Ok(array.get(index).unwrap_or(Value::undefined()));
        }
        if let Some(string_value) = self.array_like_string_primitive(value) {
            let string_ptr =
                crate::vm::interpreter::opcodes::native::checked_string_ptr(string_value)
                    .expect("string primitive");
            let string = unsafe { &*string_ptr.as_ptr() };
            if let Some(ch) = string.data.chars().nth(index) {
                let ptr = self.gc.lock().allocate(RayaString::new(ch.to_string()));
                return Ok(unsafe {
                    Value::from_ptr(NonNull::new(ptr.as_ptr()).expect("string char ptr"))
                });
            }
            return Ok(Value::undefined());
        }
        Ok(self
            .get_property_value_via_js_semantics_with_context(value, &index.to_string(), task, module)?
            .unwrap_or(Value::undefined()))
    }

    fn array_from_constructor_target(
        &mut self,
        constructor: Value,
        init_args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        self.construct_value_with_new_target(constructor, constructor, init_args, task, module)
    }

    fn array_from_is_object_value(&self, value: Value) -> bool {
        crate::vm::interpreter::opcodes::native::checked_array_ptr(value).is_some()
            || crate::vm::interpreter::opcodes::native::checked_object_ptr(value).is_some()
            || self.callable_function_info(value).is_some()
    }

    fn array_from_is_constructor_candidate(&self, value: Value, module: &Module) -> bool {
        let _ = module;
        self.callable_is_constructible(value)
    }

    fn array_from_iterator_method(
        &mut self,
        items: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let debug_array_from = std::env::var("RAYA_DEBUG_ARRAY_FROM").is_ok();
        let Some(iterator_method) =
            self.well_known_symbol_property_value(items, "Symbol.iterator", task, module)?
        else {
            if debug_array_from {
                eprintln!("[array.from] no @@iterator on items={:#x}", items.raw());
            }
            return Ok(None);
        };
        if debug_array_from {
            eprintln!(
                "[array.from] @@iterator items={:#x} method={:#x} callable={}",
                items.raw(),
                iterator_method.raw(),
                Self::is_callable_value(iterator_method)
            );
        }
        if iterator_method.is_undefined() || iterator_method.is_null() {
            return Ok(None);
        }
        if !Self::is_callable_value(iterator_method) {
            return Err(VmError::TypeError(
                "Array.from iterator method is not callable".to_string(),
            ));
        }
        Ok(Some(iterator_method))
    }

    fn array_from_length_of_array_like(&self, items: Value) -> Result<usize, VmError> {
        if let Some(array_ptr) = crate::vm::interpreter::opcodes::native::checked_array_ptr(items) {
            let array = unsafe { &*array_ptr.as_ptr() };
            return Ok(array.len());
        }
        if let Some(string_ptr) = crate::vm::interpreter::opcodes::native::checked_string_ptr(items)
        {
            let string = unsafe { &*string_ptr.as_ptr() };
            return Ok(string.data.chars().count());
        }

        let length_value = self
            .get_field_value_by_name(items, "length")
            .unwrap_or(Value::undefined());
        if length_value.is_undefined() || length_value.is_null() {
            return Ok(0);
        }
        let numeric = if let Some(boolean) = length_value.as_bool() {
            if boolean {
                1.0
            } else {
                0.0
            }
        } else {
            value_to_f64(length_value)?
        };
        if !numeric.is_finite() || numeric <= 0.0 {
            return Ok(0);
        }
        Ok(numeric.floor().min(u32::MAX as f64) as usize)
    }

    fn array_from_index_value(&self, items: Value, index: usize) -> Value {
        if let Some(array_ptr) = crate::vm::interpreter::opcodes::native::checked_array_ptr(items) {
            let array = unsafe { &*array_ptr.as_ptr() };
            return array.get(index).unwrap_or(Value::undefined());
        }
        if let Some(string_ptr) = crate::vm::interpreter::opcodes::native::checked_string_ptr(items)
        {
            let string = unsafe { &*string_ptr.as_ptr() };
            if let Some(ch) = string.data.chars().nth(index) {
                let ptr = self.gc.lock().allocate(RayaString::new(ch.to_string()));
                return unsafe {
                    Value::from_ptr(NonNull::new(ptr.as_ptr()).expect("string char ptr"))
                };
            }
            return Value::undefined();
        }
        self.get_field_value_by_name(items, &index.to_string())
            .unwrap_or(Value::undefined())
    }

    fn array_from_define_index(
        &mut self,
        target: Value,
        index: usize,
        value: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        if let Some(array_ptr) = crate::vm::interpreter::opcodes::native::checked_array_ptr(target)
        {
            let array = unsafe { &mut *array_ptr.as_ptr() };
            array.set(index, value).map_err(VmError::RuntimeError)?;
            return Ok(());
        }
        self.define_data_property_on_target_with_context(
            target,
            &index.to_string(),
            value,
            true,
            true,
            true,
            task,
            module,
        )
    }

    fn array_from_set_length(
        &mut self,
        target: Value,
        len: usize,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let length_value = if len <= i32::MAX as usize {
            Value::i32(len as i32)
        } else {
            Value::f64(len as f64)
        };
        let updated = self.set_property_value_via_js_semantics(
            target,
            "length",
            length_value,
            target,
            task,
            module,
        )?;
        if updated {
            Ok(())
        } else {
            Err(VmError::TypeError(
                "Cannot assign to non-writable property 'length'".to_string(),
            ))
        }
    }

    fn array_from_iterator_close(
        &mut self,
        iterator: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let Some(return_method) = self.get_field_value_by_name(iterator, "return") else {
            return Ok(());
        };
        if return_method.is_undefined() || return_method.is_null() {
            return Ok(());
        }
        if !Self::is_callable_value(return_method) {
            return Err(VmError::TypeError(
                "Array.from iterator return is not callable".to_string(),
            ));
        }
        let _ =
            self.invoke_callable_sync_with_this(return_method, Some(iterator), &[], task, module)?;
        Ok(())
    }

    fn array_from_iterator_step(
        &mut self,
        iterator: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let debug_array_from = std::env::var("RAYA_DEBUG_ARRAY_FROM").is_ok();
        let Some(next_method) = self.get_field_value_by_name(iterator, "next") else {
            if debug_array_from {
                eprintln!(
                    "[array.from] iterator missing next iterator={:#x} proto={:?}",
                    iterator.raw(),
                    self.prototype_of_value(iterator)
                        .map(|v| format!("{:#x}", v.raw()))
                );
            }
            return Err(VmError::TypeError(
                "Array.from iterator is missing next()".to_string(),
            ));
        };
        if debug_array_from {
            eprintln!(
                "[array.from] iterator next iterator={:#x} next={:#x}",
                iterator.raw(),
                next_method.raw()
            );
        }
        if !Self::is_callable_value(next_method) {
            return Err(VmError::TypeError(
                "Array.from iterator next is not callable".to_string(),
            ));
        }
        let next_result =
            self.invoke_callable_sync_with_this(next_method, Some(iterator), &[], task, module)?;
        if debug_array_from {
            eprintln!(
                "[array.from] iterator next-result raw={:#x} object={} array={} callable={} null={} undefined={}",
                next_result.raw(),
                crate::vm::interpreter::opcodes::native::checked_object_ptr(next_result).is_some(),
                crate::vm::interpreter::opcodes::native::checked_array_ptr(next_result).is_some(),
                self.callable_function_info(next_result).is_some(),
                next_result.is_null(),
                next_result.is_undefined()
            );
        }
        if !self.array_from_is_object_value(next_result) {
            return Err(VmError::TypeError(
                "Array.from iterator result must be an object".to_string(),
            ));
        }
        let done_value =
            self.array_from_iterator_result_property(next_result, "done", task, module)?;
        let done = if let Some(boolean) = done_value.as_bool() {
            boolean
        } else {
            !done_value.is_null() && !done_value.is_undefined()
        };
        if done {
            return Ok(None);
        }
        Ok(Some(self.array_from_iterator_result_property(
            next_result,
            "value",
            task,
            module,
        )?))
    }

    fn array_from_iterator_result_property(
        &mut self,
        result: Value,
        key: &str,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        if let Some(getter) = self.descriptor_accessor(result, key, "get") {
            if !Self::is_callable_value(getter) {
                return Err(VmError::TypeError(format!(
                    "Array.from iterator result '{}' getter is not callable",
                    key
                )));
            }
            return self.invoke_callable_sync_with_this(getter, Some(result), &[], task, module);
        }
        Ok(self
            .get_field_value_by_name(result, key)
            .unwrap_or(Value::undefined()))
    }

    fn array_from_target(
        &mut self,
        constructor: Value,
        init_args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        if self.array_from_is_constructor_candidate(constructor, module) {
            return self.array_from_constructor_target(constructor, init_args, task, module);
        }
        let array_ptr = self.gc.lock().allocate(Array::new(0, 0));
        Ok(unsafe { Value::from_ptr(NonNull::new(array_ptr.as_ptr()).expect("array ptr")) })
    }

    /// Handle built-in array methods
    pub(in crate::vm::interpreter) fn call_array_method(
        &mut self,
        task: &Arc<Task>,
        stack: &mut Stack,
        method_id: u16,
        arg_count: usize,
        module: &Module,
    ) -> Result<(), VmError> {
        use crate::vm::builtin::array;

        match method_id {
            array::FROM => {
                if arg_count == 0 || arg_count > 3 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.from expects 1-3 arguments, got {}",
                        arg_count
                    )));
                }

                let this_arg = if arg_count == 3 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let map_fn_provided = arg_count >= 2;
                let map_fn = if map_fn_provided {
                    stack.pop()?
                } else {
                    Value::undefined()
                };
                let items = stack.pop()?;
                let constructor = stack.pop()?;
                if constructor.is_heap_allocated() {
                    self.ephemeral_gc_roots.write().push(constructor);
                }
                if items.is_heap_allocated() {
                    self.ephemeral_gc_roots.write().push(items);
                }

                if items.is_null() || items.is_undefined() {
                    self.array_release_ephemeral_root(items);
                    self.array_release_ephemeral_root(constructor);
                    return Err(VmError::TypeError(
                        "Array.from requires an iterable or array-like value".to_string(),
                    ));
                }

                if map_fn.is_heap_allocated() {
                    self.ephemeral_gc_roots.write().push(map_fn);
                }
                if map_fn_provided && !map_fn.is_undefined() && !Self::is_callable_value(map_fn) {
                    self.array_release_ephemeral_root(map_fn);
                    self.array_release_ephemeral_root(items);
                    self.array_release_ephemeral_root(constructor);
                    return Err(VmError::TypeError(
                        "Array.from mapfn must be callable".to_string(),
                    ));
                }

                let map_this = Self::array_callback_this_arg(this_arg);
                if map_this.is_heap_allocated() {
                    self.ephemeral_gc_roots.write().push(map_this);
                }
                if let Some(iterator_method) =
                    self.array_from_iterator_method(items, task, module)?
                {
                    let iterator = self.invoke_callable_sync_with_this(
                        iterator_method,
                        Some(items),
                        &[],
                        task,
                        module,
                    )?;
                    if std::env::var("RAYA_DEBUG_ARRAY_FROM").is_ok() {
                        eprintln!(
                            "[array.from] iterator call method={:#x} result={:#x} proto={:?}",
                            iterator_method.raw(),
                            iterator.raw(),
                            self.prototype_of_value(iterator)
                                .map(|v| format!("{:#x}", v.raw()))
                        );
                    }
                    if iterator.is_heap_allocated() {
                        self.ephemeral_gc_roots.write().push(iterator);
                    }
                    let target = self.array_from_target(constructor, &[], task, module)?;
                    if target.is_heap_allocated() {
                        self.ephemeral_gc_roots.write().push(target);
                    }
                    let mut index = 0usize;
                    loop {
                        let next_value = match self.array_from_iterator_step(iterator, task, module)
                        {
                            Ok(Some(value)) => value,
                            Ok(None) => break,
                            Err(error) => {
                                let _ = self.array_from_iterator_close(iterator, task, module);
                                self.array_release_ephemeral_root(target);
                                self.array_release_ephemeral_root(iterator);
                                self.array_release_ephemeral_root(map_this);
                                self.array_release_ephemeral_root(map_fn);
                                self.array_release_ephemeral_root(items);
                                self.array_release_ephemeral_root(constructor);
                                return Err(error);
                            }
                        };
                        let mapped_value = if map_fn_provided && !map_fn.is_undefined() {
                            let index_value = if index <= i32::MAX as usize {
                                Value::i32(index as i32)
                            } else {
                                Value::f64(index as f64)
                            };
                            match self.invoke_callable_sync_with_this(
                                map_fn,
                                Some(map_this),
                                &[next_value, index_value],
                                task,
                                module,
                            ) {
                                Ok(value) => value,
                                Err(error) => {
                                    let _ = self.array_from_iterator_close(iterator, task, module);
                                    self.array_release_ephemeral_root(target);
                                    self.array_release_ephemeral_root(iterator);
                                    self.array_release_ephemeral_root(map_this);
                                    self.array_release_ephemeral_root(map_fn);
                                    self.array_release_ephemeral_root(items);
                                    self.array_release_ephemeral_root(constructor);
                                    return Err(error);
                                }
                            }
                        } else {
                            next_value
                        };
                        if mapped_value.is_heap_allocated() {
                            self.ephemeral_gc_roots.write().push(mapped_value);
                        }
                        if let Err(error) =
                            self.array_from_define_index(target, index, mapped_value, task, module)
                        {
                            let _ = self.array_from_iterator_close(iterator, task, module);
                            self.array_release_ephemeral_root(mapped_value);
                            self.array_release_ephemeral_root(target);
                            self.array_release_ephemeral_root(iterator);
                            self.array_release_ephemeral_root(map_this);
                            self.array_release_ephemeral_root(map_fn);
                            self.array_release_ephemeral_root(items);
                            self.array_release_ephemeral_root(constructor);
                            return Err(error);
                        }
                        self.array_release_ephemeral_root(mapped_value);
                        index += 1;
                    }
                    self.array_from_set_length(target, index, task, module)?;
                    stack.push(target)?;
                    self.array_release_ephemeral_root(target);
                    self.array_release_ephemeral_root(iterator);
                    self.array_release_ephemeral_root(map_this);
                    self.array_release_ephemeral_root(map_fn);
                    self.array_release_ephemeral_root(items);
                    self.array_release_ephemeral_root(constructor);
                    return Ok(());
                }

                let len = self.array_from_length_of_array_like(items)?;
                let len_value = if len <= i32::MAX as usize {
                    Value::i32(len as i32)
                } else {
                    Value::f64(len as f64)
                };
                let target = self.array_from_target(constructor, &[len_value], task, module)?;
                if target.is_heap_allocated() {
                    self.ephemeral_gc_roots.write().push(target);
                }
                for index in 0..len {
                    let value = self.array_from_index_value(items, index);
                    let mapped_value = if map_fn_provided && !map_fn.is_undefined() {
                        let index_value = if index <= i32::MAX as usize {
                            Value::i32(index as i32)
                        } else {
                            Value::f64(index as f64)
                        };
                        self.invoke_callable_sync_with_this(
                            map_fn,
                            Some(map_this),
                            &[value, index_value],
                            task,
                            module,
                        )?
                    } else {
                        value
                    };
                    if mapped_value.is_heap_allocated() {
                        self.ephemeral_gc_roots.write().push(mapped_value);
                    }
                    if let Err(error) =
                        self.array_from_define_index(target, index, mapped_value, task, module)
                    {
                        self.array_release_ephemeral_root(mapped_value);
                        self.array_release_ephemeral_root(target);
                        self.array_release_ephemeral_root(map_this);
                        self.array_release_ephemeral_root(map_fn);
                        self.array_release_ephemeral_root(items);
                        self.array_release_ephemeral_root(constructor);
                        return Err(error);
                    }
                    self.array_release_ephemeral_root(mapped_value);
                }
                self.array_from_set_length(target, len, task, module)?;
                stack.push(target)?;
                self.array_release_ephemeral_root(target);
                self.array_release_ephemeral_root(map_this);
                self.array_release_ephemeral_root(map_fn);
                self.array_release_ephemeral_root(items);
                self.array_release_ephemeral_root(constructor);
                Ok(())
            }
            array::PUSH => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.push expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let debug_native_stack = std::env::var("RAYA_DEBUG_NATIVE_STACK").is_ok();
                if debug_native_stack {
                    eprintln!(
                        "[array.push] pre-pop stack_depth={} arg_count={}",
                        stack.depth(),
                        arg_count
                    );
                }
                let value = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => {
                        if debug_native_stack {
                            eprintln!(
                                "[array.push] pop value underflow stack_depth={}",
                                stack.depth()
                            );
                        }
                        return Err(e);
                    }
                };
                let array_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => {
                        if debug_native_stack {
                            eprintln!(
                                "[array.push] pop receiver underflow stack_depth={}",
                                stack.depth()
                            );
                        }
                        return Err(e);
                    }
                };
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let new_len = arr.push(value);
                stack.push(Value::i32(new_len as i32))?;
                Ok(())
            }
            array::POP => {
                if arg_count != 0 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.pop expects 0 arguments, got {}",
                        arg_count
                    )));
                }
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let result = arr.pop().unwrap_or(Value::null());
                stack.push(result)?;
                Ok(())
            }
            array::SHIFT => {
                if arg_count != 0 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.shift expects 0 arguments, got {}",
                        arg_count
                    )));
                }
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let result = arr.shift().unwrap_or(Value::null());
                stack.push(result)?;
                Ok(())
            }
            array::UNSHIFT => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.unshift expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let value = stack.pop()?;
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                let new_len = arr.unshift(value);
                stack.push(Value::i32(new_len as i32))?;
                Ok(())
            }
            array::INDEX_OF => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.indexOf expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let from_index = if arg_count == 2 {
                    Some(self.array_integer_argument(stack.pop()?)?)
                } else {
                    None
                };
                let value = stack.pop()?;
                let array_val = stack.pop()?;
                let len = self.array_like_length_with_context(array_val, task, module)?;
                let start = if let Some(from_index) = from_index {
                    if from_index >= 0 {
                        (from_index as usize).min(len)
                    } else {
                        len.saturating_sub(from_index.unsigned_abs() as usize)
                    }
                } else {
                    0
                };
                let mut result: i32 = -1;
                for index in start..len {
                    if !self.array_like_has_index_with_context(array_val, index) {
                        continue;
                    }
                    let candidate =
                        self.array_like_index_value_with_context(array_val, index, task, module)?;
                    if self.array_search_values_strict_equal(candidate, value) {
                        result = index as i32;
                        break;
                    }
                }
                stack.push(Value::i32(result))?;
                Ok(())
            }
            array::INCLUDES => {
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.includes expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let value = stack.pop()?;
                let array_val = stack.pop()?;
                let len = self.array_like_length_with_context(array_val, task, module)?;
                let mut result = false;
                for index in 0..len {
                    if !self.array_like_has_index_with_context(array_val, index) {
                        continue;
                    }
                    let candidate =
                        self.array_like_index_value_with_context(array_val, index, task, module)?;
                    if self.array_search_values_same_value_zero(candidate, value) {
                        result = true;
                        break;
                    }
                }
                stack.push(Value::bool(result))?;
                Ok(())
            }
            array::SLICE => {
                // slice(start, end?) - arg_count is 1 or 2
                // Supports negative indices: -1 = last element, -2 = second-to-last, etc.
                let end_val = if arg_count >= 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let start_val = if arg_count >= 1 {
                    stack.pop()?
                } else {
                    Value::i32(0)
                };
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                let len = arr.len();

                // Normalize negative indices
                let start_raw = start_val.as_i32().unwrap_or(0);
                let start = if start_raw < 0 {
                    ((len as i32 + start_raw).max(0) as usize).min(len)
                } else {
                    (start_raw as usize).min(len)
                };

                let end = end_val
                    .and_then(|v| v.as_i32())
                    .map(|e| {
                        if e < 0 {
                            ((len as i32 + e).max(0) as usize).min(len)
                        } else {
                            (e as usize).min(len)
                        }
                    })
                    .unwrap_or(len);

                let mut new_arr = Array::new(arr.type_id, 0);
                if start < end {
                    for i in start..end {
                        if let Some(v) = arr.get(i) {
                            new_arr.push(v);
                        }
                    }
                }
                let gc_ptr = self.gc.lock().allocate(new_arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::SPLICE => {
                let debug_splice = std::env::var("RAYA_DEBUG_SPLICE").is_ok();
                // splice(start, deleteCount?, ...items): remove elements and optionally insert new ones
                // Returns array of removed elements
                if arg_count < 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.splice expects at least 1 argument, got {}",
                        arg_count
                    )));
                }

                // Pop arguments in reverse order
                let mut items = Vec::new();
                for _ in 2..arg_count {
                    items.push(stack.pop()?);
                }
                items.reverse();

                let delete_count_val = if arg_count >= 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let start_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                if debug_splice {
                    eprintln!(
                        "[splice] before receiver={:#x} len={} elems={} present={}",
                        array_val.raw(),
                        arr.length,
                        arr.elements.len(),
                        arr.present.len()
                    );
                }

                let len = arr.len();
                let relative_start = self.array_integer_argument(start_val)?;
                let start = if relative_start < 0 {
                    len.saturating_sub(relative_start.unsigned_abs() as usize)
                } else {
                    (relative_start as usize).min(len)
                };

                // Calculate delete count (default to rest of array if not specified)
                let delete_count = if let Some(dc_val) = delete_count_val {
                    self.array_integer_argument(dc_val)?.max(0) as usize
                } else {
                    len.saturating_sub(start)
                };

                // Calculate actual end of deletion
                let end = (start + delete_count).min(len);
                if debug_splice {
                    eprintln!(
                        "[splice] arg_count={} start={} delete_count={} end={} items={}",
                        arg_count,
                        start,
                        delete_count,
                        end,
                        items.len()
                    );
                }

                // Collect removed elements
                let removed_vals: Vec<Value> = (start..end).filter_map(|i| arr.get(i)).collect();

                // Build new dense storage: [0..start] + items + [end..len].
                // Keep the logical length and presence bitmap in sync so live
                // length-sensitive iteration observes mutations immediately.
                let mut new_elements: Vec<Value> = Vec::new();
                let mut new_present: Vec<bool> = Vec::new();
                for i in 0..start {
                    new_elements.push(arr.elements[i]);
                    new_present.push(arr.present.get(i).copied().unwrap_or(false));
                }
                for item in items {
                    new_elements.push(item);
                    new_present.push(true);
                }
                for i in end..len {
                    new_elements.push(arr.elements[i]);
                    new_present.push(arr.present.get(i).copied().unwrap_or(false));
                }

                // Update array elements - use std::mem::take to avoid reallocation
                let old_elements = std::mem::take(&mut arr.elements);
                arr.elements = new_elements;
                arr.present = new_present;
                arr.length = arr.present.len();
                arr.sparse_elements
                    .retain(|index, _| (*index as usize) < arr.length);
                if debug_splice {
                    eprintln!(
                        "[splice] after receiver={:#x} len={} elems={} present={}",
                        array_val.raw(),
                        arr.length,
                        arr.elements.len(),
                        arr.present.len()
                    );
                }
                drop(old_elements); // Explicitly drop old elements

                // Create removed array with same element type as source array
                let mut removed = Array::new(arr.type_id, removed_vals.len());
                for (i, v) in removed_vals.iter().enumerate() {
                    let _ = removed.set(i, *v);
                }

                // Return removed elements
                let gc_ptr = self.gc.lock().allocate(removed);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::REVERSE => {
                if arg_count != 0 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.reverse expects 0 arguments, got {}",
                        arg_count
                    )));
                }
                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };
                arr.elements.reverse();
                stack.push(array_val)?;
                Ok(())
            }
            array::CONCAT => {
                // concat(other): merge two arrays
                if arg_count != 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.concat expects 1 argument, got {}",
                        arg_count
                    )));
                }
                let other_val = stack.pop()?;
                let array_val = stack.pop()?;

                if !array_val.is_ptr() || !other_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };
                let other_ptr = unsafe { other_val.as_ptr::<Array>() };
                let other = unsafe { &*other_ptr.unwrap().as_ptr() };

                let mut new_arr = Array::new(0, 0);
                for elem in arr.elements.iter() {
                    new_arr.push(*elem);
                }
                for elem in other.elements.iter() {
                    new_arr.push(*elem);
                }

                let gc_ptr = self.gc.lock().allocate(new_arr);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::LAST_INDEX_OF => {
                // lastIndexOf(value, fromIndex?): find last occurrence
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.lastIndexOf expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let from_index = if arg_count == 2 {
                    Some(self.array_integer_argument(stack.pop()?)?)
                } else {
                    None
                };
                let search_val = stack.pop()?;
                let array_val = stack.pop()?;
                let len = self.array_like_length_with_context(array_val, task, module)?;
                let end = if len == 0 {
                    None
                } else if let Some(from_index) = from_index {
                    if from_index >= 0 {
                        Some((from_index as usize).min(len - 1))
                    } else {
                        Some(len.saturating_sub(from_index.unsigned_abs() as usize))
                    }
                } else {
                    Some(len - 1)
                };
                let mut found_index: i32 = -1;
                if let Some(end) = end {
                    for index in (0..=end).rev() {
                        if !self.array_like_has_index_with_context(array_val, index) {
                            continue;
                        }
                        let candidate = self.array_like_index_value_with_context(
                            array_val,
                            index,
                            task,
                            module,
                        )?;
                        if self.array_search_values_strict_equal(candidate, search_val) {
                            found_index = index as i32;
                            break;
                        }
                    }
                }

                stack.push(Value::i32(found_index))?;
                Ok(())
            }
            array::FILL => {
                // fill(value, start?, end?): fill with value
                if !(1..=3).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.fill expects 1-3 arguments, got {}",
                        arg_count
                    )));
                }

                // Pop arguments in reverse order
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(stack.pop()?);
                }
                args.reverse();

                let array_val = stack.pop()?;
                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &mut *arr_ptr.unwrap().as_ptr() };

                let fill_value = args[0];
                let start = if arg_count >= 2 {
                    args[1].as_i32().unwrap_or(0).max(0) as usize
                } else {
                    0
                };
                let end = if arg_count >= 3 {
                    args[2].as_i32().unwrap_or(arr.len() as i32).max(0) as usize
                } else {
                    arr.len()
                };

                for i in start..end.min(arr.len()) {
                    arr.elements[i] = fill_value;
                }

                stack.push(array_val)?;
                Ok(())
            }
            array::FLAT => {
                // flat(depth?): flatten nested arrays
                let depth = if arg_count >= 1 {
                    let d = stack.pop()?.as_i32().unwrap_or(1);
                    d.max(0) as usize
                } else {
                    1
                };
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }

                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                // `_gc` will be needed when flatten allocates GC-managed sub-arrays.
                fn flatten(
                    _gc: &parking_lot::Mutex<crate::vm::gc::GarbageCollector>,
                    arr: &Array,
                    depth: usize,
                ) -> Array {
                    let mut result = Array::new(0, 0);
                    for elem in arr.elements.iter() {
                        if depth > 0 && elem.is_ptr() {
                            if let Some(ptr) = unsafe { elem.as_ptr::<Array>() } {
                                let inner = unsafe { &*ptr.as_ptr() };
                                let flattened = flatten(_gc, inner, depth - 1);
                                for inner_elem in flattened.elements {
                                    result.push(inner_elem);
                                }
                                continue;
                            }
                        }
                        result.push(*elem);
                    }
                    result
                }

                let result = flatten(self.gc, arr, depth);
                let gc_ptr = self.gc.lock().allocate(result);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::FOR_EACH => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.forEach expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let this_arg = if arg_count == 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let callback = stack.pop()?;
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let elements = unsafe { &*arr_ptr.as_ptr() }.elements.clone();
                let callback_this = Self::array_callback_this_arg(this_arg);
                for (index, element) in elements.iter().copied().enumerate() {
                    let _ = self.invoke_callable_sync_with_this(
                        callback,
                        Some(callback_this),
                        &[element, Value::i32(index as i32), array_val],
                        task,
                        module,
                    )?;
                }
                stack.push(Value::undefined())?;
                Ok(())
            }
            array::FILTER => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.filter expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let this_arg = if arg_count == 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let callback = stack.pop()?;
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let (array_type_id, elements) = {
                    let arr = unsafe { &*arr_ptr.as_ptr() };
                    (arr.type_id, arr.elements.clone())
                };
                let callback_this = Self::array_callback_this_arg(this_arg);
                let mut result = Array::new(array_type_id, 0);
                for (index, element) in elements.iter().copied().enumerate() {
                    let keep = self.invoke_callable_sync_with_this(
                        callback,
                        Some(callback_this),
                        &[element, Value::i32(index as i32), array_val],
                        task,
                        module,
                    )?;
                    if keep.is_truthy() {
                        result.push(element);
                    }
                }
                let gc_ptr = self.gc.lock().allocate(result);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::FIND => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.find expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let this_arg = if arg_count == 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let callback = stack.pop()?;
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let elements = unsafe { &*arr_ptr.as_ptr() }.elements.clone();
                let callback_this = Self::array_callback_this_arg(this_arg);
                for (index, element) in elements.iter().copied().enumerate() {
                    let found = self.invoke_callable_sync_with_this(
                        callback,
                        Some(callback_this),
                        &[element, Value::i32(index as i32), array_val],
                        task,
                        module,
                    )?;
                    if found.is_truthy() {
                        stack.push(element)?;
                        return Ok(());
                    }
                }
                stack.push(Value::undefined())?;
                Ok(())
            }
            array::FIND_INDEX => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.findIndex expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let this_arg = if arg_count == 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let callback = stack.pop()?;
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let elements = unsafe { &*arr_ptr.as_ptr() }.elements.clone();
                let callback_this = Self::array_callback_this_arg(this_arg);
                for (index, element) in elements.iter().copied().enumerate() {
                    let found = self.invoke_callable_sync_with_this(
                        callback,
                        Some(callback_this),
                        &[element, Value::i32(index as i32), array_val],
                        task,
                        module,
                    )?;
                    if found.is_truthy() {
                        stack.push(Value::i32(index as i32))?;
                        return Ok(());
                    }
                }
                stack.push(Value::i32(-1))?;
                Ok(())
            }
            array::EVERY => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.every expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let this_arg = if arg_count == 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let callback = stack.pop()?;
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let elements = unsafe { &*arr_ptr.as_ptr() }.elements.clone();
                let callback_this = Self::array_callback_this_arg(this_arg);
                for (index, element) in elements.iter().copied().enumerate() {
                    let keep = self.invoke_callable_sync_with_this(
                        callback,
                        Some(callback_this),
                        &[element, Value::i32(index as i32), array_val],
                        task,
                        module,
                    )?;
                    if !keep.is_truthy() {
                        stack.push(Value::bool(false))?;
                        return Ok(());
                    }
                }
                stack.push(Value::bool(true))?;
                Ok(())
            }
            array::SOME => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.some expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let this_arg = if arg_count == 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let callback = stack.pop()?;
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let elements = unsafe { &*arr_ptr.as_ptr() }.elements.clone();
                let callback_this = Self::array_callback_this_arg(this_arg);
                for (index, element) in elements.iter().copied().enumerate() {
                    let keep = self.invoke_callable_sync_with_this(
                        callback,
                        Some(callback_this),
                        &[element, Value::i32(index as i32), array_val],
                        task,
                        module,
                    )?;
                    if keep.is_truthy() {
                        stack.push(Value::bool(true))?;
                        return Ok(());
                    }
                }
                stack.push(Value::bool(false))?;
                Ok(())
            }
            array::MAP => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.map expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let this_arg = if arg_count == 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let callback = stack.pop()?;
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let elements = unsafe { &*arr_ptr.as_ptr() }.elements.clone();
                let callback_this = Self::array_callback_this_arg(this_arg);
                let mut result = Array::new(0, 0);
                for (index, element) in elements.iter().copied().enumerate() {
                    let mapped = self.invoke_callable_sync_with_this(
                        callback,
                        Some(callback_this),
                        &[element, Value::i32(index as i32), array_val],
                        task,
                        module,
                    )?;
                    result.push(mapped);
                }
                let gc_ptr = self.gc.lock().allocate(result);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            array::REDUCE => {
                if !(1..=2).contains(&arg_count) {
                    return Err(VmError::RuntimeError(format!(
                        "Array.reduce expects 1-2 arguments, got {}",
                        arg_count
                    )));
                }
                let initial = if arg_count == 2 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let callback = stack.pop()?;
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let elements = unsafe { &*arr_ptr.as_ptr() }.elements.clone();
                let (mut acc, start_index) = if let Some(initial) = initial {
                    (initial, 0usize)
                } else if let Some(first) = elements.first().copied() {
                    (first, 1usize)
                } else {
                    return Err(VmError::TypeError(
                        "Reduce of empty array with no initial value".to_string(),
                    ));
                };
                for (index, element) in elements.iter().copied().enumerate().skip(start_index) {
                    acc = self.invoke_callable_sync_with_this(
                        callback,
                        Some(Value::undefined()),
                        &[acc, element, Value::i32(index as i32), array_val],
                        task,
                        module,
                    )?;
                }
                stack.push(acc)?;
                Ok(())
            }
            array::SORT => {
                if arg_count > 1 {
                    return Err(VmError::RuntimeError(format!(
                        "Array.sort expects 0-1 arguments, got {}",
                        arg_count
                    )));
                }
                let compare_fn = if arg_count == 1 {
                    Some(stack.pop()?)
                } else {
                    None
                };
                let array_val = stack.pop()?;
                let Some(arr_ptr) = (unsafe { array_val.as_ptr::<Array>() }) else {
                    return Err(VmError::TypeError("Expected array".to_string()));
                };
                let mut elements = unsafe { &*arr_ptr.as_ptr() }.elements.clone();
                for i in 1..elements.len() {
                    let key = elements[i];
                    let mut j = i;
                    while j > 0 {
                        let prev = elements[j - 1];
                        let ordering = if let Some(compare_fn) = compare_fn {
                            let result = self.invoke_callable_sync_with_this(
                                compare_fn,
                                Some(Value::undefined()),
                                &[prev, key],
                                task,
                                module,
                            )?;
                            result
                                .as_i32()
                                .unwrap_or_else(|| result.as_f64().map(|f| f as i32).unwrap_or(0))
                        } else {
                            self.array_sort_compare_default(prev, key)
                        };
                        if ordering <= 0 {
                            break;
                        }
                        elements[j] = prev;
                        j -= 1;
                    }
                    elements[j] = key;
                }
                unsafe { &mut *arr_ptr.as_ptr() }.elements = elements;
                stack.push(array_val)?;
                Ok(())
            }
            array::JOIN => {
                // join(separator?) - arg_count is 0 or 1
                let sep = if arg_count >= 1 {
                    let sep_val = stack.pop()?;
                    if let Some(ptr) = unsafe { sep_val.as_ptr::<RayaString>() } {
                        let s = unsafe { &*ptr.as_ptr() };
                        s.data.clone()
                    } else {
                        ",".to_string()
                    }
                } else {
                    ",".to_string()
                };
                let array_val = stack.pop()?;

                if !array_val.is_ptr() {
                    return Err(VmError::TypeError("Expected array".to_string()));
                }
                let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
                let arr = unsafe { &*arr_ptr.unwrap().as_ptr() };

                // Convert elements to strings and join
                let parts: Vec<String> = arr
                    .elements
                    .iter()
                    .map(|v| {
                        if let Some(ptr) = unsafe { v.as_ptr::<RayaString>() } {
                            unsafe { &*ptr.as_ptr() }.data.clone()
                        } else if let Some(i) = v.as_i32() {
                            i.to_string()
                        } else if let Some(f) = v.as_f64() {
                            f.to_string()
                        } else if v.is_null() {
                            String::new()
                        } else if let Some(b) = v.as_bool() {
                            b.to_string()
                        } else {
                            String::new()
                        }
                    })
                    .collect();
                let result = parts.join(&sep);
                let raya_string = RayaString::new(result);
                let gc_ptr = self.gc.lock().allocate(raya_string);
                let value =
                    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                stack.push(value)?;
                Ok(())
            }
            // NOTE: FILTER, MAP, FIND, FIND_INDEX, FOR_EACH, EVERY, SOME, SORT, REDUCE
            // are now compiled as inline loops by the compiler (see lower_array_intrinsic in expr.rs)
            // and never reach this handler at runtime.
            _ => Err(VmError::RuntimeError(format!(
                "Array method {:#06x} not yet implemented in Interpreter",
                method_id
            ))),
        }
    }
}
