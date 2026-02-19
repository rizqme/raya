//! `raya build` — Compile Raya source to .ryb bytecode.

pub fn execute(
    files: Vec<String>,
    out_dir: String,
    release: bool,
    watch: bool,
    sourcemap: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    println!("Building: {:?}", files);
    println!("Output: {}", out_dir);
    if release { println!("  Release mode: enabled"); }
    if watch { println!("  Watch mode: enabled"); }
    if sourcemap { println!("  Source maps: enabled"); }
    if dry_run { println!("  Dry run: enabled"); }
    eprintln!("(Not yet implemented)");
    Ok(())
}
