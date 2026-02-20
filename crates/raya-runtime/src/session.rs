//! Persistent evaluation session for REPL and incremental execution.
//!
//! Each eval accumulates declarations (let, const, function, class, import)
//! and re-compiles them as a prefix for subsequent evals. This ensures
//! variables, functions, and types persist across inputs.
//!
//! # Example
//!
//! ```rust,ignore
//! use raya_runtime::{Session, RuntimeOptions};
//!
//! let mut session = Session::new(&RuntimeOptions::default());
//! session.eval("let x = 42").unwrap();
//! let value = session.eval("return x * 2;").unwrap();
//! assert_eq!(value.as_i32(), Some(84));
//! ```

use raya_engine::vm::{RayaString, Vm, Value};

use crate::{compile, vm_setup, RuntimeOptions};
use crate::error::RuntimeError;

/// A persistent evaluation session that maintains state across evals.
///
/// Declarations (let, const, function, class, import) are accumulated and
/// replayed as a prefix for each subsequent eval, so that variables and
/// definitions persist across inputs.
pub struct Session {
    options: RuntimeOptions,
    /// Accumulated declarations from prior evals.
    declarations: Vec<String>,
    /// VM from the most recent eval (kept alive for heap pointer reads).
    last_vm: Option<Vm>,
}

impl Session {
    /// Create a new session with the given options.
    pub fn new(options: &RuntimeOptions) -> Self {
        Self {
            options: options.clone(),
            declarations: Vec::new(),
            last_vm: None,
        }
    }

    /// Evaluate code in this session. Declarations persist across calls.
    ///
    /// Prior declarations are prepended to the code and the whole thing is
    /// compiled and executed as a single module on a fresh VM.
    pub fn eval(&mut self, code: &str) -> Result<Value, RuntimeError> {
        // Build full source: accumulated declarations + new code
        let full_source = if self.declarations.is_empty() {
            code.to_string()
        } else {
            format!("{}\n{}", self.declarations.join("\n"), code)
        };

        let (module, _interner) = compile::compile_source(&full_source)?;

        let mut vm = vm_setup::create_vm(&self.options);
        let result = vm.execute(&module)?;

        // If successful and this is a declaration, accumulate it
        let trimmed = code.trim();
        if is_declaration(trimmed) {
            self.declarations.push(code.to_string());
        }

        // Keep VM alive so we can read heap pointers (e.g., strings)
        self.last_vm = Some(vm);

        Ok(result)
    }

    /// Format a Value to a human-readable display string.
    ///
    /// Handles primitives directly and reads strings from the GC heap
    /// of the most recent eval's VM.
    pub fn format_value(&self, value: &Value) -> String {
        if value.is_null() {
            return "null".to_string();
        }
        if let Some(b) = value.as_bool() {
            return b.to_string();
        }
        if let Some(i) = value.as_i32() {
            return i.to_string();
        }
        if let Some(f) = value.as_f64() {
            if f.fract() == 0.0 && f.abs() < i64::MAX as f64 {
                return format!("{}", f as i64);
            }
            return f.to_string();
        }
        // Attempt to read as string from the last VM's GC heap
        if value.is_ptr() {
            if let Some(s) = try_read_string(value) {
                return format!("\"{}\"", s);
            }
        }
        // Fallback: debug representation
        format!("{:?}", value)
    }

    /// Reset the session (discards all accumulated state).
    pub fn reset(&mut self, options: &RuntimeOptions) {
        self.options = options.clone();
        self.declarations.clear();
        self.last_vm = None;
    }
}

/// Check if code is a declaration that should be accumulated for persistence.
fn is_declaration(code: &str) -> bool {
    let trimmed = code.trim();
    trimmed.starts_with("let ")
        || trimmed.starts_with("const ")
        || trimmed.starts_with("function ")
        || trimmed.starts_with("class ")
        || trimmed.starts_with("import ")
        || trimmed.starts_with("export ")
}

/// Try to read a string value from a GC heap pointer.
///
/// # Safety
/// The Value must be a valid pointer to a RayaString that hasn't been freed.
/// This is safe when called immediately after eval, before the VM is dropped.
fn try_read_string(value: &Value) -> Option<String> {
    let ptr = unsafe { value.as_ptr::<RayaString>() }?;
    let s = unsafe { &*ptr.as_ptr() };
    Some(s.data.clone())
}
