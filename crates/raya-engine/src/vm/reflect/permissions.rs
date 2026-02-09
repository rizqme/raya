//! Reflection Security & Permissions
//!
//! Controls access to reflection capabilities with fine-grained permission flags.
//! This module implements Phase 16: Reflection Security & Permissions.
//!
//! ## Native Call IDs (0x0E00-0x0E0F)
//!
//! | ID     | Method                    | Description                          |
//! |--------|---------------------------|--------------------------------------|
//! | 0x0E00 | setPermissions            | Set object-level permissions         |
//! | 0x0E01 | getPermissions            | Get resolved permissions             |
//! | 0x0E02 | hasPermission             | Check specific permission flag       |
//! | 0x0E03 | clearPermissions          | Clear object-level permissions       |
//! | 0x0E04 | setClassPermissions       | Set class-level permissions          |
//! | 0x0E05 | getClassPermissions       | Get class-level permissions          |
//! | 0x0E06 | clearClassPermissions     | Clear class-level permissions        |
//! | 0x0E07 | setModulePermissions      | Set module-level permissions         |
//! | 0x0E08 | getModulePermissions      | Get module-level permissions         |
//! | 0x0E09 | clearModulePermissions    | Clear module-level permissions       |
//! | 0x0E0A | setGlobalPermissions      | Set global default                   |
//! | 0x0E0B | getGlobalPermissions      | Get global default                   |
//! | 0x0E0C | sealPermissions           | Make permissions immutable           |
//! | 0x0E0D | isPermissionsSealed       | Check if permissions are sealed      |
//!
//! ## TOML Configuration
//!
//! Module permissions can be configured in `raya.toml`:
//!
//! ```toml
//! [reflect.permissions]
//! global = "ALL"  # Default for all modules
//!
//! [reflect.permissions.modules]
//! "myapp" = "FULL_ACCESS"
//! "plugins/*" = "PUBLIC_ONLY"
//! "untrusted/*" = "READ_PUBLIC"
//! ```

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use crate::vm::VmError;

/// Reflection permission flags (bitflags)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReflectionPermission(u8);

impl ReflectionPermission {
    /// No reflection allowed
    pub const NONE: Self = Self(0x00);
    /// Read public fields
    pub const READ_PUBLIC: Self = Self(0x01);
    /// Read private fields
    pub const READ_PRIVATE: Self = Self(0x02);
    /// Write public fields
    pub const WRITE_PUBLIC: Self = Self(0x04);
    /// Write private fields
    pub const WRITE_PRIVATE: Self = Self(0x08);
    /// Invoke public methods
    pub const INVOKE_PUBLIC: Self = Self(0x10);
    /// Invoke private methods
    pub const INVOKE_PRIVATE: Self = Self(0x20);
    /// Create classes/subclasses dynamically
    pub const CREATE_TYPES: Self = Self(0x40);
    /// Use BytecodeBuilder, createFunction
    pub const GENERATE_CODE: Self = Self(0x80);

    // Common combinations
    /// READ_PUBLIC | READ_PRIVATE
    pub const READ_ALL: Self = Self(0x03);
    /// WRITE_PUBLIC | WRITE_PRIVATE
    pub const WRITE_ALL: Self = Self(0x0C);
    /// INVOKE_PUBLIC | INVOKE_PRIVATE
    pub const INVOKE_ALL: Self = Self(0x30);
    /// READ_PUBLIC | WRITE_PUBLIC | INVOKE_PUBLIC
    pub const PUBLIC_ONLY: Self = Self(0x15);
    /// All read/write/invoke (no code generation)
    pub const FULL_ACCESS: Self = Self(0x3F);
    /// Everything including code generation
    pub const ALL: Self = Self(0xFF);

    /// Create from raw bits
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// Get raw bits
    pub const fn bits(&self) -> u8 {
        self.0
    }

    /// Check if permission contains a flag
    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Union of permissions
    pub const fn union(&self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Intersection of permissions
    pub const fn intersection(&self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Difference (remove flags)
    pub const fn difference(&self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Parse from string representation
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "NONE" => Some(Self::NONE),
            "READ_PUBLIC" => Some(Self::READ_PUBLIC),
            "READ_PRIVATE" => Some(Self::READ_PRIVATE),
            "WRITE_PUBLIC" => Some(Self::WRITE_PUBLIC),
            "WRITE_PRIVATE" => Some(Self::WRITE_PRIVATE),
            "INVOKE_PUBLIC" => Some(Self::INVOKE_PUBLIC),
            "INVOKE_PRIVATE" => Some(Self::INVOKE_PRIVATE),
            "CREATE_TYPES" => Some(Self::CREATE_TYPES),
            "GENERATE_CODE" => Some(Self::GENERATE_CODE),
            "READ_ALL" => Some(Self::READ_ALL),
            "WRITE_ALL" => Some(Self::WRITE_ALL),
            "INVOKE_ALL" => Some(Self::INVOKE_ALL),
            "PUBLIC_ONLY" => Some(Self::PUBLIC_ONLY),
            "FULL_ACCESS" => Some(Self::FULL_ACCESS),
            "ALL" => Some(Self::ALL),
            _ => {
                // Try parsing as hex or decimal
                if let Some(hex) = s.strip_prefix("0x") {
                    u8::from_str_radix(hex, 16).ok().map(Self::from_bits)
                } else {
                    s.parse::<u8>().ok().map(Self::from_bits)
                }
            }
        }
    }

    /// Convert to string representation
    pub fn to_string(&self) -> String {
        match *self {
            Self::NONE => "NONE".to_string(),
            Self::READ_PUBLIC => "READ_PUBLIC".to_string(),
            Self::READ_PRIVATE => "READ_PRIVATE".to_string(),
            Self::WRITE_PUBLIC => "WRITE_PUBLIC".to_string(),
            Self::WRITE_PRIVATE => "WRITE_PRIVATE".to_string(),
            Self::INVOKE_PUBLIC => "INVOKE_PUBLIC".to_string(),
            Self::INVOKE_PRIVATE => "INVOKE_PRIVATE".to_string(),
            Self::CREATE_TYPES => "CREATE_TYPES".to_string(),
            Self::GENERATE_CODE => "GENERATE_CODE".to_string(),
            Self::READ_ALL => "READ_ALL".to_string(),
            Self::WRITE_ALL => "WRITE_ALL".to_string(),
            Self::INVOKE_ALL => "INVOKE_ALL".to_string(),
            Self::PUBLIC_ONLY => "PUBLIC_ONLY".to_string(),
            Self::FULL_ACCESS => "FULL_ACCESS".to_string(),
            Self::ALL => "ALL".to_string(),
            _ => format!("0x{:02X}", self.0),
        }
    }

    /// Parse combined flags from pipe-separated string (e.g., "READ_PUBLIC|WRITE_PUBLIC")
    pub fn from_combined_str(s: &str) -> Option<Self> {
        let mut result = Self::NONE;
        for part in s.split('|') {
            let perm = Self::from_str(part.trim())?;
            result = result.union(perm);
        }
        Some(result)
    }
}

impl Default for ReflectionPermission {
    fn default() -> Self {
        Self::ALL
    }
}

impl std::fmt::Display for ReflectionPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

/// Entry in the permission store with optional sealing
#[derive(Debug, Clone)]
struct PermissionEntry {
    permissions: ReflectionPermission,
    sealed: bool,
}

impl PermissionEntry {
    fn new(permissions: ReflectionPermission) -> Self {
        Self {
            permissions,
            sealed: false,
        }
    }
}

/// Module permission pattern (supports wildcards)
#[derive(Debug, Clone)]
pub struct ModulePermissionRule {
    /// Pattern (e.g., "myapp", "plugins/*", "**")
    pub pattern: String,
    /// Permissions for matching modules
    pub permissions: ReflectionPermission,
}

impl ModulePermissionRule {
    /// Check if a module name matches this pattern
    pub fn matches(&self, module_name: &str) -> bool {
        if self.pattern == "**" || self.pattern == "*" {
            return true;
        }

        if let Some(prefix) = self.pattern.strip_suffix("/*") {
            module_name.starts_with(prefix) && module_name.len() > prefix.len()
        } else if let Some(prefix) = self.pattern.strip_suffix("/**") {
            module_name.starts_with(prefix)
        } else {
            self.pattern == module_name
        }
    }
}

/// Store for reflection permissions
#[derive(Debug, Default)]
pub struct PermissionStore {
    /// Global default permissions
    global_default: ReflectionPermission,

    /// Object-level permissions: object_id -> entry
    object_permissions: HashMap<usize, PermissionEntry>,

    /// Class-level permissions: class_id -> entry
    class_permissions: HashMap<usize, PermissionEntry>,

    /// Module-level permissions: module_name -> entry
    module_permissions: HashMap<String, PermissionEntry>,

    /// Module permission rules (for wildcard patterns)
    module_rules: Vec<ModulePermissionRule>,

    /// Sealed objects (cannot modify permissions)
    sealed_objects: HashSet<usize>,

    /// Sealed classes
    sealed_classes: HashSet<usize>,
}

impl PermissionStore {
    /// Create a new permission store with default (ALL) permissions
    pub fn new() -> Self {
        Self {
            global_default: ReflectionPermission::ALL,
            object_permissions: HashMap::new(),
            class_permissions: HashMap::new(),
            module_permissions: HashMap::new(),
            module_rules: Vec::new(),
            sealed_objects: HashSet::new(),
            sealed_classes: HashSet::new(),
        }
    }

    /// Check if any permissions are configured (for fast-path optimization)
    pub fn has_any_restrictions(&self) -> bool {
        self.global_default != ReflectionPermission::ALL
            || !self.object_permissions.is_empty()
            || !self.class_permissions.is_empty()
            || !self.module_permissions.is_empty()
            || !self.module_rules.is_empty()
    }

    // ===== Global Permissions =====

    /// Set global default permissions
    pub fn set_global(&mut self, permissions: ReflectionPermission) {
        self.global_default = permissions;
    }

    /// Get global default permissions
    pub fn get_global(&self) -> ReflectionPermission {
        self.global_default
    }

    // ===== Object Permissions =====

    /// Set object-level permissions
    pub fn set_object(&mut self, object_id: usize, permissions: ReflectionPermission) -> Result<(), VmError> {
        if self.sealed_objects.contains(&object_id) {
            return Err(VmError::RuntimeError(
                "Cannot modify sealed object permissions".to_string(),
            ));
        }
        self.object_permissions.insert(object_id, PermissionEntry::new(permissions));
        Ok(())
    }

    /// Get object-level permissions (not resolved)
    pub fn get_object(&self, object_id: usize) -> Option<ReflectionPermission> {
        self.object_permissions.get(&object_id).map(|e| e.permissions)
    }

    /// Clear object-level permissions
    pub fn clear_object(&mut self, object_id: usize) -> Result<(), VmError> {
        if self.sealed_objects.contains(&object_id) {
            return Err(VmError::RuntimeError(
                "Cannot modify sealed object permissions".to_string(),
            ));
        }
        self.object_permissions.remove(&object_id);
        Ok(())
    }

    /// Seal object permissions
    pub fn seal_object(&mut self, object_id: usize) {
        self.sealed_objects.insert(object_id);
    }

    /// Check if object permissions are sealed
    pub fn is_object_sealed(&self, object_id: usize) -> bool {
        self.sealed_objects.contains(&object_id)
    }

    // ===== Class Permissions =====

    /// Set class-level permissions
    pub fn set_class(&mut self, class_id: usize, permissions: ReflectionPermission) -> Result<(), VmError> {
        if self.sealed_classes.contains(&class_id) {
            return Err(VmError::RuntimeError(
                "Cannot modify sealed class permissions".to_string(),
            ));
        }
        self.class_permissions.insert(class_id, PermissionEntry::new(permissions));
        Ok(())
    }

    /// Get class-level permissions (not resolved)
    pub fn get_class(&self, class_id: usize) -> Option<ReflectionPermission> {
        self.class_permissions.get(&class_id).map(|e| e.permissions)
    }

    /// Clear class-level permissions
    pub fn clear_class(&mut self, class_id: usize) -> Result<(), VmError> {
        if self.sealed_classes.contains(&class_id) {
            return Err(VmError::RuntimeError(
                "Cannot modify sealed class permissions".to_string(),
            ));
        }
        self.class_permissions.remove(&class_id);
        Ok(())
    }

    /// Seal class permissions
    pub fn seal_class(&mut self, class_id: usize) {
        self.sealed_classes.insert(class_id);
    }

    /// Check if class permissions are sealed
    pub fn is_class_sealed(&self, class_id: usize) -> bool {
        self.sealed_classes.contains(&class_id)
    }

    // ===== Module Permissions =====

    /// Set module-level permissions
    pub fn set_module(&mut self, module_name: &str, permissions: ReflectionPermission) {
        self.module_permissions.insert(
            module_name.to_string(),
            PermissionEntry::new(permissions),
        );
    }

    /// Get module-level permissions (not resolved, direct match only)
    pub fn get_module(&self, module_name: &str) -> Option<ReflectionPermission> {
        self.module_permissions.get(module_name).map(|e| e.permissions)
    }

    /// Get module permissions with pattern matching
    pub fn get_module_resolved(&self, module_name: &str) -> Option<ReflectionPermission> {
        // First check direct match
        if let Some(entry) = self.module_permissions.get(module_name) {
            return Some(entry.permissions);
        }

        // Then check rules (first match wins)
        for rule in &self.module_rules {
            if rule.matches(module_name) {
                return Some(rule.permissions);
            }
        }

        None
    }

    /// Clear module-level permissions
    pub fn clear_module(&mut self, module_name: &str) {
        self.module_permissions.remove(module_name);
    }

    /// Add a module permission rule
    pub fn add_module_rule(&mut self, rule: ModulePermissionRule) {
        self.module_rules.push(rule);
    }

    /// Clear all module rules
    pub fn clear_module_rules(&mut self) {
        self.module_rules.clear();
    }

    // ===== Permission Resolution =====

    /// Resolve permissions for an object, checking all levels
    pub fn resolve(
        &self,
        object_id: Option<usize>,
        class_id: Option<usize>,
        module_name: Option<&str>,
    ) -> ReflectionPermission {
        // Object-level check (most specific)
        if let Some(oid) = object_id {
            if let Some(perms) = self.get_object(oid) {
                return perms;
            }
        }

        // Class-level check
        if let Some(cid) = class_id {
            if let Some(perms) = self.get_class(cid) {
                return perms;
            }
        }

        // Module-level check
        if let Some(name) = module_name {
            if let Some(perms) = self.get_module_resolved(name) {
                return perms;
            }
        }

        // Global default
        self.global_default
    }

    /// Check if a specific permission is allowed
    pub fn check_permission(
        &self,
        object_id: Option<usize>,
        class_id: Option<usize>,
        module_name: Option<&str>,
        required: ReflectionPermission,
    ) -> bool {
        let perms = self.resolve(object_id, class_id, module_name);
        perms.contains(required)
    }

    // ===== TOML Configuration =====

    /// Load permissions from TOML configuration
    pub fn load_from_toml(&mut self, toml_content: &str) -> Result<(), String> {
        // Parse TOML manually (simple parser for our specific format)
        // Expected format:
        // [reflect.permissions]
        // global = "ALL"
        // [reflect.permissions.modules]
        // "myapp" = "FULL_ACCESS"
        // "plugins/*" = "PUBLIC_ONLY"

        let mut in_permissions_section = false;
        let mut in_modules_section = false;

        for line in toml_content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Section headers
            if line == "[reflect.permissions]" {
                in_permissions_section = true;
                in_modules_section = false;
                continue;
            }
            if line == "[reflect.permissions.modules]" {
                in_permissions_section = false;
                in_modules_section = true;
                continue;
            }
            if line.starts_with('[') {
                in_permissions_section = false;
                in_modules_section = false;
                continue;
            }

            // Key-value pairs
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().trim_matches('"');
                let value = value.trim().trim_matches('"');

                if in_permissions_section && key == "global" {
                    if let Some(perms) = ReflectionPermission::from_combined_str(value) {
                        self.global_default = perms;
                    } else {
                        return Err(format!("Invalid permission value: {}", value));
                    }
                } else if in_modules_section {
                    if let Some(perms) = ReflectionPermission::from_combined_str(value) {
                        if key.contains('*') {
                            // Wildcard pattern - add as rule
                            self.add_module_rule(ModulePermissionRule {
                                pattern: key.to_string(),
                                permissions: perms,
                            });
                        } else {
                            // Exact match
                            self.set_module(key, perms);
                        }
                    } else {
                        return Err(format!("Invalid permission value for {}: {}", key, value));
                    }
                }
            }
        }

        Ok(())
    }

    /// Load permissions from a TOML file
    pub fn load_from_file(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        self.load_from_toml(&content)
    }
}

/// Check if code generation is allowed
pub fn check_code_generation(store: &PermissionStore) -> Result<(), VmError> {
    if !store.get_global().contains(ReflectionPermission::GENERATE_CODE) {
        return Err(VmError::RuntimeError(
            "Permission denied: bytecode generation is not permitted".to_string(),
        ));
    }
    Ok(())
}

/// Check if type creation is allowed
pub fn check_type_creation(store: &PermissionStore) -> Result<(), VmError> {
    if !store.get_global().contains(ReflectionPermission::CREATE_TYPES) {
        return Err(VmError::RuntimeError(
            "Permission denied: type creation is not permitted".to_string(),
        ));
    }
    Ok(())
}

/// Check field read permission
pub fn check_field_read(
    store: &PermissionStore,
    object_id: Option<usize>,
    class_id: Option<usize>,
    module_name: Option<&str>,
    is_private: bool,
) -> Result<(), VmError> {
    let required = if is_private {
        ReflectionPermission::READ_PRIVATE
    } else {
        ReflectionPermission::READ_PUBLIC
    };

    if !store.check_permission(object_id, class_id, module_name, required) {
        return Err(VmError::RuntimeError(format!(
            "Permission denied: cannot read {} fields",
            if is_private { "private" } else { "public" }
        )));
    }
    Ok(())
}

/// Check field write permission
pub fn check_field_write(
    store: &PermissionStore,
    object_id: Option<usize>,
    class_id: Option<usize>,
    module_name: Option<&str>,
    is_private: bool,
) -> Result<(), VmError> {
    let required = if is_private {
        ReflectionPermission::WRITE_PRIVATE
    } else {
        ReflectionPermission::WRITE_PUBLIC
    };

    if !store.check_permission(object_id, class_id, module_name, required) {
        return Err(VmError::RuntimeError(format!(
            "Permission denied: cannot write {} fields",
            if is_private { "private" } else { "public" }
        )));
    }
    Ok(())
}

/// Check method invocation permission
pub fn check_invoke(
    store: &PermissionStore,
    object_id: Option<usize>,
    class_id: Option<usize>,
    module_name: Option<&str>,
    is_private: bool,
) -> Result<(), VmError> {
    let required = if is_private {
        ReflectionPermission::INVOKE_PRIVATE
    } else {
        ReflectionPermission::INVOKE_PUBLIC
    };

    if !store.check_permission(object_id, class_id, module_name, required) {
        return Err(VmError::RuntimeError(format!(
            "Permission denied: cannot invoke {} methods",
            if is_private { "private" } else { "public" }
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_flags() {
        assert_eq!(ReflectionPermission::NONE.bits(), 0x00);
        assert_eq!(ReflectionPermission::ALL.bits(), 0xFF);
        assert_eq!(ReflectionPermission::READ_ALL.bits(), 0x03);
    }

    #[test]
    fn test_permission_contains() {
        assert!(ReflectionPermission::ALL.contains(ReflectionPermission::READ_PUBLIC));
        assert!(ReflectionPermission::ALL.contains(ReflectionPermission::GENERATE_CODE));
        assert!(!ReflectionPermission::PUBLIC_ONLY.contains(ReflectionPermission::READ_PRIVATE));
        assert!(ReflectionPermission::PUBLIC_ONLY.contains(ReflectionPermission::READ_PUBLIC));
    }

    #[test]
    fn test_permission_union() {
        let perms = ReflectionPermission::READ_PUBLIC.union(ReflectionPermission::WRITE_PUBLIC);
        assert!(perms.contains(ReflectionPermission::READ_PUBLIC));
        assert!(perms.contains(ReflectionPermission::WRITE_PUBLIC));
        assert!(!perms.contains(ReflectionPermission::READ_PRIVATE));
    }

    #[test]
    fn test_permission_from_str() {
        assert_eq!(ReflectionPermission::from_str("ALL"), Some(ReflectionPermission::ALL));
        assert_eq!(ReflectionPermission::from_str("all"), Some(ReflectionPermission::ALL));
        assert_eq!(ReflectionPermission::from_str("PUBLIC_ONLY"), Some(ReflectionPermission::PUBLIC_ONLY));
        assert_eq!(ReflectionPermission::from_str("0xFF"), Some(ReflectionPermission::ALL));
        assert_eq!(ReflectionPermission::from_str("255"), Some(ReflectionPermission::ALL));
    }

    #[test]
    fn test_permission_combined_str() {
        let perms = ReflectionPermission::from_combined_str("READ_PUBLIC|WRITE_PUBLIC").unwrap();
        assert!(perms.contains(ReflectionPermission::READ_PUBLIC));
        assert!(perms.contains(ReflectionPermission::WRITE_PUBLIC));
        assert!(!perms.contains(ReflectionPermission::READ_PRIVATE));
    }

    #[test]
    fn test_permission_store_global() {
        let mut store = PermissionStore::new();
        assert_eq!(store.get_global(), ReflectionPermission::ALL);

        store.set_global(ReflectionPermission::PUBLIC_ONLY);
        assert_eq!(store.get_global(), ReflectionPermission::PUBLIC_ONLY);
    }

    #[test]
    fn test_permission_store_object() {
        let mut store = PermissionStore::new();

        store.set_object(123, ReflectionPermission::READ_PUBLIC).unwrap();
        assert_eq!(store.get_object(123), Some(ReflectionPermission::READ_PUBLIC));
        assert_eq!(store.get_object(456), None);

        store.clear_object(123).unwrap();
        assert_eq!(store.get_object(123), None);
    }

    #[test]
    fn test_permission_store_class() {
        let mut store = PermissionStore::new();

        store.set_class(10, ReflectionPermission::FULL_ACCESS).unwrap();
        assert_eq!(store.get_class(10), Some(ReflectionPermission::FULL_ACCESS));
    }

    #[test]
    fn test_permission_store_module() {
        let mut store = PermissionStore::new();

        store.set_module("myapp", ReflectionPermission::ALL);
        assert_eq!(store.get_module("myapp"), Some(ReflectionPermission::ALL));
        assert_eq!(store.get_module("other"), None);
    }

    #[test]
    fn test_permission_store_module_rules() {
        let mut store = PermissionStore::new();

        store.add_module_rule(ModulePermissionRule {
            pattern: "plugins/*".to_string(),
            permissions: ReflectionPermission::PUBLIC_ONLY,
        });

        assert_eq!(store.get_module_resolved("plugins/foo"), Some(ReflectionPermission::PUBLIC_ONLY));
        assert_eq!(store.get_module_resolved("plugins/bar"), Some(ReflectionPermission::PUBLIC_ONLY));
        assert_eq!(store.get_module_resolved("plugins"), None); // Exact match doesn't work for prefix/*
        assert_eq!(store.get_module_resolved("other"), None);
    }

    #[test]
    fn test_permission_resolution() {
        let mut store = PermissionStore::new();
        store.set_global(ReflectionPermission::ALL);
        store.set_class(10, ReflectionPermission::FULL_ACCESS);
        store.set_object(123, ReflectionPermission::READ_PUBLIC).unwrap();

        // Object-level takes precedence
        assert_eq!(
            store.resolve(Some(123), Some(10), None),
            ReflectionPermission::READ_PUBLIC
        );

        // Class-level when no object permissions
        assert_eq!(
            store.resolve(Some(456), Some(10), None),
            ReflectionPermission::FULL_ACCESS
        );

        // Global when no object or class permissions
        assert_eq!(
            store.resolve(Some(456), Some(20), None),
            ReflectionPermission::ALL
        );
    }

    #[test]
    fn test_sealed_permissions() {
        let mut store = PermissionStore::new();

        store.set_object(123, ReflectionPermission::READ_PUBLIC).unwrap();
        store.seal_object(123);
        assert!(store.is_object_sealed(123));

        // Cannot modify sealed permissions
        let result = store.set_object(123, ReflectionPermission::ALL);
        assert!(result.is_err());

        let result = store.clear_object(123);
        assert!(result.is_err());
    }

    #[test]
    fn test_has_any_restrictions() {
        let mut store = PermissionStore::new();
        assert!(!store.has_any_restrictions());

        store.set_global(ReflectionPermission::PUBLIC_ONLY);
        assert!(store.has_any_restrictions());

        store.set_global(ReflectionPermission::ALL);
        assert!(!store.has_any_restrictions());

        store.set_object(123, ReflectionPermission::NONE).unwrap();
        assert!(store.has_any_restrictions());
    }

    #[test]
    fn test_load_from_toml() {
        let mut store = PermissionStore::new();

        let toml = r#"
[reflect.permissions]
global = "PUBLIC_ONLY"

[reflect.permissions.modules]
"myapp" = "ALL"
"plugins/*" = "READ_PUBLIC|INVOKE_PUBLIC"
"#;

        store.load_from_toml(toml).unwrap();

        assert_eq!(store.get_global(), ReflectionPermission::PUBLIC_ONLY);
        assert_eq!(store.get_module("myapp"), Some(ReflectionPermission::ALL));
        assert_eq!(
            store.get_module_resolved("plugins/foo"),
            Some(ReflectionPermission::from_bits(0x11)) // READ_PUBLIC | INVOKE_PUBLIC
        );
    }

    #[test]
    fn test_check_permission_helpers() {
        let mut store = PermissionStore::new();
        store.set_global(ReflectionPermission::PUBLIC_ONLY);

        assert!(check_field_read(&store, None, None, None, false).is_ok());
        assert!(check_field_read(&store, None, None, None, true).is_err());

        assert!(check_field_write(&store, None, None, None, false).is_ok());
        assert!(check_field_write(&store, None, None, None, true).is_err());

        assert!(check_invoke(&store, None, None, None, false).is_ok());
        assert!(check_invoke(&store, None, None, None, true).is_err());
    }

    #[test]
    fn test_check_code_generation() {
        let mut store = PermissionStore::new();

        // Default allows all
        assert!(check_code_generation(&store).is_ok());
        assert!(check_type_creation(&store).is_ok());

        // Restrict code generation
        store.set_global(ReflectionPermission::FULL_ACCESS);
        assert!(check_code_generation(&store).is_err());
        assert!(check_type_creation(&store).is_err());

        // Allow type creation but not code gen
        store.set_global(ReflectionPermission::FULL_ACCESS.union(ReflectionPermission::CREATE_TYPES));
        assert!(check_code_generation(&store).is_err());
        assert!(check_type_creation(&store).is_ok());
    }

    #[test]
    fn test_module_pattern_matching() {
        let rule = ModulePermissionRule {
            pattern: "plugins/*".to_string(),
            permissions: ReflectionPermission::PUBLIC_ONLY,
        };

        assert!(rule.matches("plugins/foo"));
        assert!(rule.matches("plugins/bar/baz")); // Also matches
        assert!(!rule.matches("plugins"));
        assert!(!rule.matches("other"));

        let rule2 = ModulePermissionRule {
            pattern: "**".to_string(),
            permissions: ReflectionPermission::NONE,
        };

        assert!(rule2.matches("anything"));
        assert!(rule2.matches("deeply/nested/module"));
    }
}
