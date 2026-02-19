//! `raya fmt` — Format source files.

pub fn execute(
    files: Vec<String>,
    check: bool,
    diff: bool,
    stdin: bool,
) -> anyhow::Result<()> {
    if stdin {
        println!("Formatting from stdin...");
    } else {
        println!("Formatting: {:?}", files);
    }
    if check { println!("  Check mode: enabled"); }
    if diff { println!("  Diff mode: enabled"); }
    eprintln!("(Not yet implemented)");
    Ok(())
}
