//! Register-based concurrency opcode handlers

use crate::compiler::bytecode::reg_opcode::{RegInstr, RegOpcode};
use crate::vm::interpreter::reg_execution::RegOpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::object::{Array, Closure};
use crate::vm::register_file::RegisterFile;
use crate::vm::scheduler::{SuspendReason, Task, TaskId, TaskState};
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use crate::vm::VmError;
use std::sync::Arc;
use std::time::Instant;

impl<'a> Interpreter<'a> {
    pub(in crate::vm::interpreter) fn exec_reg_concurrency_ops(
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
            RegOpcode::Spawn => {
                // rA = spawn func(rB..rB+C-1); extra = func_id (extended)
                let dest_reg = instr.a();
                let arg_base = instr.b();
                let arg_count = instr.c() as usize;
                let func_id = extra as usize;

                let mut args = Vec::with_capacity(arg_count);
                for i in 0..arg_count {
                    let val = match regs.get_reg(reg_base, arg_base.wrapping_add(i as u8)) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };
                    args.push(val);
                }

                let new_task = Arc::new(Task::with_args(
                    func_id,
                    task.module().clone(),
                    Some(task.id()),
                    args,
                ));

                let task_id = new_task.id();
                self.tasks.write().insert(task_id, new_task.clone());
                self.injector.push(new_task);

                if let Err(e) = regs.set_reg(reg_base, dest_reg, Value::u64(task_id.as_u64())) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::SpawnClosure => {
                // rA = spawn rB(rB+1..rB+C-1) (closure spawn, ABC format)
                let dest_reg = instr.a();
                let closure_reg = instr.b();
                let arg_count = instr.c() as usize;

                let closure_val = match regs.get_reg(reg_base, closure_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !closure_val.is_ptr() {
                    return RegOpcodeResult::runtime_error("Expected closure for SpawnClosure");
                }

                let closure_ptr = unsafe { closure_val.as_ptr::<Closure>() };
                let closure = unsafe { &*closure_ptr.unwrap().as_ptr() };
                let func_id = closure.func_id;

                let mut args = Vec::with_capacity(arg_count);
                for i in 0..arg_count {
                    let val = match regs.get_reg(
                        reg_base,
                        closure_reg.wrapping_add(1).wrapping_add(i as u8),
                    ) {
                        Ok(v) => v,
                        Err(e) => return RegOpcodeResult::Error(e),
                    };
                    args.push(val);
                }

                let new_task = Arc::new(Task::with_args(
                    func_id,
                    task.module().clone(),
                    Some(task.id()),
                    args,
                ));
                new_task.push_closure(closure_val);

                let task_id = new_task.id();
                self.tasks.write().insert(task_id, new_task.clone());
                self.injector.push(new_task);

                if let Err(e) = regs.set_reg(reg_base, dest_reg, Value::u64(task_id.as_u64())) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::Await => {
                // rA = await rB (C unused)
                let dest_reg = instr.a();
                let task_reg = instr.b();

                let task_id_val = match regs.get_reg(reg_base, task_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let task_id_u64 = match task_id_val.as_u64() {
                    Some(id) => id,
                    None => {
                        return RegOpcodeResult::runtime_error("Expected TaskId for Await");
                    }
                };

                let awaited_id = TaskId::from_u64(task_id_u64);

                let tasks_guard = self.tasks.read();
                if let Some(awaited_task) = tasks_guard.get(&awaited_id).cloned() {
                    drop(tasks_guard);
                    match awaited_task.state() {
                        TaskState::Completed => {
                            let result = awaited_task.result().unwrap_or(Value::null());
                            if let Err(e) = regs.set_reg(reg_base, dest_reg, result) {
                                return RegOpcodeResult::Error(e);
                            }
                            RegOpcodeResult::Continue
                        }
                        TaskState::Failed => {
                            if let Some(exc) = awaited_task.current_exception() {
                                task.set_exception(exc);
                            }
                            RegOpcodeResult::Error(VmError::RuntimeError(format!(
                                "Awaited task {:?} failed",
                                awaited_id
                            )))
                        }
                        _ => {
                            // Not done yet — set resume dest and suspend
                            task.set_resume_reg_dest(dest_reg);
                            awaited_task.add_waiter(task.id());
                            RegOpcodeResult::Suspend(SuspendReason::AwaitTask(awaited_id))
                        }
                    }
                } else {
                    drop(tasks_guard);
                    RegOpcodeResult::runtime_error(format!(
                        "Task {:?} not found",
                        awaited_id
                    ))
                }
            }

            RegOpcode::AwaitAll => {
                // rA = await_all rB (C unused, rB is array of tasks)
                let dest_reg = instr.a();
                let arr_reg = instr.b();

                let arr_val = match regs.get_reg(reg_base, arr_reg) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                if !arr_val.is_ptr() {
                    return RegOpcodeResult::runtime_error(
                        "AwaitAll expects an array of tasks",
                    );
                }

                let arr_ptr = match unsafe { arr_val.as_ptr::<Array>() } {
                    Some(p) => p,
                    None => {
                        return RegOpcodeResult::runtime_error(
                            "Expected array for AwaitAll",
                        );
                    }
                };
                let arr = unsafe { &*arr_ptr.as_ptr() };
                let task_count = arr.len();

                let mut results = Vec::with_capacity(task_count);
                let mut all_completed = true;
                let mut first_incomplete: Option<TaskId> = None;

                {
                    let tasks_guard = self.tasks.read();
                    for i in 0..task_count {
                        let elem = arr.get(i).unwrap_or(Value::null());
                        let task_id_u64 = match elem.as_u64() {
                            Some(id) => id,
                            None => {
                                return RegOpcodeResult::runtime_error(
                                    "Expected TaskId in array",
                                );
                            }
                        };
                        let awaited_id = TaskId::from_u64(task_id_u64);

                        if let Some(awaited_task) = tasks_guard.get(&awaited_id) {
                            match awaited_task.state() {
                                TaskState::Completed => {
                                    let result =
                                        awaited_task.result().unwrap_or(Value::null());
                                    results.push(result);
                                }
                                TaskState::Failed => {
                                    let exc = awaited_task.current_exception();
                                    drop(tasks_guard);
                                    if let Some(exc_val) = exc {
                                        task.set_exception(exc_val);
                                    }
                                    return RegOpcodeResult::Error(VmError::RuntimeError(
                                        format!(
                                            "Awaited task {:?} failed in AwaitAll",
                                            awaited_id
                                        ),
                                    ));
                                }
                                _ => {
                                    all_completed = false;
                                    if first_incomplete.is_none() {
                                        first_incomplete = Some(awaited_id);
                                    }
                                    results.push(Value::null());
                                }
                            }
                        } else {
                            drop(tasks_guard);
                            return RegOpcodeResult::runtime_error(format!(
                                "Task {:?} not found in AwaitAll",
                                awaited_id
                            ));
                        }
                    }
                }

                if all_completed {
                    // Create result array
                    let mut result_arr = Array::new(task_count, task_count);
                    for (i, result) in results.into_iter().enumerate() {
                        let _ = result_arr.set(i, result);
                    }
                    let gc_ptr = self.gc.lock().allocate(result_arr);
                    let result_val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    if let Err(e) = regs.set_reg(reg_base, dest_reg, result_val) {
                        return RegOpcodeResult::Error(e);
                    }
                    RegOpcodeResult::Continue
                } else {
                    // Not all complete — suspend and re-execute this instruction on resume
                    // The dispatch loop will decrement IP before suspending
                    task.set_resume_reg_dest(dest_reg);

                    if let Some(awaited_id) = first_incomplete {
                        let tasks_guard = self.tasks.read();
                        if let Some(awaited_task) = tasks_guard.get(&awaited_id) {
                            awaited_task.add_waiter(task.id());
                        }
                        drop(tasks_guard);
                        RegOpcodeResult::Suspend(SuspendReason::AwaitTask(awaited_id))
                    } else {
                        RegOpcodeResult::Continue
                    }
                }
            }

            RegOpcode::Sleep => {
                // sleep rA ms (B, C unused)
                let ms_val = match regs.get_reg(reg_base, instr.a()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let ms = ms_val.as_i64().unwrap_or(0) as u64;
                let wake_at = Instant::now() + std::time::Duration::from_millis(ms);
                RegOpcodeResult::Suspend(SuspendReason::Sleep { wake_at })
            }

            RegOpcode::Yield => {
                // yield to scheduler
                RegOpcodeResult::Suspend(SuspendReason::Sleep {
                    wake_at: Instant::now(),
                })
            }

            RegOpcode::NewMutex => {
                // rA = new Mutex (B, C unused)
                let dest_reg = instr.a();

                let (mutex_id, _) = self.mutex_registry.create_mutex();
                let val = Value::u64(mutex_id.as_u64());

                if let Err(e) = regs.set_reg(reg_base, dest_reg, val) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::MutexLock => {
                // lock rA (B, C unused)
                let mutex_val = match regs.get_reg(reg_base, instr.a()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let mutex_id = MutexId::from_u64(mutex_val.as_u64().unwrap_or(0));

                if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                    match mutex.try_lock(task.id()) {
                        Ok(()) => {
                            task.add_held_mutex(mutex_id);
                            RegOpcodeResult::Continue
                        }
                        Err(_) => {
                            RegOpcodeResult::Suspend(SuspendReason::MutexLock { mutex_id })
                        }
                    }
                } else {
                    RegOpcodeResult::runtime_error(format!(
                        "Mutex {:?} not found",
                        mutex_id
                    ))
                }
            }

            RegOpcode::MutexUnlock => {
                // unlock rA (B, C unused)
                let mutex_val = match regs.get_reg(reg_base, instr.a()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let mutex_id = MutexId::from_u64(mutex_val.as_u64().unwrap_or(0));

                if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                    match mutex.unlock(task.id()) {
                        Ok(next_waiter) => {
                            task.remove_held_mutex(mutex_id);

                            if let Some(waiter_id) = next_waiter {
                                let tasks = self.tasks.read();
                                if let Some(waiter_task) = tasks.get(&waiter_id) {
                                    waiter_task.add_held_mutex(mutex_id);
                                    waiter_task.set_state(TaskState::Resumed);
                                    waiter_task.clear_suspend_reason();
                                    self.injector.push(waiter_task.clone());
                                }
                            }
                            RegOpcodeResult::Continue
                        }
                        Err(e) => {
                            RegOpcodeResult::runtime_error(format!("{}", e))
                        }
                    }
                } else {
                    RegOpcodeResult::runtime_error(format!(
                        "Mutex {:?} not found",
                        mutex_id
                    ))
                }
            }

            RegOpcode::NewChannel => {
                // rA = new Channel(rB) (C unused)
                // rB = buffer capacity
                let dest_reg = instr.a();
                let cap_val = match regs.get_reg(reg_base, instr.b()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };
                let _capacity = cap_val.as_i32().unwrap_or(0) as usize;

                // Channel creation placeholder — channels need registry like mutexes
                // For now, return null (will be implemented with channel registry)
                if let Err(e) = regs.set_reg(reg_base, dest_reg, Value::null()) {
                    return RegOpcodeResult::Error(e);
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::TaskCancel => {
                // cancel rA (B, C unused)
                let task_id_val = match regs.get_reg(reg_base, instr.a()) {
                    Ok(v) => v,
                    Err(e) => return RegOpcodeResult::Error(e),
                };

                let task_id_u64 = match task_id_val.as_u64() {
                    Some(id) => id,
                    None => {
                        return RegOpcodeResult::runtime_error(
                            "TaskCancel: expected task handle (u64)",
                        );
                    }
                };

                let target_id = TaskId::from_u64(task_id_u64);

                if let Some(target_task) = self.tasks.read().get(&target_id).cloned() {
                    target_task.cancel();
                }
                RegOpcodeResult::Continue
            }

            RegOpcode::TaskThen => {
                // rA.then(rB); extra = func_id (extended)
                // Register a callback to run when task completes
                // For now, this is a no-op placeholder
                RegOpcodeResult::Continue
            }

            _ => RegOpcodeResult::runtime_error(format!(
                "Not a concurrency opcode: {:?}",
                opcode
            )),
        }
    }
}
