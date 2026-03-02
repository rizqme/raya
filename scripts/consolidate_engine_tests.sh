#!/usr/bin/env bash
# consolidate_engine_tests.sh
#
# Merges the 46 raya-engine integration test binaries into one.
#
# Strategy:
#   1. For each tests/X.rs — create tests/X/ and move to tests/X/mod.rs
#   2. Strip the crate-level #![cfg(feature = "...")] inner attributes from
#      aot_integration and jit_integration (those go on the `mod` line in the
#      entry file instead).
#   3. Write tests/engine.rs — the single entry point — with:
#        • consolidated #![allow(...)] covering all per-file suppressions
#        • #[cfg(feature = "aot")] mod aot_integration;
#        • #[cfg(feature = "jit")] mod jit_integration;
#        • mod X; for every other module

set -euo pipefail

TESTS_DIR="crates/raya-engine/tests"
ENTRY="${TESTS_DIR}/engine.rs"

cd "$(dirname "$0")/.."

# ── 1. Feature-gated files — strip the #![cfg(...)] inner attr ──────────────

for feature in aot jit; do
    file="${TESTS_DIR}/${feature}_integration.rs"
    if [[ -f "$file" ]]; then
        # Remove any line that is exactly: #![cfg(feature = "aot")]  or jit
        sed -i.bak "/^#!\[cfg(feature = \"${feature}\")\]/d" "$file"
        rm -f "${file}.bak"
        echo "  stripped #![cfg(feature = \"${feature}\")] from ${file}"
    fi
done

# ── 2. Move every tests/X.rs → tests/X/mod.rs ───────────────────────────────

for src in "${TESTS_DIR}"/*.rs; do
    name="$(basename "$src" .rs)"
    [[ "$name" == "engine" ]] && continue   # skip the entry file if it already exists

    dest_dir="${TESTS_DIR}/${name}"
    dest="${dest_dir}/mod.rs"

    mkdir -p "$dest_dir"
    mv "$src" "$dest"
    echo "  moved ${src} → ${dest}"
done

# ── 3. Write the single entry file ───────────────────────────────────────────
#
# Consolidated allows come from the 5 files that had per-file suppressions:
#   safepoint_integration      — unused_variables, unused_imports
#   snapshot_integration       — clippy::identity_op
#   concurrency_integration    — clippy::identity_op, unused_variables
#   scheduler_integration      — clippy::identity_op, unused_variables
#   vm_context_integration     — clippy::unnecessary_cast

cat > "$ENTRY" << 'EOF'
//! raya-engine integration tests — single binary entry point.
//!
//! Each submodule corresponds to a former top-level test file.
//! Build variants:
//!   cargo test -p raya-engine                      — base tests
//!   cargo test -p raya-engine --features jit       — + JIT tests
//!   cargo test -p raya-engine --features aot       — + AOT tests

#![allow(
    unused_variables,
    unused_imports,
    clippy::identity_op,
    clippy::unnecessary_cast
)]

// ── feature-gated suites ─────────────────────────────────────────────────────
#[cfg(feature = "aot")]
mod aot_integration;
#[cfg(feature = "jit")]
mod jit_integration;

// ── base suites ──────────────────────────────────────────────────────────────
mod ast_tests;
mod async_call_tests;
mod basic_codegen;
mod closure_tests;
mod codegen_tests;
mod concurrency_integration;
mod concurrent_task_tests;
mod context_isolation_tests;
mod decorator_test;
mod exception_handling_basic;
mod expression_tests;
mod gc_integration_tests;
mod gc_stress_tests;
mod hardening_test;
mod import_resolution;
mod inner_vm_integration;
mod interpreter_integration;
mod ir_comprehensive;
mod ir_demo;
mod json_integration;
mod jsx_lowering_tests;
mod jsx_tests;
mod milestone_2_9_test;
mod module_linking;
mod module_loading;
mod monomorphize_tests;
mod object_model_tests;
mod opcode_tests;
mod pattern_tests;
mod reflect_phase8_tests;
mod rest_pattern_test;
mod safepoint_integration;
mod scheduler_integration;
mod snapshot_integration;
mod snapshot_jit;
mod snapshot_restore_validation;
mod stack_integration;
mod statement_tests;
mod template_test;
mod tokens;
mod type_tests;
mod visibility_test;
mod visitor_tests;
mod vm_context_integration;
EOF

echo ""
echo "Done. Single entry point: ${ENTRY}"
echo ""
echo "Verify with:"
echo "  cargo test -p raya-engine -q"
echo "  cargo test -p raya-engine -q --features jit"
echo "  cargo test -p raya-engine -q --features aot"
