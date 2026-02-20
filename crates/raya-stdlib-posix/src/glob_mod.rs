//! std:glob — File pattern matching
//!
//! Provides glob pattern matching for finding files on the filesystem
//! and testing paths against glob patterns. Uses the `glob` crate for
//! pattern expansion and matching.

use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};

/// Find files matching a glob pattern relative to the current working directory.
///
/// Args: pattern (string)
/// Returns: string[] of matching file paths
pub fn glob_find(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let pattern = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(_) => return NativeCallResult::Error("glob.find: expected pattern string".into()),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match glob::glob(&pattern) {
            Ok(paths) => {
                let results: Vec<String> = paths
                    .filter_map(|p| p.ok())
                    .map(|p| p.display().to_string())
                    .collect();
                IoCompletion::StringArray(results)
            }
            Err(e) => IoCompletion::Error(format!("glob.find: {}", e)),
        }),
    })
}

/// Find files matching a glob pattern within a specific directory.
///
/// Args: pattern (string), cwd (string)
/// Returns: string[] of matching file paths
pub fn glob_find_in_dir(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let pattern = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(_) => {
            return NativeCallResult::Error("glob.findInDir: expected pattern string".into())
        }
    };
    let cwd = match args.get(1).and_then(|v| ctx.read_string(*v).ok()) {
        Some(s) => s,
        None => {
            return NativeCallResult::Error("glob.findInDir: expected cwd string".into())
        }
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let full_pattern = format!("{}/{}", cwd, pattern);
            match glob::glob(&full_pattern) {
                Ok(paths) => {
                    let results: Vec<String> = paths
                        .filter_map(|p| p.ok())
                        .map(|p| p.display().to_string())
                        .collect();
                    IoCompletion::StringArray(results)
                }
                Err(e) => IoCompletion::Error(format!("glob.findInDir: {}", e)),
            }
        }),
    })
}

/// Test if a path matches a glob pattern (no filesystem access).
///
/// Args: path (string), pattern (string)
/// Returns: boolean
pub fn glob_matches(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(_) => return NativeCallResult::Error("glob.matches: expected path string".into()),
    };
    let pattern = match args.get(1).and_then(|v| ctx.read_string(*v).ok()) {
        Some(s) => s,
        None => return NativeCallResult::Error("glob.matches: expected pattern string".into()),
    };

    match glob::Pattern::new(&pattern) {
        Ok(pat) => NativeCallResult::bool(pat.matches(&path)),
        Err(e) => NativeCallResult::Error(format!("glob.matches: {}", e)),
    }
}
