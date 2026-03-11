# raya-pm

This crate owns package-management primitives. It does not run installs by itself, but it defines the data models and helpers that runtime and CLI package flows depend on.

## What This Crate Owns

- `raya.toml` parsing.
- `raya.lock` parsing and persistence.
- Semver versions and constraints.
- Project-root and local path resolution.
- Package/cache metadata.
- URL import cache support.

## Layout

- `src/manifest.rs`: manifest types and parsing.
- `src/lockfile.rs`: lockfile types and persistence.
- `src/path.rs`: project-root and path resolution helpers.
- `src/semver.rs`: version and constraint logic.
- `src/cache/`: package/module cache metadata helpers.
- `src/url/`: URL caching helpers.

## Start Here When

- Manifest fields need to change.
- Lockfile semantics are wrong.
- Version resolution or constraint checking is wrong.
- Package or URL caches behave incorrectly.

## Read Next

- Runtime dependency loading: [`../raya-runtime/CLAUDE.md`](../raya-runtime/CLAUDE.md)
- CLI package commands: [`../raya-cli/CLAUDE.md`](../raya-cli/CLAUDE.md)
