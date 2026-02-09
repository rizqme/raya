# Reflection Security & Permissions

## Overview

This document describes the security model for Raya's Reflection API, enabling fine-grained control over what reflection operations are permitted on objects, classes, and modules.

## Design Goals

1. **Opt-in Security**: By default, reflection is unrestricted (ALL permissions). Security must be explicitly enabled.
2. **Granular Control**: Different permission levels for read, write, invoke, and code generation.
3. **Zero Overhead When Disabled**: No permission checks when security is not configured.
4. **Defense in Depth**: Multiple layers of restriction (object, class, module).

## Permission Flags

```typescript
enum ReflectionPermission {
    NONE            = 0x00,   // No reflection allowed
    READ_PUBLIC     = 0x01,   // Read public fields
    READ_PRIVATE    = 0x02,   // Read private fields
    WRITE_PUBLIC    = 0x04,   // Write public fields
    WRITE_PRIVATE   = 0x08,   // Write private fields
    INVOKE_PUBLIC   = 0x10,   // Invoke public methods
    INVOKE_PRIVATE  = 0x20,   // Invoke private methods
    CREATE_TYPES    = 0x40,   // Create classes/subclasses dynamically
    GENERATE_CODE   = 0x80,   // Use BytecodeBuilder, createFunction

    // Common combinations
    READ_ALL        = 0x03,   // READ_PUBLIC | READ_PRIVATE
    WRITE_ALL       = 0x0C,   // WRITE_PUBLIC | WRITE_PRIVATE
    INVOKE_ALL      = 0x30,   // INVOKE_PUBLIC | INVOKE_PRIVATE
    PUBLIC_ONLY     = 0x15,   // READ_PUBLIC | WRITE_PUBLIC | INVOKE_PUBLIC
    FULL_ACCESS     = 0x3F,   // All read/write/invoke
    ALL             = 0xFF,   // Everything including code generation
}
```

## Permission Hierarchy

Permissions are checked at multiple levels, with the most restrictive winning:

```
Global Default (configurable, default: ALL)
    └── Module Level (per-module restrictions)
        └── Class Level (per-class restrictions)
            └── Object Level (per-instance restrictions)
```

### Resolution Order

1. Check object-level permissions (if set)
2. Check class-level permissions (if set)
3. Check module-level permissions (if set)
4. Fall back to global default

```rust
fn resolve_permissions(target: Value) -> ReflectionPermission {
    // Object-level check
    if let Some(perms) = get_object_permissions(target) {
        return perms;
    }

    // Class-level check
    if let Some(class_id) = get_class_id(target) {
        if let Some(perms) = get_class_permissions(class_id) {
            return perms;
        }
    }

    // Module-level check
    if let Some(module_id) = get_module_id(target) {
        if let Some(perms) = get_module_permissions(module_id) {
            return perms;
        }
    }

    // Global default
    get_global_permissions()
}
```

## Storage Design

### PermissionStore

```rust
pub struct PermissionStore {
    /// Global default permissions
    global_default: ReflectionPermission,

    /// Object-level permissions: object_id -> permissions
    object_permissions: HashMap<usize, ReflectionPermission>,

    /// Class-level permissions: class_id -> permissions
    class_permissions: HashMap<usize, ReflectionPermission>,

    /// Module-level permissions: module_name -> permissions
    module_permissions: HashMap<String, ReflectionPermission>,
}
```

### Memory Considerations

- Object-level permissions use object identity (pointer/ID)
- Permissions are NOT stored on the object itself (no memory overhead for objects without restrictions)
- HashMap lookup is O(1) amortized
- Empty store means zero overhead (no lookups needed when global = ALL)

## API Design

### Setting Permissions

```typescript
// Set permissions on an object instance
Reflect.setPermissions(myObject, ReflectionPermission.READ_PUBLIC);

// Set permissions on a class (affects all instances)
Reflect.setClassPermissions(MyClass, ReflectionPermission.PUBLIC_ONLY);

// Set permissions on a module
Reflect.setModulePermissions("myModule", ReflectionPermission.FULL_ACCESS);

// Set global default
Reflect.setGlobalPermissions(ReflectionPermission.ALL);
```

### Querying Permissions

```typescript
// Get effective permissions for a target
const perms = Reflect.getPermissions(myObject);

// Get class-level permissions (not resolved)
const classPerms = Reflect.getClassPermissions(MyClass);

// Check specific permission
const canRead = Reflect.hasPermission(myObject, ReflectionPermission.READ_PRIVATE);
```

### Clearing Permissions

```typescript
// Clear object-level permissions (falls back to class/module/global)
Reflect.clearPermissions(myObject);

// Clear class-level permissions
Reflect.clearClassPermissions(MyClass);
```

## Native Call IDs

Phase 16 uses the 0x0E00-0x0E0F range:

| ID     | Method                    | Description                          |
|--------|---------------------------|--------------------------------------|
| 0x0E00 | setPermissions            | Set object-level permissions         |
| 0x0E01 | getPermissions            | Get resolved permissions             |
| 0x0E02 | hasPermission             | Check specific permission flag       |
| 0x0E03 | clearPermissions          | Clear object-level permissions       |
| 0x0E04 | setClassPermissions       | Set class-level permissions          |
| 0x0E05 | getClassPermissions       | Get class-level permissions          |
| 0x0E06 | clearClassPermissions     | Clear class-level permissions        |
| 0x0E07 | setModulePermissions      | Set module-level permissions         |
| 0x0E08 | getModulePermissions      | Get module-level permissions         |
| 0x0E09 | clearModulePermissions    | Clear module-level permissions       |
| 0x0E0A | setGlobalPermissions      | Set global default                   |
| 0x0E0B | getGlobalPermissions      | Get global default                   |

## Integration with Existing API

### Field Access (Phase 3)

```rust
fn handle_get_field(target: Value, field_name: &str, is_private: bool) -> Result<Value, VmError> {
    let perms = resolve_permissions(target);

    let required = if is_private {
        ReflectionPermission::READ_PRIVATE
    } else {
        ReflectionPermission::READ_PUBLIC
    };

    if !perms.contains(required) {
        return Err(VmError::PermissionDenied(format!(
            "Cannot read {} field '{}'",
            if is_private { "private" } else { "public" },
            field_name
        )));
    }

    // Proceed with field access...
}
```

### Method Invocation (Phase 4)

```rust
fn handle_invoke(target: Value, method_name: &str, is_private: bool) -> Result<Value, VmError> {
    let perms = resolve_permissions(target);

    let required = if is_private {
        ReflectionPermission::INVOKE_PRIVATE
    } else {
        ReflectionPermission::INVOKE_PUBLIC
    };

    if !perms.contains(required) {
        return Err(VmError::PermissionDenied(...));
    }

    // Proceed with invocation...
}
```

### Type Creation (Phase 10, 14)

```rust
fn handle_create_subclass(parent_class_id: usize) -> Result<usize, VmError> {
    // Check global/module permissions for type creation
    let perms = get_global_permissions();

    if !perms.contains(ReflectionPermission::CREATE_TYPES) {
        return Err(VmError::PermissionDenied(
            "Type creation is not permitted".to_string()
        ));
    }

    // Proceed with class creation...
}
```

### Bytecode Generation (Phase 15)

```rust
fn handle_new_bytecode_builder() -> Result<usize, VmError> {
    let perms = get_global_permissions();

    if !perms.contains(ReflectionPermission::GENERATE_CODE) {
        return Err(VmError::PermissionDenied(
            "Bytecode generation is not permitted".to_string()
        ));
    }

    // Proceed with builder creation...
}
```

## Security Considerations

### Immutable Permissions

Once an object is "sealed" with `Reflect.sealPermissions(target)`, its permissions cannot be changed. This prevents escalation attacks.

```typescript
Reflect.setPermissions(secretData, ReflectionPermission.NONE);
Reflect.sealPermissions(secretData);  // Now immutable

// This will throw an error:
Reflect.setPermissions(secretData, ReflectionPermission.ALL);
```

### Permission Inheritance

New objects inherit class-level permissions by default. Subclasses can have stricter (but not looser) permissions than their parent.

### Trusted Code

Code running with `GENERATE_CODE` permission can create arbitrary bytecode. This should only be granted to trusted code (e.g., a template engine, not user input).

### Module Isolation

Modules can be isolated by setting `ReflectionPermission.NONE` on them, preventing any reflection into that module's types.

## Performance Optimization

### Fast Path

When global permissions are `ALL` and no object/class/module permissions are set, skip all permission checks entirely:

```rust
fn should_check_permissions() -> bool {
    let store = PERMISSION_STORE.lock();
    store.global_default != ReflectionPermission::ALL ||
    !store.object_permissions.is_empty() ||
    !store.class_permissions.is_empty() ||
    !store.module_permissions.is_empty()
}

// In hot path:
if should_check_permissions() {
    check_and_enforce_permissions(target, required)?;
}
```

### Caching

For frequently accessed objects, cache the resolved permissions:

```rust
// Per-task permission cache (cleared on permission changes)
thread_local! {
    static PERM_CACHE: RefCell<HashMap<usize, ReflectionPermission>> = RefCell::new(HashMap::new());
}
```

## Example Usage

### Sandboxed Plugin System

```typescript
// Plugin module has restricted permissions
Reflect.setModulePermissions("plugins/untrusted",
    ReflectionPermission.READ_PUBLIC |
    ReflectionPermission.INVOKE_PUBLIC
);

// Plugin cannot:
// - Read private fields
// - Write any fields
// - Create new types
// - Generate bytecode
```

### Sensitive Data Protection

```typescript
class UserCredentials {
    private passwordHash: string;

    constructor(hash: string) {
        this.passwordHash = hash;
        // Lock down this instance
        Reflect.setPermissions(this, ReflectionPermission.NONE);
        Reflect.sealPermissions(this);
    }
}

// Even with reflection, cannot access passwordHash
const creds = new UserCredentials("...");
Reflect.get(creds, "passwordHash");  // throws PermissionDenied
```

### Development vs Production

```typescript
if (process.env.NODE_ENV === "production") {
    // Disable dynamic code generation in production
    Reflect.setGlobalPermissions(
        ReflectionPermission.ALL & ~ReflectionPermission.GENERATE_CODE
    );
}
```

## Implementation Checklist

- [ ] Define `ReflectionPermission` bitflags
- [ ] Implement `PermissionStore` struct
- [ ] Add `PERMISSION_STORE` lazy static
- [ ] Implement native handlers (0x0E00-0x0E0B)
- [ ] Integrate permission checks into existing handlers:
  - [ ] Field access (GET, SET, GET_FIELD_INFO)
  - [ ] Method invocation (INVOKE, INVOKE_ASYNC)
  - [ ] Type creation (CREATE_SUBCLASS, DEFINE_CLASS, newClassBuilder)
  - [ ] Bytecode generation (newBytecodeBuilder, createFunction)
- [ ] Add `sealPermissions` for immutable restrictions
- [ ] Add fast-path optimization
- [ ] Add unit tests for all permission scenarios
- [ ] Add integration tests with existing Reflect API
