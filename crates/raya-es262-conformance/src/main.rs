use anyhow::Result;

fn main() -> Result<()> {
    std::process::exit(raya_es262_conformance::main_entry()?)
}
