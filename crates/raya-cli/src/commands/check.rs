//! `raya check` — Type-check without building.

pub fn execute(
    files: Vec<String>,
    watch: bool,
    strict: bool,
    format: String,
) -> anyhow::Result<()> {
    println!("Type-checking: {:?}", files);
    if watch { println!("  Watch mode: enabled"); }
    if strict { println!("  Strict mode: enabled"); }
    if format != "pretty" { println!("  Format: {}", format); }
    eprintln!("(Not yet implemented)");
    Ok(())
}
