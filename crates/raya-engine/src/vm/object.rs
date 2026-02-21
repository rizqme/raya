//! Object model and class system

use crate::vm::value::Value;
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for generating unique object IDs
static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(1);

/// Generate a new unique object ID
fn generate_object_id() -> u64 {
    NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Object instance (heap-allocated)
#[derive(Debug, Clone)]
pub struct Object {
    /// Unique object ID (assigned on creation, used for hashCode/equals)
    pub object_id: u64,
    /// Class ID (index into VM class registry)
    pub class_id: usize,
    /// Field values
    pub fields: Vec<Value>,
}

impl Object {
    /// Create a new object with uninitialized fields
    pub fn new(class_id: usize, field_count: usize) -> Self {
        Self {
            object_id: generate_object_id(),
            class_id,
            fields: vec![Value::null(); field_count],
        }
    }

    /// Get a field value by index
    pub fn get_field(&self, index: usize) -> Option<Value> {
        self.fields.get(index).copied()
    }

    /// Set a field value by index
    pub fn set_field(&mut self, index: usize, value: Value) -> Result<(), String> {
        if index < self.fields.len() {
            self.fields[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Field index {} out of bounds (object has {} fields)",
                index,
                self.fields.len()
            ))
        }
    }

    /// Get number of fields
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}

/// Class definition metadata
#[derive(Debug, Clone)]
pub struct Class {
    /// Class ID (unique identifier)
    pub id: usize,
    /// Class name
    pub name: String,
    /// Number of instance fields (including inherited)
    pub field_count: usize,
    /// Parent class ID (None for root classes)
    pub parent_id: Option<usize>,
    /// Virtual method table
    pub vtable: VTable,
    /// Static fields (class-level, shared across all instances)
    pub static_fields: Vec<Value>,
    /// Constructor function ID (None if no explicit constructor)
    pub constructor_id: Option<usize>,
}

impl Class {
    /// Create a new class
    pub fn new(id: usize, name: String, field_count: usize) -> Self {
        Self {
            id,
            name,
            field_count,
            parent_id: None,
            vtable: VTable::new(),
            static_fields: Vec::new(),
            constructor_id: None,
        }
    }

    /// Create a new class with parent
    pub fn with_parent(id: usize, name: String, field_count: usize, parent_id: usize) -> Self {
        Self {
            id,
            name,
            field_count,
            parent_id: Some(parent_id),
            vtable: VTable::new(),
            static_fields: Vec::new(),
            constructor_id: None,
        }
    }

    /// Create a new class with static fields
    pub fn with_static_fields(
        id: usize,
        name: String,
        field_count: usize,
        static_field_count: usize,
    ) -> Self {
        Self {
            id,
            name,
            field_count,
            parent_id: None,
            vtable: VTable::new(),
            static_fields: vec![Value::null(); static_field_count],
            constructor_id: None,
        }
    }

    /// Set the constructor function ID
    pub fn set_constructor(&mut self, function_id: usize) {
        self.constructor_id = Some(function_id);
    }

    /// Get the constructor function ID
    pub fn get_constructor(&self) -> Option<usize> {
        self.constructor_id
    }

    /// Get a static field value by index
    pub fn get_static_field(&self, index: usize) -> Option<Value> {
        self.static_fields.get(index).copied()
    }

    /// Set a static field value by index
    pub fn set_static_field(&mut self, index: usize, value: Value) -> Result<(), String> {
        if index < self.static_fields.len() {
            self.static_fields[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Static field index {} out of bounds (class has {} static fields)",
                index,
                self.static_fields.len()
            ))
        }
    }

    /// Get number of static fields
    pub fn static_field_count(&self) -> usize {
        self.static_fields.len()
    }

    /// Add a method to the vtable
    pub fn add_method(&mut self, function_id: usize) {
        self.vtable.add_method(function_id);
    }

    /// Get method from vtable
    pub fn get_method(&self, method_index: usize) -> Option<usize> {
        self.vtable.get_method(method_index)
    }
}

/// Virtual method table for dynamic dispatch
#[derive(Debug, Clone)]
pub struct VTable {
    /// Method function IDs (indexed by method slot)
    pub methods: Vec<usize>,
}

impl VTable {
    /// Create a new empty vtable
    pub fn new() -> Self {
        Self {
            methods: Vec::new(),
        }
    }

    /// Add a method to the vtable (appends to end)
    pub fn add_method(&mut self, function_id: usize) {
        self.methods.push(function_id);
    }

    /// Get method function ID by index
    pub fn get_method(&self, index: usize) -> Option<usize> {
        self.methods.get(index).copied()
    }

    /// Get number of methods
    pub fn method_count(&self) -> usize {
        self.methods.len()
    }

    /// Override a method at specific index
    pub fn override_method(&mut self, index: usize, function_id: usize) -> Result<(), String> {
        if index < self.methods.len() {
            self.methods[index] = function_id;
            Ok(())
        } else {
            Err(format!("Method index {} out of bounds", index))
        }
    }
}

impl Default for VTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Array object (heap-allocated)
#[derive(Debug, Clone)]
pub struct Array {
    /// Element type ID (for type checking)
    pub type_id: usize,
    /// Array elements
    pub elements: Vec<Value>,
}

impl Array {
    /// Create a new array with given length
    pub fn new(type_id: usize, length: usize) -> Self {
        Self {
            type_id,
            elements: vec![Value::null(); length],
        }
    }

    /// Get array length
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Check if array is empty
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Get element at index
    pub fn get(&self, index: usize) -> Option<Value> {
        self.elements.get(index).copied()
    }

    /// Set element at index
    pub fn set(&mut self, index: usize, value: Value) -> Result<(), String> {
        if index < self.elements.len() {
            self.elements[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Array index {} out of bounds (length: {})",
                index,
                self.elements.len()
            ))
        }
    }

    /// Push element to end of array, returns new length
    pub fn push(&mut self, value: Value) -> usize {
        self.elements.push(value);
        self.elements.len()
    }

    /// Pop element from end of array
    pub fn pop(&mut self) -> Option<Value> {
        self.elements.pop()
    }

    /// Shift element from beginning of array
    pub fn shift(&mut self) -> Option<Value> {
        if self.elements.is_empty() {
            None
        } else {
            Some(self.elements.remove(0))
        }
    }

    /// Unshift element to beginning of array, returns new length
    pub fn unshift(&mut self, value: Value) -> usize {
        self.elements.insert(0, value);
        self.elements.len()
    }

    /// Find index of value, returns -1 if not found
    pub fn index_of(&self, value: Value) -> i32 {
        for (i, elem) in self.elements.iter().enumerate() {
            // Use equality check - Value implements PartialEq
            if *elem == value {
                return i as i32;
            }
        }
        -1
    }

    /// Check if array contains value
    pub fn includes(&self, value: Value) -> bool {
        self.index_of(value) >= 0
    }
}

/// Closure object (heap-allocated)
///
/// A closure captures the function ID and any captured variables from
/// the enclosing scope. When the closure is called, the captured values
/// are available to the function body.
#[derive(Debug, Clone)]
pub struct Closure {
    /// Function ID (index into module's function table)
    pub func_id: usize,
    /// Captured variable values
    pub captures: Vec<Value>,
}

impl Closure {
    /// Create a new closure with captured variables
    pub fn new(func_id: usize, captures: Vec<Value>) -> Self {
        Self { func_id, captures }
    }

    /// Get the function ID
    pub fn func_id(&self) -> usize {
        self.func_id
    }

    /// Get a captured variable by index
    pub fn get_captured(&self, index: usize) -> Option<Value> {
        self.captures.get(index).copied()
    }

    /// Set a captured variable by index
    pub fn set_captured(&mut self, index: usize, value: Value) -> Result<(), String> {
        if index < self.captures.len() {
            self.captures[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Captured variable index {} out of bounds (closure has {} captures)",
                index,
                self.captures.len()
            ))
        }
    }

    /// Get number of captured variables
    pub fn capture_count(&self) -> usize {
        self.captures.len()
    }
}

/// A method bound to its receiver object (heap-allocated).
///
/// Created when accessing `obj.method` as a value (not a call).
/// When called, the receiver is automatically passed as `this` (locals[0]).
#[derive(Debug, Clone)]
pub struct BoundMethod {
    /// The receiver object (becomes `this`)
    pub receiver: Value,
    /// Function ID of the method (resolved from vtable at bind time)
    pub func_id: usize,
}

/// RefCell - A heap-allocated mutable cell for capture-by-reference semantics
///
/// When a variable is captured by a closure AND modified (either in the closure
/// or in the outer scope), both need to share the same storage. RefCell provides
/// this shared mutable storage - both the outer scope and closure hold a pointer
/// to the same RefCell, and all reads/writes go through it.
#[derive(Debug, Clone)]
pub struct RefCell {
    /// The contained value
    pub value: Value,
}

impl RefCell {
    /// Create a new RefCell with an initial value
    pub fn new(value: Value) -> Self {
        Self { value }
    }

    /// Get the current value
    pub fn get(&self) -> Value {
        self.value
    }

    /// Set a new value
    pub fn set(&mut self, value: Value) {
        self.value = value;
    }
}

/// String object (heap-allocated) with cached metadata for fast comparison
///
/// The hash is computed lazily on first comparison and cached for O(1)
/// subsequent access. This enables the multi-level SEQ optimization:
/// 1. Pointer equality (O(1))
/// 2. Length check (O(1))
/// 3. Hash check (O(1) after first computation)
/// 4. Character comparison (O(n)) - only if all else fails
pub struct RayaString {
    /// UTF-8 string data
    pub data: String,
    /// Cached hash (computed lazily on first comparison)
    hash: Cell<Option<u64>>,
}

impl std::fmt::Debug for RayaString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RayaString")
            .field("data", &self.data)
            .field("hash", &self.hash.get())
            .finish()
    }
}

impl Clone for RayaString {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            // Copy cached hash if available
            hash: Cell::new(self.hash.get()),
        }
    }
}

impl RayaString {
    /// Create a new string
    pub fn new(data: String) -> Self {
        Self {
            data,
            hash: Cell::new(None),
        }
    }

    /// Get string length (in bytes)
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if string is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get or compute hash (O(n) first time, O(1) subsequent)
    pub fn hash(&self) -> u64 {
        if let Some(h) = self.hash.get() {
            return h;
        }
        let h = self.compute_hash();
        self.hash.set(Some(h));
        h
    }

    /// Compute hash using FxHasher for speed
    fn compute_hash(&self) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        self.data.hash(&mut hasher);
        hasher.finish()
    }

    /// Concatenate two strings
    pub fn concat(&self, other: &RayaString) -> RayaString {
        RayaString::new(format!("{}{}", self.data, other.data))
    }
}

// ============================================================================
// Builtin collection types
// ============================================================================

/// Wrapper type for Value that implements Hash and Eq for use in HashMap/HashSet
///
/// For pointer values (like strings), this compares by content rather than pointer address.
/// For primitive values, it uses raw bit comparison.
#[derive(Clone, Copy, Debug)]
pub struct HashableValue(pub Value);

impl HashableValue {
    /// Try to get the string content if this value is a RayaString pointer
    fn try_as_string(&self) -> Option<&str> {
        if self.0.is_ptr() {
            // Safety: We're checking is_ptr() first, and in the context of Map/Set
            // we expect pointer values to be RayaString when used as keys
            let ptr = unsafe { self.0.as_ptr::<RayaString>() };
            if let Some(non_null) = ptr {
                // Safety: The pointer was just verified to be non-null
                let raya_str = unsafe { &*non_null.as_ptr() };
                return Some(&raya_str.data);
            }
        }
        None
    }
}

impl Hash for HashableValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // For string pointers, hash the string content
        if let Some(s) = self.try_as_string() {
            // Hash a discriminator first to distinguish strings from raw values
            1u8.hash(state);
            s.hash(state);
        } else {
            // For primitive values (numbers, booleans, null), use raw bits
            0u8.hash(state);
            self.0.raw().hash(state);
        }
    }
}

impl PartialEq for HashableValue {
    fn eq(&self, other: &Self) -> bool {
        // First try to compare as strings (by content)
        match (self.try_as_string(), other.try_as_string()) {
            (Some(s1), Some(s2)) => s1 == s2,
            (None, None) => self.0 == other.0, // Both are primitives, use raw comparison
            _ => false, // One is string, one is not
        }
    }
}

impl Eq for HashableValue {}

/// Map builtin - generic key-value store
/// Native IDs: 0x0800-0x080A
#[derive(Debug, Clone)]
pub struct MapObject {
    /// Internal HashMap storage
    pub inner: HashMap<HashableValue, Value>,
}

impl MapObject {
    /// Create a new empty map
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Get the number of entries
    pub fn size(&self) -> usize {
        self.inner.len()
    }

    /// Get a value by key
    pub fn get(&self, key: Value) -> Option<Value> {
        self.inner.get(&HashableValue(key)).copied()
    }

    /// Set a key-value pair
    pub fn set(&mut self, key: Value, value: Value) {
        self.inner.insert(HashableValue(key), value);
    }

    /// Check if key exists
    pub fn has(&self, key: Value) -> bool {
        self.inner.contains_key(&HashableValue(key))
    }

    /// Delete a key, returns true if key existed
    pub fn delete(&mut self, key: Value) -> bool {
        self.inner.remove(&HashableValue(key)).is_some()
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Get all keys as a vector
    pub fn keys(&self) -> Vec<Value> {
        self.inner.keys().map(|k| k.0).collect()
    }

    /// Get all values as a vector
    pub fn values(&self) -> Vec<Value> {
        self.inner.values().copied().collect()
    }

    /// Get all entries as key-value pairs
    pub fn entries(&self) -> Vec<(Value, Value)> {
        self.inner.iter().map(|(k, v)| (k.0, *v)).collect()
    }
}

impl Default for MapObject {
    fn default() -> Self {
        Self::new()
    }
}

/// Set builtin - collection of unique values
/// Native IDs: 0x0900-0x090A
#[derive(Debug, Clone)]
pub struct SetObject {
    /// Internal HashSet storage
    pub inner: HashSet<HashableValue>,
}

impl SetObject {
    /// Create a new empty set
    pub fn new() -> Self {
        Self {
            inner: HashSet::new(),
        }
    }

    /// Get the number of elements
    pub fn size(&self) -> usize {
        self.inner.len()
    }

    /// Add a value to the set
    pub fn add(&mut self, value: Value) {
        self.inner.insert(HashableValue(value));
    }

    /// Check if value exists
    pub fn has(&self, value: Value) -> bool {
        self.inner.contains(&HashableValue(value))
    }

    /// Delete a value, returns true if value existed
    pub fn delete(&mut self, value: Value) -> bool {
        self.inner.remove(&HashableValue(value))
    }

    /// Clear all elements
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Get all values as a vector
    pub fn values(&self) -> Vec<Value> {
        self.inner.iter().map(|v| v.0).collect()
    }
}

impl Default for SetObject {
    fn default() -> Self {
        Self::new()
    }
}

/// Buffer builtin - raw binary data buffer
/// Native IDs: 0x0700-0x070B
#[derive(Debug, Clone)]
pub struct Buffer {
    /// Raw byte data
    pub data: Vec<u8>,
}

impl Buffer {
    /// Create a new buffer of given size (zero-filled)
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0; size],
        }
    }

    /// Get buffer length in bytes
    pub fn length(&self) -> usize {
        self.data.len()
    }

    /// Get byte at index
    pub fn get_byte(&self, index: usize) -> Option<u8> {
        self.data.get(index).copied()
    }

    /// Set byte at index
    pub fn set_byte(&mut self, index: usize, value: u8) -> Result<(), String> {
        if index < self.data.len() {
            self.data[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Buffer index {} out of bounds (length: {})",
                index,
                self.data.len()
            ))
        }
    }

    /// Get 32-bit signed integer at index (little-endian)
    pub fn get_int32(&self, index: usize) -> Option<i32> {
        if index + 4 <= self.data.len() {
            let bytes = [
                self.data[index],
                self.data[index + 1],
                self.data[index + 2],
                self.data[index + 3],
            ];
            Some(i32::from_le_bytes(bytes))
        } else {
            None
        }
    }

    /// Set 32-bit signed integer at index (little-endian)
    pub fn set_int32(&mut self, index: usize, value: i32) -> Result<(), String> {
        if index + 4 <= self.data.len() {
            let bytes = value.to_le_bytes();
            self.data[index..index + 4].copy_from_slice(&bytes);
            Ok(())
        } else {
            Err(format!(
                "Buffer index {} out of bounds for int32 (length: {})",
                index,
                self.data.len()
            ))
        }
    }

    /// Get 64-bit float at index (little-endian)
    pub fn get_float64(&self, index: usize) -> Option<f64> {
        if index + 8 <= self.data.len() {
            let bytes = [
                self.data[index],
                self.data[index + 1],
                self.data[index + 2],
                self.data[index + 3],
                self.data[index + 4],
                self.data[index + 5],
                self.data[index + 6],
                self.data[index + 7],
            ];
            Some(f64::from_le_bytes(bytes))
        } else {
            None
        }
    }

    /// Set 64-bit float at index (little-endian)
    pub fn set_float64(&mut self, index: usize, value: f64) -> Result<(), String> {
        if index + 8 <= self.data.len() {
            let bytes = value.to_le_bytes();
            self.data[index..index + 8].copy_from_slice(&bytes);
            Ok(())
        } else {
            Err(format!(
                "Buffer index {} out of bounds for float64 (length: {})",
                index,
                self.data.len()
            ))
        }
    }

    /// Create a slice of this buffer (returns new buffer)
    pub fn slice(&self, start: usize, end: usize) -> Buffer {
        let end = end.min(self.data.len());
        let start = start.min(end);
        Buffer {
            data: self.data[start..end].to_vec(),
        }
    }
}

/// RegExp builtin - regular expression pattern
/// Native IDs: 0x0A01-0x0A06
#[derive(Debug, Clone)]
pub struct RegExpObject {
    /// The pattern string
    pub pattern: String,
    /// Regex flags (g=global, i=ignoreCase, m=multiline)
    pub flags: String,
    /// Compiled regex (using Rust regex crate)
    pub compiled: regex::Regex,
}

impl RegExpObject {
    /// Create a new RegExp from pattern and flags
    pub fn new(pattern: &str, flags: &str) -> Result<Self, String> {
        // Build regex pattern with flags
        let mut regex_pattern = String::new();

        // Handle flags
        let case_insensitive = flags.contains('i');
        let multiline = flags.contains('m');

        if case_insensitive || multiline {
            regex_pattern.push_str("(?");
            if case_insensitive {
                regex_pattern.push('i');
            }
            if multiline {
                regex_pattern.push('m');
            }
            regex_pattern.push(')');
        }

        regex_pattern.push_str(pattern);

        let compiled = regex::Regex::new(&regex_pattern)
            .map_err(|e| format!("Invalid regular expression: {}", e))?;

        Ok(Self {
            pattern: pattern.to_string(),
            flags: flags.to_string(),
            compiled,
        })
    }

    /// Get the source pattern
    pub fn source(&self) -> &str {
        &self.pattern
    }

    /// Get the flags string
    pub fn flags(&self) -> &str {
        &self.flags
    }

    /// Check if global flag is set
    pub fn global(&self) -> bool {
        self.flags.contains('g')
    }

    /// Check if case-insensitive flag is set
    pub fn ignore_case(&self) -> bool {
        self.flags.contains('i')
    }

    /// Check if multiline flag is set
    pub fn multiline(&self) -> bool {
        self.flags.contains('m')
    }

    /// Test if pattern matches string
    pub fn test(&self, text: &str) -> bool {
        self.compiled.is_match(text)
    }

    /// Execute pattern on string, return first match
    /// Returns (matched_text, index, groups) or None
    pub fn exec(&self, text: &str) -> Option<(String, usize, Vec<String>)> {
        self.compiled.captures(text).map(|caps| {
            let full_match = caps.get(0).unwrap();
            let matched_text = full_match.as_str().to_string();
            let index = full_match.start();

            // Collect captured groups (skip group 0 which is the full match)
            let groups: Vec<String> = caps.iter()
                .skip(1)
                .map(|m| m.map(|m| m.as_str().to_string()).unwrap_or_default())
                .collect();

            (matched_text, index, groups)
        })
    }

    /// Execute pattern on string, return all matches
    pub fn exec_all(&self, text: &str) -> Vec<(String, usize, Vec<String>)> {
        self.compiled.captures_iter(text).map(|caps| {
            let full_match = caps.get(0).unwrap();
            let matched_text = full_match.as_str().to_string();
            let index = full_match.start();

            let groups: Vec<String> = caps.iter()
                .skip(1)
                .map(|m| m.map(|m| m.as_str().to_string()).unwrap_or_default())
                .collect();

            (matched_text, index, groups)
        }).collect()
    }

    /// Replace first match (or all if global)
    pub fn replace(&self, text: &str, replacement: &str) -> String {
        if self.global() {
            self.compiled.replace_all(text, replacement).to_string()
        } else {
            self.compiled.replace(text, replacement).to_string()
        }
    }

    /// Split string by pattern
    pub fn split(&self, text: &str, limit: Option<usize>) -> Vec<String> {
        match limit {
            Some(n) => self.compiled.splitn(text, n).map(|s| s.to_string()).collect(),
            None => self.compiled.split(text).map(|s| s.to_string()).collect(),
        }
    }
}

/// Date builtin - date and time handling
/// Native IDs: 0x0B00-0x0B23
#[derive(Debug, Clone, Copy)]
pub struct DateObject {
    /// Timestamp in milliseconds since Unix epoch
    pub timestamp_ms: i64,
}

impl DateObject {
    /// Create a new date with current time
    pub fn now() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self { timestamp_ms }
    }

    /// Create a date from timestamp (milliseconds since epoch)
    pub fn from_timestamp(timestamp_ms: i64) -> Self {
        Self { timestamp_ms }
    }

    /// Get timestamp in milliseconds
    pub fn get_time(&self) -> i64 {
        self.timestamp_ms
    }

    // ---- Civil date helpers (Howard Hinnant's algorithms) ----

    /// Convert days since Unix epoch to (year, month[1-12], day[1-31])
    fn civil_from_days(days: i64) -> (i32, i32, i32) {
        let z = days + 719468;
        let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
        let doe = (z - era * 146097) as u32;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        (y as i32, m as i32, d as i32)
    }

    /// Convert (year, month[1-12], day[1-31]) to days since Unix epoch
    fn days_from_civil(y: i32, m: i32, d: i32) -> i64 {
        let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
        let era = (if y >= 0 { y } else { y - 399 }) / 400;
        let yoe = (y - era * 400) as u32;
        let m = m as u32;
        let d = d as u32;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe as i64 - 719468
    }

    /// Decompose timestamp into (year, month[0-11], day[1-31], hour, min, sec, ms)
    fn decompose(&self) -> (i32, i32, i32, i32, i32, i32, i32) {
        let total_ms = self.timestamp_ms;
        let ms = ((total_ms % 1000 + 1000) % 1000) as i32;
        let total_secs = if total_ms >= 0 {
            total_ms / 1000
        } else {
            (total_ms - 999) / 1000
        };
        let day_secs = ((total_secs % 86400) + 86400) % 86400;
        let hour = (day_secs / 3600) as i32;
        let min = ((day_secs % 3600) / 60) as i32;
        let sec = (day_secs % 60) as i32;
        let days = (total_secs - day_secs) / 86400;
        let (y, m, d) = Self::civil_from_days(days);
        (y, m - 1, d, hour, min, sec, ms)
    }

    /// Recompose from (year, month[0-11], day[1-31], hour, min, sec, ms) to timestamp_ms
    fn recompose(y: i32, m: i32, d: i32, h: i32, min: i32, sec: i32, ms: i32) -> i64 {
        let days = Self::days_from_civil(y, m + 1, d);
        days * 86400_000 + h as i64 * 3600_000 + min as i64 * 60_000 + sec as i64 * 1000 + ms as i64
    }

    // ---- Getters ----

    /// Get year (4-digit)
    pub fn get_full_year(&self) -> i32 {
        self.decompose().0
    }

    /// Get month (0-11)
    pub fn get_month(&self) -> i32 {
        self.decompose().1
    }

    /// Get day of month (1-31)
    pub fn get_date(&self) -> i32 {
        self.decompose().2
    }

    /// Get day of week (0-6, 0=Sunday)
    pub fn get_day(&self) -> i32 {
        let total_secs = if self.timestamp_ms >= 0 {
            self.timestamp_ms / 1000
        } else {
            (self.timestamp_ms - 999) / 1000
        };
        let day_secs = ((total_secs % 86400) + 86400) % 86400;
        let days = (total_secs - day_secs) / 86400;
        (((days + 4) % 7 + 7) % 7) as i32
    }

    /// Get hours (0-23)
    pub fn get_hours(&self) -> i32 {
        self.decompose().3
    }

    /// Get minutes (0-59)
    pub fn get_minutes(&self) -> i32 {
        self.decompose().4
    }

    /// Get seconds (0-59)
    pub fn get_seconds(&self) -> i32 {
        self.decompose().5
    }

    /// Get milliseconds (0-999)
    pub fn get_milliseconds(&self) -> i32 {
        self.decompose().6
    }

    // ---- Setters (return new timestamp) ----

    pub fn set_full_year(&self, year: i32) -> i64 {
        let (_, m, d, h, min, sec, ms) = self.decompose();
        Self::recompose(year, m, d, h, min, sec, ms)
    }

    pub fn set_month(&self, month: i32) -> i64 {
        let (y, _, d, h, min, sec, ms) = self.decompose();
        Self::recompose(y, month, d, h, min, sec, ms)
    }

    pub fn set_date(&self, day: i32) -> i64 {
        let (y, m, _, h, min, sec, ms) = self.decompose();
        Self::recompose(y, m, day, h, min, sec, ms)
    }

    pub fn set_hours(&self, hours: i32) -> i64 {
        let (y, m, d, _, min, sec, ms) = self.decompose();
        Self::recompose(y, m, d, hours, min, sec, ms)
    }

    pub fn set_minutes(&self, minutes: i32) -> i64 {
        let (y, m, d, h, _, sec, ms) = self.decompose();
        Self::recompose(y, m, d, h, minutes, sec, ms)
    }

    pub fn set_seconds(&self, seconds: i32) -> i64 {
        let (y, m, d, h, min, _, ms) = self.decompose();
        Self::recompose(y, m, d, h, min, seconds, ms)
    }

    pub fn set_milliseconds(&self, millis: i32) -> i64 {
        let (y, m, d, h, min, sec, _) = self.decompose();
        Self::recompose(y, m, d, h, min, sec, millis)
    }

    // ---- Formatting ----

    fn day_name(dow: i32) -> &'static str {
        match dow {
            0 => "Sun", 1 => "Mon", 2 => "Tue", 3 => "Wed",
            4 => "Thu", 5 => "Fri", 6 => "Sat", _ => "???",
        }
    }

    fn month_name(m: i32) -> &'static str {
        match m {
            0 => "Jan", 1 => "Feb", 2 => "Mar", 3 => "Apr",
            4 => "May", 5 => "Jun", 6 => "Jul", 7 => "Aug",
            8 => "Sep", 9 => "Oct", 10 => "Nov", 11 => "Dec",
            _ => "???",
        }
    }

    /// Human-readable string: "Mon Jan 15 2024 10:30:00"
    pub fn to_string_repr(&self) -> String {
        let (y, m, d, h, min, sec, _) = self.decompose();
        let dow = self.get_day();
        format!(
            "{} {} {:02} {:04} {:02}:{:02}:{:02}",
            Self::day_name(dow), Self::month_name(m), d, y, h, min, sec
        )
    }

    /// ISO 8601: "2024-01-15T10:30:00.000Z"
    pub fn to_iso_string(&self) -> String {
        let (y, m, d, h, min, sec, ms) = self.decompose();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            y, m + 1, d, h, min, sec, ms
        )
    }

    /// Date portion: "Mon Jan 15 2024"
    pub fn to_date_string(&self) -> String {
        let (y, m, d, _, _, _, _) = self.decompose();
        let dow = self.get_day();
        format!("{} {} {:02} {:04}", Self::day_name(dow), Self::month_name(m), d, y)
    }

    /// Time portion: "10:30:00"
    pub fn to_time_string(&self) -> String {
        let (_, _, _, h, min, sec, _) = self.decompose();
        format!("{:02}:{:02}:{:02}", h, min, sec)
    }

    // ---- Parsing ----

    /// Parse ISO 8601 date string to timestamp ms
    pub fn parse(s: &str) -> Option<i64> {
        let s = s.trim().trim_end_matches('Z');
        let (date_part, time_part) = if let Some(idx) = s.find('T') {
            (&s[..idx], Some(&s[idx + 1..]))
        } else {
            (s, None)
        };

        let date_parts: Vec<&str> = date_part.split('-').collect();
        if date_parts.len() != 3 { return None; }
        let y: i32 = date_parts[0].parse().ok()?;
        let m: i32 = date_parts[1].parse().ok()?;
        let d: i32 = date_parts[2].parse().ok()?;
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) { return None; }

        let (h, min, sec, ms) = if let Some(tp) = time_part {
            let (time_str, ms) = if let Some(dot_idx) = tp.find('.') {
                let ms: i32 = tp[dot_idx + 1..].parse().ok()?;
                (&tp[..dot_idx], ms)
            } else {
                (tp, 0)
            };
            let time_parts: Vec<&str> = time_str.split(':').collect();
            let h: i32 = time_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            let min: i32 = time_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let sec: i32 = time_parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            (h, min, sec, ms)
        } else {
            (0, 0, 0, 0)
        };

        Some(Self::recompose(y, m - 1, d, h, min, sec, ms))
    }
}

/// Channel builtin - inter-task communication primitive
/// Native IDs: 0x0400-0x0408
///
/// Simple bounded queue. All waiter tracking and waking is managed by the
/// reactor's channel_waiters + pair matching. The interpreter uses try_send/
/// try_receive and suspends on failure; the reactor retries and matches
/// sender-receiver pairs (critical for unbuffered channels).
pub struct ChannelObject {
    /// Internal state protected by a mutex
    inner: parking_lot::Mutex<ChannelInner>,
}

/// Internal channel state
struct ChannelInner {
    /// Buffer capacity (0 = unbuffered)
    capacity: usize,
    /// Message queue
    queue: VecDeque<Value>,
    /// Whether channel is closed
    closed: bool,
}

impl std::fmt::Debug for ChannelObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("ChannelObject")
            .field("capacity", &inner.capacity)
            .field("length", &inner.queue.len())
            .field("closed", &inner.closed)
            .finish()
    }
}

impl ChannelObject {
    /// Create a new channel with given buffer capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: parking_lot::Mutex::new(ChannelInner {
                capacity,
                queue: VecDeque::with_capacity(capacity),
                closed: false,
            }),
        }
    }

    /// Get buffer capacity
    pub fn capacity(&self) -> usize {
        self.inner.lock().capacity
    }

    /// Get number of items in queue
    pub fn length(&self) -> usize {
        self.inner.lock().queue.len()
    }

    /// Check if channel is closed
    pub fn is_closed(&self) -> bool {
        self.inner.lock().closed
    }

    /// Close the channel. Reactor handles waking any waiting tasks.
    pub fn close(&self) {
        self.inner.lock().closed = true;
    }

    /// Try to send a value (non-blocking)
    /// Returns true if sent, false if full or closed
    pub fn try_send(&self, value: Value) -> bool {
        let mut inner = self.inner.lock();
        if inner.closed {
            return false;
        }
        if inner.queue.len() < inner.capacity {
            inner.queue.push_back(value);
            true
        } else {
            false
        }
    }

    /// Try to receive a value (non-blocking)
    /// Returns Some(value) if available, None if empty
    pub fn try_receive(&self) -> Option<Value> {
        self.inner.lock().queue.pop_front()
    }
}

// ============================================================================
// Proxy Objects (Phase 9 Reflect API)
// ============================================================================

/// Proxy object for intercepting property access and method calls
///
/// A Proxy wraps a target object and delegates operations through
/// trap handlers. When a property is accessed or a method is called
/// on the proxy, the corresponding trap handler is invoked if present.
///
/// Traps:
/// - `get(target, property)` - intercept property read
/// - `set(target, property, value)` - intercept property write
/// - `has(target, property)` - intercept property existence check
/// - `invoke(target, method, args)` - intercept method call
#[derive(Debug, Clone)]
pub struct Proxy {
    /// Unique proxy ID for identity checking
    pub proxy_id: u64,
    /// The underlying target object (as a Value pointing to Object)
    pub target: Value,
    /// The handler object containing trap functions (as a Value pointing to Object)
    /// Handler fields by name:
    /// - "get": (target, property) -> value
    /// - "set": (target, property, value) -> boolean
    /// - "has": (target, property) -> boolean
    /// - "invoke": (target, method, args) -> value
    pub handler: Value,
}

impl Proxy {
    /// Create a new proxy wrapping the target with the given handler
    pub fn new(target: Value, handler: Value) -> Self {
        Self {
            proxy_id: generate_object_id(),
            target,
            handler,
        }
    }

    /// Get the target object
    pub fn get_target(&self) -> Value {
        self.target
    }

    /// Get the handler object
    pub fn get_handler(&self) -> Value {
        self.handler
    }
}

impl Clone for ChannelObject {
    fn clone(&self) -> Self {
        let inner = self.inner.lock();
        Self {
            inner: parking_lot::Mutex::new(ChannelInner {
                capacity: inner.capacity,
                queue: inner.queue.clone(),
                closed: inner.closed,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_creation() {
        let obj = Object::new(0, 3);
        assert_eq!(obj.field_count(), 3);
        assert_eq!(obj.class_id, 0);
    }

    #[test]
    fn test_object_field_access() {
        let mut obj = Object::new(0, 2);
        let value = Value::i32(42);

        obj.set_field(0, value).unwrap();
        assert_eq!(obj.get_field(0).unwrap(), value);

        obj.set_field(1, Value::bool(true)).unwrap();
        assert_eq!(obj.get_field(1).unwrap(), Value::bool(true));
    }

    #[test]
    fn test_object_field_bounds() {
        let mut obj = Object::new(0, 2);
        assert!(obj.set_field(2, Value::null()).is_err());
        assert_eq!(obj.get_field(10), None);
    }

    #[test]
    fn test_class_creation() {
        let class = Class::new(0, "Point".to_string(), 2);
        assert_eq!(class.id, 0);
        assert_eq!(class.name, "Point");
        assert_eq!(class.field_count, 2);
        assert_eq!(class.parent_id, None);
    }

    #[test]
    fn test_class_with_parent() {
        let class = Class::with_parent(1, "ColoredPoint".to_string(), 3, 0);
        assert_eq!(class.parent_id, Some(0));
        assert_eq!(class.field_count, 3);
    }

    #[test]
    fn test_vtable() {
        let mut vtable = VTable::new();
        vtable.add_method(10); // function ID 10
        vtable.add_method(20); // function ID 20

        assert_eq!(vtable.method_count(), 2);
        assert_eq!(vtable.get_method(0), Some(10));
        assert_eq!(vtable.get_method(1), Some(20));
        assert_eq!(vtable.get_method(2), None);
    }

    #[test]
    fn test_vtable_override() {
        let mut vtable = VTable::new();
        vtable.add_method(10);
        vtable.add_method(20);

        vtable.override_method(0, 30).unwrap();
        assert_eq!(vtable.get_method(0), Some(30));
    }

    #[test]
    fn test_array_creation() {
        let arr = Array::new(0, 5);
        assert_eq!(arr.len(), 5);
        assert_eq!(arr.type_id, 0);
    }

    #[test]
    fn test_array_access() {
        let mut arr = Array::new(0, 3);

        arr.set(0, Value::i32(10)).unwrap();
        arr.set(1, Value::i32(20)).unwrap();
        arr.set(2, Value::i32(30)).unwrap();

        assert_eq!(arr.get(0), Some(Value::i32(10)));
        assert_eq!(arr.get(1), Some(Value::i32(20)));
        assert_eq!(arr.get(2), Some(Value::i32(30)));
    }

    #[test]
    fn test_array_bounds() {
        let mut arr = Array::new(0, 2);
        assert!(arr.set(2, Value::null()).is_err());
        assert_eq!(arr.get(5), None);
    }

    #[test]
    fn test_string_creation() {
        let s = RayaString::new("hello".to_string());
        assert_eq!(s.len(), 5);
        assert_eq!(s.data, "hello");
    }

    #[test]
    fn test_string_concat() {
        let s1 = RayaString::new("hello".to_string());
        let s2 = RayaString::new(" world".to_string());
        let s3 = s1.concat(&s2);

        assert_eq!(s3.data, "hello world");
    }
}
