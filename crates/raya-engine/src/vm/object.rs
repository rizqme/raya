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

    /// Get year (4-digit)
    pub fn get_full_year(&self) -> i32 {
        // Convert timestamp to date components
        let secs = self.timestamp_ms / 1000;
        let days = secs / 86400;
        // Approximate year calculation (doesn't handle leap years perfectly)
        let years_since_1970 = days / 365;
        (1970 + years_since_1970) as i32
    }

    /// Get month (0-11)
    pub fn get_month(&self) -> i32 {
        // Simplified calculation - a proper implementation would handle calendars correctly
        let secs = self.timestamp_ms / 1000;
        let days = (secs / 86400) % 365;
        (days / 30) as i32 % 12
    }

    /// Get day of month (1-31)
    pub fn get_date(&self) -> i32 {
        let secs = self.timestamp_ms / 1000;
        let days = (secs / 86400) % 365;
        ((days % 30) + 1) as i32
    }

    /// Get day of week (0-6, 0=Sunday)
    pub fn get_day(&self) -> i32 {
        // Jan 1, 1970 was a Thursday (4)
        let secs = self.timestamp_ms / 1000;
        let days = secs / 86400;
        ((days + 4) % 7) as i32
    }

    /// Get hours (0-23)
    pub fn get_hours(&self) -> i32 {
        let secs = self.timestamp_ms / 1000;
        ((secs % 86400) / 3600) as i32
    }

    /// Get minutes (0-59)
    pub fn get_minutes(&self) -> i32 {
        let secs = self.timestamp_ms / 1000;
        ((secs % 3600) / 60) as i32
    }

    /// Get seconds (0-59)
    pub fn get_seconds(&self) -> i32 {
        let secs = self.timestamp_ms / 1000;
        (secs % 60) as i32
    }

    /// Get milliseconds (0-999)
    pub fn get_milliseconds(&self) -> i32 {
        (self.timestamp_ms % 1000) as i32
    }
}

/// Channel builtin - inter-task communication primitive
/// Native IDs: 0x0400-0x0408
///
/// This is a thread-safe, blocking channel implementation using parking_lot
/// for synchronization. The channel supports:
/// - Buffered channels (capacity > 0): can hold up to `capacity` values
/// - Blocking send: waits if buffer is full
/// - Blocking receive: waits if buffer is empty
/// - Non-blocking try_send/try_receive variants
pub struct ChannelObject {
    /// Internal state protected by a mutex
    inner: parking_lot::Mutex<ChannelInner>,
    /// Condition variable for senders waiting on full channel
    not_full: parking_lot::Condvar,
    /// Condition variable for receivers waiting on empty channel
    not_empty: parking_lot::Condvar,
}

/// Internal channel state
struct ChannelInner {
    /// Buffer capacity (0 = unbuffered)
    capacity: usize,
    /// Message queue
    queue: VecDeque<Value>,
    /// Whether channel is closed
    closed: bool,
    /// Tasks waiting to send (task_id, value to send)
    waiting_senders: VecDeque<(u64, Value)>,
    /// Tasks waiting to receive
    waiting_receivers: VecDeque<u64>,
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
                waiting_senders: VecDeque::new(),
                waiting_receivers: VecDeque::new(),
            }),
            not_full: parking_lot::Condvar::new(),
            not_empty: parking_lot::Condvar::new(),
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

    /// Close the channel
    /// Wakes all waiting senders and receivers
    pub fn close(&self) {
        let mut inner = self.inner.lock();
        inner.closed = true;
        // Wake all waiters
        self.not_full.notify_all();
        self.not_empty.notify_all();
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
            // Wake one waiting receiver
            self.not_empty.notify_one();
            true
        } else {
            false
        }
    }

    /// Try to receive a value (non-blocking)
    /// Returns Some(value) if available, None if empty
    pub fn try_receive(&self) -> Option<Value> {
        let mut inner = self.inner.lock();
        let result = inner.queue.pop_front();
        if result.is_some() {
            // Wake one waiting sender
            self.not_full.notify_one();
        }
        result
    }

    /// Send a value, blocking until space is available
    /// Returns Ok(()) if sent, Err if channel is closed
    pub fn send(&self, value: Value) -> Result<(), ChannelError> {
        let mut inner = self.inner.lock();
        loop {
            if inner.closed {
                return Err(ChannelError::Closed);
            }
            if inner.queue.len() < inner.capacity {
                inner.queue.push_back(value);
                self.not_empty.notify_one();
                return Ok(());
            }
            // Wait for space
            self.not_full.wait(&mut inner);
        }
    }

    /// Receive a value, blocking until one is available
    /// Returns Ok(value) if received, Err if channel is closed and empty
    pub fn receive(&self) -> Result<Value, ChannelError> {
        let mut inner = self.inner.lock();
        loop {
            if let Some(value) = inner.queue.pop_front() {
                self.not_full.notify_one();
                return Ok(value);
            }
            if inner.closed {
                return Err(ChannelError::Closed);
            }
            // Wait for data
            self.not_empty.wait(&mut inner);
        }
    }

    /// Check if channel can send without blocking
    pub fn can_send(&self) -> bool {
        let inner = self.inner.lock();
        !inner.closed && inner.queue.len() < inner.capacity
    }

    /// Check if channel can receive without blocking
    pub fn can_receive(&self) -> bool {
        let inner = self.inner.lock();
        !inner.queue.is_empty()
    }

    // ===== Task-aware operations for cooperative scheduling =====

    /// Try to send, returning whether the task should suspend
    /// If Ok(None), send succeeded. If Ok(Some(task_id)), task should suspend.
    /// If Err, channel is closed.
    pub fn send_or_suspend(&self, value: Value, task_id: u64) -> Result<Option<u64>, ChannelError> {
        let mut inner = self.inner.lock();
        if inner.closed {
            return Err(ChannelError::Closed);
        }
        if inner.queue.len() < inner.capacity {
            // Space available, send immediately
            inner.queue.push_back(value);
            self.not_empty.notify_one();
            // Wake a waiting receiver if any
            if let Some(receiver_id) = inner.waiting_receivers.pop_front() {
                return Ok(Some(receiver_id)); // Return receiver to wake
            }
            Ok(None)
        } else {
            // Full, must suspend - store task and value
            inner.waiting_senders.push_back((task_id, value));
            Ok(Some(task_id)) // Signal that this task should suspend
        }
    }

    /// Try to receive, returning whether the task should suspend
    /// If Ok(Some(value)), receive succeeded. If Ok(None), task should suspend.
    /// If Err, channel is closed and empty.
    pub fn receive_or_suspend(&self, task_id: u64) -> Result<Option<Value>, ChannelError> {
        let mut inner = self.inner.lock();
        if let Some(value) = inner.queue.pop_front() {
            self.not_full.notify_one();
            // Check if there's a waiting sender we can now accept
            if let Some((sender_id, sender_value)) = inner.waiting_senders.pop_front() {
                // Accept the sender's value
                inner.queue.push_back(sender_value);
                // Return the sender_id as a wake signal (we return it via the wake list)
                // For now, just let the value through
                // The sender will be woken by the caller checking waiting_senders
            }
            Ok(Some(value))
        } else if inner.closed {
            Err(ChannelError::Closed)
        } else {
            // Empty, must suspend
            inner.waiting_receivers.push_back(task_id);
            Ok(None)
        }
    }

    /// Complete a suspended send operation (called when task resumes)
    /// Returns true if send completed, false if channel closed
    pub fn complete_send(&self, value: Value) -> Result<(), ChannelError> {
        let mut inner = self.inner.lock();
        if inner.closed {
            return Err(ChannelError::Closed);
        }
        inner.queue.push_back(value);
        self.not_empty.notify_one();
        Ok(())
    }

    /// Get and remove the next waiting sender (returns task_id and value)
    pub fn pop_waiting_sender(&self) -> Option<(u64, Value)> {
        let mut inner = self.inner.lock();
        inner.waiting_senders.pop_front()
    }

    /// Get and remove the next waiting receiver (returns task_id)
    pub fn pop_waiting_receiver(&self) -> Option<u64> {
        let mut inner = self.inner.lock();
        inner.waiting_receivers.pop_front()
    }

    /// Check if there are waiting senders
    pub fn has_waiting_senders(&self) -> bool {
        let inner = self.inner.lock();
        !inner.waiting_senders.is_empty()
    }

    /// Check if there are waiting receivers
    pub fn has_waiting_receivers(&self) -> bool {
        let inner = self.inner.lock();
        !inner.waiting_receivers.is_empty()
    }
}

/// Channel operation errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelError {
    /// Channel is closed
    Closed,
}

impl Clone for ChannelObject {
    fn clone(&self) -> Self {
        let inner = self.inner.lock();
        Self {
            inner: parking_lot::Mutex::new(ChannelInner {
                capacity: inner.capacity,
                queue: inner.queue.clone(),
                closed: inner.closed,
                waiting_senders: VecDeque::new(), // Don't clone waiting tasks
                waiting_receivers: VecDeque::new(),
            }),
            not_full: parking_lot::Condvar::new(),
            not_empty: parking_lot::Condvar::new(),
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
