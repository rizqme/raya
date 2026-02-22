//! Type Registry
//!
//! Single source of truth for built-in type dispatch, built from `.raya` builtin files.
//! Replaces DispatchRegistry, BUILTIN_SIGS, get_type_name, resolve_type_annotation,
//! and normalize_type_for_dispatch.
//!
//! The registry is populated at compiler init by scanning embedded `.raya` source
//! files for `//@@builtin_native` classes, extracting native IDs from
//! `__NATIVE_CALL(CONST, ...)` patterns, and `//@@opcode` annotations.

use std::sync::LazyLock;
use rustc_hash::{FxHashMap, FxHashSet};
use crate::parser::types::ty::{Type, TypeId};
use crate::parser::TypeContext;

/// Sentinel TypeId for when the lowerer cannot determine the type.
/// Distinct from TypeId(0) (Number) and TypeId(6) (Unknown).
pub const UNRESOLVED_TYPE_ID: u32 = u32::MAX;

// ============================================================================
// Dispatch Action Types (moved from dispatch.rs)
// ============================================================================

/// How to dispatch a type-specific method or property access.
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchAction {
    /// Emit a specialized opcode directly (e.g., StringLen, ArrayLen)
    Opcode(OpcodeKind),
    /// Emit CallMethod with this builtin method ID (VM handles natively)
    NativeCall(u16),
    /// Emit Call to a pre-compiled class method function.
    /// Contains (type_name, method_name) to look up the IR builder.
    ClassMethod(String, String),
}

/// Specialized opcodes for property/method access.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpcodeKind {
    StringLen,
    ArrayLen,
}

// ============================================================================
// Embedded Builtin Sources
// ============================================================================

/// Builtin primitive `.raya` sources embedded at compile time.
/// These are scanned at TypeRegistry init to build dispatch tables.
pub(crate) const BUILTIN_NATIVE_SOURCES: &[(&str, &str)] = &[
    ("string", include_str!("../../builtins/string.raya")),
    ("number", include_str!("../../builtins/number.raya")),
    ("Array", include_str!("../../builtins/array.raya")),
    ("RegExp", include_str!("../../builtins/regexp.raya")),
    ("Map", include_str!("../../builtins/map.raya")),
    ("Set", include_str!("../../builtins/set.raya")),
    ("Buffer", include_str!("../../builtins/buffer.raya")),
    ("Channel", include_str!("../../builtins/channel.raya")),
];

/// Additional builtin `.raya` sources for type signature extraction.
/// These types are not yet `//@@builtin_native` but their method signatures
/// are needed by the checker for type resolution.
const BUILTIN_CLASS_SOURCES: &[(&str, &str)] = &[
    ("Date", include_str!("../../builtins/date.raya")),
    ("Mutex", include_str!("../../builtins/mutex.raya")),
    ("Task", include_str!("../../builtins/task.raya")),
];

// ============================================================================
// Type Registry
// ============================================================================

/// Central type registry built from `.raya` builtin files.
///
/// Provides dispatch lookup for methods and properties on native types,
/// constructor native IDs, name↔TypeId mapping (via TypeContext),
/// and union type resolution.
pub struct TypeRegistry {
    /// Method dispatch by exact TypeId: type_id → { method_name → action }
    method_dispatch: FxHashMap<u32, FxHashMap<String, DispatchAction>>,
    /// Property dispatch by exact TypeId: type_id → { prop_name → action }
    property_dispatch: FxHashMap<u32, FxHashMap<String, DispatchAction>>,
    /// Array method dispatch (matches any array TypeId)
    array_methods: FxHashMap<String, DispatchAction>,
    /// Array property dispatch
    array_properties: FxHashMap<String, DispatchAction>,
    /// Constructor native IDs: type_name → native_id
    constructors: FxHashMap<String, u16>,
    /// Set of type names that are `//@@builtin_native`
    builtin_natives: FxHashSet<String>,
    /// TypeId → type name (reverse lookup)
    type_names: FxHashMap<u32, String>,
    /// native_id → return TypeId (for return type propagation)
    method_return_types: FxHashMap<u16, u32>,
    /// Per-type method signatures extracted from .raya AST
    native_type_info: FxHashMap<String, NativeTypeInfo>,
}

impl TypeRegistry {
    /// Build the registry from embedded `.raya` sources and the TypeContext.
    pub fn new(type_ctx: &TypeContext) -> Self {
        let mut registry = Self {
            method_dispatch: FxHashMap::default(),
            property_dispatch: FxHashMap::default(),
            array_methods: FxHashMap::default(),
            array_properties: FxHashMap::default(),
            constructors: FxHashMap::default(),
            builtin_natives: FxHashSet::default(),
            type_names: FxHashMap::default(),
            method_return_types: FxHashMap::default(),
            native_type_info: FxHashMap::default(),
        };

        // Build reverse name lookup from TypeContext's named types
        // We check all known type names
        let known_names = [
            "number", "string", "boolean", "null", "void", "never", "unknown",
            "Mutex", "RegExp", "Date", "Buffer", "Task", "Channel", "Map", "Set",
            "Json", "int", "Array",
        ];
        for name in &known_names {
            if let Some(id) = type_ctx.lookup_named_type(name) {
                registry.type_names.insert(id.as_u32(), name.to_string());
            }
        }

        // Scan each builtin primitive source
        for &(type_name, source) in BUILTIN_NATIVE_SOURCES {
            registry.scan_builtin_native(type_name, source, type_ctx);
        }

        // Register return types for compiler-internal method variants.
        // The lowerer remaps string methods when the argument is RegExp:
        //   REPLACE(0x020A) → REPLACE_REGEXP(0x0215), same return type
        //   SPLIT(0x0207) → SPLIT_REGEXP(0x0216), same return type
        //   REPLACE_WITH → REPLACE_WITH_REGEXP(0x0217), same return type
        if let Some(&ret) = registry.method_return_types.get(&0x020A) {
            registry.method_return_types.insert(0x0215, ret); // REPLACE_REGEXP
            registry.method_return_types.insert(0x0217, ret); // REPLACE_WITH_REGEXP
        }
        if let Some(&ret) = registry.method_return_types.get(&0x0207) {
            registry.method_return_types.insert(0x0216, ret); // SPLIT_REGEXP
        }

        // Register int as sharing number's dispatch (number | int subsumption)
        if let (Some(num_id), Some(int_id)) = (
            type_ctx.lookup_named_type("number"),
            type_ctx.lookup_named_type("int"),
        ) {
            if let Some(num_methods) = registry.method_dispatch.get(&num_id.as_u32()).cloned() {
                registry.method_dispatch.insert(int_id.as_u32(), num_methods);
            }
        }

        // Register builtin class dispatch (Map, Set, Channel, etc.) — these don't
        // have .raya files yet, so their native IDs are registered programmatically.
        registry.register_builtin_class_dispatch(type_ctx);

        // Use pre-extracted method signatures from lazy static
        registry.native_type_info = NATIVE_TYPE_INFO.clone();

        registry
    }

    /// Register dispatch entries for builtin classes that don't have .raya files yet.
    ///
    /// These are classes whose methods are handled natively by the VM.
    /// Eventually these will be migrated to .raya files.
    fn register_builtin_class_dispatch(&mut self, type_ctx: &TypeContext) {
        use crate::vm::builtin::{date, mutex, task};

        // Only types still using prepended class dispatch (not yet //@@builtin_native)
        let builtin_types: &[(&str, &[(&str, u16)])] = &[
            ("Date", &[
                ("getTime", date::GET_TIME), ("getFullYear", date::GET_FULL_YEAR),
                ("getMonth", date::GET_MONTH), ("getDate", date::GET_DATE),
                ("getDay", date::GET_DAY), ("getHours", date::GET_HOURS),
                ("getMinutes", date::GET_MINUTES), ("getSeconds", date::GET_SECONDS),
                ("getMilliseconds", date::GET_MILLISECONDS),
                ("setFullYear", date::SET_FULL_YEAR), ("setMonth", date::SET_MONTH),
                ("setDate", date::SET_DATE), ("setHours", date::SET_HOURS),
                ("setMinutes", date::SET_MINUTES), ("setSeconds", date::SET_SECONDS),
                ("setMilliseconds", date::SET_MILLISECONDS),
                ("toString", date::TO_STRING), ("toISOString", date::TO_ISO_STRING),
                ("toDateString", date::TO_DATE_STRING), ("toTimeString", date::TO_TIME_STRING),
            ]),
            ("Mutex", &[
                ("tryLock", mutex::TRY_LOCK), ("isLocked", mutex::IS_LOCKED),
            ]),
            ("Task", &[
                ("isDone", task::IS_DONE), ("isCancelled", task::IS_CANCELLED),
            ]),
        ];

        for &(type_name, methods) in builtin_types {
            if let Some(id) = type_ctx.lookup_named_type(type_name) {
                let tid = id.as_u32();
                let meths = self.method_dispatch.entry(tid).or_default();
                for &(method_name, native_id) in methods {
                    meths.insert(method_name.to_string(), DispatchAction::NativeCall(native_id));
                }
            }
        }
    }

    /// Scan a `//@@builtin_native` `.raya` source and populate dispatch tables.
    fn scan_builtin_native(&mut self, type_name: &str, source: &str, type_ctx: &TypeContext) {
        // Verify the source contains //@@builtin_native
        if !source.contains("//@@builtin_native") {
            return;
        }

        self.builtin_natives.insert(type_name.to_string());

        // Step 1: Extract const declarations → name→value map
        let constants = extract_constants(source);

        // Step 2: Extract opcode properties
        let opcode_props = extract_opcode_properties(source);

        // Step 3: Extract methods and their dispatch behavior
        let methods = extract_methods(source, &constants);

        // Step 4: Check for constructor
        let constructor_id = extract_constructor(source, &constants);

        // Step 5: Extract //@@class_method annotated methods
        let class_methods = extract_class_method_names(source);

        // Step 6: Register in dispatch tables
        if type_name == crate::parser::TypeContext::ARRAY_TYPE_NAME {
            // Array has special dispatch: matches any array TypeId
            for (prop_name, kind) in &opcode_props {
                self.array_properties.insert(prop_name.clone(), DispatchAction::Opcode(*kind));
            }
            for &(ref method_name, native_id, ref ret_type) in &methods {
                self.array_methods.insert(method_name.clone(), DispatchAction::NativeCall(native_id));
                if let Some(ret_tid) = ret_type.as_ref().and_then(|rt| resolve_return_type_str(type_ctx, rt)) {
                    self.method_return_types.insert(native_id, ret_tid);
                }
            }
            // Register class methods (callback methods like map, filter, etc.)
            for cm_name in &class_methods {
                self.array_methods.insert(
                    cm_name.clone(),
                    DispatchAction::ClassMethod(type_name.to_string(), cm_name.clone()),
                );
            }
        } else {
            // Look up TypeId for this type
            let type_id = type_ctx.lookup_named_type(type_name);
            if let Some(id) = type_id {
                let tid = id.as_u32();

                // Properties
                if !opcode_props.is_empty() {
                    let props = self.property_dispatch.entry(tid).or_default();
                    for (prop_name, kind) in &opcode_props {
                        props.insert(prop_name.clone(), DispatchAction::Opcode(*kind));
                    }
                }

                // Atomic methods (NativeCall)
                if !methods.is_empty() {
                    let meths = self.method_dispatch.entry(tid).or_default();
                    for &(ref method_name, native_id, ref ret_type) in &methods {
                        meths.insert(method_name.clone(), DispatchAction::NativeCall(native_id));
                        if let Some(ret_tid) = ret_type.as_ref().and_then(|rt| resolve_return_type_str(type_ctx, rt)) {
                            self.method_return_types.insert(native_id, ret_tid);
                        }
                    }
                }

                // Class methods (callback methods)
                if !class_methods.is_empty() {
                    let meths = self.method_dispatch.entry(tid).or_default();
                    for cm_name in &class_methods {
                        meths.insert(
                            cm_name.clone(),
                            DispatchAction::ClassMethod(type_name.to_string(), cm_name.clone()),
                        );
                    }
                }
            }
        }

        // Constructor
        if let Some(native_id) = constructor_id {
            self.constructors.insert(type_name.to_string(), native_id);
        }
    }

    // ========================================================================
    // Public API
    // ========================================================================

    /// Look up a property dispatch for a given type.
    pub fn lookup_property(&self, type_id: u32, name: &str) -> Option<DispatchAction> {
        // Exact type match
        if let Some(props) = self.property_dispatch.get(&type_id) {
            if let Some(action) = props.get(name) {
                return Some(action.clone());
            }
        }
        // Array type fallback
        if self.is_array_type_id(type_id) {
            if let Some(action) = self.array_properties.get(name) {
                return Some(action.clone());
            }
        }
        None
    }

    /// Look up a method dispatch for a given type.
    pub fn lookup_method(&self, type_id: u32, name: &str) -> Option<DispatchAction> {
        // Exact type match
        if let Some(meths) = self.method_dispatch.get(&type_id) {
            if let Some(action) = meths.get(name) {
                return Some(action.clone());
            }
        }
        // Array type fallback
        if self.is_array_type_id(type_id) {
            if let Some(action) = self.array_methods.get(name) {
                return Some(action.clone());
            }
        }
        None
    }

    /// Get the constructor native ID for a type (e.g., Array, RegExp).
    pub fn constructor_native_id(&self, type_name: &str) -> Option<u16> {
        self.constructors.get(type_name).copied()
    }

    /// Get the type name for a TypeId.
    pub fn type_name(&self, type_id: u32) -> Option<&str> {
        self.type_names.get(&type_id).map(|s| s.as_str())
    }

    /// Look up the return TypeId for a native method ID.
    /// Used for return type propagation after CallMethod.
    pub fn lookup_return_type(&self, native_id: u16) -> Option<u32> {
        self.method_return_types.get(&native_id).copied()
    }

    /// Check if a type is a `//@@builtin_native`.
    pub fn is_builtin_native(&self, type_name: &str) -> bool {
        self.builtin_natives.contains(type_name)
    }

    /// Check if a TypeId represents an array type.
    fn is_array_type_id(&self, type_id: u32) -> bool {
        type_id == TypeContext::ARRAY_TYPE_ID
    }

    /// Resolve a method type for a native type using extracted signatures.
    ///
    /// Given a type name (e.g., "Map"), method name (e.g., "get"), and concrete
    /// generic args (e.g., [string_id, number_id]), returns the fully resolved
    /// function TypeId for the method.
    pub fn resolve_method_type(
        &self,
        type_name: &str,
        method_name: &str,
        concrete_args: &[TypeId],
        type_ctx: &mut TypeContext,
    ) -> Option<TypeId> {
        let info = self.native_type_info.get(type_name)?;
        let method = info.methods.get(method_name)?;

        let resolved_params: Vec<TypeId> = method.params.iter()
            .map(|p| resolve_typesig(p, concrete_args, type_ctx))
            .collect();

        let resolved_ret = resolve_typesig(&method.return_type, concrete_args, type_ctx);

        Some(type_ctx.function_type(resolved_params, resolved_ret, method.is_async))
    }

    /// Get the NativeTypeInfo for a type.
    pub fn native_type_info(&self, type_name: &str) -> Option<&NativeTypeInfo> {
        self.native_type_info.get(type_name)
    }

    // ========================================================================
    // Type Normalization
    // ========================================================================

    /// Normalize a type to its canonical dispatch type.
    ///
    /// Maps structural types (Array<number>, string | null, etc.) to their
    /// canonical pre-interned TypeId for dispatch lookup.
    ///
    /// Returns Err for ambiguous unions (compile error per spec:
    /// "all union types during lowering must resolve to concrete types").
    pub fn normalize_type(&self, type_id: u32, type_ctx: &TypeContext) -> Result<u32, String> {
        // Pre-interned types are already canonical
        if type_id <= TypeContext::ARRAY_TYPE_ID || type_id == UNRESOLVED_TYPE_ID {
            return Ok(type_id);
        }

        let Some(ty) = type_ctx.get(TypeId::new(type_id)) else {
            return Ok(UNRESOLVED_TYPE_ID);
        };

        match ty {
            Type::Array(_) | Type::Tuple(_) => Ok(TypeContext::ARRAY_TYPE_ID),
            Type::Primitive(p) => {
                use crate::parser::types::ty::PrimitiveType as P;
                let name = match p {
                    P::Number => "number",
                    P::String => "string",
                    P::Boolean => "boolean",
                    P::Null => "null",
                    P::Void => "void",
                    P::Int => "int",
                };
                Ok(type_ctx.lookup_named_type(name)
                    .map(|id| id.as_u32())
                    .unwrap_or(UNRESOLVED_TYPE_ID))
            }
            Type::Json => Ok(lookup_or_unresolved(type_ctx, "Json")),
            Type::RegExp => Ok(lookup_or_unresolved(type_ctx, "RegExp")),
            Type::Mutex => Ok(lookup_or_unresolved(type_ctx, "Mutex")),
            Type::Date => Ok(lookup_or_unresolved(type_ctx, "Date")),
            Type::Buffer => Ok(lookup_or_unresolved(type_ctx, "Buffer")),
            Type::Task(_) => Ok(lookup_or_unresolved(type_ctx, "Task")),
            Type::Channel(_) => Ok(lookup_or_unresolved(type_ctx, "Channel")),
            Type::Map(_) => Ok(lookup_or_unresolved(type_ctx, "Map")),
            Type::Set(_) => Ok(lookup_or_unresolved(type_ctx, "Set")),
            Type::Union(_) => self.resolve_union_for_dispatch(type_id, type_ctx),
            Type::Class(class_type) => {
                // Map builtin primitive classes back to their canonical dispatch TypeId.
                // Must use the pre-interned IDs (from TypeContext::new()), not lookup_named_type,
                // because the binder may register a different ClassType under the same name.
                // For //@@builtin_native classes (Channel, Map, Set, Buffer), we use
                // lookup_named_type because the registry is also keyed by that same TypeId.
                match class_type.name.as_str() {
                    "number" => Ok(0),  // Pre-interned: Primitive(Number)
                    "string" => Ok(1),  // Pre-interned: Primitive(String)
                    "RegExp" => Ok(8),  // Pre-interned: Type::RegExp
                    "Array" => Ok(TypeContext::ARRAY_TYPE_ID), // 17
                    "Channel" => Ok(lookup_or_unresolved(type_ctx, "Channel")),
                    "Map" => Ok(lookup_or_unresolved(type_ctx, "Map")),
                    "Set" => Ok(lookup_or_unresolved(type_ctx, "Set")),
                    "Buffer" => Ok(lookup_or_unresolved(type_ctx, "Buffer")),
                    _ => Ok(UNRESOLVED_TYPE_ID),
                }
            }
            _ => Ok(UNRESOLVED_TYPE_ID),
        }
    }

    // ========================================================================
    // Union Resolution
    // ========================================================================

    /// Resolve a union type to its concrete dispatch type.
    ///
    /// Rules:
    /// 1. Strip null/void/never — they don't contribute to dispatch
    /// 2. Single remaining type → return it
    /// 3. number | int → number (int is a subtype for dispatch)
    /// 4. Multiple incompatible types → error
    /// 5. Non-union types → pass through unchanged
    pub fn resolve_union_for_dispatch(
        &self,
        type_id: u32,
        type_ctx: &TypeContext,
    ) -> Result<u32, String> {
        let Some(ty) = type_ctx.get(TypeId::new(type_id)) else {
            return Ok(type_id); // Unknown type, pass through
        };

        let Type::Union(union) = ty else {
            // Not a union — pass through
            return Ok(type_id);
        };

        // Collect TypeIds to strip: null, void, never
        let null_id = type_ctx.lookup_named_type("null").map(|id| id.as_u32());
        let void_id = type_ctx.lookup_named_type("void").map(|id| id.as_u32());
        let never_id = type_ctx.lookup_named_type("never").map(|id| id.as_u32());

        let mut candidates: Vec<u32> = Vec::new();

        for &member_id in &union.members {
            let mid = member_id.as_u32();

            // Strip null, void, never
            if Some(mid) == null_id || Some(mid) == void_id || Some(mid) == never_id {
                continue;
            }

            // Normalize each member (handles nested unions, object types → UNRESOLVED, etc.)
            let resolved = self.normalize_type(mid, type_ctx)?;
            if !candidates.contains(&resolved) {
                candidates.push(resolved);
            }
        }

        match candidates.len() {
            0 => {
                // All members were null/void/never — use null as the type
                Ok(null_id.unwrap_or(type_id))
            }
            1 => Ok(candidates[0]),
            _ => {
                // Check for number | int subsumption
                let num_id = type_ctx.lookup_named_type("number").map(|id| id.as_u32());
                let int_id = type_ctx.lookup_named_type("int").map(|id| id.as_u32());

                if candidates.len() == 2 {
                    if let (Some(nid), Some(iid)) = (num_id, int_id) {
                        if candidates.contains(&nid) && candidates.contains(&iid) {
                            return Ok(nid); // number | int → number
                        }
                    }
                }

                // Multiple incompatible types — build error message
                let type_names: Vec<String> = candidates
                    .iter()
                    .map(|&id| {
                        self.type_name(id)
                            .unwrap_or("unknown")
                            .to_string()
                    })
                    .collect();

                Err(format!(
                    "Cannot dispatch on ambiguous union type `{}`",
                    type_names.join(" | ")
                ))
            }
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Look up a named type, returning UNRESOLVED_TYPE_ID if not found.
fn lookup_or_unresolved(type_ctx: &TypeContext, name: &str) -> u32 {
    type_ctx.lookup_named_type(name)
        .map(|id| id.as_u32())
        .unwrap_or(UNRESOLVED_TYPE_ID)
}

// ============================================================================
// Source Scanner Functions
// ============================================================================

/// Extract `const NAME: number = VALUE;` declarations from source.
fn extract_constants(source: &str) -> FxHashMap<String, u16> {
    let mut constants = FxHashMap::default();

    for line in source.lines() {
        let trimmed = line.trim();
        // Match: const NAME: number = 0xNNNN;
        if let Some(rest) = trimmed.strip_prefix("const ") {
            if let Some(colon_idx) = rest.find(':') {
                let name = rest[..colon_idx].trim();
                // Find the = sign
                if let Some(eq_idx) = rest.find('=') {
                    let value_str = rest[eq_idx + 1..].trim().trim_end_matches(';').trim();
                    if let Some(value) = parse_number_literal(value_str) {
                        constants.insert(name.to_string(), value as u16);
                    }
                }
            }
        }
    }

    constants
}

/// Parse a number literal (hex or decimal) to u64.
fn parse_number_literal(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// Extract `//@@opcode KIND` annotations and the following property name.
fn extract_opcode_properties(source: &str) -> Vec<(String, OpcodeKind)> {
    let mut result = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for i in 0..lines.len() {
        let trimmed = lines[i].trim();
        if let Some(rest) = trimmed.strip_prefix("//@@opcode ") {
            let kind_name = rest.trim();
            let kind = match kind_name {
                "StringLen" => Some(OpcodeKind::StringLen),
                "ArrayLen" => Some(OpcodeKind::ArrayLen),
                _ => None,
            };

            if let Some(kind) = kind {
                // Next non-empty line should be the property declaration
                for line in lines.iter().skip(i + 1) {
                    let next = line.trim();
                    if next.is_empty() {
                        continue;
                    }
                    // Match: name: type;
                    if let Some(colon_idx) = next.find(':') {
                        let prop_name = next[..colon_idx].trim().to_string();
                        result.push((prop_name, kind));
                    }
                    break;
                }
            }
        }
    }

    result
}

/// Extract methods that are atomic (single `__NATIVE_CALL` return).
///
/// Returns (method_name, native_id, return_type_str) triples for methods whose body
/// is essentially `return __NATIVE_CALL(CONST, ...);` — these get native dispatch entries.
///
/// Methods with loops, conditionals, or other complex logic are skipped —
/// they use vtable dispatch instead.
fn extract_methods(source: &str, constants: &FxHashMap<String, u16>) -> Vec<(String, u16, Option<String>)> {
    let mut result = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut pos = 0;

    while pos < len {
        // Look for method declarations: name(...) {
        // Skip 'constructor' — handled separately
        // Skip 'class' keyword to avoid matching class name as method
        if try_match_keyword(&chars, pos, "class ") || try_match_keyword(&chars, pos, "const ") {
            // Skip to end of line
            while pos < len && chars[pos] != '\n' {
                pos += 1;
            }
            pos += 1;
            continue;
        }

        // Try to find a method pattern: identifier(
        if let Some((method_name, open_paren)) = try_extract_method_name(&chars, pos) {
            if method_name == "constructor" {
                // Skip the entire constructor body
                if let Some(open_brace) = find_char_after(&chars, open_paren, '{') {
                    if let Some(close_brace) = find_matching_brace(&chars, open_brace) {
                        pos = close_brace + 1;
                        continue;
                    }
                }
                pos = open_paren + 1;
                continue;
            }

            // Find the opening brace of the method body
            if let Some(open_brace) = find_char_after(&chars, open_paren, '{') {
                // Extract return type from between `)` and `{`
                let return_type_str = find_matching_paren(&chars, open_paren)
                    .and_then(|close_paren| {
                        let between: String = chars[close_paren + 1..open_brace].iter().collect();
                        between.find(':').map(|colon_idx| between[colon_idx + 1..].trim().to_string())
                    });

                // Find the matching closing brace
                if let Some(close_brace) = find_matching_brace(&chars, open_brace) {
                    let body = &chars[open_brace + 1..close_brace];
                    let body_str: String = body.iter().collect();

                    // Check if this is an atomic method (single __NATIVE_CALL return)
                    if let Some(native_id) = extract_native_call_id(&body_str, constants) {
                        // Only register if the body doesn't contain loops
                        if !body_str.contains("for ") && !body_str.contains("while ") {
                            result.push((method_name, native_id, return_type_str));
                        }
                    }

                    pos = close_brace + 1;
                    continue;
                }
            }
        }

        pos += 1;
    }

    result
}

/// Extract constructor's native ID if present.
fn extract_constructor(source: &str, constants: &FxHashMap<String, u16>) -> Option<u16> {
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut pos = 0;

    while pos < len {
        if try_match_keyword(&chars, pos, "constructor(") || try_match_keyword(&chars, pos, "constructor (") {
            // Find the opening brace
            if let Some(open_brace) = find_char_after(&chars, pos, '{') {
                if let Some(close_brace) = find_matching_brace(&chars, open_brace) {
                    let body: String = chars[open_brace + 1..close_brace].iter().collect();
                    return extract_native_call_id(&body, constants);
                }
            }
            return None;
        }
        pos += 1;
    }

    None
}

// ============================================================================
// Scanner Helpers
// ============================================================================

/// Check if chars at `pos` match the given keyword.
fn try_match_keyword(chars: &[char], pos: usize, keyword: &str) -> bool {
    let kw_chars: Vec<char> = keyword.chars().collect();
    if pos + kw_chars.len() > chars.len() {
        return false;
    }
    chars[pos..pos + kw_chars.len()] == kw_chars[..]
}

/// Try to extract a method name from an identifier followed by `(`.
/// Returns (method_name, position_of_open_paren).
fn try_extract_method_name(chars: &[char], pos: usize) -> Option<(String, usize)> {
    let len = chars.len();

    // Skip whitespace
    let mut i = pos;
    while i < len && chars[i].is_whitespace() {
        i += 1;
    }

    // Must start with a letter or underscore
    if i >= len || (!chars[i].is_alphabetic() && chars[i] != '_') {
        return None;
    }

    // Collect identifier
    let start = i;
    while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
        i += 1;
    }
    let name: String = chars[start..i].iter().collect();

    // Skip optional generic params <...>
    if i < len && chars[i] == '<' {
        let mut depth = 1;
        i += 1;
        while i < len && depth > 0 {
            if chars[i] == '<' {
                depth += 1;
            } else if chars[i] == '>' {
                depth -= 1;
            }
            i += 1;
        }
    }

    // Must be followed by `(`
    if i < len && chars[i] == '(' {
        // Filter out keywords
        match name.as_str() {
            "class" | "const" | "let" | "if" | "for" | "while" | "return" | "export" => None,
            _ => Some((name, i)),
        }
    } else {
        None
    }
}

/// Find the first occurrence of `ch` after position `start`.
fn find_char_after(chars: &[char], start: usize, ch: char) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i] == ch)
}

/// Find matching closing paren for an opening paren at `open`.
fn find_matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 1;
    let mut i = open + 1;
    while i < chars.len() && depth > 0 {
        match chars[i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Find the matching closing brace for an opening brace at `open`.
fn find_matching_brace(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 1;
    let mut i = open + 1;
    while i < chars.len() && depth > 0 {
        match chars[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Extract the native call ID from a method body string.
/// Looks for `__NATIVE_CALL(CONST_NAME, ...)` or `__NATIVE_CALL(CONST_NAME)`
/// and resolves CONST_NAME via the constants map.
fn extract_native_call_id(body: &str, constants: &FxHashMap<String, u16>) -> Option<u16> {
    let native_call_marker = "__NATIVE_CALL(";
    if let Some(idx) = body.find(native_call_marker) {
        let after = &body[idx + native_call_marker.len()..];
        // Extract the first argument (constant name) — delimited by ',' or ')'
        let end = after.find(',')
            .or_else(|| after.find(')'))?;
        let const_name = after[..end].trim();
        constants.get(const_name).copied()
    } else {
        None
    }
}

/// Extract method names annotated with `//@@class_method`.
///
/// Scans for `//@@class_method` comment lines and extracts the method name
/// from the following method declaration.
pub(crate) fn extract_class_method_names(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for i in 0..lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == "//@@class_method" {
            // Next non-empty line should be the method declaration
            for line in lines.iter().skip(i + 1) {
                let next = line.trim();
                if next.is_empty() {
                    continue;
                }
                // Extract method name: identifier before '('
                if let Some(paren_idx) = next.find('(') {
                    let name = next[..paren_idx].trim();
                    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        result.push(name.to_string());
                    }
                }
                break;
            }
        }
    }

    result
}

/// Resolve a return type string from a `.raya` method signature to a canonical TypeId.
///
/// Handles: `string`, `number`, `int`, `boolean`, `void`, `null`, `RegExp`,
/// array types (`T[]`, `string[]`, `string[][]`), and nullable types (`string[] | null`).
/// Returns None for generic types (T, U) or unrecognized types.
fn resolve_return_type_str(type_ctx: &TypeContext, return_type: &str) -> Option<u32> {
    let trimmed = return_type.trim();

    // Handle nullable: strip `| null` suffix
    let base = if let Some(stripped) = trimmed.strip_suffix("| null") {
        stripped.trim()
    } else {
        trimmed
    };

    // Handle array types: anything ending with `[]`
    if base.ends_with("[]") {
        return Some(TypeContext::ARRAY_TYPE_ID);
    }

    // Handle known types
    match base {
        "string" => type_ctx.lookup_named_type("string").map(|id| id.as_u32()),
        "number" => type_ctx.lookup_named_type("number").map(|id| id.as_u32()),
        "int" => type_ctx.lookup_named_type("int").map(|id| id.as_u32()),
        "boolean" => type_ctx.lookup_named_type("boolean").map(|id| id.as_u32()),
        "void" => type_ctx.lookup_named_type("void").map(|id| id.as_u32()),
        "null" => type_ctx.lookup_named_type("null").map(|id| id.as_u32()),
        "RegExp" => type_ctx.lookup_named_type("RegExp").map(|id| id.as_u32()),
        _ => None, // Generic types (T, U, etc.) — no propagation
    }
}

// ============================================================================
// Type Signatures (parsed from .raya AST)
// ============================================================================

/// A type signature extracted from a `.raya` builtin file.
///
/// Uses index-based generic param references that are resolved lazily
/// against a TypeContext with concrete type arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeSig {
    /// Named type: "string", "number", "boolean", "void", "null", "Buffer", etc.
    Named(String),
    /// Reference to a class generic param by index into NativeTypeInfo.generic_params
    GenericParam(usize),
    /// Array type: T[]
    Array(Box<TypeSig>),
    /// Tuple type: [K, V]
    Tuple(Vec<TypeSig>),
    /// Union type: V | null
    Union(Vec<TypeSig>),
    /// Function type: (value: V, key: K) => void
    Function { params: Vec<TypeSig>, return_type: Box<TypeSig> },
    /// Generic type reference: Set<T>, Map<K,V>, Channel<T>
    Generic(String, Vec<TypeSig>),
}

/// Method signature extracted from a `.raya` file.
#[derive(Debug, Clone)]
pub struct NativeMethodSig {
    pub params: Vec<TypeSig>,
    pub return_type: TypeSig,
    pub is_async: bool,
}

/// Type info extracted from a `.raya` builtin class declaration.
#[derive(Debug, Clone)]
pub struct NativeTypeInfo {
    /// Class name (e.g., "Map", "string", "Array")
    pub name: String,
    /// Generic type parameters (e.g., ["K", "V"] for Map, ["T"] for Set, [] for Buffer)
    pub generic_params: Vec<String>,
    /// Method signatures by name
    pub methods: FxHashMap<String, NativeMethodSig>,
    /// Constructor signature (if any)
    pub constructor: Option<NativeMethodSig>,
}

/// Extract type information from a `.raya` builtin source file.
///
/// Parses the source with the real Raya Parser, finds the class declaration,
/// and extracts generic params, method signatures, and constructor info.
fn extract_native_type_info(source: &str) -> Option<NativeTypeInfo> {
    use crate::parser::Parser;
    use crate::parser::ast::{Statement, ClassMember};

    let parser = Parser::new(source).ok()?;
    let (module, interner) = parser.parse().ok()?;

    // Find the first class declaration
    let class = module.statements.iter().find_map(|stmt| {
        if let Statement::ClassDecl(decl) = stmt { Some(decl) } else { None }
    })?;

    let name = interner.resolve(class.name.name).to_string();

    // Extract generic params
    let generic_params: Vec<String> = class.type_params.as_ref()
        .map(|params| params.iter()
            .map(|p| interner.resolve(p.name.name).to_string())
            .collect())
        .unwrap_or_default();

    let mut methods = FxHashMap::default();
    let mut constructor = None;

    for member in &class.members {
        match member {
            ClassMember::Method(method) => {
                if method.is_static {
                    continue;
                }
                let method_name = interner.resolve(method.name.name).to_string();

                let params: Vec<TypeSig> = method.params.iter()
                    .filter_map(|p| {
                        p.type_annotation.as_ref()
                            .map(|ann| ast_type_to_typesig(&ann.ty, &generic_params, &interner))
                    })
                    .collect();

                let return_type = method.return_type.as_ref()
                    .map(|ann| ast_type_to_typesig(&ann.ty, &generic_params, &interner))
                    .unwrap_or(TypeSig::Named("void".to_string()));

                methods.insert(method_name, NativeMethodSig {
                    params,
                    return_type,
                    is_async: method.is_async,
                });
            }
            ClassMember::Constructor(ctor) => {
                let params: Vec<TypeSig> = ctor.params.iter()
                    .filter_map(|p| {
                        p.type_annotation.as_ref()
                            .map(|ann| ast_type_to_typesig(&ann.ty, &generic_params, &interner))
                    })
                    .collect();

                constructor = Some(NativeMethodSig {
                    params,
                    return_type: TypeSig::Named("void".to_string()),
                    is_async: false,
                });
            }
            _ => {}
        }
    }

    Some(NativeTypeInfo {
        name,
        generic_params,
        methods,
        constructor,
    })
}

/// Convert an AST type annotation to a TypeSig.
fn ast_type_to_typesig(
    ty: &crate::parser::ast::types::Type,
    generic_params: &[String],
    interner: &crate::parser::Interner,
) -> TypeSig {
    use crate::parser::ast::types::Type as AstType;

    match ty {
        AstType::Primitive(p) => TypeSig::Named(p.name().to_string()),

        AstType::Reference(ref_) => {
            let name = interner.resolve(ref_.name.name).to_string();
            // Check if this is a generic param reference
            if let Some(idx) = generic_params.iter().position(|p| *p == name) {
                TypeSig::GenericParam(idx)
            } else if let Some(args) = &ref_.type_args {
                // Generic type reference with args: Map<K,V>, Set<T>, etc.
                let type_args: Vec<TypeSig> = args.iter()
                    .map(|a| ast_type_to_typesig(&a.ty, generic_params, interner))
                    .collect();
                TypeSig::Generic(name, type_args)
            } else {
                TypeSig::Named(name)
            }
        }

        AstType::Array(arr) => {
            TypeSig::Array(Box::new(ast_type_to_typesig(&arr.element_type.ty, generic_params, interner)))
        }

        AstType::Tuple(tup) => {
            TypeSig::Tuple(tup.element_types.iter()
                .map(|e| ast_type_to_typesig(&e.ty, generic_params, interner))
                .collect())
        }

        AstType::Union(union) => {
            TypeSig::Union(union.types.iter()
                .map(|t| ast_type_to_typesig(&t.ty, generic_params, interner))
                .collect())
        }

        AstType::Function(func) => {
            let params: Vec<TypeSig> = func.params.iter()
                .map(|p| ast_type_to_typesig(&p.ty.ty, generic_params, interner))
                .collect();
            let ret = ast_type_to_typesig(&func.return_type.ty, generic_params, interner);
            TypeSig::Function { params, return_type: Box::new(ret) }
        }

        AstType::Parenthesized(inner) => {
            ast_type_to_typesig(&inner.ty, generic_params, interner)
        }

        // Intersection, Object, Typeof, literals — unlikely in builtin .raya files
        _ => TypeSig::Named("unknown".to_string()),
    }
}

/// Resolve a TypeSig to a concrete TypeId given generic type arguments.
fn resolve_typesig(
    sig: &TypeSig,
    concrete_args: &[TypeId],
    type_ctx: &mut TypeContext,
) -> TypeId {
    match sig {
        TypeSig::Named(name) => resolve_named_type(name, type_ctx),

        TypeSig::GenericParam(idx) => {
            concrete_args.get(*idx).copied()
                .unwrap_or_else(|| type_ctx.unknown_type())
        }

        TypeSig::Array(inner) => {
            let elem = resolve_typesig(inner, concrete_args, type_ctx);
            type_ctx.array_type(elem)
        }

        TypeSig::Tuple(elems) => {
            let resolved: Vec<TypeId> = elems.iter()
                .map(|e| resolve_typesig(e, concrete_args, type_ctx))
                .collect();
            type_ctx.tuple_type(resolved)
        }

        TypeSig::Union(members) => {
            let resolved: Vec<TypeId> = members.iter()
                .map(|m| resolve_typesig(m, concrete_args, type_ctx))
                .collect();
            type_ctx.union_type(resolved)
        }

        TypeSig::Function { params, return_type } => {
            let resolved_params: Vec<TypeId> = params.iter()
                .map(|p| resolve_typesig(p, concrete_args, type_ctx))
                .collect();
            let resolved_ret = resolve_typesig(return_type, concrete_args, type_ctx);
            type_ctx.function_type(resolved_params, resolved_ret, false)
        }

        TypeSig::Generic(name, args) => {
            let resolved_args: Vec<TypeId> = args.iter()
                .map(|a| resolve_typesig(a, concrete_args, type_ctx))
                .collect();
            resolve_generic_type(name, &resolved_args, type_ctx)
        }
    }
}

/// Resolve a named type string to a TypeId.
fn resolve_named_type(name: &str, type_ctx: &mut TypeContext) -> TypeId {
    match name {
        "string" => type_ctx.string_type(),
        "number" => type_ctx.number_type(),
        "boolean" => type_ctx.boolean_type(),
        "void" => type_ctx.void_type(),
        "null" => type_ctx.null_type(),
        "never" => type_ctx.never_type(),
        "Buffer" => type_ctx.buffer_type(),
        "Date" => type_ctx.date_type(),
        "Mutex" => type_ctx.mutex_type(),
        "RegExp" => type_ctx.regexp_type(),
        _ => type_ctx.lookup_named_type(name).unwrap_or_else(|| type_ctx.unknown_type()),
    }
}

/// Resolve a generic type with concrete args to a TypeId.
fn resolve_generic_type(name: &str, args: &[TypeId], type_ctx: &mut TypeContext) -> TypeId {
    match (name, args.len()) {
        ("Set", 1) => type_ctx.set_type_with(args[0]),
        ("Map", 2) => type_ctx.map_type_with(args[0], args[1]),
        ("Channel", 1) => type_ctx.channel_type_with(args[0]),
        ("Task", 1) => type_ctx.task_type(args[0]),
        ("Array", 1) => type_ctx.array_type(args[0]),
        _ => type_ctx.unknown_type(),
    }
}

// ============================================================================
// Global Type Info (lazy, shared between TypeRegistry and checker)
// ============================================================================

/// Lazily extracted type info from all builtin `.raya` files.
/// Used by both the TypeRegistry (for dispatch) and the checker (for type resolution).
static NATIVE_TYPE_INFO: LazyLock<FxHashMap<String, NativeTypeInfo>> = LazyLock::new(|| {
    let mut map = FxHashMap::default();
    for &(_name, source) in BUILTIN_NATIVE_SOURCES.iter().chain(BUILTIN_CLASS_SOURCES.iter()) {
        if let Some(info) = extract_native_type_info(source) {
            map.insert(info.name.clone(), info);
        }
    }
    map
});

/// Resolve a method type for a native type.
///
/// Called by the checker to get fully resolved function TypeIds for methods
/// on builtin types (Map, Set, Channel, Task, Buffer, Date, Mutex, etc.).
///
/// Returns the method's function type with generic params substituted.
pub(crate) fn resolve_native_method_type(
    type_name: &str,
    method_name: &str,
    concrete_args: &[TypeId],
    type_ctx: &mut TypeContext,
) -> Option<TypeId> {
    let info = NATIVE_TYPE_INFO.get(type_name)?;
    let method = info.methods.get(method_name)?;

    let resolved_params: Vec<TypeId> = method.params.iter()
        .map(|p| resolve_typesig(p, concrete_args, type_ctx))
        .collect();

    let resolved_ret = resolve_typesig(&method.return_type, concrete_args, type_ctx);

    Some(type_ctx.function_type(resolved_params, resolved_ret, method.is_async))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_constants() {
        let source = r#"
const PUSH: number = 0x0100;
const POP: number = 0x0101;
const SOME_VAL: number = 42;
"#;
        let constants = extract_constants(source);
        assert_eq!(constants.get("PUSH"), Some(&0x0100));
        assert_eq!(constants.get("POP"), Some(&0x0101));
        assert_eq!(constants.get("SOME_VAL"), Some(&42));
    }

    #[test]
    fn test_extract_opcode_properties() {
        let source = r#"
class string {
    //@@opcode StringLen
    length: number;

    charAt(index: number): string {
        return __NATIVE_CALL(CHAR_AT, this, index);
    }
}
"#;
        let props = extract_opcode_properties(source);
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].0, "length");
        assert_eq!(props[0].1, OpcodeKind::StringLen);
    }

    #[test]
    fn test_extract_methods_atomic_only() {
        let source = r#"
const PUSH: number = 0x0100;
const MAP: number = 0x0112;

class Array<T> {
    push(element: T): number {
        return __NATIVE_CALL(PUSH, this, element);
    }

    map(fn: (element: T) => T): T[] {
        let result: T[] = [];
        for (let i = 0; i < this.length; i++) {
            result.push(fn(this[i]));
        }
        return result;
    }
}
"#;
        let constants = extract_constants(source);
        let methods = extract_methods(source, &constants);

        // push should be extracted (atomic), map should NOT (has for loop)
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].0, "push");
        assert_eq!(methods[0].1, 0x0100);
        assert_eq!(methods[0].2.as_deref(), Some("number"));
    }

    #[test]
    fn test_extract_constructor() {
        let source = r#"
const ARRAY_NEW: number = 0x0116;

class Array<T> {
    constructor() {
        __NATIVE_CALL(ARRAY_NEW, this);
    }
}
"#;
        let constants = extract_constants(source);
        let ctor = extract_constructor(source, &constants);
        assert_eq!(ctor, Some(0x0116));
    }

    #[test]
    fn test_registry_new() {
        let type_ctx = TypeContext::new();
        let registry = TypeRegistry::new(&type_ctx);

        // Verify string methods
        let str_id = type_ctx.lookup_named_type("string").unwrap().as_u32();
        assert!(registry.lookup_method(str_id, "charAt").is_some());
        assert!(registry.lookup_method(str_id, "substring").is_some());
        assert!(registry.lookup_method(str_id, "trim").is_some());
        assert!(registry.lookup_method(str_id, "indexOf").is_some(), "indexOf should be registered for string");
        assert!(registry.lookup_method(str_id, "split").is_some(), "split should be registered for string");
        assert!(registry.lookup_property(str_id, "length").is_some());

        // replaceWith should be a ClassMethod (Raya loop, not NativeCall)
        assert!(matches!(registry.lookup_method(str_id, "replaceWith"),
            Some(DispatchAction::ClassMethod(_, _))));

        // Verify Array methods
        let arr_id = TypeContext::ARRAY_TYPE_ID;
        assert!(registry.lookup_method(arr_id, "push").is_some());
        assert!(registry.lookup_method(arr_id, "pop").is_some());
        assert!(registry.lookup_property(arr_id, "length").is_some());

        // Callback methods should be ClassMethods (Raya loops, not NativeCall)
        assert!(matches!(registry.lookup_method(arr_id, "map"),
            Some(DispatchAction::ClassMethod(_, _))));
        assert!(matches!(registry.lookup_method(arr_id, "filter"),
            Some(DispatchAction::ClassMethod(_, _))));
        assert!(matches!(registry.lookup_method(arr_id, "forEach"),
            Some(DispatchAction::ClassMethod(_, _))));

        // Verify constructors
        assert!(registry.constructor_native_id("Array").is_some());
        assert!(registry.constructor_native_id("RegExp").is_some());
        assert!(registry.constructor_native_id("string").is_none());

        // Verify number methods
        let num_id = type_ctx.lookup_named_type("number").unwrap().as_u32();
        assert!(registry.lookup_method(num_id, "toFixed").is_some());

        // Verify int shares number dispatch
        let int_id = type_ctx.lookup_named_type("int").unwrap().as_u32();
        assert!(registry.lookup_method(int_id, "toFixed").is_some());

        // Verify RegExp methods
        let re_id = type_ctx.lookup_named_type("RegExp").unwrap().as_u32();
        assert!(registry.lookup_method(re_id, "test").is_some());
        assert!(registry.lookup_method(re_id, "exec").is_some());
        // replaceWith should be a ClassMethod
        assert!(matches!(registry.lookup_method(re_id, "replaceWith"),
            Some(DispatchAction::ClassMethod(_, _))));
    }

    #[test]
    fn test_union_resolution_simple() {
        let mut type_ctx = TypeContext::new();

        // string | null → string
        let str_id = type_ctx.lookup_named_type("string").unwrap();
        let null_id = type_ctx.lookup_named_type("null").unwrap();
        let union_id = type_ctx.union_type(vec![str_id, null_id]);

        let registry = TypeRegistry::new(&type_ctx);
        let result = registry.resolve_union_for_dispatch(union_id.as_u32(), &type_ctx);
        assert_eq!(result, Ok(str_id.as_u32()));
    }

    #[test]
    fn test_union_resolution_number_int() {
        let mut type_ctx = TypeContext::new();

        // number | int → number
        let num_id = type_ctx.lookup_named_type("number").unwrap();
        let int_id = type_ctx.lookup_named_type("int").unwrap();
        let union_id = type_ctx.union_type(vec![num_id, int_id]);

        let registry = TypeRegistry::new(&type_ctx);
        let result = registry.resolve_union_for_dispatch(union_id.as_u32(), &type_ctx);
        assert_eq!(result, Ok(num_id.as_u32()));
    }

    #[test]
    fn test_union_resolution_ambiguous() {
        let mut type_ctx = TypeContext::new();

        // string | number → error
        let str_id = type_ctx.lookup_named_type("string").unwrap();
        let num_id = type_ctx.lookup_named_type("number").unwrap();
        let union_id = type_ctx.union_type(vec![str_id, num_id]);

        let registry = TypeRegistry::new(&type_ctx);
        let result = registry.resolve_union_for_dispatch(union_id.as_u32(), &type_ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ambiguous union"));
    }

    #[test]
    fn test_union_resolution_strip_void_never() {
        let mut type_ctx = TypeContext::new();

        // string | null | void → string
        let str_id = type_ctx.lookup_named_type("string").unwrap();
        let null_id = type_ctx.lookup_named_type("null").unwrap();
        let void_id = type_ctx.lookup_named_type("void").unwrap();
        let union_id = type_ctx.union_type(vec![str_id, null_id, void_id]);

        let registry = TypeRegistry::new(&type_ctx);
        let result = registry.resolve_union_for_dispatch(union_id.as_u32(), &type_ctx);
        assert_eq!(result, Ok(str_id.as_u32()));
    }

    #[test]
    fn test_type_name_lookup() {
        let type_ctx = TypeContext::new();
        let registry = TypeRegistry::new(&type_ctx);

        let str_id = type_ctx.lookup_named_type("string").unwrap().as_u32();
        assert_eq!(registry.type_name(str_id), Some("string"));

        let arr_id = type_ctx.lookup_named_type("Array").unwrap().as_u32();
        assert_eq!(registry.type_name(arr_id), Some("Array"));
    }

    #[test]
    fn test_return_type_propagation() {
        let type_ctx = TypeContext::new();
        let registry = TypeRegistry::new(&type_ctx);

        let str_id = type_ctx.lookup_named_type("string").unwrap().as_u32();
        let num_id = type_ctx.lookup_named_type("number").unwrap().as_u32();
        let bool_id = type_ctx.lookup_named_type("boolean").unwrap().as_u32();
        let int_id = type_ctx.lookup_named_type("int").unwrap().as_u32();

        // String methods — return types from string.raya
        assert_eq!(registry.lookup_return_type(0x0200), Some(str_id));   // charAt → string
        assert_eq!(registry.lookup_return_type(0x0201), Some(str_id));   // substring → string
        assert_eq!(registry.lookup_return_type(0x0205), Some(int_id));   // indexOf → int
        assert_eq!(registry.lookup_return_type(0x0206), Some(bool_id));  // includes → boolean
        assert_eq!(registry.lookup_return_type(0x020E), Some(int_id));   // charCodeAt → int
        assert_eq!(registry.lookup_return_type(0x0212), Some(TypeContext::ARRAY_TYPE_ID)); // match → string[] | null → Array
        assert_eq!(registry.lookup_return_type(0x0213), Some(TypeContext::ARRAY_TYPE_ID)); // matchAll → string[][] → Array
        assert_eq!(registry.lookup_return_type(0x0214), Some(int_id));   // search → int

        // Variant IDs (compiler-internal remaps)
        assert_eq!(registry.lookup_return_type(0x0215), Some(str_id));   // REPLACE_REGEXP → string
        assert_eq!(registry.lookup_return_type(0x0216), Some(TypeContext::ARRAY_TYPE_ID)); // SPLIT_REGEXP → string[]

        // Number methods — return types from number.raya
        assert_eq!(registry.lookup_return_type(0x0F00), Some(str_id));   // toFixed → string
        assert_eq!(registry.lookup_return_type(0x0F01), Some(str_id));   // toPrecision → string
        assert_eq!(registry.lookup_return_type(0x0F02), Some(str_id));   // toString → string

        // RegExp methods — return types from regexp.raya
        assert_eq!(registry.lookup_return_type(0x0A01), Some(bool_id));  // test → boolean

        // Array push → return type is number (from array.raya)
        assert_eq!(registry.lookup_return_type(0x0100), Some(num_id));   // push → number
    }

    #[test]
    fn test_extract_class_method_names() {
        let source = r#"
class Array<T> {
    push(element: T): number {
        return __NATIVE_CALL(PUSH, this, element);
    }

    //@@class_method
    map(fn: (element: T) => T): T[] {
        let result: T[] = [];
        for (let i = 0; i < this.length; i++) {
            result.push(fn(this[i]));
        }
        return result;
    }

    //@@class_method
    filter(predicate: (element: T) => boolean): T[] {
        let result: T[] = [];
        for (let i = 0; i < this.length; i++) {
            if (predicate(this[i])) {
                result.push(this[i]);
            }
        }
        return result;
    }
}
"#;
        let class_methods = extract_class_method_names(source);
        assert_eq!(class_methods, vec!["map", "filter"]);
    }

    // ====================================================================
    // TypeSig extraction tests
    // ====================================================================

    #[test]
    fn test_extract_map_type_info() {
        let source = include_str!("../../builtins/map.raya");
        let info = extract_native_type_info(source).unwrap();

        assert_eq!(info.name, "Map");
        assert_eq!(info.generic_params, vec!["K", "V"]);

        // Check methods exist
        assert!(info.methods.contains_key("get"));
        assert!(info.methods.contains_key("set"));
        assert!(info.methods.contains_key("has"));
        assert!(info.methods.contains_key("delete"));
        assert!(info.methods.contains_key("clear"));
        assert!(info.methods.contains_key("keys"));
        assert!(info.methods.contains_key("values"));
        assert!(info.methods.contains_key("entries"));
        assert!(info.methods.contains_key("size"));

        // get(key: K): V | null
        let get = &info.methods["get"];
        assert_eq!(get.params, vec![TypeSig::GenericParam(0)]); // K
        assert_eq!(get.return_type, TypeSig::Union(vec![
            TypeSig::GenericParam(1), // V
            TypeSig::Named("null".to_string()),
        ]));

        // entries(): [K, V][]
        let entries = &info.methods["entries"];
        assert!(entries.params.is_empty());
        assert_eq!(entries.return_type, TypeSig::Array(Box::new(
            TypeSig::Tuple(vec![TypeSig::GenericParam(0), TypeSig::GenericParam(1)])
        )));
    }

    #[test]
    fn test_extract_channel_type_info() {
        let source = include_str!("../../builtins/channel.raya");
        let info = extract_native_type_info(source).unwrap();

        assert_eq!(info.name, "Channel");
        assert_eq!(info.generic_params, vec!["T"]);

        // send(value: T): void
        let send = &info.methods["send"];
        assert_eq!(send.params, vec![TypeSig::GenericParam(0)]); // T
        assert_eq!(send.return_type, TypeSig::Named("void".to_string()));

        // tryReceive(): T | null
        let try_receive = &info.methods["tryReceive"];
        assert!(try_receive.params.is_empty());
        assert_eq!(try_receive.return_type, TypeSig::Union(vec![
            TypeSig::GenericParam(0), // T
            TypeSig::Named("null".to_string()),
        ]));

        // Constructor: constructor(capacity: number)
        let ctor = info.constructor.as_ref().unwrap();
        assert_eq!(ctor.params, vec![TypeSig::Named("number".to_string())]);
    }

    #[test]
    fn test_extract_buffer_type_info() {
        let source = include_str!("../../builtins/buffer.raya");
        let info = extract_native_type_info(source).unwrap();

        assert_eq!(info.name, "Buffer");
        assert!(info.generic_params.is_empty()); // No generics

        // length(): number
        let length = &info.methods["length"];
        assert!(length.params.is_empty());
        assert_eq!(length.return_type, TypeSig::Named("number".to_string()));
    }

    #[test]
    fn test_extract_set_type_info() {
        let source = include_str!("../../builtins/set.raya");
        let info = extract_native_type_info(source).unwrap();

        assert_eq!(info.name, "Set");
        assert_eq!(info.generic_params, vec!["T"]);

        // union(other: Set<T>): Set<T>
        let union_method = &info.methods["union"];
        assert_eq!(union_method.params, vec![
            TypeSig::Generic("Set".to_string(), vec![TypeSig::GenericParam(0)])
        ]);
        assert_eq!(union_method.return_type,
            TypeSig::Generic("Set".to_string(), vec![TypeSig::GenericParam(0)])
        );
    }

    #[test]
    fn test_extract_all_builtin_sources() {
        // Verify all builtin sources parse and extract successfully
        for &(type_name, source) in BUILTIN_NATIVE_SOURCES.iter().chain(BUILTIN_CLASS_SOURCES.iter()) {
            let info = extract_native_type_info(source);
            assert!(info.is_some(), "Failed to extract type info from {}", type_name);
        }
    }

    #[test]
    fn test_resolve_method_type_map() {
        let mut type_ctx = TypeContext::new();
        let registry = TypeRegistry::new(&type_ctx);

        let str_id = type_ctx.string_type();
        let num_id = type_ctx.number_type();

        // Map<string, number>.get(key: string) -> number | null
        let get_type = registry.resolve_method_type("Map", "get", &[str_id, num_id], &mut type_ctx);
        assert!(get_type.is_some(), "Map.get should resolve");

        // Map<string, number>.keys() -> string[]
        let keys_type = registry.resolve_method_type("Map", "keys", &[str_id, num_id], &mut type_ctx);
        assert!(keys_type.is_some(), "Map.keys should resolve");

        // Map<string, number>.set(key: string, value: number) -> void
        let set_type = registry.resolve_method_type("Map", "set", &[str_id, num_id], &mut type_ctx);
        assert!(set_type.is_some(), "Map.set should resolve");

        // Map<string, number>.entries() -> [string, number][]
        let entries_type = registry.resolve_method_type("Map", "entries", &[str_id, num_id], &mut type_ctx);
        assert!(entries_type.is_some(), "Map.entries should resolve");
    }

    #[test]
    fn test_resolve_method_type_channel() {
        let mut type_ctx = TypeContext::new();
        let registry = TypeRegistry::new(&type_ctx);

        let str_id = type_ctx.string_type();

        // Channel<string>.send(value: string) -> void
        let send_type = registry.resolve_method_type("Channel", "send", &[str_id], &mut type_ctx);
        assert!(send_type.is_some(), "Channel.send should resolve");

        // Channel<string>.tryReceive() -> string | null
        let try_recv = registry.resolve_method_type("Channel", "tryReceive", &[str_id], &mut type_ctx);
        assert!(try_recv.is_some(), "Channel.tryReceive should resolve");
    }

    #[test]
    fn test_resolve_method_type_set_union() {
        let mut type_ctx = TypeContext::new();
        let registry = TypeRegistry::new(&type_ctx);

        let num_id = type_ctx.number_type();

        // Set<number>.union(other: Set<number>) -> Set<number>
        let union_type = registry.resolve_method_type("Set", "union", &[num_id], &mut type_ctx);
        assert!(union_type.is_some(), "Set.union should resolve");
    }

    #[test]
    fn test_resolve_nonexistent_returns_none() {
        let mut type_ctx = TypeContext::new();
        let registry = TypeRegistry::new(&type_ctx);

        assert!(registry.resolve_method_type("Map", "nonexistent", &[], &mut type_ctx).is_none());
        assert!(registry.resolve_method_type("NonexistentType", "get", &[], &mut type_ctx).is_none());
    }

    #[test]
    fn test_registry_has_type_info() {
        let type_ctx = TypeContext::new();
        let registry = TypeRegistry::new(&type_ctx);

        // All builtin types should have type info
        assert!(registry.native_type_info("Map").is_some());
        assert!(registry.native_type_info("Set").is_some());
        assert!(registry.native_type_info("Channel").is_some());
        assert!(registry.native_type_info("Buffer").is_some());
        assert!(registry.native_type_info("Date").is_some());
        assert!(registry.native_type_info("Mutex").is_some());
        assert!(registry.native_type_info("Task").is_some());
        assert!(registry.native_type_info("string").is_some());
        assert!(registry.native_type_info("Array").is_some());
    }

    #[test]
    fn test_resolve_return_type_str() {
        let type_ctx = TypeContext::new();

        let str_id = type_ctx.lookup_named_type("string").unwrap().as_u32();
        let num_id = type_ctx.lookup_named_type("number").unwrap().as_u32();
        let bool_id = type_ctx.lookup_named_type("boolean").unwrap().as_u32();

        assert_eq!(resolve_return_type_str(&type_ctx, "string"), Some(str_id));
        assert_eq!(resolve_return_type_str(&type_ctx, "number"), Some(num_id));
        assert_eq!(resolve_return_type_str(&type_ctx, "boolean"), Some(bool_id));
        assert_eq!(resolve_return_type_str(&type_ctx, "string[]"), Some(TypeContext::ARRAY_TYPE_ID));
        assert_eq!(resolve_return_type_str(&type_ctx, "string[][]"), Some(TypeContext::ARRAY_TYPE_ID));
        assert_eq!(resolve_return_type_str(&type_ctx, "string[] | null"), Some(TypeContext::ARRAY_TYPE_ID));
        assert_eq!(resolve_return_type_str(&type_ctx, "T"), None); // Generic — no propagation
    }
}
