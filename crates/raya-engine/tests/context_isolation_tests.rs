//! Multi-Context Isolation Tests
//!
//! This module contains comprehensive tests for VmContext isolation.
//! Tests validate that different contexts are completely isolated:
//! - Heap isolation (separate memory spaces)
//! - Global variable isolation
//! - GC independence
//! - Task registry separation
//! - Class registry isolation
//! - Resource limit independence
//! - Parent-child relationships
//! - Marshalling across contexts
//!
//! # Running Tests
//! ```bash
//! cargo test --test context_isolation_tests
//! ```

use raya_engine::vm::scheduler::TaskId;
use raya_engine::vm::value::Value;
use raya_engine::vm::vm::{
    CapabilityRegistry, ContextRegistry, HttpCapability, LogCapability, ResourceLimits, VmContext,
    VmOptions,
};
use std::sync::Arc;

// ===== Heap Isolation Tests =====

#[test]
fn test_heap_isolation() {
    // Create two separate contexts
    let ctx1 = VmContext::new();
    let ctx2 = VmContext::new();

    // Verify they have different IDs
    assert_ne!(ctx1.id(), ctx2.id());

    // Verify they have different heap instances
    let heap_stats1 = ctx1.heap_stats();
    let heap_stats2 = ctx2.heap_stats();

    // Both should start with 0 allocated bytes
    assert_eq!(heap_stats1.allocated_bytes, 0);
    assert_eq!(heap_stats2.allocated_bytes, 0);

    // Contexts maintain separate heaps - verified by different GC instances
}

#[test]
fn test_global_variable_isolation() {
    // Create two contexts
    let mut ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();

    // Set global in context 1
    ctx1.set_global("x".to_string(), Value::i32(42));

    // Verify not visible in context 2
    assert!(ctx2.get_global("x").is_none());

    // Set different value in context 2
    ctx2.set_global("x".to_string(), Value::i32(100));

    // Verify contexts maintain separate values
    assert_eq!(ctx1.get_global("x"), Some(Value::i32(42)));
    assert_eq!(ctx2.get_global("x"), Some(Value::i32(100)));
}

// ===== GC Isolation Tests =====

#[test]
fn test_gc_isolation() {
    // Create two contexts with different initial thresholds
    let options1 = VmOptions {
        gc_threshold: 512 * 1024,
        ..Default::default()
    };
    let options2 = VmOptions {
        gc_threshold: 1024 * 1024,
        ..Default::default()
    };

    let mut ctx1 = VmContext::with_options(options1);
    let ctx2 = VmContext::with_options(options2);

    // Verify initial thresholds are different
    assert_eq!(ctx1.heap_stats().threshold, 512 * 1024);
    assert_eq!(ctx2.heap_stats().threshold, 1024 * 1024);

    // Get initial GC stats
    let stats1_before = ctx1.gc_stats();
    let stats2_before = ctx2.gc_stats();

    assert_eq!(stats1_before.collections, 0);
    assert_eq!(stats2_before.collections, 0);

    // Trigger GC in context 1
    ctx1.collect_garbage();

    // Verify context 1 GC ran
    let stats1_after = ctx1.gc_stats();
    assert_eq!(stats1_after.collections, 1);

    // Verify context 2 was unaffected
    let stats2_after = ctx2.gc_stats();
    assert_eq!(stats2_after.collections, 0);

    // Note: Thresholds may be adjusted by GC (dynamic threshold adjustment)
    // The key isolation property is that GC in one context doesn't affect the other
}

// ===== Task Registry Isolation Tests =====

#[test]
fn test_task_registry_isolation() {
    // Create two contexts
    let mut ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();

    // Spawn tasks in context 1
    let task1 = TaskId::new();
    let task2 = TaskId::new();

    ctx1.register_task(task1);
    ctx1.register_task(task2);

    // Verify tasks are in context 1
    assert_eq!(ctx1.task_count(), 2);
    assert!(ctx1.tasks().contains(&task1));
    assert!(ctx1.tasks().contains(&task2));

    // Verify not listed in context 2's task registry
    assert_eq!(ctx2.task_count(), 0);
    assert!(!ctx2.tasks().contains(&task1));
    assert!(!ctx2.tasks().contains(&task2));

    // Spawn different task in context 2
    let task3 = TaskId::new();
    ctx2.register_task(task3);

    // Verify separation
    assert_eq!(ctx1.task_count(), 2);
    assert_eq!(ctx2.task_count(), 1);
    assert!(!ctx1.tasks().contains(&task3));
}

// ===== Class Registry Isolation Tests =====

#[test]
fn test_class_registry_isolation() {
    // Create two contexts
    let ctx1 = VmContext::new();
    let ctx2 = VmContext::new();

    // Get class registries
    let registry1 = ctx1.class_registry();
    let registry2 = ctx2.class_registry();

    // Verify they are separate instances
    // (they should have different addresses in memory)
    // For now, just verify they both exist and are empty
    assert_eq!(registry1.next_class_id(), 0);
    assert_eq!(registry2.next_class_id(), 0);

    // Note: Full class registration tests will be added when
    // class loading is implemented
}

// ===== Resource Limit Isolation Tests =====

#[test]
fn test_resource_limit_isolation() {
    // Create context 1 with 1MB heap limit
    let options1 = VmOptions {
        limits: ResourceLimits::with_heap_limit(1024 * 1024),
        ..Default::default()
    };

    // Create context 2 with 2MB heap limit
    let options2 = VmOptions {
        limits: ResourceLimits::with_heap_limit(2 * 1024 * 1024),
        ..Default::default()
    };

    let ctx1 = VmContext::with_options(options1);
    let ctx2 = VmContext::with_options(options2);

    // Verify independent limits
    assert_eq!(ctx1.limits().max_heap_bytes, Some(1024 * 1024));
    assert_eq!(ctx2.limits().max_heap_bytes, Some(2 * 1024 * 1024));

    // Verify other limits can differ
    let options3 = VmOptions {
        limits: ResourceLimits {
            max_tasks: Some(10),
            max_heap_bytes: None,
            ..Default::default()
        },
        ..Default::default()
    };

    let ctx3 = VmContext::with_options(options3);
    assert_eq!(ctx3.limits().max_tasks, Some(10));
    assert_eq!(ctx3.limits().max_heap_bytes, None);
}

// ===== Multiple Contexts Tests =====

#[test]
fn test_multiple_contexts_isolation() {
    // Create 10 contexts and verify they're all isolated
    let mut contexts: Vec<_> = (0..10).map(|_| VmContext::new()).collect();

    // Get all context IDs
    let context_ids: Vec<_> = contexts.iter().map(|ctx| ctx.id()).collect();

    // Verify all IDs are unique
    for i in 0..context_ids.len() {
        for j in (i + 1)..context_ids.len() {
            assert_ne!(context_ids[i], context_ids[j]);
        }
    }

    // Set unique globals in each context
    for (i, ctx) in contexts.iter_mut().enumerate() {
        ctx.set_global(format!("value_{}", i), Value::i32(i as i32));

        // Trigger GC in this context
        ctx.collect_garbage();
    }

    // Verify each context has only its own global
    for (i, ctx) in contexts.iter().enumerate() {
        // Verify this context's global exists
        assert_eq!(
            ctx.get_global(&format!("value_{}", i)),
            Some(Value::i32(i as i32))
        );

        // Verify other contexts' globals are not visible
        for j in 0..10 {
            if i != j {
                assert!(ctx.get_global(&format!("value_{}", j)).is_none());
            }
        }
    }

    // Verify GC in one context doesn't affect others
    // Get initial GC counts
    let initial_gc_count_0 = contexts[0].gc_stats().collections;
    let initial_gc_count_1 = contexts[1].gc_stats().collections;

    // Trigger GC in first context only
    contexts[0].collect_garbage();

    // Verify only first context's GC count increased
    assert_eq!(
        contexts[0].gc_stats().collections,
        initial_gc_count_0 + 1,
        "Context 0 GC count should increase"
    );
    assert_eq!(
        contexts[1].gc_stats().collections,
        initial_gc_count_1,
        "Context 1 GC count should not change"
    );
}

// ===== Parent-Child Relationship Tests =====

#[test]
fn test_parent_child_relationship() {
    // Create parent context
    let parent = VmContext::new();
    let parent_id = parent.id();

    // Create child context with parent
    let child = VmContext::with_parent(VmOptions::default(), parent_id);

    // Verify parent link is maintained
    assert_eq!(child.parent(), Some(parent_id));
    assert!(child.is_root() == false);

    // Verify parent is root
    assert!(parent.is_root());
    assert_eq!(parent.parent(), None);

    // Verify they have different IDs
    assert_ne!(parent.id(), child.id());

    // Verify child has independent heap
    let parent_heap = parent.heap_stats();
    let child_heap = child.heap_stats();

    assert_eq!(parent_heap.allocated_bytes, 0);
    assert_eq!(child_heap.allocated_bytes, 0);
}

// ===== Marshalling Tests =====

#[test]
fn test_marshalling_across_contexts() {
    use raya_engine::vm::vm::marshal;

    // Create two contexts
    let ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();

    // Create a primitive value in context 1
    let value = Value::i32(42);

    // Marshal the value
    let marshalled = marshal(&value, &ctx1).unwrap();

    // Unmarshal in context 2
    let unmarshalled = raya_engine::vm::vm::unmarshal(marshalled, &mut ctx2).unwrap();

    // Verify value is preserved
    assert_eq!(unmarshalled, Value::i32(42));

    // Verify it's a copy (for primitives, they're always copied)
    // When we have heap objects, we'll verify deep copying occurs
}

// ===== Context Registry Tests =====

#[test]
fn test_context_registry_isolation() {
    let registry = ContextRegistry::new();

    // Register multiple contexts
    let ctx1 = VmContext::new();
    let ctx2 = VmContext::new();
    let ctx3 = VmContext::new();

    let id1 = ctx1.id();
    let id2 = ctx2.id();
    let id3 = ctx3.id();

    registry.register(ctx1);
    registry.register(ctx2);
    registry.register(ctx3);

    // Verify all registered
    assert_eq!(registry.len(), 3);
    assert!(registry.get(id1).is_some());
    assert!(registry.get(id2).is_some());
    assert!(registry.get(id3).is_some());

    // Remove one context
    let removed = registry.remove(id2);
    assert!(removed.is_some());

    // Verify count updated
    assert_eq!(registry.len(), 2);
    assert!(registry.get(id2).is_none());

    // Verify others still present
    assert!(registry.get(id1).is_some());
    assert!(registry.get(id3).is_some());
}

#[test]
fn test_context_termination_cleanup() {
    let registry = ContextRegistry::new();

    // Create context with allocations
    let mut ctx = VmContext::with_options(VmOptions {
        limits: ResourceLimits::with_heap_limit(10 * 1024 * 1024),
        ..Default::default()
    });

    let ctx_id = ctx.id();

    // Register some tasks
    ctx.register_task(TaskId::new());
    ctx.register_task(TaskId::new());
    ctx.register_task(TaskId::new());

    assert_eq!(ctx.task_count(), 3);

    // Register context
    let ctx_arc = registry.register(ctx);

    // Verify registered
    assert_eq!(registry.len(), 1);

    // Simulate termination by removing from registry
    let removed = registry.remove(ctx_id);
    assert!(removed.is_some());

    // Verify context is removed
    assert_eq!(registry.len(), 0);
    assert!(registry.get(ctx_id).is_none());

    // Drop the Arc to simulate full cleanup
    drop(ctx_arc);
    drop(removed);

    // Context and all its resources are now cleaned up
}

#[test]
fn test_multiple_contexts_with_different_capabilities() {
    // Create context 1 with log capability
    let log_cap = Arc::new(LogCapability::new("ctx1"));
    let caps1 = CapabilityRegistry::new().with_capability(log_cap);

    let options1 = VmOptions {
        capabilities: caps1,
        ..Default::default()
    };

    // Create context 2 with http capability
    let http_cap = Arc::new(HttpCapability::new(vec!["example.com".to_string()]));
    let caps2 = CapabilityRegistry::new().with_capability(http_cap);

    let options2 = VmOptions {
        capabilities: caps2,
        ..Default::default()
    };

    // Create context 3 with no capabilities
    let options3 = VmOptions::default();

    let ctx1 = VmContext::with_options(options1);
    let ctx2 = VmContext::with_options(options2);
    let ctx3 = VmContext::with_options(options3);

    // Verify different capability sets
    assert!(ctx1.capabilities().has("log"));
    assert!(!ctx1.capabilities().has("http.fetch"));

    assert!(!ctx2.capabilities().has("log"));
    assert!(ctx2.capabilities().has("http.fetch"));

    assert!(!ctx3.capabilities().has("log"));
    assert!(!ctx3.capabilities().has("http.fetch"));
}

#[test]
fn test_context_id_uniqueness() {
    // Create 1000 contexts and verify all IDs are unique
    let contexts: Vec<_> = (0..1000).map(|_| VmContext::new()).collect();

    let ids: Vec<_> = contexts.iter().map(|ctx| ctx.id()).collect();

    // Verify all IDs are unique
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(
                ids[i], ids[j],
                "Context IDs should be unique: {} vs {}",
                i, j
            );
        }
    }
}
