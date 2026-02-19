//! `raya publish` — Publish package to registry.

pub fn execute(tag: String, dry_run: bool, access: String) -> anyhow::Result<()> {
    println!("Publishing package...");
    println!("  Tag: {}", tag);
    println!("  Access: {}", access);
    if dry_run { println!("  Dry run: enabled"); }
    eprintln!("(Not yet implemented)");
    Ok(())
}
