//! Integration tests for SPAWN and AWAIT opcodes (Milestone 1.10)

#![allow(clippy::identity_op)]

use raya_compiler::{Function, Module, Opcode};
use raya_core::value::Value;
use raya_core::vm::Vm;

/// Create a module with a simple task function that returns a value
fn create_module_with_task(task_result: i32) -> Module {
    let mut module = Module::new("test".to_string());

    // Function 0: task that returns a value
    module.functions.push(Function {
        name: "task".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstI32 as u8,
            (task_result & 0xFF) as u8,
            ((task_result >> 8) & 0xFF) as u8,
            ((task_result >> 16) & 0xFF) as u8,
            ((task_result >> 24) & 0xFF) as u8,
            Opcode::Return as u8,
        ],
    });

    // Function 1: main that spawns and awaits the task
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // Spawn task (function 0)
            Opcode::Spawn as u8,
            0,
            0, // func_index = 0 (u16)
            0,
            0, // arg_count = 0 (u16)
            // Now TaskId (u64) is on stack
            // Await the task
            Opcode::Await as u8,
            // Task result is now on stack
            Opcode::Return as u8,
        ],
    });

    module
}

/// Create a module with multiple tasks
fn create_module_with_multiple_tasks() -> Module {
    let mut module = Module::new("test".to_string());

    // Function 0: task1 returns 10
    module.functions.push(Function {
        name: "task1".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 10, 0, 0, 0, Opcode::Return as u8],
    });

    // Function 1: task2 returns 20
    module.functions.push(Function {
        name: "task2".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 20, 0, 0, 0, Opcode::Return as u8],
    });

    // Function 2: task3 returns 30
    module.functions.push(Function {
        name: "task3".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 30, 0, 0, 0, Opcode::Return as u8],
    });

    // Function 3: main spawns all three tasks and awaits them
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 3, // Store 3 TaskIds
        code: vec![
            // Spawn task1 and store TaskId in local 0
            Opcode::Spawn as u8,
            0,
            0, // func_index = 0 (u16)
            0,
            0, // arg_count = 0 (u16)
            Opcode::StoreLocal as u8,
            0,
            0,
            // Spawn task2 and store TaskId in local 1
            Opcode::Spawn as u8,
            1,
            0, // func_index = 1 (u16)
            0,
            0, // arg_count = 0 (u16)
            Opcode::StoreLocal as u8,
            1,
            0,
            // Spawn task3 and store TaskId in local 2
            Opcode::Spawn as u8,
            2,
            0, // func_index = 2 (u16)
            0,
            0, // arg_count = 0 (u16)
            Opcode::StoreLocal as u8,
            2,
            0,
            // Await task1
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::Await as u8,
            // Result 1 on stack

            // Await task2
            Opcode::LoadLocal as u8,
            1,
            0,
            Opcode::Await as u8,
            // Result 2 on stack

            // Add results 1 and 2
            Opcode::Iadd as u8,
            // Await task3
            Opcode::LoadLocal as u8,
            2,
            0,
            Opcode::Await as u8,
            // Result 3 on stack

            // Add to previous sum
            Opcode::Iadd as u8,
            // Return total (should be 10 + 20 + 30 = 60)
            Opcode::Return as u8,
        ],
    });

    module
}

/// Create a module with a compute-intensive task
fn create_module_with_compute_task(iterations: u32) -> Module {
    let mut module = Module::new("test".to_string());

    // Function 0: compute task that counts to N
    module.functions.push(Function {
        name: "compute".to_string(),
        param_count: 0,
        local_count: 2, // counter, result
        code: vec![
            // counter = 0
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            0,
            0,
            // result = 0
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0,
            Opcode::StoreLocal as u8,
            1,
            0,
            // Loop start (offset 18)
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            (iterations & 0xFF) as u8,
            ((iterations >> 8) & 0xFF) as u8,
            ((iterations >> 16) & 0xFF) as u8,
            ((iterations >> 24) & 0xFF) as u8,
            Opcode::Ilt as u8,
            Opcode::JmpIfFalse as u8,
            30,
            0, // Jump to end
            // result += 1
            Opcode::LoadLocal as u8,
            1,
            0,
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Iadd as u8,
            Opcode::StoreLocal as u8,
            1,
            0,
            // counter += 1
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::Iadd as u8,
            Opcode::StoreLocal as u8,
            0,
            0,
            // Jump back to loop start
            Opcode::Jmp as u8,
            (-39i16 & 0xFF) as u8,
            (((-39i16) >> 8) & 0xFF) as u8,
            // End: return result
            Opcode::LoadLocal as u8,
            1,
            0,
            Opcode::Return as u8,
        ],
    });

    // Function 1: main spawns and awaits compute task
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::Spawn as u8,
            0,
            0, // func_index = 0 (u16)
            0,
            0, // arg_count = 0 (u16)
            Opcode::Await as u8,
            Opcode::Return as u8,
        ],
    });

    module
}

#[test]
fn test_spawn_and_await_simple_task() {
    let module = create_module_with_task(42);
    let mut vm = Vm::new();

    let result = vm.execute(&module).expect("Execution failed");

    assert_eq!(result, Value::i32(42), "Expected task to return 42");
}

#[test]
fn test_spawn_and_await_multiple_tasks() {
    let module = create_module_with_multiple_tasks();
    let mut vm = Vm::new();

    let result = vm.execute(&module).expect("Execution failed");

    assert_eq!(result, Value::i32(60), "Expected sum of 10 + 20 + 30 = 60");
}

#[test]
fn test_spawn_and_await_different_values() {
    for expected in [0, 1, 100, -42, 12345] {
        let module = create_module_with_task(expected);
        let mut vm = Vm::new();

        let result = vm.execute(&module).expect("Execution failed");

        assert_eq!(
            result,
            Value::i32(expected),
            "Expected task to return {}",
            expected
        );
    }
}

#[test]
fn test_spawn_and_await_compute_task() {
    let module = create_module_with_compute_task(100);
    let mut vm = Vm::new();

    let result = vm.execute(&module).expect("Execution failed");

    assert_eq!(
        result,
        Value::i32(100),
        "Expected compute task to count to 100"
    );
}

#[test]
fn test_spawn_and_await_many_compute_tasks() {
    let module = create_module_with_compute_task(1000);
    let mut vm = Vm::new();

    let result = vm.execute(&module).expect("Execution failed");

    assert_eq!(
        result,
        Value::i32(1000),
        "Expected compute task to count to 1000"
    );
}

#[test]
fn test_concurrent_task_limit() {
    // Test that SPAWN/AWAIT work with limited concurrency
    // NOTE: This test currently uses default VM since we need scheduler limits
    // integrated into VM constructor for full testing

    // This should work fine spawning one task at a time
    let module = create_module_with_task(99);
    let mut vm = Vm::with_worker_count(1);

    let result = vm.execute(&module).expect("Execution should succeed");
    assert_eq!(result, Value::i32(99));
}

#[test]
fn test_spawn_await_with_scheduler_stress() {
    // Spawn many tasks rapidly
    let mut module = Module::new("stress_test".to_string());

    // Function 0: simple task
    module.functions.push(Function {
        name: "task".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 1, 0, 0, 0, Opcode::Return as u8],
    });

    // Function 1: main spawns 10 tasks and sums results
    let mut main_code = vec![];

    // Spawn 10 tasks and store their IDs
    for i in 0..10 {
        main_code.extend_from_slice(&[
            Opcode::Spawn as u8,
            0,
            0, // func_index = 0 (u16)
            0,
            0, // arg_count = 0 (u16)
            Opcode::StoreLocal as u8,
            i as u8,
            0,
        ]);
    }

    // Await first task
    main_code.extend_from_slice(&[Opcode::LoadLocal as u8, 0, 0, Opcode::Await as u8]);

    // Await remaining tasks and add results
    for i in 1..10 {
        main_code.extend_from_slice(&[
            Opcode::LoadLocal as u8,
            i as u8,
            0,
            Opcode::Await as u8,
            Opcode::Iadd as u8,
        ]);
    }

    main_code.push(Opcode::Return as u8);

    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 10, // Store 10 TaskIds
        code: main_code,
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).expect("Execution failed");

    assert_eq!(result, Value::i32(10), "Expected sum of 10 tasks = 10");
}

#[test]
fn test_nested_task_spawning() {
    // Create a module where spawned tasks spawn their own tasks
    let mut module = Module::new("nested_test".to_string());

    // Function 0: leaf task returns 5
    module.functions.push(Function {
        name: "leaf".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 5, 0, 0, 0, Opcode::Return as u8],
    });

    // Function 1: middle task spawns leaf and doubles result
    module.functions.push(Function {
        name: "middle".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // Spawn leaf task
            Opcode::Spawn as u8,
            0,
            0, // func_index = 0 (u16)
            0,
            0, // arg_count = 0 (u16)
            Opcode::Await as u8,
            // Double the result
            Opcode::ConstI32 as u8,
            2,
            0,
            0,
            0,
            Opcode::Imul as u8,
            Opcode::Return as u8,
        ],
    });

    // Function 2: main spawns middle task
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::Spawn as u8,
            1,
            0, // func_index = 1 (u16)
            0,
            0, // arg_count = 0 (u16)
            Opcode::Await as u8,
            Opcode::Return as u8,
        ],
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).expect("Execution failed");

    assert_eq!(result, Value::i32(10), "Expected leaf(5) * 2 = 10");
}

#[test]
fn test_spawn_await_returns_null() {
    // Test task that doesn't explicitly return a value
    let mut module = Module::new("null_test".to_string());

    // Function 0: task with no explicit return
    module.functions.push(Function {
        name: "task".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::Nop as u8, Opcode::Return as u8],
    });

    // Function 1: main
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::Spawn as u8,
            0,
            0, // func_index = 0 (u16)
            0,
            0, // arg_count = 0 (u16)
            Opcode::Await as u8,
            Opcode::Return as u8,
        ],
    });

    let mut vm = Vm::new();
    let result = vm.execute(&module).expect("Execution failed");

    assert!(result.is_null(), "Expected null return from empty task");
}
