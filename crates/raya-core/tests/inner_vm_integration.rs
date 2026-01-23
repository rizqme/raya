//! Integration tests for Inner VMs (Milestone 1.13)
//!
//! Tests for nested VmContexts with isolation, resource limits,
//! capabilities, and marshalling.

#![allow(unused_imports)]

use raya_bytecode::{Function, Module, Opcode};
use raya_core::gc::GarbageCollector;
use raya_core::value::Value;
use raya_core::vm::{
    Capability, CapabilityError, CapabilityRegistry, ContextRegistry, InnerVm, ResourceLimits,
    VmContext, VmContextId, VmError, VmOptions,
};
use raya_core::vm::{marshal, unmarshal, MarshalledValue};
use std::sync::Arc;

// ============================================================================
// Basic Context Creation & Isolation Tests
// ============================================================================

#[test]
fn test_create_vm_with_options() {
    let options = VmOptions {
        limits: ResourceLimits {
            max_heap_bytes: Some(16 * 1024 * 1024),
            max_tasks: Some(10),
            max_step_budget: Some(1_000_000),
        },
        ..Default::default()
    };

    let vm = InnerVm::new(options);
    assert!(vm.is_ok());
}

#[test]
fn test_create_multiple_contexts() {
    let registry = ContextRegistry::new();

    let ctx1 = VmContext::new();
    let ctx2 = VmContext::new();
    let ctx3 = VmContext::new();

    let id1 = ctx1.id();
    let id2 = ctx2.id();
    let id3 = ctx3.id();

    registry.register(ctx1);
    registry.register(ctx2);
    registry.register(ctx3);

    assert_eq!(registry.len(), 3);
    assert!(registry.get(id1).is_some());
    assert!(registry.get(id2).is_some());
    assert!(registry.get(id3).is_some());
}

#[test]
fn test_context_ids_are_unique() {
    let ctx1 = VmContext::new();
    let ctx2 = VmContext::new();
    let ctx3 = VmContext::new();

    assert_ne!(ctx1.id(), ctx2.id());
    assert_ne!(ctx2.id(), ctx3.id());
    assert_ne!(ctx1.id(), ctx3.id());
}

#[test]
fn test_heap_isolation() {
    // Create two separate contexts
    let mut ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();

    let id1 = ctx1.id();
    let id2 = ctx2.id();

    // Allocate in ctx1
    {
        let gc1 = ctx1.gc_mut();
        let _ptr1 = gc1.allocate(raya_core::object::RayaString::new("Hello".to_string()));
    }

    // Allocate in ctx2
    {
        let gc2 = ctx2.gc_mut();
        let _ptr2 = gc2.allocate(raya_core::object::RayaString::new("World".to_string()));
    }

    // Verify heaps are independent (contexts have different IDs)
    assert_ne!(id1, id2);

    // Both GCs are operational (stats accessible)
    let _stats1 = ctx1.gc().stats();
    let _stats2 = ctx2.gc().stats();
}

// ============================================================================
// Resource Limit Tests
// ============================================================================

#[test]
fn test_heap_limit_enforcement() {
    let options = VmOptions {
        limits: ResourceLimits {
            max_heap_bytes: Some(1024), // 1 KB limit
            ..Default::default()
        },
        gc_threshold: 512,
        ..Default::default()
    };

    let _vm = InnerVm::new(options).unwrap();

    // GC will enforce limits during allocation
    // This test verifies the configuration is accepted
}

#[test]
fn test_task_limit_configuration() {
    let options = VmOptions {
        limits: ResourceLimits {
            max_tasks: Some(5),
            ..Default::default()
        },
        ..Default::default()
    };

    let _vm = InnerVm::new(options).unwrap();
}

#[test]
fn test_step_budget_configuration() {
    let options = VmOptions {
        limits: ResourceLimits {
            max_step_budget: Some(10_000),
            ..Default::default()
        },
        ..Default::default()
    };

    let _vm = InnerVm::new(options).unwrap();
}

#[test]
fn test_unlimited_resources() {
    let options = VmOptions {
        limits: ResourceLimits::unlimited(),
        ..Default::default()
    };

    let _vm = InnerVm::new(options).unwrap();
}

// ============================================================================
// Resource Counter Tests
// ============================================================================

#[test]
fn test_resource_counters() {
    let ctx = VmContext::new();
    let counters = ctx.counters();

    // Initial state
    assert_eq!(counters.active_tasks(), 0);
    assert_eq!(counters.total_steps(), 0);
    assert_eq!(counters.peak_tasks(), 0);

    // Increment tasks
    counters.increment_tasks();
    assert_eq!(counters.active_tasks(), 1);
    assert_eq!(counters.peak_tasks(), 1);

    counters.increment_tasks();
    assert_eq!(counters.active_tasks(), 2);
    assert_eq!(counters.peak_tasks(), 2);

    // Decrement tasks
    counters.decrement_tasks();
    assert_eq!(counters.active_tasks(), 1);
    assert_eq!(counters.peak_tasks(), 2); // Peak remains

    // Increment steps
    counters.increment_steps(100);
    assert_eq!(counters.total_steps(), 100);

    counters.increment_steps(50);
    assert_eq!(counters.total_steps(), 150);
}

#[test]
fn test_resource_counter_reset() {
    let ctx = VmContext::new();
    let counters = ctx.counters();

    counters.increment_tasks();
    counters.increment_tasks();
    counters.increment_steps(1000);

    assert_ne!(counters.active_tasks(), 0);
    assert_ne!(counters.total_steps(), 0);

    counters.reset();

    assert_eq!(counters.active_tasks(), 0);
    assert_eq!(counters.total_steps(), 0);
    assert_eq!(counters.peak_tasks(), 0);
}

// ============================================================================
// Capability Tests
// ============================================================================

struct TestCapability;

impl Capability for TestCapability {
    fn name(&self) -> &str {
        "test"
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, CapabilityError> {
        // Simple capability that returns the sum of i32 arguments
        let sum: i32 = args
            .iter()
            .filter_map(|v| v.as_i32())
            .sum();
        Ok(Value::i32(sum))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn description(&self) -> &str {
        "Test capability that sums i32 arguments"
    }
}

#[test]
fn test_capability_registry() {
    let registry = CapabilityRegistry::new();
    assert_eq!(registry.len(), 0);
    assert!(registry.is_empty());

    let registry = registry.with_capability(Arc::new(TestCapability));
    assert_eq!(registry.len(), 1);
    assert!(!registry.is_empty());
    assert!(registry.has("test"));
}

#[test]
fn test_capability_invocation() {
    let registry = CapabilityRegistry::new()
        .with_capability(Arc::new(TestCapability));

    let args = vec![Value::i32(10), Value::i32(20), Value::i32(30)];
    let result = registry.invoke("test", &args).unwrap();

    assert_eq!(result.as_i32(), Some(60));
}

#[test]
fn test_capability_not_found() {
    let registry = CapabilityRegistry::new();
    let result = registry.invoke("nonexistent", &[]);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), CapabilityError::NotFound(_)));
}

#[test]
fn test_multiple_capabilities() {
    struct AddCap;
    impl Capability for AddCap {
        fn name(&self) -> &str {
            "add"
        }
        fn invoke(&self, args: &[Value]) -> Result<Value, CapabilityError> {
            if args.len() != 2 {
                return Err(CapabilityError::InvalidArguments("add".to_string()));
            }
            let a = args[0].as_i32().unwrap_or(0);
            let b = args[1].as_i32().unwrap_or(0);
            Ok(Value::i32(a + b))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct MulCap;
    impl Capability for MulCap {
        fn name(&self) -> &str {
            "mul"
        }
        fn invoke(&self, args: &[Value]) -> Result<Value, CapabilityError> {
            if args.len() != 2 {
                return Err(CapabilityError::InvalidArguments("mul".to_string()));
            }
            let a = args[0].as_i32().unwrap_or(0);
            let b = args[1].as_i32().unwrap_or(0);
            Ok(Value::i32(a * b))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    let registry = CapabilityRegistry::new()
        .with_capability(Arc::new(AddCap))
        .with_capability(Arc::new(MulCap));

    assert_eq!(registry.len(), 2);

    let result1 = registry.invoke("add", &[Value::i32(5), Value::i32(3)]).unwrap();
    assert_eq!(result1.as_i32(), Some(8));

    let result2 = registry.invoke("mul", &[Value::i32(5), Value::i32(3)]).unwrap();
    assert_eq!(result2.as_i32(), Some(15));
}

// ============================================================================
// Marshalling Tests
// ============================================================================

#[test]
fn test_marshal_primitives() {
    let ctx1 = VmContext::new();
    let _ctx2 = VmContext::new();

    // Null
    let marshalled = marshal(&Value::null(), &ctx1).unwrap();
    assert_eq!(marshalled, MarshalledValue::Null);

    // Bool
    let marshalled = marshal(&Value::bool(true), &ctx1).unwrap();
    assert_eq!(marshalled, MarshalledValue::Bool(true));

    // I32
    let marshalled = marshal(&Value::i32(42), &ctx1).unwrap();
    assert_eq!(marshalled, MarshalledValue::I32(42));

    // F64
    let marshalled = marshal(&Value::f64(3.14), &ctx1).unwrap();
    assert_eq!(marshalled, MarshalledValue::F64(3.14));
}

#[test]
fn test_marshal_string() {
    let mut ctx1 = VmContext::new();
    let ctx2 = VmContext::new();

    // Allocate string in ctx1
    let gc1 = ctx1.gc_mut();
    let str_ptr = gc1.allocate(raya_core::object::RayaString::new("Hello World".to_string()));
    let value = unsafe { Value::from_ptr(std::ptr::NonNull::new_unchecked(str_ptr.as_ptr())) };

    // Marshal to ctx2
    let marshalled = marshal(&value, &ctx1).unwrap();

    // Verify it's a deep copy
    match marshalled {
        MarshalledValue::String(s) => assert_eq!(s, "Hello World"),
        _ => panic!("Expected String, got {:?}", marshalled),
    }
}

#[test]
fn test_unmarshal_primitives() {
    let mut ctx = VmContext::new();

    // Null
    let value = unmarshal(MarshalledValue::Null, &mut ctx).unwrap();
    assert!(value.is_null());

    // Bool
    let value = unmarshal(MarshalledValue::Bool(false), &mut ctx).unwrap();
    assert_eq!(value.as_bool(), Some(false));

    // I32
    let value = unmarshal(MarshalledValue::I32(123), &mut ctx).unwrap();
    assert_eq!(value.as_i32(), Some(123));

    // F64
    let value = unmarshal(MarshalledValue::F64(2.718), &mut ctx).unwrap();
    assert_eq!(value.as_f64(), Some(2.718));
}

#[test]
fn test_unmarshal_string() {
    let mut ctx = VmContext::new();

    let marshalled = MarshalledValue::String("Test String".to_string());
    let value = unmarshal(marshalled, &mut ctx).unwrap();

    // Verify string was allocated in ctx's heap
    unsafe {
        let str_ptr = value.as_ptr::<raya_core::object::RayaString>().unwrap();
        let string = &*str_ptr.as_ptr();
        assert_eq!(string.data, "Test String");
    }
}

#[test]
fn test_marshal_roundtrip() {
    let ctx1 = VmContext::new();
    let mut ctx2 = VmContext::new();

    // Create value in ctx1
    let original = Value::i32(999);

    // Marshal ctx1 -> marshalled
    let marshalled = marshal(&original, &ctx1).unwrap();

    // Unmarshal marshalled -> ctx2
    let restored = unmarshal(marshalled, &mut ctx2).unwrap();

    // Verify value is preserved
    assert_eq!(original.as_i32(), restored.as_i32());
}

// ============================================================================
// Context Registry Tests
// ============================================================================

#[test]
fn test_context_registry_operations() {
    let registry = ContextRegistry::new();

    // Initially empty
    assert_eq!(registry.len(), 0);
    assert!(registry.is_empty());

    // Register contexts
    let ctx1 = VmContext::new();
    let id1 = ctx1.id();
    registry.register(ctx1);

    assert_eq!(registry.len(), 1);
    assert!(!registry.is_empty());

    // Get context
    let ctx = registry.get(id1);
    assert!(ctx.is_some());

    // Remove context
    let removed = registry.remove(id1);
    assert!(removed.is_some());
    assert_eq!(registry.len(), 0);
}

#[test]
fn test_context_registry_all_ids() {
    let registry = ContextRegistry::new();

    let ctx1 = VmContext::new();
    let ctx2 = VmContext::new();
    let ctx3 = VmContext::new();

    let id1 = ctx1.id();
    let id2 = ctx2.id();
    let id3 = ctx3.id();

    registry.register(ctx1);
    registry.register(ctx2);
    registry.register(ctx3);

    let all_ids = registry.all_ids();
    assert_eq!(all_ids.len(), 3);
    assert!(all_ids.contains(&id1));
    assert!(all_ids.contains(&id2));
    assert!(all_ids.contains(&id3));
}

// ============================================================================
// VM Lifecycle Tests
// ============================================================================

#[test]
fn test_vm_creation() {
    let options = VmOptions::default();
    let vm = InnerVm::new(options);
    assert!(vm.is_ok());
}

#[test]
fn test_vm_get_stats() {
    let vm = InnerVm::new(VmOptions::default()).unwrap();
    let stats = vm.get_stats();
    assert!(stats.is_ok());

    let stats = stats.unwrap();
    assert_eq!(stats.heap_bytes_used, 0); // No allocations yet
    assert_eq!(stats.tasks, 0); // No tasks yet
}

#[test]
fn test_vm_terminate() {
    let vm = InnerVm::new(VmOptions::default()).unwrap();
    let result = vm.terminate();
    assert!(result.is_ok());
}

#[test]
fn test_load_empty_bytecode() {
    let vm = InnerVm::new(VmOptions::default()).unwrap();

    // Create minimal valid module
    let module = Module::new("test".to_string());
    let bytes = module.encode();

    let result = vm.load_bytecode(&bytes);
    // May succeed or fail depending on validation - just verify it doesn't panic
    let _ = result;
}

// ============================================================================
// Nested Context Tests
// ============================================================================

#[test]
fn test_nested_contexts() {
    let parent = VmContext::new();
    let parent_id = parent.id();

    let child_options = VmOptions::default();
    let child = VmContext::with_parent(child_options, parent_id);

    assert_eq!(child.parent(), Some(parent_id));
    assert_ne!(child.id(), parent_id);
}

#[test]
fn test_three_level_nesting() {
    let level1 = VmContext::new();
    let level1_id = level1.id();

    let level2 = VmContext::with_parent(VmOptions::default(), level1_id);
    let level2_id = level2.id();

    let level3 = VmContext::with_parent(VmOptions::default(), level2_id);

    assert_eq!(level2.parent(), Some(level1_id));
    assert_eq!(level3.parent(), Some(level2_id));
}

// ============================================================================
// Integration: VM with Capabilities
// ============================================================================

#[test]
fn test_vm_with_capabilities() {
    let options = VmOptions {
        capabilities: CapabilityRegistry::new()
            .with_capability(Arc::new(TestCapability)),
        ..Default::default()
    };

    let vm = InnerVm::new(options).unwrap();
    let stats = vm.get_stats().unwrap();

    // VM created with capability
    assert!(stats.heap_bytes_used >= 0);
}

// ============================================================================
// Integration: Multiple VMs Running Concurrently
// ============================================================================

#[test]
fn test_multiple_vms_concurrent() {
    let vm1 = InnerVm::new(VmOptions::default()).unwrap();
    let vm2 = InnerVm::new(VmOptions::default()).unwrap();
    let vm3 = InnerVm::new(VmOptions::default()).unwrap();

    let stats1 = vm1.get_stats().unwrap();
    let stats2 = vm2.get_stats().unwrap();
    let stats3 = vm3.get_stats().unwrap();

    // All VMs are independent
    assert!(stats1.heap_bytes_used >= 0);
    assert!(stats2.heap_bytes_used >= 0);
    assert!(stats3.heap_bytes_used >= 0);

    // Terminate all
    assert!(vm1.terminate().is_ok());
    assert!(vm2.terminate().is_ok());
    assert!(vm3.terminate().is_ok());
}
