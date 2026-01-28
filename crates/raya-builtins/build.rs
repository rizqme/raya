//! Build script for precompiling Raya builtin types
//!
//! This compiles all .raya files in the builtins/ directory at build time
//! and embeds the bytecode in the resulting library.

use std::env;
use std::fs;
use std::path::Path;

use raya_compiler::{Compiler, CompileError};
use raya_parser::{Parser, TypeContext};

/// Builtin source files to compile
const BUILTINS: &[&str] = &[
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

    // Compile each builtin and write bytecode
    let mut compiled_count = 0;
    let mut failed = Vec::new();

    for name in BUILTINS {
        let source_path = builtins_dir.join(format!("{}.raya", name));
        let output_path = Path::new(&out_dir).join(format!("{}.rbc", name));

        match compile_builtin(&source_path, &output_path) {
            Ok(()) => {
                compiled_count += 1;
                println!("cargo:warning=Compiled builtin: {}", name);
            }
            Err(e) => {
                failed.push((name.to_string(), e));
            }
        }
    }

    // Generate the index file listing all compiled builtins
    let index_path = Path::new(&out_dir).join("builtins_index.rs");
    let index_content = generate_index(BUILTINS, &failed);
    fs::write(&index_path, index_content).expect("Failed to write index");

    if !failed.is_empty() {
        for (name, error) in &failed {
            println!("cargo:warning=Failed to compile builtin '{}': {}", name, error);
        }
    }

    println!("cargo:warning=Compiled {}/{} builtins", compiled_count, BUILTINS.len());
}

fn compile_builtin(source_path: &Path, output_path: &Path) -> Result<(), String> {
    // Read source file
    let source = fs::read_to_string(source_path)
        .map_err(|e| format!("Failed to read {}: {}", source_path.display(), e))?;

    // Parse
    let parser = Parser::new(&source)
        .map_err(|e| format!("Lex error: {:?}", e))?;

    let (module, interner) = parser.parse()
        .map_err(|e| format!("Parse error: {:?}", e))?;

    // Compile via IR
    let type_ctx = TypeContext::new();
    let compiler = Compiler::new(type_ctx, &interner);

    let bytecode_module = compiler.compile_via_ir(&module)
        .map_err(|e| format!("Compile error: {}", e))?;

    // Encode to bytes
    let bytes = bytecode_module.encode();

    // Write to output file
    fs::write(output_path, &bytes)
        .map_err(|e| format!("Failed to write {}: {}", output_path.display(), e))?;

    Ok(())
}

fn generate_index(builtins: &[&str], failed: &[(String, String)]) -> String {
    let mut code = String::new();

    code.push_str("/// Precompiled builtin bytecode\n");
    code.push_str("pub static BUILTINS: &[BuiltinModule] = &[\n");

    for name in builtins {
        let is_failed = failed.iter().any(|(n, _)| n == *name);
        if !is_failed {
            code.push_str(&format!(
                "    BuiltinModule {{ name: \"{}\", bytecode: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{}.rbc\")) }},\n",
                name, name
            ));
        }
    }

    code.push_str("];\n");

    code
}
