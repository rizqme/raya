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
    /// `new Array()` - Create empty array via constructor
    pub const NEW: u16 = 0x0116;
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
    /// Get match data for replaceWith (compiler intrinsic support)
    /// Returns array of [matched_text, start_index] arrays, respecting 'g' flag
    pub const REPLACE_MATCHES: u16 = 0x0A07;
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

    // Decorator Registration (for Phase 3 codegen)
    /// `Reflect.registerClassDecorator(classId, name, args)` - Register decorator on class
    pub const REGISTER_CLASS_DECORATOR: u16 = 0x0D18;
    /// `Reflect.registerMethodDecorator(classId, methodName, name, args)` - Register decorator on method
    pub const REGISTER_METHOD_DECORATOR: u16 = 0x0D19;
    /// `Reflect.registerFieldDecorator(classId, fieldName, name, args)` - Register decorator on field
    pub const REGISTER_FIELD_DECORATOR: u16 = 0x0D1A;
    /// `Reflect.registerParameterDecorator(classId, methodName, paramIndex, name, args)` - Register on param
    pub const REGISTER_PARAMETER_DECORATOR: u16 = 0x0D1B;
    /// `Reflect.getClassDecorators(classId)` - Get decorators on a class
    pub const GET_CLASS_DECORATORS: u16 = 0x0D1C;
    /// `Reflect.getMethodDecorators(classId, methodName)` - Get decorators on a method
    pub const GET_METHOD_DECORATORS: u16 = 0x0D1D;
    /// `Reflect.getFieldDecorators(classId, fieldName)` - Get decorators on a field
    pub const GET_FIELD_DECORATORS: u16 = 0x0D1E;

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

    // ===== Phase 6: Type Utilities (0x0D50-0x0D5F) =====

    /// `Reflect.isString(value)` - Check if value is string
    pub const IS_STRING: u16 = 0x0D50;
    /// `Reflect.isNumber(value)` - Check if value is number
    pub const IS_NUMBER: u16 = 0x0D51;
    /// `Reflect.isBoolean(value)` - Check if value is boolean
    pub const IS_BOOLEAN: u16 = 0x0D52;
    /// `Reflect.isNull(value)` - Check if value is null
    pub const IS_NULL: u16 = 0x0D53;
    /// `Reflect.isArray(value)` - Check if value is array
    pub const IS_ARRAY: u16 = 0x0D54;
    /// `Reflect.isFunction(value)` - Check if value is function
    pub const IS_FUNCTION: u16 = 0x0D55;
    /// `Reflect.isObject(value)` - Check if value is object
    pub const IS_OBJECT: u16 = 0x0D56;
    /// `Reflect.typeOf(typeName)` - Get TypeInfo from string
    pub const TYPE_OF: u16 = 0x0D57;
    /// `Reflect.isAssignableTo(source, target)` - Check type compatibility
    pub const IS_ASSIGNABLE_TO: u16 = 0x0D58;
    /// `Reflect.cast(value, classId)` - Safe cast (returns null if incompatible)
    pub const CAST: u16 = 0x0D59;
    /// `Reflect.castOrThrow(value, classId)` - Cast or throw error
    pub const CAST_OR_THROW: u16 = 0x0D5A;

    // ===== Phase 7: Interface and Hierarchy Query (0x0D60-0x0D6F) =====

    /// `Reflect.implements(cls, interfaceName)` - Check if class implements interface
    pub const IMPLEMENTS: u16 = 0x0D60;
    /// `Reflect.getInterfaces(cls)` - Get interfaces implemented by class
    pub const GET_INTERFACES: u16 = 0x0D61;
    /// `Reflect.getSuperclass(cls)` - Get parent class
    pub const GET_SUPERCLASS: u16 = 0x0D62;
    /// `Reflect.getSubclasses(cls)` - Get direct subclasses
    pub const GET_SUBCLASSES: u16 = 0x0D63;
    /// `Reflect.getImplementors(interfaceName)` - Get all classes implementing interface
    pub const GET_IMPLEMENTORS: u16 = 0x0D64;
    /// `Reflect.isStructurallyCompatible(a, b)` - Check structural type compatibility
    pub const IS_STRUCTURALLY_COMPATIBLE: u16 = 0x0D65;

    // ===== Phase 8: Object Inspection (0x0D70-0x0D7F) =====

    /// `Reflect.inspect(obj)` - Human-readable object representation
    pub const INSPECT: u16 = 0x0D70;
    /// `Reflect.getObjectId(obj)` - Get unique object identity
    pub const GET_OBJECT_ID: u16 = 0x0D71;
    /// `Reflect.describe(classId)` - Detailed class description string
    pub const DESCRIBE: u16 = 0x0D72;
    /// `Reflect.snapshot(obj)` - Capture object state
    pub const SNAPSHOT: u16 = 0x0D73;
    /// `Reflect.diff(a, b)` - Compare two objects/snapshots
    pub const DIFF: u16 = 0x0D74;

    // ===== Phase 8: Memory Analysis (0x0D80-0x0D8F) =====

    /// `Reflect.getObjectSize(obj)` - Shallow memory footprint in bytes
    pub const GET_OBJECT_SIZE: u16 = 0x0D80;
    /// `Reflect.getRetainedSize(obj)` - Size with retained objects
    pub const GET_RETAINED_SIZE: u16 = 0x0D81;
    /// `Reflect.getReferences(obj)` - Objects referenced by this object
    pub const GET_REFERENCES: u16 = 0x0D82;
    /// `Reflect.getReferrers(obj)` - Objects referencing this (if GC supports)
    pub const GET_REFERRERS: u16 = 0x0D83;
    /// `Reflect.getHeapStats()` - Total objects, memory usage by class
    pub const GET_HEAP_STATS: u16 = 0x0D84;
    /// `Reflect.findInstances(classId)` - All live instances of a class
    pub const FIND_INSTANCES: u16 = 0x0D85;

    // ===== Phase 8: Stack Introspection (0x0D90-0x0D9F) =====

    /// `Reflect.getCallStack()` - Current call frames
    pub const GET_CALL_STACK: u16 = 0x0D90;
    /// `Reflect.getLocals(frameIndex?)` - Local variables in frame
    pub const GET_LOCALS: u16 = 0x0D91;
    /// `Reflect.getSourceLocation(methodInfo)` - File:line:col mapping
    pub const GET_SOURCE_LOCATION: u16 = 0x0D92;

    // ===== Phase 8: Serialization Helpers (0x0DA0-0x0DAF) =====

    /// `Reflect.toJSON(obj)` - Serializable representation
    pub const TO_JSON: u16 = 0x0DA0;
    /// `Reflect.getEnumerableKeys(obj)` - Keys suitable for iteration
    pub const GET_ENUMERABLE_KEYS: u16 = 0x0DA1;
    /// `Reflect.isCircular(obj)` - Check for circular references
    pub const IS_CIRCULAR: u16 = 0x0DA2;

    // ===== Phase 9: Proxy Objects (0x0DB0-0x0DBF) =====

    /// `Reflect.createProxy(target, handler)` - Create a proxy object
    pub const CREATE_PROXY: u16 = 0x0DB0;
    /// `Reflect.isProxy(obj)` - Check if object is a proxy
    pub const IS_PROXY: u16 = 0x0DB1;
    /// `Reflect.getProxyTarget(proxy)` - Get the underlying target of a proxy
    pub const GET_PROXY_TARGET: u16 = 0x0DB2;
    /// `Reflect.getProxyHandler(proxy)` - Get the handler of a proxy
    pub const GET_PROXY_HANDLER: u16 = 0x0DB3;

    // ===== Phase 10: Dynamic Subclass Creation (0x0DC0-0x0DCF) =====

    /// `Reflect.createSubclass(superclassId, name, definition)` - Create a new subclass
    pub const CREATE_SUBCLASS: u16 = 0x0DC0;
    /// `Reflect.extendWith(classId, fields)` - Add fields to a class (returns new class)
    pub const EXTEND_WITH: u16 = 0x0DC1;
    /// `Reflect.defineClass(name, definition)` - Create a new root class
    pub const DEFINE_CLASS: u16 = 0x0DC2;
    /// `Reflect.addMethod(classId, name, methodImpl)` - Add method to class
    pub const ADD_METHOD: u16 = 0x0DC3;
    /// `Reflect.setConstructor(classId, constructorImpl)` - Set class constructor
    pub const SET_CONSTRUCTOR: u16 = 0x0DC4;

    // ===== Phase 13: Generic Type Metadata (0x0DD0-0x0DDF) =====

    /// `Reflect.getGenericOrigin(cls)` - Get generic class name (e.g., "Box" for Box_number)
    pub const GET_GENERIC_ORIGIN: u16 = 0x0DD0;
    /// `Reflect.getTypeParameters(cls)` - Get type parameter info
    pub const GET_TYPE_PARAMETERS: u16 = 0x0DD1;
    /// `Reflect.getTypeArguments(cls)` - Get actual type arguments
    pub const GET_TYPE_ARGUMENTS: u16 = 0x0DD2;
    /// `Reflect.isGenericInstance(cls)` - Check if monomorphized
    pub const IS_GENERIC_INSTANCE: u16 = 0x0DD3;
    /// `Reflect.getGenericBase(genericName)` - Get base generic class ID
    pub const GET_GENERIC_BASE: u16 = 0x0DD4;
    /// `Reflect.findSpecializations(genericName)` - Find all monomorphized versions
    pub const FIND_SPECIALIZATIONS: u16 = 0x0DD5;

    // ===== Phase 14: Runtime Type Creation (0x0DE0-0x0DEF) =====

    // Class Builder
    /// `Reflect.newClassBuilder(name)` - Create a new class builder
    pub const NEW_CLASS_BUILDER: u16 = 0x0DE0;
    /// `ClassBuilder.addField(builderId, name, typeName, options)` - Add field to builder
    pub const BUILDER_ADD_FIELD: u16 = 0x0DE1;
    /// `ClassBuilder.addMethod(builderId, name, functionId, options)` - Add method to builder
    pub const BUILDER_ADD_METHOD: u16 = 0x0DE2;
    /// `ClassBuilder.setConstructor(builderId, functionId)` - Set constructor
    pub const BUILDER_SET_CONSTRUCTOR: u16 = 0x0DE3;
    /// `ClassBuilder.setParent(builderId, parentClassId)` - Set parent class
    pub const BUILDER_SET_PARENT: u16 = 0x0DE4;
    /// `ClassBuilder.addInterface(builderId, interfaceName)` - Add interface
    pub const BUILDER_ADD_INTERFACE: u16 = 0x0DE5;
    /// `ClassBuilder.build(builderId)` - Finalize and register class
    pub const BUILDER_BUILD: u16 = 0x0DE6;

    // Function Creation
    /// `Reflect.createFunction(name, paramCount, bytecode)` - Create function from bytecode
    pub const CREATE_FUNCTION: u16 = 0x0DE7;
    /// `Reflect.createAsyncFunction(name, paramCount, bytecode)` - Create async function
    pub const CREATE_ASYNC_FUNCTION: u16 = 0x0DE8;
    /// `Reflect.createClosure(functionId, captures)` - Create closure with captures
    pub const CREATE_CLOSURE: u16 = 0x0DE9;
    /// `Reflect.createNativeCallback(callbackId)` - Register native callback
    pub const CREATE_NATIVE_CALLBACK: u16 = 0x0DEA;

    // Generic Specialization
    /// `Reflect.specialize(genericName, typeArgs)` - Create new monomorphization
    pub const SPECIALIZE: u16 = 0x0DEB;
    /// `Reflect.getSpecializationCache()` - Get cached specializations
    pub const GET_SPECIALIZATION_CACHE: u16 = 0x0DEC;

    // High-level Function Builder (for decorators)
    /// `Reflect.createWrapper(method, hooks)` - Create method wrapper with before/after/around hooks
    pub const CREATE_WRAPPER: u16 = 0x0DED;
    /// `Reflect.createMethodWrapper(method, wrapper)` - Create wrapper that calls wrapper(method, args)
    pub const CREATE_METHOD_WRAPPER: u16 = 0x0DEE;

    // ===== Phase 15: Dynamic Bytecode Generation (0x0DF0-0x0DFF) =====

    // BytecodeBuilder creation and management
    /// `Reflect.newBytecodeBuilder(name, paramCount, returnType)` - Create bytecode builder
    pub const NEW_BYTECODE_BUILDER: u16 = 0x0DF0;
    /// `BytecodeBuilder.emit(builderId, opcode, ...operands)` - Emit instruction
    pub const BUILDER_EMIT: u16 = 0x0DF1;
    /// `BytecodeBuilder.emitPush(builderId, value)` - Push constant value
    pub const BUILDER_EMIT_PUSH: u16 = 0x0DF2;
    /// `BytecodeBuilder.defineLabel(builderId)` - Define a new label
    pub const BUILDER_DEFINE_LABEL: u16 = 0x0DF3;
    /// `BytecodeBuilder.markLabel(builderId, labelId)` - Mark label position
    pub const BUILDER_MARK_LABEL: u16 = 0x0DF4;
    /// `BytecodeBuilder.emitJump(builderId, labelId)` - Emit unconditional jump
    pub const BUILDER_EMIT_JUMP: u16 = 0x0DF5;
    /// `BytecodeBuilder.emitJumpIf(builderId, labelId)` - Emit conditional jump
    pub const BUILDER_EMIT_JUMP_IF: u16 = 0x0DF6;
    /// `BytecodeBuilder.declareLocal(builderId, typeName)` - Declare local variable
    pub const BUILDER_DECLARE_LOCAL: u16 = 0x0DF7;
    /// `BytecodeBuilder.emitLoadLocal(builderId, index)` - Load local variable
    pub const BUILDER_EMIT_LOAD_LOCAL: u16 = 0x0DF8;
    /// `BytecodeBuilder.emitStoreLocal(builderId, index)` - Store local variable
    pub const BUILDER_EMIT_STORE_LOCAL: u16 = 0x0DF9;
    /// `BytecodeBuilder.emitCall(builderId, functionId)` - Emit function call
    pub const BUILDER_EMIT_CALL: u16 = 0x0DFA;
    /// `BytecodeBuilder.emitReturn(builderId)` - Emit return instruction
    pub const BUILDER_EMIT_RETURN: u16 = 0x0DFB;
    /// `BytecodeBuilder.validate(builderId)` - Validate bytecode
    pub const BUILDER_VALIDATE: u16 = 0x0DFC;
    /// `BytecodeBuilder.build(builderId)` - Build and register function
    pub const BUILDER_BUILD_FUNCTION: u16 = 0x0DFD;
    /// `Reflect.extendModule(moduleName, additions)` - Extend module with dynamic code
    pub const EXTEND_MODULE: u16 = 0x0DFE;

    // ===== Phase 16: Reflection Security & Permissions (0x0E00-0x0E0F) =====

    // Object-level permissions
    /// `Reflect.setPermissions(target, permissions)` - Set object-level permissions
    pub const SET_PERMISSIONS: u16 = 0x0E00;
    /// `Reflect.getPermissions(target)` - Get resolved permissions for target
    pub const GET_PERMISSIONS: u16 = 0x0E01;
    /// `Reflect.hasPermission(target, permission)` - Check specific permission flag
    pub const HAS_PERMISSION: u16 = 0x0E02;
    /// `Reflect.clearPermissions(target)` - Clear object-level permissions
    pub const CLEAR_PERMISSIONS: u16 = 0x0E03;

    // Class-level permissions
    /// `Reflect.setClassPermissions(classId, permissions)` - Set class-level permissions
    pub const SET_CLASS_PERMISSIONS: u16 = 0x0E04;
    /// `Reflect.getClassPermissions(classId)` - Get class-level permissions
    pub const GET_CLASS_PERMISSIONS: u16 = 0x0E05;
    /// `Reflect.clearClassPermissions(classId)` - Clear class-level permissions
    pub const CLEAR_CLASS_PERMISSIONS: u16 = 0x0E06;

    // Module-level permissions
    /// `Reflect.setModulePermissions(moduleName, permissions)` - Set module-level permissions
    pub const SET_MODULE_PERMISSIONS: u16 = 0x0E07;
    /// `Reflect.getModulePermissions(moduleName)` - Get module-level permissions
    pub const GET_MODULE_PERMISSIONS: u16 = 0x0E08;
    /// `Reflect.clearModulePermissions(moduleName)` - Clear module-level permissions
    pub const CLEAR_MODULE_PERMISSIONS: u16 = 0x0E09;

    // Global permissions
    /// `Reflect.setGlobalPermissions(permissions)` - Set global default permissions
    pub const SET_GLOBAL_PERMISSIONS: u16 = 0x0E0A;
    /// `Reflect.getGlobalPermissions()` - Get global default permissions
    pub const GET_GLOBAL_PERMISSIONS: u16 = 0x0E0B;

    // Permission sealing
    /// `Reflect.sealPermissions(target)` - Make permissions immutable
    pub const SEAL_PERMISSIONS: u16 = 0x0E0C;
    /// `Reflect.isPermissionsSealed(target)` - Check if permissions are sealed
    pub const IS_PERMISSIONS_SEALED: u16 = 0x0E0D;

    // ===== Phase 17: Dynamic VM Bootstrap (0x0E10-0x0E2F) =====

    // Module creation (0x0E10-0x0E17)
    /// `Reflect.createModule(name)` - Create empty dynamic module
    pub const CREATE_MODULE: u16 = 0x0E10;
    /// `Reflect.moduleAddFunction(moduleId, func)` - Add function to module
    pub const MODULE_ADD_FUNCTION: u16 = 0x0E11;
    /// `Reflect.moduleAddClass(moduleId, classId)` - Add class to module
    pub const MODULE_ADD_CLASS: u16 = 0x0E12;
    /// `Reflect.moduleAddGlobal(moduleId, name, value)` - Add global variable
    pub const MODULE_ADD_GLOBAL: u16 = 0x0E13;
    /// `Reflect.moduleSeal(moduleId)` - Finalize module for execution
    pub const MODULE_SEAL: u16 = 0x0E14;
    /// `Reflect.moduleLink(moduleId, imports)` - Resolve imports
    pub const MODULE_LINK: u16 = 0x0E15;
    /// `Reflect.getModule(moduleId)` - Get module info by ID
    pub const GET_MODULE: u16 = 0x0E16;
    /// `Reflect.getModuleByName(name)` - Get module by name
    pub const GET_MODULE_BY_NAME: u16 = 0x0E17;

    // Execution (0x0E18-0x0E1F)
    /// `Reflect.execute(functionId, args)` - Execute function synchronously
    pub const EXECUTE: u16 = 0x0E18;
    /// `Reflect.spawn(functionId, args)` - Execute function as Task
    pub const SPAWN: u16 = 0x0E19;
    /// `Reflect.eval(bytecode)` - Execute raw bytecode
    pub const EVAL: u16 = 0x0E1A;
    /// `Reflect.callDynamic(functionId, args)` - Call dynamic function by ID
    pub const CALL_DYNAMIC: u16 = 0x0E1B;
    /// `Reflect.invokeDynamicMethod(target, methodIndex, args)` - Invoke method on dynamic class
    pub const INVOKE_DYNAMIC_METHOD: u16 = 0x0E1C;

    // Bootstrap (0x0E20-0x0E2F)
    /// `Reflect.bootstrap()` - Initialize minimal runtime environment
    pub const BOOTSTRAP: u16 = 0x0E20;
    /// `Reflect.getObjectClass()` - Get core Object class ID
    pub const GET_OBJECT_CLASS: u16 = 0x0E21;
    /// `Reflect.getArrayClass()` - Get core Array class ID
    pub const GET_ARRAY_CLASS: u16 = 0x0E22;
    /// `Reflect.getStringClass()` - Get core String class ID
    pub const GET_STRING_CLASS: u16 = 0x0E23;
    /// `Reflect.getTaskClass()` - Get core Task class ID
    pub const GET_TASK_CLASS: u16 = 0x0E24;
    /// `Reflect.dynamicPrint(message)` - Print to console from dynamic code
    pub const DYNAMIC_PRINT: u16 = 0x0E25;
    /// `Reflect.createDynamicArray(elements)` - Create array from values
    pub const CREATE_DYNAMIC_ARRAY: u16 = 0x0E26;
    /// `Reflect.createDynamicString(value)` - Create string value
    pub const CREATE_DYNAMIC_STRING: u16 = 0x0E27;
    /// `Reflect.isBootstrapped()` - Check if bootstrap context exists
    pub const IS_BOOTSTRAPPED: u16 = 0x0E28;
}

/// Built-in method IDs for runtime operations (std:runtime)
pub mod runtime {
    // ── Compiler class (0x3000-0x3005) ──
    /// `Compiler.compile(source)` - Parse + type-check + compile to bytecode, returns module ID
    pub const COMPILE: u16 = 0x3000;
    /// `Compiler.compileExpression(expr)` - Wrap expression in `return <expr>;`, compile
    pub const COMPILE_EXPRESSION: u16 = 0x3001;
    /// `Compiler.compileAst(astId)` - Compile a pre-parsed AST to bytecode
    pub const COMPILE_AST: u16 = 0x3002;
    /// `Compiler.eval(source)` - Compile and immediately execute source
    pub const EVAL: u16 = 0x3003;
    /// `Compiler.execute(moduleId)` - Execute a compiled module's main function
    pub const EXECUTE: u16 = 0x3004;
    /// `Compiler.executeFunction(moduleId, funcName, ...args)` - Execute a named function
    pub const EXECUTE_FUNCTION: u16 = 0x3005;

    // ── Bytecode class (0x3010-0x3019) ──
    /// `Bytecode.encode(moduleId)` - Serialize module to .ryb binary
    pub const ENCODE: u16 = 0x3010;
    /// `Bytecode.decode(data)` - Deserialize .ryb binary to module
    pub const DECODE: u16 = 0x3011;
    /// `Bytecode.validate(moduleId)` - Verify module integrity
    pub const VALIDATE: u16 = 0x3012;
    /// `Bytecode.disassemble(moduleId)` - Disassemble to human-readable listing
    pub const DISASSEMBLE: u16 = 0x3013;
    /// `Bytecode.getModuleName(moduleId)` - Get module name
    pub const GET_MODULE_NAME: u16 = 0x3014;
    /// `Bytecode.getModuleFunctions(moduleId)` - List function names
    pub const GET_MODULE_FUNCTIONS: u16 = 0x3015;
    /// `Bytecode.getModuleClasses(moduleId)` - List class names
    pub const GET_MODULE_CLASSES: u16 = 0x3016;
    /// `Bytecode.loadLibrary(path)` - Load .ryb file from path
    pub const LOAD_LIBRARY: u16 = 0x3017;
    /// `Bytecode.loadDependency(path, name)` - Load .ryb and register as importable module
    pub const LOAD_DEPENDENCY: u16 = 0x3018;
    /// `Bytecode.resolveDependency(name)` - Auto-resolve .ryb from search paths
    pub const RESOLVE_DEPENDENCY: u16 = 0x3019;

    // ── Parser class (0x3050-0x3051) ──
    /// `Parser.parse(source)` - Parse source to AST, returns AST ID
    pub const PARSE: u16 = 0x3050;
    /// `Parser.parseExpression(expr)` - Parse a single expression to AST
    pub const PARSE_EXPRESSION: u16 = 0x3051;

    // ── TypeChecker class (0x3060-0x3061) ──
    /// `TypeChecker.check(astId)` - Type-check AST, returns typed AST ID
    pub const CHECK: u16 = 0x3060;
    /// `TypeChecker.checkExpression(astId)` - Type-check expression AST
    pub const CHECK_EXPRESSION: u16 = 0x3061;

    // ── Vm class (0x3020-0x3021) ──
    /// `Vm.current()` - Get root VM instance handle
    pub const VM_CURRENT: u16 = 0x3020;
    /// `Vm.spawn()` - Create child VM instance
    pub const VM_SPAWN: u16 = 0x3021;

    // ── VmInstance methods (0x3022-0x302C) ──
    /// `instance.id()` - Get instance ID
    pub const VM_INSTANCE_ID: u16 = 0x3022;
    /// `instance.isRoot()` - Check if root VM
    pub const VM_INSTANCE_IS_ROOT: u16 = 0x3023;
    /// `instance.isAlive()` - Check if alive
    pub const VM_INSTANCE_IS_ALIVE: u16 = 0x3024;
    /// `instance.loadBytecode(bytes)` - Load bytecode into child
    pub const VM_INSTANCE_LOAD_BYTECODE: u16 = 0x3025;
    /// `instance.runEntry(name)` - Run named entry function
    pub const VM_INSTANCE_RUN_ENTRY: u16 = 0x3027;
    /// `instance.compile(source)` - Compile within child
    pub const VM_INSTANCE_COMPILE: u16 = 0x3028;
    /// `instance.terminate()` - Terminate child and descendants
    pub const VM_INSTANCE_TERMINATE: u16 = 0x3029;
    /// `instance.isDestroyed()` - Check if terminated
    pub const VM_INSTANCE_IS_DESTROYED: u16 = 0x302A;
    /// `instance.execute(moduleId)` - Execute module in child
    pub const VM_INSTANCE_EXECUTE: u16 = 0x302B;
    /// `instance.eval(source)` - Compile + execute in child
    pub const VM_INSTANCE_EVAL: u16 = 0x302C;

    // ── Permission management (0x3030-0x3035) ──
    /// `Vm.hasPermission(name)` - Check if current VM has a named permission
    pub const HAS_PERMISSION: u16 = 0x3030;
    /// `Vm.getPermissions()` - Get current VM's permission policy as comma-separated string
    pub const GET_PERMISSIONS: u16 = 0x3031;
    /// `Vm.getAllowedStdlib()` - List allowed stdlib modules as comma-separated string
    pub const GET_ALLOWED_STDLIB: u16 = 0x3034;
    /// `Vm.isStdlibAllowed(module)` - Check if a specific stdlib module is allowed
    pub const IS_STDLIB_ALLOWED: u16 = 0x3035;

    // ── VM Introspection & Resource Control (0x3040-0x304A) ──
    /// `Vm.heapUsed()` - Current heap allocation in bytes
    pub const HEAP_USED: u16 = 0x3040;
    /// `Vm.heapLimit()` - Max heap size (0 = unlimited)
    pub const HEAP_LIMIT: u16 = 0x3041;
    /// `Vm.taskCount()` - Total tasks
    pub const TASK_COUNT: u16 = 0x3042;
    /// `Vm.concurrency()` - Tasks actively running
    pub const CONCURRENCY: u16 = 0x3043;
    /// `Vm.threadCount()` - Max worker threads
    pub const THREAD_COUNT: u16 = 0x3044;
    /// `Vm.gcCollect()` - Trigger manual GC
    pub const GC_COLLECT: u16 = 0x3045;
    /// `Vm.gcStats()` - Total bytes freed by GC
    pub const GC_STATS: u16 = 0x3046;
    /// `Vm.version()` - Raya VM version string
    pub const VERSION: u16 = 0x3047;
    /// `Vm.uptime()` - VM uptime in milliseconds
    pub const UPTIME: u16 = 0x3048;
    /// `Vm.loadedModules()` - Comma-separated list of loaded module names
    pub const LOADED_MODULES: u16 = 0x3049;
    /// `Vm.hasModule(name)` - Check if a module is loaded
    pub const HAS_MODULE: u16 = 0x304A;

    // ── VmInstance Debug Control (0x3070-0x3081) ──
    /// `instance.enableDebug()` - Activate DebugState for child VM
    pub const VM_ENABLE_DEBUG: u16 = 0x3070;
    /// `instance.debugRun(moduleId)` - Run module until breakpoint/end
    pub const VM_DEBUG_RUN: u16 = 0x3071;
    /// `instance.debugContinue()` - Resume until next pause
    pub const VM_DEBUG_CONTINUE: u16 = 0x3072;
    /// `instance.debugStepOver()` - Step over (same depth, different line)
    pub const VM_DEBUG_STEP_OVER: u16 = 0x3073;
    /// `instance.debugStepInto()` - Step into (any depth, different line)
    pub const VM_DEBUG_STEP_INTO: u16 = 0x3074;
    /// `instance.debugStepOut()` - Step out (shallower depth)
    pub const VM_DEBUG_STEP_OUT: u16 = 0x3075;
    /// `instance.setBreakpoint(file, line)` - Returns breakpoint ID
    pub const VM_SET_BREAKPOINT: u16 = 0x3076;
    /// `instance.removeBreakpoint(bpId)` - Remove a breakpoint
    pub const VM_REMOVE_BREAKPOINT: u16 = 0x3077;
    /// `instance.listBreakpoints()` - JSON array of breakpoint entries
    pub const VM_LIST_BREAKPOINTS: u16 = 0x3078;
    /// `instance.debugStackTrace()` - JSON array of stack frames
    pub const VM_DEBUG_STACK_TRACE: u16 = 0x3079;
    /// `instance.debugGetLocals(frameIndex)` - JSON array of locals
    pub const VM_DEBUG_GET_LOCALS: u16 = 0x307A;
    /// `instance.debugEvaluate(expression)` - Eval in paused context
    pub const VM_DEBUG_EVALUATE: u16 = 0x307B;
    /// `instance.debugLocation()` - JSON with current pause location
    pub const VM_DEBUG_LOCATION: u16 = 0x307C;
    /// `instance.debugGetSource(file, startLine, endLine)` - Source text
    pub const VM_DEBUG_GET_SOURCE: u16 = 0x307D;
    /// `instance.debugIsPaused()` - Check if child is paused
    pub const VM_DEBUG_IS_PAUSED: u16 = 0x307E;
    /// `instance.debugGetVariables(frameIndex)` - JSON locals with types
    pub const VM_DEBUG_GET_VARIABLES: u16 = 0x307F;
    /// `instance.setBreakpointCondition(bpId, condition)` - Set condition
    pub const VM_SET_BP_CONDITION: u16 = 0x3080;
    /// `instance.debugBreakAtEntry(moduleId)` - Break at first instruction
    pub const VM_DEBUG_BREAK_AT_ENTRY: u16 = 0x3081;
}

/// Check if a method ID is a runtime method (std:runtime)
pub fn is_runtime_method(method_id: u16) -> bool {
    (0x3000..=0x30FF).contains(&method_id)
}

/// Built-in method IDs for Crypto (std:crypto)
pub mod crypto {
    // ── Hashing (0x4000-0x4001) ──
    /// `crypto.hash(algorithm, data)` - One-shot hash, returns hex string
    pub const HASH: u16 = 0x4000;
    /// `crypto.hashBytes(algorithm, data)` - Hash binary data, returns raw bytes
    pub const HASH_BYTES: u16 = 0x4001;

    // ── HMAC (0x4002-0x4003) ──
    /// `crypto.hmac(algorithm, key, data)` - Keyed HMAC, returns hex string
    pub const HMAC: u16 = 0x4002;
    /// `crypto.hmacBytes(algorithm, key, data)` - HMAC on binary data, returns raw bytes
    pub const HMAC_BYTES: u16 = 0x4003;

    // ── Random (0x4004-0x4006) ──
    /// `crypto.randomBytes(size)` - Cryptographically secure random bytes
    pub const RANDOM_BYTES: u16 = 0x4004;
    /// `crypto.randomInt(min, max)` - Random integer in [min, max)
    pub const RANDOM_INT: u16 = 0x4005;
    /// `crypto.randomUUID()` - Generate UUID v4 string
    pub const RANDOM_UUID: u16 = 0x4006;

    // ── Encoding (0x4007-0x400A) ──
    /// `crypto.toHex(data)` - Binary to hex string
    pub const TO_HEX: u16 = 0x4007;
    /// `crypto.fromHex(hex)` - Hex string to binary
    pub const FROM_HEX: u16 = 0x4008;
    /// `crypto.toBase64(data)` - Binary to base64 string
    pub const TO_BASE64: u16 = 0x4009;
    /// `crypto.fromBase64(b64)` - Base64 string to binary
    pub const FROM_BASE64: u16 = 0x400A;

    // ── Comparison (0x400B) ──
    /// `crypto.timingSafeEqual(a, b)` - Constant-time equality check
    pub const TIMING_SAFE_EQUAL: u16 = 0x400B;

    // ── Encryption (0x400C-0x400D) ──
    /// `crypto.encrypt(key, plaintext)` - AES-256-GCM encrypt, returns nonce+ciphertext+tag
    pub const ENCRYPT: u16 = 0x400C;
    /// `crypto.decrypt(key, ciphertext)` - AES-256-GCM decrypt
    pub const DECRYPT: u16 = 0x400D;

    // ── Key Generation (0x400E) ──
    /// `crypto.generateKey(bits)` - Generate random symmetric key (128, 192, or 256 bits)
    pub const GENERATE_KEY: u16 = 0x400E;

    // ── Signing (0x400F-0x4011) ──
    /// `crypto.sign(algorithm, privateKey, data)` - Sign data with Ed25519
    pub const SIGN: u16 = 0x400F;
    /// `crypto.verify(algorithm, publicKey, data, signature)` - Verify signature
    pub const VERIFY: u16 = 0x4010;
    /// `crypto.generateKeyPair(algorithm)` - Generate key pair, returns [publicPem, privatePem]
    pub const GENERATE_KEY_PAIR: u16 = 0x4011;

    // ── Key Derivation (0x4012-0x4013) ──
    /// `crypto.hkdf(hash, ikm, salt, info, length)` - HKDF key derivation
    pub const HKDF: u16 = 0x4012;
    /// `crypto.pbkdf2(password, salt, iterations, length, hash)` - PBKDF2 key derivation
    pub const PBKDF2: u16 = 0x4013;
}

/// Check if a method ID is a crypto method (std:crypto)
pub fn is_crypto_method(method_id: u16) -> bool {
    (0x4000..=0x40FF).contains(&method_id)
}

/// Built-in method IDs for Time (std:time)
pub mod time {
    // ── Clock (0x5000-0x5002) ──
    /// `time.now()` - Wall clock: ms since Unix epoch
    pub const NOW: u16 = 0x5000;
    /// `time.monotonic()` - Monotonic clock: ms since process start
    pub const MONOTONIC: u16 = 0x5001;
    /// `time.hrtime()` - High-resolution monotonic: nanoseconds
    pub const HRTIME: u16 = 0x5002;

    // ── Sleep (0x5003-0x5004) ──
    /// `time.sleep(ms)` - Synchronous sleep (blocks thread)
    pub const SLEEP: u16 = 0x5003;
    /// `time.sleepMicros(us)` - Microsecond precision sleep
    pub const SLEEP_MICROS: u16 = 0x5004;
}

/// Check if a method ID is a time method (std:time)
pub fn is_time_method(method_id: u16) -> bool {
    (0x5000..=0x50FF).contains(&method_id)
}

/// Built-in method IDs for Path (std:path)
pub mod path {
    // ── Join & Normalize (0x6000-0x6001) ──
    /// `path.join(a, b)` - Join two path segments
    pub const JOIN: u16 = 0x6000;
    /// `path.normalize(p)` - Normalize path (resolve `.` and `..`)
    pub const NORMALIZE: u16 = 0x6001;

    // ── Components (0x6002-0x6004) ──
    /// `path.dirname(p)` - Directory name
    pub const DIRNAME: u16 = 0x6002;
    /// `path.basename(p)` - Base filename
    pub const BASENAME: u16 = 0x6003;
    /// `path.extname(p)` - File extension (with dot)
    pub const EXTNAME: u16 = 0x6004;

    // ── Resolution (0x6005-0x6008) ──
    /// `path.isAbsolute(p)` - Check if path is absolute
    pub const IS_ABSOLUTE: u16 = 0x6005;
    /// `path.resolve(from, to)` - Resolve path relative to base
    pub const RESOLVE: u16 = 0x6006;
    /// `path.relative(from, to)` - Compute relative path
    pub const RELATIVE: u16 = 0x6007;
    /// `path.cwd()` - Current working directory
    pub const CWD: u16 = 0x6008;

    // ── OS Constants (0x6009-0x600A) ──
    /// `path.sep()` - Path separator ("/" or "\\")
    pub const SEP: u16 = 0x6009;
    /// `path.delimiter()` - Path list delimiter (":" or ";")
    pub const DELIMITER: u16 = 0x600A;

    // ── Utilities (0x600B-0x600C) ──
    /// `path.stripExt(p)` - Remove extension from path
    pub const STRIP_EXT: u16 = 0x600B;
    /// `path.withExt(p, ext)` - Replace extension
    pub const WITH_EXT: u16 = 0x600C;
}

/// Check if a method ID is a path method (std:path)
pub fn is_path_method(method_id: u16) -> bool {
    (0x6000..=0x60FF).contains(&method_id)
}

/// Built-in method IDs for Compress (std:compress)
pub mod compress {
    // ── Gzip (0x8000-0x8001) ──
    /// `compress.gzip(data, level)` - Gzip compress data
    pub const GZIP: u16 = 0x8000;
    /// `compress.gunzip(data)` - Gzip decompress data
    pub const GUNZIP: u16 = 0x8001;

    // ── Raw Deflate (0x8002-0x8003) ──
    /// `compress.deflate(data, level)` - Raw deflate compress
    pub const DEFLATE: u16 = 0x8002;
    /// `compress.inflate(data)` - Raw deflate decompress
    pub const INFLATE: u16 = 0x8003;

    // ── Zlib (0x8004-0x8005) ──
    /// `compress.zlibCompress(data, level)` - Zlib compress
    pub const ZLIB_COMPRESS: u16 = 0x8004;
    /// `compress.zlibDecompress(data)` - Zlib decompress
    pub const ZLIB_DECOMPRESS: u16 = 0x8005;
}

/// Check if a method ID is a compress method (std:compress)
pub fn is_compress_method(method_id: u16) -> bool {
    (0x8000..=0x80FF).contains(&method_id)
}

/// Built-in method IDs for URL (std:url)
pub mod url {
    // ── URL Parsing (0x9000-0x9001) ──
    /// `url.parse(input)` - Parse a URL string, returns handle
    pub const PARSE: u16 = 0x9000;
    /// `url.parseWithBase(input, base)` - Parse with base URL, returns handle
    pub const PARSE_WITH_BASE: u16 = 0x9001;

    // ── URL Components (0x9010-0x901E) ──
    /// `url.protocol(handle)` - Get URL protocol/scheme
    pub const PROTOCOL: u16 = 0x9010;
    /// `url.hostname(handle)` - Get hostname
    pub const HOSTNAME: u16 = 0x9011;
    /// `url.port(handle)` - Get port
    pub const PORT: u16 = 0x9012;
    /// `url.host(handle)` - Get host (hostname:port)
    pub const HOST: u16 = 0x9013;
    /// `url.pathname(handle)` - Get pathname
    pub const PATHNAME: u16 = 0x9014;
    /// `url.search(handle)` - Get search/query string
    pub const SEARCH: u16 = 0x9015;
    /// `url.hash(handle)` - Get hash/fragment
    pub const HASH: u16 = 0x9016;
    /// `url.origin(handle)` - Get origin
    pub const ORIGIN: u16 = 0x9017;
    /// `url.href(handle)` - Get full URL string
    pub const HREF: u16 = 0x9018;
    /// `url.username(handle)` - Get username
    pub const USERNAME: u16 = 0x9019;
    /// `url.password(handle)` - Get password
    pub const PASSWORD: u16 = 0x901A;
    /// `url.searchParams(handle)` - Get search params handle
    pub const SEARCH_PARAMS: u16 = 0x901B;
    /// `url.toString(handle)` - Convert URL to string
    pub const TO_STRING: u16 = 0x901C;

    // ── Encoding (0x9020-0x9021) ──
    /// `url.encode(component)` - Percent-encode a URI component
    pub const ENCODE: u16 = 0x9020;
    /// `url.decode(component)` - Percent-decode a URI component
    pub const DECODE: u16 = 0x9021;

    // ── URLSearchParams (0x9030-0x903F) ──
    /// `url.paramsNew()` - Create empty search params
    pub const PARAMS_NEW: u16 = 0x9030;
    /// `url.paramsFromString(init)` - Create params from query string
    pub const PARAMS_FROM_STRING: u16 = 0x9031;
    /// `url.paramsGet(handle, name)` - Get first value for name
    pub const PARAMS_GET: u16 = 0x9032;
    /// `url.paramsGetAll(handle, name)` - Get all values for name
    pub const PARAMS_GET_ALL: u16 = 0x9033;
    /// `url.paramsHas(handle, name)` - Check if name exists
    pub const PARAMS_HAS: u16 = 0x9034;
    /// `url.paramsSet(handle, name, value)` - Set value for name
    pub const PARAMS_SET: u16 = 0x9035;
    /// `url.paramsAppend(handle, name, value)` - Append name=value pair
    pub const PARAMS_APPEND: u16 = 0x9036;
    /// `url.paramsDelete(handle, name)` - Delete all entries for name
    pub const PARAMS_DELETE: u16 = 0x9037;
    /// `url.paramsKeys(handle)` - Get all parameter names
    pub const PARAMS_KEYS: u16 = 0x9038;
    /// `url.paramsValues(handle)` - Get all parameter values
    pub const PARAMS_VALUES: u16 = 0x9039;
    /// `url.paramsEntries(handle)` - Get all entries as flat array
    pub const PARAMS_ENTRIES: u16 = 0x903A;
    /// `url.paramsSort(handle)` - Sort params by name
    pub const PARAMS_SORT: u16 = 0x903B;
    /// `url.paramsToString(handle)` - Serialize params to query string
    pub const PARAMS_TO_STRING: u16 = 0x903C;
    /// `url.paramsSize(handle)` - Get number of params
    pub const PARAMS_SIZE: u16 = 0x903D;
}

/// Check if a method ID is a URL method (std:url)
pub fn is_url_method(method_id: u16) -> bool {
    (0x9000..=0x90FF).contains(&method_id)
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

/// Built-in method IDs for Logger (std:logger)
pub mod logger {
    /// `logger.debug(message)` - Print debug output to stdout
    pub const DEBUG: u16 = 0x1000;
    /// `logger.info(message)` - Print info output to stdout
    pub const INFO: u16 = 0x1001;
    /// `logger.warn(message)` - Print warning to stderr
    pub const WARN: u16 = 0x1002;
    /// `logger.error(message)` - Print error to stderr
    pub const ERROR: u16 = 0x1003;
}

/// Check if a method ID is a logger method
pub fn is_logger_method(method_id: u16) -> bool {
    (0x1000..=0x10FF).contains(&method_id)
}

/// Built-in method IDs for Math (std:math)
pub mod math {
    /// `math.abs(x)` - Absolute value
    pub const ABS: u16 = 0x2000;
    /// `math.sign(x)` - Sign of number (-1, 0, 1)
    pub const SIGN: u16 = 0x2001;
    /// `math.floor(x)` - Round down
    pub const FLOOR: u16 = 0x2002;
    /// `math.ceil(x)` - Round up
    pub const CEIL: u16 = 0x2003;
    /// `math.round(x)` - Round to nearest integer
    pub const ROUND: u16 = 0x2004;
    /// `math.trunc(x)` - Truncate decimal part
    pub const TRUNC: u16 = 0x2005;
    /// `math.min(a, b)` - Minimum of two numbers
    pub const MIN: u16 = 0x2006;
    /// `math.max(a, b)` - Maximum of two numbers
    pub const MAX: u16 = 0x2007;
    /// `math.pow(base, exp)` - Power
    pub const POW: u16 = 0x2008;
    /// `math.sqrt(x)` - Square root
    pub const SQRT: u16 = 0x2009;
    /// `math.sin(x)` - Sine
    pub const SIN: u16 = 0x200A;
    /// `math.cos(x)` - Cosine
    pub const COS: u16 = 0x200B;
    /// `math.tan(x)` - Tangent
    pub const TAN: u16 = 0x200C;
    /// `math.asin(x)` - Arcsine
    pub const ASIN: u16 = 0x200D;
    /// `math.acos(x)` - Arccosine
    pub const ACOS: u16 = 0x200E;
    /// `math.atan(x)` - Arctangent
    pub const ATAN: u16 = 0x200F;
    /// `math.atan2(y, x)` - Two-argument arctangent
    pub const ATAN2: u16 = 0x2010;
    /// `math.exp(x)` - e^x
    pub const EXP: u16 = 0x2011;
    /// `math.log(x)` - Natural logarithm
    pub const LOG: u16 = 0x2012;
    /// `math.log10(x)` - Base-10 logarithm
    pub const LOG10: u16 = 0x2013;
    /// `math.random()` - Random number in [0, 1)
    pub const RANDOM: u16 = 0x2014;
    /// `math.PI()` - Pi constant
    pub const PI: u16 = 0x2015;
    /// `math.E()` - Euler's number constant
    pub const E: u16 = 0x2016;
}

/// Check if a method ID is a math method
pub fn is_math_method(method_id: u16) -> bool {
    (0x2000..=0x20FF).contains(&method_id)
}

/// Built-in method IDs for Number
pub mod number {
    /// `number.toFixed(digits)` - Format with fixed decimal places
    pub const TO_FIXED: u16 = 0x0F00;
    /// `number.toPrecision(precision)` - Format with significant digits
    pub const TO_PRECISION: u16 = 0x0F01;
    /// `number.toString(radix?)` - Convert to string with optional radix
    pub const TO_STRING_RADIX: u16 = 0x0F02;
    /// `parseInt(value)` - Parse string to integer
    pub const PARSE_INT: u16 = 0x0F03;
    /// `parseFloat(value)` - Parse string to float
    pub const PARSE_FLOAT: u16 = 0x0F04;
    /// `isNaN(value)` - Check if value is NaN
    pub const IS_NAN: u16 = 0x0F05;
    /// `isFinite(value)` - Check if value is finite
    pub const IS_FINITE: u16 = 0x0F06;
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
/// Covers Phases 1-17: 0x0D00-0x0DFF (core) + 0x0E00-0x0E2F (permissions + bootstrap)
pub fn is_reflect_method(method_id: u16) -> bool {
    (0x0D00..=0x0E2F).contains(&method_id)
}

/// Check if a method ID is a built-in number method
pub fn is_number_method(method_id: u16) -> bool {
    (0x0F00..=0x0F0F).contains(&method_id)
}

/// Check if a method ID is a proxy-related reflect method
pub fn is_proxy_method(method_id: u16) -> bool {
    (0x0DB0..=0x0DBF).contains(&method_id)
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
