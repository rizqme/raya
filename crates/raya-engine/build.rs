//! Build script for embedding precompiled Raya builtin bytecode.
//! Touch this file when builtin bytecode wire formats change so Cargo reruns
//! the embedded builtin compilation step.
//!
//! The engine crate cannot self-host builtin compilation during its own build,
//! so it must consume real precompiled `.ryb` artifacts. Treating missing
//! artifacts as "empty builtins" only hides bootstrap regressions.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let builtins_bin_dir = Path::new(&manifest_dir).join("builtins/bin");

    println!("cargo:rerun-if-changed={}", builtins_bin_dir.display());
    println!("cargo:rerun-if-changed=build.rs");

    let builtins = collect_precompiled_builtins(&builtins_bin_dir)
        .unwrap_or_else(|error| panic!("failed to locate precompiled builtins: {error}"));
    if builtins.is_empty() {
        panic!(
            "no precompiled builtins found in '{}'; expected .ryb artifacts",
            builtins_bin_dir.display()
        );
    }

    let index_path = Path::new(&out_dir).join("builtins_index.rs");
    let index_content = generate_index(&builtins);
    fs::write(&index_path, index_content).expect("Failed to write index");
}

fn collect_precompiled_builtins(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut builtins = Vec::new();
    let entries = fs::read_dir(dir)
        .map_err(|error| format!("Failed to read builtin artifact directory: {error}"))?;

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Failed to read builtin artifact entry: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("ryb") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        builtins.push((stem.to_string(), path));
    }

    builtins.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(builtins)
}

fn generate_index(builtins: &[(String, PathBuf)]) -> String {
    let mut code = String::new();
    code.push_str("/// Precompiled builtin bytecode.\n");
    code.push_str("pub static BUILTINS: &[BuiltinModule] = &[\n");
    for (name, path) in builtins {
        code.push_str(&format!(
            "    BuiltinModule {{ name: {:?}, bytecode: include_bytes!({:?}) }},\n",
            name,
            path.display().to_string()
        ));
    }
    code.push_str("];\n");
    code
}
