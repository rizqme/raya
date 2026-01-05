# Raya VM Makefile

.PHONY: help build test bench clean fmt lint doc

help:
	@echo "Raya VM Development Commands"
	@echo ""
	@echo "  make build      - Build all crates"
	@echo "  make test       - Run all tests"
	@echo "  make bench      - Run benchmarks"
	@echo "  make fmt        - Format code"
	@echo "  make lint       - Run clippy"
	@echo "  make doc        - Generate documentation"
	@echo "  make clean      - Clean build artifacts"
	@echo "  make check      - Run format, lint, and test"
	@echo ""

build:
	cargo build --workspace

build-release:
	cargo build --workspace --release

test:
	cargo test --workspace --verbose

test-doc:
	cargo test --workspace --doc

bench:
	cargo bench --workspace

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

doc:
	cargo doc --workspace --no-deps --open

clean:
	cargo clean

check: fmt-check lint test
	@echo "✓ All checks passed!"

install:
	cargo install --path crates/raya-cli
	@echo "✓ Installed 'raya' command"
	@echo ""
	@echo "Try: raya --help"

# Development targets
dev-vm:
	cargo watch -x 'build -p raya-core'

dev-cli:
	cargo watch -x 'run -p raya-cli'

# Benchmark-specific targets
bench-vm:
	cargo bench -p raya-core

bench-bytecode:
	cargo bench -p raya-bytecode

# Coverage
coverage:
	cargo tarpaulin --workspace --out Html --output-dir coverage/
