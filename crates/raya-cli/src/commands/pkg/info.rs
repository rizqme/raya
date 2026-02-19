//! `raya pkg info` — Show metadata for a package from the registry.

pub fn execute(package: String) -> anyhow::Result<()> {
    println!("Fetching info for package: {}", package);
    println!("(Not yet implemented — requires registry API)");
    Ok(())
}
