#![allow(missing_docs)]
//! AOT task executor
//!
//! Bridges AOT-compiled functions with the scheduler's Task model.
//! Called from the VM worker loop as an alternative to the bytecode interpreter.
//!
//! The executor handles:
//! - First call: allocate frame, load arguments, call compiled function
//! - Resume: restore frame, set resume value, re-enter at saved resume point
//! - Completion: convert NaN-boxed result to Value, free frame chain
//! - Suspension: decode AOT suspend transport into the shared runtime reason

use std::time::{Duration, Instant};
use std::sync::Arc;

use crate::vm::interpreter::{ExecutionResult, SharedVmState};
use crate::vm::scheduler::{SuspendReason as SchedulerSuspendReason, Task};
use crate::vm::suspend::{ResumePolicy, ResumeRecord, SuspendRecord, SuspendTag, TaskId};
use crate::vm::value::Value;
use crate::vm::VmError;

use super::abi;
use super::frame::{AotEntryFn, AotFrame, AotHelperTable, AotTaskContext, AOT_SUSPEND};
use super::helpers::create_default_helper_table;

// =============================================================================
// Executor result
// =============================================================================

/// Result of running an AOT task. Includes the frame for persistence.
pub struct AotRunResult {
    /// The execution result (Completed/Suspended/Failed).
    pub result: ExecutionResult,
    /// The root frame. Non-null on suspension (caller must save it).
    /// Null on completion or failure (frame has been freed).
    pub frame: *mut AotFrame,
}

// =============================================================================
// Main executor
// =============================================================================

/// Run an AOT-compiled function to completion or suspension.
///
/// This is the AOT equivalent of `Interpreter::run()`. The caller is responsible
/// for building the frame and context — this function just calls the compiled
/// code and interprets the result.
///
/// # Arguments
///
/// - `frame`: The function's heap-allocated frame. On first call, `resume_point`
///   should be 0 and locals should contain the arguments. On resume, the frame
///   is restored from the previous suspension.
/// - `ctx`: Task context with helpers, preemption flag, and resume value.
/// - `max_preemptions`: Maximum consecutive preemptions before killing the task.
///
/// # Returns
///
/// An `AotRunResult` containing the execution result and the frame pointer.
/// On suspension, the frame is preserved (non-null) for later resumption.
/// On completion or failure, the frame chain is freed (null).
///
/// # Safety
///
/// The caller must ensure:
/// - `frame` is a valid, non-null pointer to a properly initialized AotFrame
/// - `ctx` is a valid, non-null pointer to a properly initialized AotTaskContext
/// - The function pointer in the frame is valid compiled code
pub unsafe fn run_aot_function(
    frame: *mut AotFrame,
    ctx: *mut AotTaskContext,
    _max_preemptions: u32,
) -> AotRunResult {
    debug_assert!(!frame.is_null(), "AOT frame must not be null");
    debug_assert!(!ctx.is_null(), "AOT context must not be null");

    let func_ptr = (*frame).function_ptr;
    let result = func_ptr(frame, ctx);

    if result == AOT_SUSPEND {
        let record = (*ctx).suspend_record;
        if record.tag == SuspendTag::None {
            free_frame_chain(frame, &(*ctx).helpers);
            AotRunResult {
                result: ExecutionResult::Failed(VmError::RuntimeError(
                    "AOT function returned AOT_SUSPEND with no suspend reason".to_string(),
                )),
                frame: std::ptr::null_mut(),
            }
        } else if let Some(reason) = record.to_runtime_reason() {
            AotRunResult {
                result: ExecutionResult::Suspended(reason),
                frame,
            }
        } else {
            free_frame_chain(frame, &(*ctx).helpers);
            AotRunResult {
                result: ExecutionResult::Failed(VmError::RuntimeError(format!(
                    "AOT: unhandled suspend tag {:?}",
                    record.tag
                ))),
                frame: std::ptr::null_mut(),
            }
        }
    } else {
        // Completed — convert NaN-boxed result to Value
        let value = Value::from_raw(result);
        free_frame_chain(frame, &(*ctx).helpers);
        AotRunResult {
            result: ExecutionResult::Completed(value),
            frame: std::ptr::null_mut(),
        }
    }
}

/// Run a scheduler-owned AOT task using the shared suspend/resume contract.
///
/// The task carries its compiled entry metadata and any preserved root frame.
/// The scheduler/worker provides the shared VM state; this helper builds a
/// fresh `AotTaskContext`, restores resume values when needed, and persists the
/// root frame back onto the task on suspension.
pub fn run_scheduled_aot_task(
    task: &Arc<Task>,
    shared_state: &SharedVmState,
    max_preemptions: u32,
) -> AotRunResult {
    let Some((local_count, entry_fn)) = task.aot_entry() else {
        return AotRunResult {
            result: ExecutionResult::Failed(VmError::RuntimeError(
                "Scheduled AOT task is missing compiled entry metadata".to_string(),
            )),
            frame: std::ptr::null_mut(),
        };
    };

    let current_module = task.current_module();
    let mut ctx = build_task_context(
        task.preempt_flag_ptr(),
        create_default_helper_table(),
        None,
    );
    ctx.shared_state = shared_state as *const _ as *mut ();
    ctx.current_task = Arc::as_ptr(task) as *mut ();
    ctx.module = Arc::as_ptr(&current_module) as *const ();

    let frame = match task.take_aot_frame().filter(|frame| !frame.is_null()) {
        Some(saved_frame) => {
            unsafe {
                prepare_resume(&mut ctx, task.take_resume_record());
            }
            saved_frame
        }
        None => {
            let initial_args = task.take_initial_args();
            let frame = unsafe {
                allocate_initial_frame(
                    task.function_id() as u32,
                    local_count,
                    entry_fn,
                    &initial_args,
                    &ctx.helpers,
                )
            };
            if frame.is_null() {
                return AotRunResult {
                    result: ExecutionResult::Failed(VmError::RuntimeError(
                        "AOT task entry frame allocation failed".to_string(),
                    )),
                    frame: std::ptr::null_mut(),
                };
            }
            frame
        }
    };

    let result = unsafe { run_aot_function(frame, &mut ctx, max_preemptions) };
    if matches!(result.result, ExecutionResult::Suspended(_)) {
        task.store_aot_frame(result.frame);
    } else {
        task.store_aot_frame(std::ptr::null_mut());
    }
    result
}

// =============================================================================
// Frame management
// =============================================================================

/// Allocate a root frame for a function's first invocation.
///
/// Loads initial arguments into the frame's locals.
///
/// # Safety
///
/// Caller must ensure `helpers.alloc_frame` is a valid function pointer and
/// that `args.len()` does not exceed `local_count`.
pub unsafe fn allocate_initial_frame(
    function_id: u32,
    local_count: u32,
    func_ptr: AotEntryFn,
    args: &[Value],
    helpers: &AotHelperTable,
) -> *mut AotFrame {
    let frame = (helpers.alloc_frame)(function_id, local_count, func_ptr);
    if frame.is_null() {
        return frame;
    }

    // Set param_count
    (*frame).param_count = args.len() as u32;

    // Load arguments into locals[0..arg_count]
    let locals = (*frame).locals;
    for (i, arg) in args.iter().enumerate() {
        if (i as u32) < local_count {
            *locals.add(i) = arg.raw();
        }
    }

    frame
}

/// Prepare an existing frame for resumption.
///
/// Sets the resume value in the context. The frame's `resume_point` is
/// already set by the compiled code before it returned AOT_SUSPEND.
///
/// # Safety
///
/// Caller must ensure `ctx` is a valid, non-null pointer to an initialized `AotTaskContext`.
pub unsafe fn prepare_resume(ctx: *mut AotTaskContext, resume_record: ResumeRecord) {
    (*ctx).resume_record = resume_record;
    (*ctx).suspend_record.clear();
}

/// Build an `AotTaskContext` for a task execution.
///
/// The caller provides the preemption flag (from the Task's AtomicBool),
/// the helper table, and optional resume value.
pub fn build_task_context(
    preempt_flag: *const std::sync::atomic::AtomicBool,
    helpers: AotHelperTable,
    resume_record: Option<ResumeRecord>,
) -> AotTaskContext {
    AotTaskContext {
        preempt_requested: preempt_flag,
        resume_record: resume_record.unwrap_or_else(ResumeRecord::none),
        suspend_record: SuspendRecord::none(),
        helpers,
        shared_state: std::ptr::null_mut(),
        current_task: std::ptr::null_mut(),
        module: std::ptr::null(),
    }
}

/// Free an AotFrame and all its child frames.
///
/// Walks the `child_frame` chain and frees each frame.
///
/// # Safety
///
/// `frame` must be a valid pointer (or null) allocated by `helpers.alloc_frame`.
pub unsafe fn free_frame_chain(frame: *mut AotFrame, helpers: &AotHelperTable) {
    if frame.is_null() {
        return;
    }
    // Free child frames first (depth-first)
    let child = (*frame).child_frame;
    if !child.is_null() {
        free_frame_chain(child, helpers);
        (*frame).child_frame = std::ptr::null_mut();
    }
    (helpers.free_frame)(frame);
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aot::helpers::create_default_helper_table;
    use std::sync::atomic::AtomicBool;

    /// A test AOT function that immediately returns NaN-boxed i32(42).
    unsafe extern "C" fn test_return_42(_frame: *mut AotFrame, _ctx: *mut AotTaskContext) -> u64 {
        abi::I32_TAG_BASE | 42
    }

    /// A test AOT function that suspends with AwaitTask reason.
    unsafe extern "C" fn test_suspend_await(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        (*ctx)
            .suspend_record
            .set_reason(&SchedulerSuspendReason::AwaitTask(TaskId::from_u64(7)));
        AOT_SUSPEND
    }

    /// A test AOT function that suspends with Sleep reason.
    unsafe extern "C" fn test_suspend_sleep(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        (*ctx).suspend_record.set_reason(&SchedulerSuspendReason::Sleep {
            wake_at: Instant::now() + Duration::from_millis(100),
        });
        AOT_SUSPEND
    }

    /// A test AOT function that suspends with Preempted reason.
    unsafe extern "C" fn test_suspend_preempted(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        (*ctx).suspend_record.set_tag(SuspendTag::Preemption);
        AOT_SUSPEND
    }

    /// A test AOT function that suspends with KernelBoundary reason.
    unsafe extern "C" fn test_suspend_native_boundary(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        (*ctx).suspend_record.set_tag(SuspendTag::KernelBoundary);
        AOT_SUSPEND
    }

    /// A test AOT function that reads the resume payload and returns it.
    unsafe extern "C" fn test_resume_returns_value(
        frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        if (*frame).resume_point == 0 {
            // First call — suspend
            (*frame).resume_point = 1;
            (*ctx)
                .suspend_record
                .set_reason(&SchedulerSuspendReason::AwaitTask(TaskId::from_u64(1)));
            AOT_SUSPEND
        } else {
            // Resume — return the resume value
            (*ctx).resume_record.value
        }
    }

    /// Simulates compiled native-call path that completes immediately.
    unsafe extern "C" fn test_native_helper_fast(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        ((*ctx).helpers.native_call)(ctx, 1, std::ptr::null(), 0).payload
    }

    /// Simulates compiled native-call path that returns suspend token.
    unsafe extern "C" fn test_native_helper_suspend(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        ((*ctx).helpers.native_call)(ctx, 0x7FFF, std::ptr::null(), 0).payload
    }

    #[test]
    fn test_run_immediate_return() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);

        unsafe {
            let frame = allocate_initial_frame(0, 1, test_return_42, &[], &helpers);
            assert!(!frame.is_null());

            let mut ctx = build_task_context(&preempt, helpers, None);
            let result = run_aot_function(frame, &mut ctx, 100);

            match result.result {
                ExecutionResult::Completed(val) => {
                    assert!(val.is_i32(), "Expected i32, got tag={}", val.tag());
                    assert_eq!(val.as_i32(), Some(42));
                }
                other => panic!("Expected Completed, got {:?}", other),
            }
            assert!(
                result.frame.is_null(),
                "Frame should be freed on completion"
            );
        }
    }

    #[test]
    fn test_run_suspend_await() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);

        unsafe {
            let frame = allocate_initial_frame(0, 1, test_suspend_await, &[], &helpers);
            let mut ctx = build_task_context(&preempt, helpers, None);
            let result = run_aot_function(frame, &mut ctx, 100);

            match &result.result {
                ExecutionResult::Suspended(SchedulerSuspendReason::AwaitTask(tid)) => {
                    assert_eq!(tid.as_u64(), 7);
                }
                other => panic!("Expected Suspended(AwaitTask), got {:?}", other),
            }
            assert!(
                !result.frame.is_null(),
                "Frame should be preserved on suspend"
            );

            // Clean up
            free_frame_chain(result.frame, &helpers);
        }
    }

    #[test]
    fn test_run_suspend_sleep() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);
        let before = Instant::now();

        unsafe {
            let frame = allocate_initial_frame(0, 1, test_suspend_sleep, &[], &helpers);
            let mut ctx = build_task_context(&preempt, helpers, None);
            let result = run_aot_function(frame, &mut ctx, 100);

            match &result.result {
                ExecutionResult::Suspended(SchedulerSuspendReason::Sleep { wake_at }) => {
                    assert!(*wake_at >= before + Duration::from_millis(100));
                }
                other => panic!("Expected Suspended(Sleep), got {:?}", other),
            }

            free_frame_chain(result.frame, &helpers);
        }
    }

    #[test]
    fn test_run_suspend_preempted() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);

        unsafe {
            let frame = allocate_initial_frame(0, 1, test_suspend_preempted, &[], &helpers);
            let mut ctx = build_task_context(&preempt, helpers, None);
            let result = run_aot_function(frame, &mut ctx, 100);

            match &result.result {
                ExecutionResult::Suspended(SchedulerSuspendReason::Preemption) => {}
                other => panic!("Expected Suspended(Preemption), got {:?}", other),
            }

            free_frame_chain(result.frame, &helpers);
        }
    }

    #[test]
    fn test_run_suspend_native_call_boundary() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);

        unsafe {
            let frame = allocate_initial_frame(0, 1, test_suspend_native_boundary, &[], &helpers);
            let mut ctx = build_task_context(&preempt, helpers, None);
            let result = run_aot_function(frame, &mut ctx, 100);

            match &result.result {
                ExecutionResult::Suspended(SchedulerSuspendReason::KernelBoundary) => {}
                other => panic!("Expected Suspended(KernelBoundary), got {:?}", other),
            }

            free_frame_chain(result.frame, &helpers);
        }
    }

    #[test]
    fn test_resume_with_value() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);

        unsafe {
            // First call — should suspend
            let frame = allocate_initial_frame(0, 1, test_resume_returns_value, &[], &helpers);
            let mut ctx = build_task_context(&preempt, helpers, None);
            let result = run_aot_function(frame, &mut ctx, 100);

            assert!(!result.frame.is_null(), "Should suspend, not complete");
            match &result.result {
                ExecutionResult::Suspended(SchedulerSuspendReason::AwaitTask(_)) => {}
                other => panic!("Expected Suspended(AwaitTask), got {:?}", other),
            }

            // Resume with value i32(99)
            let resume_frame = result.frame;
            prepare_resume(&mut ctx, ResumeRecord::with_value(Value::i32(99)));
            let result2 = run_aot_function(resume_frame, &mut ctx, 100);

            match result2.result {
                ExecutionResult::Completed(val) => {
                    assert_eq!(val.as_i32(), Some(99));
                }
                other => panic!("Expected Completed(99), got {:?}", other),
            }
            assert!(result2.frame.is_null());
        }
    }

    #[test]
    fn test_run_native_helper_fast_path_completes() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);

        unsafe {
            let frame = allocate_initial_frame(0, 1, test_native_helper_fast, &[], &helpers);
            let mut ctx = build_task_context(&preempt, helpers, None);
            let result = run_aot_function(frame, &mut ctx, 100);

            match result.result {
                ExecutionResult::Completed(val) => {
                    assert_eq!(
                        val,
                        Value::null(),
                        "stub fast path should return null sentinel"
                    );
                }
                other => panic!("Expected Completed(null), got {:?}", other),
            }
            assert!(
                result.frame.is_null(),
                "Frame should be freed on completion"
            );
        }
    }

    #[test]
    fn test_run_native_helper_suspend_path_handoffs_to_thread_loop() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);

        unsafe {
            let frame = allocate_initial_frame(0, 1, test_native_helper_suspend, &[], &helpers);
            let mut ctx = build_task_context(&preempt, helpers, None);
            let result = run_aot_function(frame, &mut ctx, 100);

            match &result.result {
                ExecutionResult::Suspended(SchedulerSuspendReason::KernelBoundary) => {}
                other => panic!("Expected Suspended(KernelBoundary), got {:?}", other),
            }
            assert!(
                !result.frame.is_null(),
                "Frame should be preserved on native-boundary suspension"
            );

            free_frame_chain(result.frame, &helpers);
        }
    }

    #[test]
    fn test_allocate_frame_with_args() {
        let helpers = create_default_helper_table();

        unsafe {
            let args = [Value::i32(10), Value::i32(20), Value::i32(30)];
            let frame = allocate_initial_frame(42, 5, test_return_42, &args, &helpers);
            assert!(!frame.is_null());

            assert_eq!((*frame).function_id, 42);
            assert_eq!((*frame).local_count, 5);
            assert_eq!((*frame).param_count, 3);
            assert_eq!((*frame).resume_point, 0);

            // Check args were loaded into locals
            assert_eq!(*(*frame).locals.add(0), Value::i32(10).raw());
            assert_eq!(*(*frame).locals.add(1), Value::i32(20).raw());
            assert_eq!(*(*frame).locals.add(2), Value::i32(30).raw());

            // Remaining locals should be null (from alloc_zeroed → then set to NULL_VALUE)
            assert_eq!(*(*frame).locals.add(3), abi::NULL_VALUE);
            assert_eq!(*(*frame).locals.add(4), abi::NULL_VALUE);

            free_frame_chain(frame, &helpers);
        }
    }

    #[test]
    fn test_build_task_context() {
        let helpers = create_default_helper_table();
        let preempt = AtomicBool::new(false);

        let ctx = build_task_context(
            &preempt,
            helpers,
            Some(ResumeRecord::with_value(Value::i32(42))),
        );
        assert_eq!(ctx.resume_record.value, Value::i32(42).raw());
        assert_eq!(ctx.suspend_record, SuspendRecord::none());

        let ctx2 = build_task_context(&preempt, helpers, None);
        assert!(ctx2.resume_record.is_none());
    }

    #[test]
    fn test_suspend_record_roundtrip() {
        let mut record = SuspendRecord::none();
        record.set_reason(&SchedulerSuspendReason::AwaitTask(TaskId::from_u64(42)));
        assert!(matches!(
            record.to_runtime_reason(),
            Some(SchedulerSuspendReason::AwaitTask(tid)) if tid.as_u64() == 42
        ));

        record.set_reason(&SchedulerSuspendReason::MutexAcquire {
            mutex_id: crate::vm::sync::MutexId::from_u64(5),
            resume_policy: ResumePolicy::ReturnNull,
        });
        assert!(matches!(
            record.to_runtime_reason(),
            Some(SchedulerSuspendReason::MutexAcquire {
                resume_policy: ResumePolicy::ReturnNull,
                ..
            })
        ));

        record.set_tag(SuspendTag::KernelBoundary);
        assert!(matches!(
            record.to_runtime_reason(),
            Some(SchedulerSuspendReason::KernelBoundary)
        ));
    }

    #[test]
    fn test_free_frame_chain_null() {
        let helpers = create_default_helper_table();
        // Should not panic
        unsafe {
            free_frame_chain(std::ptr::null_mut(), &helpers);
        }
    }

    #[test]
    fn test_free_frame_chain_with_children() {
        let helpers = create_default_helper_table();

        unsafe {
            // Build a chain: parent -> child -> grandchild
            let grandchild = (helpers.alloc_frame)(3, 1, test_return_42);
            let child = (helpers.alloc_frame)(2, 1, test_return_42);
            let parent = (helpers.alloc_frame)(1, 1, test_return_42);

            (*child).child_frame = grandchild;
            (*parent).child_frame = child;

            // Free the whole chain — should not panic or leak
            free_frame_chain(parent, &helpers);
        }
    }
}
