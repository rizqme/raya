# raya-pm

Package manager library for Raya. This is a library crate used by the main `raya` CLI.

## Overview

This crate provides package management functionality:
- Module caching (content-addressable storage)
- Package manifest parsing (`raya.toml`)
- Lockfile management (`raya.lock`)
- Semver version parsing and constraint matching
- Dependency resolution
- Local path dependency resolution
- Registry client (HTTP API for raya.dev)
- URL imports (direct HTTP/HTTPS fetching and caching)
- Command implementations (init, install, add, remove)

## Usage

The raya-pm library is integrated into the main `raya` CLI:

```bash
# Initialize a new project
raya init [path] [--name NAME]
raya new <name>

# Install dependencies from raya.toml
raya install [--production] [--force] [--update]

# Add a dependency
raya add <package>[@version] [--dev] [--exact] [--no-install]

# Remove a dependency
raya remove <package>

# Update dependencies
raya update [package]
```

## Module Structure

```
src/
├── lib.rs       # Crate entry point, re-exports
├── cache/       # Content-addressable module cache
│   ├── mod.rs
│   └── metadata.rs
├── commands/    # Command implementations (used by raya-cli)
│   ├── mod.rs
│   ├── init.rs     # init/new
│   ├── install.rs  # install/update
│   └── add.rs      # add/remove
├── registry/    # Registry client
│   ├── mod.rs
│   ├── api.rs      # API response types
│   └── client.rs   # HTTP client
├── url/         # URL import handling
│   ├── mod.rs      # Module entry point
│   ├── fetch.rs    # HTTP fetching with checksum
│   └── cache.rs    # URL caching and extraction
├── manifest.rs  # raya.toml parsing
├── lockfile.rs  # raya.lock parsing
├── semver.rs    # Version parsing and constraints
├── resolver.rs  # Dependency resolution algorithm
└── path.rs      # Path resolution utilities
```

## Key Types

```rust
// Package manifest (raya.toml)
PackageManifest {
    package: PackageInfo,
    scripts: HashMap<String, String>,
    dependencies: HashMap<String, Dependency>,
    dev_dependencies: HashMap<String, Dependency>,
    registry: Option<RegistryConfig>,
}

// Lockfile (raya.lock)
Lockfile {
    packages: Vec<LockedPackage>,
}

// Version constraint
Constraint::parse("^1.2.0") -> Constraint

// Dependency resolver
DependencyResolver::resolve(&manifest) -> ResolvedDependencies

// Registry client
RegistryClient::new()?.get_package("logging") -> PackageMetadata
RegistryClient::download_to_cache("pkg", "1.0.0", cache_root) -> PathBuf

// URL imports
UrlFetcher::new().fetch("https://...") -> FetchResult
UrlCache::new(cache_root).fetch_and_cache("https://...") -> (CachedUrl, LockedPackage)
```

## Cache Structure

```
~/.raya/cache/
├── <sha256>/               # Content-addressed storage
│   ├── module.ryb          # Compiled bytecode (required)
│   ├── module.d.raya       # Type definitions with doc comments (required)
│   ├── raya.toml           # Package manifest (required)
│   └── README.md           # Package documentation (optional)
├── tmp/                    # Temporary download directory
└── registry/               # Registry metadata cache
```

## Manifest Format (raya.toml)

```toml
[package]
name = "my-app"
version = "1.0.0"
description = "My Raya application"

[scripts]
dev = "src/main.raya --watch"
build = "raya build --release"

[dependencies]
logging = "^1.2.0"
utils = { path = "../utils" }

[dev-dependencies]
testing = "^2.0.0"

[registry]
url = "https://registry.raya.dev"
```

## Lockfile Format (raya.lock)

```toml
version = 1
root = "my-app"

[[packages]]
name = "logging"
version = "1.2.3"
checksum = "abc123...64hex..."
source = { type = "registry" }

[[packages]]
name = "utils"
version = "0.0.0"
checksum = "def456...64hex..."
source = { type = "path", path = "../utils" }

[[packages]]
name = "remote-lib"
version = "1.0.0"
checksum = "789abc...64hex..."
source = { type = "url", url = "https://example.com/lib.tar.gz" }
```

## Registry API

```
GET /packages/{name}           → PackageMetadata
GET /packages/{name}/{version} → VersionInfo + download URL
```

**Response Types:**
- `PackageMetadata`: name, description, versions[], keywords[], maintainers[]
- `VersionInfo`: name, version, checksum, download.url, dependencies{}

## Implementation Status

| Feature | Status |
|---------|--------|
| Manifest parsing | ✅ Complete |
| Lockfile parsing | ✅ Complete |
| Semver parsing | ✅ Complete |
| Local path resolution | ✅ Complete |
| Module cache | ✅ Complete |
| Registry client | ✅ Complete |
| Dependency resolution | ✅ Complete |
| init command | ✅ Complete |
| install command | ✅ Complete |
| add command | ✅ Complete |
| remove command | ✅ Complete |

## For AI Assistants

- This is a **library** crate, not a binary
- Commands are exposed via `raya_pm::commands::{init, install, add}`
- The main `raya` CLI (raya-cli) uses this library
- Version constraint matching uses standard semver rules
- Cache uses SHA-256 for content addressing
- Local path dependencies bypass the registry (compiled from source)
- Registry client uses `reqwest` with blocking API
- Package archives are `.tar.gz` format
- Resolver handles diamond dependencies with conflict detection
- Lockfile checksums must be 64 hex characters (SHA-256)
- Scoped packages (@org/name) are supported
