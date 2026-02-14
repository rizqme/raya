//! Capability-based security system for Inner VMs
//!
//! Capabilities provide controlled access to host APIs for isolated VmContexts.
//! Each capability represents a specific permission that can be granted to an
//! Inner VM, following the principle of least privilege.

use crate::vm::value::Value;
use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use thiserror::Error;

/// Error types for capability operations
#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error("Capability not found: {0}")]
    NotFound(String),

    #[error("Capability invocation failed: {0}")]
    InvocationError(String),

    #[error("Invalid arguments for capability {0}")]
    InvalidArguments(String),

    #[error("Permission denied for capability {0}")]
    PermissionDenied(String),
}

/// Capability trait - allows host to expose controlled functionality to Inner VMs
///
/// Capabilities are the only way for Inner VMs to interact with the host environment.
/// They follow the principle of least privilege: Inner VMs only get the capabilities
/// they are explicitly granted.
pub trait Capability: Send + Sync {
    /// Get the capability name
    fn name(&self) -> &str;

    /// Invoke the capability with arguments
    ///
    /// # Arguments
    /// * `args` - Array of values passed from the VM
    ///
    /// # Returns
    /// * `Ok(Value)` - Result value to return to the VM
    /// * `Err(CapabilityError)` - Error if invocation fails
    fn invoke(&self, args: &[Value]) -> Result<Value, CapabilityError>;

    /// Get capability as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get capability description (for debugging/introspection)
    fn description(&self) -> &str {
        "No description provided"
    }
}

impl fmt::Debug for dyn Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Capability")
            .field("name", &self.name())
            .field("description", &self.description())
            .finish()
    }
}

/// Registry for capabilities available to a VmContext
///
/// Capabilities are registered by name and can be invoked from the VM
/// using the CAPABILITY_INVOKE opcode.
#[derive(Clone)]
pub struct CapabilityRegistry {
    capabilities: Arc<HashMap<String, Arc<dyn Capability>>>,
}

impl CapabilityRegistry {
    /// Create a new empty capability registry
    pub fn new() -> Self {
        Self {
            capabilities: Arc::new(HashMap::new()),
        }
    }

    /// Create a registry with a set of capabilities
    pub fn with_capabilities(caps: Vec<Arc<dyn Capability>>) -> Self {
        let mut map = HashMap::new();
        for cap in caps {
            map.insert(cap.name().to_string(), cap);
        }
        Self {
            capabilities: Arc::new(map),
        }
    }

    /// Register a capability
    ///
    /// Note: This creates a new registry with the added capability,
    /// as capabilities are immutable after VM creation.
    pub fn with_capability(self, capability: Arc<dyn Capability>) -> Self {
        let mut map = (*self.capabilities).clone();
        map.insert(capability.name().to_string(), capability);
        Self {
            capabilities: Arc::new(map),
        }
    }

    /// Get a capability by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Capability>> {
        self.capabilities.get(name).cloned()
    }

    /// Check if a capability exists
    pub fn has(&self, name: &str) -> bool {
        self.capabilities.contains_key(name)
    }

    /// Invoke a capability by name
    pub fn invoke(&self, name: &str, args: &[Value]) -> Result<Value, CapabilityError> {
        match self.get(name) {
            Some(cap) => cap.invoke(args),
            None => Err(CapabilityError::NotFound(name.to_string())),
        }
    }

    /// Get all capability names
    pub fn names(&self) -> Vec<&str> {
        self.capabilities.keys().map(|s| s.as_str()).collect()
    }

    /// Get the number of registered capabilities
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CapabilityRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapabilityRegistry")
            .field("count", &self.len())
            .field("capabilities", &self.names())
            .finish()
    }
}

// ============================================================================
// Built-in Capabilities
// ============================================================================

/// Log capability - allows Inner VM to log messages to host
#[derive(Debug)]
pub struct LogCapability {
    prefix: String,
}

impl LogCapability {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }
}

impl Capability for LogCapability {
    fn name(&self) -> &str {
        "log"
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, CapabilityError> {
        // Expected: log(message: string)
        if args.len() != 1 {
            return Err(CapabilityError::InvalidArguments(
                "log() expects 1 argument".to_string(),
            ));
        }

        // Convert value to string (simplified - real implementation would use proper string extraction)
        let message = format!("{:?}", args[0]);
        println!("[{}] {}", self.prefix, message);

        Ok(Value::null())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn description(&self) -> &str {
        "Allows Inner VM to log messages to the host console"
    }
}

/// Read capability - allows Inner VM to read host files (with path restrictions)
#[derive(Debug)]
pub struct ReadCapability {
    allowed_paths: Vec<String>,
}

impl ReadCapability {
    pub fn new(allowed_paths: Vec<String>) -> Self {
        Self { allowed_paths }
    }
}

impl Capability for ReadCapability {
    fn name(&self) -> &str {
        "fs.read"
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, CapabilityError> {
        // Expected: read(path: string) -> string
        if args.len() != 1 {
            return Err(CapabilityError::InvalidArguments(
                "fs.read() expects 1 argument".to_string(),
            ));
        }

        // TODO: Extract string from Value properly
        let _path = format!("{:?}", args[0]);

        // TODO: Check if path is in allowed_paths
        // TODO: Read file and return contents

        // For now, return null (not yet implemented)
        Err(CapabilityError::InvocationError(
            "fs.read not yet implemented".to_string(),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn description(&self) -> &str {
        "Allows Inner VM to read files from allowed paths"
    }
}

/// HTTP capability - allows Inner VM to make HTTP requests (with domain restrictions)
#[derive(Debug)]
pub struct HttpCapability {
    allowed_domains: Vec<String>,
}

impl HttpCapability {
    pub fn new(allowed_domains: Vec<String>) -> Self {
        Self { allowed_domains }
    }
}

impl Capability for HttpCapability {
    fn name(&self) -> &str {
        "http.fetch"
    }

    fn invoke(&self, args: &[Value]) -> Result<Value, CapabilityError> {
        // Expected: fetch(url: string) -> string
        if args.is_empty() {
            return Err(CapabilityError::InvalidArguments(
                "http.fetch() expects at least 1 argument".to_string(),
            ));
        }

        // TODO: Extract URL from Value
        // TODO: Check if domain is in allowed_domains
        // TODO: Make HTTP request

        Err(CapabilityError::InvocationError(
            "http.fetch not yet implemented".to_string(),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn description(&self) -> &str {
        "Allows Inner VM to make HTTP requests to allowed domains"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_registry_creation() {
        let registry = CapabilityRegistry::new();
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_capability_registry_with_capabilities() {
        let log_cap = Arc::new(LogCapability::new("test"));
        let registry = CapabilityRegistry::with_capabilities(vec![log_cap]);

        assert_eq!(registry.len(), 1);
        assert!(registry.has("log"));
        assert!(!registry.has("fs.read"));
    }

    #[test]
    fn test_capability_registry_add() {
        let registry = CapabilityRegistry::new();
        let registry = registry.with_capability(Arc::new(LogCapability::new("test")));

        assert_eq!(registry.len(), 1);
        assert!(registry.has("log"));

        let registry = registry.with_capability(Arc::new(ReadCapability::new(vec![])));
        assert_eq!(registry.len(), 2);
        assert!(registry.has("log"));
        assert!(registry.has("fs.read"));
    }

    #[test]
    fn test_capability_get() {
        let log_cap = Arc::new(LogCapability::new("test"));
        let registry = CapabilityRegistry::new().with_capability(log_cap);

        let cap = registry.get("log");
        assert!(cap.is_some());
        assert_eq!(cap.unwrap().name(), "log");

        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_capability_invoke_not_found() {
        let registry = CapabilityRegistry::new();
        let result = registry.invoke("nonexistent", &[]);

        assert!(result.is_err());
        matches!(result.unwrap_err(), CapabilityError::NotFound(_));
    }

    #[test]
    fn test_log_capability() {
        let cap = LogCapability::new("test");
        assert_eq!(cap.name(), "log");

        // Test with valid args
        let result = cap.invoke(&[Value::i32(42)]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::null());

        // Test with invalid args
        let result = cap.invoke(&[]);
        assert!(result.is_err());
        matches!(result.unwrap_err(), CapabilityError::InvalidArguments(_));
    }

    #[test]
    fn test_capability_names() {
        let registry = CapabilityRegistry::new()
            .with_capability(Arc::new(LogCapability::new("test")))
            .with_capability(Arc::new(ReadCapability::new(vec![])));

        let names = registry.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"log"));
        assert!(names.contains(&"fs.read"));
    }

    #[test]
    fn test_read_capability_creation() {
        let cap = ReadCapability::new(vec!["/allowed/path".to_string()]);
        assert_eq!(cap.name(), "fs.read");
    }

    #[test]
    fn test_http_capability_creation() {
        let cap = HttpCapability::new(vec!["example.com".to_string()]);
        assert_eq!(cap.name(), "http.fetch");
    }
}
