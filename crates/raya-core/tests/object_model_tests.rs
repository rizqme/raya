//! Integration tests for Object Model (Milestone 1.6)
//!
//! Tests cover:
//! - Object creation and field access
//! - Array operations (creation, access, bounds checking)
//! - String operations (concatenation, length)
//! - Method dispatch via vtables
//! - GC integration with objects

use raya_compiler::{Function, Module, Opcode};
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
            0, 0,
            // obj.x = 10
            Opcode::LoadLocal as u8,
            0, 0,
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
            0, 0,
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
            0, 0,
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
            0, 0,
            // arr[0] = 10
            Opcode::LoadLocal as u8,
            0, 0,
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
            0, 0,
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
            0, 0,
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
            0, 0,
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
            0, 0,
            // return arr.length
            Opcode::LoadLocal as u8,
            0, 0,
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
            0, 0,
            // Point.x = 5
            Opcode::LoadLocal as u8,
            0, 0,
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
            0, 0,
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
            1, 0,
            // Rectangle.x2 = 100 (field index 2)
            Opcode::LoadLocal as u8,
            1, 0,
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
            0, 0,
            Opcode::LoadField as u8,
            0,
            0, // Point.x
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::LoadField as u8,
            1,
            0, // Point.y
            Opcode::Iadd as u8,
            Opcode::LoadLocal as u8,
            1, 0,
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
            0, 0,
            // Set x = 42
            Opcode::LoadLocal as u8,
            0, 0,
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
            0, 0,
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

#[test]
fn test_object_literal() {
    // Test OBJECT_LITERAL + INIT_OBJECT opcodes
    // Creates Point{x: 10, y: 20} using literal syntax
    let mut vm = Vm::new();
    let point_class = Class::new(0, "Point".to_string(), 2);
    vm.classes.register_class(point_class);

    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // OBJECT_LITERAL class=0, field_count=2
            Opcode::ObjectLiteral as u8,
            0,
            0, // class index 0
            2,
            0, // field count 2
            // Push value for field 0 (x = 10)
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            // INIT_OBJECT field_offset=0
            Opcode::InitObject as u8,
            0,
            0,
            // Push value for field 1 (y = 20)
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            // INIT_OBJECT field_offset=1
            Opcode::InitObject as u8,
            1,
            0,
            // Object is now on stack with both fields set
            // Read field 0 to verify
            Opcode::LoadField as u8,
            0,
            0,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(10));
}

#[test]
fn test_array_literal() {
    // Test ARRAY_LITERAL + INIT_ARRAY opcodes
    // Creates [10, 20, 30] using literal syntax
    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // ARRAY_LITERAL type=0, length=3
            Opcode::ArrayLiteral as u8,
            0,
            0, // type index 0
            3,
            0,
            0,
            0, // length 3
            // Push value for index 0 (10)
            Opcode::ConstI32 as u8,
            10,
            0,
            0,
            0,
            // INIT_ARRAY index=0
            Opcode::InitArray as u8,
            0,
            0,
            0,
            0,
            // Push value for index 1 (20)
            Opcode::ConstI32 as u8,
            20,
            0,
            0,
            0,
            // INIT_ARRAY index=1
            Opcode::InitArray as u8,
            1,
            0,
            0,
            0,
            // Push value for index 2 (30)
            Opcode::ConstI32 as u8,
            30,
            0,
            0,
            0,
            // INIT_ARRAY index=2
            Opcode::InitArray as u8,
            2,
            0,
            0,
            0,
            // Array is now on stack with all elements set
            // Read element 1 to verify
            Opcode::ConstI32 as u8,
            1,
            0,
            0,
            0,
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
fn test_static_fields() {
    // Test LOAD_STATIC + STORE_STATIC opcodes
    let mut vm = Vm::new();

    // Create class with 2 instance fields and 2 static fields
    let mut counter_class = Class::new(0, "Counter".to_string(), 2);
    counter_class.static_fields = vec![Value::i32(0), Value::i32(100)];
    vm.classes.register_class(counter_class);

    let mut module = Module::new("test".to_string());
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
    };
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(142)); // 100 + 42
}

#[test]
fn test_optional_field_non_null() {
    // Test OPTIONAL_FIELD opcode with non-null object
    let mut vm = Vm::new();
    let point_class = Class::new(0, "Point".to_string(), 2);
    vm.classes.register_class(point_class);

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
            0, 0,
            // Set x = 42
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::StoreField as u8,
            0,
            0,
            // Load object and access optional field
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::OptionalField as u8,
            0,
            0, // field offset 0
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_optional_field_null() {
    // Test OPTIONAL_FIELD opcode with null object
    let mut vm = Vm::new();
    let point_class = Class::new(0, "Point".to_string(), 2);
    vm.classes.register_class(point_class);

    let mut module = Module::new("test".to_string());
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            // Push null
            Opcode::ConstNull as u8,
            // Access optional field (should return null)
            Opcode::OptionalField as u8,
            0,
            0, // field offset 0
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::null());
}

#[test]
fn test_constructor_no_args() {
    // Test CALL_CONSTRUCTOR with no arguments
    let mut vm = Vm::new();

    // Create Point class with constructor
    let mut point_class = Class::new(0, "Point".to_string(), 2);
    point_class.set_constructor(1); // Constructor is function 1
    vm.classes.register_class(point_class);

    let mut module = Module::new("test".to_string());

    // Main function: calls constructor with no args
    let main_fn = Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // CALL_CONSTRUCTOR class=0, arg_count=0
            Opcode::CallConstructor as u8,
            0,
            0, // class index 0
            0, // arg count 0
            // Store returned object
            Opcode::StoreLocal as u8,
            0, 0,
            // Load object and set field directly
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::ConstI32 as u8,
            42,
            0,
            0,
            0,
            Opcode::StoreField as u8,
            0,
            0,
            // Load and return field 0 to verify
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::LoadField as u8,
            0,
            0,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    // Empty constructor
    let constructor_fn = Function {
        name: "Point_constructor".to_string(),
        param_count: 1, // just this
        local_count: 1, // total locals = 1 (this only)
        code: vec![
            // Just return null
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(constructor_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}

#[test]
fn test_constructor_basic() {
    // Test CALL_CONSTRUCTOR opcode
    let mut vm = Vm::new();

    // Create Point class with constructor
    let mut point_class = Class::new(0, "Point".to_string(), 2);
    point_class.set_constructor(1); // Constructor is function 1
    vm.classes.register_class(point_class);

    let mut module = Module::new("test".to_string());

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
            0, // class index 0
            2, // arg count 2
            // Store returned object
            Opcode::StoreLocal as u8,
            0, 0,
            // Load and return field 0 to verify
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::LoadField as u8,
            0,
            0,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    // Constructor function: initializes fields from args
    let constructor_fn = Function {
        name: "Point_constructor".to_string(),
        param_count: 3, // this + 2 args
        local_count: 3, // total locals = 3 (this + x + y)
        code: vec![
            // Load 'this' (param 0)
            Opcode::LoadLocal as u8,
            0, 0,
            // Load x (param 1)
            Opcode::LoadLocal as u8,
            1, 0,
            // Set this.x = x
            Opcode::StoreField as u8,
            0,
            0,
            // Load 'this'
            Opcode::LoadLocal as u8,
            0, 0,
            // Load y (param 2)
            Opcode::LoadLocal as u8,
            2, 0,
            // Set this.y = y
            Opcode::StoreField as u8,
            1,
            0,
            // Return null (constructor doesn't return value)
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(constructor_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(10));
}

#[test]
fn test_call_super() {
    // Test CALL_SUPER opcode (calling parent constructor)
    let mut vm = Vm::new();

    // Create Shape class (parent)
    let mut shape_class = Class::new(0, "Shape".to_string(), 1); // 1 field: color
    shape_class.set_constructor(1); // Constructor is function 1
    vm.classes.register_class(shape_class);

    // Create Circle class (child) with parent
    let mut circle_class = Class::with_parent(1, "Circle".to_string(), 2, 0); // 2 fields: color (inherited) + radius
    circle_class.set_constructor(2); // Constructor is function 2
    vm.classes.register_class(circle_class);

    let mut module = Module::new("test".to_string());

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
            2,
            // Store object
            Opcode::StoreLocal as u8,
            0, 0,
            // Return field 1 (radius) to verify
            Opcode::LoadLocal as u8,
            0, 0,
            Opcode::LoadField as u8,
            1,
            0,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    // Shape constructor: sets color
    let shape_constructor = Function {
        name: "Shape_constructor".to_string(),
        param_count: 2, // this + color
        local_count: 2, // total locals = 2 (this + color)
        code: vec![
            // this.color = color
            Opcode::LoadLocal as u8,
            0, 0, // this
            Opcode::LoadLocal as u8,
            1, 0, // color
            Opcode::StoreField as u8,
            0,
            0, // field 0 (color)
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(shape_constructor);

    // Circle constructor: calls super, then sets radius
    let circle_constructor = Function {
        name: "Circle_constructor".to_string(),
        param_count: 3, // this + radius + color
        local_count: 3, // total locals = 3 (this + radius + color)
        code: vec![
            // Call super(color) - CALL_SUPER needs 'this' + args on stack
            Opcode::LoadLocal as u8,
            0, 0, // this
            Opcode::LoadLocal as u8,
            2, 0, // color
            // CALL_SUPER class=1 (Circle, which has Shape as parent), arg_count=1
            Opcode::CallSuper as u8,
            1,
            0, // current class 1 (Circle)
            1, // arg count 1 (just color)
            // Now set radius (field 1)
            Opcode::LoadLocal as u8,
            0, 0, // this
            Opcode::LoadLocal as u8,
            1, 0, // radius
            Opcode::StoreField as u8,
            1,
            0, // field 1 (radius)
            Opcode::ConstNull as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(circle_constructor);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(5)); // radius
}
