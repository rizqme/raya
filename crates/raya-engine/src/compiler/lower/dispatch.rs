//! Dispatch Registry
//!
//! Data-driven dispatch for type-specific methods and properties.
//! Instead of hardcoded if-else chains, the registry maps
//! (TypeId, name) → DispatchAction.
//!
//! Dispatch order for method calls:
//! 1. Class instance → vtable method dispatch
//! 2. Class reference → static method dispatch
//! 3. Registry lookup → opcode or native call dispatch
//! 4. Fallback → closure call

use rustc_hash::FxHashMap;
use crate::parser::TypeContext;
use crate::vm::builtin::{array, buffer, channel, date, map, mutex, number, regexp, set, string, task};

// ============================================================================
// Dispatch Action Types
// ============================================================================

/// How to dispatch a type-specific method or property access.
#[derive(Debug, Clone, Copy)]
pub enum DispatchAction {
    /// Emit a specialized opcode directly (e.g., StringLen, ArrayLen)
    Opcode(OpcodeKind),
    /// Emit CallMethod with this builtin method ID (VM handles natively)
    NativeCall(u16),
}

/// Specialized opcodes for property/method access.
#[derive(Debug, Clone, Copy)]
pub enum OpcodeKind {
    StringLen,
    ArrayLen,
}

// ============================================================================
// Dispatch Registry
// ============================================================================

/// Dispatch entries for a single type.
struct TypeDispatch {
    properties: FxHashMap<&'static str, DispatchAction>,
    methods: FxHashMap<&'static str, DispatchAction>,
}

impl TypeDispatch {
    fn new() -> Self {
        Self {
            properties: FxHashMap::default(),
            methods: FxHashMap::default(),
        }
    }
}

/// Registry mapping types to their dispatch entries.
///
/// The registry is data-driven: adding support for a new type's methods
/// only requires adding entries here, not changing dispatch logic.
pub struct DispatchRegistry {
    /// Dispatch entries keyed by exact TypeId
    by_type: FxHashMap<u32, TypeDispatch>,
    /// Dispatch for array types (any TypeId that is an array)
    array_dispatch: TypeDispatch,
}

/// Helper: register methods into a TypeDispatch from a slice of (name, native_id) pairs.
fn register_methods(dispatch: &mut TypeDispatch, methods: &[(&'static str, u16)]) {
    for &(name, id) in methods {
        dispatch.methods.insert(name, DispatchAction::NativeCall(id));
    }
}

impl DispatchRegistry {
    /// Build the dispatch registry using TypeContext for type ID lookups.
    pub fn new(type_ctx: &TypeContext) -> Self {
        let mut registry = Self {
            by_type: FxHashMap::default(),
            array_dispatch: TypeDispatch::new(),
        };

        registry.init_number(type_ctx);
        registry.init_string(type_ctx);
        registry.init_regexp(type_ctx);
        registry.init_array();
        registry.init_map(type_ctx);
        registry.init_set(type_ctx);
        registry.init_channel(type_ctx);
        registry.init_buffer(type_ctx);
        registry.init_date(type_ctx);
        registry.init_mutex(type_ctx);
        registry.init_task(type_ctx);

        registry
    }

    /// Look up a property dispatch for a given type.
    pub fn lookup_property(&self, type_id: u32, name: &str) -> Option<DispatchAction> {
        // Exact type match
        if let Some(dispatch) = self.by_type.get(&type_id) {
            if let Some(action) = dispatch.properties.get(name) {
                return Some(*action);
            }
        }
        // Array type check
        if is_array_type(type_id) {
            if let Some(action) = self.array_dispatch.properties.get(name) {
                return Some(*action);
            }
        }
        None
    }

    /// Look up a method dispatch for a given type.
    pub fn lookup_method(&self, type_id: u32, name: &str) -> Option<DispatchAction> {
        // Exact type match
        if let Some(dispatch) = self.by_type.get(&type_id) {
            if let Some(action) = dispatch.methods.get(name) {
                return Some(*action);
            }
        }
        // Array type check
        if is_array_type(type_id) {
            if let Some(action) = self.array_dispatch.methods.get(name) {
                return Some(*action);
            }
        }
        None
    }

    // ── Number and Int ──────────────────────────────────────────────────

    fn init_number(&mut self, type_ctx: &TypeContext) {
        let methods: &[(&str, u16)] = &[
            ("toFixed", number::TO_FIXED),
            ("toPrecision", number::TO_PRECISION),
            ("toString", number::TO_STRING_RADIX),
        ];

        // Both Number and Int share the same dispatch
        let num_id = type_ctx.lookup_named_type("number");
        let int_id = type_ctx.lookup_named_type("int");
        for type_id in [num_id, int_id].into_iter().flatten() {
            let mut dispatch = TypeDispatch::new();
            register_methods(&mut dispatch, methods);
            self.by_type.insert(type_id.as_u32(), dispatch);
        }
    }

    // ── String ──────────────────────────────────────────────────────────

    fn init_string(&mut self, type_ctx: &TypeContext) {
        let Some(str_id) = type_ctx.lookup_named_type("string") else { return };
        let mut dispatch = TypeDispatch::new();

        dispatch.properties.insert("length", DispatchAction::Opcode(OpcodeKind::StringLen));

        register_methods(&mut dispatch, &[
            ("charAt", string::CHAR_AT),
            ("substring", string::SUBSTRING),
            ("toUpperCase", string::TO_UPPER_CASE),
            ("toLowerCase", string::TO_LOWER_CASE),
            ("trim", string::TRIM),
            ("indexOf", string::INDEX_OF),
            ("includes", string::INCLUDES),
            ("split", string::SPLIT),
            ("startsWith", string::STARTS_WITH),
            ("endsWith", string::ENDS_WITH),
            ("replace", string::REPLACE),
            ("repeat", string::REPEAT),
            ("padStart", string::PAD_START),
            ("padEnd", string::PAD_END),
            ("charCodeAt", string::CHAR_CODE_AT),
            ("lastIndexOf", string::LAST_INDEX_OF),
            ("trimStart", string::TRIM_START),
            ("trimEnd", string::TRIM_END),
            ("match", string::MATCH),
            ("matchAll", string::MATCH_ALL),
            ("search", string::SEARCH),
            ("replaceWith", string::REPLACE_WITH_REGEXP),
        ]);

        self.by_type.insert(str_id.as_u32(), dispatch);
    }

    // ── RegExp ──────────────────────────────────────────────────────────

    fn init_regexp(&mut self, type_ctx: &TypeContext) {
        let Some(re_id) = type_ctx.lookup_named_type("RegExp") else { return };
        let mut dispatch = TypeDispatch::new();

        register_methods(&mut dispatch, &[
            ("test", regexp::TEST),
            ("exec", regexp::EXEC),
            ("execAll", regexp::EXEC_ALL),
            ("replace", regexp::REPLACE),
            ("replaceWith", regexp::REPLACE_WITH),
            ("split", regexp::SPLIT),
        ]);

        self.by_type.insert(re_id.as_u32(), dispatch);
    }

    // ── Array ───────────────────────────────────────────────────────────

    fn init_array(&mut self) {
        self.array_dispatch.properties.insert("length", DispatchAction::Opcode(OpcodeKind::ArrayLen));

        register_methods(&mut self.array_dispatch, &[
            ("push", array::PUSH),
            ("pop", array::POP),
            ("shift", array::SHIFT),
            ("unshift", array::UNSHIFT),
            ("indexOf", array::INDEX_OF),
            ("includes", array::INCLUDES),
            ("slice", array::SLICE),
            ("concat", array::CONCAT),
            ("reverse", array::REVERSE),
            ("join", array::JOIN),
            ("forEach", array::FOR_EACH),
            ("filter", array::FILTER),
            ("find", array::FIND),
            ("findIndex", array::FIND_INDEX),
            ("every", array::EVERY),
            ("some", array::SOME),
            ("lastIndexOf", array::LAST_INDEX_OF),
            ("sort", array::SORT),
            ("map", array::MAP),
            ("reduce", array::REDUCE),
            ("fill", array::FILL),
            ("flat", array::FLAT),
        ]);
    }

    // ── Map ─────────────────────────────────────────────────────────────

    fn init_map(&mut self, type_ctx: &TypeContext) {
        let Some(map_id) = type_ctx.lookup_named_type("Map") else { return };
        let mut dispatch = TypeDispatch::new();

        register_methods(&mut dispatch, &[
            ("get", map::GET),
            ("set", map::SET),
            ("has", map::HAS),
            ("delete", map::DELETE),
            ("clear", map::CLEAR),
            ("keys", map::KEYS),
            ("values", map::VALUES),
            ("entries", map::ENTRIES),
            ("forEach", map::FOR_EACH),
            ("size", map::SIZE),
        ]);

        self.by_type.insert(map_id.as_u32(), dispatch);
    }

    // ── Set ─────────────────────────────────────────────────────────────

    fn init_set(&mut self, type_ctx: &TypeContext) {
        let Some(set_id) = type_ctx.lookup_named_type("Set") else { return };
        let mut dispatch = TypeDispatch::new();

        register_methods(&mut dispatch, &[
            ("add", set::ADD),
            ("has", set::HAS),
            ("delete", set::DELETE),
            ("clear", set::CLEAR),
            ("values", set::VALUES),
            ("forEach", set::FOR_EACH),
            ("size", set::SIZE),
            ("union", set::UNION),
            ("intersection", set::INTERSECTION),
            ("difference", set::DIFFERENCE),
        ]);

        self.by_type.insert(set_id.as_u32(), dispatch);
    }

    // ── Channel ─────────────────────────────────────────────────────────

    fn init_channel(&mut self, type_ctx: &TypeContext) {
        let Some(ch_id) = type_ctx.lookup_named_type("Channel") else { return };
        let mut dispatch = TypeDispatch::new();

        register_methods(&mut dispatch, &[
            ("send", channel::SEND),
            ("receive", channel::RECEIVE),
            ("trySend", channel::TRY_SEND),
            ("tryReceive", channel::TRY_RECEIVE),
            ("close", channel::CLOSE),
            ("isClosed", channel::IS_CLOSED),
            ("length", channel::LENGTH),
            ("capacity", channel::CAPACITY),
        ]);

        self.by_type.insert(ch_id.as_u32(), dispatch);
    }

    // ── Buffer ──────────────────────────────────────────────────────────

    fn init_buffer(&mut self, type_ctx: &TypeContext) {
        let Some(buf_id) = type_ctx.lookup_named_type("Buffer") else { return };
        let mut dispatch = TypeDispatch::new();

        register_methods(&mut dispatch, &[
            ("length", buffer::LENGTH),
            ("getByte", buffer::GET_BYTE),
            ("setByte", buffer::SET_BYTE),
            ("getInt32", buffer::GET_INT32),
            ("setInt32", buffer::SET_INT32),
            ("getFloat64", buffer::GET_FLOAT64),
            ("setFloat64", buffer::SET_FLOAT64),
            ("slice", buffer::SLICE),
            ("copy", buffer::COPY),
            ("toString", buffer::TO_STRING),
        ]);

        self.by_type.insert(buf_id.as_u32(), dispatch);
    }

    // ── Date ────────────────────────────────────────────────────────────

    fn init_date(&mut self, type_ctx: &TypeContext) {
        let Some(date_id) = type_ctx.lookup_named_type("Date") else { return };
        let mut dispatch = TypeDispatch::new();

        register_methods(&mut dispatch, &[
            ("getTime", date::GET_TIME),
            ("getFullYear", date::GET_FULL_YEAR),
            ("getMonth", date::GET_MONTH),
            ("getDate", date::GET_DATE),
            ("getDay", date::GET_DAY),
            ("getHours", date::GET_HOURS),
            ("getMinutes", date::GET_MINUTES),
            ("getSeconds", date::GET_SECONDS),
            ("getMilliseconds", date::GET_MILLISECONDS),
            ("setFullYear", date::SET_FULL_YEAR),
            ("setMonth", date::SET_MONTH),
            ("setDate", date::SET_DATE),
            ("setHours", date::SET_HOURS),
            ("setMinutes", date::SET_MINUTES),
            ("setSeconds", date::SET_SECONDS),
            ("setMilliseconds", date::SET_MILLISECONDS),
            ("toString", date::TO_STRING),
            ("toISOString", date::TO_ISO_STRING),
            ("toDateString", date::TO_DATE_STRING),
            ("toTimeString", date::TO_TIME_STRING),
        ]);

        self.by_type.insert(date_id.as_u32(), dispatch);
    }

    // ── Mutex ───────────────────────────────────────────────────────────

    fn init_mutex(&mut self, type_ctx: &TypeContext) {
        let Some(mtx_id) = type_ctx.lookup_named_type("Mutex") else { return };
        let mut dispatch = TypeDispatch::new();

        register_methods(&mut dispatch, &[
            ("tryLock", mutex::TRY_LOCK),
            ("isLocked", mutex::IS_LOCKED),
        ]);

        self.by_type.insert(mtx_id.as_u32(), dispatch);
    }

    // ── Task ────────────────────────────────────────────────────────────

    fn init_task(&mut self, type_ctx: &TypeContext) {
        let Some(task_id) = type_ctx.lookup_named_type("Task") else { return };
        let mut dispatch = TypeDispatch::new();

        register_methods(&mut dispatch, &[
            ("isDone", task::IS_DONE),
            ("isCancelled", task::IS_CANCELLED),
        ]);

        self.by_type.insert(task_id.as_u32(), dispatch);
    }
}

/// Check if a TypeId represents an array type.
/// Only matches the canonical ARRAY_TYPE_ID (17).
/// Dynamic array types (e.g., Array<number>) should be normalized to 17
/// via `Lowerer::normalize_type_for_dispatch` before registry lookup.
fn is_array_type(type_id: u32) -> bool {
    type_id == super::ARRAY_TYPE_ID
}

/// Array callback methods that are compiler intrinsics (inlined as loops).
pub const ARRAY_INTRINSIC_METHODS: &[u16] = &[
    array::MAP,
    array::FILTER,
    array::REDUCE,
    array::FOR_EACH,
    array::FIND,
    array::FIND_INDEX,
    array::SOME,
    array::EVERY,
    array::SORT,
];

/// Replace-with methods that are compiler intrinsics (inlined with callback).
pub const REPLACE_WITH_INTRINSIC_METHODS: &[u16] = &[
    string::REPLACE_WITH_REGEXP,
    regexp::REPLACE_WITH,
];
