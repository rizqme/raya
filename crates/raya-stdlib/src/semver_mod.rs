//! Semver module implementation (std:semver)
//!
//! Native implementation using the `semver` crate for semantic versioning
//! operations including parsing, comparison, and range matching.

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

// ============================================================================
// Handle Registry
// ============================================================================

/// Next unique handle ID for parsed semver versions.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Global registry mapping numeric handles to parsed semver versions.
static VERSIONS: LazyLock<Mutex<HashMap<u64, semver::Version>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ============================================================================
// Helpers
// ============================================================================

/// Extract a handle (u64) from a NativeValue argument at the given index.
fn get_handle(args: &[NativeValue], index: usize) -> u64 {
    args.get(index)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64
}

/// Parse two version strings from args for comparison operations.
fn parse_two_versions(
    ctx: &dyn NativeContext,
    args: &[NativeValue],
    fn_name: &str,
) -> Result<(semver::Version, semver::Version), NativeCallResult> {
    if args.len() < 2 {
        return Err(NativeCallResult::Error(format!(
            "semver.{}: requires 2 arguments",
            fn_name
        )));
    }
    let a_str = ctx.read_string(args[0]).map_err(|e| {
        NativeCallResult::Error(format!("semver.{}: invalid first argument: {}", fn_name, e))
    })?;
    let b_str = ctx.read_string(args[1]).map_err(|e| {
        NativeCallResult::Error(format!("semver.{}: invalid second argument: {}", fn_name, e))
    })?;
    let a = semver::Version::parse(&a_str).map_err(|e| {
        NativeCallResult::Error(format!("semver.{}: invalid version '{}': {}", fn_name, a_str, e))
    })?;
    let b = semver::Version::parse(&b_str).map_err(|e| {
        NativeCallResult::Error(format!("semver.{}: invalid version '{}': {}", fn_name, b_str, e))
    })?;
    Ok((a, b))
}

// ============================================================================
// Public API
// ============================================================================

/// Handle semver method calls by numeric ID.
///
/// Routes IDs in the 0xC000-0xC01F range to their implementations.
pub fn call_semver_method(
    ctx: &dyn NativeContext,
    method_id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match method_id {
        0xC000 => semver_parse(ctx, args),
        0xC001 => semver_valid(ctx, args),
        0xC002 => semver_compare(ctx, args),
        0xC003 => semver_satisfies(ctx, args),
        0xC004 => semver_gt(ctx, args),
        0xC005 => semver_gte(ctx, args),
        0xC006 => semver_lt(ctx, args),
        0xC007 => semver_lte(ctx, args),
        0xC008 => semver_eq(ctx, args),
        0xC010 => version_major(args),
        0xC011 => version_minor(args),
        0xC012 => version_patch(args),
        0xC013 => version_prerelease(ctx, args),
        0xC014 => version_build(ctx, args),
        0xC015 => version_to_string(ctx, args),
        0xC016 => version_release(args),
        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// Parsing
// ============================================================================

/// Parse a semver version string and store it as a handle.
///
/// Returns the handle as f64 for use in subsequent version accessor calls.
fn semver_parse(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("semver.parse: requires 1 argument".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("semver.parse: invalid input: {}", e)),
    };
    match semver::Version::parse(&input) {
        Ok(version) => {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            VERSIONS.lock().insert(id, version);
            NativeCallResult::f64(id as f64)
        }
        Err(e) => NativeCallResult::Error(format!("semver.parse: invalid version '{}': {}", input, e)),
    }
}

/// Check if a string is a valid semver version.
///
/// Returns true if the string can be parsed as a valid semantic version.
fn semver_valid(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("semver.valid: requires 1 argument".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("semver.valid: invalid input: {}", e)),
    };
    NativeCallResult::bool(semver::Version::parse(&input).is_ok())
}

// ============================================================================
// Comparison
// ============================================================================

/// Compare two version strings.
///
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
fn semver_compare(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    match parse_two_versions(ctx, args, "compare") {
        Ok((a, b)) => {
            let result = match a.cmp(&b) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            };
            NativeCallResult::f64(result)
        }
        Err(e) => e,
    }
}

/// Check if a version satisfies a semver range requirement.
///
/// Uses `semver::VersionReq` for range parsing and matching.
fn semver_satisfies(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("semver.satisfies: requires 2 arguments".to_string());
    }
    let version_str = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("semver.satisfies: invalid version: {}", e)),
    };
    let range_str = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("semver.satisfies: invalid range: {}", e)),
    };
    let version = match semver::Version::parse(&version_str) {
        Ok(v) => v,
        Err(e) => return NativeCallResult::Error(format!(
            "semver.satisfies: invalid version '{}': {}",
            version_str, e
        )),
    };
    let req = match semver::VersionReq::parse(&range_str) {
        Ok(r) => r,
        Err(e) => return NativeCallResult::Error(format!(
            "semver.satisfies: invalid range '{}': {}",
            range_str, e
        )),
    };
    NativeCallResult::bool(req.matches(&version))
}

/// Check if version a is greater than version b.
fn semver_gt(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    match parse_two_versions(ctx, args, "gt") {
        Ok((a, b)) => NativeCallResult::bool(a > b),
        Err(e) => e,
    }
}

/// Check if version a is greater than or equal to version b.
fn semver_gte(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    match parse_two_versions(ctx, args, "gte") {
        Ok((a, b)) => NativeCallResult::bool(a >= b),
        Err(e) => e,
    }
}

/// Check if version a is less than version b.
fn semver_lt(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    match parse_two_versions(ctx, args, "lt") {
        Ok((a, b)) => NativeCallResult::bool(a < b),
        Err(e) => e,
    }
}

/// Check if version a is less than or equal to version b.
fn semver_lte(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    match parse_two_versions(ctx, args, "lte") {
        Ok((a, b)) => NativeCallResult::bool(a <= b),
        Err(e) => e,
    }
}

/// Check if version a is equal to version b.
fn semver_eq(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    match parse_two_versions(ctx, args, "eq") {
        Ok((a, b)) => NativeCallResult::bool(a == b),
        Err(e) => e,
    }
}

// ============================================================================
// Version Component Accessors (handle-based)
// ============================================================================

/// Get the major version number from a parsed version handle.
fn version_major(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VERSIONS.lock().get(&handle) {
        Some(v) => NativeCallResult::f64(v.major as f64),
        None => NativeCallResult::Error("semver.versionMajor: invalid handle".to_string()),
    }
}

/// Get the minor version number from a parsed version handle.
fn version_minor(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VERSIONS.lock().get(&handle) {
        Some(v) => NativeCallResult::f64(v.minor as f64),
        None => NativeCallResult::Error("semver.versionMinor: invalid handle".to_string()),
    }
}

/// Get the patch version number from a parsed version handle.
fn version_patch(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VERSIONS.lock().get(&handle) {
        Some(v) => NativeCallResult::f64(v.patch as f64),
        None => NativeCallResult::Error("semver.versionPatch: invalid handle".to_string()),
    }
}

/// Get the prerelease string from a parsed version handle.
///
/// Returns an empty string if no prerelease identifier is present.
fn version_prerelease(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VERSIONS.lock().get(&handle) {
        Some(v) => {
            let pre = v.pre.to_string();
            NativeCallResult::Value(ctx.create_string(&pre))
        }
        None => NativeCallResult::Error("semver.versionPrerelease: invalid handle".to_string()),
    }
}

/// Get the build metadata string from a parsed version handle.
///
/// Returns an empty string if no build metadata is present.
fn version_build(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VERSIONS.lock().get(&handle) {
        Some(v) => {
            let build = v.build.to_string();
            NativeCallResult::Value(ctx.create_string(&build))
        }
        None => NativeCallResult::Error("semver.versionBuild: invalid handle".to_string()),
    }
}

/// Get the full version string from a parsed version handle.
fn version_to_string(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VERSIONS.lock().get(&handle) {
        Some(v) => {
            let s = v.to_string();
            NativeCallResult::Value(ctx.create_string(&s))
        }
        None => NativeCallResult::Error("semver.versionToString: invalid handle".to_string()),
    }
}

/// Release a parsed version handle, freeing the associated memory.
fn version_release(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    VERSIONS.lock().remove(&handle);
    NativeCallResult::null()
}
