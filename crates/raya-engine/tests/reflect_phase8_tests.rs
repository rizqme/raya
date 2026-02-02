//! Integration tests for Phase 8 Reflect API
//!
//! Tests for memory analysis, stack introspection, and debug info handlers.

use raya_engine::vm::gc::GarbageCollector;
use raya_engine::vm::object::{Array, Object, RayaString};
use raya_engine::vm::reflect::{ObjectDiff, ObjectSnapshot, SnapshotValue};

// ============================================================================
// Memory Analysis Tests (getHeapStats, findInstances)
// ============================================================================

mod memory_analysis {
    use super::*;

    #[test]
    fn test_heap_stats_empty() {
        let gc = GarbageCollector::default();
        let stats = gc.heap_stats();

        assert_eq!(stats.allocated_bytes, 0);
        assert_eq!(stats.allocation_count, 0);
    }

    #[test]
    fn test_heap_stats_with_allocations() {
        let mut gc = GarbageCollector::default();

        // Allocate some objects
        let _obj1 = gc.allocate(Object::new(0, 3));
        let _obj2 = gc.allocate(Object::new(0, 5));
        let _arr = gc.allocate(Array::new(10, 0));
        let _str = gc.allocate(RayaString::new("hello".to_string()));

        let stats = gc.heap_stats();
        assert_eq!(stats.allocation_count, 4);
        assert!(stats.allocated_bytes > 0);
    }

    #[test]
    fn test_heap_stats_after_collection() {
        let mut gc = GarbageCollector::default();

        // Allocate objects
        let _obj1 = gc.allocate(Object::new(0, 3));
        let _obj2 = gc.allocate(Object::new(1, 2));

        let before_stats = gc.heap_stats();
        assert_eq!(before_stats.allocation_count, 2);

        // Collect (without roots, objects may be collected)
        gc.collect();

        let after_stats = gc.stats();
        assert_eq!(after_stats.collections, 1);
    }

    #[test]
    fn test_heap_iteration() {
        let mut gc = GarbageCollector::default();

        // Allocate several objects
        let _obj1 = gc.allocate(Object::new(0, 3));
        let _obj2 = gc.allocate(Object::new(1, 5));
        let _obj3 = gc.allocate(Object::new(0, 2));

        // Verify we can iterate over allocations
        let count = gc.heap().iter_allocations().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_find_instances_by_class() {
        let mut gc = GarbageCollector::default();
        let class_id_a = 42;
        let class_id_b = 99;

        // Allocate objects of different classes
        let _obj_a1 = gc.allocate(Object::new(class_id_a, 2));
        let _obj_a2 = gc.allocate(Object::new(class_id_a, 2));
        let _obj_b1 = gc.allocate(Object::new(class_id_b, 3));
        let _obj_a3 = gc.allocate(Object::new(class_id_a, 2));

        // Count instances of class A
        let mut count_a = 0;
        for header_ptr in gc.heap().iter_allocations() {
            let header = unsafe { &*header_ptr };
            if header.type_id() == std::any::TypeId::of::<Object>() {
                let obj_ptr = unsafe { header_ptr.add(1) as *const Object };
                let obj = unsafe { &*obj_ptr };
                if obj.class_id == class_id_a {
                    count_a += 1;
                }
            }
        }
        assert_eq!(count_a, 3);
    }
}

// ============================================================================
// Snapshot Tests
// ============================================================================

mod snapshot_tests {
    use super::*;

    #[test]
    fn test_snapshot_primitive_values() {
        // Test SnapshotValue for primitives
        let null_val = SnapshotValue::Null;
        let bool_val = SnapshotValue::Boolean(true);
        let int_val = SnapshotValue::Integer(42);
        let float_val = SnapshotValue::Float(3.14);
        let str_val = SnapshotValue::String("test".to_string());

        // Check type names
        assert_eq!(null_val.type_name(), "null");
        assert_eq!(bool_val.type_name(), "boolean");
        assert_eq!(int_val.type_name(), "number");
        assert_eq!(float_val.type_name(), "number");
        assert_eq!(str_val.type_name(), "string");
    }

    #[test]
    fn test_object_snapshot_with_nested_fields() {
        let mut snapshot = ObjectSnapshot::new("Parent".to_string(), 100);
        snapshot.add_field("name".to_string(), SnapshotValue::String("root".to_string()));

        // Add a nested object reference
        let mut child_snapshot = ObjectSnapshot::new("Child".to_string(), 200);
        child_snapshot.add_field("value".to_string(), SnapshotValue::Integer(42));

        snapshot.add_field(
            "child".to_string(),
            SnapshotValue::Object(child_snapshot),
        );

        assert_eq!(snapshot.fields.len(), 2);
        assert!(snapshot.fields.contains_key("name"));
        assert!(snapshot.fields.contains_key("child"));
    }

    #[test]
    fn test_diff_complex_changes() {
        let mut old = ObjectSnapshot::new("Config".to_string(), 1);
        old.add_field("debug".to_string(), SnapshotValue::Boolean(false));
        old.add_field("timeout".to_string(), SnapshotValue::Integer(1000));
        old.add_field("deprecated".to_string(), SnapshotValue::Null);

        let mut new = ObjectSnapshot::new("Config".to_string(), 1);
        new.add_field("debug".to_string(), SnapshotValue::Boolean(true)); // changed
        new.add_field("timeout".to_string(), SnapshotValue::Integer(1000)); // unchanged
        new.add_field("version".to_string(), SnapshotValue::String("2.0".to_string())); // added
        // deprecated removed

        let diff = ObjectDiff::compute(&old, &new);

        assert!(!diff.is_empty());
        assert!(diff.changed.contains_key("debug"));
        assert!(diff.added.contains(&"version".to_string()));
        assert!(diff.removed.contains(&"deprecated".to_string()));
        assert!(!diff.changed.contains_key("timeout"));
    }

    #[test]
    fn test_diff_array_changes() {
        let mut old = ObjectSnapshot::new("Container".to_string(), 1);
        old.add_field(
            "items".to_string(),
            SnapshotValue::Array(vec![
                SnapshotValue::Integer(1),
                SnapshotValue::Integer(2),
            ]),
        );

        let mut new = ObjectSnapshot::new("Container".to_string(), 1);
        new.add_field(
            "items".to_string(),
            SnapshotValue::Array(vec![
                SnapshotValue::Integer(1),
                SnapshotValue::Integer(2),
                SnapshotValue::Integer(3),
            ]),
        );

        let diff = ObjectDiff::compute(&old, &new);
        assert!(!diff.is_empty());
        assert!(diff.changed.contains_key("items"));
    }
}

// ============================================================================
// Stack Introspection Tests
// ============================================================================

mod stack_introspection {
    use raya_engine::vm::stack::Stack;

    #[test]
    fn test_stack_frame_creation() {
        let mut stack = Stack::new();

        // Push some locals
        stack.push(raya_engine::vm::value::Value::i32(10)).unwrap();
        stack.push(raya_engine::vm::value::Value::i32(20)).unwrap();

        assert_eq!(stack.depth(), 2);
    }

    #[test]
    fn test_stack_local_access() {
        let mut stack = Stack::new();

        // Push locals
        stack.push(raya_engine::vm::value::Value::i32(100)).unwrap();
        stack.push(raya_engine::vm::value::Value::i32(200)).unwrap();
        stack.push(raya_engine::vm::value::Value::i32(300)).unwrap();

        // Access values at stack positions using peek_at (works without a call frame)
        let val0 = stack.peek_at(0).unwrap();
        let val1 = stack.peek_at(1).unwrap();
        let val2 = stack.peek_at(2).unwrap();

        assert_eq!(val0.as_i32(), Some(100));
        assert_eq!(val1.as_i32(), Some(200));
        assert_eq!(val2.as_i32(), Some(300));
    }

    #[test]
    fn test_call_frame_iteration() {
        let stack = Stack::new();

        // Without any frames, iteration should be empty
        let frames: Vec<_> = stack.frames().collect();
        assert!(frames.is_empty());
    }
}

// ============================================================================
// Debug Info Tests
// ============================================================================

mod debug_info {
    use raya_engine::compiler::bytecode::{ClassDebugInfo, DebugInfo, FunctionDebugInfo};

    #[test]
    fn test_debug_info_creation() {
        let debug_info = DebugInfo::new();
        assert!(debug_info.source_files.is_empty());
        assert!(debug_info.functions.is_empty());
        assert!(debug_info.classes.is_empty());
    }

    #[test]
    fn test_function_debug_info() {
        let mut func_debug = FunctionDebugInfo::new(
            0, // source_file_index
            10, // start_line
            1, // start_column
            20, // end_line
            1, // end_column
        );

        // Add line entries
        func_debug.add_line_entry(0, 10, 1);
        func_debug.add_line_entry(5, 12, 5);
        func_debug.add_line_entry(10, 15, 1);

        // Test line lookup
        let loc0 = func_debug.lookup_location(0).unwrap();
        assert_eq!(loc0.line, 10);
        assert_eq!(loc0.column, 1);

        let loc5 = func_debug.lookup_location(5).unwrap();
        assert_eq!(loc5.line, 12);
        assert_eq!(loc5.column, 5);

        let loc7 = func_debug.lookup_location(7).unwrap(); // Between entries
        assert_eq!(loc7.line, 12); // Should use previous entry

        let loc10 = func_debug.lookup_location(10).unwrap();
        assert_eq!(loc10.line, 15);
    }

    #[test]
    fn test_debug_info_source_file_lookup() {
        let mut debug_info = DebugInfo::new();
        debug_info.add_source_file("main.raya".to_string());
        debug_info.add_source_file("utils.raya".to_string());

        assert_eq!(debug_info.get_source_file(0), Some("main.raya"));
        assert_eq!(debug_info.get_source_file(1), Some("utils.raya"));
        assert_eq!(debug_info.get_source_file(2), None);
    }

    #[test]
    fn test_class_debug_info() {
        let class_debug = ClassDebugInfo {
            source_file_index: 0,
            start_line: 5,
            start_column: 1,
            end_line: 50,
            end_column: 1,
        };

        assert_eq!(class_debug.source_file_index, 0);
        assert_eq!(class_debug.start_line, 5);
    }
}

// ============================================================================
// Value Type Tests for Reflect
// ============================================================================

mod reflect_value_tests {
    use raya_engine::vm::value::Value;

    #[test]
    fn test_value_type_checks() {
        // These are used by Reflect.isString, isNumber, etc.
        let null_val = Value::null();
        let bool_val = Value::bool(true);
        let i32_val = Value::i32(42);
        let f64_val = Value::f64(3.14);

        assert!(null_val.is_null());
        assert!(!bool_val.is_null());

        assert_eq!(bool_val.as_bool(), Some(true));
        assert_eq!(i32_val.as_i32(), Some(42));
        assert_eq!(f64_val.as_f64(), Some(3.14));
    }

    #[test]
    fn test_value_pointer_detection() {
        let i32_val = Value::i32(42);
        let null_val = Value::null();

        // Primitives are not pointers
        assert!(!i32_val.is_ptr());
        // Null is a special case
        assert!(null_val.is_null());
    }
}

// ============================================================================
// Object ID and Identity Tests
// ============================================================================

mod object_identity {
    use raya_engine::vm::gc::GarbageCollector;
    use raya_engine::vm::object::Object;
    use raya_engine::vm::value::Value;

    #[test]
    fn test_object_identity_via_pointer() {
        let mut gc = GarbageCollector::default();

        let obj1 = gc.allocate(Object::new(0, 2));
        let obj2 = gc.allocate(Object::new(0, 2));

        // Different allocations have different pointers
        let ptr1 = obj1.as_ptr() as usize;
        let ptr2 = obj2.as_ptr() as usize;

        assert_ne!(ptr1, ptr2);
    }

    #[test]
    fn test_reflect_get_object_id() {
        use raya_engine::vm::reflect::get_class_id;

        let mut gc = GarbageCollector::default();
        let obj = gc.allocate(Object::new(42, 3));

        // Create Value from the object pointer
        let value =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(obj.as_ptr() as *mut Object).unwrap()) };

        // get_class_id should return the class ID
        let class_id = get_class_id(value);
        assert_eq!(class_id, Some(42));
    }
}

// ============================================================================
// Phase 9: Proxy Object Tests
// ============================================================================

mod proxy_tests {
    use raya_engine::vm::gc::GarbageCollector;
    use raya_engine::vm::object::{Object, Proxy};
    use raya_engine::vm::value::Value;

    #[test]
    fn test_proxy_creation() {
        let mut gc = GarbageCollector::default();

        // Create target and handler objects
        let target_obj = gc.allocate(Object::new(1, 2));
        let handler_obj = gc.allocate(Object::new(2, 4));

        let target_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(target_obj.as_ptr() as *mut Object).unwrap()) };
        let handler_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(handler_obj.as_ptr() as *mut Object).unwrap()) };

        // Create proxy
        let proxy = Proxy::new(target_val, handler_val);
        assert_ne!(proxy.proxy_id, 0);
    }

    #[test]
    fn test_proxy_get_target() {
        let mut gc = GarbageCollector::default();

        let target_obj = gc.allocate(Object::new(42, 3));
        let handler_obj = gc.allocate(Object::new(0, 0));

        let target_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(target_obj.as_ptr() as *mut Object).unwrap()) };
        let handler_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(handler_obj.as_ptr() as *mut Object).unwrap()) };

        let proxy = Proxy::new(target_val, handler_val);

        // get_target should return the original target
        let retrieved_target = proxy.get_target();
        assert_eq!(retrieved_target.raw(), target_val.raw());
    }

    #[test]
    fn test_proxy_get_handler() {
        let mut gc = GarbageCollector::default();

        let target_obj = gc.allocate(Object::new(1, 1));
        let handler_obj = gc.allocate(Object::new(99, 4));

        let target_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(target_obj.as_ptr() as *mut Object).unwrap()) };
        let handler_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(handler_obj.as_ptr() as *mut Object).unwrap()) };

        let proxy = Proxy::new(target_val, handler_val);

        // get_handler should return the original handler
        let retrieved_handler = proxy.get_handler();
        assert_eq!(retrieved_handler.raw(), handler_val.raw());
    }

    #[test]
    fn test_proxy_allocation_via_gc() {
        let mut gc = GarbageCollector::default();

        let target_obj = gc.allocate(Object::new(1, 2));
        let handler_obj = gc.allocate(Object::new(2, 4));

        let target_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(target_obj.as_ptr() as *mut Object).unwrap()) };
        let handler_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(handler_obj.as_ptr() as *mut Object).unwrap()) };

        // Allocate proxy through GC (like the handler does)
        let proxy = Proxy::new(target_val, handler_val);
        let proxy_gc = gc.allocate(proxy);

        // Can dereference and access fields
        let proxy_ref = unsafe { &*proxy_gc.as_ptr() };
        assert_eq!(proxy_ref.get_target().raw(), target_val.raw());
        assert_eq!(proxy_ref.get_handler().raw(), handler_val.raw());
    }

    #[test]
    fn test_proxy_is_distinct_from_target() {
        let mut gc = GarbageCollector::default();

        let target_obj = gc.allocate(Object::new(1, 2));
        let handler_obj = gc.allocate(Object::new(2, 4));

        let target_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(target_obj.as_ptr() as *mut Object).unwrap()) };
        let handler_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(handler_obj.as_ptr() as *mut Object).unwrap()) };

        let proxy = Proxy::new(target_val, handler_val);
        let proxy_gc = gc.allocate(proxy);
        let proxy_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(proxy_gc.as_ptr() as *mut Proxy).unwrap()) };

        // Proxy value should be different from target value
        assert_ne!(proxy_val.raw(), target_val.raw());
    }

    #[test]
    fn test_proxy_unique_ids() {
        let mut gc = GarbageCollector::default();

        let target1 = gc.allocate(Object::new(1, 1));
        let handler1 = gc.allocate(Object::new(0, 0));
        let target2 = gc.allocate(Object::new(2, 1));
        let handler2 = gc.allocate(Object::new(0, 0));

        let target1_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(target1.as_ptr() as *mut Object).unwrap()) };
        let handler1_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(handler1.as_ptr() as *mut Object).unwrap()) };
        let target2_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(target2.as_ptr() as *mut Object).unwrap()) };
        let handler2_val =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(handler2.as_ptr() as *mut Object).unwrap()) };

        let proxy1 = Proxy::new(target1_val, handler1_val);
        let proxy2 = Proxy::new(target2_val, handler2_val);

        // Each proxy should have unique ID
        assert_ne!(proxy1.proxy_id, proxy2.proxy_id);
    }
}
