//! Integration tests for module linking
//!
//! Tests the ModuleLinker for resolving imports to exports.

use raya_engine::compiler::{ConstantPool, Export, Function, Import, Metadata, Module, SymbolType};
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
        },
        exports: vec![],
        imports: vec![],
        checksum: [0; 32],
        reflection: None,
        debug_info: None,
        native_functions: vec![],
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
    logging.exports.push(Export {
        name: "info".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
    });

    linker.add_module(Arc::new(logging)).unwrap();

    // Create a main module that imports logging.info
    let import = Import {
        module_specifier: "logging".to_string(),
        symbol: "info".to_string(),
        alias: None,
        version_constraint: None,
    };

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
    utils.exports.push(Export {
        name: "helper".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
    });

    linker.add_module(Arc::new(utils)).unwrap();

    // Import with version specifier (linker strips version for now)
    let import = Import {
        module_specifier: "utils@1.2.3".to_string(),
        symbol: "helper".to_string(),
        alias: None,
        version_constraint: Some("1.2.3".to_string()),
    };

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
    org_package.exports.push(Export {
        name: "doWork".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
    });

    linker.add_module(Arc::new(org_package)).unwrap();

    // Import from scoped package with version
    let import = Import {
        module_specifier: "@org/package@^2.0.0".to_string(),
        symbol: "doWork".to_string(),
        alias: None,
        version_constraint: Some("^2.0.0".to_string()),
    };

    let resolved = linker.resolve_import(&import, "main").unwrap();
    assert_eq!(resolved.export.name, "doWork");
    assert_eq!(resolved.module.metadata.name, "@org/package");
}

#[test]
fn test_link_module_not_found() {
    let linker = ModuleLinker::new();

    let import = Import {
        module_specifier: "missing".to_string(),
        symbol: "foo".to_string(),
        alias: None,
        version_constraint: None,
    };

    let result = linker.resolve_import(&import, "main");
    assert!(matches!(result, Err(LinkError::ModuleNotFound(_))));
}

#[test]
fn test_link_symbol_not_found() {
    let mut linker = ModuleLinker::new();

    let logging = create_module("logging");
    linker.add_module(Arc::new(logging)).unwrap();

    let import = Import {
        module_specifier: "logging".to_string(),
        symbol: "debug".to_string(), // Not exported
        alias: None,
        version_constraint: None,
    };

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
    utils.exports.push(Export {
        name: "add".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
    });
    utils.exports.push(Export {
        name: "multiply".to_string(),
        symbol_type: SymbolType::Function,
        index: 1,
    });

    linker.add_module(Arc::new(utils)).unwrap();

    // Create main module with multiple imports
    let mut main = create_module("main");
    main.imports.push(Import {
        module_specifier: "utils".to_string(),
        symbol: "add".to_string(),
        alias: None,
        version_constraint: None,
    });
    main.imports.push(Import {
        module_specifier: "utils".to_string(),
        symbol: "multiply".to_string(),
        alias: None,
        version_constraint: None,
    });

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
    utils.exports.push(Export {
        name: "log".to_string(),
        symbol_type: SymbolType::Function,
        index: 0,
    });

    linker.add_module(Arc::new(utils)).unwrap();

    // Import with alias
    let import = Import {
        module_specifier: "utils".to_string(),
        symbol: "log".to_string(),
        alias: Some("print".to_string()),
        version_constraint: None,
    };

    let resolved = linker.resolve_import(&import, "main").unwrap();
    assert_eq!(resolved.export.name, "log"); // Original name in export
}
