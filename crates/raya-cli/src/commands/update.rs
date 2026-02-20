//! `raya update` — Update dependencies.

use raya_runtime::Runtime;

pub fn execute(package: Option<String>) -> anyhow::Result<()> {
    if let Some(pkg) = package {
        println!("Updating package: {}", pkg);
        eprintln!("(Single package update not yet implemented)");
        return Ok(());
    }

    let rt = Runtime::new();
    let cwd = std::env::current_dir()?
        .display()
        .to_string()
        .replace('\\', "/")
        .replace('"', "\\\"");
    let script = format!(
        r#"const result = pm.update("{}", false);
io.writeln("Done! " + result.installed.toString() + " installed, " + result.cached.toString() + " from cache.")"#,
        cwd
    );
    match rt.eval(&script) {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
