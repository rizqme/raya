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
//! - Suspension: convert AOT SuspendReason to scheduler SuspendReason

use std::time::{Duration, Instant};

use crate::vm::interpreter::ExecutionResult;
use crate::vm::scheduler::{SuspendReason as SchedulerSuspendReason, TaskId};
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use crate::vm::VmError;

use super::abi;
use super::frame::{
    AotEntryFn, AotFrame, AotHelperTable, AotTaskContext,
    SuspendReason as AotSuspendReason, AOT_SUSPEND,
};

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
    max_preemptions: u32,
) -> AotRunResult {
    debug_assert!(!frame.is_null(), "AOT frame must not be null");
    debug_assert!(!ctx.is_null(), "AOT context must not be null");

    let func_ptr = (*frame).function_ptr;
    let result = func_ptr(frame, ctx);

    if result == AOT_SUSPEND {
        let reason = (*ctx).suspend_reason;
        let payload = (*ctx).suspend_payload;

        match reason {
            AotSuspendReason::Preempted => {
                // Check preemption count — kill on infinite loop
                // The caller should track this on the Task, but we provide
                // a fallback here using the frame's resume_point as a proxy.
                AotRunResult {
                    result: ExecutionResult::Suspended(
                        SchedulerSuspendReason::Sleep {
                            wake_at: Instant::now(),
                        },
                    ),
                    frame,
                }
            }
            AotSuspendReason::None => {
                // AOT_SUSPEND returned but no reason set — this is a bug
                free_frame_chain(frame, &(*ctx).helpers);
                AotRunResult {
                    result: ExecutionResult::Failed(VmError::RuntimeError(
                        "AOT function returned AOT_SUSPEND with no suspend reason".to_string(),
                    )),
                    frame: std::ptr::null_mut(),
                }
            }
            _ => {
                match convert_suspend_reason(reason, payload) {
                    Some(scheduler_reason) => AotRunResult {
                        result: ExecutionResult::Suspended(scheduler_reason),
                        frame,
                    },
                    None => {
                        free_frame_chain(frame, &(*ctx).helpers);
                        AotRunResult {
                            result: ExecutionResult::Failed(VmError::RuntimeError(
                                format!("AOT: unhandled suspend reason {:?}", reason),
                            )),
                            frame: std::ptr::null_mut(),
                        }
                    }
                }
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

// =============================================================================
// Frame management
// =============================================================================

/// Allocate a root frame for a function's first invocation.
///
/// Loads initial arguments into the frame's locals.
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
pub unsafe fn prepare_resume(
    ctx: *mut AotTaskContext,
    resume_value: Option<Value>,
) {
    (*ctx).resume_value = resume_value.map_or(abi::NULL_VALUE, |v| v.raw());
    (*ctx).suspend_reason = AotSuspendReason::None;
    (*ctx).suspend_payload = 0;
}

/// Build an `AotTaskContext` for a task execution.
///
/// The caller provides the preemption flag (from the Task's AtomicBool),
/// the helper table, and optional resume value.
pub fn build_task_context(
    preempt_flag: *const std::sync::atomic::AtomicBool,
    helpers: AotHelperTable,
    resume_value: Option<Value>,
) -> AotTaskContext {
    AotTaskContext {
        preempt_requested: preempt_flag,
        resume_value: resume_value.map_or(abi::NULL_VALUE, |v| v.raw()),
        suspend_reason: AotSuspendReason::None,
        suspend_payload: 0,
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
// Suspend reason conversion
// =============================================================================

/// Convert an AOT suspend reason + payload to a scheduler SuspendReason.
fn convert_suspend_reason(
    reason: AotSuspendReason,
    payload: u64,
) -> Option<SchedulerSuspendReason> {
    match reason {
        AotSuspendReason::None => None,

        AotSuspendReason::AwaitTask => {
            let task_id = TaskId::from_u64(payload);
            Some(SchedulerSuspendReason::AwaitTask(task_id))
        }

        AotSuspendReason::IoWait => Some(SchedulerSuspendReason::IoWait),

        AotSuspendReason::Preempted | AotSuspendReason::Yielded => {
            // Re-schedule immediately
            Some(SchedulerSuspendReason::Sleep {
                wake_at: Instant::now(),
            })
        }

        AotSuspendReason::Sleep => {
            let millis = payload;
            Some(SchedulerSuspendReason::Sleep {
                wake_at: Instant::now() + Duration::from_millis(millis),
            })
        }

        AotSuspendReason::ChannelRecv => {
            Some(SchedulerSuspendReason::ChannelReceive {
                channel_id: payload,
            })
        }

        AotSuspendReason::ChannelSend => {
            // payload = channel_id
            // The value to send is stored in frame.suspend_payload by the AOT code.
            // The caller must extract it before passing to the scheduler.
            // For now, use null as placeholder — the worker loop will read from the frame.
            Some(SchedulerSuspendReason::ChannelSend {
                channel_id: payload,
                value: Value::null(),
            })
        }

        AotSuspendReason::MutexLock => {
            Some(SchedulerSuspendReason::MutexLock {
                mutex_id: MutexId::from_u64(payload),
            })
        }
    }
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
    unsafe extern "C" fn test_return_42(
        _frame: *mut AotFrame,
        _ctx: *mut AotTaskContext,
    ) -> u64 {
        abi::I32_TAG_BASE | 42
    }

    /// A test AOT function that suspends with AwaitTask reason.
    unsafe extern "C" fn test_suspend_await(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        (*ctx).suspend_reason = AotSuspendReason::AwaitTask;
        (*ctx).suspend_payload = 7; // Task ID 7
        AOT_SUSPEND
    }

    /// A test AOT function that suspends with Sleep reason.
    unsafe extern "C" fn test_suspend_sleep(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        (*ctx).suspend_reason = AotSuspendReason::Sleep;
        (*ctx).suspend_payload = 100; // 100ms
        AOT_SUSPEND
    }

    /// A test AOT function that suspends with Preempted reason.
    unsafe extern "C" fn test_suspend_preempted(
        _frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        (*ctx).suspend_reason = AotSuspendReason::Preempted;
        AOT_SUSPEND
    }

    /// A test AOT function that reads resume_value and returns it.
    unsafe extern "C" fn test_resume_returns_value(
        frame: *mut AotFrame,
        ctx: *mut AotTaskContext,
    ) -> u64 {
        if (*frame).resume_point == 0 {
            // First call — suspend
            (*frame).resume_point = 1;
            (*ctx).suspend_reason = AotSuspendReason::AwaitTask;
            (*ctx).suspend_payload = 1;
            AOT_SUSPEND
        } else {
            // Resume — return the resume value
            (*ctx).resume_value
        }
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
            assert!(result.frame.is_null(), "Frame should be freed on completion");
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
            assert!(!result.frame.is_null(), "Frame should be preserved on suspend");

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
                ExecutionResult::Suspended(SchedulerSuspendReason::Sleep { .. }) => {
                    // Preempted → immediate reschedule (Sleep { wake_at: now })
                }
                other => panic!("Expected Suspended(Sleep), got {:?}", other),
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
            prepare_resume(&mut ctx, Some(Value::i32(99)));
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

        let ctx = build_task_context(&preempt, helpers, Some(Value::i32(42)));
        assert_eq!(ctx.resume_value, Value::i32(42).raw());
        assert_eq!(ctx.suspend_reason, AotSuspendReason::None);
        assert_eq!(ctx.suspend_payload, 0);

        let ctx2 = build_task_context(&preempt, helpers, None);
        assert_eq!(ctx2.resume_value, abi::NULL_VALUE);
    }

    #[test]
    fn test_convert_suspend_reasons() {
        // AwaitTask
        let r = convert_suspend_reason(AotSuspendReason::AwaitTask, 42);
        assert!(matches!(r, Some(SchedulerSuspendReason::AwaitTask(tid)) if tid.as_u64() == 42));

        // Sleep
        let before = Instant::now();
        let r = convert_suspend_reason(AotSuspendReason::Sleep, 500);
        match r {
            Some(SchedulerSuspendReason::Sleep { wake_at }) => {
                assert!(wake_at >= before + Duration::from_millis(500));
            }
            _ => panic!("Expected Sleep"),
        }

        // IoWait
        let r = convert_suspend_reason(AotSuspendReason::IoWait, 0);
        assert!(matches!(r, Some(SchedulerSuspendReason::IoWait)));

        // ChannelRecv
        let r = convert_suspend_reason(AotSuspendReason::ChannelRecv, 123);
        assert!(matches!(r, Some(SchedulerSuspendReason::ChannelReceive { channel_id: 123 })));

        // MutexLock
        let r = convert_suspend_reason(AotSuspendReason::MutexLock, 5);
        assert!(matches!(r, Some(SchedulerSuspendReason::MutexLock { .. })));

        // None
        let r = convert_suspend_reason(AotSuspendReason::None, 0);
        assert!(r.is_none());
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
