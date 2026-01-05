//! Integration tests for Bytecode Module (Milestone 1.2)

use raya_bytecode::{verify_module, ConstantPool, Function, Module, Opcode};

#[test]
fn test_create_and_encode_module() {
    let mut module = Module::new("test_module".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8],
    });

    let bytes = module.encode();
    assert!(!bytes.is_empty());
    assert!(bytes.len() > 20);
}

#[test]
fn test_decode_module() {
    let mut module = Module::new("test_decode".to_string());
    module.functions.push(Function {
        name: "add".to_string(),
        param_count: 2,
        local_count: 2,
        code: vec![
            Opcode::LoadLocal as u8, 0,
            Opcode::LoadLocal as u8, 1,
            Opcode::Iadd as u8,
            Opcode::Return as u8,
        ],
    });

    let bytes = module.encode();
    let decoded = Module::decode(&bytes).expect("Failed to decode");

    assert_eq!(decoded.metadata.name, "test_decode");
    assert_eq!(decoded.functions.len(), 1);
    assert_eq!(decoded.functions[0].name, "add");
}

#[test]
fn test_roundtrip_encoding() {
    let mut module = Module::new("roundtrip_test".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            Opcode::ConstI32 as u8, 10, 0, 0, 0,
            Opcode::StoreLocal as u8, 0,
            Opcode::LoadLocal as u8, 0,
            Opcode::Return as u8,
        ],
    });

    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();

    assert_eq!(decoded.metadata.name, module.metadata.name);
    assert_eq!(decoded.functions.len(), module.functions.len());
}

#[test]
fn test_verify_valid_module() {
    let mut module = Module::new("valid".to_string());
    module.functions.push(Function {
        name: "test".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 100, 0, 0, 0, Opcode::Return as u8],
    });

    verify_module(&module).expect("Should be valid");
}

#[test]
fn test_verify_invalid_no_return() {
    let mut module = Module::new("invalid".to_string());
    module.functions.push(Function {
        name: "bad".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Pop as u8],
    });

    assert!(verify_module(&module).is_err());
}

#[test]
fn test_module_with_constants() {
    let mut module = Module::new("with_const".to_string());
    let str_idx = module.constants.add_string("Hello".to_string());
    
    module.functions.push(Function {
        name: "get_str".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![
            Opcode::ConstStr as u8,
            str_idx as u8,
            (str_idx >> 8) as u8,
            Opcode::Return as u8,
        ],
    });

    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();

    assert_eq!(decoded.constants.get_string(str_idx), Some("Hello"));
}

#[test]
fn test_constant_pool_types() {
    let mut pool = ConstantPool::new();
    
    let s = pool.add_string("test".to_string());
    let i = pool.add_integer(42);
    let f = pool.add_float(3.14);
    
    assert_eq!(pool.get_string(s), Some("test"));
    assert_eq!(pool.get_integer(i), Some(42));
    assert_eq!(pool.get_float(f), Some(3.14));
}

#[test]
fn test_empty_module() {
    let module = Module::new("empty".to_string());
    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();
    
    assert_eq!(decoded.metadata.name, "empty");
    assert_eq!(decoded.functions.len(), 0);
}

#[test]
fn test_many_functions() {
    let mut module = Module::new("many".to_string());
    
    for i in 0..50 {
        module.functions.push(Function {
            name: format!("fn_{}", i),
            param_count: 0,
            local_count: 0,
            code: vec![Opcode::ConstI32 as u8, i as u8, 0, 0, 0, Opcode::Return as u8],
        });
    }
    
    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();
    
    assert_eq!(decoded.functions.len(), 50);
}

#[test]
fn test_large_function() {
    let mut module = Module::new("large".to_string());
    let mut code = Vec::new();
    
    for i in 0..250 {
        code.extend_from_slice(&[Opcode::ConstI32 as u8, i as u8, 0, 0, 0, Opcode::Pop as u8]);
    }
    code.extend_from_slice(&[Opcode::ConstI32 as u8, 42, 0, 0, 0, Opcode::Return as u8]);
    
    module.functions.push(Function {
        name: "large".to_string(),
        param_count: 0,
        local_count: 0,
        code,
    });
    
    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();
    
    assert!(decoded.functions[0].code.len() > 1000);
}

#[test]
fn test_invalid_magic() {
    let bytes = vec![0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 1];
    assert!(Module::decode(&bytes).is_err());
}

#[test]
fn test_arithmetic_function() {
    let mut module = Module::new("arith".to_string());
    module.functions.push(Function {
        name: "compute".to_string(),
        param_count: 2,
        local_count: 2,
        code: vec![
            Opcode::LoadLocal as u8, 0,
            Opcode::LoadLocal as u8, 1,
            Opcode::Iadd as u8,
            Opcode::ConstI32 as u8, 2, 0, 0, 0,
            Opcode::Imul as u8,
            Opcode::Return as u8,
        ],
    });
    
    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();
    
    assert_eq!(decoded.functions[0].name, "compute");
}

#[test]
fn test_control_flow() {
    let mut module = Module::new("control".to_string());
    // Simple function with comparison
    module.functions.push(Function {
        name: "compare".to_string(),
        param_count: 2,
        local_count: 2,
        code: vec![
            Opcode::LoadLocal as u8, 0,
            Opcode::LoadLocal as u8, 1,
            Opcode::Igt as u8,
            Opcode::Return as u8,
        ],
    });

    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();

    // Verify round-trip works
    assert_eq!(decoded.functions.len(), 1);
    assert_eq!(decoded.functions[0].name, "compare");
}

#[test]
fn test_constant_pool_persistence() {
    let mut module = Module::new("const_persist".to_string());
    
    let s1 = module.constants.add_string("hello".to_string());
    let s2 = module.constants.add_string("world".to_string());
    let i1 = module.constants.add_integer(100);
    let f1 = module.constants.add_float(2.5);
    
    module.functions.push(Function {
        name: "dummy".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstNull as u8, Opcode::Return as u8],
    });
    
    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();
    
    assert_eq!(decoded.constants.get_string(s1), Some("hello"));
    assert_eq!(decoded.constants.get_string(s2), Some("world"));
    assert_eq!(decoded.constants.get_integer(i1), Some(100));
    assert_eq!(decoded.constants.get_float(f1), Some(2.5));
}

#[test]
fn test_module_checksum() {
    let mut module = Module::new("checksum".to_string());
    module.functions.push(Function {
        name: "test".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ConstI32 as u8, 1, 0, 0, 0, Opcode::Return as u8],
    });
    
    let bytes = module.encode();
    Module::decode(&bytes).expect("Checksum should validate");
}

#[test]
fn test_empty_function() {
    let mut module = Module::new("empty_fn".to_string());
    module.functions.push(Function {
        name: "noop".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![Opcode::ReturnVoid as u8],
    });
    
    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();
    
    assert_eq!(decoded.functions[0].code.len(), 1);
}

#[test]
fn test_many_locals() {
    let mut module = Module::new("many_locals".to_string());
    module.functions.push(Function {
        name: "lots_of_locals".to_string(),
        param_count: 0,
        local_count: 100,
        code: vec![
            Opcode::ConstI32 as u8, 42, 0, 0, 0,
            Opcode::StoreLocal as u8, 99,
            Opcode::LoadLocal as u8, 99,
            Opcode::Return as u8,
        ],
    });
    
    let bytes = module.encode();
    let decoded = Module::decode(&bytes).unwrap();
    
    assert_eq!(decoded.functions[0].local_count, 100);
}
