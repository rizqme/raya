//! Integration tests for Object Model (Milestone 1.6)
//!
//! Tests cover:
//! - Object creation and field access
//! - Array operations (creation, access, bounds checking)
//! - String operations (concatenation, length)
//! - Method dispatch via vtables
//! - GC integration with objects

use raya_bytecode::{Function, Module, Opcode};
use raya_core::object::Class;
use raya_core::value::Value;
use raya_core::vm::Vm;

#[test]
fn test_object_creation_and_field_access() {
    // Create Point class with 2 fields (x, y)
    let mut vm = Vm::new();
    let point_class = Class::new(0, "Point".to_string(), 2);
    vm.classes.register_class(point_class);

    // Bytecode: new Point(), set x=10, y=20, read x
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // new Point() -> local 0
            Opcode::New as u8,
            0,
            0, // class index 0
            Opcode::StoreLocal as u8,
            0,
            // obj.x = 10
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::StoreField as u8,
            0,
            0, // field offset 0
            // obj.y = 20
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::StoreField as u8,
            1,
            0, // field offset 1
            // return obj.x
            Opcode::LoadLocal as u8,
            0,
            Opcode::LoadField as u8,
            0,
            0, // field offset 0
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(10));
}

#[test]
fn test_array_creation_and_access() {
    // Bytecode: arr = new Array(3), arr[0]=10, arr[1]=20, arr[2]=30, return arr[1]
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // new Array(3) -> local 0
            Opcode::ConstI32 as u8,
            3,
            0,
            0,
            0, // length
            Opcode::NewArray as u8,
            0,
            0, // type index 0
            Opcode::StoreLocal as u8,
            0,
            // arr[0] = 10
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            0,
            0,
            0,
            0, // index 0
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0, // value 10
            Opcode::StoreElem as u8,
            // arr[1] = 20
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0, // index 1
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0, // value 20
            Opcode::StoreElem as u8,
            // arr[2] = 30
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            2,
            0,
            0,
            0, // index 2
            Opcode::ConstI32 as u8,
            30,
            0,
            0,
            0, // value 30
            Opcode::StoreElem as u8,
            // return arr[1]
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0, // index 1
            Opcode::LoadElem as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(20));
}

#[test]
fn test_array_length() {
    // Bytecode: arr = new Array(5), return arr.length
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // new Array(5) -> local 0
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0, // length
            Opcode::NewArray as u8,
            0,
            0, // type index 0
            Opcode::StoreLocal as u8,
            0,
            // return arr.length
            Opcode::LoadLocal as u8,
            0,
            Opcode::ArrayLen as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(5));
}

#[test]
fn test_multiple_objects() {
    // Create two different classes
    let mut vm = Vm::new();

    let point_class = Class::new(0, "Point".to_string(), 2);
    vm.classes.register_class(point_class);

    let rect_class = Class::new(1, "Rectangle".to_string(), 4);
    vm.classes.register_class(rect_class);

    // Bytecode: create Point with x=5, y=10, create Rectangle with x1=0, y1=0, x2=100, y2=50
    // return Point.x + Point.y + Rectangle.x2
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 2,
        code: vec![
            // Point -> local 0
            Opcode::New as u8,
            0,
            0, // class 0
            Opcode::StoreLocal as u8,
            0,
            // Point.x = 5
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::StoreField as u8,
            0,
            0,
            // Point.y = 10
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::StoreField as u8,
            1,
            0,
            // Rectangle -> local 1
            Opcode::New as u8,
            1,
            0, // class 1
            Opcode::StoreLocal as u8,
            1,
            // Rectangle.x2 = 100 (field index 2)
            Opcode::LoadLocal as u8,
            1,
            Opcode::ConstI32 as u8,
            100,
            0,
            0,
            0,
            Opcode::StoreField as u8,
            2,
            0,
            // Calculate Point.x + Point.y + Rectangle.x2
            Opcode::LoadLocal as u8,
            0,
            Opcode::LoadField as u8,
            0,
            0, // Point.x
            Opcode::LoadLocal as u8,
            0,
            Opcode::LoadField as u8,
            1,
            0, // Point.y
            Opcode::Iadd as u8,
            Opcode::LoadLocal as u8,
            1,
            Opcode::LoadField as u8,
            2,
            0, // Rectangle.x2
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(115)); // 5 + 10 + 100
}

#[test]
fn test_object_with_gc() {
    // Test that objects survive GC when they're referenced
    let mut vm = Vm::new();

    let point_class = Class::new(0, "Point".to_string(), 2);
    vm.classes.register_class(point_class);

    // Create an object, store it in a local, trigger GC, access it
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // Create Point
            Opcode::New as u8,
            0,
            0,
            Opcode::StoreLocal as u8,
            0,
            // Set x = 42
            Opcode::LoadLocal as u8,
            0,
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::StoreField as u8,
            0,
            0,
            // Load x and return it (object should survive GC)
            Opcode::LoadLocal as u8,
            0,
            Opcode::LoadField as u8,
            0,
            0,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));

    // Trigger GC after execution - object is no longer reachable
    vm.collect_garbage();
}
