//! Native function registry
//!
//! Re-exports `NativeFunctionRegistry` from raya-sdk and provides
//! `ResolvedNatives` — the engine-side dispatch table built at load time.

pub use raya_sdk::{NativeHandlerFn as NativeFn, NativeFunctionRegistry};
use raya_sdk::{NativeContext, NativeValue, NativeCallResult};

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
        ctx: &dyn NativeContext,
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
