//! `raya bench` — Run benchmarks.

pub fn execute(
    filter: Option<String>,
    warmup: usize,
    iterations: usize,
    save: Option<String>,
    compare: Option<String>,
    json: bool,
) -> anyhow::Result<()> {
    if let Some(ref f) = filter {
        println!("Running benchmarks matching: {}", f);
    } else {
        println!("Running all benchmarks...");
    }
    let _ = (warmup, iterations, save, compare, json);
    eprintln!("(Not yet implemented)");
    Ok(())
}
