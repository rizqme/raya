//! JSON and TOML parsing/serialization (std:encoding)
//!
//! Handle-based approach: parsed data stays in Rust as serde_json::Value,
//! Raya code navigates it via native calls. Same pattern as CSV/XML.

use parking_lot::Mutex;
use raya_sdk::{NativeCallResult, NativeContext, NativeValue};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

// ============================================================================
// Handle Registry
// ============================================================================

struct ValueRegistry {
    map: Mutex<HashMap<u64, JsonValue>>,
    next_id: AtomicU64,
}

impl ValueRegistry {
    fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn insert(&self, value: JsonValue) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.map.lock().insert(id, value);
        id
    }

    fn with<F, R>(&self, id: u64, f: F) -> Option<R>
    where
        F: FnOnce(&JsonValue) -> R,
    {
        self.map.lock().get(&id).map(f)
    }

    fn remove(&self, id: u64) {
        self.map.lock().remove(&id);
    }
}

static VALUES: LazyLock<ValueRegistry> = LazyLock::new(ValueRegistry::new);

fn get_handle(args: &[NativeValue], index: usize) -> u64 {
    args.get(index)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64
}

// ============================================================================
// JSON Operations
// ============================================================================

/// encoding.jsonParse(input: string) -> handle
pub fn json_parse(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("encoding.jsonParse requires 1 argument".into());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("encoding.jsonParse: {}", e)),
    };
    match serde_json::from_str::<JsonValue>(&input) {
        Ok(val) => NativeCallResult::f64(VALUES.insert(val) as f64),
        Err(e) => NativeCallResult::Error(format!("encoding.jsonParse: {}", e)),
    }
}

/// encoding.jsonStringify(handle) -> string
pub fn json_stringify(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| serde_json::to_string(val)) {
        Some(Ok(s)) => NativeCallResult::Value(ctx.create_string(&s)),
        Some(Err(e)) => NativeCallResult::Error(format!("encoding.jsonStringify: {}", e)),
        None => NativeCallResult::Error("encoding.jsonStringify: invalid handle".into()),
    }
}

/// encoding.jsonStringifyPretty(handle) -> string
pub fn json_stringify_pretty(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| serde_json::to_string_pretty(val)) {
        Some(Ok(s)) => NativeCallResult::Value(ctx.create_string(&s)),
        Some(Err(e)) => NativeCallResult::Error(format!("encoding.jsonStringifyPretty: {}", e)),
        None => NativeCallResult::Error("encoding.jsonStringifyPretty: invalid handle".into()),
    }
}

/// encoding.jsonGet(handle, key: string) -> handle (child value, cloned)
pub fn json_get(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("encoding.jsonGet requires 2 arguments".into());
    }
    let handle = get_handle(args, 0);
    let key = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("encoding.jsonGet: {}", e)),
    };
    match VALUES.with(handle, |val| {
        val.get(&key).cloned().unwrap_or(JsonValue::Null)
    }) {
        Some(child) => NativeCallResult::f64(VALUES.insert(child) as f64),
        None => NativeCallResult::Error("encoding.jsonGet: invalid handle".into()),
    }
}

/// encoding.jsonAt(handle, index: number) -> handle (array element, cloned)
pub fn json_at(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("encoding.jsonAt requires 2 arguments".into());
    }
    let handle = get_handle(args, 0);
    let index = args
        .get(1)
        .and_then(|v| v.as_i32().or_else(|| v.as_f64().map(|f| f as i32)))
        .unwrap_or(-1);
    if index < 0 {
        return NativeCallResult::Error("encoding.jsonAt: invalid index".into());
    }
    match VALUES.with(handle, |val| {
        val.get(index as usize).cloned().unwrap_or(JsonValue::Null)
    }) {
        Some(child) => NativeCallResult::f64(VALUES.insert(child) as f64),
        None => NativeCallResult::Error("encoding.jsonAt: invalid handle".into()),
    }
}

/// encoding.jsonString(handle) -> string
pub fn json_string(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| match val {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Null => String::new(),
        _ => val.to_string(),
    }) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("encoding.jsonString: invalid handle".into()),
    }
}

/// encoding.jsonNumber(handle) -> number
pub fn json_number(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| val.as_f64().unwrap_or(0.0)) {
        Some(n) => NativeCallResult::f64(n),
        None => NativeCallResult::Error("encoding.jsonNumber: invalid handle".into()),
    }
}

/// encoding.jsonBool(handle) -> boolean
pub fn json_bool(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| val.as_bool().unwrap_or(false)) {
        Some(b) => NativeCallResult::bool(b),
        None => NativeCallResult::Error("encoding.jsonBool: invalid handle".into()),
    }
}

/// encoding.jsonIsNull(handle) -> boolean
pub fn json_is_null(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| val.is_null()) {
        Some(b) => NativeCallResult::bool(b),
        None => NativeCallResult::Error("encoding.jsonIsNull: invalid handle".into()),
    }
}

/// encoding.jsonType(handle) -> string ("string"|"number"|"boolean"|"null"|"array"|"object")
pub fn json_type(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| match val {
        JsonValue::String(_) => "string",
        JsonValue::Number(_) => "number",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Null => "null",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }) {
        Some(t) => NativeCallResult::Value(ctx.create_string(t)),
        None => NativeCallResult::Error("encoding.jsonType: invalid handle".into()),
    }
}

/// encoding.jsonKeys(handle) -> string[]
pub fn json_keys(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| {
        if let JsonValue::Object(map) = val {
            map.keys().map(|k| ctx.create_string(k)).collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    }) {
        Some(items) => NativeCallResult::Value(ctx.create_array(&items)),
        None => NativeCallResult::Error("encoding.jsonKeys: invalid handle".into()),
    }
}

/// encoding.jsonLength(handle) -> number
pub fn json_length(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| match val {
        JsonValue::Array(arr) => arr.len(),
        JsonValue::Object(map) => map.len(),
        JsonValue::String(s) => s.len(),
        _ => 0,
    }) {
        Some(n) => NativeCallResult::f64(n as f64),
        None => NativeCallResult::Error("encoding.jsonLength: invalid handle".into()),
    }
}

/// encoding.jsonRelease(handle) -> void
pub fn json_release(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    VALUES.remove(handle);
    NativeCallResult::null()
}

// ============================================================================
// TOML Operations
// ============================================================================

/// Convert a toml::Value to serde_json::Value
fn toml_to_json(val: toml::Value) -> JsonValue {
    match val {
        toml::Value::String(s) => JsonValue::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => JsonValue::Bool(b),
        toml::Value::Datetime(d) => JsonValue::String(d.to_string()),
        toml::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map: serde_json::Map<String, JsonValue> = table
                .into_iter()
                .map(|(k, v)| (k, toml_to_json(v)))
                .collect();
            JsonValue::Object(map)
        }
    }
}

/// Convert serde_json::Value back to toml::Value
fn json_to_toml(val: &JsonValue) -> toml::Value {
    match val {
        JsonValue::String(s) => toml::Value::String(s.clone()),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                toml::Value::Integer(0)
            }
        }
        JsonValue::Bool(b) => toml::Value::Boolean(*b),
        JsonValue::Null => toml::Value::String(String::new()),
        JsonValue::Array(arr) => {
            toml::Value::Array(arr.iter().map(json_to_toml).collect())
        }
        JsonValue::Object(map) => {
            let table: toml::map::Map<String, toml::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_toml(v)))
                .collect();
            toml::Value::Table(table)
        }
    }
}

/// encoding.tomlParse(input: string) -> handle
pub fn toml_parse(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("encoding.tomlParse requires 1 argument".into());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("encoding.tomlParse: {}", e)),
    };
    match input.parse::<toml::Value>() {
        Ok(val) => {
            let json_val = toml_to_json(val);
            NativeCallResult::f64(VALUES.insert(json_val) as f64)
        }
        Err(e) => NativeCallResult::Error(format!("encoding.tomlParse: {}", e)),
    }
}

/// encoding.tomlStringify(handle) -> string
pub fn toml_stringify(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match VALUES.with(handle, |val| {
        let toml_val = json_to_toml(val);
        toml::to_string_pretty(&toml_val)
    }) {
        Some(Ok(s)) => NativeCallResult::Value(ctx.create_string(&s)),
        Some(Err(e)) => NativeCallResult::Error(format!("encoding.tomlStringify: {}", e)),
        None => NativeCallResult::Error("encoding.tomlStringify: invalid handle".into()),
    }
}

/// encoding.jsonFromValue(str: string, num: number, bool: boolean, isNull: boolean) -> handle
/// Creates a JSON value handle from a primitive. Used by Raya code to construct JSON for tomlStringify.
pub fn json_from_string(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("encoding.jsonFromString requires 1 argument".into());
    }
    let s = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("encoding.jsonFromString: {}", e)),
    };
    NativeCallResult::f64(VALUES.insert(JsonValue::String(s)) as f64)
}

/// encoding.jsonFromNumber(n: number) -> handle
pub fn json_from_number(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let n = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0);
    NativeCallResult::f64(VALUES.insert(serde_json::json!(n)) as f64)
}

/// encoding.jsonFromBool(b: boolean) -> handle
pub fn json_from_bool(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let b = args.first().and_then(|v| v.as_bool()).unwrap_or(false);
    NativeCallResult::f64(VALUES.insert(JsonValue::Bool(b)) as f64)
}

/// encoding.jsonNull() -> handle
pub fn json_null(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(VALUES.insert(JsonValue::Null) as f64)
}

/// encoding.jsonNewObject() -> handle
pub fn json_new_object(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(
        VALUES.insert(JsonValue::Object(serde_json::Map::new())) as f64,
    )
}

/// encoding.jsonNewArray() -> handle
pub fn json_new_array(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(VALUES.insert(JsonValue::Array(Vec::new())) as f64)
}

/// encoding.jsonSet(objectHandle, key: string, valueHandle) -> void
pub fn json_set(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 3 {
        return NativeCallResult::Error("encoding.jsonSet requires 3 arguments".into());
    }
    let obj_handle = get_handle(args, 0);
    let key = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("encoding.jsonSet: {}", e)),
    };
    let val_handle = get_handle(args, 2);

    // Clone the value first
    let value = match VALUES.with(val_handle, |v| v.clone()) {
        Some(v) => v,
        None => return NativeCallResult::Error("encoding.jsonSet: invalid value handle".into()),
    };

    // Set on the object
    let mut map = VALUES.map.lock();
    match map.get_mut(&obj_handle) {
        Some(JsonValue::Object(obj)) => {
            obj.insert(key, value);
            NativeCallResult::null()
        }
        Some(_) => NativeCallResult::Error("encoding.jsonSet: handle is not an object".into()),
        None => NativeCallResult::Error("encoding.jsonSet: invalid object handle".into()),
    }
}

/// encoding.jsonPush(arrayHandle, valueHandle) -> void
pub fn json_push(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("encoding.jsonPush requires 2 arguments".into());
    }
    let arr_handle = get_handle(args, 0);
    let val_handle = get_handle(args, 1);

    let value = match VALUES.with(val_handle, |v| v.clone()) {
        Some(v) => v,
        None => return NativeCallResult::Error("encoding.jsonPush: invalid value handle".into()),
    };

    let mut map = VALUES.map.lock();
    match map.get_mut(&arr_handle) {
        Some(JsonValue::Array(arr)) => {
            arr.push(value);
            NativeCallResult::null()
        }
        Some(_) => NativeCallResult::Error("encoding.jsonPush: handle is not an array".into()),
        None => NativeCallResult::Error("encoding.jsonPush: invalid array handle".into()),
    }
}
