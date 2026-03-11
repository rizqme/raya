# raya-lsp

This crate is reserved for the language-server implementation. Today it is mostly a placeholder, but this is still the right place for editor/LSP protocol logic rather than putting that behavior into the CLI or engine crates.

## Layout

- `src/lib.rs`: current placeholder entrypoint.

## Start Here When

- Implementing LSP features, requests, or server startup.
- Adding types shared with the CLI `lsp` command.
