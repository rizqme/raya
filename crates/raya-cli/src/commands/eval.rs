//! `raya eval` — Evaluate an inline expression.

pub fn execute(
    code: String,
    _print: bool,
    _no_print: bool,
    _no_jit: bool,
) -> anyhow::Result<()> {
    println!("Evaluating: {}", code);
    eprintln!("(Not yet implemented)");
    Ok(())
}
