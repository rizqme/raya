//! Integration tests for Garbage Collection (Milestone 1.7)
//!
//! Tests cover:
//! - Basic GC collection
//! - Nested object graphs

#![allow(clippy::approx_constant)]
//! - Circular references
//! - Array element tracking
//! - Multiple collection cycles
//! - Automatic threshold triggering

use raya_core::gc::GarbageCollector;
use raya_core::object::{Array, Object, RayaString};
use raya_core::value::Value;
use std::ptr::NonNull;

#[test]
fn test_gc_basic_collection() {
    let mut gc = GarbageCollector::default();

    // Allocate objects
    let obj1 = Object::new(0, 2);
    let obj2 = Object::new(0, 2);
    let obj3 = Object::new(0, 2);

    let ptr1 = gc.allocate(obj1);
    let ptr2 = gc.allocate(obj2);
    let _ptr3 = gc.allocate(obj3);

    // Only keep obj1 and obj2 as roots
    let val1 = unsafe { Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap()) };
    let val2 = unsafe { Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap()) };

    gc.add_root(val1);
    gc.add_root(val2);

    // Collect - obj3 should be freed
    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 2); // obj1 and obj2 survive
}

#[test]
fn test_gc_nested_objects() {
    let mut gc = GarbageCollector::default();

    // Create object graph: root -> obj1 -> obj2 -> obj3
    let mut obj1 = Object::new(0, 1);
    let mut obj2 = Object::new(0, 1);
    let obj3 = Object::new(0, 1);

    let ptr3 = gc.allocate(obj3);
    let val3 = unsafe { Value::from_ptr(NonNull::new(ptr3.as_ptr()).unwrap()) };

    obj2.set_field(0, val3).unwrap();
    let ptr2 = gc.allocate(obj2);
    let val2 = unsafe { Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap()) };

    obj1.set_field(0, val2).unwrap();
    let ptr1 = gc.allocate(obj1);
    let val1 = unsafe { Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap()) };

    // Only root is obj1
    gc.add_root(val1);

    // Collect - all 3 should be kept
    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 3); // All three objects survive
}

#[test]
fn test_gc_circular_references() {
    let mut gc = GarbageCollector::default();

    // Create circular reference: obj1 <-> obj2
    let obj1 = Object::new(0, 1);
    let obj2 = Object::new(0, 1);

    let ptr1 = gc.allocate(obj1);
    let ptr2 = gc.allocate(obj2);

    let val1 = unsafe { Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap()) };
    let val2 = unsafe { Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap()) };

    unsafe {
        (*ptr1.as_ptr()).set_field(0, val2).unwrap();
        (*ptr2.as_ptr()).set_field(0, val1).unwrap();
    }

    // No roots - both should be collected
    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 0); // Both collected
}

#[test]
fn test_gc_array_elements() {
    let mut gc = GarbageCollector::default();

    // Create array with object elements
    let obj1 = Object::new(0, 1);
    let obj2 = Object::new(0, 1);

    let ptr1 = gc.allocate(obj1);
    let ptr2 = gc.allocate(obj2);

    let val1 = unsafe { Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap()) };
    let val2 = unsafe { Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap()) };

    let mut arr = Array::new(0, 2);
    arr.set(0, val1).unwrap();
    arr.set(1, val2).unwrap();

    let arr_ptr = gc.allocate(arr);
    let arr_val = unsafe { Value::from_ptr(NonNull::new(arr_ptr.as_ptr()).unwrap()) };

    // Only keep array as root
    gc.add_root(arr_val);

    // Collect - array and both objects should survive
    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 3); // array + obj1 + obj2
}

#[test]
fn test_gc_multiple_collections() {
    let mut gc = GarbageCollector::default();

    for i in 0..5 {
        // Allocate objects
        let obj = Object::new(0, 1);
        let ptr = gc.allocate(obj);

        // Keep only the current one
        gc.clear_stack_roots();
        let val = unsafe { Value::from_ptr(NonNull::new(ptr.as_ptr()).unwrap()) };
        gc.add_root(val);

        // Collect
        gc.collect();

        // Should have exactly 1 live object
        let heap_stats = gc.heap_stats();
        assert_eq!(heap_stats.allocation_count, 1);

        let gc_stats = gc.stats();
        assert_eq!(gc_stats.collections, i + 1);
    }
}

#[test]
fn test_gc_threshold_trigger() {
    let mut gc = GarbageCollector::default();
    gc.set_threshold(256); // Small threshold

    // Allocate until GC triggers
    let initial_collections = gc.stats().collections;

    for _ in 0..100 {
        let obj = Object::new(0, 10); // Object with 10 fields
        let _ptr = gc.allocate(obj);
    }

    // GC should have been triggered automatically
    assert!(gc.stats().collections > initial_collections);
}

#[test]
fn test_gc_strings() {
    let mut gc = GarbageCollector::default();

    // Allocate strings
    let s1 = RayaString::new("hello".to_string());
    let s2 = RayaString::new("world".to_string());
    let s3 = RayaString::new("test".to_string());

    let ptr1 = gc.allocate(s1);
    let ptr2 = gc.allocate(s2);
    let _ptr3 = gc.allocate(s3);

    // Keep only s1 and s2
    let val1 = unsafe { Value::from_ptr(NonNull::new(ptr1.as_ptr()).unwrap()) };
    let val2 = unsafe { Value::from_ptr(NonNull::new(ptr2.as_ptr()).unwrap()) };

    gc.add_root(val1);
    gc.add_root(val2);

    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 2); // s1 and s2 survive
}

#[test]
fn test_gc_mixed_types() {
    let mut gc = GarbageCollector::default();

    // Mix of objects, arrays, and strings
    let obj = Object::new(0, 2);
    let arr = Array::new(0, 5);
    let string = RayaString::new("test".to_string());

    let obj_ptr = gc.allocate(obj);
    let arr_ptr = gc.allocate(arr);
    let str_ptr = gc.allocate(string);

    // Keep all as roots
    let obj_val = unsafe { Value::from_ptr(NonNull::new(obj_ptr.as_ptr()).unwrap()) };
    let arr_val = unsafe { Value::from_ptr(NonNull::new(arr_ptr.as_ptr()).unwrap()) };
    let str_val = unsafe { Value::from_ptr(NonNull::new(str_ptr.as_ptr()).unwrap()) };

    gc.add_root(obj_val);
    gc.add_root(arr_val);
    gc.add_root(str_val);

    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 3); // All three survive
}

#[test]
fn test_gc_empty_collection() {
    let mut gc = GarbageCollector::default();

    // Collect with no allocations
    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 0);
    assert_eq!(heap_stats.allocated_bytes, 0);
}

#[test]
fn test_gc_stats_tracking() {
    let mut gc = GarbageCollector::default();

    // Allocate and collect multiple times
    for _ in 0..3 {
        for _ in 0..5 {
            let obj = Object::new(0, 2);
            let _ptr = gc.allocate(obj);
        }
        gc.clear_stack_roots();
        gc.collect();
    }

    let stats = gc.stats();
    assert_eq!(stats.collections, 3);
    assert!(stats.objects_freed > 0);
    assert!(stats.bytes_freed > 0);
}

#[test]
fn test_gc_large_object_graph() {
    let mut gc = GarbageCollector::default();

    // Create a chain of 50 objects
    let mut current_val = Value::null();
    for _ in 0..50 {
        let mut obj = Object::new(0, 1);
        if !current_val.is_null() {
            obj.set_field(0, current_val).unwrap();
        }
        let ptr = gc.allocate(obj);
        current_val = unsafe { Value::from_ptr(NonNull::new(ptr.as_ptr()).unwrap()) };
    }

    // Keep root
    gc.add_root(current_val);

    // Collect - all 50 should survive
    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 50);
}

#[test]
fn test_gc_no_roots_clears_all() {
    let mut gc = GarbageCollector::default();

    // Allocate many objects without roots
    for _ in 0..20 {
        let obj = Object::new(0, 2);
        let _ptr = gc.allocate(obj);
    }

    // Collect without any roots - everything should be freed
    gc.collect();

    let heap_stats = gc.heap_stats();
    assert_eq!(heap_stats.allocation_count, 0);
}

#[test]
fn test_gc_preserve_primitives_in_objects() {
    let mut gc = GarbageCollector::default();

    // Create object with primitive fields
    let mut obj = Object::new(0, 3);
    obj.set_field(0, Value::i32(42)).unwrap();
    obj.set_field(1, Value::f64(3.14)).unwrap();
    obj.set_field(2, Value::bool(true)).unwrap();

    let ptr = gc.allocate(obj);
    let val = unsafe { Value::from_ptr(NonNull::new(ptr.as_ptr()).unwrap()) };

    gc.add_root(val);
    gc.collect();

    // Verify object survived and fields are intact
    let obj_ref = unsafe { &*ptr.as_ptr() };
    assert_eq!(obj_ref.get_field(0), Some(Value::i32(42)));
    assert_eq!(obj_ref.get_field(1), Some(Value::f64(3.14)));
    assert_eq!(obj_ref.get_field(2), Some(Value::bool(true)));
}

#[test]
fn test_gc_threshold_adjustment() {
    let mut gc = GarbageCollector::default();

    // Set various thresholds
    gc.set_threshold(100);
    assert_eq!(gc.heap_stats().threshold, 100);

    gc.set_threshold(1000);
    assert_eq!(gc.heap_stats().threshold, 1000);

    gc.set_threshold(10000);
    assert_eq!(gc.heap_stats().threshold, 10000);
}
