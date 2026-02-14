//! VM-specific native module functions
//!
//! This module provides VM-specific functionality for native modules:
//! - GC pinning (to prevent values from being collected during native calls)
//! - Module registration with the VM
//!
//! The core FFI types (NativeValue, NativeModule, etc.) are provided by raya-sdk.

use crate::vm::interpreter::VmContext;
use raya_sdk::{NativeModule, NativeValue};

// ============================================================================
// GC Pinning Functions (Stubs for now)
// ============================================================================

/// Pin a value to prevent GC from moving/freeing it.
///
/// MUST be called by the VM before passing a value to native code.
/// The GC will not collect any value with pin_count > 0.
///
/// # Safety
///
/// This function is safe to call, but the VM must ensure proper pairing
/// with `unpin_value()`. Failure to unpin will cause memory leaks.
///
/// # Thread Safety
///
/// This function uses atomic operations and is safe to call from any thread.
///
/// # Current Implementation
///
/// This is currently a no-op stub. Full implementation will:
/// 1. Locate the ValueHeader for this value
/// 2. Atomically increment pin_count with Ordering::AcqRel
/// 3. Ensure the value is visible to all threads
pub fn pin_value(_value: NativeValue) {
    // TODO: Implement atomic pin_count increment
    // See design/ABI_SAFETY.md for full specification
}

/// Unpin a value to allow GC to collect it.
///
/// MUST be called by the VM after a native function returns.
/// Should be used with RAII guards to ensure unpinning even on panic.
///
/// # Safety
///
/// This function is safe to call, but panics if the value is not pinned
/// (debug builds only). The VM must not unpin a value more times than it
/// was pinned.
///
/// # Thread Safety
///
/// This function uses atomic operations and is safe to call from any thread.
///
/// # Current Implementation
///
/// This is currently a no-op stub. Full implementation will:
/// 1. Locate the ValueHeader for this value
/// 2. Atomically decrement pin_count with Ordering::AcqRel
/// 3. Assert pin_count was > 0 (debug builds)
pub fn unpin_value(_value: NativeValue) {
    // TODO: Implement atomic pin_count decrement
    // See design/ABI_SAFETY.md for full specification
}

// ============================================================================
// VM Context Extension
// ============================================================================

/// Register a native module with the VM context.
///
/// The module will be available to Raya code via standard imports.
/// Users won't know the difference between native and bytecode modules:
///
/// ```raya
/// // These look the same to users, but json is native, mylib is bytecode
/// import { parse } from "std:json";
/// import { helper } from "custom:mylib";
/// ```
///
/// The module resolver automatically detects whether to load:
/// - `.ryb` files (bytecode modules)
/// - `.so/.dylib/.dll` files (native modules)
///
/// TODO: This should be a method on VmContext once we add native module storage.
pub fn register_native_module(_vm: &mut VmContext, module: NativeModule) -> Result<(), String> {
    // TODO: Store module in VM context's module registry
    // The module resolver will check native modules first, then bytecode
    // For now, just log registration
    println!(
        "Registered native module '{}' v{} with {} functions",
        module.name(),
        module.version(),
        module.function_count()
    );

    Ok(())
}
