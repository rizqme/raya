//! Class Method IR Builders
//!
//! Compiles `//@@class_method` annotated methods from `.raya` builtin files
//! through the full Raya pipeline (parse → bind → typecheck → lower) to produce
//! IrFunctions. The `.raya` files are the single source of truth.
//!
//! Each function takes `this` as the first parameter, followed by method arguments.
//! Called via `IrInstr::Call` — regular function calls, NOT `CallMethod`.

use std::sync::LazyLock;
use rustc_hash::FxHashMap;

use crate::compiler::ir::IrFunction;
use crate::compiler::type_registry::{BUILTIN_NATIVE_SOURCES, extract_class_method_names};

/// Compiled class method IrFunctions, lazily initialized from `.raya` sources.
static COMPILED_BUILTINS: LazyLock<CompiledBuiltins> =
    LazyLock::new(CompiledBuiltins::init);

struct CompiledBuiltins {
    /// (type_name, method_name) → IrFunction
    methods: FxHashMap<(String, String), IrFunction>,
}

impl CompiledBuiltins {
    fn init() -> Self {
        let mut methods = FxHashMap::default();

        for &(type_name, source) in BUILTIN_NATIVE_SOURCES {
            // 1. Identify class methods from source
            let class_method_names = extract_class_method_names(source);
            if class_method_names.is_empty() {
                continue;
            }

            // 2. Compile through full pipeline and extract class method IrFunctions
            if let Some(extracted) = compile_and_extract(type_name, source, &class_method_names) {
                for (method_name, ir_func) in extracted {
                    methods.insert((type_name.to_string(), method_name), ir_func);
                }
            }
        }

        CompiledBuiltins { methods }
    }
}

/// Compile a `.raya` source through the full pipeline and extract class method IrFunctions.
fn compile_and_extract(
    type_name: &str,
    source: &str,
    class_method_names: &[String],
) -> Option<Vec<(String, IrFunction)>> {
    use crate::parser::{Parser, TypeContext};
    use crate::parser::checker::{Binder, TypeChecker};
    use crate::compiler::lower::Lowerer;

    // 1. Parse
    let parser = Parser::new(source).unwrap_or_else(|e| {
        panic!("Failed to lex {}: {:?}", type_name, e);
    });
    let (module, interner) = parser.parse().unwrap_or_else(|e| {
        panic!("Failed to parse {}: {:?}", type_name, e);
    });

    // 2. Bind
    let mut type_ctx = TypeContext::new();

    // Save canonical TypeIds before binding — the binder will overwrite named_types
    // with ClassType IDs, but the TypeRegistry dispatch tables need canonical IDs
    let canonical_ids: Vec<(&str, crate::parser::types::TypeId)> =
        BUILTIN_NATIVE_SOURCES.iter()
            .filter_map(|&(name, _)| {
                type_ctx.lookup_named_type(name).map(|id| (name, id))
            })
            .collect();

    let symbols = {
        let mut binder = Binder::new(&mut type_ctx, &interner);
        binder.skip_top_level_duplicate_detection();

        // Pre-register all builtin primitive type names so cross-references resolve
        // (e.g., string.raya references RegExp, array.raya references string)
        for &(name, _) in BUILTIN_NATIVE_SOURCES {
            binder.register_external_class(name);
        }

        binder.bind_module(&module).unwrap_or_else(|e| {
            panic!("Failed to bind {}: {:?}", type_name, e);
        })
    };

    // Restore canonical TypeIds — the binder may have overwritten them with
    // ClassType IDs (e.g., "string" → TypeId(18) instead of canonical TypeId(1))
    for (name, id) in canonical_ids {
        type_ctx.register_named_type(name.to_string(), id);
    }

    // 3. Type check (errors are non-fatal — we still get expr_types)
    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let check_result = match checker.check_module(&module) {
        Ok(result) => result,
        Err(_errors) => {
            // Type errors in builtin files are expected for some constructs
            // (e.g., __NATIVE_CALL returning 'any'). Fall back to empty expr_types.
            use crate::parser::checker::CheckResult;
            use crate::parser::checker::captures::ModuleCaptureInfo;
            CheckResult {
                inferred_types: FxHashMap::default(),
                captures: ModuleCaptureInfo::new(),
                expr_types: FxHashMap::default(),
                warnings: Vec::new(),
            }
        }
    };

    // 4. Lower
    let mut lowerer = Lowerer::with_expr_types(&type_ctx, &interner, check_result.expr_types);
    let ir_module = lowerer.lower_module(&module);

    // 5. Extract class method IrFunctions by name
    // The lowerer names class methods as "ClassName::methodName"
    let mut result = Vec::new();
    for method_name in class_method_names {
        let full_name = format!("{}::{}", type_name, method_name);
        for func in &ir_module.functions {
            if func.name == full_name {
                result.push((method_name.clone(), func.clone()));
                break;
            }
        }
    }

    Some(result)
}

/// Build an IR function for a class method.
/// Returns None if the method is not recognized.
pub fn build_class_method_ir(type_name: &str, method_name: &str) -> Option<IrFunction> {
    let builtins = &*COMPILED_BUILTINS;
    builtins
        .methods
        .get(&(type_name.to_string(), method_name.to_string()))
        .cloned()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_all_class_methods() {
        // Verify all 11 builders produce valid IR functions
        let methods = [
            ("Array", "forEach"),
            ("Array", "map"),
            ("Array", "filter"),
            ("Array", "find"),
            ("Array", "findIndex"),
            ("Array", "every"),
            ("Array", "some"),
            ("Array", "reduce"),
            ("Array", "sort"),
            ("string", "replaceWith"),
            ("RegExp", "replaceWith"),
        ];

        for (type_name, method_name) in &methods {
            let func = build_class_method_ir(type_name, method_name)
                .unwrap_or_else(|| panic!("Failed to build {}.{}", type_name, method_name));
            func.validate().unwrap_or_else(|e| {
                panic!("{}.{} validation failed: {}", type_name, method_name, e)
            });
        }
    }

    #[test]
    fn test_unknown_method_returns_none() {
        assert!(build_class_method_ir("Array", "nonexistent").is_none());
        assert!(build_class_method_ir("string", "map").is_none());
    }

    #[test]
    fn test_foreach_structure() {
        let func = build_class_method_ir("Array", "forEach").unwrap();
        assert_eq!(func.params.len(), 2); // this, fn
    }

    #[test]
    fn test_sort_structure() {
        let func = build_class_method_ir("Array", "sort").unwrap();
        assert_eq!(func.params.len(), 2); // this, compareFn
    }

    #[test]
    fn test_reduce_structure() {
        let func = build_class_method_ir("Array", "reduce").unwrap();
        assert_eq!(func.params.len(), 3); // this, fn, initial
    }

    #[test]
    fn test_string_replace_with_structure() {
        let func = build_class_method_ir("string", "replaceWith").unwrap();
        assert_eq!(func.params.len(), 3); // this, pattern, replacer
    }

    #[test]
    fn test_regexp_replace_with_structure() {
        let func = build_class_method_ir("RegExp", "replaceWith").unwrap();
        assert_eq!(func.params.len(), 3); // this, str, replacer
    }
}
