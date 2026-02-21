//! Build script for precompiling Raya builtin types
//!
//! This compiles all .raya files in the builtins/ directory at build time
//! and embeds the bytecode in the resulting library.

use std::env;
use std::fs;
use std::path::Path;

/// Builtin source files to compile
const BUILTINS: &[&str] = &[
    // Native primitive types (//@@builtin_primitive)
    "string",
    "number",
    "Array",
    "RegExp",
    // Builtin classes (vtable dispatch)
    "Object",
    "Error",
    "Mutex",
    "Task",
    "Channel",
    "Map",
    "Set",
    "Buffer",
    "Date",
    "RegExpMatch",
];

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // builtins/ is inside this crate
    let builtins_dir = Path::new(&manifest_dir).join("builtins");

    // Rerun if any builtin source changes
    println!("cargo:rerun-if-changed={}", builtins_dir.display());
    for name in BUILTINS {
        let source_path = builtins_dir.join(format!("{}.raya", name));
        println!("cargo:rerun-if-changed={}", source_path.display());
    }

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
