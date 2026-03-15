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
        symbol: "BigInt",
        hint: "Enable --node-compat for JS bigint compatibility helpers.",
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
    let masked = mask_non_code_regions(source);
    let source_for_scan = masked.as_str();
    let mut best: Option<SymbolMatch> = None;
    let mut best_offset = usize::MAX;

    for sym in NODE_COMPAT_ONLY_SYMBOLS {
        if is_shadowed_in_source(source_for_scan, root_symbol(sym.symbol)) {
            continue;
        }
        if let Some(offset) = find_token_occurrence(source_for_scan, sym.symbol) {
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
    let is_qualified_symbol = token.contains('.');
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
            // Unqualified symbols like `eval` should only match as bare/global usage.
            // Ignore member/property access forms such as `obj.eval(...)`.
            if !is_qualified_symbol {
                let prev_non_ws = source[..start]
                    .chars()
                    .rev()
                    .find(|c| !c.is_ascii_whitespace());
                if prev_non_ws == Some('.') {
                    continue;
                }
                if is_member_or_method_declaration_context(source, start, end) {
                    continue;
                }
            }
            return Some(start);
        }
    }

    None
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_member_or_method_declaration_context(source: &str, _start: usize, end: usize) -> bool {
    let mut idx = end;
    while let Some(ch) = source[idx..].chars().next() {
        if ch.is_ascii_whitespace() {
            idx += ch.len_utf8();
        } else {
            break;
        }
    }

    let Some(next) = source[idx..].chars().next() else {
        return false;
    };

    // Object literal key: `{ eval: ... }`
    if next == ':' {
        return true;
    }

    // Method shorthand/declaration style: `eval(...) { ... }` or `eval(...): T`
    if next != '(' {
        return false;
    }

    let mut depth = 0usize;
    let mut close_idx = None;
    for (off, ch) in source[idx..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    close_idx = Some(idx + off + ch.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }

    let Some(mut after_paren) = close_idx else {
        return false;
    };
    while let Some(ch) = source[after_paren..].chars().next() {
        if ch.is_ascii_whitespace() {
            after_paren += ch.len_utf8();
        } else {
            break;
        }
    }

    matches!(source[after_paren..].chars().next(), Some(':') | Some('{'))
}

fn mask_non_code_regions(source: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Code,
        SingleQuote,
        DoubleQuote,
        Backtick,
        LineComment,
        BlockComment,
    }

    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0usize;
    let mut state = State::Code;
    let mut escaped = false;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        match state {
            State::Code => {
                if ch == '/' && i + 1 < bytes.len() {
                    let next = bytes[i + 1] as char;
                    if next == '/' {
                        out.push(' ');
                        out.push(' ');
                        i += 2;
                        state = State::LineComment;
                        continue;
                    }
                    if next == '*' {
                        out.push(' ');
                        out.push(' ');
                        i += 2;
                        state = State::BlockComment;
                        continue;
                    }
                }
                match ch {
                    '\'' => {
                        out.push(' ');
                        i += 1;
                        state = State::SingleQuote;
                    }
                    '"' => {
                        out.push(' ');
                        i += 1;
                        state = State::DoubleQuote;
                    }
                    '`' => {
                        out.push(' ');
                        i += 1;
                        state = State::Backtick;
                    }
                    _ => {
                        out.push(ch);
                        i += 1;
                    }
                }
            }
            State::SingleQuote | State::DoubleQuote | State::Backtick => {
                let quote = match state {
                    State::SingleQuote => '\'',
                    State::DoubleQuote => '"',
                    State::Backtick => '`',
                    _ => unreachable!(),
                };
                if ch == '\n' {
                    out.push('\n');
                    escaped = false;
                    i += 1;
                    continue;
                }
                if escaped {
                    out.push(' ');
                    escaped = false;
                    i += 1;
                    continue;
                }
                if ch == '\\' {
                    out.push(' ');
                    escaped = true;
                    i += 1;
                    continue;
                }
                out.push(' ');
                i += 1;
                if ch == quote {
                    state = State::Code;
                }
            }
            State::LineComment => {
                if ch == '\n' {
                    out.push('\n');
                    state = State::Code;
                } else {
                    out.push(' ');
                }
                i += 1;
            }
            State::BlockComment => {
                if ch == '*' && i + 1 < bytes.len() && bytes[i + 1] as char == '/' {
                    out.push(' ');
                    out.push(' ');
                    i += 2;
                    state = State::Code;
                } else {
                    out.push(if ch == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
            }
        }
    }

    out
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

    #[test]
    fn ignores_member_access_for_unqualified_symbol() {
        let src = "class C { eval(x: string): number { return 1; } } const c = new C(); return c.eval(\"x\");";
        let m = find_first_node_compat_symbol_usage(src);
        assert!(m.is_none());
    }

    #[test]
    fn ignores_string_literal_occurrences() {
        let src = "const p = \"eval\"; return p.length;";
        let m = find_first_node_compat_symbol_usage(src);
        assert!(m.is_none());
    }
}
