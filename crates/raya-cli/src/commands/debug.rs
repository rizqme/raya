//! `raya debug` — Interactive debugger for Raya scripts.
//!
//! Runs the embedded `debugger.raya` script which spawns a child VM
//! in debug mode and provides a REPL for stepping, breakpoints, and inspection.

use anyhow::anyhow;
use raya_runtime::Runtime;
use std::path::Path;

/// Embedded debugger REPL script (written in Raya)
const DEBUGGER_SCRIPT: &str = include_str!("../../scripts/debugger.raya");

/// Embedded DAP adapter script (written in Raya)
const DAP_ADAPTER_SCRIPT: &str = include_str!("../../scripts/dap-adapter.raya");

pub fn execute(
    target: String,
    break_at_entry: bool,
    break_at: Option<String>,
    dap: bool,
) -> anyhow::Result<()> {
    // Validate target file exists
    let target_path = if Path::new(&target).is_relative() {
        std::env::current_dir()?.join(&target)
    } else {
        std::path::PathBuf::from(&target)
    };

    if !target_path.exists() {
        return Err(anyhow!("File not found: {}", target));
    }

    // Select the appropriate script based on mode
    let script = if dap { DAP_ADAPTER_SCRIPT } else { DEBUGGER_SCRIPT };

    // Inject CLI args as constants prepended to the script.
    // This avoids needing to manipulate process.argv().
    let target_abs = target_path.to_string_lossy();
    let break_at_str = break_at.unwrap_or_default();
    let preamble = format!(
        "const __DEBUG_TARGET: string = \"{}\";\n\
         const __DEBUG_BREAK_AT_ENTRY: boolean = {};\n\
         const __DEBUG_BREAK_AT: string = \"{}\";\n",
        escape_raya_string(&target_abs),
        break_at_entry,
        escape_raya_string(&break_at_str),
    );

    let source = format!("{}{}", preamble, script);

    let rt = Runtime::new();
    match rt.eval(&source) {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

/// Escape a string for embedding in Raya source code.
fn escape_raya_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
