//! `raya bundle` — Create standalone executable.

pub fn execute(
    file: String,
    output: String,
    target: String,
    release: bool,
    strip: bool,
    compress: bool,
    no_runtime: bool,
) -> anyhow::Result<()> {
    println!("Bundling: {}", file);
    println!("  Output: {}", output);
    println!("  Target: {}", target);
    if release { println!("  Release mode: enabled"); }
    if strip { println!("  Strip symbols: enabled"); }
    if compress { println!("  Compression: enabled"); }
    if no_runtime {
        println!("  Embedded runtime: disabled");
    } else {
        println!("  Embedded runtime: enabled");
    }
    eprintln!("(Not yet implemented)");
    Ok(())
}
