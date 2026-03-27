//! Bytecode loading and library resolution.

use raya_engine::compiler::Module;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

use crate::error::RuntimeError;
use crate::CompiledModule;
use crate::{compile, BuiltinMode, TypeMode};
use raya_engine::semantics::SemanticProfile;

/// Load a .ryb bytecode file from disk.
pub fn load_bytecode_file(path: &Path) -> Result<CompiledModule, RuntimeError> {
    let bytes = std::fs::read(path)?;
    load_bytecode_bytes(&bytes)
}

/// Load bytecode from raw bytes.
pub fn load_bytecode_bytes(bytes: &[u8]) -> Result<CompiledModule, RuntimeError> {
    let module = Module::decode(bytes).map_err(|e| RuntimeError::Bytecode(format!("{}", e)))?;
    Ok(CompiledModule {
        module,
        interner: None,
    })
}

/// Resolve import dependencies for a .ryb module by searching nearby directories.
///
/// Search order for each import:
/// 1. Same directory as the .ryb file
/// 2. `.raya/packages/` relative to the .ryb file
/// 3. `raya_packages/` (legacy) relative to the .ryb file
/// 4. `~/.raya/packages/` global directory
pub fn resolve_ryb_deps(
    module: &CompiledModule,
    ryb_dir: &Path,
) -> Result<Vec<CompiledModule>, RuntimeError> {
    let mut deps = Vec::new();

    for import in &module.module.imports {
        let specifier = &import.module_specifier;

        // Skip std: imports — they're compiled into the source
        if specifier.starts_with("std:") {
            continue;
        }

        let dep = find_library(specifier, ryb_dir)?;
        deps.push(dep);
    }

    Ok(deps)
}

/// Search for a library module by name in standard locations.
fn find_library(name: &str, base_dir: &Path) -> Result<CompiledModule, RuntimeError> {
    // 1. Same directory — as .ryb
    let local_ryb = base_dir.join(format!("{}.ryb", name));
    if local_ryb.exists() {
        return load_bytecode_file(&local_ryb);
    }

    // 2. Same directory — as .raya source
    let local_raya = base_dir.join(format!("{}.raya", name));
    if local_raya.exists() {
        let source = std::fs::read_to_string(&local_raya)?;
        let (module, interner) = crate::compile::compile_source(&source)?;
        return Ok(CompiledModule {
            module,
            interner: Some(interner),
        });
    }

    // 3. .raya/packages/{name}/ — look for entry point
    let local_raya_pkg_dir = base_dir.join(".raya").join("packages").join(name);
    if local_raya_pkg_dir.exists() {
        return load_package_dir_with_mode(&local_raya_pkg_dir, name, None);
    }

    // 4. raya_packages/{name}/ — legacy fallback
    let pkg_dir = base_dir.join("raya_packages").join(name);
    if pkg_dir.exists() {
        return load_package_dir_with_mode(&pkg_dir, name, None);
    }

    // 5. ~/.raya/packages/{name}/ — global
    if let Some(home) = dirs::home_dir() {
        let global_pkg = home.join(".raya").join("packages").join(name);
        if global_pkg.exists() {
            return load_package_dir_with_mode(&global_pkg, name, None);
        }
    }

    Err(RuntimeError::Dependency(format!(
        "Cannot find module '{}'. Searched:\n  {}\n  {}\n  {}/.raya/packages/{}/\n  {}/raya_packages/{}/\n  ~/.raya/packages/{}/",
        name,
        local_ryb.display(),
        local_raya.display(),
        base_dir.display(),
        name,
        base_dir.display(),
        name,
        name,
    )))
}

/// Load a package from its directory, finding the entry point.
///
/// Public alias for use by the dependency resolver.
pub fn load_package_dir_pub(dir: &Path, name: &str) -> Result<CompiledModule, RuntimeError> {
    load_package_dir_with_mode(dir, name, None)
}

/// Load a package using an explicit dependency type mode when available.
pub fn load_package_dir_with_mode_pub(
    dir: &Path,
    name: &str,
    forced_mode: Option<TypeMode>,
) -> Result<CompiledModule, RuntimeError> {
    load_package_dir_with_mode(dir, name, forced_mode)
}

/// Load an entry point file, dispatching by extension.
///
/// Public alias for use by the dependency resolver.
pub fn load_entry_point_pub(path: &Path) -> Result<CompiledModule, RuntimeError> {
    load_entry_point_with_mode(path, None)
}

/// Load an entry point with an explicit type mode override.
pub fn load_entry_point_with_mode_pub(
    path: &Path,
    forced_mode: Option<TypeMode>,
) -> Result<CompiledModule, RuntimeError> {
    load_entry_point_with_mode(path, forced_mode)
}

/// Load a package from its directory, finding the entry point.
fn load_package_dir_with_mode(
    dir: &Path,
    name: &str,
    forced_mode: Option<TypeMode>,
) -> Result<CompiledModule, RuntimeError> {
    // Try package.json → raya.entry/main first
    let package_json_path = dir.join("package.json");
    if package_json_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&package_json_path) {
            if let Ok(manifest) = serde_json::from_str::<JsonValue>(&content) {
                let entry = manifest
                    .get("raya")
                    .and_then(|v| v.get("entry"))
                    .and_then(|v| v.as_str())
                    .or_else(|| manifest.get("main").and_then(|v| v.as_str()));
                if let Some(main) = entry {
                    let entry = dir.join(main);
                    if entry.exists() {
                        return load_entry_point_with_mode(&entry, forced_mode);
                    }
                }
            }
        }
    }

    // Try raya.toml → [package].main
    let manifest_path = dir.join("raya.toml");
    if manifest_path.exists() {
        if let Ok(manifest) = raya_pm::PackageManifest::from_file(&manifest_path) {
            if let Some(main) = &manifest.package.main {
                let entry = dir.join(main);
                if entry.exists() {
                    return load_entry_point_with_mode(&entry, forced_mode);
                }
            }
        }
    }

    // Fallback: src/lib.raya, src/main.raya, lib.raya, main.raya, {name}.ryb
    let candidates = [
        dir.join("src/lib.raya"),
        dir.join("src/main.raya"),
        dir.join("lib.raya"),
        dir.join("main.raya"),
        dir.join(format!("{}.ryb", name)),
        dir.join("lib.ryb"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return load_entry_point_with_mode(candidate, forced_mode);
        }
    }

    Err(RuntimeError::Dependency(format!(
        "Package '{}' at {} has no entry point. \
         Add [package].main to raya.toml or create src/lib.raya.",
        name,
        dir.display(),
    )))
}

/// Load an entry point file, dispatching by extension.
fn builtin_mode_for_type_mode(type_mode: TypeMode) -> BuiltinMode {
    match type_mode {
        TypeMode::Raya => BuiltinMode::RayaStrict,
        TypeMode::Ts | TypeMode::Js => BuiltinMode::NodeCompat,
    }
}

fn load_entry_point_with_mode(
    path: &Path,
    forced_mode: Option<TypeMode>,
) -> Result<CompiledModule, RuntimeError> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ryb") => load_bytecode_file(path),
        Some("raya") => {
            let source = std::fs::read_to_string(path)?;
            let (inferred_profile, ts_options) = infer_semantic_profile_for_path(path)?;
            let profile = forced_mode
                .map(TypeMode::semantic_profile)
                .unwrap_or(inferred_profile);
            let (module, interner) = compile::compile_source_with_profile_and_ts_options(
                &source,
                builtin_mode_for_type_mode(match profile.source_kind {
                    raya_engine::semantics::SourceKind::Raya => TypeMode::Raya,
                    raya_engine::semantics::SourceKind::Ts => TypeMode::Ts,
                    raya_engine::semantics::SourceKind::Js => TypeMode::Js,
                }),
                profile,
                ts_options.as_ref(),
            )?;
            Ok(CompiledModule {
                module,
                interner: Some(interner),
            })
        }
        _ => Err(RuntimeError::Dependency(format!(
            "Unsupported file type: {}",
            path.display(),
        ))),
    }
}

fn infer_type_mode_for_path(
    path: &Path,
) -> Result<(TypeMode, Option<compile::TsCompilerOptions>), RuntimeError> {
    let (profile, ts_options) = infer_semantic_profile_for_path(path)?;
    let type_mode = match profile.source_kind {
        raya_engine::semantics::SourceKind::Js => TypeMode::Js,
        raya_engine::semantics::SourceKind::Ts => TypeMode::Ts,
        raya_engine::semantics::SourceKind::Raya => TypeMode::Raya,
    };
    Ok((type_mode, ts_options))
}

fn infer_semantic_profile_for_path(
    path: &Path,
) -> Result<(SemanticProfile, Option<compile::TsCompilerOptions>), RuntimeError> {
    let mut dir = match path.parent() {
        Some(p) => p.to_path_buf(),
        None => return Ok((SemanticProfile::raya(), None)),
    };
    loop {
        if let Some(tsconfig_path) = find_tsconfig(&dir) {
            let ts_options = load_ts_compiler_options(&tsconfig_path)?;
            if ts_options.allow_js.unwrap_or(false) {
                return Ok((SemanticProfile::js(), Some(ts_options)));
            }
            return Ok((SemanticProfile::ts_strict(), Some(ts_options)));
        }
        if !dir.pop() {
            return Ok((SemanticProfile::raya(), None));
        }
    }
}

pub fn find_tsconfig(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
    loop {
        let tsconfig_path = dir.join("tsconfig.json");
        if tsconfig_path.exists() {
            return Some(tsconfig_path);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub fn load_ts_compiler_options(path: &Path) -> Result<compile::TsCompilerOptions, RuntimeError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        RuntimeError::TypeCheck(format!(
            "Failed to read tsconfig '{}': {}",
            path.display(),
            e
        ))
    })?;
    let value: JsonValue = serde_json::from_str(&content).map_err(|e| {
        RuntimeError::TypeCheck(format!(
            "Failed to parse tsconfig '{}': {}",
            path.display(),
            e
        ))
    })?;
    let compiler = value
        .get("compilerOptions")
        .cloned()
        .unwrap_or(JsonValue::Null);
    serde_json::from_value::<compile::TsCompilerOptions>(compiler).map_err(|e| {
        RuntimeError::TypeCheck(format!(
            "Failed to parse compilerOptions in '{}': {}",
            path.display(),
            e
        ))
    })
}
