//! `raya lint` — Lint source files.

pub fn execute(
    files: Vec<String>,
    fix: bool,
    format: String,
    watch: bool,
) -> anyhow::Result<()> {
    println!("Linting: {:?}", files);
    if fix { println!("  Auto-fix: enabled"); }
    if format != "pretty" { println!("  Format: {}", format); }
    if watch { println!("  Watch mode: enabled"); }
    eprintln!("(Not yet implemented)");
    Ok(())
}
