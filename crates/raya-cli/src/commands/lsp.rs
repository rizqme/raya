//! `raya lsp` — Start Language Server.

pub fn execute(_stdio: bool, port: Option<u16>) -> anyhow::Result<()> {
    if let Some(p) = port {
        println!("Starting LSP server on port {}...", p);
    } else {
        println!("Starting LSP server (stdio)...");
    }
    eprintln!("(Not yet implemented)");
    Ok(())
}
