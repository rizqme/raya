use raya_runtime::{BuiltinMode, Runtime};

fn main() {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("strict") => BuiltinMode::RayaStrict,
        _ => BuiltinMode::NodeCompat,
    };

    let modules = Runtime::debug_compiled_builtin_modules(mode).expect("compiled builtins");
    for module in modules {
        println!("MODULE {}", module.metadata.name);
        for export in module.exports {
            println!(
                "  {} kind={:?} index={} runtime_slot={:?}",
                export.name, export.symbol_type, export.index, export.runtime_global_slot
            );
        }
    }
}
