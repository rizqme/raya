//! Raya Compiler - AST to Bytecode Code Generation
//!
//! This crate implements the compiler that transforms typed AST into bytecode.
//!
//! # Architecture
//!
//! The compilation pipeline is:
//! 1. AST (from raya-parser) → IR (intermediate representation)
//! 2. IR → Monomorphization (generic specialization)
//! 3. IR → Optimizations (constant folding, DCE)
//! 4. IR → Bytecode
//!
//! The IR uses Three-Address Code (TAC) with Basic Blocks.

pub mod bytecode;
pub mod builtins;
pub mod codegen;
#[allow(dead_code)]
pub mod codegen_ast;
pub mod compiled_support;
pub mod error;
pub mod intrinsic;
pub mod ir;
#[allow(dead_code)]
pub mod lower;
pub mod module;
pub mod module_builder;
#[allow(dead_code)]
pub mod monomorphize;
pub mod native_id;
#[allow(dead_code)]
pub mod optimize;
pub mod type_registry;

pub use codegen::IrCodeGenerator;
pub use codegen_ast::CodeGenerator;
pub use error::{CompileError, CompileResult};
pub use module::{ModuleCache, ModuleCompiler, ModuleGraph, ModuleResolver};
pub use module_builder::ModuleBuilder;

// Re-export bytecode types for convenience
pub use bytecode::{
    module_id_from_name, symbol_id_from_name, verify_module, BytecodeReader, BytecodeWriter,
    ClassDef, ConstantPool, DecodeError, Export, Function, Import, Metadata, Method, Module,
    ModuleError, ModuleId, NominalTypeExport, Opcode, StructuralLayoutInfo, StructuralShapeInfo,
    SymbolId, SymbolScope, SymbolType, TypeSignatureHash, VerifyError,
};

use crate::parser::ast;
use crate::parser::Interner;
use crate::parser::TypeContext;
use crate::parser::TypeId;
use crate::semantics::{build_semantic_lowering_plan_with_types, SemanticProfile};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::{HashMap, HashSet};

/// Monomorphization strategy for compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MonomorphizationMode {
    /// Disable monomorphization and keep generic dispatch in IR.
    Off,
    /// Specialize generics at consumer compile/link time.
    #[default]
    ConsumerLink,
}

#[derive(Debug, Clone, Default)]
pub struct LoweringMetadata {
    pub module_global_slots: HashMap<String, u32>,
    pub js_global_bindings: Vec<crate::compiler::bytecode::module::JsGlobalBindingInfo>,
}

/// Main compiler entry point
pub struct Compiler<'a> {
    type_ctx: TypeContext,
    interner: &'a Interner,
    /// Expression types from type checker (maps expr ptr to TypeId)
    expr_types: FxHashMap<usize, TypeId>,
    /// Type annotation types from type checker (maps annotation ptr to TypeId)
    type_annotation_types: FxHashMap<usize, TypeId>,
    /// JSX compilation options (None = JSX disabled)
    jsx_options: Option<lower::JsxOptions>,
    /// Shared semantic profile driving checker/lowering behavior.
    semantic_profile: SemanticProfile,
    /// Whether to emit source map (bytecode offset → source location)
    emit_sourcemap: bool,
    /// Emit generic template metadata into bytecode artifacts.
    emit_generic_templates: bool,
    /// Monomorphization mode for IR pipeline.
    monomorphization_mode: MonomorphizationMode,
    /// Stable module identity used for metadata and symbol ID derivation.
    module_identity: Option<String>,
    /// Original source text for debug dumps (enables source-annotated IR/bytecode output)
    source_text: Option<String>,
    /// Ambient builtin globals available without explicit source declarations/imports.
    ambient_builtin_globals: FxHashSet<String>,
    /// Direct-eval lowering metadata shared across compiler and lowerer.
    js_eval_compile_context: JsEvalCompileContext,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct JsEvalCompileContext {
    pub entry_function: Option<String>,
    pub binding_names: FxHashSet<String>,
}

impl JsEvalCompileContext {
    pub fn with_entry_function(mut self, function_name: impl Into<String>) -> Self {
        self.entry_function = Some(function_name.into());
        self
    }

    pub fn with_binding_names<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.binding_names = names.into_iter().map(Into::into).collect();
        self
    }

    pub fn matches_entry_function(&self, function_name: &str) -> bool {
        self.entry_function
            .as_deref()
            .is_some_and(|target| target == function_name)
    }
}

impl<'a> Compiler<'a> {
    fn uses_builtin_this_coercion_compat(&self) -> bool {
        self.module_identity
            .as_deref()
            .is_some_and(|module_identity| {
                module_identity.starts_with("__raya_builtin__/")
                    || module_identity.contains("/builtins/")
                    || module_identity.contains("\\builtins\\")
            })
    }

    pub fn new(type_ctx: TypeContext, interner: &'a Interner) -> Self {
        Self {
            type_ctx,
            interner,
            expr_types: FxHashMap::default(),
            type_annotation_types: FxHashMap::default(),
            jsx_options: None,
            semantic_profile: SemanticProfile::raya(),
            emit_sourcemap: false,
            emit_generic_templates: false,
            monomorphization_mode: MonomorphizationMode::ConsumerLink,
            module_identity: None,
            source_text: None,
            ambient_builtin_globals: FxHashSet::default(),
            js_eval_compile_context: JsEvalCompileContext::default(),
        }
    }

    /// Attach original source text for annotated debug dumps.
    ///
    /// When set and `RAYA_DEBUG_DUMP_IR` or `RAYA_DEBUG_DUMP_BYTECODE` are active,
    /// each IR instruction / bytecode opcode is followed by the corresponding source line.
    pub fn with_source_text(mut self, source: impl Into<String>) -> Self {
        self.source_text = Some(source.into());
        self
    }

    /// Set expression types from the type checker's CheckResult
    pub fn with_expr_types(mut self, expr_types: FxHashMap<usize, TypeId>) -> Self {
        self.expr_types = expr_types;
        self
    }

    /// Set resolved type annotation types from the type checker's CheckResult
    pub fn with_type_annotation_types(
        mut self,
        type_annotation_types: FxHashMap<usize, TypeId>,
    ) -> Self {
        self.type_annotation_types = type_annotation_types;
        self
    }

    /// Enable JSX compilation with the given options
    pub fn with_jsx(mut self, options: lower::JsxOptions) -> Self {
        self.jsx_options = Some(options);
        self
    }

    /// Configure lowering/runtime behavior from a shared semantic profile.
    pub fn with_semantic_profile(mut self, profile: SemanticProfile) -> Self {
        self.semantic_profile = profile;
        self
    }

    /// Enable/disable source map generation in output bytecode
    pub fn with_sourcemap(mut self, enable: bool) -> Self {
        self.emit_sourcemap = enable;
        self
    }

    /// Emit generic template metadata in the output module.
    pub fn with_emit_generic_templates(mut self, enable: bool) -> Self {
        self.emit_generic_templates = enable;
        self
    }

    /// Configure monomorphization strategy.
    pub fn with_monomorphization_mode(mut self, mode: MonomorphizationMode) -> Self {
        self.monomorphization_mode = mode;
        self
    }

    /// Set stable module identity used in output metadata and symbol ID derivation.
    pub fn with_module_identity(mut self, module_identity: impl Into<String>) -> Self {
        self.module_identity = Some(module_identity.into());
        self
    }

    /// Provide ambient builtin global names that lowering may resolve via runtime lookup.
    pub fn with_ambient_builtin_globals<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ambient_builtin_globals = names.into_iter().map(Into::into).collect();
        self
    }

    /// Provide explicit direct-eval lowering metadata.
    pub(crate) fn with_js_eval_compile_context(mut self, context: JsEvalCompileContext) -> Self {
        self.js_eval_compile_context = context;
        self
    }

    fn build_lowerer_for_module(
        &self,
        module: &ast::Module,
        emit_sourcemap: bool,
    ) -> lower::Lowerer<'_> {
        let semantic_plan = build_semantic_lowering_plan_with_types(
            module,
            self.interner,
            self.semantic_profile,
            Some(&self.type_ctx),
            Some(&self.expr_types),
        );
        lower::Lowerer::with_expr_types(&self.type_ctx, self.interner, self.expr_types.clone())
            .with_type_annotation_types(self.type_annotation_types.clone())
            .with_sourcemap(emit_sourcemap)
            .with_semantic_plan(semantic_plan)
            .with_builtin_this_coercion_compat(self.uses_builtin_this_coercion_compat())
            .with_ambient_builtin_globals(self.ambient_builtin_globals.clone())
            .with_js_eval_compile_context(self.js_eval_compile_context.clone())
    }

    /// Compile a module into bytecode
    pub fn compile(&mut self, module: &ast::Module) -> CompileResult<Module> {
        let mut codegen = CodeGenerator::new(&self.type_ctx, self.interner);
        codegen.compile_program(module)
    }

    /// Compile a module to IR (for debugging/inspection)
    pub fn compile_to_ir(&self, module: &ast::Module) -> ir::IrModule {
        let mut lowerer = self.build_lowerer_for_module(module, self.emit_sourcemap);
        if let Some(ref jsx_opts) = self.jsx_options {
            lowerer = lowerer.with_jsx(jsx_opts.clone());
        }
        let mut ir_module = lowerer.lower_module(module);
        if let Some(module_identity) = &self.module_identity {
            ir_module.name = module_identity.clone();
        }
        ir_module
    }

    /// Compile a module to IR with monomorphization
    ///
    /// This performs the full IR compilation pipeline including:
    /// 1. AST lowering to IR
    /// 2. Monomorphization (generic specialization)
    /// 3. Optimization passes
    pub fn compile_to_optimized_ir(&self, module: &ast::Module) -> CompileResult<ir::IrModule> {
        self.compile_to_optimized_ir_with_metadata(module)
            .map(|(ir_module, _, _, _)| ir_module)
    }

    fn compile_to_optimized_ir_with_metadata(
        &self,
        module: &ast::Module,
    ) -> CompileResult<(
        ir::IrModule,
        Vec<StructuralShapeInfo>,
        Vec<StructuralLayoutInfo>,
        LoweringMetadata,
    )> {
        // Auto-enable sourcemap when debug dump env vars are active
        let dump_ir = std::env::var("RAYA_DEBUG_DUMP_IR").is_ok();
        let dump_bc = std::env::var("RAYA_DEBUG_DUMP_BYTECODE").is_ok();
        let need_sourcemap = self.emit_sourcemap || dump_ir || dump_bc;

        // Step 1: Lower AST to IR
        let mut lowerer = self.build_lowerer_for_module(module, need_sourcemap);
        if let Some(ref jsx_opts) = self.jsx_options {
            lowerer = lowerer.with_jsx(jsx_opts.clone());
        }
        let mut ir_module = lowerer.lower_module(module);
        let lowering_metadata = LoweringMetadata {
            module_global_slots: lowerer.module_global_slots(),
            js_global_bindings: lowerer.js_global_bindings(),
        };
        if let Some(module_identity) = &self.module_identity {
            ir_module.name = module_identity.clone();
        }
        let structural_shapes = lowerer
            .structural_shape_member_sets()
            .into_iter()
            .map(|member_names| StructuralShapeInfo { member_names })
            .collect::<Vec<_>>();
        let structural_layouts = ir_module
            .structural_layouts
            .iter()
            .map(|(layout_id, member_names)| StructuralLayoutInfo {
                layout_id: *layout_id,
                member_names: member_names.clone(),
            })
            .collect::<Vec<_>>();

        // Check for lowerer errors (e.g., unresolved types at dispatch points)
        if let Some(err) = lowerer.errors().first() {
            return Err(CompileError::InternalError {
                message: format!("{}", err),
            });
        }

        // Step 2: Monomorphization
        if matches!(
            self.monomorphization_mode,
            MonomorphizationMode::ConsumerLink
        ) {
            let _mono_result =
                monomorphize::monomorphize(&mut ir_module, &self.type_ctx, self.interner);
        }

        // Step 2b: Resolve late-bound member accesses (TypeVar → concrete type dispatch)
        let builtin_surface = module::builtin_surface_manifest_for_mode(
            module::builtin_surface_mode_for_profile(self.semantic_profile),
        );
        let type_registry = type_registry::TypeRegistry::new(&self.type_ctx, builtin_surface);
        monomorphize::resolve_late_bound_members(&mut ir_module, &type_registry, &self.type_ctx);

        // Strict dynamic-JS invariant: dynamic JS property writes must not survive
        // lowering when JS dynamic semantics are disabled.
        if matches!(
            self.semantic_profile.js_dynamic_semantics,
            crate::semantics::JsDynamicSemantics::Disabled
        ) {
            for func in &ir_module.functions {
                for block in &func.blocks {
                    for instr in &block.instructions {
                        if matches!(instr, ir::IrInstr::DynSetProp { .. }) {
                            return Err(CompileError::InternalError {
                                message: format!(
                                    "strict mode forbids dynamic JS property writes in function '{}'",
                                    func.name
                                ),
                            });
                        }
                    }
                }
            }
        }

        // Step 3: Optimization passes
        let optimizer = optimize::Optimizer::basic();
        optimizer.optimize(&mut ir_module);

        // Dump annotated IR to stderr when RAYA_DEBUG_DUMP_IR is set
        if dump_ir {
            dump_ir_module(
                &ir_module,
                self.source_text.as_deref(),
                Some(&self.type_ctx),
            );
        }

        // Dump type table when RAYA_DEBUG_DUMP_TYPES is set
        if std::env::var("RAYA_DEBUG_DUMP_TYPES").is_ok() {
            dump_type_table(&self.type_ctx);
        }

        Ok((
            ir_module,
            structural_shapes,
            structural_layouts,
            lowering_metadata,
        ))
    }

    /// Compile a module through the full IR pipeline to bytecode
    ///
    /// This is the preferred compilation path that uses:
    /// 1. AST → IR lowering
    /// 2. Monomorphization
    /// 3. Optimizations
    /// 4. IR → Bytecode code generation
    pub fn compile_via_ir(&self, module: &ast::Module) -> CompileResult<Module> {
        self.compile_via_ir_with_lowering_metadata(module)
            .map(|(bytecode_module, _)| bytecode_module)
    }

    pub fn compile_via_ir_with_lowering_metadata(
        &self,
        module: &ast::Module,
    ) -> CompileResult<(Module, LoweringMetadata)> {
        let dump_bc = std::env::var("RAYA_DEBUG_DUMP_BYTECODE").is_ok();
        let need_sourcemap =
            self.emit_sourcemap || dump_bc || std::env::var("RAYA_DEBUG_DUMP_IR").is_ok();

        // Get optimized IR (sourcemap enabling and IR dump happen inside)
        let (ir_module, structural_shapes, structural_layouts, lowering_metadata) =
            self.compile_to_optimized_ir_with_metadata(module)?;

        // Generate bytecode from IR
        let mut bytecode_module = codegen::generate(&ir_module, need_sourcemap)?;
        if let Some(module_identity) = &self.module_identity {
            bytecode_module.metadata.name = module_identity.clone();
        }
        if self.emit_generic_templates {
            bytecode_module.metadata.generic_templates =
                monomorphize::collect_generic_templates(&ir_module);
            bytecode_module.metadata.template_symbol_table =
                monomorphize::collect_template_symbol_table(&ir_module);
            bytecode_module.metadata.mono_debug_map =
                monomorphize::collect_mono_debug_map(&ir_module);
        }
        bytecode_module.metadata.structural_shapes = structural_shapes;
        bytecode_module.metadata.structural_layouts = structural_layouts;
        bytecode_module.metadata.js_global_bindings = lowering_metadata.js_global_bindings.clone();
        populate_symbol_link_metadata(&mut bytecode_module, module, self.interner);

        // Dump annotated bytecode to stderr when RAYA_DEBUG_DUMP_BYTECODE is set
        if dump_bc {
            dump_bytecode_module(&bytecode_module, self.source_text.as_deref());
        }

        Ok((bytecode_module, lowering_metadata))
    }

    /// Compile a module through the full IR pipeline with verification
    ///
    /// Same as compile_via_ir but also verifies the generated bytecode.
    pub fn compile_via_ir_verified(&self, module: &ast::Module) -> CompileResult<Module> {
        let (bytecode_module, _) = self.compile_via_ir_with_lowering_metadata(module)?;

        // Verify the generated bytecode
        verify_module(&bytecode_module).map_err(|e| CompileError::Verification {
            message: e.to_string(),
        })?;

        Ok(bytecode_module)
    }

    /// Compile with debug output
    ///
    /// Returns both the bytecode module and a debug string showing IR and bytecode.
    pub fn compile_with_debug(&self, module: &ast::Module) -> CompileResult<(Module, String)> {
        use ir::PrettyPrint;
        use std::fmt::Write;

        let mut debug = String::new();

        // Step 1: Lower AST to IR
        let mut lowerer = self.build_lowerer_for_module(module, self.emit_sourcemap);
        if let Some(ref jsx_opts) = self.jsx_options {
            lowerer = lowerer.with_jsx(jsx_opts.clone());
        }
        let mut ir_module = lowerer.lower_module(module);
        if let Some(module_identity) = &self.module_identity {
            ir_module.name = module_identity.clone();
        }

        // Check for lowerer errors
        if let Some(err) = lowerer.errors().first() {
            return Err(CompileError::InternalError {
                message: format!("{}", err),
            });
        }

        writeln!(debug, "=== IR Before Optimization ===").unwrap();
        writeln!(debug, "{}", ir_module.pretty_print()).unwrap();

        // Step 2: Monomorphization
        let mono_result = if matches!(
            self.monomorphization_mode,
            MonomorphizationMode::ConsumerLink
        ) {
            monomorphize::monomorphize(&mut ir_module, &self.type_ctx, self.interner)
        } else {
            monomorphize::MonomorphizationResult {
                functions_specialized: 0,
                classes_specialized: 0,
                call_sites_rewritten: 0,
            }
        };
        writeln!(debug, "=== Monomorphization Stats ===").unwrap();
        writeln!(
            debug,
            "Functions specialized: {}",
            mono_result.functions_specialized
        )
        .unwrap();
        writeln!(
            debug,
            "Classes specialized: {}",
            mono_result.classes_specialized
        )
        .unwrap();

        // Step 2b: Resolve late-bound member accesses (TypeVar → concrete type dispatch)
        let builtin_surface = module::builtin_surface_manifest_for_mode(
            module::builtin_surface_mode_for_profile(self.semantic_profile),
        );
        let type_registry = type_registry::TypeRegistry::new(&self.type_ctx, builtin_surface);
        monomorphize::resolve_late_bound_members(&mut ir_module, &type_registry, &self.type_ctx);

        // Step 3: Optimization passes
        let optimizer = optimize::Optimizer::basic();
        optimizer.optimize(&mut ir_module);

        writeln!(debug, "\n=== IR After Optimization ===").unwrap();
        writeln!(debug, "{}", ir_module.pretty_print()).unwrap();

        // Step 4: Generate bytecode
        let mut bytecode_module = codegen::generate(&ir_module, self.emit_sourcemap)?;
        if let Some(module_identity) = &self.module_identity {
            bytecode_module.metadata.name = module_identity.clone();
        }
        if self.emit_generic_templates {
            bytecode_module.metadata.generic_templates =
                monomorphize::collect_generic_templates(&ir_module);
            bytecode_module.metadata.template_symbol_table =
                monomorphize::collect_template_symbol_table(&ir_module);
            bytecode_module.metadata.mono_debug_map =
                monomorphize::collect_mono_debug_map(&ir_module);
        }
        populate_symbol_link_metadata(&mut bytecode_module, module, self.interner);

        writeln!(debug, "=== Generated Bytecode ===").unwrap();
        for (i, func) in bytecode_module.functions.iter().enumerate() {
            writeln!(
                debug,
                "Function {}: {} (locals: {}, params: {})",
                i, func.name, func.local_count, func.param_count
            )
            .unwrap();
            writeln!(debug, "  Code size: {} bytes", func.code.len()).unwrap();
            writeln!(debug, "  Disassembly:").unwrap();
            writeln!(debug, "{}", disassemble_function(func)).unwrap();
        }

        Ok((bytecode_module, debug))
    }
}

#[derive(Debug, Clone)]
struct ExportBinding {
    exported_name: String,
    local_name: String,
}

fn module_name_from_specifier(specifier: &str) -> &str {
    if let Some(stripped) = specifier.strip_prefix('@') {
        if let Some(at_pos) = stripped.find('@') {
            return &specifier[..at_pos + 1];
        }
        return specifier;
    }

    if let Some(at_pos) = specifier.find('@') {
        return &specifier[..at_pos];
    }

    specifier
}

fn collect_pattern_identifiers(pattern: &ast::Pattern, interner: &Interner, out: &mut Vec<String>) {
    // Use an explicit stack instead of recursion to avoid stack overflows on
    // deeply nested generated destructuring patterns.
    let mut stack = vec![pattern];
    while let Some(current) = stack.pop() {
        match current {
            ast::Pattern::Identifier(ident) => out.push(interner.resolve(ident.name).to_string()),
            ast::Pattern::Array(array) => {
                if let Some(rest) = &array.rest {
                    stack.push(rest);
                }
                for element in array.elements.iter().rev() {
                    if let Some(element) = element {
                        stack.push(&element.pattern);
                    }
                }
            }
            ast::Pattern::Object(object) => {
                if let Some(rest) = &object.rest {
                    out.push(interner.resolve(rest.name).to_string());
                }
                for property in object.properties.iter().rev() {
                    stack.push(&property.value);
                }
            }
            ast::Pattern::Rest(rest) => stack.push(&rest.argument),
        }
    }
}

fn collect_export_bindings_from_declaration(
    statement: &ast::Statement,
    interner: &Interner,
    out: &mut Vec<ExportBinding>,
) {
    match statement {
        ast::Statement::FunctionDecl(function) => {
            let name = interner.resolve(function.name.name).to_string();
            out.push(ExportBinding {
                exported_name: name.clone(),
                local_name: name,
            });
        }
        ast::Statement::ClassDecl(class) => {
            let name = interner.resolve(class.name.name).to_string();
            out.push(ExportBinding {
                exported_name: name.clone(),
                local_name: name,
            });
        }
        ast::Statement::VariableDecl(variable) => {
            let mut names = Vec::new();
            collect_pattern_identifiers(&variable.pattern, interner, &mut names);
            for name in names {
                out.push(ExportBinding {
                    exported_name: name.clone(),
                    local_name: name,
                });
            }
        }
        _ => {}
    }
}

fn populate_symbol_link_metadata(
    bytecode_module: &mut Module,
    module: &ast::Module,
    interner: &Interner,
) {
    bytecode_module.exports.clear();
    bytecode_module.imports.clear();

    if bytecode_module.metadata.name.is_empty() {
        bytecode_module.metadata.name = "main".to_string();
    }
    let module_name = bytecode_module.metadata.name.clone();

    let mut exports = Vec::new();
    let mut imports = Vec::new();

    for statement in &module.statements {
        match statement {
            ast::Statement::ImportDecl(import_decl) => {
                let module_specifier = interner.resolve(import_decl.source.value).to_string();
                let target_module_name = module_name_from_specifier(&module_specifier);
                let target_module_id = module_id_from_name(target_module_name);

                for specifier in &import_decl.specifiers {
                    match specifier {
                        ast::ImportSpecifier::Named { name, alias } => {
                            let symbol = interner.resolve(name.name).to_string();
                            let alias = alias
                                .as_ref()
                                .map(|ident| interner.resolve(ident.name).to_string());
                            imports.push(Import {
                                module_specifier: module_specifier.clone(),
                                symbol: symbol.clone(),
                                alias,
                                module_id: target_module_id,
                                symbol_id: symbol_id_from_name(
                                    target_module_name,
                                    SymbolScope::Module,
                                    &symbol,
                                ),
                                scope: SymbolScope::Module,
                                signature_hash: 0,
                                type_signature: None,
                                runtime_global_slot: None,
                            });
                        }
                        ast::ImportSpecifier::Default(local) => {
                            let symbol = "default".to_string();
                            imports.push(Import {
                                module_specifier: module_specifier.clone(),
                                symbol: symbol.clone(),
                                alias: Some(interner.resolve(local.name).to_string()),
                                module_id: target_module_id,
                                symbol_id: symbol_id_from_name(
                                    target_module_name,
                                    SymbolScope::Module,
                                    &symbol,
                                ),
                                scope: SymbolScope::Module,
                                signature_hash: 0,
                                type_signature: None,
                                runtime_global_slot: None,
                            });
                        }
                        ast::ImportSpecifier::Namespace(alias) => {
                            let symbol = "*".to_string();
                            imports.push(Import {
                                module_specifier: module_specifier.clone(),
                                symbol: symbol.clone(),
                                alias: Some(interner.resolve(alias.name).to_string()),
                                module_id: target_module_id,
                                symbol_id: symbol_id_from_name(
                                    target_module_name,
                                    SymbolScope::Module,
                                    &symbol,
                                ),
                                scope: SymbolScope::Module,
                                signature_hash: 0,
                                type_signature: None,
                                runtime_global_slot: None,
                            });
                        }
                    }
                }
            }
            ast::Statement::ExportDecl(export_decl) => match export_decl {
                ast::ExportDecl::Declaration(inner_statement) => {
                    collect_export_bindings_from_declaration(
                        inner_statement,
                        interner,
                        &mut exports,
                    );
                }
                ast::ExportDecl::Named {
                    specifiers, source, ..
                } => {
                    if source.is_none() {
                        for specifier in specifiers {
                            let local_name = interner.resolve(specifier.name.name).to_string();
                            let exported_name = specifier
                                .alias
                                .as_ref()
                                .map(|ident| interner.resolve(ident.name).to_string())
                                .unwrap_or_else(|| local_name.clone());
                            exports.push(ExportBinding {
                                exported_name,
                                local_name,
                            });
                        }
                    } else if let Some(source) = source {
                        let module_specifier = interner.resolve(source.value).to_string();
                        let target_module_name = module_name_from_specifier(&module_specifier);
                        let target_module_id = module_id_from_name(target_module_name);
                        for specifier in specifiers {
                            let symbol = interner.resolve(specifier.name.name).to_string();
                            let alias = specifier
                                .alias
                                .as_ref()
                                .map(|ident| interner.resolve(ident.name).to_string());
                            imports.push(Import {
                                module_specifier: module_specifier.clone(),
                                symbol: symbol.clone(),
                                alias,
                                module_id: target_module_id,
                                symbol_id: symbol_id_from_name(
                                    target_module_name,
                                    SymbolScope::Module,
                                    &symbol,
                                ),
                                scope: SymbolScope::Module,
                                signature_hash: 0,
                                type_signature: None,
                                runtime_global_slot: None,
                            });
                        }
                    }
                }
                ast::ExportDecl::All { source, .. } => {
                    let module_specifier = interner.resolve(source.value).to_string();
                    let target_module_name =
                        module_name_from_specifier(&module_specifier).to_string();
                    let target_module_id = module_id_from_name(&target_module_name);
                    let symbol = "*".to_string();
                    imports.push(Import {
                        module_specifier,
                        symbol: symbol.clone(),
                        alias: None,
                        module_id: target_module_id,
                        symbol_id: symbol_id_from_name(
                            &target_module_name,
                            SymbolScope::Module,
                            &symbol,
                        ),
                        scope: SymbolScope::Module,
                        signature_hash: 0,
                        type_signature: None,
                        runtime_global_slot: None,
                    });
                }
                ast::ExportDecl::Default { expression, .. } => {
                    if let ast::Expression::Identifier(identifier) = expression.as_ref() {
                        exports.push(ExportBinding {
                            exported_name: "default".to_string(),
                            local_name: interner.resolve(identifier.name).to_string(),
                        });
                    }
                }
            },
            _ => {}
        }
    }

    let mut seen_export_symbol_ids = HashSet::new();
    for export in exports {
        let (symbol_type, index) = if let Some(index) = bytecode_module
            .functions
            .iter()
            .position(|function| function.name == export.local_name)
        {
            (SymbolType::Function, index)
        } else if let Some(index) = bytecode_module
            .classes
            .iter()
            .position(|class| class.name == export.local_name)
        {
            (SymbolType::Class, index)
        } else {
            continue;
        };

        let symbol_id =
            symbol_id_from_name(&module_name, SymbolScope::Module, &export.exported_name);
        if !seen_export_symbol_ids.insert(symbol_id) {
            continue;
        }

        bytecode_module.exports.push(Export {
            name: export.exported_name,
            symbol_type: symbol_type.clone(),
            index,
            symbol_id,
            scope: SymbolScope::Module,
            signature_hash: 0,
            type_signature: None,
            runtime_global_slot: None,
            nominal_type: None,
        });
    }

    bytecode_module.imports = imports;
}

/// Disassemble a function's bytecode into human-readable form
pub fn disassemble_function(func: &Function) -> String {
    use std::fmt::Write;

    let mut output = String::new();
    let code = &func.code;
    let mut offset = 0;

    while offset < code.len() {
        let opcode_byte = code[offset];
        if let Some(opcode) = Opcode::from_u8(opcode_byte) {
            write!(output, "    {:04x}: {:?}", offset, opcode).unwrap();
            offset += 1;

            // Read operands based on opcode
            let operand_size = codegen::emit::opcode_size(opcode) - 1;
            if operand_size > 0 && offset + operand_size <= code.len() {
                match operand_size {
                    2 => {
                        let val = u16::from_le_bytes([code[offset], code[offset + 1]]);
                        write!(output, " {}", val).unwrap();
                    }
                    4 => {
                        let val = i32::from_le_bytes([
                            code[offset],
                            code[offset + 1],
                            code[offset + 2],
                            code[offset + 3],
                        ]);
                        write!(output, " {}", val).unwrap();
                    }
                    6 => {
                        let val1 = u32::from_le_bytes([
                            code[offset],
                            code[offset + 1],
                            code[offset + 2],
                            code[offset + 3],
                        ]);
                        let val2 = u16::from_le_bytes([code[offset + 4], code[offset + 5]]);
                        write!(output, " {} {}", val1, val2).unwrap();
                    }
                    8 => {
                        let val = f64::from_le_bytes([
                            code[offset],
                            code[offset + 1],
                            code[offset + 2],
                            code[offset + 3],
                            code[offset + 4],
                            code[offset + 5],
                            code[offset + 6],
                            code[offset + 7],
                        ]);
                        write!(output, " {}", val).unwrap();
                    }
                    _ => {}
                }
                offset += operand_size;
            }
            writeln!(output).unwrap();
        } else {
            writeln!(
                output,
                "    {:04x}: <invalid opcode {:#x}>",
                offset, opcode_byte
            )
            .unwrap();
            offset += 1;
        }
    }

    output
}

/// Disassemble a function's bytecode with source-line comments.
///
/// Each opcode line is followed by `; L{line}:{col} | {source_snippet}` when
/// debug info and source text are available.
pub fn disassemble_function_annotated(
    func: &Function,
    debug_info: Option<&bytecode::FunctionDebugInfo>,
    source_lines: &[&str],
) -> String {
    use std::fmt::Write;

    let mut output = String::new();
    let code = &func.code;
    let mut offset = 0;

    while offset < code.len() {
        let opcode_byte = code[offset];
        if let Some(opcode) = Opcode::from_u8(opcode_byte) {
            write!(
                output,
                "    {:04x}: {:<22}",
                offset,
                format!("{:?}", opcode)
            )
            .unwrap();
            let instr_start_offset = offset;
            offset += 1;

            let operand_size = codegen::emit::opcode_size(opcode) - 1;
            if operand_size > 0 && offset + operand_size <= code.len() {
                match operand_size {
                    1 => {
                        write!(output, " {:3}", code[offset]).unwrap();
                    }
                    2 => {
                        let val = u16::from_le_bytes([code[offset], code[offset + 1]]);
                        write!(output, " {:5}", val).unwrap();
                    }
                    4 => {
                        let val = i32::from_le_bytes([
                            code[offset],
                            code[offset + 1],
                            code[offset + 2],
                            code[offset + 3],
                        ]);
                        write!(output, " {:10}", val).unwrap();
                    }
                    6 => {
                        let val1 = u32::from_le_bytes([
                            code[offset],
                            code[offset + 1],
                            code[offset + 2],
                            code[offset + 3],
                        ]);
                        let val2 = u16::from_le_bytes([code[offset + 4], code[offset + 5]]);
                        write!(output, " {} {}", val1, val2).unwrap();
                    }
                    8 => {
                        let val = f64::from_le_bytes([
                            code[offset],
                            code[offset + 1],
                            code[offset + 2],
                            code[offset + 3],
                            code[offset + 4],
                            code[offset + 5],
                            code[offset + 6],
                            code[offset + 7],
                        ]);
                        write!(output, " {:.6}", val).unwrap();
                    }
                    _ => {}
                }
                offset += operand_size;
            }

            // Append source annotation if debug info is available
            if let Some(dbg) = debug_info {
                if let Some(entry) = dbg
                    .line_table
                    .iter()
                    .filter(|e| e.bytecode_offset as usize <= instr_start_offset)
                    .last()
                {
                    let line = entry.line as usize;
                    let col = entry.column;
                    if !source_lines.is_empty() && line > 0 && line <= source_lines.len() {
                        let src = source_lines[line - 1].trim();
                        // Truncate long lines so output stays readable
                        let snippet = if src.len() > 60 { &src[..60] } else { src };
                        write!(output, "  ; L{}:{} | {}", line, col, snippet).unwrap();
                    } else {
                        write!(output, "  ; L{}:{}", line, col).unwrap();
                    }
                }
            }

            writeln!(output).unwrap();
        } else {
            writeln!(
                output,
                "    {:04x}: <invalid opcode {:#x}>",
                offset, opcode_byte
            )
            .unwrap();
            offset += 1;
        }
    }

    output
}

/// Replace `:N` type-id suffixes in IR text with human-readable type names.
///
/// Registers are printed as `r{id}:{typeId}` (e.g., `r0:16`).  This function
/// rewrites them to `r{id}:{typename}` (e.g., `r0:int`) using the TypeContext.
fn annotate_type_ids(text: &str, type_ctx: &TypeContext) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        // Match `r` followed by one or more digits then `:` then one or more digits
        if chars[i] == 'r' && i + 1 < n && chars[i + 1].is_ascii_digit() {
            let r_start = i;
            i += 1; // skip 'r'
                    // Consume register-id digits
            while i < n && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < n && chars[i] == ':' && i + 1 < n && chars[i + 1].is_ascii_digit() {
                let colon_pos = i;
                i += 1; // skip ':'
                let tid_start = i;
                while i < n && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let tid_str: String = chars[tid_start..i].iter().collect();
                if let Ok(n_val) = tid_str.parse::<u32>() {
                    // Output r{id} then :typename
                    out.extend(chars[r_start..colon_pos].iter());
                    out.push(':');
                    out.push_str(&type_ctx.format_type(TypeId::new(n_val)));
                    continue;
                }
                // Fallback: not a valid u32, output as-is
                out.extend(chars[r_start..i].iter());
                continue;
            }
            // Not a register pattern (no colon+digits), emit as-is
            out.extend(chars[r_start..i].iter());
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Dump the IR module to stderr with source-span annotations and type names.
///
/// Activated by the `RAYA_DEBUG_DUMP_IR` environment variable.
/// Pass `RAYA_DEBUG_DUMP_TYPES` to also get the full type table.
fn dump_ir_module(
    ir_module: &ir::IrModule,
    source_text: Option<&str>,
    type_ctx: Option<&TypeContext>,
) {
    let source_lines: Vec<&str> = source_text.map(|s| s.lines().collect()).unwrap_or_default();

    eprintln!("\n╔═══════════════════════════════════════════════════════════════╗");
    eprintln!("║  RAYA_DEBUG_DUMP_IR — Annotated IR Module                     ║");
    eprintln!("╚═══════════════════════════════════════════════════════════════╝");

    for (func_idx, func) in ir_module.functions.iter().enumerate() {
        eprintln!(
            "\n─── fn{}: {} (params={}) ───",
            func_idx,
            func.name,
            func.params.len()
        );
        for block in &func.blocks {
            let label = block.label.as_deref().unwrap_or("");
            if label.is_empty() {
                eprintln!("  {}:", block.id);
            } else {
                eprintln!("  {}: ; {}", block.id, label);
            }
            let has_spans = block.instruction_spans.len() == block.instructions.len();
            for (i, instr) in block.instructions.iter().enumerate() {
                let raw = format_ir_instr_pretty(instr);
                let ir_text = if let Some(ctx) = type_ctx {
                    annotate_type_ids(&raw, ctx)
                } else {
                    raw
                };
                if has_spans {
                    let span = &block.instruction_spans[i];
                    if span.line > 0 {
                        let line = span.line as usize;
                        let src_snippet = if !source_lines.is_empty() && line <= source_lines.len()
                        {
                            let s = source_lines[line - 1].trim();
                            if s.len() > 70 {
                                &s[..70]
                            } else {
                                s
                            }
                        } else {
                            ""
                        };
                        if src_snippet.is_empty() {
                            eprintln!("    {:<60}  ; L{}:{}", ir_text, span.line, span.column);
                        } else {
                            eprintln!(
                                "    {:<60}  ; L{}:{} | {}",
                                ir_text, span.line, span.column, src_snippet
                            );
                        }
                    } else {
                        eprintln!("    {}", ir_text);
                    }
                } else {
                    eprintln!("    {}", ir_text);
                }
            }
            // Terminator
            let raw_term = format!("{}", block.terminator);
            let term_text = if let Some(ctx) = type_ctx {
                annotate_type_ids(&raw_term, ctx)
            } else {
                raw_term
            };
            if block.terminator_span.line > 0 {
                let line = block.terminator_span.line as usize;
                let src_snippet = if !source_lines.is_empty() && line <= source_lines.len() {
                    let s = source_lines[line - 1].trim();
                    if s.len() > 70 {
                        &s[..70]
                    } else {
                        s
                    }
                } else {
                    ""
                };
                if src_snippet.is_empty() {
                    eprintln!(
                        "    {:<60}  ; L{}:{}",
                        term_text, block.terminator_span.line, block.terminator_span.column
                    );
                } else {
                    eprintln!(
                        "    {:<60}  ; L{}:{} | {}",
                        term_text,
                        block.terminator_span.line,
                        block.terminator_span.column,
                        src_snippet
                    );
                }
            } else {
                eprintln!("    {}", term_text);
            }
        }
    }
    eprintln!();
}

/// Dump the entire TypeContext as a numbered table to stderr.
///
/// Activated by the `RAYA_DEBUG_DUMP_TYPES` environment variable.
fn dump_type_table(type_ctx: &TypeContext) {
    eprintln!("\n╔═══════════════════════════════════════════════════════════════╗");
    eprintln!("║  RAYA_DEBUG_DUMP_TYPES — Interned Type Table                  ║");
    eprintln!("╚═══════════════════════════════════════════════════════════════╝");
    eprintln!("  {:>6}  {}", "TypeId", "Description");
    eprintln!("  {:>6}  {}", "------", "-----------");
    for (id, desc) in type_ctx.format_type_table() {
        eprintln!("  {:>6}  {}", id, desc);
    }
    eprintln!("  {:>6}  {}", "u32::MAX", "<unresolved>");
    eprintln!();
}

/// Format a single IR instruction for display (delegates to the pretty printer)
fn format_ir_instr_pretty(instr: &ir::IrInstr) -> String {
    ir::format_instr_pub(instr)
}

/// Dump annotated bytecode for every function to stderr.
///
/// Activated by the `RAYA_DEBUG_DUMP_BYTECODE` environment variable.
fn dump_bytecode_module(module: &Module, source_text: Option<&str>) {
    let source_lines: Vec<&str> = source_text.map(|s| s.lines().collect()).unwrap_or_default();

    eprintln!("\n╔═══════════════════════════════════════════════════════════════╗");
    eprintln!("║  RAYA_DEBUG_DUMP_BYTECODE — Annotated Bytecode                ║");
    eprintln!("╚═══════════════════════════════════════════════════════════════╝");

    for (i, func) in module.functions.iter().enumerate() {
        let debug_info = module.debug_info.as_ref().and_then(|d| d.functions.get(i));
        eprintln!(
            "\n─── fn{}: {} (locals={}, params={}, {} bytes) ───",
            i,
            func.name,
            func.local_count,
            func.param_count,
            func.code.len()
        );
        let disasm = disassemble_function_annotated(func, debug_info, &source_lines);
        eprint!("{}", disasm);
    }
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn compile_source(source: &str, module_identity: Option<&str>) -> Module {
        let parser = Parser::new(source).expect("lexer failure");
        let (ast, interner) = parser.parse().expect("parse failure");

        let compiler = if let Some(module_identity) = module_identity {
            Compiler::new(TypeContext::new(), &interner).with_module_identity(module_identity)
        } else {
            Compiler::new(TypeContext::new(), &interner)
        };

        compiler
            .compile_via_ir(&ast)
            .expect("compile_via_ir should succeed")
    }

    #[test]
    fn test_compile_via_ir_populates_symbol_link_tables() {
        let source = r#"
            export function foo(): number { return 1; }
            export class MyClass {}
            import { depFn as localDepFn } from "./dep";
            import depDefault from "pkg@1.2.3";
            import * as ns from "@org/pkg@^2.0.0";
        "#;

        let module = compile_source(source, None);
        let module_name = module.metadata.name.clone();

        assert!(
            module.exports.iter().any(|export| {
                export.name == "foo"
                    && export.symbol_type == SymbolType::Function
                    && export.symbol_id
                        == symbol_id_from_name(&module_name, SymbolScope::Module, "foo")
                    && export.signature_hash == 0
            }),
            "expected function export metadata for foo"
        );

        assert!(
            module.exports.iter().any(|export| {
                export.name == "MyClass"
                    && export.symbol_type == SymbolType::Class
                    && export.symbol_id
                        == symbol_id_from_name(&module_name, SymbolScope::Module, "MyClass")
                    && export.signature_hash == 0
            }),
            "expected class export metadata for MyClass"
        );

        let dep_import = module
            .imports
            .iter()
            .find(|import| import.module_specifier == "./dep" && import.symbol == "depFn")
            .expect("missing ./dep import");
        assert_eq!(dep_import.module_id, module_id_from_name("./dep"));
        assert_eq!(
            dep_import.symbol_id,
            symbol_id_from_name("./dep", SymbolScope::Module, "depFn")
        );
        assert_eq!(dep_import.scope, SymbolScope::Module);
        assert_eq!(dep_import.alias.as_deref(), Some("localDepFn"));

        let pkg_import = module
            .imports
            .iter()
            .find(|import| import.module_specifier == "pkg@1.2.3" && import.symbol == "default")
            .expect("missing package default import");
        assert_eq!(pkg_import.module_id, module_id_from_name("pkg"));
        assert_eq!(
            pkg_import.symbol_id,
            symbol_id_from_name("pkg", SymbolScope::Module, "default")
        );
        assert_eq!(pkg_import.alias.as_deref(), Some("depDefault"));

        let scoped_import = module
            .imports
            .iter()
            .find(|import| import.module_specifier == "@org/pkg@^2.0.0" && import.symbol == "*")
            .expect("missing scoped namespace import");
        assert_eq!(scoped_import.module_id, module_id_from_name("@org/pkg"));
        assert_eq!(
            scoped_import.symbol_id,
            symbol_id_from_name("@org/pkg", SymbolScope::Module, "*")
        );
        assert_eq!(scoped_import.alias.as_deref(), Some("ns"));
    }

    #[test]
    fn test_compile_via_ir_uses_module_identity_for_symbol_ids() {
        let source = r#"export function foo(): number { return 1; }"#;
        let identity = "/project/src/main.raya";
        let module = compile_source(source, Some(identity));

        assert_eq!(module.metadata.name, identity);
        assert!(module.exports.iter().any(|export| {
            export.name == "foo"
                && export.symbol_id == symbol_id_from_name(identity, SymbolScope::Module, "foo")
        }));
    }

    #[test]
    fn test_compile_via_ir_strict_disables_unresolved_runtime_fallback() {
        let source = r#"
            let o;
            o.missing();
        "#;
        let parser = Parser::new(source).expect("lexer failure");
        let (ast, interner) = parser.parse().expect("parse failure");
        let compiler = Compiler::new(TypeContext::new(), &interner)
            .with_semantic_profile(SemanticProfile::raya());

        let err = compiler
            .compile_via_ir(&ast)
            .expect_err("strict no-fallback compile should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("unresolved member call")
                || msg.contains("unresolved member property")
                || msg.contains("strict mode forbids dynamic member resolution")
                || msg.contains("strict mode forbids dynamic JS property access"),
            "expected unresolved member resolution diagnostic, got: {msg}"
        );
    }
}
