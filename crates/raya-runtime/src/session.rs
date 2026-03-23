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

use std::any::TypeId;

use raya_engine::vm::gc::{header_ptr_from_value_ptr, GcHeader};
use raya_engine::vm::object::{
    Array, Buffer, ChannelObject, DateObject, MapObject, RegExpObject, SetObject,
};
use raya_engine::vm::{Object, RayaString, Value, Vm, VmError};

use crate::error::RuntimeError;
use crate::{vm_setup, Runtime, RuntimeOptions};

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
    /// Prior declarations are combined with the new code and the whole thing is
    /// compiled and executed as a single module on a fresh VM.
    pub fn eval(&mut self, code: &str) -> Result<Value, RuntimeError> {
        // Build full source: accumulated declarations + new code
        let full_source = if self.declarations.is_empty() {
            code.to_string()
        } else {
            format!("{}\n{}", self.declarations.join("\n"), code)
        };

        let runtime = Runtime::with_options(self.options.clone());
        let program = runtime.compile_program_source(&full_source)?;

        let trimmed = code.trim();
        let mut vm = vm_setup::create_vm(&self.options);
        // REPL/session should execute only the compiler entry main and avoid
        // implicit invocation of user-defined `main`.
        let result = match runtime.execute_program_with_vm(&program, &mut vm) {
            Ok(value) => value,
            Err(RuntimeError::Vm(VmError::RuntimeError(message)))
                if message == "No main function" && is_declaration(trimmed) =>
            {
                // Declaration-only cells are valid session updates even when they
                // do not produce an entry function body.
                Value::null()
            }
            Err(error) => return Err(error),
        };

        // If successful and this is a declaration, accumulate it.
        if is_declaration(trimmed) {
            self.declarations.push(code.to_string());
        }

        // Keep VM alive so we can read heap pointers (e.g., strings)
        self.last_vm = Some(vm);

        Ok(result)
    }

    /// Evaluate code by routing it through the VM's JS `eval(...)` implementation.
    ///
    /// This is used by CLI eval mode for JS/TS node-compat snippets so the
    /// snippet is parsed and executed by the VM's eval infrastructure rather
    /// than the outer program wrapper/compiler entry path.
    pub fn eval_via_vm(&mut self, code: &str) -> Result<Value, RuntimeError> {
        let runtime = Runtime::with_options(self.options.clone());
        let bootstrap = format!("return eval({code:?});");
        let program = runtime.compile_program_source(&bootstrap)?;
        let mut vm = vm_setup::create_vm(&self.options);
        let result = runtime.execute_program_with_vm(&program, &mut vm)?;
        self.last_vm = Some(vm);
        Ok(result)
    }

    /// Format a Value to a human-readable display string.
    ///
    /// Handles primitives directly and reads heap objects (strings, objects,
    /// arrays, closures, etc.) using the GC header type info and class registry.
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
        // For heap pointers, use GcHeader type_id to determine the actual type
        if value.is_ptr() {
            if let Some(vm) = &self.last_vm {
                return format_heap_value(value, vm);
            }
            // No VM available — try legacy string read
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

/// Read the GcHeader for a heap-allocated Value.
///
/// # Safety
/// The Value must be a valid heap pointer that hasn't been freed.
unsafe fn read_gc_header(value: &Value) -> Option<&GcHeader> {
    let ptr = value.as_ptr::<u8>()?;
    Some(&*header_ptr_from_value_ptr(ptr.as_ptr()))
}

/// Format a heap-allocated value using GcHeader type info and the VM's class registry.
fn format_heap_value(value: &Value, vm: &Vm) -> String {
    let header = unsafe { read_gc_header(value) };
    let Some(header) = header else {
        return format!("{:?}", value);
    };

    let tid = header.type_id();

    // String
    if tid == TypeId::of::<RayaString>() {
        if let Some(s) = try_read_string(value) {
            return format!("\"{}\"", s);
        }
    }

    // Object (class instance or callable)
    if tid == TypeId::of::<Object>() {
        let obj = unsafe { &*(value.as_ptr::<Object>().unwrap().as_ptr()) };
        if obj.is_callable() {
            return "[Function]".to_string();
        }
        if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
            let classes = vm.shared_state().classes.read();
            if let Some(class) = classes.get_class(nominal_type_id) {
                return format!("[object {}]", class.name);
            }
            return format!("[object #{}]", nominal_type_id);
        }
        return "[object structural]".to_string();
    }

    // Array
    if tid == TypeId::of::<Array>() {
        let arr = unsafe { &*(value.as_ptr::<Array>().unwrap().as_ptr()) };
        return format!("[Array({})]", arr.len());
    }

    // Map
    if tid == TypeId::of::<MapObject>() {
        let map = unsafe { &*(value.as_ptr::<MapObject>().unwrap().as_ptr()) };
        return format!("[Map({})]", map.size());
    }

    // Set
    if tid == TypeId::of::<SetObject>() {
        let set = unsafe { &*(value.as_ptr::<SetObject>().unwrap().as_ptr()) };
        return format!("[Set({})]", set.size());
    }

    // Buffer
    if tid == TypeId::of::<Buffer>() {
        let buf = unsafe { &*(value.as_ptr::<Buffer>().unwrap().as_ptr()) };
        return format!("[Buffer({})]", buf.length());
    }

    // Date
    if tid == TypeId::of::<DateObject>() {
        let date = unsafe { &*(value.as_ptr::<DateObject>().unwrap().as_ptr()) };
        return date.to_iso_string();
    }

    // RegExp
    if tid == TypeId::of::<RegExpObject>() {
        let re = unsafe { &*(value.as_ptr::<RegExpObject>().unwrap().as_ptr()) };
        return format!("/{}/{}", re.pattern, re.flags);
    }

    // Channel
    if tid == TypeId::of::<ChannelObject>() {
        let ch = unsafe { &*(value.as_ptr::<ChannelObject>().unwrap().as_ptr()) };
        return format!("[Channel(cap: {})]", ch.capacity());
    }

    format!("{:?}", value)
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
