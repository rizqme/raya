#!/usr/bin/env bash
# consolidate_tests.sh
#
# Merges multiple integration test binaries into one per crate.
# Usage: bash scripts/consolidate_tests.sh
#
# Handles: raya-runtime, raya-pm, raya-examples

set -euo pipefail
cd "$(dirname "$0")/.."

consolidate() {
    local crate="$1"       # e.g. raya-runtime
    local entry_name="$2"  # name of the single entry file (without .rs)
    local tests_dir="crates/${crate}/tests"
    local entry="${tests_dir}/${entry_name}.rs"

    echo "=== ${crate} → tests/${entry_name}.rs ==="

    # Collect module names (excluding the entry file itself if it already exists)
    local modules=()
    for src in "${tests_dir}"/*.rs; do
        local name
        name="$(basename "$src" .rs)"
        [[ "$name" == "$entry_name" ]] && continue
        modules+=("$name")
    done

    # Move each X.rs → X/mod.rs
    for name in "${modules[@]}"; do
        local src="${tests_dir}/${name}.rs"
        local dest_dir="${tests_dir}/${name}"
        mkdir -p "$dest_dir"
        mv "$src" "${dest_dir}/mod.rs"
        echo "  moved ${name}.rs → ${name}/mod.rs"
    done

    # Write entry file
    {
        echo "//! ${crate} integration tests — single binary entry point."
        echo ""
        for name in "${modules[@]}"; do
            echo "mod ${name};"
        done
    } > "$entry"

    echo "  wrote ${entry}"
    echo ""
}

consolidate "raya-runtime"  "integration"
consolidate "raya-pm"       "integration"
consolidate "raya-examples" "integration"

echo "Done. Verify with:"
echo "  cargo test -p raya-runtime  --test integration -q"
echo "  cargo test -p raya-pm       --test integration -q"
echo "  cargo test -p raya-examples --test integration -q"
