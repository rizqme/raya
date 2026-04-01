//! Shared iterator protocol helpers.
//!
//! This layer centralizes ECMAScript-style `@@iterator` acquisition,
//! `next()` stepping, result-object value extraction, and `return()`
//! closing so compiler lowerings and builtins do not each reinvent their
//! own array-shaped approximation.

use crate::compiler::Module;
use crate::vm::interpreter::opcodes::native::checked_string_ptr;
use crate::vm::interpreter::Interpreter;
use crate::vm::iteration::{ResumeCompletion, ResumeCompletionKind};
use crate::vm::object::{Array, RayaString};
use crate::vm::scheduler::Task;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    fn iterator_result_object(&self, value: Value, done: bool) -> Value {
        let mut result = crate::vm::object::Object::new_dynamic(
            crate::vm::object::layout_id_from_ordered_names(&[]),
            0,
        );
        {
            let dyn_props = result.ensure_dyn_props();
            dyn_props.insert(
                self.intern_prop_key("value"),
                crate::vm::object::DynProp::data_with_attrs(value, true, true, true),
            );
            dyn_props.insert(
                self.intern_prop_key("done"),
                crate::vm::object::DynProp::data_with_attrs(Value::bool(done), true, true, true),
            );
        }
        if let Some(object_ctor) = self.builtin_global_value("Object") {
            if let Some(prototype) = self.constructor_prototype_value(object_ctor) {
                result.prototype = prototype;
            }
        }
        let result_ptr = self.gc.lock().allocate(result);
        unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(result_ptr.as_ptr()).expect("iterator result ptr"),
            )
        }
    }

    fn iterator_method_value(
        &mut self,
        iterator: Value,
        property: &str,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        Ok(self
            .get_property_value_via_js_semantics_with_context(iterator, property, task, module)?
            .unwrap_or(Value::undefined()))
    }

    fn string_iterator_fallback(
        &mut self,
        iterable: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let Some(string_ptr) = checked_string_ptr(iterable) else {
            return Ok(None);
        };

        let string = unsafe { &*string_ptr.as_ptr() };
        let mut chars = Array::new(0, 0);
        for ch in string.data.chars() {
            let raya_string = RayaString::new(ch.to_string());
            let string_ptr = self.gc.lock().allocate(raya_string);
            let string_value = unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(string_ptr.as_ptr()).expect("string iterator char ptr"),
                )
            };
            chars.push(string_value);
        }

        let array_ptr = self.gc.lock().allocate(chars);
        let array_value = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(array_ptr.as_ptr()).expect("string iterator array ptr"),
            )
        };
        self.try_get_iterator_from_value(array_value, task, module)
    }

    fn iterator_next_method(
        &mut self,
        iterator: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        self.iterator_method_value(iterator, "next", task, module)
    }

    fn normalize_iterator_candidate(
        &mut self,
        iterable: Value,
        candidate: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let next_method = self.iterator_next_method(candidate, task, module)?;
        if Self::is_callable_value(next_method) {
            return Ok(candidate);
        }

        if candidate.raw() != iterable.raw() {
            if let Some(nested_iterator) =
                self.try_get_iterator_from_value(candidate, task, module)?
            {
                let nested_next = self.iterator_next_method(nested_iterator, task, module)?;
                if Self::is_callable_value(nested_next) {
                    return Ok(nested_iterator);
                }
            }
        }

        // Preserve the iterator object itself even when `next` is missing.
        // Later `IteratorStep` / `IteratorClose` operations surface the precise
        // protocol error at the point the method is actually needed.
        Ok(candidate)
    }

    pub(in crate::vm::interpreter) fn iterator_release_ephemeral_root(&self, value: Value) {
        let mut roots = self.ephemeral_gc_roots.write();
        if let Some(index) = roots.iter().rposition(|candidate| *candidate == value) {
            roots.swap_remove(index);
        }
    }

    fn iterator_debug_enabled() -> bool {
        std::env::var("RAYA_DEBUG_ITERATOR_PROTOCOL").is_ok()
    }

    pub(in crate::vm::interpreter) fn iterator_result_is_done(
        &mut self,
        result: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        let done = self
            .get_property_value_via_js_semantics_with_context(result, "done", task, module)?
            .unwrap_or(Value::undefined());
        Ok(if let Some(boolean) = done.as_bool() {
            boolean
        } else {
            !done.is_null() && !done.is_undefined()
        })
    }

    pub(in crate::vm::interpreter) fn iterator_result_value(
        &mut self,
        result: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        Ok(self
            .get_property_value_via_js_semantics_with_context(result, "value", task, module)?
            .unwrap_or(Value::undefined()))
    }

    pub(in crate::vm::interpreter) fn try_get_iterator_from_value(
        &mut self,
        iterable: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let string_fallback_iterator = self.string_iterator_fallback(iterable, task, module)?;
        let direct_js = self.get_property_value_via_js_semantics_with_context(
            iterable,
            "Symbol.iterator",
            task,
            module,
        )?;
        if Self::iterator_debug_enabled() {
            eprintln!(
                "[iter] methods iterable={:#x} string_fallback={:?} direct_js={:?}",
                iterable.raw(),
                string_fallback_iterator.map(|value| format!("{:#x}", value.raw())),
                direct_js.map(|value| format!("{:#x}", value.raw())),
            );
        }
        if let Some(iterator) = string_fallback_iterator {
            return Ok(Some(iterator));
        }
        let Some(iterator_method) = direct_js else {
            return Ok(None);
        };
        if iterator_method.is_null() || iterator_method.is_undefined() {
            return Ok(None);
        }
        if !Self::is_callable_value(iterator_method) {
            return Err(VmError::TypeError(
                "Iterator method is not callable".to_string(),
            ));
        }
        let iterator_candidate = self.invoke_callable_sync_with_this(
            iterator_method,
            Some(iterable),
            &[],
            task,
            module,
        )?;
        if !self.is_js_object_value(iterator_candidate)
            && !Self::is_callable_value(iterator_candidate)
        {
            return Err(VmError::TypeError(
                "Iterator method must return an object".to_string(),
            ));
        }
        let iterator =
            self.normalize_iterator_candidate(iterable, iterator_candidate, task, module)?;
        if Self::iterator_debug_enabled() {
            eprintln!(
                "[iter] get iterable={:#x} iterator={:#x}",
                iterable.raw(),
                iterator.raw()
            );
        }
        Ok(Some(iterator))
    }

    pub(in crate::vm::interpreter) fn try_get_async_iterator_from_value(
        &mut self,
        iterable: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let Some(iterator_method) = self
            .get_property_value_via_js_semantics_with_context(
                iterable,
                "Symbol.asyncIterator",
                task,
                module,
            )?
        else {
            return self.try_get_iterator_from_value(iterable, task, module);
        };
        if iterator_method.is_null() || iterator_method.is_undefined() {
            return self.try_get_iterator_from_value(iterable, task, module);
        }
        if !Self::is_callable_value(iterator_method) {
            return Err(VmError::TypeError(
                "Async iterator method is not callable".to_string(),
            ));
        }
        let iterator_candidate = self.invoke_callable_sync_with_this(
            iterator_method,
            Some(iterable),
            &[],
            task,
            module,
        )?;
        if !self.is_js_object_value(iterator_candidate)
            && !Self::is_callable_value(iterator_candidate)
        {
            return Err(VmError::TypeError(
                "Async iterator method must return an object".to_string(),
            ));
        }
        Ok(Some(iterator_candidate))
    }

    pub(in crate::vm::interpreter) fn get_async_iterator_from_value(
        &mut self,
        iterable: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        self.try_get_async_iterator_from_value(iterable, task, module)?
            .ok_or_else(|| VmError::TypeError("Value is not async iterable".to_string()))
    }

    pub(in crate::vm::interpreter) fn get_iterator_from_value(
        &mut self,
        iterable: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        self.try_get_iterator_from_value(iterable, task, module)?
            .ok_or_else(|| VmError::TypeError("Value is not iterable".to_string()))
    }

    pub(in crate::vm::interpreter) fn iterator_step_result(
        &mut self,
        iterator: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let next_method = self.iterator_next_method(iterator, task, module)?;
        if !Self::is_callable_value(next_method) {
            return Err(VmError::TypeError(
                "Iterator is missing callable next()".to_string(),
            ));
        }
        let next_result =
            self.invoke_callable_sync_with_this(next_method, Some(iterator), &[], task, module)?;
        if !self.is_js_object_value(next_result) && !Self::is_callable_value(next_result) {
            return Err(VmError::TypeError(
                "Iterator result must be an object".to_string(),
            ));
        }
        if self.iterator_result_is_done(next_result, task, module)? {
            if Self::iterator_debug_enabled() {
                eprintln!("[iter] step iterator={:#x} -> done", iterator.raw());
            }
            return Ok(None);
        }
        if Self::iterator_debug_enabled() {
            eprintln!(
                "[iter] step iterator={:#x} -> result={:#x}",
                iterator.raw(),
                next_result.raw()
            );
        }
        Ok(Some(next_result))
    }

    pub(in crate::vm::interpreter) fn iterator_close(
        &mut self,
        iterator: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let return_method = self
            .get_property_value_via_js_semantics_with_context(iterator, "return", task, module)?
            .unwrap_or(Value::undefined());
        if return_method.is_null() || return_method.is_undefined() {
            return Ok(());
        }
        if !Self::is_callable_value(return_method) {
            return Err(VmError::TypeError(
                "Iterator return is not callable".to_string(),
            ));
        }
        let returned =
            self.invoke_callable_sync_with_this(return_method, Some(iterator), &[], task, module)?;
        if !self.is_js_object_value(returned) && !Self::is_callable_value(returned) {
            return Err(VmError::TypeError(
                "Iterator return must produce an object".to_string(),
            ));
        }
        if Self::iterator_debug_enabled() {
            eprintln!("[iter] close iterator={:#x}", iterator.raw());
        }
        Ok(())
    }

    pub(in crate::vm::interpreter) fn iterator_resume_result(
        &mut self,
        iterator: Value,
        completion: ResumeCompletion,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let (method_name, missing_behavior) = match completion.kind {
            ResumeCompletionKind::Next => ("next", None),
            ResumeCompletionKind::Return => ("return", Some(ResumeCompletionKind::Return)),
            ResumeCompletionKind::Throw => ("throw", Some(ResumeCompletionKind::Throw)),
        };
        let method = self.iterator_method_value(iterator, method_name, task, module)?;
        if method.is_null() || method.is_undefined() {
            return match missing_behavior {
                Some(ResumeCompletionKind::Return) => {
                    Ok(self.iterator_result_object(completion.value, true))
                }
                Some(ResumeCompletionKind::Throw) => {
                    let _ = self.iterator_close(iterator, task, module);
                    Err(VmError::TypeError(
                        "Iterator is missing callable throw()".to_string(),
                    ))
                }
                _ => Err(VmError::TypeError(
                    "Iterator is missing callable next()".to_string(),
                )),
            };
        }
        if !Self::is_callable_value(method) {
            return Err(VmError::TypeError(format!(
                "Iterator {} is not callable",
                method_name
            )));
        }
        let result = self.invoke_callable_sync_with_this(
            method,
            Some(iterator),
            &[completion.value],
            task,
            module,
        )?;
        if !self.is_js_object_value(result) && !Self::is_callable_value(result) {
            return Err(VmError::TypeError(format!(
                "Iterator {} must produce an object",
                method_name
            )));
        }
        Ok(result)
    }

    pub(in crate::vm::interpreter) fn append_iterable_to_array(
        &mut self,
        target: Value,
        iterable: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let Some(array_ptr) = crate::vm::interpreter::opcodes::native::checked_array_ptr(target)
        else {
            return Err(VmError::TypeError(
                "Iterator append target must be an array".to_string(),
            ));
        };

        if target.is_heap_allocated() {
            self.ephemeral_gc_roots.write().push(target);
        }
        if iterable.is_heap_allocated() {
            self.ephemeral_gc_roots.write().push(iterable);
        }

        let iterator = match self.get_iterator_from_value(iterable, task, module) {
            Ok(iterator) => iterator,
            Err(error) => {
                self.iterator_release_ephemeral_root(iterable);
                self.iterator_release_ephemeral_root(target);
                return Err(error);
            }
        };
        if iterator.is_heap_allocated() {
            self.ephemeral_gc_roots.write().push(iterator);
        }

        loop {
            let next_result = match self.iterator_step_result(iterator, task, module) {
                Ok(Some(result)) => result,
                Ok(None) => break,
                Err(error) => {
                    let _ = self.iterator_close(iterator, task, module);
                    self.iterator_release_ephemeral_root(iterator);
                    self.iterator_release_ephemeral_root(iterable);
                    self.iterator_release_ephemeral_root(target);
                    return Err(error);
                }
            };

            let value = match self.iterator_result_value(next_result, task, module) {
                Ok(value) => value,
                Err(error) => {
                    let _ = self.iterator_close(iterator, task, module);
                    self.iterator_release_ephemeral_root(iterator);
                    self.iterator_release_ephemeral_root(iterable);
                    self.iterator_release_ephemeral_root(target);
                    return Err(error);
                }
            };

            let array = unsafe { &mut *array_ptr.as_ptr() };
            array.push(value);
        }

        self.iterator_release_ephemeral_root(iterator);
        self.iterator_release_ephemeral_root(iterable);
        self.iterator_release_ephemeral_root(target);
        Ok(())
    }
}
