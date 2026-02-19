//! `raya test` — Run tests.

#[allow(clippy::too_many_arguments)]
pub fn execute(
    filter: Option<String>,
    watch: bool,
    coverage: bool,
    bail: bool,
    timeout: u64,
    concurrency: usize,
    reporter: String,
    file: Option<String>,
    update_snapshots: bool,
) -> anyhow::Result<()> {
    if let Some(ref f) = filter {
        println!("Running tests matching: {}", f);
    } else {
        println!("Running all tests...");
    }
    if watch { println!("  Watch mode: enabled"); }
    if coverage { println!("  Coverage: enabled"); }
    if bail { println!("  Bail on first failure: enabled"); }
    if timeout != 5000 { println!("  Timeout: {}ms", timeout); }
    if concurrency > 0 { println!("  Concurrency: {}", concurrency); }
    if reporter != "default" { println!("  Reporter: {}", reporter); }
    if let Some(ref f) = file { println!("  File filter: {}", f); }
    if update_snapshots { println!("  Update snapshots: enabled"); }
    eprintln!("(Not yet implemented)");
    Ok(())
}
