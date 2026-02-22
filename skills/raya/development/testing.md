# Testing Infrastructure

Raya has comprehensive test coverage across all components.

## Test Organization

**Total: 4,121+ tests (0 ignored)**

| Component | Tests | Type |
|-----------|-------|------|
| raya-engine | 1,136 | Unit + integration |
| raya-engine (JIT) | +147 | JIT compilation |
| raya-engine (AOT) | +55 | AOT compilation |
| raya-runtime | 2,450 | E2E tests |
| raya-runtime | 30 | Runtime API unit |
| raya-runtime (bundle) | 15 | Bundle format |
| raya-cli | 26 | CLI integration |
| raya-cli (REPL) | 13 | REPL unit |
| raya-stdlib | 41 | Stdlib unit |
| raya-pm | 204 | Package manager |

## Running Tests

### All Tests

```bash
cargo test --workspace
```

### By Crate

```bash
cargo test -p raya-engine
cargo test -p raya-runtime
cargo test -p raya-cli
cargo test -p raya-stdlib
cargo test -p raya-pm
```

### With Features

```bash
cargo test -p raya-engine --features jit
cargo test -p raya-engine --features aot
cargo test --workspace --features jit,aot
```

### Specific Tests

```bash
cargo test -p raya-engine -- test_name
cargo test -p raya-runtime -- discriminated_unions
```

## E2E Tests

**Location:** `crates/raya-runtime/tests/e2e/`

**62 test modules:**
- `arrays.rs`, `classes.rs`, `closures.rs`
- `concurrency.rs`, `control_flow.rs`
- `decorators.rs`, `expressions.rs`
- `functions.rs`, `generics.rs`
- `imports.rs`, `type_narrowing.rs`
- `bug_hunting1.rs` through `bug_hunting5.rs` (773 tests)
- `compiler_edge_cases.rs`
- `cross_feature.rs`
- `diagnostics.rs`
- `missing_features.rs`
- `parser_stress.rs`
- `type_system_edge_cases.rs`

### E2E Test Harness

```rust
use raya_runtime::Runtime;

#[test]
fn test_feature() {
    let rt = Runtime::new();
    let source = r#"
        function main(): void {
            logger.info("Hello");
        }
    "#;
    let module = rt.compile(source).unwrap();
    rt.execute(&module).unwrap();
}
```

## Test Categories

### Unit Tests

Test individual functions/modules in isolation.

**Example:**
```rust
#[test]
fn test_constant_folding() {
    let optimizer = Optimizer::new();
    let ir = parse_ir("const x = 2 + 3");
    let optimized = optimizer.fold_constants(ir);
    assert!(matches!(optimized, IrValue::Const(5)));
}
```

### Integration Tests

Test multiple components together.

**Example:**
```rust
#[test]
fn test_compile_and_execute() {
    let source = "function main() { return 42; }";
    let module = compile(source).unwrap();
    let result = execute(&module).unwrap();
    assert_eq!(result, 42);
}
```

### E2E Tests

Test entire compilation + execution pipeline.

**Example:**
```rust
#[test]
fn test_discriminated_unions() {
    let source = r#"
        type Result<T> = 
          | { ok: true; value: T }
          | { ok: false; error: string };
        
        function test(): int {
            const r: Result<int> = { ok: true, value: 42 };
            if (r.ok) {
                return r.value;
            }
            return -1;
        }
    "#;
    assert_execution(source, 42);
}
```

## Bug Hunting Tests

5 rounds of comprehensive testing:

**bug_hunting1.rs** - Initial edge cases (150 tests)
**bug_hunting2.rs** - Type system edge cases (155 tests)
**bug_hunting3.rs** - Control flow edge cases (148 tests)
**bug_hunting4.rs** - Concurrency edge cases (160 tests)
**bug_hunting5.rs** - Cross-feature interactions (160 tests)

**Bugs Found & Fixed:** 26+

## Test Utilities

### assert_execution

```rust
fn assert_execution(source: &str, expected_exit_code: i32) {
    let rt = Runtime::new();
    let module = rt.compile(source).unwrap();
    let exit_code = rt.execute(&module).unwrap();
    assert_eq!(exit_code, expected_exit_code);
}
```

### assert_compile_error

```rust
fn assert_compile_error(source: &str, expected_error: &str) {
    let rt = Runtime::new();
    let result = rt.compile(source);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains(expected_error));
}
```

## Benchmarks

**Location:** `benches/`

```bash
cargo bench
```

## Coverage (Planned)

```bash
cargo tarpaulin
```

## Related

- [Workflow](workflow.md) - Development practices
- [Adding Modules](adding-modules.md) - How to test new modules
