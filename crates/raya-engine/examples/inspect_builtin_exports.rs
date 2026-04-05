use raya_engine::compiler::module::BuiltinSurfaceMode;
use raya_engine::compiler::module::builtin_global_exports;

fn main() {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("strict") => BuiltinSurfaceMode::RayaStrict,
        _ => BuiltinSurfaceMode::NodeCompat,
    };
    let filter = std::env::args().nth(2);
    let exports = builtin_global_exports(mode).expect("builtin exports");

    for export in exports
        .symbols
        .values()
        .filter(|export| filter.as_deref().is_none_or(|needle| export.name.contains(needle)))
    {
        println!(
            "{} kind={:?} local={} const={} async={} sig={}",
            export.name,
            export.kind,
            export.local_name,
            export.is_const,
            export.is_async,
            export.type_signature
        );
    }
}
