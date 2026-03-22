//! Shared iterator protocol helpers.
//!
//! This layer centralizes ECMAScript-style `@@iterator` acquisition,
//! `next()` stepping, result-object value extraction, and `return()`
//! closing so compiler lowerings and builtins do not each reinvent their
//! own array-shaped approximation.

use crate::compiler::Module;
use crate::vm::interpreter::Interpreter;
use crate::vm::scheduler::Task;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn iterator_release_ephemeral_root(&self, value: Value) {
        let mut roots = self.ephemeral_gc_roots.write();
        if let Some(index) = roots.iter().rposition(|candidate| *candidate == value) {
            roots.swap_remove(index);
        }
    }

    fn iterator_debug_enabled() -> bool {
        std::env::var("RAYA_DEBUG_ITERATOR_PROTOCOL").is_ok()
    }

    fn iterator_result_is_done(&mut self, result: Value, task: &Arc<Task>, module: &Module) -> Result<bool, VmError> {
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
        let Some(iterator_method) =
            self.well_known_symbol_property_value(iterable, "Symbol.iterator", task, module)?
        else {
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
        let iterator =
            self.invoke_callable_sync_with_this(iterator_method, Some(iterable), &[], task, module)?;
        if !self.is_js_object_value(iterator) && !Self::is_callable_value(iterator) {
            return Err(VmError::TypeError(
                "Iterator method must return an object".to_string(),
            ));
        }
        if Self::iterator_debug_enabled() {
            eprintln!(
                "[iter] get iterable={:#x} iterator={:#x}",
                iterable.raw(),
                iterator.raw()
            );
        }
        Ok(Some(iterator))
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
        let next_method = self
            .get_property_value_via_js_semantics_with_context(iterator, "next", task, module)?
            .unwrap_or(Value::undefined());
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
        let _ = self.invoke_callable_sync_with_this(return_method, Some(iterator), &[], task, module)?;
        if Self::iterator_debug_enabled() {
            eprintln!("[iter] close iterator={:#x}", iterator.raw());
        }
        Ok(())
    }

    pub(in crate::vm::interpreter) fn append_iterable_to_array(
        &mut self,
        target: Value,
        iterable: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let Some(array_ptr) = crate::vm::interpreter::opcodes::native::checked_array_ptr(target) else {
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
