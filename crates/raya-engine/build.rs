//! Build script for precompiling Raya builtin types
//!
//! This compiles all .raya files in the builtins/strict and builtins/node_compat
//! directories at build time
//! and embeds the bytecode in the resulting library.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // builtins/{strict,node_compat} are inside this crate
    let builtins_strict_dir = Path::new(&manifest_dir).join("builtins/strict");
    let builtins_node_compat_dir = Path::new(&manifest_dir).join("builtins/node_compat");

    // Rerun if any builtin source changes
    println!("cargo:rerun-if-changed={}", builtins_strict_dir.display());
    println!("cargo:rerun-if-changed={}", builtins_node_compat_dir.display());

    // For now, generate an empty BUILTINS array since we need the compiler
    // to be built first. In a future iteration, we can add a separate
    // pre-compilation step.
    let index_path = Path::new(&out_dir).join("builtins_index.rs");
    let index_content = generate_empty_index();
    fs::write(&index_path, index_content).expect("Failed to write index");

    println!("cargo:warning=Generated empty builtins index (builtins not precompiled yet)");
}

fn generate_empty_index() -> String {
    let mut code = String::new();
    code.push_str("/// Precompiled builtin bytecode (empty for now)\n");
    code.push_str("pub static BUILTINS: &[BuiltinModule] = &[\n");
    code.push_str("];\n");
    code
}
