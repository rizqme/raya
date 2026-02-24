#![cfg(feature = "aot")]

use raya_engine::aot::executor::{allocate_initial_frame, build_task_context, prepare_resume};
use raya_engine::aot::helpers::create_default_helper_table;
use raya_engine::aot::{
    run_aot_function, AotFrame, AotTaskContext, SuspendReason as AotSuspendReason, AOT_SUSPEND,
};
use raya_engine::vm::interpreter::ExecutionResult;
use raya_engine::vm::scheduler::SuspendReason as SchedulerSuspendReason;
use raya_engine::vm::value::Value;
use std::sync::atomic::AtomicBool;

const NAN_BOX_BASE: u64 = 0xFFF8_0000_0000_0000;
const TAG_SHIFT: u64 = 48;
const TAG_I32: u64 = 0x1 << TAG_SHIFT;
const TAG_MASK: u64 = 0x7 << TAG_SHIFT;
const I32_TAG_BASE: u64 = NAN_BOX_BASE | TAG_I32;
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

fn is_i32(val: u64) -> bool {
    (val & (NAN_BOX_BASE | TAG_MASK)) == I32_TAG_BASE
}

fn decode_i32(val: u64) -> i32 {
    assert!(is_i32(val), "Expected NaN-boxed i32, got 0x{:016X}", val);
    (val & PAYLOAD_MASK) as i32
}

unsafe extern "C" fn aot_returns_i32(_frame: *mut AotFrame, _ctx: *mut AotTaskContext) -> u64 {
    Value::i32(123).raw()
}

unsafe extern "C" fn stub_native_call_sum(
    _ctx: *mut AotTaskContext,
    _native_id: u16,
    args_ptr: *const u64,
    argc: u8,
) -> u64 {
    assert_eq!(argc, 2);
    let a = unsafe { *args_ptr.add(0) };
    let b = unsafe { *args_ptr.add(1) };
    Value::i32(decode_i32(a) + decode_i32(b)).raw()
}

unsafe extern "C" fn aot_native_arg_fast_path(
    frame: *mut AotFrame,
    ctx: *mut AotTaskContext,
) -> u64 {
    let a = unsafe { *(*frame).locals.add(0) };
    let b = unsafe { *(*frame).locals.add(1) };
    let args = [a, b];
    unsafe { ((*ctx).helpers.native_call)(ctx, 0, args.as_ptr(), args.len() as u8) }
}

unsafe extern "C" fn stub_native_call_suspend(
    _ctx: *mut AotTaskContext,
    _native_id: u16,
    _args_ptr: *const u64,
    _argc: u8,
) -> u64 {
    AOT_SUSPEND
}

unsafe extern "C" fn aot_native_suspend_boundary(
    _frame: *mut AotFrame,
    ctx: *mut AotTaskContext,
) -> u64 {
    unsafe {
        (*ctx).suspend_reason = AotSuspendReason::NativeCallBoundary;
        (*ctx).suspend_payload = 0;
        ((*ctx).helpers.native_call)(ctx, 0, std::ptr::null(), 0)
    }
}

unsafe extern "C" fn aot_suspend_then_resume(
    frame: *mut AotFrame,
    ctx: *mut AotTaskContext,
) -> u64 {
    unsafe {
        if (*frame).resume_point == 0 {
            (*frame).resume_point = 1;
            (*ctx).suspend_reason = AotSuspendReason::AwaitTask;
            (*ctx).suspend_payload = 1;
            AOT_SUSPEND
        } else {
            (*ctx).resume_value
        }
    }
}

#[test]
fn aot_e2e_completes_with_value() {
    let helpers = create_default_helper_table();
    let preempt = AtomicBool::new(false);
    let mut ctx = build_task_context(&preempt, helpers, None);

    unsafe {
        let frame = allocate_initial_frame(0, 0, aot_returns_i32, &[], &helpers);
        let result = run_aot_function(frame, &mut ctx, 100);
        match result.result {
            ExecutionResult::Completed(v) => assert_eq!(v.as_i32(), Some(123)),
            other => panic!("expected completion, got {:?}", other),
        }
    }
}

#[test]
fn aot_e2e_native_arg_fast_path_completes() {
    let mut helpers = create_default_helper_table();
    helpers.native_call = stub_native_call_sum;
    let preempt = AtomicBool::new(false);
    let mut ctx = build_task_context(&preempt, helpers, None);
    let args = [Value::i32(7), Value::i32(11)];

    unsafe {
        let frame = allocate_initial_frame(0, 2, aot_native_arg_fast_path, &args, &helpers);
        let result = run_aot_function(frame, &mut ctx, 100);
        match result.result {
            ExecutionResult::Completed(v) => assert_eq!(v.as_i32(), Some(18)),
            other => panic!("expected completion, got {:?}", other),
        }
    }
}

#[test]
fn aot_e2e_native_boundary_suspend_handoffs() {
    let mut helpers = create_default_helper_table();
    helpers.native_call = stub_native_call_suspend;
    let preempt = AtomicBool::new(false);
    let mut ctx = build_task_context(&preempt, helpers, None);

    unsafe {
        let frame = allocate_initial_frame(0, 0, aot_native_suspend_boundary, &[], &helpers);
        let result = run_aot_function(frame, &mut ctx, 100);
        match result.result {
            ExecutionResult::Suspended(SchedulerSuspendReason::Sleep { .. }) => {}
            other => panic!("expected native-boundary suspend handoff, got {:?}", other),
        }
        // Cleanup: run_aot_function preserves frame on suspension.
        (helpers.free_frame)(result.frame);
    }
}

#[test]
fn aot_e2e_suspend_resume_roundtrip() {
    let helpers = create_default_helper_table();
    let preempt = AtomicBool::new(false);
    let mut ctx = build_task_context(&preempt, helpers, None);

    unsafe {
        let frame = allocate_initial_frame(0, 0, aot_suspend_then_resume, &[], &helpers);
        let first = run_aot_function(frame, &mut ctx, 100);
        match first.result {
            ExecutionResult::Suspended(SchedulerSuspendReason::AwaitTask(_)) => {}
            other => panic!("expected await suspend, got {:?}", other),
        }

        prepare_resume(&mut ctx, Some(Value::i32(55)));
        let second = run_aot_function(first.frame, &mut ctx, 100);
        match second.result {
            ExecutionResult::Completed(v) => assert_eq!(v.as_i32(), Some(55)),
            other => panic!("expected completion after resume, got {:?}", other),
        }
    }
}
