use crate::compile;
use crate::compile::TsCompilerOptions;
use crate::error::RuntimeError;
use crate::BuiltinMode;
use raya_engine::compiler::module::{
    specialization_template_from_symbol, BuiltinSurfaceMode, LateLinkRequirement,
};
use raya_engine::compiler::module::{ModuleCompileError, ModuleCompiler as BinaryModuleCompiler};
use raya_engine::compiler::{module_id_from_name, SymbolType};
use raya_engine::parser::{Interner, ParseGoal, Parser};
use raya_engine::parser::checker::TsTypeFlags;
use raya_engine::semantics::{SemanticProfile, SourceKind};
use raya_engine::vm::module::ModuleLinker;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct CompiledProgram {
    pub entry_path: PathBuf,
    pub module_order: Vec<PathBuf>,
    pub merged_source: String,
    pub entry: crate::CompiledModule,
    pub dependencies: Vec<crate::CompiledModule>,
    /// Unresolved declaration-backed module requirements to satisfy at runtime.
    pub late_link_requirements: Vec<LateLinkRequirement>,
}

pub struct ProgramDiagnostics {
    pub entry_path: PathBuf,
    pub module_order: Vec<PathBuf>,
    pub merged_source: String,
    pub diagnostics: compile::CheckDiagnostics,
}

#[derive(Debug, Clone)]
pub struct ProgramCompiler {
    pub builtin_mode: BuiltinMode,
    pub semantic_profile: SemanticProfile,
    pub ts_options: Option<TsCompilerOptions>,
    pub compile_options: Option<compile::CompileOptions>,
}

impl ProgramCompiler {
    pub fn compile_program_file(&self, path: &Path) -> Result<CompiledProgram, RuntimeError> {
        if !self.can_use_binary_module_pipeline() {
            return Err(RuntimeError::Dependency(
                "compile_program_file now requires the binary module pipeline; disable unsupported compile options or type-mode overrides".to_string(),
            ));
        }
        self.compile_program_file_binary(path)
            .map_err(map_module_compile_error)
    }

    fn compile_program_file_binary(
        &self,
        path: &Path,
    ) -> Result<CompiledProgram, ModuleCompileError> {
        let entry_path = path
            .canonicalize()
            .map_err(|e| ModuleCompileError::IoError {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
        let project_root = entry_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let builtin_globals = crate::Runtime::builtin_global_exports_for_mode(self.builtin_mode)
            .map_err(|error| ModuleCompileError::TypeError {
                path: entry_path.clone(),
                message: format!("Failed to load builtin bytecode contracts: {}", error),
            })?;

        let mut compiler = BinaryModuleCompiler::new(project_root)
            .with_semantic_profile(self.semantic_profile)
            .with_ts_type_flags(
                self.ts_options
                    .as_ref()
                    .map(TsCompilerOptions::effective_typecheck_flags),
            )
            .with_builtin_surface_mode(self.builtin_surface_mode())
            .with_builtin_globals_override(builtin_globals);
        let mut compiled_modules = compiler.compile(&entry_path)?;
        if std::env::var("RAYA_DEBUG_MODULE_NATIVES").is_ok() {
            for compiled in &compiled_modules {
                eprintln!(
                    "[module-natives] module='{}' path='{}' natives={}",
                    compiled.bytecode.metadata.name,
                    compiled.path.display(),
                    compiled.bytecode.native_functions.len()
                );
                for (idx, name) in compiled.bytecode.native_functions.iter().enumerate() {
                    eprintln!("  [{idx}] {name}");
                }
            }
        }
        self.run_post_link_cross_module_monomorphization_pass(&mut compiled_modules)?;
        self.validate_compiled_module_links(&compiled_modules)?;
        let late_link_requirements = compiler.late_link_requirements();
        let module_order = compiled_modules
            .iter()
            .filter(|module| !module.declaration_only)
            .map(|module| module.path.clone())
            .collect::<Vec<_>>();

        let mut entry = None;
        let mut dependencies = Vec::new();
        for compiled in compiled_modules {
            if compiled.declaration_only {
                continue;
            }
            let runtime_module = crate::CompiledModule {
                module: compiled.bytecode,
                interner: None,
            };
            if compiled.path == entry_path {
                entry = Some(runtime_module);
            } else {
                dependencies.push(runtime_module);
            }
        }

        let entry = entry.ok_or_else(|| ModuleCompileError::IoError {
            path: entry_path.clone(),
            message: "Entry module missing from compiled module graph".to_string(),
        })?;

        Ok(CompiledProgram {
            entry_path,
            module_order,
            merged_source: String::new(),
            entry,
            dependencies,
            late_link_requirements,
        })
    }

    fn compile_program_source_binary(
        &self,
        source: &str,
        virtual_entry_path: &Path,
    ) -> Result<CompiledProgram, ModuleCompileError> {
        let entry_path = virtual_entry_path.to_path_buf();
        let project_root = entry_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let builtin_globals = crate::Runtime::builtin_global_exports_for_mode(self.builtin_mode)
            .map_err(|error| ModuleCompileError::TypeError {
                path: entry_path.clone(),
                message: format!("Failed to load builtin bytecode contracts: {}", error),
            })?;

        let mut compiler = BinaryModuleCompiler::new(project_root)
            .with_semantic_profile(self.semantic_profile)
            .with_ts_type_flags(
                self.ts_options
                    .as_ref()
                    .map(TsCompilerOptions::effective_typecheck_flags),
            )
            .with_builtin_surface_mode(self.builtin_surface_mode())
            .with_builtin_globals_override(builtin_globals);
        let mut compiled_modules =
            compiler.compile_with_virtual_entry_source(&entry_path, source.to_string())?;
        if std::env::var("RAYA_DEBUG_MODULE_NATIVES").is_ok() {
            for compiled in &compiled_modules {
                eprintln!(
                    "[module-natives] module='{}' path='{}' natives={}",
                    compiled.bytecode.metadata.name,
                    compiled.path.display(),
                    compiled.bytecode.native_functions.len()
                );
                for (idx, name) in compiled.bytecode.native_functions.iter().enumerate() {
                    eprintln!("  [{idx}] {name}");
                }
            }
        }
        self.run_post_link_cross_module_monomorphization_pass(&mut compiled_modules)?;
        self.validate_compiled_module_links(&compiled_modules)?;
        let late_link_requirements = compiler.late_link_requirements();
        let module_order = compiled_modules
            .iter()
            .filter(|module| !module.declaration_only)
            .map(|module| module.path.clone())
            .collect::<Vec<_>>();

        let mut entry = None;
        let mut dependencies = Vec::new();
        for compiled in compiled_modules {
            if compiled.declaration_only {
                continue;
            }
            let runtime_module = crate::CompiledModule {
                module: compiled.bytecode,
                interner: None,
            };
            if compiled.path == entry_path {
                entry = Some(runtime_module);
            } else {
                dependencies.push(runtime_module);
            }
        }

        let entry = entry.ok_or_else(|| ModuleCompileError::IoError {
            path: entry_path.clone(),
            message: "Entry module missing from compiled module graph".to_string(),
        })?;

        Ok(CompiledProgram {
            entry_path,
            module_order,
            merged_source: String::new(),
            entry,
            dependencies,
            late_link_requirements,
        })
    }

    fn validate_compiled_module_links(
        &self,
        compiled_modules: &[raya_engine::compiler::module::CompiledModule],
    ) -> Result<(), ModuleCompileError> {
        let mut linker = ModuleLinker::new();

        for compiled in compiled_modules {
            linker
                .add_module(Arc::new(compiled.bytecode.clone()))
                .map_err(|message| ModuleCompileError::TypeError {
                    path: compiled.path.clone(),
                    message: format!("Module linker registration failed: {}", message),
                })?;
        }

        for compiled in compiled_modules {
            linker.link_module(&compiled.bytecode).map_err(|error| {
                ModuleCompileError::TypeError {
                    path: compiled.path.clone(),
                    message: format!("Module link validation failed: {}", error),
                }
            })?;
        }

        Ok(())
    }

    /// Post-link cross-module monomorphization normalization + validation.
    ///
    /// This pass:
    /// 1. Normalizes specialized imports (`__mono_...`) to canonical export IDs/signatures
    ///    in target modules.
    /// 2. Validates specialization contracts against target module metadata.
    fn run_post_link_cross_module_monomorphization_pass(
        &self,
        compiled_modules: &mut [raya_engine::compiler::module::CompiledModule],
    ) -> Result<(), ModuleCompileError> {
        let mut modules_by_id = HashMap::new();
        for compiled in compiled_modules.iter() {
            modules_by_id.insert(
                module_id_from_name(&compiled.bytecode.metadata.name),
                (
                    compiled.bytecode.metadata.name.clone(),
                    compiled.bytecode.exports.clone(),
                    compiled.bytecode.metadata.template_symbol_table.clone(),
                    compiled.bytecode.metadata.mono_debug_map.clone(),
                    compiled.declaration_only,
                ),
            );
        }

        for compiled in compiled_modules.iter_mut() {
            if compiled.declaration_only {
                continue;
            }

            for import in &mut compiled.bytecode.imports {
                if import.symbol == "*" {
                    continue;
                }

                let Some((
                    target_module_name,
                    target_exports,
                    target_template_symbols,
                    target_mono_debug_map,
                    target_is_decl_only,
                )) = modules_by_id.get(&import.module_id)
                else {
                    continue;
                };

                if specialization_template_from_symbol(&import.symbol).is_some() {
                    if let Some(exported) = target_exports
                        .iter()
                        .find(|export| export.name == import.symbol)
                        .or_else(|| {
                            target_exports
                                .iter()
                                .find(|export| export.symbol_id == import.symbol_id)
                        })
                    {
                        import.symbol_id = exported.symbol_id;
                        import.scope = exported.scope;
                        import.signature_hash = exported.signature_hash;
                        import.type_signature = exported.type_signature.clone();
                    }
                }

                let Some(template_symbol) = specialization_template_from_symbol(&import.symbol)
                else {
                    continue;
                };
                if *target_is_decl_only {
                    // Declaration-backed imports are validated at runtime late-link stage.
                    continue;
                }

                let Some(exported) = target_exports
                    .iter()
                    .find(|export| export.symbol_id == import.symbol_id)
                else {
                    return Err(ModuleCompileError::TypeError {
                        path: compiled.path.clone(),
                        message: format!(
                            "Post-link specialization validation failed: unresolved export id {} for import '{}'",
                            import.symbol_id, import.symbol
                        ),
                    });
                };

                let Some(mono_entry) = target_mono_debug_map
                    .iter()
                    .find(|entry| entry.specialized_symbol == exported.name)
                else {
                    return Err(ModuleCompileError::TypeError {
                        path: compiled.path.clone(),
                        message: format!(
                            "Post-link specialization contract missing for '{}': target module '{}' does not expose mono-debug metadata",
                            import.symbol, target_module_name
                        ),
                    });
                };

                let template_matches = mono_entry.template_id
                    == format!("fn-template:{template_symbol}")
                    || mono_entry.template_id == format!("class-template:{template_symbol}")
                    || mono_entry
                        .template_id
                        .ends_with(&format!(":{template_symbol}"));
                if !template_matches {
                    return Err(ModuleCompileError::TypeError {
                        path: compiled.path.clone(),
                        message: format!(
                            "Post-link specialization template mismatch for '{}': expected '{}', got '{}'",
                            import.symbol, template_symbol, mono_entry.template_id
                        ),
                    });
                }

                if exported.symbol_type == SymbolType::Function {
                    let has_template_symbol = target_template_symbols
                        .iter()
                        .any(|entry| entry.symbol == template_symbol);
                    let has_template_export = target_exports
                        .iter()
                        .any(|entry| entry.name == template_symbol);
                    let has_template_contract = has_template_symbol || has_template_export;
                    if !has_template_contract {
                        return Err(ModuleCompileError::TypeError {
                            path: compiled.path.clone(),
                            message: format!(
                                "Post-link specialization contract missing function template '{}' in module '{}'",
                                template_symbol, target_module_name
                            ),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn can_use_binary_module_pipeline(&self) -> bool {
        if self.compile_options.is_some() {
            return false;
        }

        true
    }

    fn builtin_surface_mode(&self) -> BuiltinSurfaceMode {
        match self.builtin_mode {
            BuiltinMode::RayaStrict => BuiltinSurfaceMode::RayaStrict,
            BuiltinMode::NodeCompat => BuiltinSurfaceMode::NodeCompat,
        }
    }

    pub fn check_program_file(&self, path: &Path) -> Result<ProgramDiagnostics, RuntimeError> {
        let program = self.compile_program_file(path)?;
        let source = fs::read_to_string(path).map_err(RuntimeError::Io)?;
        let diagnostics = compile::CheckDiagnostics {
            errors: Vec::new(),
            bind_errors: Vec::new(),
            warnings: Vec::new(),
            source,
            user_offset: 0,
        };

        Ok(ProgramDiagnostics {
            entry_path: program.entry_path,
            module_order: program.module_order,
            merged_source: String::new(),
            diagnostics,
        })
    }

    pub fn compile_program_source(
        &self,
        source: &str,
        virtual_entry_path: &Path,
    ) -> Result<CompiledProgram, RuntimeError> {
        if !self.can_use_binary_module_pipeline() {
            return Err(RuntimeError::Dependency(
                "compile_program_source now requires the binary module pipeline; disable unsupported compile options or type-mode overrides".to_string(),
            ));
        }
        self.enforce_dynamic_import_policy(source)?;

        let mut program = self
            .compile_program_source_binary(source, virtual_entry_path)
            .map_err(map_module_compile_error)?;

        let entry_name = virtual_entry_path.to_string_lossy().to_string();
        if !entry_name.is_empty() {
            program.entry.module.metadata.name = entry_name;
        }
        program.entry.interner = Some(parse_interner(source, virtual_entry_path)?);

        let compiled_entry_path = program.entry_path.clone();
        for module_path in &mut program.module_order {
            if *module_path == compiled_entry_path {
                *module_path = virtual_entry_path.to_path_buf();
            }
        }
        program.entry_path = virtual_entry_path.to_path_buf();

        Ok(program)
    }

    pub fn check_program_source(
        &self,
        source: &str,
        virtual_entry_path: &Path,
    ) -> Result<ProgramDiagnostics, RuntimeError> {
        let program = self.compile_program_source(source, virtual_entry_path)?;
        let diagnostics = compile::CheckDiagnostics {
            errors: Vec::new(),
            bind_errors: Vec::new(),
            warnings: Vec::new(),
            source: source.to_string(),
            user_offset: 0,
        };

        Ok(ProgramDiagnostics {
            entry_path: program.entry_path,
            module_order: program.module_order,
            merged_source: String::new(),
            diagnostics,
        })
    }

    fn enforce_dynamic_import_policy(&self, source: &str) -> Result<(), RuntimeError> {
        if matches!(self.semantic_profile.source_kind, SourceKind::Raya)
            && looks_like_dynamic_import(source)
        {
            return Err(RuntimeError::TypeCheck(
                "Dynamic import is not supported in strict mode. Use static import declarations."
                    .to_string(),
            ));
        }
        Ok(())
    }
}

fn looks_like_dynamic_import(source: &str) -> bool {
    source.contains("import(") || source.contains("import (")
}

fn parse_interner(source: &str, virtual_entry_path: &Path) -> Result<Interner, RuntimeError> {
    let parser = Parser::new_with_mode(
        source,
        SemanticProfile::from_path(virtual_entry_path).parser_mode(),
    )
    .map_err(|errors| {
        RuntimeError::Lex(
            errors
                .iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?
    .with_goal(ParseGoal::from_path(virtual_entry_path));
    let (_, interner) = parser.parse().map_err(|errors| {
        RuntimeError::Parse(
            errors
                .iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    Ok(interner)
}

fn map_module_compile_error(error: ModuleCompileError) -> RuntimeError {
    match error {
        ModuleCompileError::Resolution(error) => {
            RuntimeError::Dependency(format!("Module resolution error: {error}"))
        }
        ModuleCompileError::CircularDependency(error) => {
            RuntimeError::Dependency(format!("Circular dependency: {error}"))
        }
        ModuleCompileError::IoError { path, message } => RuntimeError::Io(std::io::Error::other(
            format!("{}: {}", path.display(), message),
        )),
        ModuleCompileError::LexError { path, message } => {
            RuntimeError::Lex(format!("{}: {}", path.display(), message))
        }
        ModuleCompileError::ParseError { path, message } => {
            RuntimeError::Parse(format!("{}: {}", path.display(), message))
        }
        ModuleCompileError::TypeError { path, message } => {
            RuntimeError::TypeCheck(format!("{}: {}", path.display(), message))
        }
        ModuleCompileError::CompileError { source, .. } => RuntimeError::Compile(source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn detects_dynamic_import_syntax() {
        assert!(looks_like_dynamic_import(r#"const x = import("x");"#));
        assert!(looks_like_dynamic_import(r#"const x = import ("x");"#));
        assert!(!looks_like_dynamic_import(r#"import x from "std:path";"#));
    }

    #[test]
    fn strict_mode_rejects_dynamic_import() {
        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let err = compiler
            .enforce_dynamic_import_policy(r#"const x = import("std:path");"#)
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Dynamic import is not supported in strict mode"));
    }

    #[test]
    fn compile_program_source_with_std_stream_import_succeeds() {
        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let result = compiler.compile_program_source(
            r#"
            import { ReadableStream } from "std:stream";
            let streamRef = ReadableStream;
            return 1;
            "#,
            Path::new("/virtual/main.raya"),
        );

        assert!(
            result.is_ok(),
            "std:stream import should compile through linker path: {:?}",
            result.err()
        );
    }

    #[test]
    fn compile_program_file_emits_binary_dependency_modules_for_local_imports() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let utils_path = temp.path().join("utils.raya");

        fs::write(
            &utils_path,
            "export function inc(x: number): number { return x + 1; }",
        )
        .expect("write utils");
        fs::write(
            &main_path,
            r#"
            import { inc } from "./utils";
            let x: number = 41;
            return inc(x);
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");
        assert!(
            program.merged_source.is_empty(),
            "binary pipeline should not depend on merged source rewrite"
        );
        assert_eq!(program.module_order.len(), 2);
        assert_eq!(program.dependencies.len(), 1);
        assert!(!program.entry.module.imports.is_empty());
        assert_eq!(
            program.entry.module.metadata.name,
            main_path
                .canonicalize()
                .expect("canonical entry")
                .to_string_lossy()
        );
        assert_eq!(
            program.dependencies[0].module.metadata.name,
            utils_path
                .canonicalize()
                .expect("canonical utils")
                .to_string_lossy()
        );
        assert!(
            program.late_link_requirements.is_empty(),
            "pure source graph should have no unresolved late-link requirements"
        );
    }

    #[test]
    fn compile_program_file_uses_per_module_profiles_in_mixed_js_raya_graph() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.js");
        let helper_path = temp.path().join("helper.raya");

        fs::write(
            &helper_path,
            r#"
            export function inc(x: number): number {
                return x + 1;
            }
            "#,
        )
        .expect("write helper");
        fs::write(
            &main_path,
            r#"
            import { inc } from "./helper";
            let x = 41;
            return inc(x);
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::NodeCompat,
            semantic_profile: SemanticProfile::js(),
            ts_options: None,
            compile_options: None,
        };

        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile mixed graph");

        assert_eq!(program.module_order.len(), 2);
        assert_eq!(
            program.entry.module.metadata.name,
            main_path
                .canonicalize()
                .expect("canonical entry")
                .to_string_lossy()
        );
        assert_eq!(
            program.dependencies[0].module.metadata.name,
            helper_path
                .canonicalize()
                .expect("canonical helper")
                .to_string_lossy()
        );
    }

    #[test]
    fn compile_program_file_collects_late_link_requirements_for_declaration_imports() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let decl_path = temp.path().join("dep.d.ts");

        fs::write(
            &main_path,
            r#"
            import { foo } from "./dep";
            let x: number = 1;
            return x;
            "#,
        )
        .expect("write main");
        fs::write(&decl_path, "export function foo(a: number): number;")
            .expect("write declaration");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");
        assert!(
            program.dependencies.is_empty(),
            "declaration-only dependency should not be emitted as runnable dependency module"
        );
        assert_eq!(program.late_link_requirements.len(), 1);
        let requirement = &program.late_link_requirements[0];
        assert!(
            requirement.module_specifiers.contains(&"./dep".to_string()),
            "late-link requirement should track import specifier"
        );
        assert!(
            requirement
                .symbols
                .iter()
                .any(|symbol| symbol.symbol == "foo"),
            "late-link requirement should include imported symbol contract"
        );
    }

    #[test]
    fn compile_program_file_binary_path_rejects_unresolved_import_symbol() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let dep_path = temp.path().join("dep.raya");

        fs::write(&dep_path, "export let y: number = 1;").expect("write dep");
        fs::write(
            &main_path,
            r#"
            import { x } from "./dep";
            return 0;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let result = compiler.compile_program_file(&main_path);
        assert!(
            result.is_err(),
            "expected unresolved import to fail linking"
        );
        let message = format!("{}", result.err().unwrap());
        assert!(
            message.contains("Module link validation failed")
                || message.contains("Symbol 'x' not found")
                || message.contains("Unresolved import"),
            "unexpected error: {}",
            message
        );
    }

    #[test]
    fn compile_program_file_binary_path_supports_std_module_imports() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &main_path,
            r#"
            import { join } from "std:path";
            let p = join;
            return 1;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");
        assert!(
            !program.dependencies.is_empty(),
            "std import should materialize dependency modules in binary pipeline"
        );
    }

    #[test]
    fn compile_program_file_binary_path_supports_node_module_imports() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &main_path,
            r#"
            import { ParsedPath } from "node:path";
            let p = ParsedPath;
            return 1;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");
        assert!(
            !program.dependencies.is_empty(),
            "node import should materialize dependency modules in binary pipeline"
        );
    }

    #[test]
    fn compile_program_file_binary_path_supports_std_default_imports() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &main_path,
            r#"
            import path from "std:path";
            let p = path;
            return 1;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");
        assert!(
            !program.dependencies.is_empty(),
            "std default import should materialize dependency modules in binary pipeline"
        );
        assert!(program
            .entry
            .module
            .imports
            .iter()
            .any(|import| import.module_specifier == "std:path" && import.symbol == "default"));
    }

    #[test]
    fn compile_program_file_rejects_specialized_import_without_template_contract() {
        let temp = TempDir::new().expect("temp dir");
        let dep_path = temp.path().join("dep.raya");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &dep_path,
            r#"
            export function identity__mono_deadbeefcafe(x: number): number { return x; }
            "#,
        )
        .expect("write dep");
        fs::write(
            &main_path,
            r#"
            import { identity__mono_deadbeefcafe } from "./dep";
            return identity__mono_deadbeefcafe(1);
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let error = match compiler.compile_program_file(&main_path) {
            Ok(_) => panic!("expected specialization contract failure"),
            Err(error) => error,
        };
        let message = format!("{error}");
        assert!(
            message.contains("Post-link specialization contract missing function template"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn compile_program_file_accepts_specialized_import_with_template_contract() {
        let temp = TempDir::new().expect("temp dir");
        let dep_path = temp.path().join("dep.raya");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &dep_path,
            r#"
            export function identity<T>(x: T): T { return x; }
            export function identity__mono_deadbeefcafe(x: number): number { return x; }
            "#,
        )
        .expect("write dep");
        fs::write(
            &main_path,
            r#"
            import { identity__mono_deadbeefcafe } from "./dep";
            return identity__mono_deadbeefcafe(1);
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };

        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile with specialization contract");
        assert!(
            !program.entry.module.imports.is_empty(),
            "expected import metadata for specialized symbol"
        );
    }

    #[test]
    fn execute_with_deps_runs_imported_function_for_binary_program() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let utils_path = temp.path().join("utils.raya");

        fs::write(
            &utils_path,
            "export function inc(x: number): number { return x + 1; }",
        )
        .expect("write utils");
        fs::write(
            &main_path,
            r#"
            import { inc } from "./utils";
            let observed: number = inc(41);
            if (observed != 42) {
                throw new Error("unexpected import result");
            }
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        runtime
            .execute_with_deps(&program.entry, &program.dependencies)
            .expect("execute");
    }

    #[test]
    fn execute_program_late_links_declaration_backed_binary_dependency() {
        let temp = TempDir::new().expect("temp dir");
        let dep_source_path = temp.path().join("dep.raya");
        let dep_decl_path = temp.path().join("dep.d.ts");
        let dep_binary_path = temp.path().join("dep.ryb");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &dep_source_path,
            r#"
            export function foo(x: number): number { return x + 1; }
            "#,
        )
        .expect("write dep source");

        let runtime = crate::Runtime::new();
        let dep_module = runtime
            .compile_file(&dep_source_path)
            .expect("compile dep source");
        fs::write(&dep_binary_path, dep_module.encode()).expect("write dep binary");

        fs::remove_file(&dep_source_path).expect("remove dep source");
        fs::write(&dep_decl_path, "export function foo(x: number): number;")
            .expect("write dep declaration");
        fs::write(
            &main_path,
            r#"
            import { foo } from "./dep";
            let observed: number = foo(41);
            return observed;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile main");
        assert!(program.dependencies.is_empty());
        assert_eq!(program.late_link_requirements.len(), 1);

        let result = runtime.execute_program(&program).expect("execute program");
        assert_eq!(result, crate::Value::f64(42.0));
    }

    #[test]
    fn execute_program_late_link_fails_on_signature_mismatch() {
        let temp = TempDir::new().expect("temp dir");
        let dep_source_path = temp.path().join("dep.raya");
        let dep_decl_path = temp.path().join("dep.d.ts");
        let dep_binary_path = temp.path().join("dep.ryb");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &dep_source_path,
            r#"
            export function foo(x: string): string { return x; }
            "#,
        )
        .expect("write dep source");

        let runtime = crate::Runtime::new();
        let dep_module = runtime
            .compile_file(&dep_source_path)
            .expect("compile dep source");
        fs::write(&dep_binary_path, dep_module.encode()).expect("write dep binary");

        fs::remove_file(&dep_source_path).expect("remove dep source");
        fs::write(&dep_decl_path, "export function foo(x: number): number;")
            .expect("write dep declaration");
        fs::write(
            &main_path,
            r#"
            import { foo } from "./dep";
            let observed: number = foo(1);
            return 1;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile main");
        let error = runtime
            .execute_program(&program)
            .expect_err("expected late-link signature mismatch");
        let message = format!("{error}");
        assert!(
            message.contains("Late-link type signature mismatch"),
            "unexpected error: {}",
            message
        );
    }

    #[test]
    fn execute_program_late_link_fails_on_missing_specialization_contract() {
        let temp = TempDir::new().expect("temp dir");
        let dep_source_path = temp.path().join("dep.raya");
        let dep_decl_path = temp.path().join("dep.d.ts");
        let dep_binary_path = temp.path().join("dep.ryb");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &dep_source_path,
            r#"
            export function identity__mono_deadbeefcafe(x: number): number { return x; }
            "#,
        )
        .expect("write dep source");

        let runtime = crate::Runtime::new();
        let dep_module = runtime
            .compile_file(&dep_source_path)
            .expect("compile dep source");
        fs::write(&dep_binary_path, dep_module.encode()).expect("write dep binary");

        fs::remove_file(&dep_source_path).expect("remove dep source");
        fs::write(
            &dep_decl_path,
            "export function identity__mono_deadbeefcafe(x: number): number;",
        )
        .expect("write dep declaration");
        fs::write(
            &main_path,
            r#"
            import { identity__mono_deadbeefcafe } from "./dep";
            return identity__mono_deadbeefcafe(1);
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile main");
        let error = runtime
            .execute_program(&program)
            .expect_err("expected late-link specialization contract failure");
        let message = format!("{error}");
        assert!(
            message.contains("Late-link specialization contract missing template symbol"),
            "unexpected error: {}",
            message
        );
    }

    #[test]
    fn execute_program_late_link_accepts_specialization_contract() {
        let temp = TempDir::new().expect("temp dir");
        let dep_source_path = temp.path().join("dep.raya");
        let dep_decl_path = temp.path().join("dep.d.ts");
        let dep_binary_path = temp.path().join("dep.ryb");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &dep_source_path,
            r#"
            export function identity<T>(x: T): T { return x; }
            export function identity__mono_deadbeefcafe(x: number): number { return x; }
            "#,
        )
        .expect("write dep source");

        let runtime = crate::Runtime::new();
        let dep_module = runtime
            .compile_file(&dep_source_path)
            .expect("compile dep source");
        fs::write(&dep_binary_path, dep_module.encode()).expect("write dep binary");

        fs::remove_file(&dep_source_path).expect("remove dep source");
        fs::write(
            &dep_decl_path,
            "export function identity__mono_deadbeefcafe(x: number): number;",
        )
        .expect("write dep declaration");
        fs::write(
            &main_path,
            r#"
            import { identity__mono_deadbeefcafe } from "./dep";
            return identity__mono_deadbeefcafe(2);
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile main");
        let result = runtime.execute_program(&program).expect("execute program");
        assert_eq!(result, crate::Value::i32(2));
    }

    #[test]
    fn execute_program_late_link_accepts_alpha_equivalent_generic_signatures() {
        let temp = TempDir::new().expect("temp dir");
        let dep_source_path = temp.path().join("dep.raya");
        let dep_decl_path = temp.path().join("dep.d.ts");
        let dep_binary_path = temp.path().join("dep.ryb");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &dep_source_path,
            r#"
            export function identity<T>(x: T): T { return x; }
            "#,
        )
        .expect("write dep source");

        let runtime = crate::Runtime::new();
        let dep_module = runtime
            .compile_file(&dep_source_path)
            .expect("compile dep source");
        fs::write(&dep_binary_path, dep_module.encode()).expect("write dep binary");

        fs::remove_file(&dep_source_path).expect("remove dep source");
        fs::write(&dep_decl_path, "export function identity<U>(x: U): U;")
            .expect("write dep declaration");
        fs::write(
            &main_path,
            r#"
            import { identity } from "./dep";
            return identity(7);
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile main");
        let result = runtime.execute_program(&program).expect("execute program");
        assert_eq!(result, crate::Value::i32(7));
    }

    #[test]
    fn execute_with_deps_hydrates_transitive_constant_imports_for_dependency_functions() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let b_path = temp.path().join("b.raya");
        let c_path = temp.path().join("c.raya");

        fs::write(&c_path, "export let answer: number = 42;").expect("write c");
        fs::write(
            &b_path,
            r#"
            import { answer } from "./c";
            export function getAnswer(): number { return answer; }
            "#,
        )
        .expect("write b");
        fs::write(
            &main_path,
            r#"
            import { getAnswer } from "./b";
            return getAnswer();
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        let result = runtime
            .execute_with_deps(&program.entry, &program.dependencies)
            .expect("execute");
        assert_eq!(result, crate::Value::i32(42));
    }

    #[test]
    fn execute_with_deps_hydrates_direct_constant_imports() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let dep_path = temp.path().join("dep.raya");

        fs::write(&dep_path, "export let answer: number = 42;").expect("write dep");
        fs::write(
            &main_path,
            r#"
            import { answer } from "./dep";
            return answer;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        let result = runtime
            .execute_with_deps(&program.entry, &program.dependencies)
            .expect("execute");
        assert_eq!(result, crate::Value::i32(42));
    }

    #[test]
    fn execute_with_deps_hydrates_direct_class_imports() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let dep_path = temp.path().join("dep.raya");

        fs::write(
            &dep_path,
            r#"
            export class Counter {}
            "#,
        )
        .expect("write dep");
        fs::write(
            &main_path,
            r#"
            import { Counter } from "./dep";
            if (Counter == null) {
                return 0;
            }
            return 42;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        let result = runtime
            .execute_with_deps(&program.entry, &program.dependencies)
            .expect("execute");
        assert_eq!(result, crate::Value::i32(42));
    }

    #[test]
    fn execute_with_deps_hydrates_transitive_class_imports_for_dependency_functions() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let b_path = temp.path().join("b.raya");
        let c_path = temp.path().join("c.raya");

        fs::write(
            &c_path,
            r#"
            export class Counter {}
            "#,
        )
        .expect("write c");
        fs::write(
            &b_path,
            r#"
            import { Counter } from "./c";
            export function getCount(): number {
                if (Counter == null) {
                    return 0;
                }
                return 42;
            }
            "#,
        )
        .expect("write b");
        fs::write(
            &main_path,
            r#"
            import { getCount } from "./b";
            return getCount();
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        let result = runtime
            .execute_with_deps(&program.entry, &program.dependencies)
            .expect("execute");
        assert_eq!(result, crate::Value::i32(42));
    }

    #[test]
    fn execute_with_deps_isolates_global_slots_and_class_ids_across_modules() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let a_path = temp.path().join("a.raya");
        let b_path = temp.path().join("b.raya");

        fs::write(
            &a_path,
            r#"
            export let value: number = 10;
            export class A {}
            export function readA(): number { return value; }
            "#,
        )
        .expect("write a");
        fs::write(
            &b_path,
            r#"
            export let value: number = 20;
            export class B {}
            export function readB(): number { return value; }
            "#,
        )
        .expect("write b");
        fs::write(
            &main_path,
            r#"
            import { readA, A } from "./a";
            import { readB, B } from "./b";

            if (readA() != 10) { return 1; }
            if (readB() != 20) { return 2; }
            if (A == B) { return 3; }

            return 42;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        let result = runtime
            .execute_with_deps(&program.entry, &program.dependencies)
            .expect("execute");
        assert_eq!(result, crate::Value::i32(42));
    }

    #[test]
    fn execute_with_deps_initializes_shared_dependency_once_before_dependents() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let a_path = temp.path().join("a.raya");
        let b_path = temp.path().join("b.raya");
        let c_path = temp.path().join("c.raya");

        fs::write(
            &c_path,
            r#"
            export let counter: number = 0;
            counter = counter + 1;
            export function getCounter(): number { return counter; }
            "#,
        )
        .expect("write c");
        fs::write(
            &a_path,
            r#"
            import { getCounter } from "./c";
            export function fromA(): number { return getCounter(); }
            "#,
        )
        .expect("write a");
        fs::write(
            &b_path,
            r#"
            import { getCounter } from "./c";
            export function fromB(): number { return getCounter(); }
            "#,
        )
        .expect("write b");
        fs::write(
            &main_path,
            r#"
            import { fromA } from "./a";
            import { fromB } from "./b";

            let left = fromA();
            let right = fromB();
            if (left != 1) { return 11; }
            if (right != 1) { return 12; }
            return 42;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        let result = runtime
            .execute_with_deps(&program.entry, &program.dependencies)
            .expect("execute");
        assert_eq!(result, crate::Value::i32(42));
    }

    #[test]
    fn execute_with_deps_uses_module_local_native_tables_for_mixed_std_modules() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let a_path = temp.path().join("a.raya");
        let b_path = temp.path().join("b.raya");

        fs::write(
            &a_path,
            r#"
            import { sqrt } from "std:math";
            export function runA(): number { return sqrt(81); }
            "#,
        )
        .expect("write a");
        fs::write(
            &b_path,
            r#"
            import { basename } from "std:path";
            export function runB(): string { return basename("/tmp/demo.txt"); }
            "#,
        )
        .expect("write b");
        fs::write(
            &main_path,
            r#"
            import { runA } from "./a";
            import { runB } from "./b";

            if (runA() != 9) { return 1; }
            if (runB() != "demo.txt") { return 2; }
            return 42;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        let result = runtime
            .execute_with_deps(&program.entry, &program.dependencies)
            .expect("execute");
        assert_eq!(result, crate::Value::i32(42));
    }

    #[test]
    fn execute_with_deps_supports_imported_class_constructor_calls() {
        let temp = TempDir::new().expect("temp dir");
        let main_path = temp.path().join("main.raya");
        let dep_path = temp.path().join("dep.raya");

        fs::write(
            &dep_path,
            r#"
            export class Counter {
                constructor() {
                    throw new Error("boom");
                }
            }
            "#,
        )
        .expect("write dep");
        fs::write(
            &main_path,
            r#"
            import { Counter } from "./dep";
            let counter = new Counter();
            return 1;
            "#,
        )
        .expect("write main");

        let compiler = ProgramCompiler {
            builtin_mode: BuiltinMode::RayaStrict,
            semantic_profile: SemanticProfile::raya(),
            ts_options: None,
            compile_options: None,
        };
        let program = compiler
            .compile_program_file(&main_path)
            .expect("compile program");

        let runtime = crate::Runtime::new();
        let result = runtime.execute_with_deps(&program.entry, &program.dependencies);
        assert!(
            result.is_err(),
            "constructor throw should propagate through imported-class construction path"
        );
    }
}
