//! Path method handlers (std:path)
//!
//! Native implementation of std:path module for path manipulation,
//! resolution, and OS-specific constants.

use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};

use parking_lot::Mutex;

use crate::vm::builtin::path as path_ids;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::RayaString;
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::VmError;

// ============================================================================
// Handler Context
// ============================================================================

/// Context needed for path method execution
pub struct PathHandlerContext<'a> {
    /// GC for allocating strings
    pub gc: &'a Mutex<Gc>,
}

// ============================================================================
// Handler
// ============================================================================

/// Handle built-in path methods (std:path)
pub fn call_path_method(
    ctx: &PathHandlerContext,
    stack: &mut Stack,
    method_id: u16,
    arg_count: usize,
) -> Result<(), VmError> {
    // Pop arguments
    let mut args = Vec::with_capacity(arg_count);
    for _ in 0..arg_count {
        args.push(stack.pop()?);
    }
    args.reverse();

    // Helper to get string from Value
    let get_string = |v: Value| -> Result<String, VmError> {
        if !v.is_ptr() {
            return Err(VmError::TypeError("Expected string".to_string()));
        }
        let s_ptr = unsafe { v.as_ptr::<RayaString>() };
        let s = unsafe { &*s_ptr.unwrap().as_ptr() };
        Ok(s.data.clone())
    };

    let result = match method_id {
        path_ids::JOIN => {
            // path.join(a, b): string
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "path.join requires 2 arguments".to_string(),
                ));
            }
            let a = get_string(args[0])?;
            let b = get_string(args[1])?;
            let joined = PathBuf::from(&a).join(&b).to_string_lossy().to_string();
            allocate_string(ctx, joined)
        }

        path_ids::NORMALIZE => {
            // path.normalize(p): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "path.normalize requires 1 argument".to_string(),
                ));
            }
            let p = get_string(args[0])?;
            let path = Path::new(&p);
            let mut components = Vec::new();
            for comp in path.components() {
                match comp {
                    std::path::Component::ParentDir => {
                        components.pop();
                    }
                    std::path::Component::CurDir => {}
                    other => components.push(other),
                }
            }
            let normalized = components
                .iter()
                .collect::<PathBuf>()
                .to_string_lossy()
                .to_string();
            allocate_string(ctx, normalized)
        }

        path_ids::DIRNAME => {
            // path.dirname(p): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "path.dirname requires 1 argument".to_string(),
                ));
            }
            let p = get_string(args[0])?;
            let dir = Path::new(&p)
                .parent()
                .map_or(".", |p| p.to_str().unwrap_or("."))
                .to_string();
            allocate_string(ctx, dir)
        }

        path_ids::BASENAME => {
            // path.basename(p): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "path.basename requires 1 argument".to_string(),
                ));
            }
            let p = get_string(args[0])?;
            let base = Path::new(&p)
                .file_name()
                .map_or("", |n| n.to_str().unwrap_or(""))
                .to_string();
            allocate_string(ctx, base)
        }

        path_ids::EXTNAME => {
            // path.extname(p): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "path.extname requires 1 argument".to_string(),
                ));
            }
            let p = get_string(args[0])?;
            let ext = Path::new(&p)
                .extension()
                .map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
            allocate_string(ctx, ext)
        }

        path_ids::IS_ABSOLUTE => {
            // path.isAbsolute(p): boolean
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "path.isAbsolute requires 1 argument".to_string(),
                ));
            }
            let p = get_string(args[0])?;
            Value::bool(Path::new(&p).is_absolute())
        }

        path_ids::RESOLVE => {
            // path.resolve(from, to): string
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "path.resolve requires 2 arguments".to_string(),
                ));
            }
            let from = get_string(args[0])?;
            let to = get_string(args[1])?;
            let base = if Path::new(&from).is_absolute() {
                PathBuf::from(&from)
            } else {
                std::env::current_dir().unwrap_or_default().join(&from)
            };
            let resolved = base.join(&to).to_string_lossy().to_string();
            allocate_string(ctx, resolved)
        }

        path_ids::RELATIVE => {
            // path.relative(from, to): string
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "path.relative requires 2 arguments".to_string(),
                ));
            }
            let from = get_string(args[0])?;
            let to = get_string(args[1])?;
            let rel = pathdiff::diff_paths(&to, &from)
                .map_or(to.clone(), |p| p.to_string_lossy().to_string());
            allocate_string(ctx, rel)
        }

        path_ids::CWD => {
            // path.cwd(): string
            let cwd = std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            allocate_string(ctx, cwd)
        }

        path_ids::SEP => {
            // path.sep(): string
            allocate_string(ctx, MAIN_SEPARATOR_STR.to_string())
        }

        path_ids::DELIMITER => {
            // path.delimiter(): string
            let delim = if cfg!(windows) { ";" } else { ":" };
            allocate_string(ctx, delim.to_string())
        }

        _ => {
            return Err(VmError::RuntimeError(format!(
                "Unknown path method: {:#06x}",
                method_id
            )));
        }
    };

    stack.push(result)?;
    Ok(())
}

/// Allocate a string on the GC heap and return a Value
fn allocate_string(ctx: &PathHandlerContext, s: String) -> Value {
    let raya_str = RayaString::new(s);
    let gc_ptr = ctx.gc.lock().allocate(raya_str);
    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
}
