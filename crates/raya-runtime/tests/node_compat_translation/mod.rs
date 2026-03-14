//! Auto-translation harness for Node `node:` module compatibility cases.
//!
//! This scans a Node source tree, extracts `node:` imports, filters to supported
//! v1 shim modules, and emits a normalized fixture list for compatibility tests.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_NODE_ROOT: &str = "/Users/rizqme/Workspace/node/test";
const SUPPORTED_NODE_MODULES: [&str; 38] = [
    "node:fs",
    "node:fs/promises",
    "node:path",
    "node:os",
    "node:process",
    "node:dns",
    "node:net",
    "node:http",
    "node:https",
    "node:crypto",
    "node:url",
    "node:stream",
    "node:events",
    "node:assert",
    "node:assert/strict",
    "node:util",
    "node:module",
    "node:child_process",
    "node:test",
    "node:test/reporters",
    "node:timers",
    "node:timers/promises",
    "node:buffer",
    "node:string_decoder",
    "node:stream/promises",
    "node:stream/web",
    "node:worker_threads",
    "node:vm",
    "node:http2",
    "node:inspector",
    "node:inspector/promises",
    "node:async_hooks",
    "node:diagnostics_channel",
    "node:v8",
    "node:dgram",
    "node:cluster",
    "node:repl",
    "node:perf_hooks",
];

#[test]
fn generate_node_compat_fixture_from_node_tests() {
    let node_root = resolve_node_root();

    if !node_root.exists() {
        eprintln!(
            "Skipping node translation fixture generation; root does not exist: {}",
            node_root.display()
        );
        return;
    }

    let mut files = Vec::new();
    collect_js_files(&node_root, &mut files).expect("failed to collect Node test files");
    files.sort();

    let supported: BTreeSet<&'static str> = SUPPORTED_NODE_MODULES.into_iter().collect();
    let mut translated: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for file in files {
        let source = match fs::read_to_string(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut modules = BTreeSet::new();
        for specifier in extract_node_specifiers(&source) {
            if supported.contains(specifier.as_str()) {
                modules.insert(specifier);
            }
        }
        if modules.is_empty() {
            continue;
        }
        translated.insert(file.display().to_string(), modules);
    }

    assert!(
        !translated.is_empty(),
        "expected at least one translated node-compat fixture case"
    );

    let mut normalized = String::new();
    for (file, modules) in translated {
        normalized.push_str(&file);
        normalized.push('|');
        normalized.push_str(&modules.into_iter().collect::<Vec<_>>().join(","));
        normalized.push('\n');
    }

    let output_path = generated_fixture_path();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).expect("failed to create generated fixture directory");
    }
    fs::write(&output_path, normalized).expect("failed to write generated fixture file");

    let written = fs::read_to_string(&output_path).expect("failed to read generated fixture file");
    assert!(
        written.contains("node:fs") || written.contains("node:path"),
        "expected generated fixture file to include at least node:fs or node:path"
    );
}

fn resolve_node_root() -> PathBuf {
    let configured = std::env::var("RAYA_NODE_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_NODE_ROOT));
    if configured.is_absolute() {
        return configured;
    }

    if configured.exists() {
        return configured;
    }

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));

    let from_manifest = manifest_dir.join(&configured);
    if from_manifest.exists() {
        return from_manifest;
    }

    let workspace_guess = manifest_dir.join("../..").join(&configured);
    if workspace_guess.exists() {
        return workspace_guess;
    }

    configured
}

fn collect_js_files(root: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if ext == "js" || ext == "mjs" {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn extract_node_specifiers(source: &str) -> BTreeSet<String> {
    let mut specs = BTreeSet::new();
    for quote in ['"', '\''] {
        for piece in source.split(quote) {
            if piece.starts_with("node:") {
                let token = piece
                    .split(|c: char| c.is_whitespace() || c == ';' || c == ')' || c == ',')
                    .next()
                    .unwrap_or("")
                    .trim();
                if !token.is_empty() {
                    specs.insert(token.to_string());
                }
            }
        }
    }
    specs
}

fn generated_fixture_path() -> PathBuf {
    let manifest_dir = option_env!("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join("crates/raya-runtime"))
        })
        .expect("unable to resolve raya-runtime manifest dir");
    manifest_dir
        .join("../../target/generated/node_compat")
        .join("translated_node_cases.txt")
}
