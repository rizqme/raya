//! `raya init` — Initialize a new Raya project.

use raya_runtime::Runtime;
use std::path::PathBuf;

pub fn execute(
    path: PathBuf,
    name: Option<String>,
    _template: String,
    _yes: bool,
) -> anyhow::Result<()> {
    let rt = Runtime::new();
    let dir_str = path.display().to_string().replace('\\', "/").replace('"', "\\\"");
    let name_arg = match name {
        Some(n) => format!("\"{}\"", n.replace('"', "\\\"")),
        None => "null".to_string(),
    };
    let script = format!(
        r#"pm.init("{}", {})"#,
        dir_str, name_arg
    );
    match rt.eval(&script) {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
