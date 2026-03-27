//! Concurrency opcode handlers: Spawn, SpawnClosure, Await, WaitAll, Sleep, MutexLock, MutexUnlock, Yield, TaskCancel

use crate::compiler::{Module, Opcode};
use crate::vm::interpreter::execution::{ExecutionResult, OpcodeResult};
use crate::vm::interpreter::opcodes::objects::CallableInvocationPlan;
use crate::vm::interpreter::{Interpreter, PromiseHandle};
use crate::vm::object::Array;
use crate::vm::scheduler::{SuspendReason, Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::{MutexId, SemaphoreId};
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;
use std::time::Instant;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn spawn_async_task_handle(
        &mut self,
        func_index: usize,
        target_module: Arc<Module>,
        parent_task: &Arc<Task>,
        args: Vec<Value>,
        closure_val: Option<Value>,
    ) -> Result<Value, VmError> {
        let debug_async_tasks = std::env::var("RAYA_DEBUG_ASYNC_TASKS").is_ok();
        let arg_len = args.len();
        let debug_func_name = if debug_async_tasks {
            target_module
                .functions
                .get(func_index)
                .map(|function| function.name.clone())
                .unwrap_or_else(|| "<unknown>".to_string())
        } else {
            String::new()
        };
        let debug_module_name = if debug_async_tasks {
            target_module.metadata.name.clone()
        } else {
            String::new()
        };
        let new_task = Arc::new(Task::with_args(
            func_index,
            target_module.clone(),
            Some(parent_task.id()),
            args,
        ));
        if let Some(closure) = closure_val {
            new_task.push_closure(closure);
        }
        new_task.replace_stack(self.stack_pool.acquire());

        if debug_async_tasks {
            eprintln!(
                "[async-task] spawn task={:?} parent={:?} module={} func={}#{} argc={}",
                new_task.id(),
                parent_task.id(),
                debug_module_name,
                debug_func_name,
                func_index,
                arg_len
            );
        }

        let eager_start = target_module
            .functions
            .get(func_index)
            .is_some_and(|function| function.is_async && function.uses_js_runtime_semantics);
        let task_id = new_task.id();
        self.tasks.write().insert(task_id, new_task.clone());

        if eager_start {
            match self.run(&new_task) {
                ExecutionResult::Completed(value) => {
                    self.settle_completed_async_task(&new_task, value)?;
                }
                ExecutionResult::Suspended(reason) => {
                    new_task.suspend(reason);
                }
                ExecutionResult::Failed(error) => {
                    self.ensure_task_exception_for_error(&new_task, &error);
                    new_task.fail();
                    if !new_task.is_cancelled() && !new_task.is_rejection_observed() {
                        self.promise_microtasks.lock().push_back(
                            crate::vm::interpreter::PromiseMicrotask::ReportUnhandledRejection(
                                task_id, 2,
                            ),
                        );
                    }
                }
            }
        } else {
            self.injector.push(new_task);
        }

        Ok(PromiseHandle::new(task_id).into_value())
    }

    pub(in crate::vm::interpreter) fn exec_concurrency_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        _module: &Module,
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::Spawn => {
                let func_index = match Self::read_u32(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                if task
                    .current_module()
                    .functions
                    .get(func_index)
                    .is_some_and(|function| function.uses_js_this_slot)
                {
                    let implicit_this = if task
                        .current_module()
                        .functions
                        .get(func_index)
                        .is_some_and(|function| function.is_strict_js)
                    {
                        Value::undefined()
                    } else {
                        self.builtin_global_value("globalThis")
                            .unwrap_or(Value::undefined())
                    };
                    args.push(implicit_this);
                }

                let handle = match self.spawn_async_task_handle(
                    func_index,
                    task.current_module(),
                    task,
                    args,
                    None,
                ) {
                    Ok(handle) => handle,
                    Err(error) => return OpcodeResult::Error(error),
                };

                if let Err(e) = stack.push(handle) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::SpawnClosure => {
                let arg_count = match Self::read_u16(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let closure_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                if !closure_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected closure or bound method".to_string(),
                    ));
                }

                let Some(plan) =
                    (match self.prepare_callable_invocation(closure_val, &args, None, task) {
                        Ok(plan) => plan,
                        Err(error) => return OpcodeResult::Error(error),
                    })
                else {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected closure or bound method".to_string(),
                    ));
                };
                let CallableInvocationPlan::Function {
                    func_id,
                    module: target_module,
                    closure_val: closure_to_push,
                    args: spawn_args,
                    is_async,
                    is_generator,
                } = plan
                else {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected closure or bound method".to_string(),
                    ));
                };
                if is_generator || !is_async {
                    return OpcodeResult::Error(VmError::TypeError(
                        "Expected async closure or bound method".to_string(),
                    ));
                }

                let handle = match self.spawn_async_task_handle(
                    func_id,
                    target_module,
                    task,
                    spawn_args,
                    closure_to_push,
                ) {
                    Ok(handle) => handle,
                    Err(error) => return OpcodeResult::Error(error),
                };

                if let Err(e) = stack.push(handle) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Await => {
                let awaited_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // JS-like await normalization: awaiting a non-promise value resolves immediately.
                let Some(awaited_handle) = self.promise_handle_from_value(awaited_val) else {
                    if let Err(e) = stack.push(awaited_val) {
                        return OpcodeResult::Error(e);
                    }
                    return OpcodeResult::Continue;
                };

                let mut awaited_id = awaited_handle.task_id();

                loop {
                    let tasks_guard = self.tasks.read();
                    let Some(awaited_task) = tasks_guard.get(&awaited_id).cloned() else {
                        drop(tasks_guard);
                        if let Err(e) = stack.push(awaited_val) {
                            return OpcodeResult::Error(e);
                        }
                        return OpcodeResult::Continue;
                    };
                    drop(tasks_guard);

                    if awaited_task.is_cancelled() {
                        awaited_task.mark_rejection_observed();
                        return OpcodeResult::Error(VmError::RuntimeError(format!(
                            "Awaited task {:?} cancelled",
                            awaited_id
                        )));
                    }

                    match awaited_task.state() {
                        TaskState::Completed => {
                            let result = awaited_task.result().unwrap_or(Value::null());
                            if let Some(nested_handle) = self.promise_handle_from_value(result) {
                                awaited_id = nested_handle.task_id();
                                continue;
                            }
                            if let Err(e) = stack.push(result) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        TaskState::Failed => {
                            awaited_task.mark_rejection_observed();
                            if let Some(exc) = awaited_task.current_exception() {
                                task.set_exception(exc);
                            }
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Awaited task {:?} failed",
                                awaited_id
                            )));
                        }
                        _ => {
                            if awaited_task.add_waiter_if_incomplete(task.id()) {
                                awaited_task.mark_rejection_observed();
                                return OpcodeResult::Suspend(SuspendReason::AwaitTask(awaited_id));
                            }
                            match awaited_task.state() {
                                TaskState::Completed => {
                                    let result = awaited_task.result().unwrap_or(Value::null());
                                    if let Some(nested_handle) =
                                        self.promise_handle_from_value(result)
                                    {
                                        awaited_id = nested_handle.task_id();
                                        continue;
                                    }
                                    if let Err(e) = stack.push(result) {
                                        return OpcodeResult::Error(e);
                                    }
                                    return OpcodeResult::Continue;
                                }
                                TaskState::Failed => {
                                    awaited_task.mark_rejection_observed();
                                    if let Some(exc) = awaited_task.current_exception() {
                                        task.set_exception(exc);
                                    }
                                    return OpcodeResult::Error(VmError::RuntimeError(format!(
                                        "Awaited task {:?} failed",
                                        awaited_id
                                    )));
                                }
                                _ => return OpcodeResult::Continue,
                            }
                        }
                    }
                }
            }

            Opcode::WaitAll => {
                // WaitAll: await [task1, task2, ...] - wait for all tasks and return results array
                // The array operand is preserved across suspension; run() does not
                // push resume values for WaitAll sites.
                let arr_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError(
                        "WaitAll expects an array of tasks".to_string(),
                    ));
                }

                let arr_ptr = match unsafe { arr_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected array for WaitAll".to_string(),
                        ))
                    }
                };
                let arr = unsafe { &*arr_ptr.as_ptr() };
                let task_count = arr.len();

                // Collect task IDs and check their states
                let mut results = Vec::with_capacity(task_count);
                let mut all_completed = true;
                let mut first_incomplete: Option<TaskId> = None;
                let mut failed_task_info: Option<(TaskId, Option<Value>)> = None;
                let mut missing_task: Option<TaskId> = None;

                {
                    let tasks_guard = self.tasks.read();
                    for i in 0..task_count {
                        let elem = arr.get(i).unwrap_or(Value::null());
                        let Some(awaited_handle) = self.promise_handle_from_value(elem) else {
                            // WaitAll arrays can be re-executed after suspension with
                            // elements that are already materialized results (for
                            // example pointer-backed strings). Treat them like Await:
                            // non-task values are already resolved literals.
                            results.push(elem);
                            continue;
                        };
                        let awaited_id = awaited_handle.task_id();

                        if let Some(awaited_task) = tasks_guard.get(&awaited_id) {
                            // Await-all attaches a consumer for every member task.
                            // Mark any future rejection as observed as soon as the
                            // aggregate wait is established, not only after failure
                            // is re-read later. This matches promise-handler semantics
                            // and avoids spurious unhandled-rejection reporting.
                            awaited_task.mark_rejection_observed();
                            if awaited_task.is_cancelled() {
                                failed_task_info = Some((awaited_id, None));
                                break;
                            }

                            match awaited_task.state() {
                                TaskState::Completed => {
                                    let result = awaited_task.result().unwrap_or(Value::null());
                                    results.push(result);
                                }
                                TaskState::Failed => {
                                    // Record failure info to handle after releasing lock
                                    let exc = awaited_task.current_exception();
                                    failed_task_info = Some((awaited_id, exc));
                                    break;
                                }
                                _ => {
                                    all_completed = false;
                                    if first_incomplete.is_none() {
                                        first_incomplete = Some(awaited_id);
                                    }
                                    results.push(Value::null()); // placeholder
                                }
                            }
                        } else {
                            // Mirror Await semantics: if the numeric handle no longer
                            // refers to a live task, treat the value as an already
                            // resolved literal rather than failing the aggregate await.
                            results.push(elem);
                        }
                    }
                } // tasks_guard dropped here

                // Handle error cases after releasing the lock
                if let Some(task_id) = missing_task {
                    return OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Task {:?} not found in WaitAll",
                        task_id
                    )));
                }
                if let Some((awaited_id, exc)) = failed_task_info {
                    if let Some(awaited_task) = self.tasks.read().get(&awaited_id).cloned() {
                        awaited_task.mark_rejection_observed();
                    }
                    let waitall_msg = format!("Awaited task {:?} failed in WaitAll", awaited_id);
                    let wrapped_exc = {
                        let mut gc = self.gc.lock();
                        let gc_ptr = gc.allocate(crate::vm::RayaString::new(waitall_msg.clone()));
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
                    };
                    task.set_exception(wrapped_exc);
                    let _ = exc; // child rejection is observed above; aggregate await throws its own failure surface
                    return OpcodeResult::Error(VmError::RuntimeError(waitall_msg));
                }

                if all_completed {
                    // All tasks done - create result array
                    let mut result_arr = Array::new(task_count, task_count);
                    for (i, result) in results.into_iter().enumerate() {
                        let _ = result_arr.set(i, result);
                    }
                    let gc_ptr = self.gc.lock().allocate(result_arr);
                    let result_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    if let Err(e) = stack.push(result_val) {
                        return OpcodeResult::Error(e);
                    }
                    // `ip` already points to the next instruction when exec_concurrency_ops
                    // is called, so don't advance it here or we'll skip a byte and desync decode.
                    OpcodeResult::Continue
                } else {
                    // Not all complete - push array back and suspend
                    // When we resume, we'll re-execute WaitAll with the same array
                    if let Err(e) = stack.push(arr_val) {
                        return OpcodeResult::Error(e);
                    }
                    // Decrement ip to re-execute WaitAll when resumed
                    // We modify the local ip so that when run() calls task.set_ip(ip),
                    // it will point back to the WaitAll opcode
                    *ip -= 1;

                    // Register as waiter on first incomplete task
                    if let Some(awaited_id) = first_incomplete {
                        let tasks_guard = self.tasks.read();
                        if let Some(awaited_task) = tasks_guard.get(&awaited_id) {
                            if awaited_task.add_waiter_if_incomplete(task.id()) {
                                drop(tasks_guard);
                                return OpcodeResult::Suspend(SuspendReason::AwaitTask(awaited_id));
                            }
                        }
                        drop(tasks_guard);
                        OpcodeResult::Continue
                    } else {
                        // Shouldn't happen
                        OpcodeResult::Continue
                    }
                }
            }

            Opcode::Sleep => {
                let duration_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let ms = duration_val.as_i64().unwrap_or(0) as u64;
                let wake_at = Instant::now() + std::time::Duration::from_millis(ms);

                // Suspend until wake time - scheduler will wake us up
                OpcodeResult::Suspend(SuspendReason::Sleep { wake_at })
            }

            Opcode::MutexLock => {
                let mutex_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mutex_id = MutexId::from_u64(mutex_id_val.as_i64().unwrap_or(0) as u64);

                // Try to acquire the lock
                if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                    match mutex.try_lock(task.id()) {
                        Ok(()) => {
                            // Acquired immediately
                            task.add_held_mutex(mutex_id);
                            OpcodeResult::Continue
                        }
                        Err(_) => {
                            // Need to wait - suspend
                            OpcodeResult::Suspend(SuspendReason::MutexLock { mutex_id })
                        }
                    }
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Mutex {:?} not found",
                        mutex_id
                    )))
                }
            }

            Opcode::MutexUnlock => {
                let mutex_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let mutex_id = MutexId::from_u64(mutex_id_val.as_i64().unwrap_or(0) as u64);

                if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                    match mutex.unlock(task.id()) {
                        Ok(next_waiter) => {
                            task.remove_held_mutex(mutex_id);

                            // If there's a waiting task, wake it up
                            if let Some(waiter_id) = next_waiter {
                                let tasks = self.tasks.read();
                                if let Some(waiter_task) = tasks.get(&waiter_id) {
                                    // Only wake tasks that have already finished parking.
                                    // If the waiter is still Running, the reactor will notice that
                                    // ownership was transferred once its suspend result is handled.
                                    if waiter_task.try_resume() {
                                        waiter_task.add_held_mutex(mutex_id);
                                        if matches!(
                                            waiter_task.suspend_reason(),
                                            Some(crate::vm::scheduler::SuspendReason::MutexLockCall { .. })
                                        ) {
                                            waiter_task.set_resume_value(Value::null());
                                        }
                                        waiter_task.clear_suspend_reason();
                                        self.injector.push(waiter_task.clone());
                                    }
                                }
                            }
                            OpcodeResult::Continue
                        }
                        Err(e) => OpcodeResult::Error(VmError::RuntimeError(format!("{}", e))),
                    }
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Mutex {:?} not found",
                        mutex_id
                    )))
                }
            }

            Opcode::SemAcquire => {
                let count_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let sem_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let count = count_val.as_i64().filter(|v| *v >= 0).unwrap_or(0) as usize;
                let semaphore_id = SemaphoreId::from_u64(sem_id_val.as_i64().unwrap_or(0) as u64);

                if let Some(semaphore) = self.semaphore_registry.get(semaphore_id) {
                    match semaphore.try_acquire(task.id(), count) {
                        Ok(()) => OpcodeResult::Continue,
                        Err(_) => {
                            OpcodeResult::Suspend(SuspendReason::SemaphoreAcquire { semaphore_id })
                        }
                    }
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Semaphore {:?} not found",
                        semaphore_id
                    )))
                }
            }

            Opcode::SemRelease => {
                let count_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let sem_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let count = count_val.as_i64().filter(|v| *v >= 0).unwrap_or(0) as usize;
                let semaphore_id = SemaphoreId::from_u64(sem_id_val.as_i64().unwrap_or(0) as u64);

                if let Some(semaphore) = self.semaphore_registry.get(semaphore_id) {
                    match semaphore.release(count) {
                        Ok(resumed_tasks) => {
                            let tasks = self.tasks.read();
                            for waiter_id in resumed_tasks {
                                if let Some(waiter_task) = tasks.get(&waiter_id) {
                                    waiter_task.set_state(TaskState::Resumed);
                                    waiter_task.clear_suspend_reason();
                                    self.injector.push(waiter_task.clone());
                                }
                            }
                            OpcodeResult::Continue
                        }
                        Err(e) => OpcodeResult::Error(VmError::RuntimeError(format!("{}", e))),
                    }
                } else {
                    OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Semaphore {:?} not found",
                        semaphore_id
                    )))
                }
            }

            Opcode::Yield => {
                // Voluntary yield - suspend with immediate wake
                OpcodeResult::Suspend(SuspendReason::Sleep {
                    wake_at: Instant::now(),
                })
            }

            Opcode::TaskCancel => {
                // Pop task ID from stack
                let task_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let handle = match self.promise_handle_from_value(task_id_val) {
                    Some(handle) => handle,
                    None => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "TaskCancel: expected task-backed promise handle".to_string(),
                        ));
                    }
                };

                let target_id = handle.task_id();

                // Look up the task and cancel it
                if let Some(target_task) = self.tasks.read().get(&target_id).cloned() {
                    target_task.cancel();
                }
                // Silently ignore if task not found (may have already completed)

                OpcodeResult::Continue
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_concurrency_ops: {:?}",
                opcode
            ))),
        }
    }
}
