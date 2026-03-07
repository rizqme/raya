//! Integration tests for module linking
//!
//! Tests the ModuleLinker for resolving imports to exports.

use raya_engine::compiler::{
    module_id_from_name, ConstantPool, Export, Function, Import, Metadata, Module, ModuleId,
    SymbolScope, SymbolType,
};
use raya_engine::vm::module::{LinkError, ModuleLinker};
use std::sync::Arc;

fn create_module(name: &str) -> Module {
    Module {
        magic: *b"RAYA",
        version: 1,
        flags: 0,
        constants: ConstantPool::new(),
        functions: vec![],
        classes: vec![],
        metadata: Metadata {
            name: name.to_string(),
            source_file: Some(format!("{}.raya", name)),
            generic_templates: vec![],
            template_symbol_table: vec![],
            mono_debug_map: vec![],
                structural_shapes: vec![],
        },
        exports: vec![],
        imports: vec![],
        checksum: [0; 32],
        reflection: None,
        debug_info: None,
        native_functions: vec![],
        jit_hints: vec![],
    }
}

fn test_symbol_id(module_id: ModuleId, symbol: &str, index: usize) -> u64 {
    let mut acc = module_id ^ ((index as u64) << 32) ^ 0x9E37_79B9_7F4A_7C15;
    for b in symbol.bytes() {
        acc = acc.rotate_left(5) ^ u64::from(b);
    }
    acc
}

fn make_export(module_name: &str, symbol: &str, index: usize) -> Export {
    let module_id = module_id_from_name(module_name);
    let symbol_id = test_symbol_id(module_id, symbol, index);
    Export {
        name: symbol.to_string(),
        symbol_type: SymbolType::Function,
        index,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: symbol_id ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
    }
}

fn make_import(module_specifier: &str, symbol: &str, index: usize) -> Import {
    let module_name = if let Some(stripped) = module_specifier.strip_prefix('@') {
        if let Some(at_pos) = stripped.find('@') {
            &module_specifier[..at_pos + 1]
        } else {
            module_specifier
        }
    } else if let Some(at_pos) = module_specifier.find('@') {
        &module_specifier[..at_pos]
    } else {
        module_specifier
    };
    let module_id = module_id_from_name(module_name);
    let symbol_id = test_symbol_id(module_id, symbol, index);
    Import {
        module_specifier: module_specifier.to_string(),
        symbol: symbol.to_string(),
        alias: None,
        module_id,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: symbol_id ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
        runtime_global_slot: None,
    }
}

#[test]
fn test_link_simple_import() {
    let mut linker = ModuleLinker::new();

    // Create a logging module that exports an "info" function
    let mut logging = create_module("logging");
    logging.functions.push(Function {
        name: "info".to_string(),
        param_count: 1,
        local_count: 0,
        code: vec![],
    });
    logging.exports.push(make_export("logging", "info", 0));

    linker.add_module(Arc::new(logging)).unwrap();

    // Create a main module that imports logging.info
    let import = make_import("logging", "info", 0);

    let resolved = linker.resolve_import(&import, "main").unwrap();
    assert_eq!(resolved.export.name, "info");
    assert_eq!(resolved.export.symbol_type, SymbolType::Function);
    assert_eq!(resolved.index, 0);
}

#[test]
fn test_link_with_version_specifier() {
    let mut linker = ModuleLinker::new();

    // Create a module
    let mut utils = create_module("utils");
    utils.functions.push(Function {
        name: "helper".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });
    utils.exports.push(make_export("utils", "helper", 0));

    linker.add_module(Arc::new(utils)).unwrap();

    // Import with version specifier (linker strips version for now)
    let import = make_import("utils@1.2.3", "helper", 0);

    let resolved = linker.resolve_import(&import, "main").unwrap();
    assert_eq!(resolved.export.name, "helper");
}

#[test]
fn test_link_scoped_package() {
    let mut linker = ModuleLinker::new();

    // Create a scoped package
    let mut org_package = create_module("@org/package");
    org_package.functions.push(Function {
        name: "doWork".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });
    org_package
        .exports
        .push(make_export("@org/package", "doWork", 0));

    linker.add_module(Arc::new(org_package)).unwrap();

    // Import from scoped package with version
    let import = make_import("@org/package@^2.0.0", "doWork", 0);

    let resolved = linker.resolve_import(&import, "main").unwrap();
    assert_eq!(resolved.export.name, "doWork");
    assert_eq!(resolved.module.metadata.name, "@org/package");
}

#[test]
fn test_link_module_not_found() {
    let linker = ModuleLinker::new();

    let import = make_import("missing", "foo", 0);

    let result = linker.resolve_import(&import, "main");
    assert!(matches!(result, Err(LinkError::ModuleIdNotFound(_))));
}

#[test]
fn test_link_symbol_not_found() {
    let mut linker = ModuleLinker::new();

    let logging = create_module("logging");
    linker.add_module(Arc::new(logging)).unwrap();

    let import = make_import("logging", "debug", 0); // Not exported

    let result = linker.resolve_import(&import, "main");
    assert!(matches!(result, Err(LinkError::SymbolNotFound { .. })));
}

#[test]
fn test_link_multiple_imports() {
    let mut linker = ModuleLinker::new();

    // Create utils module with two exports
    let mut utils = create_module("utils");
    utils.functions.push(Function {
        name: "add".to_string(),
        param_count: 2,
        local_count: 0,
        code: vec![],
    });
    utils.functions.push(Function {
        name: "multiply".to_string(),
        param_count: 2,
        local_count: 0,
        code: vec![],
    });
    utils.exports.push(make_export("utils", "add", 0));
    utils.exports.push(make_export("utils", "multiply", 1));

    linker.add_module(Arc::new(utils)).unwrap();

    // Create main module with multiple imports
    let mut main = create_module("main");
    main.imports.push(make_import("utils", "add", 0));
    main.imports.push(make_import("utils", "multiply", 1));

    let resolved = linker.link_module(&main).unwrap();
    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0].export.name, "add");
    assert_eq!(resolved[1].export.name, "multiply");
}

#[test]
fn test_link_with_alias() {
    let mut linker = ModuleLinker::new();

    let mut utils = create_module("utils");
    utils.functions.push(Function {
        name: "log".to_string(),
        param_count: 1,
        local_count: 0,
        code: vec![],
    });
    utils.exports.push(make_export("utils", "log", 0));

    linker.add_module(Arc::new(utils)).unwrap();

    // Import with alias
    let mut import = make_import("utils", "log", 0);
    import.alias = Some("print".to_string());

    let resolved = linker.resolve_import(&import, "main").unwrap();
    assert_eq!(resolved.export.name, "log"); // Original name in export
}

#[test]
fn test_link_prefers_symbol_id_over_name() {
    let mut linker = ModuleLinker::new();

    // Two exports intentionally share the same textual name.
    let mut dup = create_module("dup_mod");
    dup.functions.push(Function {
        name: "f0".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });
    dup.functions.push(Function {
        name: "f1".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });
    let module_id = module_id_from_name("dup_mod");
    let symbol_id_0 = test_symbol_id(module_id, "dup", 0);
    let symbol_id_1 = test_symbol_id(module_id, "dup", 1);
    dup.exports.push(Export {
        name: "dup".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
        symbol_id: symbol_id_0,
        scope: SymbolScope::Module,
        signature_hash: symbol_id_0 ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
    });
    dup.exports.push(Export {
        name: "dup".to_string(),
        symbol_type: SymbolType::Function,
        index: 1,
        symbol_id: symbol_id_1,
        scope: SymbolScope::Module,
        signature_hash: symbol_id_1 ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
    });

    linker.add_module(Arc::new(dup)).unwrap();

    // Import by explicit symbol ID should resolve index 1 despite name collision.
    let import = Import {
        module_specifier: "dup_mod".to_string(),
        symbol: "dup".to_string(),
        alias: None,
        module_id,
        symbol_id: symbol_id_1,
        scope: SymbolScope::Module,
        signature_hash: symbol_id_1 ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
        runtime_global_slot: None,
    };

    let resolved = linker.resolve_import(&import, "main").unwrap();
    assert_eq!(resolved.index, 1);
}

#[test]
fn test_link_type_symbol_mismatch() {
    let mut linker = ModuleLinker::new();
    let mut typed = create_module("typed");
    typed.functions.push(Function {
        name: "fn1".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });
    let module_id = module_id_from_name("typed");
    let symbol_id = test_symbol_id(module_id, "value", 0);
    typed.exports.push(Export {
        name: "value".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: 1001,
        type_signature: Some("fn(min=0,params=[],rest=_,ret=string)".to_string()),
    });
    linker.add_module(Arc::new(typed)).unwrap();

    let import = Import {
        module_specifier: "typed".to_string(),
        symbol: "value".to_string(),
        alias: None,
        module_id,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: 2002,
        type_signature: Some("fn(min=1,params=[number],rest=_,ret=number)".to_string()),
        runtime_global_slot: None,
    };
    let result = linker.resolve_import(&import, "main");
    assert!(matches!(
        result,
        Err(LinkError::TypeSignatureMismatch {
            expected_hash: 2002,
            actual_hash: 1001,
            ..
        })
    ));
}

#[test]
fn test_link_type_hash_diff_but_structurally_assignable() {
    let mut linker = ModuleLinker::new();
    let mut typed = create_module("typed");
    typed.functions.push(Function {
        name: "fn1".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });
    let module_id = module_id_from_name("typed");
    let symbol_id = test_symbol_id(module_id, "value", 0);
    typed.exports.push(Export {
        name: "value".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: 1001,
        type_signature: Some("fn(min=0,params=[],rest=_,ret=number)".to_string()),
    });
    linker.add_module(Arc::new(typed)).unwrap();

    let import = Import {
        module_specifier: "typed".to_string(),
        symbol: "value".to_string(),
        alias: None,
        module_id,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: 2002,
        type_signature: Some("fn(min=1,params=[number],rest=_,ret=number)".to_string()),
        runtime_global_slot: None,
    };
    let result = linker.resolve_import(&import, "main");
    assert!(result.is_ok());
}

#[test]
fn test_link_scope_mismatch() {
    let mut linker = ModuleLinker::new();
    let mut scoped = create_module("scoped");
    scoped.functions.push(Function {
        name: "global_like".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });

    let module_id = module_id_from_name("scoped");
    let symbol_id = test_symbol_id(module_id, "global_like", 0);
    scoped.exports.push(Export {
        name: "global_like".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
        symbol_id,
        scope: SymbolScope::Global,
        signature_hash: symbol_id ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
    });
    linker.add_module(Arc::new(scoped)).unwrap();

    let import = Import {
        module_specifier: "scoped".to_string(),
        symbol: "global_like".to_string(),
        alias: None,
        module_id,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: symbol_id ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
        runtime_global_slot: None,
    };

    let result = linker.resolve_import(&import, "main");
    assert!(matches!(result, Err(LinkError::ScopeMismatch { .. })));
}

#[test]
fn test_add_module_rejects_duplicate_symbol_ids() {
    let mut linker = ModuleLinker::new();
    let mut module = create_module("dup_ids");
    module.functions.push(Function {
        name: "f0".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });
    module.functions.push(Function {
        name: "f1".to_string(),
        param_count: 0,
        local_count: 0,
        code: vec![],
    });

    let module_id = module_id_from_name("dup_ids");
    let symbol_id = test_symbol_id(module_id, "same", 0);
    module.exports.push(Export {
        name: "same_a".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: symbol_id ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
    });
    module.exports.push(Export {
        name: "same_b".to_string(),
        symbol_type: SymbolType::Function,
        index: 1,
        symbol_id,
        scope: SymbolScope::Module,
        signature_hash: symbol_id ^ 0x00FF_00FF_00FF_00FF,
        type_signature: None,
    });

    let result = linker.add_module(Arc::new(module));
    assert!(result.is_err());
    let message = result.err().unwrap();
    assert!(message.contains("duplicate symbol id"));
}
