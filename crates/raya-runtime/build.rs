use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let builtins_bin_dir = manifest_dir
        .parent()
        .expect("runtime crate parent directory")
        .join("raya-engine")
        .join("builtins")
        .join("bin");

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

    write_embedded_artifacts(&out_dir, &builtins)
        .unwrap_or_else(|error| panic!("failed to write embedded builtins: {error}"));
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

fn write_embedded_artifacts(out_dir: &Path, builtins: &[(String, PathBuf)]) -> Result<(), String> {
    let strict_modules = render_mode_modules(builtins);
    let node_modules = render_mode_modules(builtins);
    let generated = format!(
        "pub static STRICT_EMBEDDED_BUILTIN_MODULES: &[EmbeddedBuiltinModule] = &[\n{strict_modules}];\n\n\
         pub static NODE_EMBEDDED_BUILTIN_MODULES: &[EmbeddedBuiltinModule] = &[\n{node_modules}];\n\n\
         pub static STRICT_EMBEDDED_LITERAL_GLOBALS: &[EmbeddedLiteralGlobal] = &[];\n\n\
         pub static NODE_EMBEDDED_LITERAL_GLOBALS: &[EmbeddedLiteralGlobal] = &[];\n",
        strict_modules = strict_modules,
        node_modules = node_modules,
    );
    fs::write(out_dir.join("embedded_builtins.rs"), generated)
        .map_err(|error| format!("Failed to write embedded builtins index: {error}"))?;
    Ok(())
}

fn render_mode_modules(builtins: &[(String, PathBuf)]) -> String {
    let mut rendered = String::new();
    for (name, path) in builtins {
        rendered.push_str(&format!(
            "    EmbeddedBuiltinModule {{ logical_path: {:?}, bytecode: include_bytes!({:?}) }},\n",
            name,
            path.display().to_string()
        ));
    }
    rendered
}
