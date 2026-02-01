//! Built-in method definitions
//!
//! This module defines constants for built-in methods on primitive types
//! (arrays, strings, etc.) that are handled specially by the VM.

use crate::vm::{Value, VmError};

// ============================================================================
// Native Call Result Types
// ============================================================================

/// Result of a native call that can signal yielding to the VM
///
/// Native calls can return one of three outcomes:
/// - `Ok(Value)` - The call completed successfully with a return value
/// - `Yield(WaitReason)` - The task should yield and wait for a condition
/// - `Err(VmError)` - The call failed with an error
#[derive(Debug)]
pub enum NativeResult {
    /// Call completed successfully with a value
    Ok(Value),
    /// Task should yield and wait for the specified condition
    Yield(WaitReason),
    /// Call failed with an error
    Err(VmError),
}

impl NativeResult {
    /// Create a successful result with null value
    pub fn ok_null() -> Self {
        NativeResult::Ok(Value::null())
    }

    /// Create a successful result with a boolean value
    pub fn ok_bool(v: bool) -> Self {
        NativeResult::Ok(Value::bool(v))
    }

    /// Create a successful result with an i32 value
    pub fn ok_i32(v: i32) -> Self {
        NativeResult::Ok(Value::i32(v))
    }

    /// Check if this is a yield result
    pub fn is_yield(&self) -> bool {
        matches!(self, NativeResult::Yield(_))
    }
}

impl From<Value> for NativeResult {
    fn from(v: Value) -> Self {
        NativeResult::Ok(v)
    }
}

impl From<VmError> for NativeResult {
    fn from(e: VmError) -> Self {
        NativeResult::Err(e)
    }
}

/// Reason why a task is yielding
///
/// When a native call returns `NativeResult::Yield`, it includes a reason
/// that tells the VM what condition the task is waiting for.
#[derive(Debug, Clone)]
pub enum WaitReason {
    /// Waiting to send on a full channel
    /// The VM should retry when the channel has space
    ChannelSend {
        /// Pointer to the channel object
        channel: Value,
        /// Value to send
        value: Value,
    },
    /// Waiting to receive from an empty channel
    /// The VM should retry when the channel has data
    ChannelReceive {
        /// Pointer to the channel object
        channel: Value,
    },
    /// Waiting for a mutex to become available
    MutexLock {
        /// Mutex handle
        mutex_id: u64,
    },
    /// Waiting for a duration (sleep)
    Sleep {
        /// Timestamp (in ms since epoch) when to wake up
        wake_at_ms: u64,
    },
}

/// Built-in method IDs for arrays
///
/// These IDs are used in CallMethod instructions when calling methods on arrays.
/// The VM recognizes these IDs and executes the built-in implementation.
pub mod array {
    /// `arr.push(value)` - Add element to end, returns new length
    pub const PUSH: u16 = 0x0100;
    /// `arr.pop()` - Remove and return last element
    pub const POP: u16 = 0x0101;
    /// `arr.shift()` - Remove and return first element
    pub const SHIFT: u16 = 0x0102;
    /// `arr.unshift(value)` - Add element to beginning, returns new length
    pub const UNSHIFT: u16 = 0x0103;
    /// `arr.indexOf(value)` - Find index of value, returns -1 if not found
    pub const INDEX_OF: u16 = 0x0104;
    /// `arr.includes(value)` - Check if array contains value
    pub const INCLUDES: u16 = 0x0105;
    /// `arr.slice(start, end)` - Return portion of array
    pub const SLICE: u16 = 0x0106;
    /// `arr.concat(other)` - Concatenate arrays
    pub const CONCAT: u16 = 0x0107;
    /// `arr.reverse()` - Reverse array in place
    pub const REVERSE: u16 = 0x0108;
    /// `arr.join(separator)` - Join elements into string
    pub const JOIN: u16 = 0x0109;
    /// `arr.forEach(fn)` - Call function for each element
    pub const FOR_EACH: u16 = 0x010A;
    /// `arr.filter(predicate)` - Filter elements by predicate
    pub const FILTER: u16 = 0x010B;
    /// `arr.find(predicate)` - Find first element matching predicate
    pub const FIND: u16 = 0x010C;
    /// `arr.findIndex(predicate)` - Find index of first element matching predicate
    pub const FIND_INDEX: u16 = 0x010D;
    /// `arr.every(predicate)` - Check if all elements match predicate
    pub const EVERY: u16 = 0x010E;
    /// `arr.some(predicate)` - Check if any element matches predicate
    pub const SOME: u16 = 0x010F;
    /// `arr.lastIndexOf(value)` - Find last index of value, returns -1 if not found
    pub const LAST_INDEX_OF: u16 = 0x0110;
    /// `arr.sort(compareFn)` - Sort array with comparison function
    pub const SORT: u16 = 0x0111;
    /// `arr.map(fn)` - Map elements to new array
    pub const MAP: u16 = 0x0112;
    /// `arr.reduce(fn, initial)` - Reduce array to single value
    pub const REDUCE: u16 = 0x0113;
    /// `arr.fill(value, start, end)` - Fill array with value
    pub const FILL: u16 = 0x0114;
    /// `arr.flat()` - Flatten nested array by one level
    pub const FLAT: u16 = 0x0115;
}

/// Built-in method IDs for strings
pub mod string {
    /// `str.charAt(index)` - Get character at index
    pub const CHAR_AT: u16 = 0x0200;
    /// `str.substring(start, end)` - Get substring
    pub const SUBSTRING: u16 = 0x0201;
    /// `str.toUpperCase()` - Convert to uppercase
    pub const TO_UPPER_CASE: u16 = 0x0202;
    /// `str.toLowerCase()` - Convert to lowercase
    pub const TO_LOWER_CASE: u16 = 0x0203;
    /// `str.trim()` - Remove whitespace from both ends
    pub const TRIM: u16 = 0x0204;
    /// `str.indexOf(searchStr)` - Find index of substring
    pub const INDEX_OF: u16 = 0x0205;
    /// `str.includes(searchStr)` - Check if string contains substring
    pub const INCLUDES: u16 = 0x0206;
    /// `str.split(separator)` - Split string into array
    pub const SPLIT: u16 = 0x0207;
    /// `str.startsWith(prefix)` - Check if starts with prefix
    pub const STARTS_WITH: u16 = 0x0208;
    /// `str.endsWith(suffix)` - Check if ends with suffix
    pub const ENDS_WITH: u16 = 0x0209;
    /// `str.replace(search, replacement)` - Replace first occurrence
    pub const REPLACE: u16 = 0x020A;
    /// `str.repeat(count)` - Repeat string n times
    pub const REPEAT: u16 = 0x020B;
    /// `str.padStart(length, padString)` - Pad start of string
    pub const PAD_START: u16 = 0x020C;
    /// `str.padEnd(length, padString)` - Pad end of string
    pub const PAD_END: u16 = 0x020D;
    /// `str.charCodeAt(index)` - Get character code at index
    pub const CHAR_CODE_AT: u16 = 0x020E;
    /// `str.lastIndexOf(searchStr)` - Find last index of substring
    pub const LAST_INDEX_OF: u16 = 0x020F;
    /// `str.trimStart()` - Remove whitespace from start
    pub const TRIM_START: u16 = 0x0210;
    /// `str.trimEnd()` - Remove whitespace from end
    pub const TRIM_END: u16 = 0x0211;
    /// `str.match(regexp)` - Match string against RegExp
    pub const MATCH: u16 = 0x0212;
    /// `str.matchAll(regexp)` - Match all occurrences against RegExp
    pub const MATCH_ALL: u16 = 0x0213;
    /// `str.search(regexp)` - Search for RegExp match, return index
    pub const SEARCH: u16 = 0x0214;
    /// `str.replace(regexp, replacement)` - Replace RegExp matches
    pub const REPLACE_REGEXP: u16 = 0x0215;
    /// `str.split(regexp, limit)` - Split by RegExp
    pub const SPLIT_REGEXP: u16 = 0x0216;
    /// `str.replaceWith(regexp, replacer)` - Replace RegExp matches with callback
    pub const REPLACE_WITH_REGEXP: u16 = 0x0217;
}

/// Built-in method IDs for Mutex
pub mod mutex {
    /// `mutex.tryLock()` - Try to acquire lock without blocking, returns boolean
    pub const TRY_LOCK: u16 = 0x0300;
    /// `mutex.isLocked()` - Check if mutex is currently locked
    pub const IS_LOCKED: u16 = 0x0301;
}

/// Built-in method IDs for Channel<T>
pub mod channel {
    /// `new Channel(capacity)` - Create channel
    pub const NEW: u16 = 0x0400;
    /// `ch.send(value)` - Send value (blocks if full)
    pub const SEND: u16 = 0x0401;
    /// `ch.receive()` - Receive value (blocks if empty)
    pub const RECEIVE: u16 = 0x0402;
    /// `ch.trySend(value)` - Try send without blocking
    pub const TRY_SEND: u16 = 0x0403;
    /// `ch.tryReceive()` - Try receive without blocking
    pub const TRY_RECEIVE: u16 = 0x0404;
    /// `ch.close()` - Close the channel
    pub const CLOSE: u16 = 0x0405;
    /// `ch.isClosed()` - Check if closed
    pub const IS_CLOSED: u16 = 0x0406;
    /// `ch.length()` - Get queue length
    pub const LENGTH: u16 = 0x0407;
    /// `ch.capacity()` - Get buffer capacity
    pub const CAPACITY: u16 = 0x0408;
}

/// Built-in method IDs for Task<T>
pub mod task {
    /// `task.isDone()` - Check if task has completed (success or failure)
    pub const IS_DONE: u16 = 0x0500;
    /// `task.isCancelled()` - Check if task was cancelled
    pub const IS_CANCELLED: u16 = 0x0501;
}

/// Built-in method IDs for Buffer
pub mod buffer {
    /// `new Buffer(size)` - Create buffer
    pub const NEW: u16 = 0x0700;
    /// `buf.length()` - Get length
    pub const LENGTH: u16 = 0x0701;
    /// `buf.getByte(index)` - Get byte
    pub const GET_BYTE: u16 = 0x0702;
    /// `buf.setByte(index, value)` - Set byte
    pub const SET_BYTE: u16 = 0x0703;
    /// `buf.getInt32(index)` - Get int32
    pub const GET_INT32: u16 = 0x0704;
    /// `buf.setInt32(index, value)` - Set int32
    pub const SET_INT32: u16 = 0x0705;
    /// `buf.getFloat64(index)` - Get float64
    pub const GET_FLOAT64: u16 = 0x0706;
    /// `buf.setFloat64(index, value)` - Set float64
    pub const SET_FLOAT64: u16 = 0x0707;
    /// `buf.slice(start, end)` - Slice buffer
    pub const SLICE: u16 = 0x0708;
    /// `buf.copy(target, targetStart, sourceStart, sourceEnd)` - Copy bytes
    pub const COPY: u16 = 0x0709;
    /// `buf.toString(encoding)` - Convert to string
    pub const TO_STRING: u16 = 0x070A;
    /// `Buffer.fromString(str, encoding)` - Create from string
    pub const FROM_STRING: u16 = 0x070B;
}

/// Built-in method IDs for Map<K, V>
pub mod map {
    /// `new Map()` - Create map
    pub const NEW: u16 = 0x0800;
    /// `map.size()` - Get size
    pub const SIZE: u16 = 0x0801;
    /// `map.get(key)` - Get value
    pub const GET: u16 = 0x0802;
    /// `map.set(key, value)` - Set value
    pub const SET: u16 = 0x0803;
    /// `map.has(key)` - Check if key exists
    pub const HAS: u16 = 0x0804;
    /// `map.delete(key)` - Delete key
    pub const DELETE: u16 = 0x0805;
    /// `map.clear()` - Clear all entries
    pub const CLEAR: u16 = 0x0806;
    /// `map.keys()` - Get all keys
    pub const KEYS: u16 = 0x0807;
    /// `map.values()` - Get all values
    pub const VALUES: u16 = 0x0808;
    /// `map.entries()` - Get all entries
    pub const ENTRIES: u16 = 0x0809;
    /// `map.forEach(fn)` - Iterate entries
    pub const FOR_EACH: u16 = 0x080A;
}

/// Built-in method IDs for Set<T>
pub mod set {
    /// `new Set()` - Create set
    pub const NEW: u16 = 0x0900;
    /// `set.size()` - Get size
    pub const SIZE: u16 = 0x0901;
    /// `set.add(value)` - Add value
    pub const ADD: u16 = 0x0902;
    /// `set.has(value)` - Check if value exists
    pub const HAS: u16 = 0x0903;
    /// `set.delete(value)` - Delete value
    pub const DELETE: u16 = 0x0904;
    /// `set.clear()` - Clear all values
    pub const CLEAR: u16 = 0x0905;
    /// `set.values()` - Get all values
    pub const VALUES: u16 = 0x0906;
    /// `set.forEach(fn)` - Iterate values
    pub const FOR_EACH: u16 = 0x0907;
    /// `set.union(other)` - Union with other set
    pub const UNION: u16 = 0x0908;
    /// `set.intersection(other)` - Intersection with other set
    pub const INTERSECTION: u16 = 0x0909;
    /// `set.difference(other)` - Difference with other set
    pub const DIFFERENCE: u16 = 0x090A;
}

/// Built-in method IDs for RegExp
pub mod regexp {
    /// `new RegExp(pattern, flags)` - Create new RegExp
    pub const NEW: u16 = 0x0A00;
    /// `regex.test(str)` - Test if pattern matches
    pub const TEST: u16 = 0x0A01;
    /// `regex.exec(str)` - Execute pattern and return match
    pub const EXEC: u16 = 0x0A02;
    /// `regex.execAll(str)` - Execute pattern and return all matches
    pub const EXEC_ALL: u16 = 0x0A03;
    /// `regex.replace(str, replacement)` - Replace matches with string
    pub const REPLACE: u16 = 0x0A04;
    /// `regex.replaceWith(str, replacer)` - Replace matches with callback result
    pub const REPLACE_WITH: u16 = 0x0A05;
    /// `regex.split(str, limit?)` - Split string by pattern
    pub const SPLIT: u16 = 0x0A06;
}

/// Built-in method IDs for Reflect operations
pub mod reflect {
    // ===== Phase 1: Metadata Operations (0x0D00-0x0D0F) =====

    /// `Reflect.defineMetadata(key, value, target)` - Define metadata on target
    pub const DEFINE_METADATA: u16 = 0x0D00;
    /// `Reflect.defineMetadata(key, value, target, propertyKey)` - Define metadata on property
    pub const DEFINE_METADATA_PROP: u16 = 0x0D01;
    /// `Reflect.getMetadata(key, target)` - Get metadata from target
    pub const GET_METADATA: u16 = 0x0D02;
    /// `Reflect.getMetadata(key, target, propertyKey)` - Get metadata from property
    pub const GET_METADATA_PROP: u16 = 0x0D03;
    /// `Reflect.hasMetadata(key, target)` - Check if target has metadata
    pub const HAS_METADATA: u16 = 0x0D04;
    /// `Reflect.hasMetadata(key, target, propertyKey)` - Check if property has metadata
    pub const HAS_METADATA_PROP: u16 = 0x0D05;
    /// `Reflect.getMetadataKeys(target)` - Get all metadata keys on target
    pub const GET_METADATA_KEYS: u16 = 0x0D06;
    /// `Reflect.getMetadataKeys(target, propertyKey)` - Get all metadata keys on property
    pub const GET_METADATA_KEYS_PROP: u16 = 0x0D07;
    /// `Reflect.deleteMetadata(key, target)` - Delete metadata from target
    pub const DELETE_METADATA: u16 = 0x0D08;
    /// `Reflect.deleteMetadata(key, target, propertyKey)` - Delete metadata from property
    pub const DELETE_METADATA_PROP: u16 = 0x0D09;

    // ===== Phase 2: Class Introspection (0x0D10-0x0D1F) =====

    /// `Reflect.getClass(obj)` - Get class of an object
    pub const GET_CLASS: u16 = 0x0D10;
    /// `Reflect.getClassByName(name)` - Lookup class by name
    pub const GET_CLASS_BY_NAME: u16 = 0x0D11;
    /// `Reflect.getAllClasses()` - Get all registered classes
    pub const GET_ALL_CLASSES: u16 = 0x0D12;
    /// `Reflect.getClassesWithDecorator(decorator)` - Filter classes by decorator
    pub const GET_CLASSES_WITH_DECORATOR: u16 = 0x0D13;
    /// `Reflect.isSubclassOf(sub, super)` - Check inheritance relationship
    pub const IS_SUBCLASS_OF: u16 = 0x0D14;
    /// `Reflect.isInstanceOf(obj, cls)` - Type guard for class membership
    pub const IS_INSTANCE_OF: u16 = 0x0D15;
    /// `Reflect.getTypeInfo(target)` - Get type info for target
    pub const GET_TYPE_INFO: u16 = 0x0D16;
    /// `Reflect.getClassHierarchy(obj)` - Get inheritance chain
    pub const GET_CLASS_HIERARCHY: u16 = 0x0D17;

    // ===== Phase 3: Field Access (0x0D20-0x0D2F) =====

    /// `Reflect.get(target, propertyKey)` - Get field value by name
    pub const GET: u16 = 0x0D20;
    /// `Reflect.set(target, propertyKey, value)` - Set field value by name
    pub const SET: u16 = 0x0D21;
    /// `Reflect.has(target, propertyKey)` - Check if field exists
    pub const HAS: u16 = 0x0D22;
    /// `Reflect.getFieldNames(target)` - List all field names
    pub const GET_FIELD_NAMES: u16 = 0x0D23;
    /// `Reflect.getFieldInfo(target, propertyKey)` - Get field metadata
    pub const GET_FIELD_INFO: u16 = 0x0D24;
    /// `Reflect.getFields(target)` - Get all field infos
    pub const GET_FIELDS: u16 = 0x0D25;
    /// `Reflect.getStaticFieldNames(cls)` - List static field names
    pub const GET_STATIC_FIELD_NAMES: u16 = 0x0D26;
    /// `Reflect.getStaticFields(cls)` - Get all static field infos
    pub const GET_STATIC_FIELDS: u16 = 0x0D27;

    // ===== Phase 4: Method Invocation (0x0D30-0x0D3F) =====

    /// `Reflect.invoke(target, methodName, ...args)` - Call method dynamically
    pub const INVOKE: u16 = 0x0D30;
    /// `Reflect.invokeAsync(target, methodName, ...args)` - Call async method
    pub const INVOKE_ASYNC: u16 = 0x0D31;
    /// `Reflect.getMethod(target, methodName)` - Get method reference
    pub const GET_METHOD: u16 = 0x0D32;
    /// `Reflect.getMethodInfo(target, methodName)` - Get method metadata
    pub const GET_METHOD_INFO: u16 = 0x0D33;
    /// `Reflect.getMethods(target)` - List all methods
    pub const GET_METHODS: u16 = 0x0D34;
    /// `Reflect.hasMethod(target, methodName)` - Check if method exists
    pub const HAS_METHOD: u16 = 0x0D35;
    /// `Reflect.invokeStatic(cls, methodName, ...args)` - Call static method
    pub const INVOKE_STATIC: u16 = 0x0D36;
    /// `Reflect.getStaticMethods(cls)` - Get all static method infos
    pub const GET_STATIC_METHODS: u16 = 0x0D37;

    // ===== Phase 5: Object Creation (0x0D40-0x0D4F) =====

    /// `Reflect.construct(cls, ...args)` - Create instance
    pub const CONSTRUCT: u16 = 0x0D40;
    /// `Reflect.constructWith(cls, params)` - Create with named params
    pub const CONSTRUCT_WITH: u16 = 0x0D41;
    /// `Reflect.allocate(cls)` - Allocate uninitialized instance
    pub const ALLOCATE: u16 = 0x0D42;
    /// `Reflect.clone(obj)` - Shallow clone
    pub const CLONE: u16 = 0x0D43;
    /// `Reflect.deepClone(obj)` - Deep clone
    pub const DEEP_CLONE: u16 = 0x0D44;
    /// `Reflect.getConstructorInfo(cls)` - Get constructor metadata
    pub const GET_CONSTRUCTOR_INFO: u16 = 0x0D45;
}

/// Built-in method IDs for Date
pub mod date {
    /// `Date.now()` - Get current timestamp
    pub const NOW: u16 = 0x0B00;
    /// `Date.parse(str)` - Parse date string
    pub const PARSE: u16 = 0x0B01;
    /// `date.getTime()` - Get timestamp in milliseconds
    pub const GET_TIME: u16 = 0x0B02;
    /// `date.getFullYear()` - Get year
    pub const GET_FULL_YEAR: u16 = 0x0B03;
    /// `date.getMonth()` - Get month (0-11)
    pub const GET_MONTH: u16 = 0x0B04;
    /// `date.getDate()` - Get day of month (1-31)
    pub const GET_DATE: u16 = 0x0B05;
    /// `date.getDay()` - Get day of week (0-6)
    pub const GET_DAY: u16 = 0x0B06;
    /// `date.getHours()` - Get hours (0-23)
    pub const GET_HOURS: u16 = 0x0B07;
    /// `date.getMinutes()` - Get minutes (0-59)
    pub const GET_MINUTES: u16 = 0x0B08;
    /// `date.getSeconds()` - Get seconds (0-59)
    pub const GET_SECONDS: u16 = 0x0B09;
    /// `date.getMilliseconds()` - Get milliseconds (0-999)
    pub const GET_MILLISECONDS: u16 = 0x0B0A;
    /// `date.setFullYear(year)` - Set year
    pub const SET_FULL_YEAR: u16 = 0x0B11;
    /// `date.setMonth(month)` - Set month
    pub const SET_MONTH: u16 = 0x0B12;
    /// `date.setDate(day)` - Set day of month
    pub const SET_DATE: u16 = 0x0B13;
    /// `date.setHours(hours)` - Set hours
    pub const SET_HOURS: u16 = 0x0B14;
    /// `date.setMinutes(minutes)` - Set minutes
    pub const SET_MINUTES: u16 = 0x0B15;
    /// `date.setSeconds(seconds)` - Set seconds
    pub const SET_SECONDS: u16 = 0x0B16;
    /// `date.setMilliseconds(ms)` - Set milliseconds
    pub const SET_MILLISECONDS: u16 = 0x0B17;
    /// `date.toString()` - Convert to string
    pub const TO_STRING: u16 = 0x0B20;
    /// `date.toISOString()` - Convert to ISO string
    pub const TO_ISO_STRING: u16 = 0x0B21;
    /// `date.toDateString()` - Get date portion
    pub const TO_DATE_STRING: u16 = 0x0B22;
    /// `date.toTimeString()` - Get time portion
    pub const TO_TIME_STRING: u16 = 0x0B23;
}

/// Look up built-in method ID by type and method name
///
/// Returns Some(method_id) if the method is a built-in, None otherwise.
pub fn lookup_builtin_method(type_name: &str, method_name: &str) -> Option<u16> {
    match type_name {
        "Array" | "array" => match method_name {
            "push" => Some(array::PUSH),
            "pop" => Some(array::POP),
            "shift" => Some(array::SHIFT),
            "unshift" => Some(array::UNSHIFT),
            "indexOf" => Some(array::INDEX_OF),
            "includes" => Some(array::INCLUDES),
            "slice" => Some(array::SLICE),
            "concat" => Some(array::CONCAT),
            "reverse" => Some(array::REVERSE),
            "join" => Some(array::JOIN),
            _ => None,
        },
        "String" | "string" => match method_name {
            "charAt" => Some(string::CHAR_AT),
            "substring" => Some(string::SUBSTRING),
            "toUpperCase" => Some(string::TO_UPPER_CASE),
            "toLowerCase" => Some(string::TO_LOWER_CASE),
            "trim" => Some(string::TRIM),
            "indexOf" => Some(string::INDEX_OF),
            "includes" => Some(string::INCLUDES),
            "split" => Some(string::SPLIT),
            "startsWith" => Some(string::STARTS_WITH),
            "endsWith" => Some(string::ENDS_WITH),
            "replace" => Some(string::REPLACE),
            "repeat" => Some(string::REPEAT),
            "padStart" => Some(string::PAD_START),
            "padEnd" => Some(string::PAD_END),
            "charCodeAt" => Some(string::CHAR_CODE_AT),
            "lastIndexOf" => Some(string::LAST_INDEX_OF),
            "trimStart" => Some(string::TRIM_START),
            "trimEnd" => Some(string::TRIM_END),
            _ => None,
        },
        _ => None,
    }
}

/// Check if a method ID is a built-in array method
pub fn is_array_method(method_id: u16) -> bool {
    (0x0100..=0x01FF).contains(&method_id)
}

/// Check if a method ID is a built-in string method
pub fn is_string_method(method_id: u16) -> bool {
    (0x0200..=0x02FF).contains(&method_id)
}

/// Check if a method ID is a built-in mutex method
pub fn is_mutex_method(method_id: u16) -> bool {
    (0x0300..=0x03FF).contains(&method_id)
}

/// Check if a method ID is a built-in channel method
pub fn is_channel_method(method_id: u16) -> bool {
    (0x0400..=0x04FF).contains(&method_id)
}

/// Check if a method ID is a built-in buffer method
pub fn is_buffer_method(method_id: u16) -> bool {
    (0x0700..=0x07FF).contains(&method_id)
}

/// Check if a method ID is a built-in map method
pub fn is_map_method(method_id: u16) -> bool {
    (0x0800..=0x08FF).contains(&method_id)
}

/// Check if a method ID is a built-in set method
pub fn is_set_method(method_id: u16) -> bool {
    (0x0900..=0x09FF).contains(&method_id)
}

/// Check if a method ID is a built-in regexp method
pub fn is_regexp_method(method_id: u16) -> bool {
    (0x0A00..=0x0AFF).contains(&method_id)
}

/// Check if a method ID is a built-in date method
pub fn is_date_method(method_id: u16) -> bool {
    (0x0B00..=0x0BFF).contains(&method_id)
}

/// Check if a method ID is a built-in reflect method
pub fn is_reflect_method(method_id: u16) -> bool {
    (0x0D00..=0x0DFF).contains(&method_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_array_methods() {
        assert_eq!(lookup_builtin_method("Array", "push"), Some(array::PUSH));
        assert_eq!(lookup_builtin_method("array", "pop"), Some(array::POP));
        assert_eq!(lookup_builtin_method("Array", "unknown"), None);
    }

    #[test]
    fn test_lookup_string_methods() {
        assert_eq!(lookup_builtin_method("String", "charAt"), Some(string::CHAR_AT));
        assert_eq!(lookup_builtin_method("string", "trim"), Some(string::TRIM));
        assert_eq!(lookup_builtin_method("String", "unknown"), None);
    }

    #[test]
    fn test_is_builtin_method() {
        assert!(is_array_method(array::PUSH));
        assert!(is_array_method(array::POP));
        assert!(!is_array_method(string::CHAR_AT));

        assert!(is_string_method(string::CHAR_AT));
        assert!(is_string_method(string::TRIM));
        assert!(!is_string_method(array::PUSH));
    }
}
