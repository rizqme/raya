# Benchmarks

This directory contains benchmarks for the Raya VM implementation.

## Running Benchmarks

```bash
# Run all benchmarks
cargo bench --workspace

# Run specific crate benchmarks
cargo bench -p rayavm-core
cargo bench -p rayavm-bytecode

# Run specific benchmark
cargo bench --bench vm_execution
```

## Benchmark Categories

### VM Core Benchmarks (`rayavm-core`)

- **vm_execution** - Bytecode interpreter performance
  - Basic opcode execution
  - Function calls
  - Control flow
  - Arithmetic operations

### Bytecode Benchmarks (`rayavm-bytecode`)

- **bytecode_encoding** - Module serialization/deserialization
  - Constant pool operations
  - Module encoding
  - Module decoding

## Performance Targets

Based on design goals:

- **Opcode dispatch:** < 10ns per instruction (interpreted)
- **Function call:** < 100ns overhead
- **GC allocation:** < 50ns per object
- **Task spawn:** < 1µs per task
- **Context switch:** < 1µs per task switch

## Profiling

For detailed profiling:

```bash
# Install flamegraph
cargo install flamegraph

# Generate flamegraph
cargo flamegraph --bench vm_execution

# Use perf (Linux)
cargo bench --bench vm_execution -- --profile-time=5
```

## Continuous Benchmarking

Benchmarks run on CI for each PR. Performance regressions are tracked automatically.
