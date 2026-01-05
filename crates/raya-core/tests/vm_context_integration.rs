//! Integration tests for VM Context (Milestone 1.3)

use raya_core::gc::GarbageCollector;
use raya_core::object::Object;
use raya_core::value::Value;
use raya_core::vm::{ResourceLimits, VmContext, VmContextId};
use std::ptr::NonNull;

#[test]
fn test_context_id_uniqueness() {
    let id1 = VmContextId::new();
    let id2 = VmContextId::new();
    let id3 = VmContextId::new();

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);
}

#[test]
fn test_context_creation() {
    let ctx = VmContext::new();
    assert!(ctx.id().as_u64() > 0);
}

#[test]
fn test_multi_context_gc_isolation() {
    // Create two separate GCs (simulating two contexts)
    let mut gc1 = GarbageCollector::default();
    let mut gc2 = GarbageCollector::default();

    // Allocate in GC 1
    let obj1 = Object::new(0, 2);
    let ptr1 = gc1.allocate(obj1);
    let val1 = unsafe { Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap()) };
    gc1.add_root(val1);

    // Allocate in GC 2
    let obj2 = Object::new(0, 2);
    let ptr2 = gc2.allocate(obj2);
    let val2 = unsafe { Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap()) };
    gc2.add_root(val2);

    // Collect in GC 1
    gc1.collect();
    assert_eq!(gc1.heap_stats().allocation_count, 1);

    // Collect in GC 2
    gc2.collect();
    assert_eq!(gc2.heap_stats().allocation_count, 1);
}

#[test]
fn test_context_global_variables() {
    let mut ctx = VmContext::new();

    // Set globals
    ctx.set_global("x".to_string(), Value::i32(100));
    ctx.set_global("y".to_string(), Value::i32(200));

    // Retrieve globals
    assert_eq!(ctx.get_global("x"), Some(Value::i32(100)));
    assert_eq!(ctx.get_global("y"), Some(Value::i32(200)));
    assert_eq!(ctx.get_global("z"), None);
}

#[test]
fn test_multiple_contexts_independent_globals() {
    let mut ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();

    // Set globals in context 1
    ctx1.set_global("var".to_string(), Value::i32(100));

    // Set globals in context 2
    ctx2.set_global("var".to_string(), Value::i32(999));

    // Verify isolation
    assert_eq!(ctx1.get_global("var"), Some(Value::i32(100)));
    assert_eq!(ctx2.get_global("var"), Some(Value::i32(999)));
}

#[test]
fn test_resource_limits_heap() {
    let limits = ResourceLimits::with_heap_limit(1024);
    assert_eq!(limits.max_heap_bytes, Some(1024));
}

#[test]
fn test_resource_limits_tasks() {
    let limits = ResourceLimits::with_task_limit(10);
    assert_eq!(limits.max_tasks, Some(10));
}

#[test]
fn test_resource_limits_steps() {
    let limits = ResourceLimits::with_step_budget(1000000);
    assert_eq!(limits.max_step_budget, Some(1000000));
}

#[test]
fn test_gc_per_context() {
    let mut ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();

    // Each context has its own GC
    let gc1_ptr = ctx1.gc_mut() as *mut _;
    let gc2_ptr = ctx2.gc_mut() as *mut _;

    assert_ne!(gc1_ptr, gc2_ptr);
}

#[test]
fn test_context_gc_collect() {
    let mut ctx = VmContext::new();

    // Allocate objects
    let obj1 = Object::new(0, 2);
    let obj2 = Object::new(0, 2);

    let ptr1 = ctx.gc_mut().allocate(obj1);
    let _ptr2 = ctx.gc_mut().allocate(obj2);

    // Add only obj1 as root
    let val1 = unsafe { Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap()) };
    ctx.gc_mut().add_root(val1);

    // Collect - obj2 should be freed
    ctx.collect_garbage();

    let stats = ctx.heap_stats();
    assert_eq!(stats.allocation_count, 1);
}

#[test]
fn test_stress_multiple_contexts() {
    let mut contexts = Vec::new();

    // Create 10 contexts
    for i in 0..10 {
        let mut ctx = VmContext::new();
        ctx.set_global("id".to_string(), Value::i32(i as i32));
        contexts.push(ctx);
    }

    // Verify each context is isolated
    for (i, ctx) in contexts.iter().enumerate() {
        assert_eq!(ctx.get_global("id"), Some(Value::i32(i as i32)));
    }
}

#[test]
fn test_context_limits_access() {
    let ctx = VmContext::new();
    let limits = ctx.limits();

    // Default limits should be unlimited
    assert_eq!(limits.max_heap_bytes, None);
    assert_eq!(limits.max_tasks, None);
    assert_eq!(limits.max_step_budget, None);
}

#[test]
fn test_context_counters_access() {
    let ctx = VmContext::new();
    let counters = ctx.counters();

    // Initial counters should be zero
    assert_eq!(counters.active_tasks(), 0);
    assert_eq!(counters.total_steps(), 0);
}

#[test]
fn test_context_can_create_task() {
    let ctx = VmContext::new();

    // With unlimited tasks, should always return true
    assert!(ctx.can_create_task());
}

#[test]
fn test_context_step_budget_check() {
    let ctx = VmContext::new();

    // With unlimited budget, should never be exhausted
    assert!(!ctx.is_step_budget_exhausted());
}
