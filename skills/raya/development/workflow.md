# Development Workflow

Best practices for contributing to Raya.

## ⚠️ Mandatory: Worktree Workflow

**Before executing any plan**, create a git worktree:

```bash
# Use the /worktree skill or manual command
git worktree add .worktrees/feature-name -b feature-name
cd .worktrees/feature-name

# Do all implementation work here
# ...

# When complete, merge to main
git checkout main
git merge feature-name
git worktree remove .worktrees/feature-name
```

**Why?**
- Isolates changes from main branch
- Allows parallel work on multiple features
- Clean separation of concerns
- Easy to abandon if needed

## ⚠️ Mandatory: Documentation Maintenance

**After EVERY turn**, update relevant documentation:

1. **Milestone files** (`plans/milestone-*.md`)
   - Mark completed tasks with `[x]`
   - Update status sections
   
2. **PLAN.md** (`plans/PLAN.md`)
   - Update overall progress
   - Update current focus

3. **Hierarchical CLAUDE.md** (`crates/**/CLAUDE.md`)
   - Keep concise, key info only
   - Update module status

4. **Design docs** (`docs/`)
   - If behavior or API changes
   - Update specifications

5. **Root CLAUDE.md**
   - Update status section if milestone progress changes

## Build Commands

```bash
# Build all crates
cargo build --workspace

# Build with JIT
cargo build --workspace --features jit

# Build with AOT
cargo build --workspace --features aot

# Build specific crate
cargo build -p raya-engine
cargo build -p raya-cli

# Release build
cargo build --release -p raya-cli
```

## Development Commands

```bash
# Run tests
cargo test

# Run specific crate tests
cargo test -p raya-engine
cargo test -p raya-runtime

# Run with features
cargo test --features jit
cargo test --features aot

# Check (fast, no codegen)
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

## Testing Workflow

1. Write tests first (TDD)
2. Run relevant test suite
3. Implement feature
4. Run full test suite
5. Verify no regressions

```bash
# Fast iteration
cargo test -p raya-engine -- test_name

# Full validation
cargo test --workspace
```

## Adding Features

1. **Plan** - Write plan in `plans/` or session workspace
2. **Worktree** - Create isolated worktree
3. **Implement** - Write code + tests
4. **Document** - Update all relevant docs
5. **Test** - Run full test suite
6. **Review** - Self-review changes
7. **Merge** - Merge to main, remove worktree

## Code Style

- **Rust:** Follow `rustfmt` (run `cargo fmt`)
- **Raya:** TypeScript-style conventions
- **Comments:** Only where needed, prefer self-documenting code
- **Tests:** Descriptive names, arrange-act-assert pattern

## Commit Messages

```
feat(crate): Brief description

Detailed explanation if needed.

- Bullet points for multiple changes
- Reference issues if applicable
```

**Co-author** (for AI assistance):
```
Co-Authored-By: Craft Agent <agents-noreply@craft.do>
```

## Related

- [Testing](testing.md) - Test infrastructure
- [Adding Modules](adding-modules.md) - Stdlib module guide
- [Project Status](status.md) - Current work
