# raya-lsp

Language Server Protocol (LSP) implementation for Raya.

## Overview

This crate provides IDE support for Raya through the Language Server Protocol, enabling features like:
- Syntax highlighting
- Code completion
- Go to definition
- Find references
- Hover information
- Diagnostics (errors/warnings)
- Code actions (quick fixes)

## Implementation Status

**Status: Placeholder**

The crate is currently empty, awaiting implementation.

## Planned Features

### Phase 1: Basic Support
- [ ] Syntax error reporting
- [ ] Type error diagnostics
- [ ] Go to definition
- [ ] Hover for type information

### Phase 2: Enhanced Features
- [ ] Code completion
- [ ] Find all references
- [ ] Rename symbol
- [ ] Document symbols

### Phase 3: Advanced Features
- [ ] Code actions (quick fixes)
- [ ] Code lens
- [ ] Semantic highlighting
- [ ] Formatting integration

## Dependencies

- `raya-engine`: For parsing and type checking
- `tower-lsp`: LSP protocol implementation
- `tokio`: Async runtime

## Architecture

```
raya-lsp
├── server.rs      # LSP server setup
├── handlers.rs    # Request/notification handlers
├── document.rs    # Document management
├── completion.rs  # Completion provider
├── diagnostics.rs # Error reporting
└── hover.rs       # Hover information
```

## For AI Assistants

- This crate is currently a placeholder
- Implementation should use `tower-lsp` crate
- Reuse `raya-engine::parser` for lexing/parsing
- Reuse `raya-engine::parser::checker` for type information
- Reference VS Code LSP extension examples
