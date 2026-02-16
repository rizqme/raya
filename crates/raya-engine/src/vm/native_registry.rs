//! Native function registry
//!
//! Provides a name→handler map for native functions. Stdlib modules register
//! their native functions by name (e.g., "math.abs", "time.now"). At module
//! load time, the VM resolves module-local native indices to handler functions
//! via this registry.

use crate::vm::abi::{NativeContext, NativeValue};
use crate::vm::native_handler::NativeCallResult;
use std::collections::HashMap;
use std::sync::Arc;

/// A native function handler
pub type NativeFn = Arc<dyn Fn(&NativeContext, &[NativeValue]) -> NativeCallResult + Send + Sync>;

/// Registry of native functions indexed by symbolic name.
///
/// Used at module load time to resolve symbolic native call names
/// (stored in bytecode) to handler functions.
pub struct NativeFunctionRegistry {
    handlers: HashMap<String, NativeFn>,
}

impl NativeFunctionRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a native function by name
    pub fn register(
        &mut self,
        name: &str,
        handler: impl Fn(&NativeContext, &[NativeValue]) -> NativeCallResult + Send + Sync + 'static,
    ) {
        self.handlers.insert(name.to_string(), Arc::new(handler));
    }

    /// Get a handler by name (used at link time)
    pub fn get(&self, name: &str) -> Option<NativeFn> {
        self.handlers.get(name).cloned()
    }

    /// Check if a handler is registered
    pub fn contains(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    /// Get the number of registered handlers
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl Default for NativeFunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolved native function table for a loaded module.
///
/// Built at load time by resolving each name in the module's `native_functions`
/// table to a handler from the `NativeFunctionRegistry`. After linking, dispatch
/// is a direct indexed call into a Vec — no hash lookup at runtime.
pub struct ResolvedNatives {
    handlers: Vec<NativeFn>,
}

impl std::fmt::Debug for ResolvedNatives {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedNatives")
            .field("count", &self.handlers.len())
            .finish()
    }
}

impl ResolvedNatives {
    /// Link a module's native function names to handlers from the registry.
    ///
    /// Returns an error if any name cannot be resolved.
    pub fn link(
        native_functions: &[String],
        registry: &NativeFunctionRegistry,
    ) -> Result<Self, String> {
        let mut handlers = Vec::with_capacity(native_functions.len());
        for name in native_functions {
            match registry.get(name) {
                Some(handler) => handlers.push(handler),
                None => {
                    return Err(format!("Unknown native function: {}", name));
                }
            }
        }
        Ok(Self { handlers })
    }

    /// Create an empty resolved natives table (for modules with no native calls)
    pub fn empty() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Call a native function by local index
    pub fn call(
        &self,
        local_idx: u16,
        ctx: &NativeContext,
        args: &[NativeValue],
    ) -> NativeCallResult {
        if let Some(handler) = self.handlers.get(local_idx as usize) {
            handler(ctx, args)
        } else {
            NativeCallResult::Error(format!("Invalid native function index: {}", local_idx))
        }
    }

    /// Get the number of resolved handlers
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Check if there are no resolved handlers
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = NativeFunctionRegistry::new();
        registry.register("math.abs", |_ctx, _args| NativeCallResult::f64(42.0));

        assert!(registry.contains("math.abs"));
        assert!(!registry.contains("math.sqrt"));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_resolved_natives_link() {
        let mut registry = NativeFunctionRegistry::new();
        registry.register("math.abs", |_ctx, _args| NativeCallResult::f64(1.0));
        registry.register("math.sqrt", |_ctx, _args| NativeCallResult::f64(2.0));

        let names = vec!["math.abs".to_string(), "math.sqrt".to_string()];
        let resolved = ResolvedNatives::link(&names, &registry).unwrap();
        assert_eq!(resolved.len(), 2);
    }

    #[test]
    fn test_resolved_natives_link_error() {
        let registry = NativeFunctionRegistry::new();
        let names = vec!["math.unknown".to_string()];
        let result = ResolvedNatives::link(&names, &registry);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("math.unknown"));
    }

    #[test]
    fn test_resolved_natives_empty() {
        let resolved = ResolvedNatives::empty();
        assert!(resolved.is_empty());
        assert_eq!(resolved.len(), 0);
    }
}
