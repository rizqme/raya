//! Bytecode loading and library resolution.

use raya_engine::compiler::Module;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

use crate::error::RuntimeError;
use crate::CompiledModule;
use crate::{compile, BuiltinMode};
use raya_engine::semantics::{SemanticProfile, SourceKind};

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
        return load_package_dir_with_profile(&local_raya_pkg_dir, name, None);
    }

    // 4. raya_packages/{name}/ — legacy fallback
    let pkg_dir = base_dir.join("raya_packages").join(name);
    if pkg_dir.exists() {
        return load_package_dir_with_profile(&pkg_dir, name, None);
    }

    // 5. ~/.raya/packages/{name}/ — global
    if let Some(home) = dirs::home_dir() {
        let global_pkg = home.join(".raya").join("packages").join(name);
        if global_pkg.exists() {
            return load_package_dir_with_profile(&global_pkg, name, None);
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
    load_package_dir_with_profile(dir, name, None)
}

/// Load a package using an explicit semantic profile when available.
pub fn load_package_dir_with_profile_pub(
    dir: &Path,
    name: &str,
    forced_profile: Option<SemanticProfile>,
) -> Result<CompiledModule, RuntimeError> {
    load_package_dir_with_profile(dir, name, forced_profile)
}

/// Load an entry point file, dispatching by extension.
///
/// Public alias for use by the dependency resolver.
pub fn load_entry_point_pub(path: &Path) -> Result<CompiledModule, RuntimeError> {
    load_entry_point_with_profile(path, None)
}

/// Load an entry point with an explicit semantic profile override.
pub fn load_entry_point_with_profile_pub(
    path: &Path,
    forced_profile: Option<SemanticProfile>,
) -> Result<CompiledModule, RuntimeError> {
    load_entry_point_with_profile(path, forced_profile)
}

/// Load a package from its directory, finding the entry point.
fn load_package_dir_with_profile(
    dir: &Path,
    name: &str,
    forced_profile: Option<SemanticProfile>,
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
                        return load_entry_point_with_profile(&entry, forced_profile);
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
                    return load_entry_point_with_profile(&entry, forced_profile);
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
            return load_entry_point_with_profile(candidate, forced_profile);
        }
    }

    Err(RuntimeError::Dependency(format!(
        "Package '{}' at {} has no entry point. \
         Add [package].main to raya.toml or create src/lib.raya.",
        name,
        dir.display(),
    )))
}

fn builtin_mode_for_profile(profile: SemanticProfile) -> BuiltinMode {
    match profile.source_kind {
        SourceKind::Raya => BuiltinMode::RayaStrict,
        SourceKind::Ts | SourceKind::Js => BuiltinMode::NodeCompat,
    }
}

fn load_entry_point_with_profile(
    path: &Path,
    forced_profile: Option<SemanticProfile>,
) -> Result<CompiledModule, RuntimeError> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ryb") => load_bytecode_file(path),
        Some("raya" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx") => {
            let source = std::fs::read_to_string(path)?;
            let (inferred_profile, ts_options) = infer_semantic_profile_for_path(path)?;
            let profile = forced_profile.unwrap_or(inferred_profile);
            let (module, interner) = compile::compile_source_with_profile_and_ts_options(
                &source,
                builtin_mode_for_profile(profile),
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

fn infer_semantic_profile_for_path(
    path: &Path,
) -> Result<(SemanticProfile, Option<compile::TsCompilerOptions>), RuntimeError> {
    let base_profile = SemanticProfile::from_path(path);
    let mut dir = match path.parent() {
        Some(p) => p.to_path_buf(),
        None => return Ok((base_profile, None)),
    };

    if matches!(base_profile.source_kind, SourceKind::Raya) {
        return Ok((base_profile, None));
    }

    loop {
        if let Some(tsconfig_path) = find_tsconfig(&dir) {
            let ts_options = load_ts_compiler_options(&tsconfig_path)?;
            match base_profile.source_kind {
                SourceKind::Js => {
                    if ts_options.allow_js.unwrap_or(false) {
                        return Ok((SemanticProfile::js(), Some(ts_options)));
                    }
                    return Ok((SemanticProfile::js(), None));
                }
                SourceKind::Ts => return Ok((SemanticProfile::ts_strict(), Some(ts_options))),
                SourceKind::Raya => return Ok((base_profile, None)),
            }
        }
        if !dir.pop() {
            return Ok((base_profile, None));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn infer_semantic_profile_uses_source_extension_without_tsconfig() {
        let temp_dir = TempDir::new().expect("temp dir");
        let js_path = temp_dir.path().join("entry.js");
        let ts_path = temp_dir.path().join("entry.ts");
        let raya_path = temp_dir.path().join("entry.raya");

        let (js_profile, js_options) =
            infer_semantic_profile_for_path(&js_path).expect("js profile inference");
        let (ts_profile, ts_options) =
            infer_semantic_profile_for_path(&ts_path).expect("ts profile inference");
        let (raya_profile, raya_options) =
            infer_semantic_profile_for_path(&raya_path).expect("raya profile inference");

        assert_eq!(js_profile, SemanticProfile::js());
        assert!(js_options.is_none());
        assert_eq!(ts_profile, SemanticProfile::ts_strict());
        assert!(ts_options.is_none());
        assert_eq!(raya_profile, SemanticProfile::raya());
        assert!(raya_options.is_none());
    }

    #[test]
    fn infer_semantic_profile_layers_tsconfig_only_where_applicable() {
        let temp_dir = TempDir::new().expect("temp dir");
        let tsconfig_path = temp_dir.path().join("tsconfig.json");
        fs::write(
            &tsconfig_path,
            r#"{"compilerOptions":{"strict":true,"allowJs":true}}"#,
        )
        .expect("write tsconfig");

        let js_path = temp_dir.path().join("entry.js");
        let ts_path = temp_dir.path().join("entry.ts");
        let raya_path = temp_dir.path().join("entry.raya");

        let (js_profile, js_options) =
            infer_semantic_profile_for_path(&js_path).expect("js profile inference");
        let (ts_profile, ts_options) =
            infer_semantic_profile_for_path(&ts_path).expect("ts profile inference");
        let (raya_profile, raya_options) =
            infer_semantic_profile_for_path(&raya_path).expect("raya profile inference");

        assert_eq!(js_profile, SemanticProfile::js());
        assert_eq!(js_options.expect("js ts options").allow_js, Some(true));
        assert_eq!(ts_profile, SemanticProfile::ts_strict());
        assert_eq!(ts_options.expect("ts options").strict, Some(true));
        assert_eq!(raya_profile, SemanticProfile::raya());
        assert!(raya_options.is_none());
    }
}
