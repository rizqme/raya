# Package Manager

Raya's integrated package manager handles dependencies, manifests, and registry operations.

## Commands

All PM commands are under `raya pkg` namespace, with common commands aliased at top-level.

### raya init / raya pkg init

Initialize a new project.

```bash
raya init
raya init -n           # Node/package.json mode
raya init --npm        # Raya TOML project with npm registry mode
raya pkg init
```

Creates `raya.toml`:

```toml
[package]
name = "my-project"
version = "0.1.0"
edition = "2021"

[dependencies]
```

### raya install / raya pkg install

Install all dependencies.

```bash
raya install
raya i  # Short alias
raya install --frozen
raya install --ignore-scripts
raya pkg install
```

Reads `raya.toml`, resolves dependencies, downloads packages.

### raya add / raya pkg add

Add a dependency.

```bash
# Registry package
raya add package-name

# Version constraint
raya add package-name@1.2.3
raya add package-name@^1.0.0

# Local path
raya add --path ../my-lib

# Git URL
raya add --git https://github.com/user/repo.git
```

Updates `raya.toml`:

```toml
[dependencies]
package-name = "1.2.3"
my-lib = { path = "../my-lib" }
repo = { git = "https://github.com/user/repo.git" }
```

### raya remove / raya pkg remove

Remove a dependency.

```bash
raya remove package-name
raya rm package-name  # Short alias
raya pkg remove package-name
```

### raya update / raya pkg update

Update dependencies to latest compatible versions.

```bash
# Update all
raya update
raya pkg update

# Update specific package
raya update package-name
```

### raya publish / raya pkg publish

Publish package to registry (stub).

```bash
raya publish
raya pkg publish
```

### raya upgrade / raya pkg upgrade

Upgrade Raya installation (stub).

```bash
raya upgrade
raya pkg upgrade
```

## Registry Operations

### raya pkg login

Authenticate with registry.

```bash
raya pkg login

# Specify registry URL
raya pkg login --registry https://registry.example.com
```

Credentials saved to `~/.raya/credentials.toml`.

### raya pkg logout

Remove credentials.

```bash
raya pkg logout
```

### raya pkg set-url

Set registry URL.

```bash
# Project-level (raya.toml)
raya pkg set-url https://registry.example.com

# Global (~/.raya/config.toml)
raya pkg set-url --global https://registry.example.com
```

### raya pkg whoami

Show current authenticated user.

```bash
raya pkg whoami
```

### raya pkg info

Show package information (stub).

```bash
raya pkg info package-name
```

## raya.toml Manifest

### Basic Structure

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2021"
main = "src/main.raya"  # Entry point (optional)

[dependencies]
logger = "1.0.0"
some-lib = { path = "../lib" }
remote-pkg = { git = "https://github.com/user/repo.git" }

[scripts]
dev = "src/main.raya --debug"
test = "src/test.raya"
build = "build.raya"

[registry]
url = "https://registry.example.com"  # Project-specific registry
```

### Dependency Types

**Registry (version):**
```toml
[dependencies]
package = "1.2.3"
package2 = "^1.0.0"  # Semver range
```

**Local path:**
```toml
[dependencies]
my-lib = { path = "../my-lib" }
```

**Git URL:**
```toml
[dependencies]
repo = { git = "https://github.com/user/repo.git" }
repo-branch = { git = "https://github.com/user/repo.git", branch = "develop" }
repo-tag = { git = "https://github.com/user/repo.git", tag = "v1.0.0" }
```

### Scripts

Define named scripts:

```toml
[scripts]
dev = "src/main.raya --watch"
test = "src/test.raya"
build = "build.raya"
```

Run with:
```bash
raya run dev
raya run test
```

## Dependency Resolution

1. Read `raya.toml`
2. Resolve dependency types:
   - **Local path**: Canonicalize and load
   - **URL/git**: Check cache, download if needed
   - **Registry**: Check local packages, download if needed
3. Recursively resolve transitive dependencies
4. Load all modules
5. Link and execute

### Cache Locations

- **URL cache**: `~/.raya/cache/urls/`
- **Registry packages**: `~/.raya/packages/`
- **Local project packages**: `./raya_packages/`
- **Credentials**: `~/.raya/credentials.toml`
- **Global config**: `~/.raya/config.toml`

## Environment Variables

- `RAYA_REGISTRY` - Override registry URL
- `RAYA_HOME` - Override `~/.raya` directory

## Related

- [Commands](commands.md) - CLI commands
- [REPL](repl.md) - Interactive shell
