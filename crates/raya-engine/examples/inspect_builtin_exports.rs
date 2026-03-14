use raya_engine::compiler::disassemble_function;
use raya_engine::compiler::module::BuiltinSurfaceMode;
use raya_engine::compiler::module::ModuleCompiler;
use raya_engine::parser::checker::TypeSystemMode;
use std::path::PathBuf;

fn main() {
    let target_arg = std::env::args().nth(1);
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf();
    let builtins_root = repo_root.join("crates/raya-engine/builtins");
    let target =
        builtins_root.join(target_arg.unwrap_or_else(|| "node_compat/globals.raya".to_string()));

    let mut compiler = ModuleCompiler::new(builtins_root)
        .with_checker_mode(TypeSystemMode::Js)
        .with_builtin_surface_mode(BuiltinSurfaceMode::NodeCompat);
    let compiled = compiler.compile(&target).expect("compile builtin");
    let module = compiled
        .into_iter()
        .find(|module| module.path == target.canonicalize().expect("canonical target"))
        .expect("compiled globals module");

    for export in module.bytecode.exports {
        println!(
            "{} kind={:?} index={} runtime_slot={:?}",
            export.name, export.symbol_type, export.index, export.runtime_global_slot
        );
    }

    if let Some(main) = module.bytecode.functions.iter().find(|f| f.name == "main") {
        println!("MAIN\n{}", disassemble_function(main));
    }
}
