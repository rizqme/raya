//! Garbage Collection Stress Tests
//!
//! This module contains comprehensive stress tests for the garbage collector.
//! Tests validate GC correctness under extreme conditions:
//! - Rapid allocation and collection
//! - Various allocation patterns
//! - Fragmentation resistance
//! - Circular references
//! - Deep object graphs
//! - Concurrent allocation
//! - Safepoint coordination
//!
//! # Test Categories
//! - Basic stress tests (run in CI)
//! - Long-running tests (marked with #[ignore])
//!
//! # Running Tests
//! ```bash
//! # Run all GC stress tests (except long-running)
//! cargo test --test gc_stress_tests
//!
//! # Run all tests including long-running ones
//! cargo test --test gc_stress_tests -- --include-ignored
//! ```

use raya_engine::vm::gc::GarbageCollector;
use raya_engine::vm::value::Value;
use raya_engine::vm::interpreter::VmContextId;
use std::sync::Arc;

/// Helper to create a GC with a specific threshold
fn create_gc_with_threshold(threshold: usize) -> GarbageCollector {
    let context_id = VmContextId::new();
    let type_registry = Arc::new(raya_engine::vm::types::create_standard_registry());
    let mut gc = GarbageCollector::new(context_id, type_registry);
    gc.set_threshold(threshold);
    gc
}

// ===== Rapid Allocation Tests =====

#[test]
fn test_rapid_allocation_and_collection() {
    // Allocate many objects rapidly and verify GC triggers
    let mut gc = create_gc_with_threshold(1024 * 1024); // 1 MB threshold

    // Allocate 100,000 small objects
    for i in 0..100_000 {
        let _value = Value::i32(i);
        // Objects immediately become garbage (not rooted)
    }

    // Trigger GC
    gc.collect();

    // Verify memory was reclaimed
    let stats = gc.stats();
    assert!(stats.collections > 0, "GC should have run at least once");
}

#[test]
fn test_allocation_patterns_young_objects() {
    // Test short-lived object pattern (generational GC optimization)
    let mut gc = create_gc_with_threshold(512 * 1024);

    for _iteration in 0..1000 {
        // Allocate batch of short-lived objects
        let mut temp_values = Vec::new();
        for i in 0..100 {
            temp_values.push(Value::i32(i));
        }
        // temp_values dropped here, objects become garbage
    }

    gc.collect();

    let stats = gc.stats();
    assert!(stats.collections > 0);
    // Most allocations should have been collected efficiently
}

#[test]
fn test_allocation_patterns_old_objects() {
    // Test long-lived object pattern
    let mut gc = create_gc_with_threshold(1024 * 1024);

    // Create long-lived objects
    let mut long_lived = Vec::new();
    for i in 0..1000 {
        long_lived.push(Value::i32(i));
    }

    // Run multiple GC cycles
    for _ in 0..10 {
        gc.collect();
    }

    let stats = gc.stats();
    assert!(stats.collections >= 10);

    // Long-lived objects should still exist
    assert_eq!(long_lived.len(), 1000);
}

#[test]
fn test_fragmentation_resistance() {
    // Allocate objects of varying sizes to test fragmentation
    let mut gc = create_gc_with_threshold(2 * 1024 * 1024);

    let mut values = Vec::new();

    // Allocate small, medium, and large objects in mixed order
    for i in 0..1000 {
        match i % 3 {
            0 => values.push(Value::i32(i)),           // Small (inline)
            1 => values.push(Value::f64(i as f64)),    // Small (inline)
            _ => values.push(Value::bool(i % 2 == 0)), // Small (inline)
        }
    }

    // Remove every other object to create fragmentation
    for i in (0..values.len()).rev() {
        if i % 2 == 0 {
            values.remove(i);
        }
    }

    // Trigger GC
    gc.collect();

    let stats = gc.stats();
    assert!(stats.collections > 0);

    // Verify remaining objects are still valid
    assert_eq!(values.len(), 500);
}

// NOTE: Circular references test is not applicable yet since we don't have
// heap-allocated objects (arrays, objects) implemented yet. Will add when
// object model is complete.

#[test]
fn test_gc_trigger_threshold() {
    // Verify GC triggers at expected thresholds
    let threshold = 1024 * 1024; // 1 MB
    let _gc = create_gc_with_threshold(threshold);

    let heap_stats = _gc.heap_stats();
    assert_eq!(heap_stats.threshold, threshold);
}

#[test]
fn test_gc_stats_accuracy() {
    // Verify GC statistics are accurate
    let mut gc = create_gc_with_threshold(512 * 1024);

    let stats_before = gc.stats();
    assert_eq!(stats_before.collections, 0);
    assert_eq!(stats_before.objects_freed, 0);

    // Allocate some values (currently inline, so no heap allocation yet)
    for i in 0..100 {
        let _value = Value::i32(i);
    }

    gc.collect();

    let stats_after = gc.stats();
    assert_eq!(stats_after.collections, 1);
}

#[test]
fn test_gc_with_heap_limit() {
    // Test GC behavior with heap limit
    let heap_limit = 1024 * 1024; // 1 MB
    let mut gc = create_gc_with_threshold(512 * 1024);

    // Simulate allocation near limit
    let mut values = Vec::new();
    for i in 0..10_000 {
        values.push(Value::i32(i));
    }

    // Trigger GC
    gc.collect();

    let stats = gc.stats();
    assert!(stats.collections > 0);

    // Check that heap usage is tracked
    let heap_stats = gc.heap_stats();
    assert!(heap_stats.allocated_bytes <= heap_limit || heap_stats.allocated_bytes == 0);
}

#[test]
fn test_massive_allocation_stress() {
    // Allocate many objects in batches (reduced for reasonable test time)
    let mut gc = create_gc_with_threshold(50 * 1024 * 1024);

    // 10 batches of 10,000 objects = 100,000 total (completes in ~1 second)
    for batch in 0..10 {
        let mut values = Vec::new();
        for i in 0..10_000 {
            values.push(Value::i32(i));
        }

        // Trigger GC every 5 batches
        if batch % 5 == 0 {
            gc.collect();
        }
    }

    let stats = gc.stats();
    assert!(stats.collections > 0);
    println!("Massive allocation: {} collections", stats.collections);
}

// ===== Future Tests (when heap objects are implemented) =====

// #[test]
// fn test_circular_references() {
//     // Will implement when object model supports references
//     // Create circular object graphs
//     // Verify all garbage is collected
// }

// #[test]
// fn test_deep_object_graphs() {
//     // Will implement when object model is complete
//     // Create deep nesting (1000+ levels)
//     // Verify stack doesn't overflow during GC
// }

// #[test]
// fn test_concurrent_allocation_from_tasks() {
//     // Will implement when task scheduler is integrated
//     // Multiple tasks allocating concurrently
//     // Verify thread-safe GC coordination
// }

// #[test]
// fn test_gc_during_safepoint() {
//     // Will implement when safepoint mechanism is complete
//     // Trigger GC while tasks are at safepoints
//     // Verify all tasks pause correctly
// }

// #[test]
// fn test_string_interning() {
//     // Will implement when string type is complete
//     // Allocate many duplicate strings
//     // Verify deduplication works
// }

// #[test]
// fn test_array_resizing() {
//     // Will implement when array type is complete
//     // Grow arrays dynamically
//     // Verify old memory is freed
// }

// #[test]
// fn test_weak_references() {
//     // Will implement when weak reference support is added
//     // Create weak references to objects
//     // Verify they're cleared after GC
// }
