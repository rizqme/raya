# Milestone 1.14: Integration Testing & Validation

**Status:** Ready for Implementation
**Priority:** High
**Estimated Effort:** 2-3 weeks
**Dependencies:** Milestones 1.2-1.13

---

## Overview

This milestone focuses on comprehensive integration testing and validation of all VM systems implemented in Phase 1. The goal is to achieve >90% test coverage, validate correctness under stress conditions, and establish performance baselines through benchmarks.

### Objectives

- ✅ Comprehensive opcode test coverage (all 150+ opcodes)
- ✅ GC stress testing (allocation patterns, memory pressure, fragmentation)
- ✅ Multi-context isolation validation
- ✅ Concurrent task execution correctness
- ✅ Snapshot/restore validation and consistency
- ✅ Inner VM security boundary enforcement
- ✅ Resource limit enforcement under stress
- ✅ Performance benchmarking and regression detection
- ✅ End-to-end integration scenarios

### Success Criteria

- All VM systems passing comprehensive tests
- >90% code coverage for raya-core
- >85% coverage for GC and memory management
- Zero memory leaks in 24-hour stress tests
- Performance benchmarks within acceptable ranges
- All design examples from specification working

---

## Test Categories

### 1. Opcode Test Suite

**File:** `crates/raya-core/tests/opcode_tests.rs`

**Objective:** Validate correctness of every VM opcode in isolation and composition.

#### Test Structure

```rust
#[cfg(test)]
mod opcode_tests {
    use raya_core::vm::Vm;
    use raya_bytecode::{Opcode, Module};

    // Constants (0x00-0x0F)
    mod constants {
        #[test]
        fn test_const_null() { /* ... */ }

        #[test]
        fn test_const_true() { /* ... */ }

        #[test]
        fn test_const_false() { /* ... */ }

        #[test]
        fn test_const_i32() { /* ... */ }

        #[test]
        fn test_const_f64() { /* ... */ }

        #[test]
        fn test_iconst_m1() { /* ... */ }

        #[test]
        fn test_iconst_0_through_5() { /* ... */ }

        #[test]
        fn test_fconst_0_and_1() { /* ... */ }
    }

    // Stack operations (0x10-0x1F)
    mod stack_ops {
        #[test]
        fn test_pop() { /* ... */ }

        #[test]
        fn test_dup() { /* ... */ }

        #[test]
        fn test_dup2() { /* ... */ }

        #[test]
        fn test_swap() { /* ... */ }

        #[test]
        fn test_stack_underflow() { /* ... */ }

        #[test]
        fn test_stack_overflow() { /* ... */ }
    }

    // Arithmetic (0x20-0x3F)
    mod arithmetic {
        #[test]
        fn test_iadd() { /* ... */ }

        #[test]
        fn test_isub() { /* ... */ }

        #[test]
        fn test_imul() { /* ... */ }

        #[test]
        fn test_idiv() { /* ... */ }

        #[test]
        fn test_idiv_by_zero() { /* ... */ }

        #[test]
        fn test_irem() { /* ... */ }

        #[test]
        fn test_ineg() { /* ... */ }

        #[test]
        fn test_fadd() { /* ... */ }

        #[test]
        fn test_fsub() { /* ... */ }

        #[test]
        fn test_fmul() { /* ... */ }

        #[test]
        fn test_fdiv() { /* ... */ }

        #[test]
        fn test_fneg() { /* ... */ }

        #[test]
        fn test_nadd() { /* ... */ } // Number (i32 or f64)

        #[test]
        fn test_nsub() { /* ... */ }

        #[test]
        fn test_nmul() { /* ... */ }

        #[test]
        fn test_ndiv() { /* ... */ }
    }

    // Comparison (0x40-0x4F)
    mod comparison {
        #[test]
        fn test_ieq() { /* ... */ }

        #[test]
        fn test_ine() { /* ... */ }

        #[test]
        fn test_ilt() { /* ... */ }

        #[test]
        fn test_ile() { /* ... */ }

        #[test]
        fn test_igt() { /* ... */ }

        #[test]
        fn test_ige() { /* ... */ }

        #[test]
        fn test_feq() { /* ... */ }

        #[test]
        fn test_flt() { /* ... */ }

        #[test]
        fn test_seq() { /* ... */ } // Strict equality

        #[test]
        fn test_sne() { /* ... */ }
    }

    // Control flow (0x50-0x5F)
    mod control_flow {
        #[test]
        fn test_jmp() { /* ... */ }

        #[test]
        fn test_jmp_if_true() { /* ... */ }

        #[test]
        fn test_jmp_if_false() { /* ... */ }

        #[test]
        fn test_jmp_if_null() { /* ... */ }

        #[test]
        fn test_jmp_if_not_null() { /* ... */ }

        #[test]
        fn test_jmp_backward() { /* ... */ }

        #[test]
        fn test_nested_branches() { /* ... */ }

        #[test]
        fn test_loop_with_backward_jump() { /* ... */ }
    }

    // Function calls (0x60-0x6F)
    mod function_calls {
        #[test]
        fn test_call() { /* ... */ }

        #[test]
        fn test_call_native() { /* ... */ }

        #[test]
        fn test_return() { /* ... */ }

        #[test]
        fn test_return_void() { /* ... */ }

        #[test]
        fn test_recursive_call() { /* ... */ }

        #[test]
        fn test_tail_call() { /* ... */ }

        #[test]
        fn test_stack_frame_isolation() { /* ... */ }
    }

    // Local variables (0x70-0x7F)
    mod local_variables {
        #[test]
        fn test_load_local() { /* ... */ }

        #[test]
        fn test_store_local() { /* ... */ }

        #[test]
        fn test_load_local_0_through_3() { /* ... */ }

        #[test]
        fn test_store_local_0_through_3() { /* ... */ }

        #[test]
        fn test_local_bounds_checking() { /* ... */ }
    }

    // Global variables (0x80-0x8F)
    mod global_variables {
        #[test]
        fn test_load_global() { /* ... */ }

        #[test]
        fn test_store_global() { /* ... */ }

        #[test]
        fn test_global_isolation_across_contexts() { /* ... */ }
    }

    // Object allocation (0x90-0x9F)
    mod object_allocation {
        #[test]
        fn test_new_object() { /* ... */ }

        #[test]
        fn test_new_array() { /* ... */ }

        #[test]
        fn test_new_string() { /* ... */ }

        #[test]
        fn test_array_length() { /* ... */ }

        #[test]
        fn test_string_length() { /* ... */ }

        #[test]
        fn test_allocation_triggers_gc() { /* ... */ }
    }

    // Field access (0xA0-0xAF)
    mod field_access {
        #[test]
        fn test_get_field() { /* ... */ }

        #[test]
        fn test_set_field() { /* ... */ }

        #[test]
        fn test_get_array_element() { /* ... */ }

        #[test]
        fn test_set_array_element() { /* ... */ }

        #[test]
        fn test_array_bounds_checking() { /* ... */ }

        #[test]
        fn test_null_pointer_exception() { /* ... */ }
    }

    // Method calls (0xB0-0xBF)
    mod method_calls {
        #[test]
        fn test_invoke_virtual() { /* ... */ }

        #[test]
        fn test_invoke_interface() { /* ... */ }

        #[test]
        fn test_virtual_dispatch() { /* ... */ }

        #[test]
        fn test_method_not_found() { /* ... */ }
    }

    // Type operations (0xC0-0xCF)
    mod type_operations {
        #[test]
        fn test_typeof() { /* ... */ }

        #[test]
        fn test_is_null() { /* ... */ }

        #[test]
        fn test_is_bool() { /* ... */ }

        #[test]
        fn test_is_number() { /* ... */ }

        #[test]
        fn test_is_string() { /* ... */ }

        #[test]
        fn test_is_object() { /* ... */ }

        #[test]
        fn test_is_array() { /* ... */ }
    }

    // Concurrency (0xD0-0xDF)
    mod concurrency {
        #[test]
        fn test_spawn() { /* ... */ }

        #[test]
        fn test_await() { /* ... */ }

        #[test]
        fn test_yield() { /* ... */ }

        #[test]
        fn test_spawn_and_await_chain() { /* ... */ }

        #[test]
        fn test_concurrent_task_limit() { /* ... */ }
    }

    // Synchronization (0xE0-0xEF)
    mod synchronization {
        #[test]
        fn test_new_mutex() { /* ... */ }

        #[test]
        fn test_mutex_lock() { /* ... */ }

        #[test]
        fn test_mutex_unlock() { /* ... */ }

        #[test]
        fn test_mutex_fifo_fairness() { /* ... */ }

        #[test]
        fn test_mutex_deadlock_detection() { /* ... */ }
    }

    // Special (0xF0-0xFF)
    mod special {
        #[test]
        fn test_nop() { /* ... */ }

        #[test]
        fn test_halt() { /* ... */ }

        #[test]
        fn test_safepoint() { /* ... */ }

        #[test]
        fn test_debug_breakpoint() { /* ... */ }
    }

    // Composite scenarios
    mod composite {
        #[test]
        fn test_factorial_recursive() { /* ... */ }

        #[test]
        fn test_fibonacci_iterative() { /* ... */ }

        #[test]
        fn test_array_sorting() { /* ... */ }

        #[test]
        fn test_object_graph_traversal() { /* ... */ }

        #[test]
        fn test_string_concatenation() { /* ... */ }
    }
}
```

**Test Count:** 150+ individual opcode tests

---

### 2. Garbage Collection Stress Tests

**File:** `crates/raya-core/tests/gc_stress_tests.rs`

**Objective:** Validate GC correctness under extreme conditions.

#### Test Cases

```rust
#[cfg(test)]
mod gc_stress {
    #[test]
    fn test_rapid_allocation_and_collection() {
        // Allocate millions of objects rapidly
        // Verify no memory leaks
        // Check GC triggers correctly
    }

    #[test]
    fn test_allocation_patterns_young_objects() {
        // Allocate many short-lived objects
        // Verify efficient collection
    }

    #[test]
    fn test_allocation_patterns_old_objects() {
        // Create long-lived object graph
        // Verify they survive multiple GC cycles
    }

    #[test]
    fn test_fragmentation_resistance() {
        // Allocate objects of varying sizes
        // Verify heap doesn't fragment excessively
    }

    #[test]
    fn test_circular_references() {
        // Create circular object graphs
        // Verify all garbage is collected
    }

    #[test]
    fn test_deep_object_graphs() {
        // Create deep nesting (1000+ levels)
        // Verify stack doesn't overflow during GC
    }

    #[test]
    fn test_concurrent_allocation_from_tasks() {
        // Multiple tasks allocating concurrently
        // Verify thread-safe GC coordination
    }

    #[test]
    fn test_gc_during_safepoint() {
        // Trigger GC while tasks are at safepoints
        // Verify all tasks pause correctly
    }

    #[test]
    fn test_gc_trigger_threshold() {
        // Verify GC triggers at expected thresholds
        // Test threshold adjustment after collection
    }

    #[test]
    fn test_gc_stats_accuracy() {
        // Verify GC statistics are accurate
        // Check allocated_bytes, freed_bytes, etc.
    }

    #[test]
    fn test_string_interning() {
        // Allocate many duplicate strings
        // Verify deduplication works
    }

    #[test]
    fn test_array_resizing() {
        // Grow arrays dynamically
        // Verify old memory is freed
    }

    #[test]
    fn test_gc_24_hour_stress() {
        // Run for 24 hours with constant allocation
        // Verify no memory leaks
        // Check heap size remains stable
    }

    #[test]
    fn test_gc_with_heap_limit() {
        // Set heap limit
        // Allocate until limit is reached
        // Verify GC frees enough memory
        // Verify allocation fails gracefully when limit hit
    }

    #[test]
    fn test_weak_references() {
        // Create weak references to objects
        // Verify they're cleared after GC
    }
}
```

**Test Count:** 15+ GC stress tests

**Duration:** Some tests run for hours

---

### 3. Multi-Context Isolation Tests

**File:** `crates/raya-core/tests/context_isolation_tests.rs`

**Objective:** Validate complete isolation between VmContexts.

#### Test Cases

```rust
#[cfg(test)]
mod context_isolation {
    #[test]
    fn test_heap_isolation() {
        // Create two contexts
        // Allocate in context 1
        // Verify context 2 can't access context 1's heap
    }

    #[test]
    fn test_global_variable_isolation() {
        // Set global in context 1
        // Verify not visible in context 2
    }

    #[test]
    fn test_gc_isolation() {
        // Trigger GC in context 1
        // Verify context 2 is unaffected
    }

    #[test]
    fn test_task_registry_isolation() {
        // Spawn tasks in context 1
        // Verify not listed in context 2's task registry
    }

    #[test]
    fn test_class_registry_isolation() {
        // Register class in context 1
        // Verify not visible in context 2
    }

    #[test]
    fn test_resource_limit_isolation() {
        // Set heap limit in context 1
        // Verify context 2 has independent limit
    }

    #[test]
    fn test_concurrent_execution_in_multiple_contexts() {
        // Run tasks in 10 different contexts simultaneously
        // Verify no interference
    }

    #[test]
    fn test_parent_child_relationship() {
        // Create parent and child contexts
        // Verify parent link is maintained
        // Verify child can't access parent heap directly
    }

    #[test]
    fn test_marshalling_across_contexts() {
        // Marshal values between contexts
        // Verify deep copying occurs
        // Verify no shared pointers
    }

    #[test]
    fn test_context_termination_cleanup() {
        // Create context with many allocations
        // Terminate context
        // Verify all memory is freed
        // Verify context is removed from registry
    }
}
```

**Test Count:** 10+ isolation tests

---

### 4. Concurrent Task Execution Tests

**File:** `crates/raya-core/tests/task_concurrency_tests.rs`

**Objective:** Validate correctness of concurrent task execution.

#### Test Cases

```rust
#[cfg(test)]
mod task_concurrency {
    #[test]
    fn test_spawn_1000_tasks() {
        // Spawn 1000 tasks
        // Wait for all to complete
        // Verify all results are correct
    }

    #[test]
    fn test_work_stealing() {
        // Spawn tasks on one worker
        // Verify other workers steal work
    }

    #[test]
    fn test_task_fairness() {
        // Run many tasks with varying compute times
        // Verify fair scheduling (no starvation)
    }

    #[test]
    fn test_nested_spawn() {
        // Task spawns task spawns task (10 levels deep)
        // Verify all complete successfully
    }

    #[test]
    fn test_await_chain() {
        // Task A awaits B awaits C awaits D
        // Verify proper suspension and resumption
    }

    #[test]
    fn test_task_parent_child() {
        // Parent spawns children
        // Verify parent ID is set correctly
        // Verify task tree structure
    }

    #[test]
    fn test_task_cancellation() {
        // Spawn task
        // Cancel before completion
        // Verify cleanup occurs
    }

    #[test]
    fn test_preemption() {
        // Run long-running task
        // Verify preemption occurs after step budget
    }

    #[test]
    fn test_scheduler_shutdown() {
        // Spawn many tasks
        // Shutdown scheduler
        // Verify all tasks are terminated gracefully
    }

    #[test]
    fn test_mutex_contention() {
        // 100 tasks contending for 1 mutex
        // Verify FIFO fairness
        // Verify no deadlocks
    }

    #[test]
    fn test_concurrent_gc_trigger() {
        // Multiple tasks allocating concurrently
        // Verify GC can pause all tasks
        // Verify all tasks resume after GC
    }

    #[test]
    fn test_task_state_transitions() {
        // Verify tasks transition: Ready -> Running -> Blocked -> Ready -> Completed
    }
}
```

**Test Count:** 12+ concurrency tests

---

### 5. Snapshot/Restore Validation Tests

**File:** `crates/raya-core/tests/snapshot_validation_tests.rs`

**Objective:** Validate snapshot/restore consistency and correctness.

#### Test Cases

```rust
#[cfg(test)]
mod snapshot_validation {
    #[test]
    fn test_snapshot_empty_vm() {
        // Snapshot VM with no allocations
        // Restore and verify equality
    }

    #[test]
    fn test_snapshot_with_heap_data() {
        // Allocate objects
        // Snapshot
        // Restore
        // Verify all objects are restored correctly
    }

    #[test]
    fn test_snapshot_with_running_tasks() {
        // Spawn tasks
        // Snapshot while tasks are running
        // Restore
        // Verify tasks resume correctly
    }

    #[test]
    fn test_snapshot_with_blocked_tasks() {
        // Tasks waiting on mutex
        // Snapshot
        // Restore
        // Verify wait queue is preserved
    }

    #[test]
    fn test_snapshot_globals() {
        // Set global variables
        // Snapshot
        // Restore
        // Verify globals are preserved
    }

    #[test]
    fn test_snapshot_stack_frames() {
        // Call stack with multiple frames
        // Snapshot mid-execution
        // Restore
        // Verify stack is intact
    }

    #[test]
    fn test_snapshot_file_roundtrip() {
        // Snapshot to file
        // Load from file
        // Verify byte-for-byte equality
    }

    #[test]
    fn test_snapshot_checksum() {
        // Snapshot VM
        // Corrupt snapshot data
        // Verify checksum detects corruption
    }

    #[test]
    fn test_snapshot_multiple_contexts() {
        // Multiple VMs with different state
        // Snapshot each
        // Restore
        // Verify each VM is independent
    }

    #[test]
    fn test_snapshot_version_compatibility() {
        // Create snapshot with old version
        // Restore with new version
        // Verify graceful handling
    }

    #[test]
    fn test_restore_with_updated_limits() {
        // Snapshot VM with 16MB limit
        // Restore with 64MB limit
        // Verify new limit is applied
    }
}
```

**Test Count:** 11+ snapshot tests

---

### 6. Inner VM Security Boundary Tests

**File:** `crates/raya-core/tests/inner_vm_security_tests.rs`

**Objective:** Validate security isolation of Inner VMs.

#### Test Cases

```rust
#[cfg(test)]
mod inner_vm_security {
    #[test]
    fn test_capability_enforcement() {
        // Create VM without log capability
        // Attempt to call log
        // Verify CapabilityError is raised
    }

    #[test]
    fn test_heap_limit_enforcement() {
        // Create VM with 1MB heap limit
        // Attempt to allocate 2MB
        // Verify VmError::ResourceLimitExceeded
    }

    #[test]
    fn test_task_limit_enforcement() {
        // Create VM with max 10 tasks
        // Spawn 11th task
        // Verify TaskCreationFailed
    }

    #[test]
    fn test_step_budget_enforcement() {
        // Create VM with 1000 step budget
        // Run infinite loop
        // Verify preemption occurs
    }

    #[test]
    fn test_no_escape_from_heap() {
        // Inner VM creates object
        // Attempt to access from outer VM
        // Verify access is blocked
    }

    #[test]
    fn test_marshalling_prevents_pointer_sharing() {
        // Marshal object from context A to B
        // Modify in B
        // Verify A is unaffected (deep copy)
    }

    #[test]
    fn test_error_containment() {
        // Inner VM throws error
        // Verify outer VM is unaffected
        // Verify error can be caught and handled
    }

    #[test]
    fn test_termination_cleanup() {
        // Inner VM with many allocations
        // Terminate
        // Verify memory is freed
        // Verify tasks are cancelled
    }

    #[test]
    fn test_foreign_handle_isolation() {
        // Create foreign handle in context A
        // Attempt to use in context B
        // Verify ForeignHandleNotFound error
    }

    #[test]
    fn test_capability_can_be_revoked() {
        // Grant capability to VM
        // Revoke capability
        // Verify subsequent calls fail
    }
}
```

**Test Count:** 10+ security tests

---

### 7. Resource Limit Enforcement Tests

**File:** `crates/raya-core/tests/resource_limit_tests.rs`

**Objective:** Validate resource limits under stress.

#### Test Cases

```rust
#[cfg(test)]
mod resource_limits {
    #[test]
    fn test_heap_limit_gradual_approach() {
        // Set 10MB limit
        // Allocate 1MB at a time
        // Verify allocation fails at 10MB
        // Verify GC runs before failure
    }

    #[test]
    fn test_heap_limit_with_gc() {
        // Set 10MB limit
        // Allocate 5MB, free, allocate 5MB more
        // Verify GC reclaims memory
        // Verify can allocate again
    }

    #[test]
    fn test_task_limit_spawn_many() {
        // Set max 100 tasks
        // Spawn 100 successfully
        // 101st spawn fails
    }

    #[test]
    fn test_step_budget_exhaustion() {
        // Set 10000 step budget
        // Run long computation
        // Verify task is preempted
        // Verify can resume with new budget
    }

    #[test]
    fn test_unlimited_resources() {
        // Create VM with no limits
        // Allocate gigabytes
        // Spawn thousands of tasks
        // Verify works (up to system limits)
    }

    #[test]
    fn test_resource_counters_accuracy() {
        // Allocate known amounts
        // Spawn known task counts
        // Execute known step counts
        // Verify counters match exactly
    }

    #[test]
    fn test_peak_tracking() {
        // Vary allocation and task count
        // Verify peak values are tracked correctly
    }
}
```

**Test Count:** 7+ resource limit tests

---

### 8. Performance Benchmarks

**File:** `benches/vm_benchmarks.rs`

**Objective:** Establish performance baselines and detect regressions.

#### Benchmark Suite

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_opcode_execution(c: &mut Criterion) {
    c.bench_function("iadd", |b| {
        b.iter(|| {
            // Execute 1000 IADD instructions
            black_box(execute_iadd_loop())
        });
    });

    c.bench_function("fadd", |b| {
        b.iter(|| black_box(execute_fadd_loop()));
    });

    c.bench_function("function_call", |b| {
        b.iter(|| black_box(execute_function_calls()));
    });

    c.bench_function("virtual_dispatch", |b| {
        b.iter(|| black_box(execute_virtual_calls()));
    });
}

fn benchmark_gc(c: &mut Criterion) {
    c.bench_function("gc_allocation_small", |b| {
        b.iter(|| {
            // Allocate 1000 small objects (16 bytes each)
            black_box(allocate_small_objects())
        });
    });

    c.bench_function("gc_allocation_large", |b| {
        b.iter(|| {
            // Allocate 100 large objects (1KB each)
            black_box(allocate_large_objects())
        });
    });

    c.bench_function("gc_collection_empty", |b| {
        b.iter(|| {
            // Run GC on empty heap
            black_box(gc_collect_empty())
        });
    });

    c.bench_function("gc_collection_full", |b| {
        b.iter(|| {
            // Run GC on full heap
            black_box(gc_collect_full())
        });
    });
}

fn benchmark_concurrency(c: &mut Criterion) {
    c.bench_function("task_spawn", |b| {
        b.iter(|| {
            // Spawn 1000 lightweight tasks
            black_box(spawn_1000_tasks())
        });
    });

    c.bench_function("task_await", |b| {
        b.iter(|| {
            // Spawn and await 100 tasks
            black_box(spawn_and_await_100())
        });
    });

    c.bench_function("work_stealing", |b| {
        b.iter(|| {
            // Measure work stealing latency
            black_box(work_stealing_bench())
        });
    });

    c.bench_function("mutex_contention", |b| {
        b.iter(|| {
            // 10 tasks contending for 1 mutex
            black_box(mutex_contention_bench())
        });
    });
}

fn benchmark_marshalling(c: &mut Criterion) {
    c.bench_function("marshal_primitives", |b| {
        b.iter(|| black_box(marshal_primitives()));
    });

    c.bench_function("marshal_array_1000", |b| {
        b.iter(|| black_box(marshal_array_1000()));
    });

    c.bench_function("marshal_object_graph", |b| {
        b.iter(|| black_box(marshal_object_graph()));
    });

    c.bench_function("unmarshal_roundtrip", |b| {
        b.iter(|| black_box(marshal_unmarshal_roundtrip()));
    });
}

fn benchmark_snapshot(c: &mut Criterion) {
    c.bench_function("snapshot_empty", |b| {
        b.iter(|| black_box(snapshot_empty_vm()));
    });

    c.bench_function("snapshot_10mb_heap", |b| {
        b.iter(|| black_box(snapshot_10mb_heap()));
    });

    c.bench_function("snapshot_with_tasks", |b| {
        b.iter(|| black_box(snapshot_with_100_tasks()));
    });

    c.bench_function("restore_10mb", |b| {
        b.iter(|| black_box(restore_10mb_snapshot()));
    });
}

fn benchmark_context_creation(c: &mut Criterion) {
    c.bench_function("create_context", |b| {
        b.iter(|| black_box(create_vm_context()));
    });

    c.bench_function("create_10_contexts", |b| {
        b.iter(|| black_box(create_10_contexts()));
    });

    c.bench_function("context_with_capabilities", |b| {
        b.iter(|| black_box(create_context_with_caps()));
    });
}

criterion_group!(
    benches,
    benchmark_opcode_execution,
    benchmark_gc,
    benchmark_concurrency,
    benchmark_marshalling,
    benchmark_snapshot,
    benchmark_context_creation
);
criterion_main!(benches);
```

**Performance Targets:**

| Operation | Target | Acceptable Range |
|-----------|--------|------------------|
| IADD execution | <5ns | <10ns |
| FADD execution | <10ns | <20ns |
| Function call | <50ns | <100ns |
| Virtual dispatch | <100ns | <200ns |
| Small object allocation | <100ns | <200ns |
| GC collection (empty) | <1ms | <5ms |
| Task spawn | <10μs | <50μs |
| Task await | <20μs | <100μs |
| Marshal primitive | <50ns | <100ns |
| Snapshot 10MB heap | <100ms | <500ms |
| Restore 10MB | <100ms | <500ms |
| Context creation | <100μs | <500μs |

---

### 9. End-to-End Integration Scenarios

**File:** `crates/raya-core/tests/e2e_scenarios.rs`

**Objective:** Validate complete workflows end-to-end.

#### Scenario Tests

```rust
#[cfg(test)]
mod e2e_scenarios {
    #[test]
    fn scenario_compute_intensive_app() {
        // Load .rbin with fibonacci computation
        // Execute main()
        // Verify correct result
        // Check GC ran during execution
        // Verify no memory leaks
    }

    #[test]
    fn scenario_concurrent_tasks() {
        // Load .rbin that spawns 100 tasks
        // Each task does computation
        // Await all tasks
        // Verify all results correct
    }

    #[test]
    fn scenario_nested_inner_vms() {
        // Create VM A
        // VM A creates VM B (inner)
        // VM B creates VM C (inner inner)
        // Run code in all 3 VMs concurrently
        // Verify isolation
    }

    #[test]
    fn scenario_plugin_system() {
        // Host VM loads plugin .rbin
        // Plugin has limited capabilities
        // Plugin runs computation
        // Returns result to host
        // Host validates result
    }

    #[test]
    fn scenario_snapshot_restore_resume() {
        // Start long computation
        // Snapshot mid-execution
        // Terminate VM
        // Restore from snapshot
        // Resume computation
        // Verify correct final result
    }

    #[test]
    fn scenario_error_recovery() {
        // Inner VM throws error
        // Host catches error
        // Host creates new VM
        // Retries operation
        // Succeeds
    }

    #[test]
    fn scenario_marshalling_pipeline() {
        // VM A produces data
        // Marshal to VM B
        // VM B transforms data
        // Marshal to VM C
        // VM C consumes data
        // Verify data integrity
    }

    #[test]
    fn scenario_resource_limit_exceeded() {
        // Inner VM with 1MB limit
        // Tries to allocate 2MB
        // Catches ResourceLimitExceeded
        // Handles gracefully
    }

    #[test]
    fn scenario_concurrent_gc_across_contexts() {
        // 10 VMs running concurrently
        // All allocating heavily
        // Verify independent GC
        // Verify no interference
    }

    #[test]
    fn scenario_design_example_1() {
        // Implement example from design/INNER_VM.md
        // Verify works as specified
    }

    #[test]
    fn scenario_design_example_2() {
        // Another design doc example
    }

    // ... more scenarios from design docs
}
```

**Test Count:** 10+ end-to-end scenarios

---

## Implementation Tasks

### Task 1: Opcode Test Suite

**Priority:** High
**Estimated Effort:** 3 days

- [ ] Create test module structure
- [ ] Implement tests for all 150+ opcodes
- [ ] Add composite scenario tests
- [ ] Verify 100% opcode coverage
- [ ] Document edge cases

### Task 2: GC Stress Tests

**Priority:** High
**Estimated Effort:** 3 days

- [ ] Implement rapid allocation tests
- [ ] Add fragmentation resistance tests
- [ ] Create circular reference tests
- [ ] Build 24-hour stress test
- [ ] Add concurrent allocation tests
- [ ] Verify zero memory leaks

### Task 3: Context Isolation Tests

**Priority:** High
**Estimated Effort:** 2 days

- [ ] Implement heap isolation tests
- [ ] Add global variable isolation
- [ ] Test GC independence
- [ ] Validate task registry separation
- [ ] Test marshalling correctness
- [ ] Verify cleanup on termination

### Task 4: Concurrency Tests

**Priority:** High
**Estimated Effort:** 3 days

- [ ] Test spawning 1000+ tasks
- [ ] Validate work stealing
- [ ] Test fairness
- [ ] Add preemption tests
- [ ] Test mutex contention
- [ ] Validate GC coordination

### Task 5: Snapshot Tests

**Priority:** Medium
**Estimated Effort:** 2 days

- [ ] Test empty VM snapshots
- [ ] Add heap data preservation
- [ ] Test task state preservation
- [ ] Validate stack frames
- [ ] Test file I/O roundtrip
- [ ] Add checksum validation

### Task 6: Security Tests

**Priority:** High
**Estimated Effort:** 2 days

- [ ] Test capability enforcement
- [ ] Validate resource limits
- [ ] Test heap isolation
- [ ] Add error containment tests
- [ ] Verify marshalling safety
- [ ] Test foreign handle isolation

### Task 7: Resource Limit Tests

**Priority:** Medium
**Estimated Effort:** 1 day

- [ ] Test heap limits
- [ ] Test task limits
- [ ] Test step budgets
- [ ] Verify counter accuracy
- [ ] Test unlimited mode

### Task 8: Performance Benchmarks

**Priority:** Medium
**Estimated Effort:** 3 days

- [ ] Set up Criterion benchmarks
- [ ] Add opcode execution benchmarks
- [ ] Benchmark GC operations
- [ ] Add concurrency benchmarks
- [ ] Benchmark marshalling
- [ ] Benchmark snapshots
- [ ] Document baseline metrics

### Task 9: End-to-End Scenarios

**Priority:** High
**Estimated Effort:** 2 days

- [ ] Implement compute-intensive scenario
- [ ] Add concurrent task scenario
- [ ] Test nested VMs
- [ ] Add plugin system scenario
- [ ] Test snapshot/resume
- [ ] Implement all design doc examples

### Task 10: Coverage Analysis

**Priority:** Medium
**Estimated Effort:** 1 day

- [ ] Set up coverage tooling (tarpaulin/cargo-llvm-cov)
- [ ] Generate coverage reports
- [ ] Identify uncovered code
- [ ] Add tests to reach >90% coverage
- [ ] Document coverage metrics

### Task 11: CI/CD Integration

**Priority:** Medium
**Estimated Effort:** 1 day

- [ ] Add tests to CI pipeline
- [ ] Configure parallel test execution
- [ ] Add benchmark regression detection
- [ ] Set up coverage reporting
- [ ] Add nightly stress test runs

---

## Test Execution Strategy

### Local Development

```bash
# Run all tests
cargo test --workspace

# Run specific test suite
cargo test --test opcode_tests

# Run benchmarks
cargo bench

# Generate coverage report
cargo tarpaulin --out Html --workspace
```

### CI/CD Pipeline

```yaml
# .github/workflows/tests.yml
name: Tests

on: [push, pull_request]

jobs:
  unit-tests:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace --verbose

  integration-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace --test '*' --verbose

  benchmarks:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo bench --no-fail-fast
      - uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: 'cargo'
          output-file-path: target/criterion/estimates.json

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cargo-tarpaulin
      - run: cargo tarpaulin --out Xml --workspace
      - uses: codecov/codecov-action@v3

  stress-tests:
    runs-on: ubuntu-latest
    if: github.event_name == 'schedule'  # Nightly only
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace --test gc_stress_tests -- --ignored
```

---

## Success Criteria

### Functional Correctness

- ✅ All 150+ opcodes tested individually
- ✅ All opcodes pass correctness tests
- ✅ GC stress tests run for 24 hours without leaks
- ✅ Context isolation is complete (no cross-contamination)
- ✅ 1000+ concurrent tasks execute correctly
- ✅ Snapshots restore identically
- ✅ Resource limits enforced accurately
- ✅ All design doc examples work

### Performance

- ✅ Opcode execution within target ranges
- ✅ GC latency <5ms for typical collections
- ✅ Task spawn/await <100μs
- ✅ Context creation <500μs
- ✅ Snapshot/restore <500ms for 10MB heap
- ✅ No performance regressions detected

### Code Coverage

- ✅ >90% line coverage for raya-core
- ✅ >85% coverage for GC subsystem
- ✅ >95% coverage for critical paths (opcode execution, task scheduling)
- ✅ 100% coverage for public APIs

### Quality Metrics

- ✅ Zero compiler warnings
- ✅ Zero Clippy warnings with `-D warnings`
- ✅ All tests passing on Linux, macOS, Windows
- ✅ No flaky tests
- ✅ No known memory leaks
- ✅ No data races (verified with ThreadSanitizer)

---

## Dependencies

### External Crates

```toml
[dev-dependencies]
criterion = "0.5"          # Benchmarking framework
tarpaulin = "0.27"         # Code coverage
tempfile = "3.8"           # Temporary files for snapshot tests
proptest = "1.4"           # Property-based testing (optional)
```

### Internal Milestones

- Milestone 1.2: Bytecode Definitions ✅
- Milestone 1.3: Value System ✅
- Milestone 1.4: Stack Machine ✅
- Milestone 1.5: Garbage Collector ✅
- Milestone 1.6: Object Model ✅
- Milestone 1.7: Type System ✅
- Milestone 1.8: VM Interpreter ✅
- Milestone 1.9: JSON Support ✅
- Milestone 1.10: Task Scheduler ✅
- Milestone 1.11: VM Snapshotting ✅
- Milestone 1.12: Synchronization Primitives ✅
- Milestone 1.13: Inner VMs & Controllability ✅

---

## Documentation

### Test Documentation

Each test file should include:

```rust
//! Opcode Execution Tests
//!
//! This module contains comprehensive tests for all VM opcodes.
//! Tests are organized by opcode category and validate:
//! - Correctness of individual opcodes
//! - Edge cases and error handling
//! - Interaction between opcodes
//!
//! # Test Structure
//! - Each opcode has at least one dedicated test
//! - Composite tests validate opcode sequences
//! - Error tests verify proper error handling
//!
//! # Running Tests
//! ```bash
//! cargo test --test opcode_tests
//! ```
```

### Coverage Reports

Generate and review coverage:

```bash
# HTML report
cargo tarpaulin --out Html --workspace
open tarpaulin-report.html

# Console output
cargo tarpaulin --workspace

# CI-friendly output
cargo tarpaulin --out Xml --workspace
```

### Benchmark Reports

```bash
# Run benchmarks
cargo bench

# View reports
open target/criterion/report/index.html

# Compare with baseline
cargo bench --bench vm_benchmarks -- --save-baseline main
# ... make changes ...
cargo bench --bench vm_benchmarks -- --baseline main
```

---

## Timeline

**Total Estimated Effort:** 2-3 weeks

**Week 1:**
- Days 1-3: Opcode test suite (Task 1)
- Days 4-5: GC stress tests (Task 2)

**Week 2:**
- Days 1-2: Context isolation tests (Task 3)
- Days 3-5: Concurrency tests (Task 4)

**Week 3:**
- Days 1-2: Snapshot and security tests (Tasks 5-6)
- Day 3: Resource limit tests (Task 7)
- Days 4-5: Performance benchmarks (Task 8)

**Week 4 (optional):**
- Days 1-2: End-to-end scenarios (Task 9)
- Day 3: Coverage analysis (Task 10)
- Day 4: CI/CD integration (Task 11)
- Day 5: Buffer for fixing issues

---

## References

### Design Documents

- [design/ARCHITECTURE.md](../design/ARCHITECTURE.md) - VM architecture
- [design/OPCODE.md](../design/OPCODE.md) - All opcode specifications
- [design/INNER_VM.md](../design/INNER_VM.md) - Inner VM examples
- [design/SNAPSHOTTING.md](../design/SNAPSHOTTING.md) - Snapshot format

### External Resources

- [Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)
- [Tarpaulin Code Coverage](https://github.com/xd009642/tarpaulin)
- [Rust Testing Guide](https://doc.rust-lang.org/book/ch11-00-testing.html)

---

**Status:** Ready for Implementation
**Created:** 2026-01-05
**Last Updated:** 2026-01-05
