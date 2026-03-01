use crate::error::RuntimeError;
use raya_engine::compiler::module::StdModuleRegistry;
use raya_engine::parser::ast::{
    ExportDecl, FunctionDecl, ImportSpecifier, Pattern, Statement, TypeAliasDecl,
};
use raya_engine::parser::{Interner, Parser};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct StdPreludeOutput {
    pub prelude_source: String,
    pub rewritten_user_source: String,
}

#[derive(Debug, Clone)]
struct ModuleMeta {
    value_exports: BTreeMap<String, String>,
    type_exports: BTreeMap<String, String>,
    function_exports: BTreeMap<String, FunctionSig>,
    class_types: BTreeMap<String, ClassTypeMeta>,
}

#[derive(Debug, Clone)]
struct ExportBinding {
    value_expr: String,
    type_expr: String,
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<FunctionParamSig>,
    return_type: String,
}

#[derive(Debug, Clone)]
struct FunctionParamSig {
    name: String,
    ty: String,
    optional: bool,
    has_default: bool,
    is_rest: bool,
}

#[derive(Debug, Clone)]
struct ClassTypeMeta {
    type_params: Vec<String>,
    properties: Vec<(String, String)>,
    methods: Vec<(String, FunctionSig)>,
}

#[derive(Clone)]
struct ParsedStdModule {
    canonical: String,
    source: String,
    ast: raya_engine::parser::ast::Module,
    interner: Interner,
    deps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Visited,
}

#[derive(Debug, Clone)]
enum BindingSpec {
    Default { local: String },
    Named { imported: String, local: String },
    Namespace { local: String },
}

#[derive(Debug, Clone)]
struct UserStdImport {
    canonical: String,
    source_specifier: String,
    bindings: Vec<BindingSpec>,
}

#[derive(Debug, Clone, Copy)]
enum BindingContext {
    Internal,
    User,
}

pub fn build_std_prelude(source: &str) -> Result<StdPreludeOutput, RuntimeError> {
    let parser =
        Parser::new(source).map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, 0)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, 0)))?;

    let registry = StdModuleRegistry::new();
    let mut user_imports: Vec<UserStdImport> = Vec::new();
    let mut parsed_modules: HashMap<String, ParsedStdModule> = HashMap::new();
    let mut topo_order: Vec<String> = Vec::new();
    let mut visit_state: HashMap<String, VisitState> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();

    for stmt in &ast.statements {
        let Statement::ImportDecl(import) = stmt else {
            continue;
        };
        let specifier = interner.resolve(import.source.value).to_string();
        if !StdModuleRegistry::is_std_import(&specifier) {
            continue;
        }

        let canonical = resolve_and_visit_module(
            &registry,
            &specifier,
            &mut parsed_modules,
            &mut topo_order,
            &mut visit_state,
            &mut stack,
        )?;

        user_imports.push(UserStdImport {
            canonical,
            source_specifier: specifier,
            bindings: convert_import_specifiers(&import.specifiers, &interner),
        });
    }

    if user_imports.is_empty() {
        return Ok(StdPreludeOutput {
            prelude_source: String::new(),
            rewritten_user_source: source.to_string(),
        });
    }

    let mut prelude = String::new();
    let mut module_meta: HashMap<String, ModuleMeta> = HashMap::new();

    for canonical in &topo_order {
        let parsed = parsed_modules.get(canonical).ok_or_else(|| {
            RuntimeError::Dependency(format!(
                "Internal error: missing parsed module metadata for '{}'",
                canonical
            ))
        })?;
        let (module_code, meta) = transform_std_module(parsed, &module_meta)?;
        prelude.push_str(&module_code);
        prelude.push('\n');
        module_meta.insert(canonical.clone(), meta);
    }

    let mut emitted_user_bindings: HashMap<String, String> = HashMap::new();
    for import in &user_imports {
        let meta = module_meta.get(&import.canonical).ok_or_else(|| {
            RuntimeError::Dependency(format!(
                "Internal error: missing export metadata for std module '{}'",
                import.canonical
            ))
        })?;
        let mut deduped_bindings = Vec::new();
        for binding in &import.bindings {
            let local = binding_local_name(binding);
            let identity = binding_identity(binding, &import.canonical);
            if let Some(previous) = emitted_user_bindings.get(local) {
                if previous == &identity {
                    // Flattened local-import sources can repeat the same std import in
                    // multiple files; emit each binding only once.
                    continue;
                }
                return Err(RuntimeError::Dependency(format!(
                    "Conflicting std import binding for '{}': '{}' vs '{}'",
                    local, previous, identity
                )));
            }
            emitted_user_bindings.insert(local.to_string(), identity);
            deduped_bindings.push(binding.clone());
        }
        if deduped_bindings.is_empty() {
            continue;
        }
        prelude.push_str(&emit_binding_lines(
            &deduped_bindings,
            &import.canonical,
            meta,
            &import.source_specifier,
            BindingContext::User,
            None,
            Some(source),
        )?);
    }

    let output = StdPreludeOutput {
        prelude_source: prelude,
        rewritten_user_source: strip_user_std_imports(source, &ast, &interner),
    };
    if let Ok(path) = std::env::var("RAYA_DEBUG_DUMP_SOURCE") {
        let merged = if output.prelude_source.is_empty() {
            output.rewritten_user_source.clone()
        } else {
            format!(
                "{}\n{}",
                output.prelude_source, output.rewritten_user_source
            )
        };
        let _ = std::fs::write(path, merged);
    }
    Ok(output)
}

fn resolve_and_visit_module(
    registry: &StdModuleRegistry,
    specifier: &str,
    parsed_modules: &mut HashMap<String, ParsedStdModule>,
    topo_order: &mut Vec<String>,
    visit_state: &mut HashMap<String, VisitState>,
    stack: &mut Vec<String>,
) -> Result<String, RuntimeError> {
    let (canonical, source) = registry.resolve_specifier(specifier).ok_or_else(|| {
        if StdModuleRegistry::is_node_import(specifier) {
            unsupported_node_import_error(specifier)
        } else {
            RuntimeError::Dependency(format!("Unknown std module import '{}'", specifier))
        }
    })?;

    if let Some(state) = visit_state.get(&canonical).copied() {
        match state {
            VisitState::Visited => return Ok(canonical),
            VisitState::Visiting => {
                let cycle_idx = stack
                    .iter()
                    .position(|name| name == &canonical)
                    .unwrap_or(0);
                let mut chain: Vec<String> = stack[cycle_idx..].to_vec();
                chain.push(canonical.clone());
                return Err(RuntimeError::Dependency(format!(
                    "Circular std module dependency detected: {}",
                    chain.join(" -> ")
                )));
            }
        }
    }

    visit_state.insert(canonical.clone(), VisitState::Visiting);
    stack.push(canonical.clone());

    let parser =
        Parser::new(source).map_err(|errors| RuntimeError::Lex(format_lex_errors(&errors, 0)))?;
    let (ast, interner) = parser
        .parse()
        .map_err(|errors| RuntimeError::Parse(format_parse_errors(&errors, 0)))?;

    let mut deps = Vec::new();
    let mut seen = HashSet::new();
    for stmt in &ast.statements {
        let Statement::ImportDecl(import) = stmt else {
            continue;
        };
        let child_specifier = interner.resolve(import.source.value).to_string();
        if !StdModuleRegistry::is_std_import(&child_specifier) {
            return Err(RuntimeError::Dependency(format!(
                "Unsupported non-std import '{}' in std module '{}'",
                child_specifier, canonical
            )));
        }
        let child_canonical = resolve_and_visit_module(
            registry,
            &child_specifier,
            parsed_modules,
            topo_order,
            visit_state,
            stack,
        )?;
        if seen.insert(child_canonical.clone()) {
            deps.push(child_canonical);
        }
    }

    parsed_modules.insert(
        canonical.clone(),
        ParsedStdModule {
            canonical: canonical.clone(),
            source: source.to_string(),
            ast,
            interner,
            deps,
        },
    );

    stack.pop();
    visit_state.insert(canonical.clone(), VisitState::Visited);
    topo_order.push(canonical.clone());
    Ok(canonical)
}

fn convert_import_specifiers(
    specifiers: &[ImportSpecifier],
    interner: &Interner,
) -> Vec<BindingSpec> {
    let mut out = Vec::new();
    for spec in specifiers {
        match spec {
            ImportSpecifier::Default(local) => out.push(BindingSpec::Default {
                local: interner.resolve(local.name).to_string(),
            }),
            ImportSpecifier::Named { name, alias } => {
                let imported = interner.resolve(name.name).to_string();
                let local = alias
                    .as_ref()
                    .map(|a| interner.resolve(a.name).to_string())
                    .unwrap_or_else(|| imported.clone());
                out.push(BindingSpec::Named { imported, local });
            }
            ImportSpecifier::Namespace(local) => out.push(BindingSpec::Namespace {
                local: interner.resolve(local.name).to_string(),
            }),
        }
    }
    out
}

fn binding_local_name(binding: &BindingSpec) -> &str {
    match binding {
        BindingSpec::Default { local } => local,
        BindingSpec::Named { local, .. } => local,
        BindingSpec::Namespace { local } => local,
    }
}

fn binding_identity(binding: &BindingSpec, canonical: &str) -> String {
    match binding {
        BindingSpec::Default { .. } => format!("{}::default", canonical),
        BindingSpec::Named { imported, .. } => format!("{}::named::{}", canonical, imported),
        BindingSpec::Namespace { .. } => format!("{}::namespace", canonical),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentifierUsage {
    TypeOnly,
    ValueOnly,
    Both,
}

fn infer_identifier_usage(source: &str, ident: &str) -> IdentifierUsage {
    let value_patterns = [
        format!("new {}", ident),
        format!("{}(", ident),
        format!("{}.", ident),
    ];
    let type_patterns = [
        format!(": {}", ident),
        format!("<{}>", ident),
        format!("<{},", ident),
        format!(", {}>", ident),
    ];
    let value_used = value_patterns.iter().any(|p| source.contains(p));
    let type_used = type_patterns.iter().any(|p| source.contains(p));
    match (type_used, value_used) {
        (true, true) => IdentifierUsage::Both,
        (true, false) => IdentifierUsage::TypeOnly,
        (false, true) => IdentifierUsage::ValueOnly,
        (false, false) => IdentifierUsage::ValueOnly,
    }
}

fn transform_std_module(
    module: &ParsedStdModule,
    module_meta: &HashMap<String, ModuleMeta>,
) -> Result<(String, ModuleMeta), RuntimeError> {
    let mut transformed = String::new();
    let hoisted_decls = module
        .ast
        .statements
        .iter()
        .filter_map(|stmt| hoisted_declaration_source(stmt, module))
        .collect::<Vec<_>>();
    let mut hoisted_emitted = false;
    let mut cursor = 0usize;
    let mut exports: BTreeMap<String, ExportBinding> = BTreeMap::new();
    let mut type_exports: BTreeMap<String, String> = BTreeMap::new();
    let mut function_exports: BTreeMap<String, FunctionSig> = BTreeMap::new();
    let mut local_function_sigs: BTreeMap<String, FunctionSig> = BTreeMap::new();
    let mut local_type_names: HashSet<String> = HashSet::new();
    let mut module_allowed_types: HashSet<String> = HashSet::new();
    let mut explicit_named_type_locals: HashSet<String> = HashSet::new();
    let mut class_types: BTreeMap<String, ClassTypeMeta> = BTreeMap::new();
    let mut local_var_types: BTreeMap<String, String> = BTreeMap::new();
    for stmt in &module.ast.statements {
        let Statement::ImportDecl(import) = stmt else {
            continue;
        };
        let specifier = module.interner.resolve(import.source.value).to_string();
        if !StdModuleRegistry::is_std_import(&specifier) {
            continue;
        }
        let dep_canonical = resolve_canonical_from_deps(module, &specifier)?;
        let Some(dep_meta) = module_meta.get(&dep_canonical) else {
            continue;
        };
        for binding in convert_import_specifiers(&import.specifiers, &module.interner) {
            if let BindingSpec::Named { imported, local } = binding {
                if dep_meta.type_exports.contains_key(&imported) {
                    explicit_named_type_locals.insert(local);
                }
            }
        }
    }
    let module_tag = sanitize_module_specifier(&module.canonical);

    for stmt in &module.ast.statements {
        let hoisted_decl = hoisted_declaration_source(stmt, module).is_some();
        if !hoisted_emitted && !matches!(stmt, Statement::ImportDecl(_)) {
            for decl in &hoisted_decls {
                transformed.push_str(decl);
                transformed.push('\n');
            }
            if !hoisted_decls.is_empty() {
                transformed.push('\n');
            }
            hoisted_emitted = true;
        }
        let span = *stmt.span();
        if cursor <= span.start && span.start <= module.source.len() {
            let prefix = &module.source[cursor..span.start];
            if matches!(stmt, Statement::ExportDecl(ExportDecl::Declaration(_))) {
                transformed.push_str(&strip_trailing_export_token(prefix));
            } else {
                transformed.push_str(prefix);
            }
        }

        match stmt {
            Statement::ImportDecl(import) => {
                let specifier = module.interner.resolve(import.source.value).to_string();
                if !StdModuleRegistry::is_std_import(&specifier) {
                    return Err(RuntimeError::Dependency(format!(
                        "Unsupported non-std import '{}' in std module '{}'",
                        specifier, module.canonical
                    )));
                }
                let dep_canonical = resolve_canonical_from_deps(module, &specifier)?;
                let dep_meta = module_meta.get(&dep_canonical).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Internal error: dependency '{}' not available while transforming '{}'",
                        dep_canonical, module.canonical
                    ))
                })?;
                let bindings = convert_import_specifiers(&import.specifiers, &module.interner);
                let mut named_locals = HashSet::new();
                let has_default_or_namespace = bindings.iter().any(|binding| {
                    matches!(
                        binding,
                        BindingSpec::Default { .. } | BindingSpec::Namespace { .. }
                    )
                });
                for binding in &bindings {
                    if let BindingSpec::Named { imported, local } = binding {
                        named_locals.insert(local.clone());
                        if dep_meta.type_exports.contains_key(imported) {
                            module_allowed_types.insert(local.clone());
                        }
                    }
                }
                if has_default_or_namespace {
                    for (type_name, type_expr) in &dep_meta.type_exports {
                        if is_identifier(type_name)
                            && !module_allowed_types.contains(type_name)
                            && !named_locals.contains(type_name)
                            && !explicit_named_type_locals.contains(type_name)
                        {
                            transformed.push_str(&format!("type {} = {};\n", type_name, type_expr));
                            module_allowed_types.insert(type_name.clone());
                        }
                    }
                }
                transformed.push_str(&emit_binding_lines(
                    &bindings,
                    &dep_canonical,
                    dep_meta,
                    &specifier,
                    BindingContext::Internal,
                    Some(&module_allowed_types),
                    Some(&module.source),
                )?);
            }
            Statement::ExportDecl(export) => {
                transform_export_decl(
                    export,
                    module,
                    module_meta,
                    !hoisted_decl,
                    &mut transformed,
                    &mut exports,
                    &mut type_exports,
                    &mut function_exports,
                    &local_function_sigs,
                    &mut local_type_names,
                    &module_tag,
                    &mut class_types,
                    &local_var_types,
                )?;
            }
            _ => {
                if let Statement::FunctionDecl(function) = stmt {
                    let name = module.interner.resolve(function.name.name).to_string();
                    local_function_sigs.insert(name, function_sig(function, module));
                }
                if let Statement::ClassDecl(class) = stmt {
                    let name = module.interner.resolve(class.name.name).to_string();
                    local_type_names.insert(name.clone());
                    module_allowed_types.insert(name.clone());
                    class_types.insert(name, extract_class_type_meta(class, module));
                }
                if let Statement::TypeAliasDecl(alias) = stmt {
                    let name = module.interner.resolve(alias.name.name).to_string();
                    local_type_names.insert(name.clone());
                    module_allowed_types.insert(name);
                }
                if let Statement::VariableDecl(v) = stmt {
                    let var_type = rewrite_local_class_refs(
                        &variable_decl_type_expr(v, module),
                        &local_type_names,
                        &module_tag,
                    );
                    if let Pattern::Identifier(id) = &v.pattern {
                        let var_name = module.interner.resolve(id.name).to_string();
                        local_var_types.insert(var_name, var_type);
                    }
                }
                if !hoisted_decl {
                    transformed.push_str(span.slice(&module.source));
                }
            }
        }

        cursor = span.end;
    }

    if cursor <= module.source.len() {
        transformed.push_str(&module.source[cursor..]);
    }

    // Build type reference map: local classes + imported types from dependencies
    let mut type_ref_map: BTreeMap<String, String> = BTreeMap::new();
    for class_name in class_types.keys() {
        type_ref_map.insert(
            class_name.clone(),
            format!("__t_{}_{}", module_tag, class_name),
        );
    }
    for dep_canonical in &module.deps {
        if let Some(dep_meta) = module_meta.get(dep_canonical) {
            for (name, type_expr) in &dep_meta.type_exports {
                if !type_ref_map.contains_key(name) {
                    type_ref_map.insert(name.clone(), type_expr.clone());
                }
            }
        }
    }

    // Synthesize top-level type aliases for classes
    let type_alias_block = synthesize_type_aliases(&module_tag, &class_types, &type_ref_map);

    let export_var = export_var_name(&module.canonical);
    let mut export_pairs: Vec<String> = Vec::new();
    let mut export_type_fields: Vec<String> = Vec::new();
    for (name, binding) in &exports {
        export_pairs.push(format!(
            "\"{}\": {}",
            escape_string(name),
            binding.value_expr
        ));
        export_type_fields.push(format!("{}: {}", type_field_key(name), binding.type_expr));
    }

    let export_literal = format!("{{{}}}", export_pairs.join(", "));
    let export_type = if export_type_fields.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", export_type_fields.join(", "))
    };
    let module_fn = format!("__std_module_{}", module_tag);
    let export_type_alias = format!("__std_exports_type_{}", module_tag);

    // Wrapper function needs a return type annotation (checker infers void otherwise).
    let wrapped = format!(
        "{}type {} = {};\nfunction {}(): {} {{\n{}\nreturn {};\n}}\nconst {} = {}();\n",
        type_alias_block,
        export_type_alias,
        export_type,
        module_fn,
        export_type_alias,
        transformed,
        export_literal,
        export_var,
        module_fn
    );

    let value_exports = exports
        .iter()
        .map(|(name, binding)| (name.clone(), binding.type_expr.clone()))
        .collect::<BTreeMap<_, _>>();

    Ok((
        wrapped,
        ModuleMeta {
            value_exports,
            type_exports,
            function_exports,
            class_types,
        },
    ))
}

fn transform_export_decl(
    export: &ExportDecl,
    module: &ParsedStdModule,
    module_meta: &HashMap<String, ModuleMeta>,
    emit_decl_source: bool,
    out: &mut String,
    exports: &mut BTreeMap<String, ExportBinding>,
    type_exports: &mut BTreeMap<String, String>,
    function_exports: &mut BTreeMap<String, FunctionSig>,
    local_function_sigs: &BTreeMap<String, FunctionSig>,
    local_type_names: &mut HashSet<String>,
    module_tag: &str,
    class_types: &mut BTreeMap<String, ClassTypeMeta>,
    local_var_types: &BTreeMap<String, String>,
) -> Result<(), RuntimeError> {
    match export {
        ExportDecl::Declaration(inner) => {
            if emit_decl_source {
                let export_span = *export.span();
                out.push_str(&strip_export_prefix(export_span.slice(&module.source)));
            }
            match inner.as_ref() {
                Statement::FunctionDecl(f) => {
                    let name = module.interner.resolve(f.name.name).to_string();
                    let sig = function_sig(f, module);
                    let ty = rewrite_local_class_refs(
                        &function_type_from_sig(&sig),
                        local_type_names,
                        module_tag,
                    );
                    // Keep as both local and exported function signature.
                    function_exports.insert(name.clone(), sig);
                    exports.insert(
                        name.clone(),
                        ExportBinding {
                            value_expr: name,
                            type_expr: ty,
                        },
                    );
                }
                Statement::ClassDecl(c) => {
                    let name = module.interner.resolve(c.name.name).to_string();
                    let class_alias = format!("__t_{}_{}", module_tag, name);
                    local_type_names.insert(name.clone());
                    type_exports.insert(name.clone(), class_alias.clone());
                    class_types.insert(name.clone(), extract_class_type_meta(c, module));
                    exports.insert(
                        name.clone(),
                        ExportBinding {
                            value_expr: name.clone(),
                            type_expr: class_alias,
                        },
                    );
                }
                Statement::VariableDecl(v) => {
                    let var_type = rewrite_local_class_refs(
                        &variable_decl_type_expr(v, module),
                        local_type_names,
                        module_tag,
                    );
                    collect_pattern_identifiers(
                        &v.pattern,
                        &module.interner,
                        &mut |name| ExportBinding {
                            value_expr: name.to_string(),
                            type_expr: var_type.clone(),
                        },
                        exports,
                    );
                }
                Statement::TypeAliasDecl(alias) => {
                    let name = module.interner.resolve(alias.name.name).to_string();
                    local_type_names.insert(name.clone());
                    type_exports.insert(name, alias_type_expr(alias, module));
                }
                _ => {}
            }
        }
        ExportDecl::Named {
            specifiers, source, ..
        } => {
            if let Some(src) = source {
                let source_specifier = module.interner.resolve(src.value).to_string();
                if !StdModuleRegistry::is_std_import(&source_specifier) {
                    return Err(RuntimeError::Dependency(format!(
                        "Unsupported non-std re-export '{}' in std module '{}'",
                        source_specifier, module.canonical
                    )));
                }
                let dep_canonical = resolve_canonical_from_deps(module, &source_specifier)?;
                let dep_meta = module_meta.get(&dep_canonical).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Internal error: dependency '{}' not available while transforming '{}'",
                        dep_canonical, module.canonical
                    ))
                })?;
                for spec in specifiers {
                    let imported = module.interner.resolve(spec.name.name).to_string();
                    let exported = spec
                        .alias
                        .as_ref()
                        .map(|a| module.interner.resolve(a.name).to_string())
                        .unwrap_or_else(|| imported.clone());

                    if let Some(type_expr) = dep_meta.type_exports.get(&imported) {
                        type_exports.insert(exported, type_expr.clone());
                        continue;
                    }
                    let dep_type_expr = dep_meta.value_exports.get(&imported).ok_or_else(|| {
                        RuntimeError::Dependency(format!(
                            "Unknown re-export '{}' from '{}' in std module '{}'",
                            imported, source_specifier, module.canonical
                        ))
                    })?;

                    let accessor = property_accessor(&export_var_name(&dep_canonical), &imported);
                    if let Some(sig) = dep_meta.function_exports.get(&imported) {
                        function_exports.insert(exported.clone(), sig.clone());
                    }
                    exports.insert(
                        exported,
                        ExportBinding {
                            value_expr: accessor,
                            type_expr: dep_type_expr.clone(),
                        },
                    );
                }
            } else {
                for spec in specifiers {
                    let imported = module.interner.resolve(spec.name.name).to_string();
                    let exported = spec
                        .alias
                        .as_ref()
                        .map(|a| module.interner.resolve(a.name).to_string())
                        .unwrap_or_else(|| imported.clone());

                    if let Some(type_expr) = type_exports.get(&imported).cloned() {
                        type_exports.insert(exported, type_expr);
                        continue;
                    }
                    let mut export_type_expr = "unknown".to_string();
                    if local_type_names.contains(&imported) {
                        let class_type = format!("__t_{}_{}", module_tag, imported);
                        type_exports.insert(exported.clone(), class_type.clone());
                        export_type_expr = class_type;
                    }
                    if let Some(sig) = local_function_sigs.get(&imported) {
                        function_exports.insert(exported.clone(), sig.clone());
                    }

                    exports.insert(
                        exported,
                        ExportBinding {
                            value_expr: imported.clone(),
                            type_expr: export_type_expr,
                        },
                    );
                }
            }
        }
        ExportDecl::All { source, .. } => {
            let source_specifier = module.interner.resolve(source.value).to_string();
            return Err(RuntimeError::Dependency(format!(
                "Unsupported export-all '{}' in std module '{}'",
                source_specifier, module.canonical
            )));
        }
        ExportDecl::Default { expression, .. } => {
            let default_var = format!("__std_default_export_{}", module_tag);
            let expr_src = expression.span().slice(&module.source).trim().to_string();

            // Infer default export type from expression
            let type_expr = if let Some(class_name) =
                expr_src.strip_prefix("new ").and_then(|rest| {
                    rest.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .next()
                }) {
                if local_type_names.contains(class_name) {
                    format!("__t_{}_{}", module_tag, class_name)
                } else {
                    "unknown".to_string()
                }
            } else if let Some(var_type) = local_var_types.get(&expr_src) {
                if var_type.starts_with("__t_") {
                    var_type.clone()
                } else if local_type_names.contains(var_type) {
                    format!("__t_{}_{}", module_tag, var_type)
                } else {
                    "unknown".to_string()
                }
            } else {
                "unknown".to_string()
            };

            out.push_str(&format!("const {} = {};\n", default_var, expr_src));
            exports.insert(
                "default".to_string(),
                ExportBinding {
                    value_expr: default_var.clone(),
                    type_expr,
                },
            );
        }
    }

    out.push('\n');
    Ok(())
}

fn collect_pattern_identifiers<F>(
    pattern: &Pattern,
    interner: &Interner,
    make_binding: &mut F,
    exports: &mut BTreeMap<String, ExportBinding>,
) where
    F: FnMut(&str) -> ExportBinding,
{
    match pattern {
        Pattern::Identifier(id) => {
            let name = interner.resolve(id.name).to_string();
            exports.insert(name.clone(), make_binding(&name));
        }
        Pattern::Array(arr) => {
            for element in &arr.elements {
                if let Some(elem) = element {
                    collect_pattern_identifiers(&elem.pattern, interner, make_binding, exports);
                }
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_identifiers(rest, interner, make_binding, exports);
            }
        }
        Pattern::Object(obj) => {
            for prop in &obj.properties {
                collect_pattern_identifiers(&prop.value, interner, make_binding, exports);
            }
            if let Some(rest) = &obj.rest {
                let name = interner.resolve(rest.name).to_string();
                exports.insert(name.clone(), make_binding(&name));
            }
        }
        Pattern::Rest(rest) => {
            collect_pattern_identifiers(&rest.argument, interner, make_binding, exports)
        }
    }
}

fn emit_binding_lines(
    bindings: &[BindingSpec],
    canonical: &str,
    meta: &ModuleMeta,
    source_specifier: &str,
    context: BindingContext,
    allowed_types: Option<&HashSet<String>>,
    usage_source: Option<&str>,
) -> Result<String, RuntimeError> {
    let mut out = String::new();
    let export_var = export_var_name(canonical);

    for binding in bindings {
        match binding {
            BindingSpec::Default { local } => {
                if !meta.value_exports.contains_key("default") {
                    return Err(RuntimeError::Dependency(format!(
                        "std module '{}' has no default export for import '{}'",
                        source_specifier, local
                    )));
                }
                let default_accessor = property_accessor(&export_var, "default");
                if let Some(default_type) = meta.value_exports.get("default") {
                    if default_type != "unknown" {
                        if canonical.starts_with("node_")
                            || canonical.starts_with("_node_")
                            || canonical.starts_with("__node_")
                        {
                            out.push_str(&format!("const {} = {};\n", local, default_accessor));
                        } else {
                            out.push_str(&format!(
                                "const {} = ({} as {});\n",
                                local, default_accessor, default_type
                            ));
                        }
                    } else if has_named_value_exports(meta)
                        && !canonical.starts_with("node_")
                        && !canonical.starts_with("_node_")
                        && !canonical.starts_with("__node_")
                    {
                        out.push_str(&emit_default_object_binding(local, &export_var, meta));
                    } else {
                        out.push_str(&format!("const {} = {};\n", local, default_accessor));
                    }
                } else if !meta.function_exports.is_empty() {
                    out.push_str(&emit_default_wrapper(
                        local,
                        &export_var,
                        meta,
                        matches!(context, BindingContext::User),
                        allowed_types,
                    ));
                } else {
                    out.push_str(&format!("const {} = {};\n", local, default_accessor));
                }
            }
            BindingSpec::Named { imported, local } => {
                let accessor = property_accessor(&export_var, imported);
                if let Some(type_expr) = meta.type_exports.get(imported) {
                    let usage = usage_source
                        .map(|src| infer_identifier_usage(src, local))
                        .unwrap_or(IdentifierUsage::Both);
                    let should_emit_type_alias = !matches!(usage, IdentifierUsage::ValueOnly);
                    if should_emit_type_alias {
                        out.push_str(&format!("type {} = {};\n", local, type_expr));
                    }
                    if matches!(usage, IdentifierUsage::TypeOnly) {
                        continue;
                    }
                }
                if !meta.value_exports.contains_key(imported) {
                    return Err(RuntimeError::Dependency(format!(
                        "std module '{}' has no named export '{}'",
                        source_specifier, imported
                    )));
                }
                if let Some(sig) = meta.function_exports.get(imported) {
                    let maybe_sig = if matches!(context, BindingContext::User) {
                        sanitize_wrapper_sig(sig, allowed_types)
                    } else {
                        sig.clone()
                    };
                    out.push_str(&format!(
                        "const {} = (({} as {{ {}: unknown }}).{} as {});\n",
                        local,
                        export_var,
                        imported,
                        imported,
                        function_type_from_sig(&maybe_sig)
                    ));
                } else if meta.class_types.contains_key(imported) {
                    let class_type = meta
                        .value_exports
                        .get(imported)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    out.push_str(&format!(
                        "const {} = ({} as {});\n",
                        local, accessor, class_type
                    ));
                } else {
                    out.push_str(&format!("const {} = {};\n", local, accessor));
                }
            }
            BindingSpec::Namespace { local } => {
                if meta.function_exports.is_empty() {
                    if has_named_value_exports(meta) {
                        out.push_str(&emit_default_object_binding(local, &export_var, meta));
                    } else {
                        out.push_str(&format!("const {} = {};\n", local, export_var));
                    }
                } else {
                    out.push_str(&emit_default_wrapper(
                        local,
                        &export_var,
                        meta,
                        matches!(context, BindingContext::User),
                        allowed_types,
                    ));
                }
            }
        }
    }

    Ok(out)
}

fn resolve_canonical_from_deps(
    module: &ParsedStdModule,
    source_specifier: &str,
) -> Result<String, RuntimeError> {
    let registry = StdModuleRegistry::new();
    let (canonical, _) = registry
        .resolve_specifier(source_specifier)
        .ok_or_else(|| {
            if StdModuleRegistry::is_node_import(source_specifier) {
                unsupported_node_import_error(source_specifier)
            } else {
                RuntimeError::Dependency(format!(
                    "Unknown std module import '{}' in '{}'",
                    source_specifier, module.canonical
                ))
            }
        })?;

    if module.deps.iter().any(|dep| dep == &canonical) {
        Ok(canonical)
    } else {
        Err(RuntimeError::Dependency(format!(
            "Internal error: '{}' import '{}' not present in resolved dependency list",
            module.canonical, source_specifier
        )))
    }
}

fn strip_user_std_imports(
    source: &str,
    ast: &raya_engine::parser::ast::Module,
    interner: &Interner,
) -> String {
    let mut out = String::new();
    let mut cursor = 0usize;

    for stmt in &ast.statements {
        let Statement::ImportDecl(import) = stmt else {
            continue;
        };
        let specifier = interner.resolve(import.source.value).to_string();
        if !StdModuleRegistry::is_std_import(&specifier) {
            continue;
        }

        if cursor <= import.span.start && import.span.start <= source.len() {
            out.push_str(&source[cursor..import.span.start]);
        }
        cursor = import.span.end;
    }

    if cursor <= source.len() {
        out.push_str(&source[cursor..]);
    }
    out
}

fn strip_trailing_export_token(prefix: &str) -> String {
    let trimmed_end = prefix.trim_end_matches(|c: char| c.is_ascii_whitespace());
    if let Some(before_export) = trimmed_end.strip_suffix("export") {
        return before_export.to_string();
    }
    prefix.to_string()
}

fn strip_export_prefix(src: &str) -> String {
    let trimmed = src.trim_start_matches(|c: char| c.is_ascii_whitespace());
    if let Some(rest) = trimmed.strip_prefix("export") {
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
        return rest.to_string();
    }
    src.to_string()
}

fn type_field_key(name: &str) -> String {
    if is_identifier(name) {
        name.to_string()
    } else {
        format!("\"{}\"", escape_string(name))
    }
}

fn export_var_name(canonical: &str) -> String {
    format!("__std_exports_{}", sanitize_module_specifier(canonical))
}

fn sanitize_module_specifier(specifier: &str) -> String {
    let mut out = String::new();
    for ch in specifier.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn property_accessor(obj: &str, prop: &str) -> String {
    if is_identifier(prop) {
        format!("{}.{}", obj, prop)
    } else {
        format!("{}[\"{}\"]", obj, escape_string(prop))
    }
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn unsupported_node_import_error(specifier: &str) -> RuntimeError {
    let supported = StdModuleRegistry::supported_node_module_names()
        .map(|name| format!("node:{}", name))
        .collect::<Vec<_>>()
        .join(", ");
    RuntimeError::Dependency(format!(
        "Unsupported node module import '{}'. Supported node modules: {}",
        specifier, supported
    ))
}

fn format_lex_errors(errors: &[raya_engine::parser::LexError], prefix_lines: usize) -> String {
    let _ = prefix_lines;
    errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_parse_errors(errors: &[raya_engine::parser::ParseError], prefix_lines: usize) -> String {
    let _ = prefix_lines;
    errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn function_sig(function: &FunctionDecl, module: &ParsedStdModule) -> FunctionSig {
    let params = function
        .params
        .iter()
        .enumerate()
        .map(|(idx, param)| {
            let name = match &param.pattern {
                Pattern::Identifier(id) => module.interner.resolve(id.name).to_string(),
                _ => format!("arg{}", idx),
            };
            FunctionParamSig {
                name,
                ty: param
                    .type_annotation
                    .as_ref()
                    .map(|ann| normalize_type_snippet(ann.span.slice(&module.source)))
                    .unwrap_or_else(|| "unknown".to_string()),
                optional: param.optional,
                has_default: param.default_value.is_some(),
                is_rest: param.is_rest,
            }
        })
        .collect::<Vec<_>>();

    let return_type = function
        .return_type
        .as_ref()
        .map(|ann| normalize_type_snippet(ann.span.slice(&module.source)))
        .unwrap_or_else(|| "void".to_string());

    FunctionSig {
        params,
        return_type,
    }
}

fn alias_type_expr(alias: &TypeAliasDecl, module: &ParsedStdModule) -> String {
    normalize_type_snippet(alias.type_annotation.span.slice(&module.source))
}

fn function_type_from_sig(sig: &FunctionSig) -> String {
    let params = sig
        .params
        .iter()
        .map(|p| {
            let name = if p.is_rest {
                format!("...{}", p.name)
            } else if p.optional || p.has_default {
                format!("{}?", p.name)
            } else {
                p.name.clone()
            };
            format!("{}: {}", name, p.ty)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("({}) => {}", params, sig.return_type)
}

fn emit_default_wrapper(
    local: &str,
    export_var: &str,
    meta: &ModuleMeta,
    sanitize_types: bool,
    allowed_types: Option<&HashSet<String>>,
) -> String {
    let class_name = format!("__StdDefault_{}", sanitize_module_specifier(local));
    let mut out = String::new();
    out.push_str(&format!("class {} {{\n", class_name));
    for (name, sig) in &meta.function_exports {
        let safe_sig = if sanitize_types {
            sanitize_wrapper_sig(sig, allowed_types)
        } else {
            sig.clone()
        };
        let param_decl = sig
            .params
            .iter()
            .zip(safe_sig.params.iter())
            .map(|(orig, safe)| {
                let name = if orig.is_rest {
                    format!("...{}", orig.name)
                } else if orig.optional || orig.has_default {
                    format!("{}?", orig.name)
                } else {
                    orig.name.clone()
                };
                format!("{}: {}", name, safe.ty)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let call_args = sig
            .params
            .iter()
            .map(|p| {
                if p.is_rest {
                    format!("...{}", p.name)
                } else {
                    p.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let fn_type = function_type_from_sig(&safe_sig);
        out.push_str(&format!(
            "  {}({}): {} {{ let __fn = ({} as {{ {}: unknown }}).{}; return (__fn as {})({}); }}\n",
            name, param_decl, safe_sig.return_type, export_var, name, name, fn_type, call_args
        ));
    }
    out.push_str("}\n");
    out.push_str(&format!("const {} = new {}();\n", local, class_name));
    out
}

fn has_named_value_exports(meta: &ModuleMeta) -> bool {
    meta.value_exports.keys().any(|name| name != "default")
}

fn emit_default_object_binding(local: &str, export_var: &str, meta: &ModuleMeta) -> String {
    let mut fields = Vec::new();
    for name in meta.value_exports.keys() {
        if name == "default" {
            continue;
        }
        let key = if is_identifier(name) {
            name.clone()
        } else {
            format!("\"{}\"", escape_string(name))
        };
        fields.push(format!("{}: {}", key, property_accessor(export_var, name)));
    }
    format!("const {} = {{ {} }};\n", local, fields.join(", "))
}

fn normalize_type_snippet(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    while s.ends_with('{') || s.ends_with(')') || s.ends_with(',') || s.ends_with(';') {
        s.pop();
        s = s.trim_end().to_string();
    }
    s
}

fn rewrite_local_class_refs(
    type_expr: &str,
    local_type_names: &HashSet<String>,
    module_tag: &str,
) -> String {
    if local_type_names.is_empty() {
        return type_expr.to_string();
    }
    let mut result = String::new();
    let mut token = String::new();
    for ch in type_expr.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if !token.is_empty() {
                if local_type_names.contains(&token) {
                    result.push_str(&format!("__t_{}_{}", module_tag, token));
                } else {
                    result.push_str(&token);
                }
                token.clear();
            }
            result.push(ch);
        }
    }
    if !token.is_empty() {
        if local_type_names.contains(&token) {
            result.push_str(&format!("__t_{}_{}", module_tag, token));
        } else {
            result.push_str(&token);
        }
    }
    result
}

fn variable_decl_type_expr(
    decl: &raya_engine::parser::ast::VariableDecl,
    module: &ParsedStdModule,
) -> String {
    if let Some(ann) = &decl.type_annotation {
        return normalize_type_snippet(ann.span.slice(&module.source));
    }
    if let Some(init) = &decl.initializer {
        let init_src = init.span().slice(&module.source).trim();
        if let Some(rest) = init_src.strip_prefix("new ") {
            let mut ty = String::new();
            for ch in rest.chars() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                    ty.push(ch);
                } else {
                    break;
                }
            }
            if !ty.is_empty() {
                return ty;
            }
        }
    }
    "unknown".to_string()
}

fn hoisted_declaration_source(stmt: &Statement, module: &ParsedStdModule) -> Option<String> {
    match stmt {
        Statement::TypeAliasDecl(_) => Some(stmt.span().slice(&module.source).to_string()),
        Statement::ClassDecl(_) if module.canonical == "pm" => {
            Some(stmt.span().slice(&module.source).to_string())
        }
        Statement::VariableDecl(var)
            if module.canonical == "pm" && is_pm_hoistable_var_decl(var) =>
        {
            Some(stmt.span().slice(&module.source).to_string())
        }
        Statement::ExportDecl(ExportDecl::Declaration(inner)) => match inner.as_ref() {
            Statement::TypeAliasDecl(_) => Some(strip_export_prefix(export_decl_source(
                stmt,
                &module.source,
            ))),
            Statement::ClassDecl(_) if module.canonical == "pm" => Some(strip_export_prefix(
                export_decl_source(stmt, &module.source),
            )),
            Statement::VariableDecl(var)
                if module.canonical == "pm" && is_pm_hoistable_var_decl(var) =>
            {
                Some(strip_export_prefix(export_decl_source(
                    stmt,
                    &module.source,
                )))
            }
            _ => None,
        },
        _ => None,
    }
}

fn is_pm_hoistable_var_decl(var: &raya_engine::parser::ast::VariableDecl) -> bool {
    if !matches!(var.kind, raya_engine::parser::ast::VariableKind::Const) {
        return false;
    }
    if !matches!(var.pattern, Pattern::Identifier(_)) {
        return false;
    }
    matches!(
        var.initializer.as_ref(),
        Some(
            raya_engine::parser::ast::Expression::StringLiteral(_)
                | raya_engine::parser::ast::Expression::FloatLiteral(_)
                | raya_engine::parser::ast::Expression::IntLiteral(_)
                | raya_engine::parser::ast::Expression::BooleanLiteral(_)
        )
    )
}

fn export_decl_source<'a>(stmt: &Statement, source: &'a str) -> &'a str {
    stmt.span().slice(source)
}

fn sanitize_wrapper_sig(sig: &FunctionSig, allowed_types: Option<&HashSet<String>>) -> FunctionSig {
    FunctionSig {
        params: sig
            .params
            .iter()
            .map(|p| FunctionParamSig {
                name: p.name.clone(),
                ty: sanitize_wrapper_type(&p.ty, allowed_types),
                optional: p.optional,
                has_default: p.has_default,
                is_rest: p.is_rest,
            })
            .collect(),
        return_type: sanitize_wrapper_type(&sig.return_type, allowed_types),
    }
}

fn sanitize_wrapper_type(ty: &str, allowed_types: Option<&HashSet<String>>) -> String {
    let allowed = [
        "string", "number", "boolean", "void", "unknown", "null", "int", "float", "Buffer",
        "Promise", "Task",
    ];
    let mut token = String::new();
    for ch in ty.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else if !token.is_empty() {
            if token
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_')
                .unwrap_or(false)
                && !token.starts_with("__t_")
                && !allowed_types
                    .map(|set| set.contains(&token))
                    .unwrap_or(false)
                && !allowed.contains(&token.as_str())
            {
                return "unknown".to_string();
            }
            token.clear();
        }
    }
    if !token.is_empty()
        && token
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        && !token.starts_with("__t_")
        && !allowed_types
            .map(|set| set.contains(&token))
            .unwrap_or(false)
        && !allowed.contains(&token.as_str())
    {
        return "unknown".to_string();
    }
    ty.to_string()
}

fn extract_class_type_meta(
    class: &raya_engine::parser::ast::ClassDecl,
    module: &ParsedStdModule,
) -> ClassTypeMeta {
    use raya_engine::parser::ast::ClassMember;
    let type_params = class
        .type_params
        .as_ref()
        .map(|params| {
            params
                .iter()
                .map(|tp| module.interner.resolve(tp.name.name).to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut properties = Vec::new();
    let mut methods = Vec::new();
    for member in &class.members {
        match member {
            ClassMember::Field(field) => {
                if field.is_static {
                    continue;
                }
                let name = module.interner.resolve(field.name.name).to_string();
                let type_expr = field
                    .type_annotation
                    .as_ref()
                    .map(|ann| normalize_type_snippet(ann.span.slice(&module.source)))
                    .unwrap_or_else(|| "unknown".to_string());
                properties.push((name, type_expr));
            }
            ClassMember::Method(method) => {
                if method.is_static {
                    continue;
                }
                let name = module.interner.resolve(method.name.name).to_string();
                let params = method
                    .params
                    .iter()
                    .enumerate()
                    .map(|(idx, param)| {
                        let pname = match &param.pattern {
                            Pattern::Identifier(id) => module.interner.resolve(id.name).to_string(),
                            _ => format!("arg{}", idx),
                        };
                        FunctionParamSig {
                            name: pname,
                            ty: param
                                .type_annotation
                                .as_ref()
                                .map(|ann| normalize_type_snippet(ann.span.slice(&module.source)))
                                .unwrap_or_else(|| "unknown".to_string()),
                            optional: param.optional,
                            has_default: param.default_value.is_some(),
                            is_rest: param.is_rest,
                        }
                    })
                    .collect::<Vec<_>>();
                let return_type = method
                    .return_type
                    .as_ref()
                    .map(|ann| normalize_type_snippet(ann.span.slice(&module.source)))
                    .unwrap_or_else(|| "void".to_string());
                methods.push((
                    name,
                    FunctionSig {
                        params,
                        return_type,
                    },
                ));
            }
            _ => {}
        }
    }
    ClassTypeMeta {
        type_params,
        properties,
        methods,
    }
}

fn synthesize_type_aliases(
    module_tag: &str,
    class_types: &BTreeMap<String, ClassTypeMeta>,
    type_ref_map: &BTreeMap<String, String>,
) -> String {
    // Topologically sort type aliases so dependencies come before dependents.
    // The binder processes type aliases in source order without a prepass,
    // so forward references cause UndefinedType errors.
    let local_class_names: HashSet<&str> = class_types.keys().map(|s| s.as_str()).collect();
    let ordered = topo_sort_classes(class_types, &local_class_names);

    let mut out = String::new();
    for name in &ordered {
        let meta = &class_types[name];
        let type_param_decl = if meta.type_params.is_empty() {
            String::new()
        } else {
            let params = meta
                .type_params
                .iter()
                .map(|p| format!("{} = unknown", p))
                .collect::<Vec<_>>()
                .join(", ");
            format!("<{}>", params)
        };
        let allowed_type_params = if meta.type_params.is_empty() {
            None
        } else {
            Some(meta.type_params.iter().cloned().collect::<HashSet<_>>())
        };
        out.push_str(&format!(
            "type __t_{}_{}{} = {{ ",
            module_tag, name, type_param_decl
        ));
        for (prop_name, type_expr) in &meta.properties {
            let resolved = resolve_type_refs(type_expr, type_ref_map);
            let safe = sanitize_wrapper_type(&resolved, allowed_type_params.as_ref());
            out.push_str(&format!("{}: {}, ", prop_name, safe));
        }
        for (method_name, sig) in &meta.methods {
            let key = if is_identifier(method_name) {
                method_name.clone()
            } else {
                format!("\"{}\"", escape_string(method_name))
            };
            let params = sig
                .params
                .iter()
                .map(|p| {
                    let name = if p.is_rest {
                        format!("...{}", p.name)
                    } else if p.optional || p.has_default {
                        format!("{}?", p.name)
                    } else {
                        p.name.clone()
                    };
                    let resolved = resolve_type_refs(&p.ty, type_ref_map);
                    let safe = sanitize_wrapper_type(&resolved, allowed_type_params.as_ref());
                    format!("{}: {}", name, safe)
                })
                .collect::<Vec<_>>()
                .join(", ");
            let resolved_ret = resolve_type_refs(&sig.return_type, type_ref_map);
            let ret = sanitize_wrapper_type(&resolved_ret, allowed_type_params.as_ref());
            out.push_str(&format!("{}: ({}) => {}, ", key, params, ret));
        }
        out.push_str("};\n");
    }
    out
}

/// Topologically sort class names so that classes referenced by other classes
/// in the same module come first. Falls back to alphabetical for unrelated classes.
fn topo_sort_classes(
    class_types: &BTreeMap<String, ClassTypeMeta>,
    local_names: &HashSet<&str>,
) -> Vec<String> {
    // Build dependency graph: deps[A] = {B, C} means A references B and C
    let mut deps: BTreeMap<&str, HashSet<&str>> = BTreeMap::new();
    for (name, meta) in class_types {
        let mut class_deps: HashSet<&str> = HashSet::new();
        for (_, type_expr) in &meta.properties {
            collect_type_refs(type_expr, local_names, &mut class_deps);
        }
        for (_, sig) in &meta.methods {
            for p in &sig.params {
                collect_type_refs(&p.ty, local_names, &mut class_deps);
            }
            collect_type_refs(&sig.return_type, local_names, &mut class_deps);
        }
        class_deps.remove(name.as_str());
        deps.insert(name.as_str(), class_deps);
    }

    // Kahn's algorithm: in_degree[X] = number of dependencies X has.
    // Emit classes with 0 remaining dependencies first.
    let mut in_deg: BTreeMap<&str, usize> = BTreeMap::new();
    for name in class_types.keys() {
        in_deg.insert(
            name.as_str(),
            deps.get(name.as_str()).map_or(0, |d| d.len()),
        );
    }

    let mut ready: Vec<&str> = in_deg
        .iter()
        .filter(|(_, &count)| count == 0)
        .map(|(&name, _)| name)
        .collect();
    ready.sort();

    let mut result: Vec<String> = Vec::new();
    let mut visited: HashSet<&str> = HashSet::new();
    while let Some(name) = ready.pop() {
        if !visited.insert(name) {
            continue;
        }
        result.push(name.to_string());
        // Decrement in-degree for all classes that depend on this one
        for (&dependent, dependent_deps) in &deps {
            if dependent_deps.contains(name) {
                if let Some(count) = in_deg.get_mut(dependent) {
                    *count = count.saturating_sub(1);
                    if *count == 0 && !visited.contains(dependent) {
                        ready.push(dependent);
                        ready.sort();
                    }
                }
            }
        }
    }

    // Append any remaining classes (cycles — shouldn't happen but be safe)
    for name in class_types.keys() {
        if !visited.contains(name.as_str()) {
            result.push(name.clone());
        }
    }

    result
}

/// Extract identifiers from a type expression that match local class names
fn collect_type_refs<'a>(
    type_expr: &str,
    local_names: &HashSet<&'a str>,
    out: &mut HashSet<&'a str>,
) {
    let mut token = String::new();
    for ch in type_expr.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if !token.is_empty() {
                if let Some(&name) = local_names.get(token.as_str()) {
                    out.insert(name);
                }
                token.clear();
            }
        }
    }
    if !token.is_empty() {
        if let Some(&name) = local_names.get(token.as_str()) {
            out.insert(name);
        }
    }
}

fn resolve_type_refs(type_expr: &str, type_ref_map: &BTreeMap<String, String>) -> String {
    if type_ref_map.is_empty() {
        return type_expr.to_string();
    }
    let mut result = String::new();
    let mut token = String::new();
    for ch in type_expr.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if !token.is_empty() {
                if let Some(replacement) = type_ref_map.get(&token) {
                    result.push_str(replacement);
                } else {
                    result.push_str(&token);
                }
                token.clear();
            }
            result.push(ch);
        }
    }
    if !token.is_empty() {
        if let Some(replacement) = type_ref_map.get(&token) {
            result.push_str(replacement);
        } else {
            result.push_str(&token);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::build_std_prelude;

    #[test]
    fn debug_dump_mixed_import_prelude() {
        let source = r#"
            import path from "std:path";
            import fs from "std:fs";
            import env from "std:env";
            let base = env.cwd();
            let full = path.join(base, "tmp");
            full.length;
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        let merged = format!("{}\n{}", out.prelude_source, out.rewritten_user_source);
        let _ = std::fs::write("/tmp/raya_std_prelude_debug.raya", merged);
    }

    #[test]
    fn debug_dump_pm_import_prelude() {
        let source = r#"
            import pm from "std:pm";
            pm != null;
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        let merged = format!("{}\n{}", out.prelude_source, out.rewritten_user_source);
        let _ = std::fs::write("/tmp/raya_std_prelude_pm_debug.raya", merged);
    }

    #[test]
    fn prelude_named_http_class_import_emits_value_binding() {
        let source = r#"
            import { HttpServer } from "std:http";
            HttpServer;
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        assert!(
            out.prelude_source.contains("const HttpServer"),
            "expected runtime value binding for HttpServer, got:\n{}",
            out.prelude_source
        );
    }

    #[test]
    fn prelude_named_class_import_emits_type_and_value_bindings_for_mixed_usage() {
        let source = r#"
            import { HttpServer } from "std:http";
            function usesType(server: HttpServer): void {}
            const server = new HttpServer("127.0.0.1", 0);
            server.localPort();
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        assert!(
            out.prelude_source.contains("const HttpServer"),
            "expected class value binding for HttpServer, got:\n{}",
            out.prelude_source
        );
        assert!(
            out.prelude_source.contains("type HttpServer ="),
            "expected class type binding for HttpServer, got:\n{}",
            out.prelude_source
        );
    }

    #[test]
    fn prelude_dedupes_repeated_default_import_bindings() {
        let source = r#"
            import env from "std:env";
            import env from "std:env";
            env.cwd();
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        let dot = out
            .prelude_source
            .matches("const env = (__std_exports_env.default as __t_env_EnvNamespace);")
            .count();
        let bracket = out
            .prelude_source
            .matches("const env = (__std_exports_env[\"default\"] as __t_env_EnvNamespace);")
            .count();
        let count = dot + bracket;
        assert_eq!(
            count, 1,
            "expected one env binding, got {} in:\n{}",
            count, out.prelude_source
        );
    }

    #[test]
    fn prelude_named_stream_class_import_emits_value_binding() {
        let source = r#"
            import { ReadableStream } from "std:stream";
            ReadableStream;
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        assert!(
            out.prelude_source.contains("const ReadableStream"),
            "expected runtime value binding for ReadableStream, got:\n{}",
            out.prelude_source
        );
    }

    #[test]
    fn prelude_alias_synthesis_preserves_generic_params() {
        let source = r#"
            import { ReadableStream } from "std:stream";
            ReadableStream;
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        assert!(
            out.prelude_source
                .contains("type __t_stream_ReadableStream<T = unknown> = {"),
            "expected generic alias for ReadableStream, got:\n{}",
            out.prelude_source
        );
    }

    #[test]
    fn prelude_alias_synthesis_marks_defaulted_params_optional() {
        let source = r#"
            import compress from "std:compress";
            compress.gzip;
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        assert!(
            out.prelude_source
                .contains("gzip: (data: Buffer, level?: number) => Buffer"),
            "expected defaulted parameter to be optional in synthesized alias, got:\n{}",
            out.prelude_source
        );
    }

    #[test]
    fn prelude_alias_synthesis_preserves_promise_returns() {
        let source = r#"
            import { HttpServer } from "std:http";
            HttpServer;
        "#;
        let out = build_std_prelude(source).expect("std prelude should build");
        assert!(
            out.prelude_source
                .contains("serve: (handler: unknown) => Promise"),
            "expected Promise return in synthesized HttpServer alias, got:\n{}",
            out.prelude_source
        );
    }
}
