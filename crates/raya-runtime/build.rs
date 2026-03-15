use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "src/builtin_inventory.rs"]
mod builtin_inventory;

use builtin_inventory::{builtin_logical_paths, BuiltinInventoryMode};
use raya_engine::compiler::module::{
    BuiltinSurfaceMode, ExportedSymbol, ModuleCompiler as BinaryModuleCompiler, ModuleExports,
};
use raya_engine::compiler::{module_id_from_name, symbol_id_from_name, SymbolScope, SymbolType};
use raya_engine::parser::ast::{Expression, Pattern, Statement, VariableKind};
use raya_engine::parser::checker::{
    Binder, CheckerPolicy, ScopeId, Symbol, SymbolFlags, SymbolKind, TypeSystemMode,
};
use raya_engine::parser::types::TypeId;
use raya_engine::parser::{Interner, Parser};
use raya_engine::TypeContext;

#[derive(Clone)]
struct ParsedBuiltinUnit {
    logical_path: &'static str,
    ast: raya_engine::parser::ast::Module,
    interner: Interner,
    local_names: Vec<String>,
    literal_globals: Vec<EmbeddedLiteralGlobal>,
}

#[derive(Clone)]
struct CompiledBuiltinMode {
    modules: Vec<EmbeddedBuiltinModule>,
    literal_globals: Vec<EmbeddedLiteralGlobal>,
}

#[derive(Clone)]
struct EmbeddedBuiltinModule {
    logical_path: &'static str,
    bytecode: Vec<u8>,
}

#[derive(Clone)]
struct EmbeddedLiteralGlobal {
    name: String,
    value: EmbeddedLiteralValue,
}

#[derive(Clone)]
enum EmbeddedLiteralValue {
    I32(i32),
    F64(f64),
    String(String),
    Bool(bool),
    Null,
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let builtins_root = manifest_dir
        .parent()
        .expect("runtime crate parent directory")
        .join("raya-engine")
        .join("builtins");

    println!("cargo:rerun-if-changed={}", builtins_root.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/builtin_inventory.rs");
    for mode in [
        BuiltinInventoryMode::RayaStrict,
        BuiltinInventoryMode::NodeCompat,
    ] {
        for logical_path in builtin_logical_paths(mode) {
            println!(
                "cargo:rerun-if-changed={}",
                builtins_root.join(logical_path).display()
            );
        }
    }

    let strict = compile_builtin_mode(BuiltinInventoryMode::RayaStrict, &builtins_root)
        .unwrap_or_else(|error| panic!("failed to precompile strict builtins: {error}"));
    let node = compile_builtin_mode(BuiltinInventoryMode::NodeCompat, &builtins_root)
        .unwrap_or_else(|error| panic!("failed to precompile node-compat builtins: {error}"));

    write_embedded_artifacts(&out_dir, &strict, &node)
        .unwrap_or_else(|error| panic!("failed to write embedded builtins: {error}"));
}

fn compile_builtin_mode(
    mode: BuiltinInventoryMode,
    builtins_root: &Path,
) -> Result<CompiledBuiltinMode, String> {
    let checker_mode = TypeSystemMode::Js;
    let checker_policy = CheckerPolicy::for_mode(checker_mode);
    let mut parsed_units = Vec::new();

    for logical_path in builtin_logical_paths(mode) {
        let source_path = builtins_root.join(logical_path);
        let module_source = fs::read_to_string(&source_path)
            .map_err(|error| format!("Failed to read builtin source '{}': {error}", logical_path))?;
        let parser = Parser::new(&module_source).map_err(|errors| {
            format!(
                "Failed to lex builtin source '{}': {}",
                logical_path,
                join_errors(&errors)
            )
        })?;
        let (ast, interner) = parser.parse().map_err(|errors| {
            format!(
                "Failed to parse builtin source '{}': {}",
                logical_path,
                join_errors(&errors)
            )
        })?;
        let mut local_names = top_level_runtime_names(&ast, &interner);
        local_names.sort();
        local_names.dedup();
        let literal_globals = top_level_literal_globals(&ast, &interner);

        parsed_units.push(ParsedBuiltinUnit {
            logical_path,
            ast,
            interner,
            local_names,
            literal_globals,
        });
    }

    let mut provisional_symbols = HashMap::new();
    for unit in &parsed_units {
        for local_name in &unit.local_names {
            if let Some(symbol_type) =
                infer_runtime_symbol_type(&unit.ast, &unit.interner, local_name)
            {
                provisional_symbols
                    .entry(local_name.clone())
                    .or_insert(symbol_type);
            }
        }
    }
    provisional_symbols.insert("EventEmitter".to_string(), SymbolType::Class);
    for name in [
        "Reflect",
        "Object",
        "Symbol",
        "Boolean",
        "Number",
        "String",
        "Array",
        "Error",
        "AggregateError",
        "TypeError",
        "Function",
        "Promise",
        "Math",
        "JSON",
    ] {
        provisional_symbols
            .entry(name.to_string())
            .or_insert(SymbolType::Constant);
    }

    let mut shared_type_ctx = TypeContext::new();
    for unit in &parsed_units {
        let mut binder = Binder::new(&mut shared_type_ctx, &unit.interner)
            .with_mode(checker_mode)
            .with_policy(checker_policy);
        binder.register_builtins(&[]);
        let mut ambient_symbols = provisional_symbols.clone();
        for local_name in &unit.local_names {
            ambient_symbols.remove(local_name);
        }
        seed_provisional_builtin_contract_symbols(&mut binder, &ambient_symbols);
        binder.bind_module(&unit.ast).map_err(|errors| {
            format!(
                "Failed to bind builtin source '{}': {}",
                unit.logical_path,
                join_errors(&errors)
            )
        })?;
    }

    let ambient_module_name = match mode {
        BuiltinInventoryMode::RayaStrict => "__raya_builtin__/strict",
        BuiltinInventoryMode::NodeCompat => "__raya_builtin__/node_compat",
    };
    let mut ambient_contract = ModuleExports::new(
        PathBuf::from(ambient_module_name),
        ambient_module_name.to_string(),
    );

    for unit in &parsed_units {
        let mut contract_type_ctx = shared_type_ctx.clone();
        let mut binder = Binder::new(&mut contract_type_ctx, &unit.interner)
            .with_mode(checker_mode)
            .with_policy(checker_policy);
        binder.register_builtins(&[]);
        let mut ambient_symbols = provisional_symbols.clone();
        for local_name in &unit.local_names {
            ambient_symbols.remove(local_name);
        }
        seed_provisional_builtin_contract_symbols(&mut binder, &ambient_symbols);
        let symbols = binder.bind_module(&unit.ast).map_err(|errors| {
            format!(
                "Failed to bind builtin export contracts for '{}': {}",
                unit.logical_path,
                join_errors(&errors)
            )
        })?;

        for local_name in &unit.local_names {
            let Some(symbol) = symbols.resolve(local_name) else {
                continue;
            };
            ambient_contract
                .symbols
                .entry(local_name.clone())
                .or_insert_with(|| {
                    ExportedSymbol::from_symbol(
                        symbol,
                        &ambient_contract.module_name,
                        SymbolScope::Global,
                        &contract_type_ctx,
                    )
                });
        }
    }

    for (name, symbol_type) in &provisional_symbols {
        if ambient_contract.has(name) {
            continue;
        }
        let kind = match symbol_type {
            SymbolType::Function => SymbolKind::Function,
            SymbolType::Class => SymbolKind::Class,
            SymbolType::Constant => SymbolKind::Variable,
        };
        let type_signature = match symbol_type {
            SymbolType::Class => name.clone(),
            SymbolType::Function | SymbolType::Constant => "any".to_string(),
        };
        ambient_contract.add_symbol(ExportedSymbol {
            name: name.clone(),
            local_name: name.clone(),
            kind,
            ty: TypeId::new(TypeContext::UNKNOWN_TYPE_ID),
            is_const: !matches!(kind, SymbolKind::Function),
            is_async: false,
            module_name: ambient_contract.module_name.clone(),
            module_id: module_id_from_name(&ambient_contract.module_name),
            symbol_id: symbol_id_from_name(&ambient_contract.module_name, SymbolScope::Global, name),
            signature_hash: raya_engine::parser::types::signature_hash(&type_signature),
            type_signature,
            scope: SymbolScope::Global,
        });
    }

    let builtin_surface_mode = match mode {
        BuiltinInventoryMode::RayaStrict => BuiltinSurfaceMode::RayaStrict,
        BuiltinInventoryMode::NodeCompat => BuiltinSurfaceMode::NodeCompat,
    };
    let mut compiler = BinaryModuleCompiler::new(builtins_root.to_path_buf())
        .with_checker_mode(checker_mode)
        .with_checker_policy(checker_policy)
        .with_builtin_surface_mode(builtin_surface_mode)
        .with_builtin_globals_override(ambient_contract);

    let virtual_entry_name = match mode {
        BuiltinInventoryMode::RayaStrict => "<__raya_builtin_runtime_entry_strict>.raya",
        BuiltinInventoryMode::NodeCompat => "<__raya_builtin_runtime_entry_node_compat>.raya",
    };
    let virtual_entry_path = builtins_root.join(virtual_entry_name);
    let synthetic_entry = parsed_units
        .iter()
        .enumerate()
        .map(|(idx, unit)| format!("import * as __builtin_{idx} from \"./{}\";", unit.logical_path))
        .collect::<Vec<_>>()
        .join("\n");
    let compiled = compiler
        .compile_with_virtual_entry_source(&virtual_entry_path, synthetic_entry)
        .map_err(|error| {
            format!(
                "Failed to compile builtin source graph for mode {:?}: {error}",
                mode
            )
        })?;
    let compiled_by_path = compiled
        .into_iter()
        .map(|module| (module.path, module.bytecode))
        .collect::<HashMap<_, _>>();

    let mut modules = Vec::with_capacity(parsed_units.len());
    let mut literal_globals = Vec::new();
    let mut seen_literals = HashSet::new();
    for unit in parsed_units {
        let canonical_path = builtins_root
            .join(unit.logical_path)
            .canonicalize()
            .map_err(|error| {
                format!(
                    "Failed to resolve builtin source '{}': {error}",
                    unit.logical_path
                )
            })?;
        let bytecode = compiled_by_path
            .get(&canonical_path)
            .ok_or_else(|| {
                format!(
                    "Compiled builtin source graph did not produce '{}'",
                    unit.logical_path
                )
            })?
            .encode();
        modules.push(EmbeddedBuiltinModule {
            logical_path: unit.logical_path,
            bytecode,
        });

        for literal in unit.literal_globals {
            if seen_literals.insert(literal.name.clone()) {
                literal_globals.push(literal);
            }
        }
    }

    Ok(CompiledBuiltinMode {
        modules,
        literal_globals,
    })
}

fn top_level_runtime_names(
    ast: &raya_engine::parser::ast::Module,
    interner: &Interner,
) -> Vec<String> {
    let mut names = Vec::new();
    for stmt in &ast.statements {
        match stmt {
            Statement::ClassDecl(class_decl) => {
                names.push(interner.resolve(class_decl.name.name).to_string());
            }
            Statement::FunctionDecl(func_decl) => {
                names.push(interner.resolve(func_decl.name.name).to_string());
            }
            Statement::VariableDecl(var_decl) => {
                collect_pattern_names(&var_decl.pattern, interner, &mut names);
            }
            _ => {}
        }
    }
    names
}

fn infer_runtime_symbol_type(
    ast: &raya_engine::parser::ast::Module,
    interner: &Interner,
    export_name: &str,
) -> Option<SymbolType> {
    for stmt in &ast.statements {
        match stmt {
            Statement::ClassDecl(class_decl)
                if interner.resolve(class_decl.name.name) == export_name =>
            {
                return Some(SymbolType::Class);
            }
            Statement::FunctionDecl(func_decl)
                if interner.resolve(func_decl.name.name) == export_name =>
            {
                return Some(SymbolType::Function);
            }
            Statement::VariableDecl(var_decl) => {
                let mut names = Vec::new();
                collect_pattern_names(&var_decl.pattern, interner, &mut names);
                if names.iter().any(|name| name == export_name) {
                    return Some(SymbolType::Constant);
                }
            }
            _ => {}
        }
    }
    None
}

fn collect_pattern_names(pattern: &Pattern, interner: &Interner, out: &mut Vec<String>) {
    match pattern {
        Pattern::Identifier(id) => out.push(interner.resolve(id.name).to_string()),
        Pattern::Array(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_names(&elem.pattern, interner, out);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_names(rest, interner, out);
            }
        }
        Pattern::Object(obj) => {
            for prop in &obj.properties {
                collect_pattern_names(&prop.value, interner, out);
            }
            if let Some(rest) = &obj.rest {
                out.push(interner.resolve(rest.name).to_string());
            }
        }
        Pattern::Rest(rest) => collect_pattern_names(&rest.argument, interner, out),
    }
}

fn top_level_literal_globals(
    ast: &raya_engine::parser::ast::Module,
    interner: &Interner,
) -> Vec<EmbeddedLiteralGlobal> {
    let mut values = Vec::new();
    for statement in &ast.statements {
        let Statement::VariableDecl(decl) = statement else {
            continue;
        };
        if decl.kind != VariableKind::Const {
            continue;
        }
        let Pattern::Identifier(ident) = &decl.pattern else {
            continue;
        };
        let Some(initializer) = &decl.initializer else {
            continue;
        };
        let Some(value) = literal_expr_value(initializer, interner) else {
            continue;
        };
        values.push(EmbeddedLiteralGlobal {
            name: interner.resolve(ident.name).to_string(),
            value,
        });
    }
    values
}

fn literal_expr_value(expr: &Expression, interner: &Interner) -> Option<EmbeddedLiteralValue> {
    match expr {
        Expression::IntLiteral(lit) => i32::try_from(lit.value)
            .ok()
            .map(EmbeddedLiteralValue::I32)
            .or_else(|| Some(EmbeddedLiteralValue::F64(lit.value as f64))),
        Expression::FloatLiteral(lit) => Some(EmbeddedLiteralValue::F64(lit.value)),
        Expression::StringLiteral(lit) => {
            Some(EmbeddedLiteralValue::String(interner.resolve(lit.value).to_string()))
        }
        Expression::BooleanLiteral(lit) => Some(EmbeddedLiteralValue::Bool(lit.value)),
        Expression::NullLiteral(_) => Some(EmbeddedLiteralValue::Null),
        _ => None,
    }
}

fn seed_provisional_builtin_contract_symbols(
    binder: &mut Binder<'_>,
    symbols: &HashMap<String, SymbolType>,
) {
    let any_ty = binder.any_type_id();

    for (name, symbol_type) in symbols {
        match symbol_type {
            SymbolType::Class => binder.register_external_class(name),
            SymbolType::Function | SymbolType::Constant => {
                let kind = match symbol_type {
                    SymbolType::Function => SymbolKind::Function,
                    SymbolType::Class => unreachable!(),
                    SymbolType::Constant => SymbolKind::Variable,
                };
                let _ = binder.define_imported(Symbol {
                    name: name.clone(),
                    kind,
                    ty: any_ty,
                    flags: SymbolFlags {
                        is_exported: false,
                        is_const: true,
                        is_async: false,
                        is_readonly: true,
                        is_imported: false,
                    },
                    scope_id: ScopeId(0),
                    span: raya_engine::parser::Span::new(0, 0, 0, 0),
                    referenced: false,
                });
            }
        }
    }

    let _ = binder.define_imported(Symbol {
        name: "undefined".to_string(),
        kind: SymbolKind::Variable,
        ty: any_ty,
        flags: SymbolFlags {
            is_exported: false,
            is_const: true,
            is_async: false,
            is_readonly: true,
            is_imported: false,
        },
        scope_id: ScopeId(0),
        span: raya_engine::parser::Span::new(0, 0, 0, 0),
        referenced: false,
    });
}

fn join_errors(errors: &[impl ToString]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

fn write_embedded_artifacts(
    out_dir: &Path,
    strict: &CompiledBuiltinMode,
    node: &CompiledBuiltinMode,
) -> Result<(), String> {
    let strict_modules =
        write_mode_bytecode_files(out_dir, "strict", BuiltinInventoryMode::RayaStrict, strict)?;
    let node_modules =
        write_mode_bytecode_files(out_dir, "node_compat", BuiltinInventoryMode::NodeCompat, node)?;

    let generated = format!(
        "pub static STRICT_EMBEDDED_BUILTIN_MODULES: &[EmbeddedBuiltinModule] = &[\n{strict_modules}];\n\n\
         pub static NODE_EMBEDDED_BUILTIN_MODULES: &[EmbeddedBuiltinModule] = &[\n{node_modules}];\n\n\
         pub static STRICT_EMBEDDED_LITERAL_GLOBALS: &[EmbeddedLiteralGlobal] = &[\n{strict_literals}];\n\n\
         pub static NODE_EMBEDDED_LITERAL_GLOBALS: &[EmbeddedLiteralGlobal] = &[\n{node_literals}];\n",
        strict_modules = strict_modules,
        node_modules = node_modules,
        strict_literals = render_literal_globals(&strict.literal_globals),
        node_literals = render_literal_globals(&node.literal_globals),
    );
    fs::write(out_dir.join("embedded_builtins.rs"), generated)
        .map_err(|error| format!("Failed to write embedded builtins index: {error}"))?;
    Ok(())
}

fn write_mode_bytecode_files(
    out_dir: &Path,
    stem_prefix: &str,
    _mode: BuiltinInventoryMode,
    compiled: &CompiledBuiltinMode,
) -> Result<String, String> {
    let mut rendered = String::new();
    for module in &compiled.modules {
        let filename = format!(
            "{stem_prefix}_{}.ryb",
            module
                .logical_path
                .replace(['/', '.', '-'], "_")
                .trim_matches('_')
        );
        let file_path = out_dir.join(filename);
        fs::write(&file_path, &module.bytecode).map_err(|error| {
            format!(
                "Failed to write embedded builtin bytecode '{}': {error}",
                module.logical_path
            )
        })?;
        rendered.push_str(&format!(
            "    EmbeddedBuiltinModule {{ logical_path: {:?}, bytecode: include_bytes!({:?}) }},\n",
            module.logical_path,
            file_path.display().to_string()
        ));
    }
    Ok(rendered)
}

fn render_literal_globals(literals: &[EmbeddedLiteralGlobal]) -> String {
    let mut rendered = String::new();
    for literal in literals {
        rendered.push_str("    EmbeddedLiteralGlobal { name: ");
        rendered.push_str(&format!("{:?}, value: ", literal.name));
        rendered.push_str(&render_literal_value(&literal.value));
        rendered.push_str(" },\n");
    }
    rendered
}

fn render_literal_value(value: &EmbeddedLiteralValue) -> String {
    match value {
        EmbeddedLiteralValue::I32(value) => format!("EmbeddedLiteralValue::I32({value})"),
        EmbeddedLiteralValue::F64(value) => format!("EmbeddedLiteralValue::F64({value:?})"),
        EmbeddedLiteralValue::String(value) => format!("EmbeddedLiteralValue::String({value:?})"),
        EmbeddedLiteralValue::Bool(value) => format!("EmbeddedLiteralValue::Bool({value})"),
        EmbeddedLiteralValue::Null => "EmbeddedLiteralValue::Null".to_string(),
    }
}
