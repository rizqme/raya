//! Builtin surface contract for strict vs node-compat mode.

#[derive(Debug, Clone, Copy)]
pub struct CompatSymbol {
    pub symbol: &'static str,
    pub hint: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct SymbolMatch {
    pub symbol: &'static str,
    pub line: usize,
    pub hint: &'static str,
}

// Node-compat-only symbols enforced in strict mode.
// This is intentionally conservative and can be expanded incrementally.
pub const NODE_COMPAT_ONLY_SYMBOLS: &[CompatSymbol] = &[
    CompatSymbol {
        symbol: "Object.defineProperty",
        hint: "Enable --node-compat for descriptor APIs.",
    },
    CompatSymbol {
        symbol: "Object.getOwnPropertyDescriptor",
        hint: "Enable --node-compat for descriptor APIs.",
    },
    CompatSymbol {
        symbol: "Object.defineProperties",
        hint: "Enable --node-compat for descriptor APIs.",
    },
    CompatSymbol {
        symbol: "ArrayBuffer",
        hint: "ArrayBuffer is node-compat-only; use Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "DataView",
        hint: "DataView is node-compat-only; use Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Uint8Array",
        hint: "Typed arrays are node-compat-only; use int[]/number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Uint8ClampedArray",
        hint: "Typed arrays are node-compat-only; use int[]/number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Int8Array",
        hint: "Typed arrays are node-compat-only; use int[]/number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Int16Array",
        hint: "Typed arrays are node-compat-only; use int[]/number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Int32Array",
        hint: "Typed arrays are node-compat-only; use int[]/number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Uint16Array",
        hint: "Typed arrays are node-compat-only; use int[]/number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Uint32Array",
        hint: "Typed arrays are node-compat-only; use int[]/number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Float32Array",
        hint: "Typed arrays are node-compat-only; use number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Float16Array",
        hint: "Typed arrays are node-compat-only; use number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "Float64Array",
        hint: "Typed arrays are node-compat-only; use number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "BigInt64Array",
        hint: "Typed arrays are node-compat-only; use number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "BigUint64Array",
        hint: "Typed arrays are node-compat-only; use number[]/Buffer in strict mode.",
    },
    CompatSymbol {
        symbol: "TypedArray",
        hint: "Typed arrays are node-compat-only; use strict collection types.",
    },
    CompatSymbol {
        symbol: "SharedArrayBuffer",
        hint: "Enable --node-compat for shared-memory APIs.",
    },
    CompatSymbol {
        symbol: "Atomics",
        hint: "Enable --node-compat for shared-memory atomics.",
    },
    CompatSymbol {
        symbol: "parseInt",
        hint: "Enable --node-compat for JS legacy parsing helpers.",
    },
    CompatSymbol {
        symbol: "parseFloat",
        hint: "Enable --node-compat for JS legacy parsing helpers.",
    },
    CompatSymbol {
        symbol: "isNaN",
        hint: "Enable --node-compat for JS global numeric helpers.",
    },
    CompatSymbol {
        symbol: "isFinite",
        hint: "Enable --node-compat for JS global numeric helpers.",
    },
    CompatSymbol {
        symbol: "eval",
        hint: "Enable --node-compat for dynamic code evaluation.",
    },
    CompatSymbol {
        symbol: "Function",
        hint: "Enable --node-compat for JS function-constructor compatibility.",
    },
    CompatSymbol {
        symbol: "AsyncFunction",
        hint: "Enable --node-compat for async function-constructor compatibility.",
    },
    CompatSymbol {
        symbol: "Generator",
        hint: "Enable --node-compat for generator constructor compatibility.",
    },
    CompatSymbol {
        symbol: "GeneratorFunction",
        hint: "Enable --node-compat for generator-function constructor compatibility.",
    },
    CompatSymbol {
        symbol: "AsyncGenerator",
        hint: "Enable --node-compat for async-generator constructor compatibility.",
    },
    CompatSymbol {
        symbol: "AsyncGeneratorFunction",
        hint: "Enable --node-compat for async-generator-function constructor compatibility.",
    },
    CompatSymbol {
        symbol: "AsyncIterator",
        hint: "Enable --node-compat for async iterator constructor compatibility.",
    },
    CompatSymbol {
        symbol: "Proxy",
        hint: "Enable --node-compat for meta-object proxy APIs.",
    },
    CompatSymbol {
        symbol: "Reflect",
        hint: "Enable --node-compat for reflective meta APIs.",
    },
    CompatSymbol {
        symbol: "WeakMap",
        hint: "Enable --node-compat for weak collection APIs.",
    },
    CompatSymbol {
        symbol: "WeakSet",
        hint: "Enable --node-compat for weak collection APIs.",
    },
    CompatSymbol {
        symbol: "WeakRef",
        hint: "Enable --node-compat for weak reference APIs.",
    },
    CompatSymbol {
        symbol: "FinalizationRegistry",
        hint: "Enable --node-compat for finalization APIs.",
    },
    CompatSymbol {
        symbol: "DisposableStack",
        hint: "Enable --node-compat for disposal stack APIs.",
    },
    CompatSymbol {
        symbol: "AsyncDisposableStack",
        hint: "Enable --node-compat for async disposal stack APIs.",
    },
    CompatSymbol {
        symbol: "Intl",
        hint: "Enable --node-compat for Intl namespace.",
    },
    CompatSymbol {
        symbol: "globalThis",
        hint: "Enable --node-compat for JS global object semantics.",
    },
    CompatSymbol {
        symbol: "escape",
        hint: "Enable --node-compat for deprecated legacy APIs.",
    },
    CompatSymbol {
        symbol: "unescape",
        hint: "Enable --node-compat for deprecated legacy APIs.",
    },
];

pub fn find_first_node_compat_symbol_usage(source: &str) -> Option<SymbolMatch> {
    let mut best: Option<SymbolMatch> = None;
    let mut best_offset = usize::MAX;

    for sym in NODE_COMPAT_ONLY_SYMBOLS {
        if is_shadowed_in_source(source, root_symbol(sym.symbol)) {
            continue;
        }
        if let Some(offset) = find_token_occurrence(source, sym.symbol) {
            if offset < best_offset {
                best_offset = offset;
                let line = source[..offset].bytes().filter(|&b| b == b'\n').count() + 1;
                best = Some(SymbolMatch {
                    symbol: sym.symbol,
                    line,
                    hint: sym.hint,
                });
            }
        }
    }

    best
}

fn root_symbol(symbol: &str) -> &str {
    symbol.split('.').next().unwrap_or(symbol)
}

fn is_shadowed_in_source(source: &str, symbol_root: &str) -> bool {
    // Fast conservative checks for common declaration forms. This avoids
    // false positives from strict compat precheck when user code intentionally
    // shadows node-compat globals.
    let patterns = [
        format!("let {}", symbol_root),
        format!("const {}", symbol_root),
        format!("function {}", symbol_root),
        format!("class {}", symbol_root),
        format!("import {{ {}", symbol_root),
        format!("import * as {}", symbol_root),
    ];
    patterns.iter().any(|p| source.contains(p))
}

fn find_token_occurrence(source: &str, token: &str) -> Option<usize> {
    for (start, _) in source.match_indices(token) {
        let end = start + token.len();

        let left_ok = if start == 0 {
            true
        } else {
            source[..start]
                .chars()
                .next_back()
                .map(|c| !is_ident_char(c))
                .unwrap_or(true)
        };

        let right_ok = if end >= source.len() {
            true
        } else {
            source[end..]
                .chars()
                .next()
                .map(|c| !is_ident_char(c))
                .unwrap_or(true)
        };

        if left_ok && right_ok {
            return Some(start);
        }
    }

    None
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_symbol_with_line() {
        let src = "let x = 1;\nlet b = new ArrayBuffer(8);\nreturn b;";
        let m = find_first_node_compat_symbol_usage(src).expect("expected match");
        assert_eq!(m.symbol, "ArrayBuffer");
        assert_eq!(m.line, 2);
    }

    #[test]
    fn avoids_partial_identifier_matches() {
        let src = "let parseInteger = 1; return parseInteger;";
        let m = find_first_node_compat_symbol_usage(src);
        assert!(m.is_none());
    }

    #[test]
    fn allows_shadowed_compat_symbol() {
        let src = "function parseInt(v: string): number { return 1; } return parseInt(\"x\");";
        let m = find_first_node_compat_symbol_usage(src);
        assert!(m.is_none());
    }
}
