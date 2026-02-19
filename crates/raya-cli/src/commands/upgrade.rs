//! `raya upgrade` — Upgrade Raya installation.

pub fn execute(
    version: Option<String>,
    check: bool,
    _force: bool,
) -> anyhow::Result<()> {
    if check {
        println!("Checking for updates...");
        eprintln!("(Not yet implemented)");
        return Ok(());
    }

    if let Some(v) = version {
        println!("Upgrading to version: {}", v);
    } else {
        println!("Upgrading to latest version...");
    }
    eprintln!("(Not yet implemented)");
    Ok(())
}
