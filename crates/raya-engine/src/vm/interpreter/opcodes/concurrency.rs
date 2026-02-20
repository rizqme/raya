//! Concurrency opcode handlers: Spawn, SpawnClosure, Await, WaitAll, Sleep, MutexLock, MutexUnlock, Yield, TaskCancel

use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, Closure};
use crate::vm::scheduler::{SuspendReason, Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use crate::vm::VmError;
use crate::compiler::{Module, Opcode};
use std::sync::Arc;
use std::time::Instant;

impl<'a> Interpreter<'a> {
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
                let func_index = match Self::read_u16(code, ip) {
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

                let new_task = Arc::new(Task::with_args(
                    func_index,
                    task.module().clone(),
                    Some(task.id()),
                    args,
                ));

                let task_id = new_task.id();
                self.tasks.write().insert(task_id, new_task.clone());
                self.injector.push(new_task);

                if let Err(e) = stack.push(Value::u64(task_id.as_u64())) {
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
                if !closure_val.is_ptr() {
                    return OpcodeResult::Error(VmError::TypeError("Expected closure".to_string()));
                }

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };

                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }

                // Don't prepend captures to args - the closure body uses LoadCaptured
                // which reads from the Closure object via task.current_closure()
                let new_task = Arc::new(Task::with_args(
                    closure.func_id,
                    task.module().clone(),
                    Some(task.id()),
                    args,
                ));

                // Push the closure onto the spawned task's closure stack
                // so LoadCaptured can find it when the task starts executing
                new_task.push_closure(closure_val);

                let task_id = new_task.id();
                self.tasks.write().insert(task_id, new_task.clone());
                self.injector.push(new_task);

                if let Err(e) = stack.push(Value::u64(task_id.as_u64())) {
                    return OpcodeResult::Error(e);
                }
                OpcodeResult::Continue
            }

            Opcode::Await => {
                let task_id_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                let task_id_u64 = match task_id_val.as_u64() {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "Expected TaskId".to_string(),
                        ));
                    }
                };

                let awaited_id = TaskId::from_u64(task_id_u64);

                // Check if the awaited task is already complete
                let tasks_guard = self.tasks.read();
                if let Some(awaited_task) = tasks_guard.get(&awaited_id).cloned() {
                    drop(tasks_guard);
                    match awaited_task.state() {
                        TaskState::Completed => {
                            // Already done, push result
                            let result = awaited_task.result().unwrap_or(Value::null());
                            if let Err(e) = stack.push(result) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        TaskState::Failed => {
                            // Propagate exception
                            if let Some(exc) = awaited_task.current_exception() {
                                task.set_exception(exc);
                            }
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Awaited task {:?} failed",
                                awaited_id
                            )));
                        }
                        _ => {
                            // Not done yet - register as waiter and suspend
                            awaited_task.add_waiter(task.id());
                            return OpcodeResult::Suspend(SuspendReason::AwaitTask(awaited_id));
                        }
                    }
                } else {
                    drop(tasks_guard);
                    return OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Task {:?} not found",
                        awaited_id
                    )));
                }
            }

            Opcode::WaitAll => {
                // WaitAll: await [task1, task2, ...] - wait for all tasks and return results array
                // Note: When resumed after awaiting, run() pushes a resume value.
                // We need to handle this by checking if we got an array or a resume value.
                let top_val = match stack.pop() {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Check if this is the array we need, or a resume value from a previous await
                let arr_val = if top_val.is_ptr() {
                    // Could be array or something else
                    if unsafe { top_val.as_ptr::<Array>() }.is_some() {
                        // Looks like an array - verify it contains task IDs
                        top_val
                    } else {
                        // Not an array - this is a resume value, pop the real array
                        match stack.pop() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        }
                    }
                } else {
                    // This is a resume value (probably a number), pop the real array
                    match stack.pop() {
                        Ok(v) => v,
                        Err(e) => return OpcodeResult::Error(e),
                    }
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
                let mut task_ids = Vec::with_capacity(task_count);
                let mut results = Vec::with_capacity(task_count);
                let mut all_completed = true;
                let mut first_incomplete: Option<TaskId> = None;
                let mut failed_task_info: Option<(TaskId, Option<Value>)> = None;
                let mut missing_task: Option<TaskId> = None;

                {
                    let tasks_guard = self.tasks.read();
                    for i in 0..task_count {
                        let elem = arr.get(i).unwrap_or(Value::null());
                        let task_id_u64 = match elem.as_u64() {
                            Some(id) => id,
                            None => {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Expected TaskId in array".to_string(),
                                ));
                            }
                        };
                        let awaited_id = TaskId::from_u64(task_id_u64);
                        task_ids.push(awaited_id);

                        if let Some(awaited_task) = tasks_guard.get(&awaited_id) {
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
                            missing_task = Some(awaited_id);
                            break;
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
                    if let Some(exc_val) = exc {
                        task.set_exception(exc_val);
                    }
                    return OpcodeResult::Error(VmError::RuntimeError(format!(
                        "Awaited task {:?} failed in WaitAll",
                        awaited_id
                    )));
                }

                if all_completed {
                    // All tasks done - create result array
                    let mut result_arr = Array::new(task_count, task_count);
                    for (i, result) in results.into_iter().enumerate() {
                        let _ = result_arr.set(i, result);
                    }
                    let gc_ptr = self.gc.lock().allocate(result_arr);
                    let result_val =
                        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
                    if let Err(e) = stack.push(result_val) {
                        return OpcodeResult::Error(e);
                    }
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
                            awaited_task.add_waiter(task.id());
                        }
                        drop(tasks_guard);
                        OpcodeResult::Suspend(SuspendReason::AwaitTask(awaited_id))
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
                                    // The mutex is now owned by the waiter (set by mutex.unlock)
                                    waiter_task.add_held_mutex(mutex_id);
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
                        "Mutex {:?} not found",
                        mutex_id
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

                let task_id_u64 = match task_id_val.as_u64() {
                    Some(id) => id,
                    None => {
                        return OpcodeResult::Error(VmError::TypeError(
                            "TaskCancel: expected task handle (u64)".to_string(),
                        ));
                    }
                };

                let target_id = TaskId::from_u64(task_id_u64);

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
