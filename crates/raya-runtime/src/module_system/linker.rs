use crate::error::RuntimeError;
use raya_engine::parser::ast::{
    ClassDecl, ClassMember, ExportDecl, Expression, FunctionDecl, ImportSpecifier, ObjectProperty,
    Pattern, PropertyKey, Statement, VariableDecl,
};
use raya_engine::parser::{Interner, Parser};
use std::collections::{BTreeMap, HashMap};

use super::graph::{ProgramGraph, ProgramGraphNode};
use super::resolver::{ModuleKey, ModuleSpecifierKind};

const INTERNAL_DEFAULT_EXPORT: &str = "__default";

#[derive(Debug, Clone)]
pub struct LinkedProgramSource {
    pub module_order: Vec<std::path::PathBuf>,
    pub source: String,
}

#[derive(Debug, Clone, Default)]
struct ModuleMeta {
    export_type_name: String,
    export_var_name: String,
    export_types: BTreeMap<String, String>,
}

#[derive(Debug, Default, Clone)]
pub struct ProgramLinkerV2;

impl ProgramLinkerV2 {
    pub fn link(graph: &ProgramGraph) -> Result<LinkedProgramSource, RuntimeError> {
        let mut merged = String::new();
        let mut metas: HashMap<ModuleKey, ModuleMeta> = HashMap::new();
        let mut module_ids: HashMap<ModuleKey, usize> = HashMap::new();
        for (idx, key) in graph.topological_order.iter().enumerate() {
            module_ids.insert(key.clone(), idx);
        }

        for key in &graph.topological_order {
            if key == &graph.entry {
                continue;
            }
            let node = graph
                .nodes
                .get(key)
                .ok_or_else(|| RuntimeError::Dependency(format!("Missing graph node '{}'", key.display_name())))?;
            let id = *module_ids
                .get(key)
                .ok_or_else(|| RuntimeError::Dependency("Missing module id".to_string()))?;
            let (code, meta) = transform_library_module(node, id, &metas, &module_ids)?;
            merged.push_str(&code);
            merged.push('\n');
            metas.insert(key.clone(), meta);
        }

        let entry_node = graph.nodes.get(&graph.entry).ok_or_else(|| {
            RuntimeError::Dependency(format!(
                "Missing graph entry node '{}'",
                graph.entry.display_name()
            ))
        })?;
        let entry_id = *module_ids
            .get(&graph.entry)
            .ok_or_else(|| RuntimeError::Dependency("Missing entry module id".to_string()))?;
        let entry_code = transform_entry_module(entry_node, entry_id, &metas, &module_ids)?;
        merged.push_str(&entry_code);

        let module_order = graph
            .topological_order
            .iter()
            .filter_map(|k| match k {
                ModuleKey::File(path) => Some(path.clone()),
                ModuleKey::Std(_) => None,
            })
            .collect::<Vec<_>>();

        Ok(LinkedProgramSource {
            module_order,
            source: merged,
        })
    }
}

fn transform_library_module(
    node: &ProgramGraphNode,
    module_id: usize,
    metas: &HashMap<ModuleKey, ModuleMeta>,
    _module_ids: &HashMap<ModuleKey, usize>,
) -> Result<(String, ModuleMeta), RuntimeError> {
    let parser = Parser::new(&node.source).map_err(|errors| {
        RuntimeError::Lex(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    let (ast, interner) = parser.parse().map_err(|errors| {
        RuntimeError::Parse(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;

    let module_tag = format!("m{}", module_id);
    let class_aliases = collect_class_aliases(&ast.statements, &interner, &module_tag);
    let class_alias_block = synthesize_class_aliases(
        &ast.statements,
        &node.source,
        &interner,
        &class_aliases,
    );

    let mut local_value_types =
        collect_local_value_types(&ast.statements, &node.source, &interner, &class_aliases);
    let mut imported_binding_types: HashMap<String, String> = HashMap::new();
    let mut export_types: BTreeMap<String, String> = BTreeMap::new();
    let mut export_values: BTreeMap<String, String> = BTreeMap::new();

    let mut body = String::new();
    let mut default_counter = 0usize;
    let mut import_counter = 0usize;
    let dep_map = dependency_map(node);

    for stmt in &ast.statements {
        match stmt {
            Statement::ImportDecl(import) => {
                let specifier = interner.resolve(import.source.value);
                let dep_key = dep_map.get(specifier).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Missing dependency for '{}' in module '{}'",
                        specifier, node.display_name
                    ))
                })?;
                let dep_meta = metas.get(dep_key).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Dependency '{}' metadata not available while linking '{}'",
                        dep_key.display_name(),
                        node.display_name
                    ))
                })?;
                let dep_binding = format!("__raya_dep_{}_{}", module_id, import_counter);
                import_counter += 1;
                body.push_str(&format!(
                    "const {}: {} = {};\n",
                    dep_binding, dep_meta.export_type_name, dep_meta.export_var_name
                ));

                for spec in &import.specifiers {
                    match spec {
                        ImportSpecifier::Named { name, alias } => {
                            let imported = interner.resolve(name.name).to_string();
                            let local = alias
                                .as_ref()
                                .map(|a| interner.resolve(a.name).to_string())
                                .unwrap_or_else(|| imported.clone());
                            let ty = dep_meta
                                .export_types
                                .get(&imported)
                                .cloned()
                                .unwrap_or_else(|| "unknown".to_string());
                            if looks_like_class_identifier(&ty)
                                && imported == ty
                                && is_known_global_class(&imported)
                            {
                                if local != imported {
                                    body.push_str(&format!("const {} = {};\n", local, imported));
                                }
                            } else {
                                body.push_str(&format!(
                                    "const {} = {};\n",
                                    local,
                                    property_accessor(&dep_binding, &imported)
                                ));
                            }
                            if ty != "unknown" && looks_like_class_identifier(&local) {
                                body.push_str(&format!("type {} = {};\n", local, ty));
                            }
                            imported_binding_types.insert(local.clone(), ty.clone());
                            local_value_types.entry(local).or_insert(ty);
                        }
                        ImportSpecifier::Default(local) => {
                            let local_name = interner.resolve(local.name).to_string();
                            let ty = dep_meta
                                .export_types
                                .get("default")
                                .cloned()
                                .unwrap_or_else(|| "unknown".to_string());
                            body.push_str(&format!(
                                "const {} = {};\n",
                                local_name,
                                property_accessor(&dep_binding, INTERNAL_DEFAULT_EXPORT)
                            ));
                            if ty != "unknown" && looks_like_class_identifier(&local_name) {
                                body.push_str(&format!("type {} = {};\n", local_name, ty));
                            }
                            imported_binding_types.insert(local_name.clone(), ty.clone());
                            local_value_types.entry(local_name).or_insert(ty);
                        }
                        ImportSpecifier::Namespace(alias) => {
                            let local_name = interner.resolve(alias.name).to_string();
                            body.push_str(&format!(
                                "const {} = {};\n",
                                local_name, dep_binding
                            ));
                            imported_binding_types
                                .insert(local_name.clone(), dep_meta.export_type_name.clone());
                            local_value_types
                                .entry(local_name)
                                .or_insert(dep_meta.export_type_name.clone());
                        }
                    }
                }
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => {
                body.push_str(inner.span().slice(&node.source));
                body.push('\n');
                for exported in declaration_runtime_names(inner, &interner) {
                    let ty = local_value_types
                        .get(&exported)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    export_types.insert(exported.clone(), ty);
                    export_values.insert(exported.clone(), exported);
                }
            }
            Statement::ExportDecl(ExportDecl::Named {
                specifiers,
                source,
                ..
            }) => {
                if let Some(src) = source {
                    let specifier = interner.resolve(src.value);
                    let dep_key = dep_map.get(specifier).ok_or_else(|| {
                        RuntimeError::Dependency(format!(
                            "Missing dependency for re-export '{}' in module '{}'",
                            specifier, node.display_name
                        ))
                    })?;
                    let dep_meta = metas.get(dep_key).ok_or_else(|| {
                        RuntimeError::Dependency(format!(
                            "Dependency '{}' metadata not available while linking '{}'",
                            dep_key.display_name(),
                            node.display_name
                        ))
                    })?;
                    for spec in specifiers {
                        let imported = interner.resolve(spec.name.name).to_string();
                        let exported = spec
                            .alias
                            .as_ref()
                            .map(|a| interner.resolve(a.name).to_string())
                            .unwrap_or_else(|| imported.clone());
                        let ty = dep_meta
                            .export_types
                            .get(&imported)
                            .cloned()
                            .unwrap_or_else(|| "unknown".to_string());
                        export_types.insert(exported.clone(), ty);
                        export_values.insert(
                            internal_export_name(&exported),
                            typed_property_accessor(
                                &dep_meta.export_var_name,
                                &dep_meta.export_type_name,
                                &imported,
                            ),
                        );
                    }
                } else {
                    for spec in specifiers {
                        let local_name = interner.resolve(spec.name.name).to_string();
                        let exported = spec
                            .alias
                            .as_ref()
                            .map(|a| interner.resolve(a.name).to_string())
                            .unwrap_or_else(|| local_name.clone());
                        let mut ty = local_value_types
                            .get(&local_name)
                            .cloned()
                            .or_else(|| imported_binding_types.get(&local_name).cloned())
                            .unwrap_or_else(|| "unknown".to_string());
                        if ty == "unknown" {
                            if let Some(alias) = class_aliases.get(&local_name) {
                                ty = alias.clone();
                            } else if is_known_global_class(&local_name) {
                                ty = local_name.clone();
                            }
                        }
                        export_types.insert(exported.clone(), ty);
                        export_values.insert(internal_export_name(&exported), local_name);
                    }
                }
            }
            Statement::ExportDecl(ExportDecl::All { source, .. }) => {
                let specifier = interner.resolve(source.value);
                let dep_key = dep_map.get(specifier).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Missing dependency for export-all '{}' in module '{}'",
                        specifier, node.display_name
                    ))
                })?;
                let dep_meta = metas.get(dep_key).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Dependency '{}' metadata not available while linking '{}'",
                        dep_key.display_name(),
                        node.display_name
                    ))
                })?;
                for (name, ty) in &dep_meta.export_types {
                    if name == "default" {
                        continue;
                    }
                    export_types.insert(name.clone(), ty.clone());
                    export_values.insert(
                        internal_export_name(name),
                        typed_property_accessor(
                            &dep_meta.export_var_name,
                            &dep_meta.export_type_name,
                            name,
                        ),
                    );
                }
            }
            Statement::ExportDecl(ExportDecl::Default { expression, .. }) => {
                let expr_src = expression.span().slice(&node.source);
                let tmp = format!("__raya_default_{}_{}", module_id, default_counter);
                default_counter += 1;
                body.push_str(&format!("const {} = {};\n", tmp, expr_src));
                let ty = infer_expression_type(
                    expr_src,
                    expression,
                    &node.source,
                    &interner,
                    &local_value_types,
                    &imported_binding_types,
                    &class_aliases,
                );
                export_types.insert("default".to_string(), ty);
                export_values.insert(INTERNAL_DEFAULT_EXPORT.to_string(), tmp);
            }
            _ => {
                body.push_str(stmt.span().slice(&node.source));
                body.push('\n');
            }
        }
    }

    let export_type_name = format!("__raya_mod_exports_type_{}", module_id);
    let export_var_name = format!("__raya_mod_exports_{}", module_id);
    let export_alias = format!(
        "type {} = {{ {} }};\n",
        export_type_name,
        export_types
            .iter()
            .map(|(name, ty)| {
                let internal_name = internal_export_name(name);
                let key = if is_identifier(&internal_name) {
                    internal_name
                } else {
                    format!("\"{}\"", escape_string(&internal_name))
                };
                format!("{}: {}", key, ty)
            })
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut code = String::new();
    code.push_str(&format!("// module: {}\n", node.display_name));
    if !class_alias_block.is_empty() {
        code.push_str(&class_alias_block);
        code.push('\n');
    }
    code.push_str(&export_alias);
    code.push_str(&format!(
        "function __raya_mod_init_{}(): {} {{\n",
        module_id, export_type_name
    ));
    code.push_str(&body);
    let object_literal = if export_values.is_empty() {
        "{}".to_string()
    } else {
        let fields = export_values
            .iter()
            .map(|(name, value)| {
                let key = if is_safe_property_identifier(name) {
                    name.clone()
                } else {
                    format!("\"{}\"", escape_string(name))
                };
                format!("{}: {}", key, value)
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{ {} }}", fields)
    };
    code.push_str(&format!(
        "let __raya_exports: {} = {};\n",
        export_type_name, object_literal
    ));
    code.push_str("return __raya_exports;\n");
    code.push_str("}\n");
    code.push_str(&format!(
        "const {}: {} = __raya_mod_init_{}();\n",
        export_var_name, export_type_name, module_id
    ));

    Ok((
        code,
        ModuleMeta {
            export_type_name,
            export_var_name,
            export_types,
        },
    ))
}

fn transform_entry_module(
    node: &ProgramGraphNode,
    module_id: usize,
    metas: &HashMap<ModuleKey, ModuleMeta>,
    _module_ids: &HashMap<ModuleKey, usize>,
) -> Result<String, RuntimeError> {
    let parser = Parser::new(&node.source).map_err(|errors| {
        RuntimeError::Lex(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    let (ast, interner) = parser.parse().map_err(|errors| {
        RuntimeError::Parse(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;

    let dep_map = dependency_map(node);
    let mut body = String::new();
    let mut cursor = 0usize;
    let mut import_counter = 0usize;

    for stmt in &ast.statements {
        match stmt {
            Statement::ImportDecl(import) => {
                let span = stmt.span();
                append_source_segment(&mut body, &node.source, &mut cursor, span.start);
                let specifier = interner.resolve(import.source.value);
                let dep_key = dep_map.get(specifier).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Missing dependency for '{}' in entry module '{}'",
                        specifier, node.display_name
                    ))
                })?;
                let dep_meta = metas.get(dep_key).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Dependency '{}' metadata not available while linking entry '{}'",
                        dep_key.display_name(),
                        node.display_name
                    ))
                })?;
                let dep_binding = format!("__raya_dep_entry_{}_{}", module_id, import_counter);
                import_counter += 1;
                body.push_str(&format!(
                    "const {}: {} = {};\n",
                    dep_binding, dep_meta.export_type_name, dep_meta.export_var_name
                ));
                for spec in &import.specifiers {
                    match spec {
                        ImportSpecifier::Named { name, alias } => {
                            let imported = interner.resolve(name.name).to_string();
                            let local = alias
                                .as_ref()
                                .map(|a| interner.resolve(a.name).to_string())
                                .unwrap_or_else(|| imported.clone());
                            let ty = dep_meta
                                .export_types
                                .get(&imported)
                                .cloned()
                                .unwrap_or_else(|| "unknown".to_string());
                            if looks_like_class_identifier(&ty)
                                && imported == ty
                                && is_known_global_class(&imported)
                            {
                                if local != imported {
                                    body.push_str(&format!("const {} = {};\n", local, imported));
                                }
                            } else {
                                body.push_str(&format!(
                                    "const {} = {};\n",
                                    local,
                                    property_accessor(&dep_binding, &imported)
                                ));
                            }
                        }
                        ImportSpecifier::Default(local) => {
                            let local_name = interner.resolve(local.name).to_string();
                            let ty = dep_meta
                                .export_types
                                .get("default")
                                .cloned()
                                .unwrap_or_else(|| "unknown".to_string());
                            body.push_str(&format!(
                                "const {} = {};\n",
                                local_name,
                                property_accessor(&dep_binding, INTERNAL_DEFAULT_EXPORT)
                            ));
                        }
                        ImportSpecifier::Namespace(alias) => {
                            let local_name = interner.resolve(alias.name).to_string();
                            body.push_str(&format!(
                                "const {} = {};\n",
                                local_name, dep_binding
                            ));
                        }
                    }
                }
                cursor = cursor.max(span.end.min(node.source.len()));
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => {
                let span = stmt.span();
                append_source_segment(&mut body, &node.source, &mut cursor, span.start);
                body.push_str(strip_export_prefix(span.slice(&node.source)));
                body.push('\n');
                cursor = cursor.max(span.end.min(node.source.len()));
            }
            Statement::ExportDecl(ExportDecl::Default { expression, .. }) => {
                let span = stmt.span();
                append_source_segment(&mut body, &node.source, &mut cursor, span.start);
                body.push_str(expression.span().slice(&node.source));
                body.push_str(";\n");
                cursor = cursor.max(span.end.min(node.source.len()));
            }
            Statement::ExportDecl(ExportDecl::Named { .. })
            | Statement::ExportDecl(ExportDecl::All { .. }) => {
                let span = stmt.span();
                append_source_segment(&mut body, &node.source, &mut cursor, span.start);
                cursor = cursor.max(span.end.min(node.source.len()));
            }
            _ => {}
        }
    }
    body.push_str(&node.source[cursor..]);

    let mut code = String::new();
    code.push_str(&format!("// entry module: {}\n", node.display_name));
    code.push_str(&format!(
        "function __raya_entry_main_{}(): unknown {{\n",
        module_id
    ));
    code.push_str(&body);
    code.push_str("return null;\n");
    code.push_str("}\n");
    code.push_str(&format!("return __raya_entry_main_{}();\n", module_id));
    Ok(code)
}

fn dependency_map(node: &ProgramGraphNode) -> HashMap<String, ModuleKey> {
    let mut map = HashMap::new();
    for import in &node.imports {
        let key = match &import.kind {
            ModuleSpecifierKind::File(path) => ModuleKey::File(path.clone()),
            ModuleSpecifierKind::Std(name) => ModuleKey::Std(name.clone()),
        };
        map.entry(import.raw_specifier.clone()).or_insert(key);
    }
    map
}

fn collect_class_aliases(
    statements: &[Statement],
    interner: &Interner,
    module_tag: &str,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for stmt in statements {
        match stmt {
            Statement::ClassDecl(class) => {
                let name = interner.resolve(class.name.name).to_string();
                out.insert(name.clone(), format!("__t_{}_{}", module_tag, name));
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => {
                if let Statement::ClassDecl(class) = inner.as_ref() {
                    let name = interner.resolve(class.name.name).to_string();
                    out.insert(name.clone(), format!("__t_{}_{}", module_tag, name));
                }
            }
            _ => {}
        }
    }
    out
}

fn synthesize_class_aliases(
    statements: &[Statement],
    source: &str,
    interner: &Interner,
    class_aliases: &HashMap<String, String>,
) -> String {
    let mut out = String::new();
    for stmt in statements {
        let class = match stmt {
            Statement::ClassDecl(class) => Some(class),
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => {
                if let Statement::ClassDecl(class) = inner.as_ref() {
                    Some(class)
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(class) = class else {
            continue;
        };

        let class_name = interner.resolve(class.name.name).to_string();
        let Some(alias_name) = class_aliases.get(&class_name) else {
            continue;
        };
        let mut members = Vec::new();
        for member in &class.members {
            match member {
                ClassMember::Field(field) if !field.is_static => {
                    let field_name = interner.resolve(field.name.name).to_string();
                    let ty = field
                        .type_annotation
                        .as_ref()
                        .map(|ann| render_type_annotation(ann, source, interner))
                        .map(|ty| rewrite_local_class_refs(&ty, class_aliases))
                        .unwrap_or_else(|| "unknown".to_string());
                    members.push(format!("{}: {}", field_name, ty));
                }
                ClassMember::Method(method) if !method.is_static => {
                    let method_name = interner.resolve(method.name.name).to_string();
                    let params = method
                        .params
                        .iter()
                        .enumerate()
                        .map(|(idx, param)| {
                            let param_name = match &param.pattern {
                                Pattern::Identifier(id) => interner.resolve(id.name).to_string(),
                                _ => format!("arg{}", idx),
                            };
                            let suffix = if param.is_rest {
                                format!("...{}", param_name)
                            } else if param.optional || param.default_value.is_some() {
                                format!("{}?", param_name)
                            } else {
                                param_name
                            };
                            let ty = param
                                .type_annotation
                                .as_ref()
                                .map(|ann| render_type_annotation(ann, source, interner))
                                .map(|ty| rewrite_local_class_refs(&ty, class_aliases))
                                .unwrap_or_else(|| "unknown".to_string());
                            format!("{}: {}", suffix, ty)
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let ret = method
                        .return_type
                        .as_ref()
                        .map(|ann| render_type_annotation(ann, source, interner))
                        .map(|ty| rewrite_local_class_refs(&ty, class_aliases))
                        .unwrap_or_else(|| "void".to_string());
                    let key = if is_identifier(&method_name) {
                        method_name
                    } else {
                        format!("\"{}\"", escape_string(&method_name))
                    };
                    members.push(format!("{}: ({}) => {}", key, params, ret));
                }
                _ => {}
            }
        }
        out.push_str(&format!("type {} = {{ {} }};\n", alias_name, members.join(", ")));
    }
    out
}

fn collect_local_value_types(
    statements: &[Statement],
    source: &str,
    interner: &Interner,
    class_aliases: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for stmt in statements {
        match stmt {
            Statement::FunctionDecl(func) => {
                let name = interner.resolve(func.name.name).to_string();
                out.insert(
                    name,
                    function_type_expr(func, source, interner, class_aliases),
                );
            }
            Statement::ClassDecl(class) => {
                let name = interner.resolve(class.name.name).to_string();
                let ty = class_aliases
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                out.insert(name, ty);
            }
            Statement::VariableDecl(decl) => {
                let ty = infer_variable_decl_type(decl, source, interner, class_aliases);
                for name in pattern_names(&decl.pattern, interner) {
                    out.insert(name, ty.clone());
                }
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => match inner.as_ref() {
                Statement::FunctionDecl(func) => {
                    let name = interner.resolve(func.name.name).to_string();
                    out.insert(
                        name,
                        function_type_expr(func, source, interner, class_aliases),
                    );
                }
                Statement::ClassDecl(class) => {
                    let name = interner.resolve(class.name.name).to_string();
                    let ty = class_aliases
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| name.clone());
                    out.insert(name, ty);
                }
                Statement::VariableDecl(decl) => {
                    let ty = infer_variable_decl_type(decl, source, interner, class_aliases);
                    for name in pattern_names(&decl.pattern, interner) {
                        out.insert(name, ty.clone());
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    out
}

fn function_type_expr(
    func: &FunctionDecl,
    source: &str,
    interner: &Interner,
    class_aliases: &HashMap<String, String>,
) -> String {
    let params = func
        .params
        .iter()
        .enumerate()
        .map(|(idx, param)| {
            let name = match &param.pattern {
                Pattern::Identifier(id) => interner.resolve(id.name).to_string(),
                _ => format!("arg{}", idx),
            };
            let ty = param
                .type_annotation
                .as_ref()
                .map(|ann| render_type_annotation(ann, source, interner))
                .map(|ty| rewrite_local_class_refs(&ty, class_aliases))
                .unwrap_or_else(|| "unknown".to_string());
            let maybe = if param.is_rest {
                format!("...{}", name)
            } else if param.optional || param.default_value.is_some() {
                format!("{}?", name)
            } else {
                name
            };
            format!("{}: {}", maybe, ty)
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = func
        .return_type
        .as_ref()
        .map(|ann| render_type_annotation(ann, source, interner))
        .map(|ty| rewrite_local_class_refs(&ty, class_aliases))
        .unwrap_or_else(|| "void".to_string());
    format!("({}) => {}", params, ret)
}

fn infer_variable_decl_type(
    decl: &VariableDecl,
    source: &str,
    interner: &Interner,
    class_aliases: &HashMap<String, String>,
) -> String {
    if let Some(ann) = &decl.type_annotation {
        let ty = render_type_annotation(ann, source, interner);
        return rewrite_local_class_refs(&ty, class_aliases);
    }
    if let Some(init) = &decl.initializer {
        let src = init.span().slice(source).trim();
        if let Some(rest) = src.strip_prefix("new ") {
            let class_name = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>();
            if !class_name.is_empty() {
                if let Some(alias) = class_aliases.get(&class_name) {
                    return alias.clone();
                }
                return "unknown".to_string();
            }
        }
    }
    "unknown".to_string()
}

fn infer_expression_type(
    expr_src: &str,
    expression: &Expression,
    source: &str,
    interner: &Interner,
    local_value_types: &HashMap<String, String>,
    imported_binding_types: &HashMap<String, String>,
    class_aliases: &HashMap<String, String>,
) -> String {
    match expression {
        Expression::Identifier(id) => {
            let name = interner.resolve(id.name).to_string();
            if let Some(ty) = local_value_types.get(&name) {
                return ty.clone();
            }
            if let Some(ty) = imported_binding_types.get(&name) {
                return ty.clone();
            }
            if let Some(alias) = class_aliases.get(&name) {
                return alias.clone();
            }
            if looks_like_class_identifier(&name) && is_known_global_class(&name) {
                return name;
            }
            if looks_like_class_identifier(&name) {
                return "unknown".to_string();
            }
        }
        Expression::Object(obj) => {
            return infer_object_expression_type(
                obj,
                source,
                interner,
                local_value_types,
                imported_binding_types,
                class_aliases,
            );
        }
        Expression::Parenthesized(paren) => {
            let inner_src = paren.expression.span().slice(source);
            return infer_expression_type(
                inner_src,
                &paren.expression,
                source,
                interner,
                local_value_types,
                imported_binding_types,
                class_aliases,
            );
        }
        Expression::TypeCast(cast) => {
            let ty = render_type_annotation(&cast.target_type, source, interner);
            return rewrite_local_class_refs(&ty, class_aliases);
        }
        _ => {}
    }

    let trimmed = expr_src.trim();
    if let Some(rest) = trimmed.strip_prefix("new ") {
        let class_name = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect::<String>();
        if !class_name.is_empty() {
            if let Some(alias) = class_aliases.get(&class_name) {
                return alias.clone();
            }
            return "unknown".to_string();
        }
    }
    "unknown".to_string()
}

fn infer_object_expression_type(
    obj: &raya_engine::parser::ast::ObjectExpression,
    source: &str,
    interner: &Interner,
    local_value_types: &HashMap<String, String>,
    imported_binding_types: &HashMap<String, String>,
    class_aliases: &HashMap<String, String>,
) -> String {
    let mut fields = Vec::new();

    for prop in &obj.properties {
        let ObjectProperty::Property(property) = prop else {
            // Preserve strictness until object spread typing is modeled in linker inference.
            return "unknown".to_string();
        };

        let Some(key) = object_property_key_type(property, interner) else {
            return "unknown".to_string();
        };

        let value_src = property.value.span().slice(source);
        let value_ty = infer_expression_type(
            value_src,
            &property.value,
            source,
            interner,
            local_value_types,
            imported_binding_types,
            class_aliases,
        );

        fields.push(format!("{}: {}", key, value_ty));
    }

    if fields.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", fields.join(", "))
    }
}

fn object_property_key_type(
    property: &raya_engine::parser::ast::Property,
    interner: &Interner,
) -> Option<String> {
    match &property.key {
        PropertyKey::Identifier(id) => {
            let name = interner.resolve(id.name).to_string();
            if is_identifier(&name) {
                Some(name)
            } else {
                Some(format!("\"{}\"", escape_string(&name)))
            }
        }
        PropertyKey::StringLiteral(s) => Some(format!(
            "\"{}\"",
            escape_string(interner.resolve(s.value))
        )),
        PropertyKey::IntLiteral(n) => Some(format!("\"{}\"", n.value)),
        PropertyKey::Computed(_) => None,
    }
}

fn declaration_runtime_names(stmt: &Statement, interner: &Interner) -> Vec<String> {
    match stmt {
        Statement::FunctionDecl(func) => vec![interner.resolve(func.name.name).to_string()],
        Statement::ClassDecl(class) => vec![interner.resolve(class.name.name).to_string()],
        Statement::VariableDecl(decl) => pattern_names(&decl.pattern, interner),
        _ => Vec::new(),
    }
}

fn pattern_names(pattern: &Pattern, interner: &Interner) -> Vec<String> {
    let mut names = Vec::new();
    collect_pattern_names(pattern, interner, &mut names);
    names
}

fn collect_pattern_names(pattern: &Pattern, interner: &Interner, out: &mut Vec<String>) {
    match pattern {
        Pattern::Identifier(id) => out.push(interner.resolve(id.name).to_string()),
        Pattern::Rest(rest) => collect_pattern_names(&rest.argument, interner, out),
        Pattern::Array(arr) => {
            for elem in &arr.elements {
                if let Some(elem) = elem {
                    collect_pattern_names(&elem.pattern, interner, out);
                }
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
    }
}

fn append_source_segment(output: &mut String, source: &str, cursor: &mut usize, next_start: usize) {
    let start = next_start.min(source.len());
    if *cursor < start {
        output.push_str(&source[*cursor..start]);
    }
}

fn property_accessor(object: &str, prop: &str) -> String {
    if is_safe_property_identifier(prop) {
        format!("{}.{}", object, prop)
    } else {
        format!("{}[\"{}\"]", object, escape_string(prop))
    }
}

fn typed_property_accessor(object: &str, object_type: &str, prop: &str) -> String {
    let typed_object = format!("({} as {})", object, object_type);
    property_accessor(&typed_object, prop)
}

fn internal_export_name(name: &str) -> String {
    if name == "default" {
        INTERNAL_DEFAULT_EXPORT.to_string()
    } else {
        name.to_string()
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn is_safe_property_identifier(name: &str) -> bool {
    is_identifier(name) && !is_reserved_keyword(name)
}

fn is_reserved_keyword(name: &str) -> bool {
    matches!(
        name,
        "function"
            | "class"
            | "type"
            | "interface"
            | "let"
            | "const"
            | "if"
            | "else"
            | "switch"
            | "case"
            | "default"
            | "for"
            | "while"
            | "do"
            | "break"
            | "continue"
            | "return"
            | "async"
            | "await"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "import"
            | "export"
            | "from"
            | "new"
            | "this"
            | "super"
            | "static"
            | "abstract"
            | "readonly"
            | "keyof"
            | "extends"
            | "implements"
            | "as"
            | "in"
            | "of"
            | "instanceof"
            | "typeof"
            | "void"
            | "delete"
            | "debugger"
            | "true"
            | "false"
            | "null"
            | "undefined"
    )
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn looks_like_class_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_known_global_class(name: &str) -> bool {
    matches!(
        name,
        "Object"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "AggregateError"
            | "Symbol"
            | "Map"
            | "Set"
            | "Buffer"
            | "Date"
            | "Channel"
            | "Mutex"
            | "Promise"
            | "EventEmitter"
            | "Iterator"
            | "Temporal"
    )
}

fn render_type_annotation(
    ann: &raya_engine::parser::ast::TypeAnnotation,
    source: &str,
    interner: &Interner,
) -> String {
    render_type_expr(&ann.ty, source, interner)
}

fn render_type_expr(
    ty: &raya_engine::parser::ast::Type,
    source: &str,
    interner: &Interner,
) -> String {
    use raya_engine::parser::ast::Type as AstType;
    use raya_engine::parser::ast::types::ObjectTypeMember;
    use raya_engine::parser::ast::PrimitiveType;

    match ty {
        AstType::Primitive(p) => match p {
            PrimitiveType::Number => "number".to_string(),
            PrimitiveType::Int => "int".to_string(),
            PrimitiveType::String => "string".to_string(),
            PrimitiveType::Boolean => "boolean".to_string(),
            PrimitiveType::Null => "null".to_string(),
            PrimitiveType::Void => "void".to_string(),
        },
        AstType::Reference(reference) => {
            let mut out = interner.resolve(reference.name.name).to_string();
            if let Some(args) = &reference.type_args {
                let rendered = args
                    .iter()
                    .map(|a| render_type_annotation(a, source, interner))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push('<');
                out.push_str(&rendered);
                out.push('>');
            }
            out
        }
        AstType::Union(union) => union
            .types
            .iter()
            .map(|t| render_type_annotation(t, source, interner))
            .collect::<Vec<_>>()
            .join(" | "),
        AstType::Intersection(intersection) => intersection
            .types
            .iter()
            .map(|t| render_type_annotation(t, source, interner))
            .collect::<Vec<_>>()
            .join(" & "),
        AstType::Function(function) => {
            let params = function
                .params
                .iter()
                .enumerate()
                .map(|(idx, p)| {
                    let name = p
                        .name
                        .as_ref()
                        .map(|id| interner.resolve(id.name).to_string())
                        .unwrap_or_else(|| format!("arg{}", idx));
                    let head = if p.is_rest {
                        format!("...{}", name)
                    } else if p.optional {
                        format!("{}?", name)
                    } else {
                        name
                    };
                    let ty = render_type_annotation(&p.ty, source, interner);
                    format!("{}: {}", head, ty)
                })
                .collect::<Vec<_>>()
                .join(", ");
            let ret = render_type_annotation(&function.return_type, source, interner);
            format!("({}) => {}", params, ret)
        }
        AstType::Array(arr) => {
            let elem = render_type_annotation(&arr.element_type, source, interner);
            format!("{}[]", elem)
        }
        AstType::Tuple(tuple) => {
            let elems = tuple
                .element_types
                .iter()
                .map(|t| render_type_annotation(t, source, interner))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", elems)
        }
        AstType::Object(obj) => {
            let members = obj
                .members
                .iter()
                .map(|m| match m {
                    ObjectTypeMember::Property(prop) => {
                        let name = interner.resolve(prop.name.name).to_string();
                        let maybe = if prop.optional {
                            format!("{}?", name)
                        } else {
                            name
                        };
                        let ty = render_type_annotation(&prop.ty, source, interner);
                        format!("{}: {}", maybe, ty)
                    }
                    ObjectTypeMember::Method(method) => {
                        let name = interner.resolve(method.name.name).to_string();
                        let params = method
                            .params
                            .iter()
                            .enumerate()
                            .map(|(idx, p)| {
                                let pname = p
                                    .name
                                    .as_ref()
                                    .map(|id| interner.resolve(id.name).to_string())
                                    .unwrap_or_else(|| format!("arg{}", idx));
                                let head = if p.is_rest {
                                    format!("...{}", pname)
                                } else if p.optional {
                                    format!("{}?", pname)
                                } else {
                                    pname
                                };
                                let pty = render_type_annotation(&p.ty, source, interner);
                                format!("{}: {}", head, pty)
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        let ret = render_type_annotation(&method.return_type, source, interner);
                        format!("{}: ({}) => {}", name, params, ret)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {} }}", members)
        }
        AstType::Typeof(typeof_ty) => {
            let expr = typeof_ty.argument.span().slice(source).trim();
            format!("typeof {}", expr)
        }
        AstType::Keyof(keyof_ty) => {
            let inner = render_type_annotation(&keyof_ty.target, source, interner);
            format!("keyof {}", inner)
        }
        AstType::IndexedAccess(indexed) => {
            let object = render_type_annotation(&indexed.object, source, interner);
            let index = render_type_annotation(&indexed.index, source, interner);
            format!("{}[{}]", object, index)
        }
        AstType::StringLiteral(sym) => format!("\"{}\"", escape_string(interner.resolve(*sym))),
        AstType::NumberLiteral(n) => {
            if (n.fract()).abs() < f64::EPSILON {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        AstType::BooleanLiteral(b) => b.to_string(),
        AstType::Parenthesized(inner) => {
            let rendered = render_type_annotation(inner, source, interner);
            format!("({})", rendered)
        }
    }
}

fn rewrite_local_class_refs(type_expr: &str, class_aliases: &HashMap<String, String>) -> String {
    if class_aliases.is_empty() {
        return type_expr.to_string();
    }
    let mut result = String::new();
    let mut token = String::new();
    for ch in type_expr.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if !token.is_empty() {
                if let Some(alias) = class_aliases.get(&token) {
                    result.push_str(alias);
                } else if looks_like_class_identifier(&token)
                    && !is_known_global_class(&token)
                    && !token.starts_with("__t_")
                {
                    result.push_str("unknown");
                } else {
                    result.push_str(&token);
                }
                token.clear();
            }
            result.push(ch);
        }
    }
    if !token.is_empty() {
        if let Some(alias) = class_aliases.get(&token) {
            result.push_str(alias);
        } else if looks_like_class_identifier(&token)
            && !is_known_global_class(&token)
            && !token.starts_with("__t_")
        {
            result.push_str("unknown");
        } else {
            result.push_str(&token);
        }
    }
    result
}

fn strip_export_prefix(src: &str) -> &str {
    let trimmed = src.trim_start();
    if let Some(rest) = trimmed.strip_prefix("export") {
        rest.trim_start()
    } else {
        src
    }
}
