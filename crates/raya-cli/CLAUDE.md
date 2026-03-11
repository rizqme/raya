# raya-cli

This crate is the command-line face of the toolchain. It does not own the language semantics; it owns how users invoke them.

## What This Crate Owns

- Clap command definitions.
- Top-level flags and aliases.
- Command dispatch into runtime, package, and future LSP functionality.
- Human-facing output and command UX.

## Layout

- `src/main.rs`: root clap tree, top-level flags, implicit file execution behavior.
- `src/output.rs`: shared CLI output helpers.
- `src/commands/`: individual command handlers.
- `src/commands/pkg/`: package-manager-specific command handlers.

## Start Here When

- You need to add or rename a CLI flag or subcommand.
- Command behavior should call into runtime/package code differently.
- Shell-facing UX or output needs improvement.

## Read Next

- Runtime execution and compile/load behavior: [`../raya-runtime/CLAUDE.md`](../raya-runtime/CLAUDE.md)
- Manifest/package logic: [`../raya-pm/CLAUDE.md`](../raya-pm/CLAUDE.md)
- Placeholder LSP support: [`../raya-lsp/CLAUDE.md`](../raya-lsp/CLAUDE.md)
