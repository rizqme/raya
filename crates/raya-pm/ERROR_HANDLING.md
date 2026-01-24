# Error Handling Guide for raya-pm

This document describes the error handling patterns and best practices used in the Raya Package Manager (rpkg) crate.

## Error Types

The rpkg crate defines several specialized error types using the `thiserror` crate for consistent error handling:

### 1. CacheError

**File:** `src/cache/mod.rs`

Errors that occur during cache operations:

- `IoError(std::io::Error)` - File system operations failed
- `CacheInitError(String)` - Failed to initialize cache directory
- `ModuleNotFound(String)` - Module not found in cache
- `ChecksumMismatch { expected, actual }` - Module checksum verification failed
- `MetadataError(String)` - Metadata operations failed
- `InvalidHash(String)` - Invalid hash format provided

**Example:**
```rust
let cache = Cache::init()?;
match cache.retrieve(&hash) {
    Ok(data) => println!("Retrieved {} bytes", data.len()),
    Err(CacheError::ModuleNotFound(h)) => eprintln!("Module {} not in cache", h),
    Err(CacheError::ChecksumMismatch { expected, actual }) => {
        eprintln!("Checksum mismatch! Expected: {}, Got: {}", expected, actual);
    }
    Err(e) => eprintln!("Cache error: {}", e),
}
```

### 2. ManifestError

**File:** `src/manifest.rs`

Errors that occur during manifest (raya.toml) parsing:

- `IoError(std::io::Error)` - Failed to read manifest file
- `ParseError(toml::de::Error)` - TOML parsing failed
- `ValidationError(String)` - Manifest validation failed (invalid name, version, etc.)
- `MissingField(String)` - Required field is missing

**Example:**
```rust
match PackageManifest::from_file(&path) {
    Ok(manifest) => println!("Loaded package: {}", manifest.package.name),
    Err(ManifestError::ValidationError(msg)) => {
        eprintln!("Invalid manifest: {}", msg);
    }
    Err(e) => eprintln!("Failed to load manifest: {}", e),
}
```

### 3. LockfileError

**File:** `src/lockfile.rs`

Errors that occur during lockfile (raya.lock) operations:

- `IoError(std::io::Error)` - Failed to read/write lockfile
- `ParseError(toml::de::Error)` - TOML parsing failed
- `ValidationError(String)` - Lockfile validation failed
- `VersionMismatch { expected, actual }` - Lockfile version incompatible

**Example:**
```rust
match Lockfile::from_file(&lock_path) {
    Ok(lockfile) => println!("Loaded {} packages", lockfile.packages.len()),
    Err(LockfileError::ValidationError(msg)) => {
        eprintln!("Invalid lockfile: {}", msg);
        // Regenerate lockfile
    }
    Err(e) => eprintln!("Failed to load lockfile: {}", e),
}
```

### 4. SemverError

**File:** `src/semver.rs`

Errors that occur during semantic version parsing:

- `InvalidVersion(String)` - Version string is malformed
- `InvalidConstraint(String)` - Constraint string is malformed

**Example:**
```rust
match Version::parse(version_str) {
    Ok(version) => println!("Version: {}", version),
    Err(SemverError::InvalidVersion(msg)) => {
        eprintln!("Invalid version format: {}", msg);
    }
}

match Constraint::parse(constraint_str) {
    Ok(constraint) => println!("Constraint: {}", constraint),
    Err(SemverError::InvalidConstraint(msg)) => {
        eprintln!("Invalid constraint format: {}", msg);
    }
}
```

### 5. PathError

**File:** `src/path.rs`

Errors that occur during path resolution:

- `PathNotFound(PathBuf)` - Path does not exist
- `NotADirectory(PathBuf)` - Path exists but is not a directory
- `MissingManifest(PathBuf)` - Directory exists but has no raya.toml
- `PathTraversal(PathBuf)` - Path is outside project bounds (security check)
- `InvalidPath(String)` - Path is invalid for other reasons

**Example:**
```rust
let resolver = PathResolver::new(project_root);
match resolver.resolve(path, manifest_dir) {
    Ok(resolved) => println!("Resolved to: {:?}", resolved),
    Err(PathError::PathNotFound(p)) => {
        eprintln!("Path not found: {:?}", p);
    }
    Err(PathError::MissingManifest(p)) => {
        eprintln!("Directory {:?} has no raya.toml", p);
    }
    Err(PathError::PathTraversal(p)) => {
        eprintln!("Security: Path {:?} is outside project", p);
    }
    Err(e) => eprintln!("Path error: {}", e),
}
```

### 6. ResolverError

**File:** `src/resolver.rs`

Errors that occur during dependency resolution:

- `PackageNotFound(String)` - Package not available
- `NoMatchingVersion { package, constraint }` - No version satisfies constraints
- `CircularDependency(Vec<String>)` - Circular dependency detected
- `ManifestError(ManifestError)` - Manifest parsing failed
- `SemverError(SemverError)` - Version parsing failed
- `PathError(PathError)` - Path resolution failed

**Example:**
```rust
match resolver.resolve() {
    Ok(resolved) => {
        println!("Resolved {} packages", resolved.packages.len());
    }
    Err(ResolverError::NoMatchingVersion { package, constraint }) => {
        eprintln!("No version of {} matches constraint {}", package, constraint);
    }
    Err(ResolverError::CircularDependency(cycle)) => {
        eprintln!("Circular dependency detected: {}", cycle.join(" -> "));
    }
    Err(ResolverError::PackageNotFound(pkg)) => {
        eprintln!("Package {} not found", pkg);
    }
    Err(e) => eprintln!("Resolution failed: {}", e),
}
```

### 7. MetadataError

**File:** `src/cache/metadata.rs`

Errors that occur during metadata operations:

- `IoError(std::io::Error)` - File operations failed
- `SerializationError(serde_json::Error)` - JSON serialization failed
- `ValidationError(String)` - Metadata validation failed

**Example:**
```rust
match ModuleMetadata::load(&metadata_path) {
    Ok(metadata) => println!("Module: {} v{}", metadata.name, metadata.version),
    Err(MetadataError::ValidationError(msg)) => {
        eprintln!("Invalid metadata: {}", msg);
    }
    Err(e) => eprintln!("Failed to load metadata: {}", e),
}
```

## Error Handling Patterns

### 1. Propagate with `?` Operator

For library code, prefer propagating errors to the caller:

```rust
pub fn process_manifest(path: &Path) -> Result<ResolvedDependencies, ResolverError> {
    let manifest = PackageManifest::from_file(path)?;
    let resolver = DependencyResolver::new(manifest);
    resolver.resolve()
}
```

### 2. Match on Specific Error Variants

When different errors require different handling:

```rust
match cache.retrieve(&hash) {
    Ok(data) => Ok(data),
    Err(CacheError::ModuleNotFound(_)) => {
        // Fetch from registry and store in cache
        fetch_and_cache(&hash)
    }
    Err(CacheError::ChecksumMismatch { .. }) => {
        // Corrupt cache entry, re-download
        invalidate_and_refetch(&hash)
    }
    Err(e) => Err(e),
}
```

### 3. Provide Context with `map_err`

Add context to errors:

```rust
manifest.validate()
    .map_err(|e| ResolverError::ManifestError(e))?;
```

### 4. Early Return for Invalid State

Validate inputs early:

```rust
pub fn resolve(&self) -> Result<ResolvedDependencies, ResolverError> {
    // Validate manifest first
    self.manifest.validate()?;

    // Then proceed with resolution
    // ...
}
```

## Error Recovery Strategies

### 1. Cache Operations

**Problem:** Module not found in cache
**Recovery:** Fetch from registry and store

**Problem:** Checksum mismatch
**Recovery:** Delete corrupt entry and re-download

### 2. Lockfile Operations

**Problem:** Lockfile version mismatch
**Recovery:** Regenerate lockfile from manifest

**Problem:** Lockfile validation failed
**Recovery:** Delete and regenerate lockfile

### 3. Dependency Resolution

**Problem:** No matching version found
**Recovery:** Suggest closest available versions to user

**Problem:** Circular dependency detected
**Recovery:** Report the cycle clearly for user to fix

### 4. Path Resolution

**Problem:** Path not found
**Recovery:** Check if path is relative vs absolute, suggest corrections

**Problem:** Missing manifest
**Recovery:** Suggest running `raya init` in that directory

## Testing Error Scenarios

All error types are thoroughly tested. See integration tests:

- `tests/end_to_end_tests.rs` - End-to-end error scenarios
- `tests/cache_tests.rs` - Cache error handling
- `tests/manifest_tests.rs` - Manifest parsing errors
- `tests/lockfile_tests.rs` - Lockfile validation errors
- `tests/semver_tests.rs` - Version parsing errors
- `tests/resolver_tests.rs` - Resolution errors
- `tests/path_tests.rs` - Path resolution errors

## Best Practices

1. **Use thiserror for all error types** - Consistent Display implementations
2. **Include context in error messages** - Help users understand what went wrong
3. **Validate early** - Catch errors before expensive operations
4. **Provide recovery paths** - When possible, suggest how to fix the issue
5. **Test all error paths** - Ensure error handling works correctly
6. **Use appropriate error granularity** - Not too broad, not too specific
7. **Document error conditions** - In function docs, note when errors can occur

## Example: Complete Error Handling Flow

```rust
use rpkg::{Cache, DependencyResolver, Lockfile, PackageManifest};
use std::path::Path;

pub fn install_dependencies(project_root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load manifest (may fail with ManifestError)
    let manifest_path = project_root.join("raya.toml");
    let manifest = PackageManifest::from_file(&manifest_path)
        .map_err(|e| format!("Failed to load manifest: {}", e))?;

    // 2. Check for existing lockfile
    let lock_path = project_root.join("raya.lock");
    let existing_lockfile = if lock_path.exists() {
        Some(Lockfile::from_file(&lock_path).ok()) // Ignore errors, regenerate if invalid
    } else {
        None
    };

    // 3. Resolve dependencies (may fail with ResolverError)
    let resolver = DependencyResolver::new(manifest)
        .with_lockfile(existing_lockfile);

    let resolved = resolver.resolve()
        .map_err(|e| format!("Failed to resolve dependencies: {}", e))?;

    // 4. Initialize cache (may fail with CacheError)
    let cache = Cache::init()
        .map_err(|e| format!("Failed to initialize cache: {}", e))?;

    // 5. Download and cache modules
    for (name, package) in &resolved.packages {
        match cache.retrieve(&package.checksum()?) {
            Ok(_) => {
                println!("✓ {} v{} (cached)", name, package.version);
            }
            Err(_) => {
                println!("⬇ Downloading {} v{}...", name, package.version);
                // Download from registry...
                // cache.store(&module_data)?;
            }
        }
    }

    // 6. Save lockfile
    let lockfile = resolved.to_lockfile(Some(manifest.package.name.clone()));
    lockfile.to_file(&lock_path)
        .map_err(|e| format!("Failed to save lockfile: {}", e))?;

    println!("✓ Successfully installed {} packages", resolved.packages.len());
    Ok(())
}
```

## Error Message Guidelines

Good error messages should:

1. **Be specific:** "Package 'logging' not found" not "Error resolving dependencies"
2. **Include context:** "No version of 'http' matches constraint '^3.0.0' (available: 1.0.0, 2.0.0)"
3. **Suggest solutions:** "Directory has no raya.toml. Run 'raya init' to create one."
4. **Use consistent formatting:** Follow the patterns established in error type definitions

## See Also

- [Rust Error Handling Book](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [thiserror documentation](https://docs.rs/thiserror/)
- Individual module documentation for detailed error conditions
