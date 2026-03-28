use crate::error::RuntimeError;
use raya_engine::parser::ast::{ExportDecl, Statement};
use raya_engine::parser::Parser;
use raya_engine::semantics::SemanticProfile;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::loader::ModuleLoaderV2;
use super::resolver::{ImportResolution, ModuleKey, ModuleResolverV2, ModuleSpecifierKind};

#[derive(Debug, Clone)]
pub struct ProgramGraphNode {
    pub key: ModuleKey,
    pub display_name: String,
    pub source: String,
    pub imports: Vec<ImportResolution>,
    pub dependencies: Vec<ModuleKey>,
}

#[derive(Debug, Clone)]
pub struct ProgramGraph {
    pub entry: ModuleKey,
    pub nodes: HashMap<ModuleKey, ProgramGraphNode>,
    pub topological_order: Vec<ModuleKey>,
}

#[derive(Debug, Default, Clone)]
pub struct ProgramGraphBuilder {
    resolver: ModuleResolverV2,
    loader: ModuleLoaderV2,
}

impl ProgramGraphBuilder {
    pub fn new() -> Self {
        Self {
            resolver: ModuleResolverV2,
            loader: ModuleLoaderV2,
        }
    }

    pub fn build(&self, entry: &Path) -> Result<ProgramGraph, RuntimeError> {
        let entry_key = ModuleKey::File(entry.canonicalize().map_err(RuntimeError::Io)?);
        self.build_internal(entry_key, None)
    }

    pub fn build_from_source(
        &self,
        virtual_entry_path: PathBuf,
        source: String,
    ) -> Result<ProgramGraph, RuntimeError> {
        let entry_key = ModuleKey::File(virtual_entry_path);
        self.build_internal(entry_key, Some(source))
    }

    fn build_internal(
        &self,
        entry_key: ModuleKey,
        entry_source: Option<String>,
    ) -> Result<ProgramGraph, RuntimeError> {
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        let mut nodes = HashMap::new();
        let mut topological_order = Vec::new();
        let mut stack = Vec::new();

        self.visit(
            &entry_key,
            entry_source.as_ref(),
            &mut visiting,
            &mut visited,
            &mut nodes,
            &mut topological_order,
            &mut stack,
        )?;

        Ok(ProgramGraph {
            entry: entry_key,
            nodes,
            topological_order,
        })
    }

    fn visit(
        &self,
        key: &ModuleKey,
        entry_source: Option<&String>,
        visiting: &mut HashSet<ModuleKey>,
        visited: &mut HashSet<ModuleKey>,
        nodes: &mut HashMap<ModuleKey, ProgramGraphNode>,
        topological_order: &mut Vec<ModuleKey>,
        stack: &mut Vec<ModuleKey>,
    ) -> Result<(), RuntimeError> {
        if visited.contains(key) {
            return Ok(());
        }

        if visiting.contains(key) {
            let cycle_start = stack.iter().position(|k| k == key).unwrap_or(0);
            let mut cycle = stack[cycle_start..]
                .iter()
                .map(ModuleKey::display_name)
                .collect::<Vec<_>>();
            cycle.push(key.display_name());
            return Err(RuntimeError::Dependency(format!(
                "Circular module dependency detected: {}",
                cycle.join(" -> ")
            )));
        }

        visiting.insert(key.clone());
        stack.push(key.clone());

        let loaded = if entry_source.is_some() && key == stack.first().unwrap_or(key) {
            match key {
                ModuleKey::File(path) => super::loader::LoadedModuleSource {
                    key: ModuleKey::File(path.clone()),
                    display_name: path.display().to_string(),
                    source: entry_source.cloned().unwrap_or_default(),
                },
                ModuleKey::Std(name) => super::loader::LoadedModuleSource {
                    key: ModuleKey::Std(name.clone()),
                    display_name: format!("std:{}", name),
                    source: entry_source.cloned().unwrap_or_default(),
                },
            }
        } else {
            self.loader.load(key)?
        };

        let imports = self.extract_imports(&loaded.key, &loaded.source)?;
        let dependencies = imports
            .iter()
            .map(|r| match &r.kind {
                ModuleSpecifierKind::File(path) => ModuleKey::File(path.clone()),
                ModuleSpecifierKind::Std(canonical) => ModuleKey::Std(canonical.clone()),
            })
            .collect::<Vec<_>>();

        for dep in &dependencies {
            self.visit(
                dep,
                None,
                visiting,
                visited,
                nodes,
                topological_order,
                stack,
            )?;
        }

        nodes.insert(
            loaded.key.clone(),
            ProgramGraphNode {
                key: loaded.key.clone(),
                display_name: loaded.display_name,
                source: loaded.source,
                imports,
                dependencies,
            },
        );

        let popped = stack.pop();
        debug_assert_eq!(popped.as_ref(), Some(key));
        visiting.remove(key);
        visited.insert(key.clone());
        topological_order.push(key.clone());

        Ok(())
    }

    fn extract_imports(
        &self,
        importer: &ModuleKey,
        source: &str,
    ) -> Result<Vec<ImportResolution>, RuntimeError> {
        let parser = Parser::new_with_mode(
            source,
            semantic_profile_for_module_key(importer).parser_mode(),
        )
        .map_err(|errors| {
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

        let mut out = Vec::new();
        for stmt in &ast.statements {
            match stmt {
                Statement::ImportDecl(import_decl) => {
                    let specifier = interner.resolve(import_decl.source.value);
                    out.push(self.resolver.resolve(importer, specifier)?);
                }
                Statement::ExportDecl(ExportDecl::Named {
                    source: Some(src), ..
                })
                | Statement::ExportDecl(ExportDecl::All { source: src, .. }) => {
                    let specifier = interner.resolve(src.value);
                    out.push(self.resolver.resolve(importer, specifier)?);
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

fn semantic_profile_for_module_key(key: &ModuleKey) -> SemanticProfile {
    match key {
        ModuleKey::File(path) => SemanticProfile::from_path(path),
        ModuleKey::Std(_) => SemanticProfile::raya(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn graph_builds_topological_order_for_local_imports() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("src");
        fs::create_dir_all(&root).unwrap();

        let a = root.join("a.raya");
        let b = root.join("b.raya");
        let c = root.join("c.raya");

        fs::write(
            &a,
            r#"
            import { b } from "./b";
            import { c } from "./c";
            export const a: number = 1;
            "#,
        )
        .unwrap();
        fs::write(&b, r#"export const b: number = 2;"#).unwrap();
        fs::write(&c, r#"export const c: number = 3;"#).unwrap();

        let graph = ProgramGraphBuilder::new().build(&a).unwrap();
        assert_eq!(
            graph.topological_order.last(),
            Some(&ModuleKey::File(a.canonicalize().unwrap()))
        );
        assert!(graph
            .topological_order
            .contains(&ModuleKey::File(b.canonicalize().unwrap())));
        assert!(graph
            .topological_order
            .contains(&ModuleKey::File(c.canonicalize().unwrap())));
    }

    #[test]
    fn graph_cycle_detection_errors() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("src");
        fs::create_dir_all(&root).unwrap();

        let a = root.join("a.raya");
        let b = root.join("b.raya");

        fs::write(
            &a,
            r#"import { b } from "./b"; export const a: number = 1;"#,
        )
        .unwrap();
        fs::write(
            &b,
            r#"import { a } from "./a"; export const b: number = 2;"#,
        )
        .unwrap();

        let err = ProgramGraphBuilder::new().build(&a).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("Circular module dependency detected"),
            "expected cycle error, got: {msg}"
        );
    }
}
