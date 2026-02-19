//! `raya doc` — Generate documentation.

pub fn execute(
    out_dir: String,
    serve: bool,
    open: bool,
    format: String,
) -> anyhow::Result<()> {
    println!("Generating documentation...");
    println!("  Output: {}", out_dir);
    println!("  Format: {}", format);
    if serve { println!("  Server: enabled"); }
    if open { println!("  Open in browser: enabled"); }
    eprintln!("(Not yet implemented)");
    Ok(())
}
