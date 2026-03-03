use crate::compile;
use crate::compile::{TsCompilerOptions, TypeMode};
use crate::error::RuntimeError;
use crate::BuiltinMode;
use std::path::{Path, PathBuf};

use super::graph::ProgramGraphBuilder;
use super::linker::ProgramLinkerV2;

pub struct CompiledProgram {
    pub entry_path: PathBuf,
    pub module_order: Vec<PathBuf>,
    pub merged_source: String,
    pub entry: crate::CompiledModule,
    pub dependencies: Vec<crate::CompiledModule>,
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
    pub type_mode: TypeMode,
    pub ts_options: Option<TsCompilerOptions>,
    pub compile_options: Option<compile::CompileOptions>,
}

impl ProgramCompiler {
    pub fn compile_program_file(&self, path: &Path) -> Result<CompiledProgram, RuntimeError> {
        let graph = ProgramGraphBuilder::new().build(path)?;
        let linked = ProgramLinkerV2::link(&graph, self.builtin_mode)?;

        self.enforce_dynamic_import_policy(&linked.source)?;
        if let Ok(path) = std::env::var("RAYA_DEBUG_DUMP_LINKED_SOURCE") {
            let _ = std::fs::write(path, &linked.source);
        }

        let (module, interner) = if let Some(options) = &self.compile_options {
            compile::compile_graph_source_with_options_and_modes_and_ts_options(
                &linked.source,
                options,
                self.builtin_mode,
                self.type_mode,
                self.ts_options.as_ref(),
            )?
        } else {
            compile::compile_graph_source_with_modes_and_ts_options(
                &linked.source,
                self.builtin_mode,
                self.type_mode,
                self.ts_options.as_ref(),
            )?
        };

        let entry_path = match &graph.entry {
            super::resolver::ModuleKey::File(path) => path.clone(),
            super::resolver::ModuleKey::Std(name) => {
                return Err(RuntimeError::Dependency(format!(
                    "Entry module cannot be std module '{}'",
                    name
                )))
            }
        };

        Ok(CompiledProgram {
            entry_path,
            module_order: linked.module_order,
            merged_source: linked.source,
            entry: crate::CompiledModule {
                module,
                interner: Some(interner),
            },
            dependencies: Vec::new(),
        })
    }

    pub fn check_program_file(&self, path: &Path) -> Result<ProgramDiagnostics, RuntimeError> {
        let graph = ProgramGraphBuilder::new().build(path)?;
        let linked = ProgramLinkerV2::link(&graph, self.builtin_mode)?;

        self.enforce_dynamic_import_policy(&linked.source)?;
        if let Ok(path) = std::env::var("RAYA_DEBUG_DUMP_LINKED_SOURCE") {
            let _ = std::fs::write(path, &linked.source);
        }

        let diagnostics = compile::check_graph_source_with_modes_and_ts_options(
            &linked.source,
            self.builtin_mode,
            self.type_mode,
            self.ts_options.as_ref(),
        )?;

        let entry_path = match &graph.entry {
            super::resolver::ModuleKey::File(path) => path.clone(),
            super::resolver::ModuleKey::Std(name) => {
                return Err(RuntimeError::Dependency(format!(
                    "Entry module cannot be std module '{}'",
                    name
                )))
            }
        };

        Ok(ProgramDiagnostics {
            entry_path,
            module_order: linked.module_order,
            merged_source: linked.source,
            diagnostics,
        })
    }

    pub fn compile_program_source(
        &self,
        source: &str,
        virtual_entry_path: &Path,
    ) -> Result<CompiledProgram, RuntimeError> {
        let graph = ProgramGraphBuilder::new()
            .build_from_source(virtual_entry_path.to_path_buf(), source.to_string())?;
        let linked = ProgramLinkerV2::link(&graph, self.builtin_mode)?;

        self.enforce_dynamic_import_policy(&linked.source)?;
        if let Ok(path) = std::env::var("RAYA_DEBUG_DUMP_LINKED_SOURCE") {
            let _ = std::fs::write(path, &linked.source);
        }

        let (module, interner) = if let Some(options) = &self.compile_options {
            compile::compile_graph_source_with_options_and_modes_and_ts_options(
                &linked.source,
                options,
                self.builtin_mode,
                self.type_mode,
                self.ts_options.as_ref(),
            )?
        } else {
            compile::compile_graph_source_with_modes_and_ts_options(
                &linked.source,
                self.builtin_mode,
                self.type_mode,
                self.ts_options.as_ref(),
            )?
        };

        Ok(CompiledProgram {
            entry_path: virtual_entry_path.to_path_buf(),
            module_order: linked.module_order,
            merged_source: linked.source,
            entry: crate::CompiledModule {
                module,
                interner: Some(interner),
            },
            dependencies: Vec::new(),
        })
    }

    pub fn check_program_source(
        &self,
        source: &str,
        virtual_entry_path: &Path,
    ) -> Result<ProgramDiagnostics, RuntimeError> {
        let graph = ProgramGraphBuilder::new()
            .build_from_source(virtual_entry_path.to_path_buf(), source.to_string())?;
        let linked = ProgramLinkerV2::link(&graph, self.builtin_mode)?;

        self.enforce_dynamic_import_policy(&linked.source)?;
        if let Ok(path) = std::env::var("RAYA_DEBUG_DUMP_LINKED_SOURCE") {
            let _ = std::fs::write(path, &linked.source);
        }

        let diagnostics = compile::check_graph_source_with_modes_and_ts_options(
            &linked.source,
            self.builtin_mode,
            self.type_mode,
            self.ts_options.as_ref(),
        )?;

        Ok(ProgramDiagnostics {
            entry_path: virtual_entry_path.to_path_buf(),
            module_order: linked.module_order,
            merged_source: linked.source,
            diagnostics,
        })
    }

    fn enforce_dynamic_import_policy(&self, source: &str) -> Result<(), RuntimeError> {
        if matches!(self.type_mode, TypeMode::Raya) && looks_like_dynamic_import(source) {
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
            type_mode: TypeMode::Raya,
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
            type_mode: TypeMode::Raya,
            ts_options: None,
            compile_options: None,
        };

        let result = compiler.compile_program_source(
            r#"
            import stream from "std:stream";
            return stream != null;
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
    fn linker_preserves_generic_alias_arity_for_class_method_references() {
        let temp = TempDir::new().expect("temp dir");
        let lib_path = temp.path().join("lib.raya");
        let main_path = temp.path().join("main.raya");

        fs::write(
            &lib_path,
            r#"
            export class C<T> {
                value: T;
                constructor(value: T) {
                    this.value = value;
                }
            }

            export class S<T> {
                pipe(c: C<T>): string {
                    return "ok";
                }
            }
            "#,
        )
        .expect("write lib");

        fs::write(
            &main_path,
            r#"
            import { C, S } from "./lib.raya";
            return 1;
            "#,
        )
        .expect("write main");

        let graph = super::ProgramGraphBuilder::new()
            .build(&main_path)
            .expect("build graph");
        let linked = super::ProgramLinkerV2::link(&graph, BuiltinMode::RayaStrict).expect("link");

        assert!(
            linked.source.contains("// __raya_builtin_prelude_begin"),
            "linked source should include explicit builtin prelude begin marker"
        );
        assert!(
            linked.source.contains("// __raya_builtin_prelude_end"),
            "linked source should include explicit builtin prelude end marker"
        );
        assert!(
            linked.source.contains("type __t_m0_C<T> ="),
            "generic class alias should preserve arity: {}",
            linked.source
        );
        assert!(
            linked.source.contains("pipe: (c: __t_m0_C<T>) => string"),
            "method reference should keep generic argument in alias expansion: {}",
            linked.source
        );
    }
}
