//! Integration tests for Object Model (Milestone 1.6)
//!
//! Tests cover:
//! - Object creation and field access
//! - Array operations (creation, access, bounds checking)
//! - String operations (concatenation, length)
//! - Method dispatch via vtables
//! - GC integration with objects

use raya_engine::compiler::{ClassDef, Function, Module, Opcode};
use raya_engine::vm::interpreter::Vm;
use raya_engine::vm::object::layout_id_from_ordered_names;
use raya_engine::vm::value::Value;
use std::sync::Arc;

fn class_def(name: &str, field_count: usize, parent_id: Option<u32>) -> ClassDef {
    ClassDef {
        name: name.to_string(),
        field_count,
        parent_id,
        methods: Vec::new(),
        ..Default::default()
    }
}

#[test]
fn test_object_creation_and_field_access() {
    let mut vm = Vm::new();

    // Bytecode: new Point(), set x=10, y=20, read x
    let mut module = Module::new("test".to_string());
    module.classes.push(class_def("Point", 2, None));
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // new Point() -> local 0
            Opcode::NewType as u8,
            0,
            0, // class index 0
            Opcode::StoreLocal as u8,
            0,
            0,
            // obj.x = 10
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::StoreFieldExact as u8,
            0,
            0, // field offset 0
            // obj.y = 20
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::StoreFieldExact as u8,
            1,
            0, // field offset 1
            // return obj.x
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::LoadFieldExact as u8,
            0,
            0, // field offset 0
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
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
            0,
            0,
            Opcode::StoreLocal as u8,
            0,
            0,
            // arr[0] = 10
            Opcode::LoadLocal as u8,
            0,
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
            0,
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0, // index 1
            Opcode::LoadElem as u8,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
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
            0,
            0,
            Opcode::StoreLocal as u8,
            0,
            0,
            // return arr.length
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ArrayLen as u8,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(5));
}

#[test]
fn test_multiple_objects() {
    let mut vm = Vm::new();

    // Bytecode: create Point with x=5, y=10, create Rectangle with x1=0, y1=0, x2=100, y2=50
    // return Point.x + Point.y + Rectangle.x2
    let mut module = Module::new("test".to_string());
    module.classes.push(class_def("Point", 2, None));
    module.classes.push(class_def("Rectangle", 4, None));
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 2,
        code: vec![
            // Point -> local 0
            Opcode::NewType as u8,
            0,
            0, // class 0
            Opcode::StoreLocal as u8,
            0,
            0,
            // Point.x = 5
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0,
            Opcode::StoreFieldExact as u8,
            0,
            0,
            // Point.y = 10
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::StoreFieldExact as u8,
            1,
            0,
            // Rectangle -> local 1
            Opcode::NewType as u8,
            1,
            0, // class 1
            Opcode::StoreLocal as u8,
            1,
            0,
            // Rectangle.x2 = 100 (field index 2)
            Opcode::LoadLocal as u8,
            1,
            0,
            Opcode::ConstI32 as u8,
            100,
            0,
            0,
            0,
            Opcode::StoreFieldExact as u8,
            2,
            0,
            // Calculate Point.x + Point.y + Rectangle.x2
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::LoadFieldExact as u8,
            0,
            0, // Point.x
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::LoadFieldExact as u8,
            1,
            0, // Point.y
            Opcode::Iadd as u8,
            Opcode::LoadLocal as u8,
            1,
            0,
            Opcode::LoadFieldExact as u8,
            2,
            0, // Rectangle.x2
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(115)); // 5 + 10 + 100
}

#[test]
fn test_object_with_gc() {
    // Test that objects survive GC when they're referenced
    let mut vm = Vm::new();

    // Create an object, store it in a local, trigger GC, access it
    let mut module = Module::new("test".to_string());
    module.classes.push(class_def("Point", 2, None));
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // Create Point
            Opcode::NewType as u8,
            0,
            0,
            Opcode::StoreLocal as u8,
            0,
            0,
            // Set x = 42
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::StoreFieldExact as u8,
            0,
            0,
            // Load x and return it (object should survive GC)
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::LoadFieldExact as u8,
            0,
            0,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));

    // Trigger GC after execution - object is no longer reachable
    vm.collect_garbage();
}

#[test]
fn test_object_literal() {
    // Test OBJECT_LITERAL + INIT_OBJECT opcodes
    // Creates Point{x: 10, y: 20} using literal syntax
    let mut vm = Vm::new();
    let layout_id = layout_id_from_ordered_names(&["x".to_string(), "y".to_string()]);
    let mut module = Module::new("test".to_string());
    let mut code = vec![Opcode::ObjectLiteral as u8];
    code.extend_from_slice(&layout_id.to_le_bytes());
    code.extend_from_slice(&2u16.to_le_bytes());
    code.extend_from_slice(&[Opcode::ConstI32 as u8, 10, 0, 0, 0]);
    code.extend_from_slice(&[Opcode::InitObject as u8, 0, 0]);
    code.extend_from_slice(&[Opcode::ConstI32 as u8, 20, 0, 0, 0]);
    code.extend_from_slice(&[Opcode::InitObject as u8, 1, 0]);
    code.extend_from_slice(&[Opcode::LoadFieldExact as u8, 0, 0, Opcode::Return as u8]);
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code,
    
    ..Default::default()};
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(10));
}

#[test]
fn test_array_literal() {
    // Test ARRAY_LITERAL opcode
    // ARRAY_LITERAL pops elements from stack and creates array
    // So we push elements first: [10, 20, 30]
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // Push elements in order (first element first)
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            Opcode::ConstI32 as u8,
            30,
            0,
            0,
            0,
            // ARRAY_LITERAL type=0, length=3
            // Pops 3 elements, creates array [10, 20, 30]
            Opcode::ArrayLiteral as u8,
            0,
            0,
            0,
            0, // type index 0 (u32)
            3,
            0,
            0,
            0, // length 3 (u32)
            // Array is now on stack with all elements set
            // Read element 1 to verify (should be 20)
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
            Opcode::LoadElem as u8,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(20));
}

#[test]
fn test_static_fields() {
    // Test LOAD_STATIC + STORE_STATIC opcodes
    let mut vm = Vm::new();

    let mut module = Module::new("test".to_string());
    module.classes.push(class_def("Counter", 2, None));
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // Load static field 1 (initial value 100)
            Opcode::LoadStatic as u8,
            0,
            0, // class index 0
            1,
            0, // field offset 1
            // Add 42
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::Iadd as u8,
            // Store back to static field 0
            Opcode::StoreStatic as u8,
            0,
            0, // class index 0
            0,
            0, // field offset 0
            // Load static field 0 to verify
            Opcode::LoadStatic as u8,
            0,
            0, // class index 0
            0,
            0, // field offset 0
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    let runtime_module = Arc::new(Module::decode(&module.encode()).unwrap());
    vm.shared_state()
        .register_module(runtime_module.clone())
        .unwrap();
    let nominal_type_base = vm
        .shared_state()
        .module_layouts
        .read()
        .get(&runtime_module.checksum)
        .unwrap()
        .nominal_type_base;
    vm.shared_state()
        .classes
        .write()
        .get_class_mut(nominal_type_base)
        .unwrap()
        .static_fields = vec![Value::i32(0), Value::i32(100)];

    let result = vm.execute(runtime_module.as_ref()).unwrap();
    assert_eq!(result, Value::i32(142)); // 100 + 42
}

#[test]
fn test_optional_field_non_null() {
    // Test OPTIONAL_FIELD opcode with non-null object
    let mut vm = Vm::new();
    let mut module = Module::new("test".to_string());
    module.classes.push(class_def("Point", 2, None));
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // Create Point
            Opcode::NewType as u8,
            0,
            0,
            Opcode::StoreLocal as u8,
            0,
            0,
            // Set x = 42
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::StoreFieldExact as u8,
            0,
            0,
            // Load object and access optional field
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::OptionalFieldExact as u8,
            0,
            0, // field offset 0
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_optional_field_null() {
    // Test OPTIONAL_FIELD opcode with null object
    let mut vm = Vm::new();
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // Push null
            Opcode::ConstNull as u8,
            // Access optional field (should return null)
            Opcode::OptionalFieldExact as u8,
            0,
            0, // field offset 0
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::null());
}

#[test]
fn test_constructor_no_args() {
    // Test CALL_CONSTRUCTOR with no arguments
    let mut vm = Vm::new();

    let mut module = Module::new("test".to_string());
    module.classes.push(class_def("Point", 2, None));

    // Main function: calls constructor with no args
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // CALL_CONSTRUCTOR class=0, arg_count=0
            Opcode::CallConstructor as u8,
            0,
            0,
            0,
            0, // class index 0
            0,
            0, // arg count 0
            // Store returned object
            Opcode::StoreLocal as u8,
            0,
            0,
            // Load object and set field directly
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::StoreFieldExact as u8,
            0,
            0,
            // Load and return field 0 to verify
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::LoadFieldExact as u8,
            0,
            0,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    // Empty constructor
    let constructor_fn = Function {
        name: "Point::constructor".to_string(),
        param_count: 1, // just this
        local_count: 1, // total locals = 1 (this only)
        code: vec![
            // Just return null
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(constructor_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_constructor_basic() {
    // Test CALL_CONSTRUCTOR opcode
    let mut vm = Vm::new();

    let mut module = Module::new("test".to_string());
    module.classes.push(class_def("Point", 2, None));

    // Main function: calls constructor with args 10, 20
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // Push constructor arguments
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0, // arg 0 (x)
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0, // arg 1 (y)
            // CALL_CONSTRUCTOR class=0, arg_count=2
            Opcode::CallConstructor as u8,
            0,
            0,
            0,
            0, // class index 0
            2,
            0, // arg count 2
            // Store returned object
            Opcode::StoreLocal as u8,
            0,
            0,
            // Load and return field 0 to verify
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::LoadFieldExact as u8,
            0,
            0,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    // Constructor function: initializes fields from args
    let constructor_fn = Function {
        name: "Point::constructor".to_string(),
        param_count: 3, // this + 2 args
        local_count: 3, // total locals = 3 (this + x + y)
        code: vec![
            // Load 'this' (param 0)
            Opcode::LoadLocal as u8,
            0,
            0,
            // Load x (param 1)
            Opcode::LoadLocal as u8,
            1,
            0,
            // Set this.x = x
            Opcode::StoreFieldExact as u8,
            0,
            0,
            // Load 'this'
            Opcode::LoadLocal as u8,
            0,
            0,
            // Load y (param 2)
            Opcode::LoadLocal as u8,
            2,
            0,
            // Set this.y = y
            Opcode::StoreFieldExact as u8,
            1,
            0,
            // Return null (constructor doesn't return value)
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(constructor_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(10));
}

#[test]
fn test_call_super() {
    // Test CALL_SUPER opcode (calling parent constructor)
    let mut vm = Vm::new();

    let mut module = Module::new("test".to_string());
    module.classes.push(class_def("Shape", 1, None));
    module.classes.push(class_def("Circle", 2, Some(0)));

    // Main function: creates Circle(5, "red")
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // Push constructor arguments (radius, color)
            Opcode::ConstI32 as u8,
            5,
            0,
            0,
            0, // radius
            Opcode::ConstI32 as u8,
            1, // Simplified: use 1 for "red"
            0,
            0,
            0,
            // CALL_CONSTRUCTOR class=1 (Circle), arg_count=2
            Opcode::CallConstructor as u8,
            1,
            0,
            0,
            0,
            2,
            0,
            // Store object
            Opcode::StoreLocal as u8,
            0,
            0,
            // Return field 1 (radius) to verify
            Opcode::LoadLocal as u8,
            0,
            0,
            Opcode::LoadFieldExact as u8,
            1,
            0,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(main_fn);

    // Shape constructor: sets color
    let shape_constructor = Function {
        name: "Shape::constructor".to_string(),
        param_count: 2, // this + color
        local_count: 2, // total locals = 2 (this + color)
        code: vec![
            // this.color = color
            Opcode::LoadLocal as u8,
            0,
            0, // this
            Opcode::LoadLocal as u8,
            1,
            0, // color
            Opcode::StoreFieldExact as u8,
            0,
            0, // field 0 (color)
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(shape_constructor);

    // Circle constructor: calls super, then sets radius
    let circle_constructor = Function {
        name: "Circle::constructor".to_string(),
        param_count: 3, // this + radius + color
        local_count: 3, // total locals = 3 (this + radius + color)
        code: vec![
            // Call super(color) - CALL_SUPER needs 'this' + args on stack
            Opcode::LoadLocal as u8,
            0,
            0, // this
            Opcode::LoadLocal as u8,
            2,
            0, // color
            // CALL_SUPER class=1 (Circle, which has Shape as parent), arg_count=1
            Opcode::CallSuper as u8,
            1,
            0,
            0,
            0, // current class 1 (Circle)
            1,
            0, // arg count 1 (just color)
            // Now set radius (field 1)
            Opcode::LoadLocal as u8,
            0,
            0, // this
            Opcode::LoadLocal as u8,
            1,
            0, // radius
            Opcode::StoreFieldExact as u8,
            1,
            0, // field 1 (radius)
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ],
    
    ..Default::default()};
    module.functions.push(circle_constructor);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(5)); // radius
}
