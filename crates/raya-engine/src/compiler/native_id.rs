//! Native Function ID Constants
//!
//! Defines unique IDs for native functions called by primitive type methods.
//! These IDs are used by the NativeCall instruction and dispatched by the VM.
//!
//! IMPORTANT: These IDs MUST match the IDs in raya-core/src/builtin.rs

// ============================================================================
// Object (0x00xx)
// ============================================================================

pub const OBJECT_TO_STRING: u16 = 0x0001;
pub const OBJECT_HASH_CODE: u16 = 0x0002;
pub const OBJECT_EQUAL: u16 = 0x0003;

// ============================================================================
// Array (0x01xx) - Must match raya-core/src/builtin.rs
// ============================================================================

pub const ARRAY_PUSH: u16 = 0x0100;
pub const ARRAY_POP: u16 = 0x0101;
pub const ARRAY_SHIFT: u16 = 0x0102;
pub const ARRAY_UNSHIFT: u16 = 0x0103;
pub const ARRAY_INDEX_OF: u16 = 0x0104;
pub const ARRAY_INCLUDES: u16 = 0x0105;
pub const ARRAY_SLICE: u16 = 0x0106;
pub const ARRAY_CONCAT: u16 = 0x0107;
pub const ARRAY_REVERSE: u16 = 0x0108;
pub const ARRAY_JOIN: u16 = 0x0109;
// Extended array methods (not yet implemented in VM)
pub const ARRAY_LAST_INDEX_OF: u16 = 0x010A;
pub const ARRAY_SORT: u16 = 0x010B;
pub const ARRAY_MAP: u16 = 0x010C;
pub const ARRAY_FILTER: u16 = 0x010D;
pub const ARRAY_REDUCE: u16 = 0x010E;
pub const ARRAY_FOR_EACH: u16 = 0x010F;
pub const ARRAY_FIND: u16 = 0x0110;
pub const ARRAY_FIND_INDEX: u16 = 0x0111;
pub const ARRAY_EVERY: u16 = 0x0112;
pub const ARRAY_SOME: u16 = 0x0113;
pub const ARRAY_FILL: u16 = 0x0114;
pub const ARRAY_FLAT: u16 = 0x0115;

// ============================================================================
// String (0x02xx) - Must match raya-core/src/builtin.rs
// ============================================================================

pub const STRING_CHAR_AT: u16 = 0x0200;
pub const STRING_SUBSTRING: u16 = 0x0201;
pub const STRING_TO_UPPER_CASE: u16 = 0x0202;
pub const STRING_TO_LOWER_CASE: u16 = 0x0203;
pub const STRING_TRIM: u16 = 0x0204;
pub const STRING_INDEX_OF: u16 = 0x0205;
pub const STRING_INCLUDES: u16 = 0x0206;
pub const STRING_SPLIT: u16 = 0x0207;
pub const STRING_STARTS_WITH: u16 = 0x0208;
pub const STRING_ENDS_WITH: u16 = 0x0209;
pub const STRING_REPLACE: u16 = 0x020A;
pub const STRING_REPEAT: u16 = 0x020B;
pub const STRING_PAD_START: u16 = 0x020C;
pub const STRING_PAD_END: u16 = 0x020D;
// Extended string methods (not yet implemented in VM)
pub const STRING_CHAR_CODE_AT: u16 = 0x020E;
pub const STRING_LAST_INDEX_OF: u16 = 0x020F;
pub const STRING_SLICE: u16 = 0x0210;
pub const STRING_TRIM_START: u16 = 0x0211;
pub const STRING_TRIM_END: u16 = 0x0212;

// ============================================================================
// Mutex (0x03xx)
// ============================================================================

pub const MUTEX_TRY_LOCK: u16 = 0x0300;
pub const MUTEX_IS_LOCKED: u16 = 0x0301;

// ============================================================================
// Channel (0x04xx) - Must match raya-core/src/builtin.rs
// ============================================================================

pub const CHANNEL_NEW: u16 = 0x0400;
pub const CHANNEL_SEND: u16 = 0x0401;
pub const CHANNEL_RECEIVE: u16 = 0x0402;
pub const CHANNEL_TRY_SEND: u16 = 0x0403;
pub const CHANNEL_TRY_RECEIVE: u16 = 0x0404;
pub const CHANNEL_CLOSE: u16 = 0x0405;
pub const CHANNEL_IS_CLOSED: u16 = 0x0406;
pub const CHANNEL_LENGTH: u16 = 0x0407;
pub const CHANNEL_CAPACITY: u16 = 0x0408;

// ============================================================================
// Task (0x05xx)
// ============================================================================

pub const TASK_IS_DONE: u16 = 0x0500;
pub const TASK_IS_CANCELLED: u16 = 0x0501;

// ============================================================================
// Error (0x06xx)
// ============================================================================

pub const ERROR_STACK: u16 = 0x0600;

// ============================================================================
// Buffer (0x07xx)
// ============================================================================

pub const BUFFER_NEW: u16 = 0x0700;
pub const BUFFER_LENGTH: u16 = 0x0701;
pub const BUFFER_GET_BYTE: u16 = 0x0702;
pub const BUFFER_SET_BYTE: u16 = 0x0703;
pub const BUFFER_GET_INT32: u16 = 0x0704;
pub const BUFFER_SET_INT32: u16 = 0x0705;
pub const BUFFER_GET_FLOAT64: u16 = 0x0706;
pub const BUFFER_SET_FLOAT64: u16 = 0x0707;
pub const BUFFER_SLICE: u16 = 0x0708;
pub const BUFFER_COPY: u16 = 0x0709;
pub const BUFFER_TO_STRING: u16 = 0x070A;
pub const BUFFER_FROM_STRING: u16 = 0x070B;

// ============================================================================
// Map (0x08xx)
// ============================================================================

pub const MAP_NEW: u16 = 0x0800;
pub const MAP_SIZE: u16 = 0x0801;
pub const MAP_GET: u16 = 0x0802;
pub const MAP_SET: u16 = 0x0803;
pub const MAP_HAS: u16 = 0x0804;
pub const MAP_DELETE: u16 = 0x0805;
pub const MAP_CLEAR: u16 = 0x0806;
pub const MAP_KEYS: u16 = 0x0807;
pub const MAP_VALUES: u16 = 0x0808;
pub const MAP_ENTRIES: u16 = 0x0809;
pub const MAP_FOR_EACH: u16 = 0x080A;

// ============================================================================
// Set (0x09xx)
// ============================================================================

pub const SET_NEW: u16 = 0x0900;
pub const SET_SIZE: u16 = 0x0901;
pub const SET_ADD: u16 = 0x0902;
pub const SET_HAS: u16 = 0x0903;
pub const SET_DELETE: u16 = 0x0904;
pub const SET_CLEAR: u16 = 0x0905;
pub const SET_VALUES: u16 = 0x0906;
pub const SET_FOR_EACH: u16 = 0x0907;
pub const SET_UNION: u16 = 0x0908;
pub const SET_INTERSECTION: u16 = 0x0909;
pub const SET_DIFFERENCE: u16 = 0x090A;

// ============================================================================
// RegExp (0x0Axx)
// ============================================================================

pub const REGEXP_NEW: u16 = 0x0A00;
pub const REGEXP_TEST: u16 = 0x0A01;
pub const REGEXP_EXEC: u16 = 0x0A02;
pub const REGEXP_EXEC_ALL: u16 = 0x0A03;
pub const REGEXP_REPLACE: u16 = 0x0A04;
pub const REGEXP_REPLACE_WITH: u16 = 0x0A05;
pub const REGEXP_SPLIT: u16 = 0x0A06;

// ============================================================================
// Date (0x0Bxx)
// ============================================================================

pub const DATE_NOW: u16 = 0x0B00;
pub const DATE_PARSE: u16 = 0x0B01;
pub const DATE_GET_TIME: u16 = 0x0B02;
pub const DATE_GET_FULL_YEAR: u16 = 0x0B03;
pub const DATE_GET_MONTH: u16 = 0x0B04;
pub const DATE_GET_DATE: u16 = 0x0B05;
pub const DATE_GET_DAY: u16 = 0x0B06;
pub const DATE_GET_HOURS: u16 = 0x0B07;
pub const DATE_GET_MINUTES: u16 = 0x0B08;
pub const DATE_GET_SECONDS: u16 = 0x0B09;
pub const DATE_GET_MILLISECONDS: u16 = 0x0B0A;
pub const DATE_SET_TIME: u16 = 0x0B10;
pub const DATE_SET_FULL_YEAR: u16 = 0x0B11;
pub const DATE_SET_MONTH: u16 = 0x0B12;
pub const DATE_SET_DATE: u16 = 0x0B13;
pub const DATE_SET_HOURS: u16 = 0x0B14;
pub const DATE_SET_MINUTES: u16 = 0x0B15;
pub const DATE_SET_SECONDS: u16 = 0x0B16;
pub const DATE_SET_MILLISECONDS: u16 = 0x0B17;
pub const DATE_TO_STRING: u16 = 0x0B20;
pub const DATE_TO_ISO_STRING: u16 = 0x0B21;
pub const DATE_TO_DATE_STRING: u16 = 0x0B22;
pub const DATE_TO_TIME_STRING: u16 = 0x0B23;

// ============================================================================
// JSON (0x0Cxx)
// ============================================================================

/// JSON.stringify(value: any): string
pub const JSON_STRINGIFY: u16 = 0x0C00;
/// JSON.parse(json: string): any
pub const JSON_PARSE: u16 = 0x0C01;
/// JSON.decode<T>(json: string): T - typed decode with field metadata
/// Args: [json_string, field_count, ...field_specs]
/// Each field_spec is: [json_key_string, field_index, field_type]
pub const JSON_DECODE_OBJECT: u16 = 0x0C02;

// ============================================================================
// Reflect (0x0Dxx) - Metadata operations
// ============================================================================

/// Reflect.defineMetadata(key, value, target) - define metadata on target
pub const REFLECT_DEFINE_METADATA: u16 = 0x0D00;
/// Reflect.defineMetadata(key, value, target, propertyKey) - define metadata on property
pub const REFLECT_DEFINE_METADATA_PROP: u16 = 0x0D01;
/// Reflect.getMetadata(key, target) - get metadata from target
pub const REFLECT_GET_METADATA: u16 = 0x0D02;
/// Reflect.getMetadata(key, target, propertyKey) - get metadata from property
pub const REFLECT_GET_METADATA_PROP: u16 = 0x0D03;
/// Reflect.hasMetadata(key, target) - check if target has metadata
pub const REFLECT_HAS_METADATA: u16 = 0x0D04;
/// Reflect.hasMetadata(key, target, propertyKey) - check if property has metadata
pub const REFLECT_HAS_METADATA_PROP: u16 = 0x0D05;
/// Reflect.getMetadataKeys(target) - get all metadata keys on target
pub const REFLECT_GET_METADATA_KEYS: u16 = 0x0D06;
/// Reflect.getMetadataKeys(target, propertyKey) - get all metadata keys on property
pub const REFLECT_GET_METADATA_KEYS_PROP: u16 = 0x0D07;
/// Reflect.deleteMetadata(key, target) - delete metadata from target
pub const REFLECT_DELETE_METADATA: u16 = 0x0D08;
/// Reflect.deleteMetadata(key, target, propertyKey) - delete metadata from property
pub const REFLECT_DELETE_METADATA_PROP: u16 = 0x0D09;

// ============================================================================
// Reflect - Class Introspection (0x0D10-0x0D1F)
// ============================================================================

/// Reflect.getClass(obj) - get class ID of object
pub const REFLECT_GET_CLASS: u16 = 0x0D10;
/// Reflect.getClassByName(name) - lookup class by name
pub const REFLECT_GET_CLASS_BY_NAME: u16 = 0x0D11;
/// Reflect.getAllClasses() - get all registered class IDs
pub const REFLECT_GET_ALL_CLASSES: u16 = 0x0D12;
/// Reflect.isSubclassOf(subClassId, superClassId) - check inheritance
pub const REFLECT_IS_SUBCLASS_OF: u16 = 0x0D13;
/// Reflect.isInstanceOf(obj, classId) - type guard
pub const REFLECT_IS_INSTANCE_OF: u16 = 0x0D14;
/// Reflect.getTypeInfo(target) - get type info string
pub const REFLECT_GET_TYPE_INFO: u16 = 0x0D15;
/// Reflect.getClassHierarchy(obj) - get inheritance chain
pub const REFLECT_GET_CLASS_HIERARCHY: u16 = 0x0D16;
/// Reflect.getClassesWithDecorator(decorator) - filter by decorator
pub const REFLECT_GET_CLASSES_WITH_DECORATOR: u16 = 0x0D17;

// ============================================================================
// Reflect - Field Access (0x0D20-0x0D2F)
// ============================================================================

/// Reflect.get(target, propertyKey) - get field value
pub const REFLECT_GET: u16 = 0x0D20;
/// Reflect.set(target, propertyKey, value) - set field value
pub const REFLECT_SET: u16 = 0x0D21;
/// Reflect.has(target, propertyKey) - check field exists
pub const REFLECT_HAS: u16 = 0x0D22;
/// Reflect.getFieldNames(target) - list all field names
pub const REFLECT_GET_FIELD_NAMES: u16 = 0x0D23;
/// Reflect.getFieldInfo(target, propertyKey) - get field metadata
pub const REFLECT_GET_FIELD_INFO: u16 = 0x0D24;
/// Reflect.getFields(target) - get all field infos
pub const REFLECT_GET_FIELDS: u16 = 0x0D25;
/// Reflect.getStaticFieldNames(classId) - get static field names
pub const REFLECT_GET_STATIC_FIELD_NAMES: u16 = 0x0D26;
/// Reflect.getStaticFields(classId) - get static field infos
pub const REFLECT_GET_STATIC_FIELDS: u16 = 0x0D27;

// ============================================================================
// Reflect - Method Invocation (0x0D30-0x0D3F)
// ============================================================================

/// Reflect.invoke(target, methodName, ...args) - call method dynamically
pub const REFLECT_INVOKE: u16 = 0x0D30;
/// Reflect.invokeAsync(target, methodName, ...args) - call async method
pub const REFLECT_INVOKE_ASYNC: u16 = 0x0D31;
/// Reflect.getMethod(target, methodName) - get method reference (function value)
pub const REFLECT_GET_METHOD: u16 = 0x0D32;
/// Reflect.getMethodInfo(target, methodName) - get method metadata
pub const REFLECT_GET_METHOD_INFO: u16 = 0x0D33;
/// Reflect.getMethods(target) - list all method infos
pub const REFLECT_GET_METHODS: u16 = 0x0D34;
/// Reflect.hasMethod(target, methodName) - check method exists
pub const REFLECT_HAS_METHOD: u16 = 0x0D35;
/// Reflect.invokeStatic(classId, methodName, ...args) - call static method
pub const REFLECT_INVOKE_STATIC: u16 = 0x0D36;
/// Reflect.getStaticMethods(classId) - get static method infos
pub const REFLECT_GET_STATIC_METHODS: u16 = 0x0D37;

// ============================================================================
// Reflect - Object Creation (0x0D40-0x0D4F)
// ============================================================================

/// Reflect.construct(classId, ...args) - create instance
pub const REFLECT_CONSTRUCT: u16 = 0x0D40;
/// Reflect.allocate(classId) - allocate uninitialized instance
pub const REFLECT_ALLOCATE: u16 = 0x0D41;
/// Reflect.clone(obj) - shallow clone
pub const REFLECT_CLONE: u16 = 0x0D42;

/// Get the name of a native function for debugging purposes
pub fn native_name(id: u16) -> &'static str {
    match id {
        // Object
        OBJECT_TO_STRING => "Object.toString",
        OBJECT_HASH_CODE => "Object.hashCode",
        OBJECT_EQUAL => "Object.equal",

        // Array
        ARRAY_PUSH => "Array.push",
        ARRAY_POP => "Array.pop",
        ARRAY_SHIFT => "Array.shift",
        ARRAY_UNSHIFT => "Array.unshift",
        ARRAY_INDEX_OF => "Array.indexOf",
        ARRAY_INCLUDES => "Array.includes",
        ARRAY_SLICE => "Array.slice",
        ARRAY_CONCAT => "Array.concat",
        ARRAY_REVERSE => "Array.reverse",
        ARRAY_JOIN => "Array.join",
        ARRAY_LAST_INDEX_OF => "Array.lastIndexOf",
        ARRAY_SORT => "Array.sort",
        ARRAY_MAP => "Array.map",
        ARRAY_FILTER => "Array.filter",
        ARRAY_REDUCE => "Array.reduce",
        ARRAY_FOR_EACH => "Array.forEach",
        ARRAY_FIND => "Array.find",
        ARRAY_FIND_INDEX => "Array.findIndex",
        ARRAY_EVERY => "Array.every",
        ARRAY_SOME => "Array.some",
        ARRAY_FILL => "Array.fill",
        ARRAY_FLAT => "Array.flat",

        // String
        STRING_CHAR_AT => "String.charAt",
        STRING_SUBSTRING => "String.substring",
        STRING_TO_UPPER_CASE => "String.toUpperCase",
        STRING_TO_LOWER_CASE => "String.toLowerCase",
        STRING_TRIM => "String.trim",
        STRING_INDEX_OF => "String.indexOf",
        STRING_INCLUDES => "String.includes",
        STRING_SPLIT => "String.split",
        STRING_STARTS_WITH => "String.startsWith",
        STRING_ENDS_WITH => "String.endsWith",
        STRING_REPLACE => "String.replace",
        STRING_REPEAT => "String.repeat",
        STRING_PAD_START => "String.padStart",
        STRING_PAD_END => "String.padEnd",
        STRING_CHAR_CODE_AT => "String.charCodeAt",
        STRING_LAST_INDEX_OF => "String.lastIndexOf",
        STRING_SLICE => "String.slice",
        STRING_TRIM_START => "String.trimStart",
        STRING_TRIM_END => "String.trimEnd",

        // RegExp
        REGEXP_NEW => "RegExp.new",
        REGEXP_TEST => "RegExp.test",
        REGEXP_EXEC => "RegExp.exec",
        REGEXP_EXEC_ALL => "RegExp.execAll",
        REGEXP_REPLACE => "RegExp.replace",
        REGEXP_REPLACE_WITH => "RegExp.replaceWith",
        REGEXP_SPLIT => "RegExp.split",

        // JSON
        JSON_STRINGIFY => "JSON.stringify",
        JSON_PARSE => "JSON.parse",
        JSON_DECODE_OBJECT => "JSON.decode",

        // Reflect - Metadata
        REFLECT_DEFINE_METADATA => "Reflect.defineMetadata",
        REFLECT_DEFINE_METADATA_PROP => "Reflect.defineMetadata (property)",
        REFLECT_GET_METADATA => "Reflect.getMetadata",
        REFLECT_GET_METADATA_PROP => "Reflect.getMetadata (property)",
        REFLECT_HAS_METADATA => "Reflect.hasMetadata",
        REFLECT_HAS_METADATA_PROP => "Reflect.hasMetadata (property)",
        REFLECT_GET_METADATA_KEYS => "Reflect.getMetadataKeys",
        REFLECT_GET_METADATA_KEYS_PROP => "Reflect.getMetadataKeys (property)",
        REFLECT_DELETE_METADATA => "Reflect.deleteMetadata",
        REFLECT_DELETE_METADATA_PROP => "Reflect.deleteMetadata (property)",

        // Reflect - Class Introspection
        REFLECT_GET_CLASS => "Reflect.getClass",
        REFLECT_GET_CLASS_BY_NAME => "Reflect.getClassByName",
        REFLECT_GET_ALL_CLASSES => "Reflect.getAllClasses",
        REFLECT_IS_SUBCLASS_OF => "Reflect.isSubclassOf",
        REFLECT_IS_INSTANCE_OF => "Reflect.isInstanceOf",
        REFLECT_GET_TYPE_INFO => "Reflect.getTypeInfo",
        REFLECT_GET_CLASS_HIERARCHY => "Reflect.getClassHierarchy",
        REFLECT_GET_CLASSES_WITH_DECORATOR => "Reflect.getClassesWithDecorator",

        // Reflect - Field Access
        REFLECT_GET => "Reflect.get",
        REFLECT_SET => "Reflect.set",
        REFLECT_HAS => "Reflect.has",
        REFLECT_GET_FIELD_NAMES => "Reflect.getFieldNames",
        REFLECT_GET_FIELD_INFO => "Reflect.getFieldInfo",
        REFLECT_GET_FIELDS => "Reflect.getFields",
        REFLECT_GET_STATIC_FIELD_NAMES => "Reflect.getStaticFieldNames",
        REFLECT_GET_STATIC_FIELDS => "Reflect.getStaticFields",

        // Reflect - Method Invocation
        REFLECT_INVOKE => "Reflect.invoke",
        REFLECT_INVOKE_ASYNC => "Reflect.invokeAsync",
        REFLECT_GET_METHOD => "Reflect.getMethod",
        REFLECT_GET_METHOD_INFO => "Reflect.getMethodInfo",
        REFLECT_GET_METHODS => "Reflect.getMethods",
        REFLECT_HAS_METHOD => "Reflect.hasMethod",
        REFLECT_INVOKE_STATIC => "Reflect.invokeStatic",
        REFLECT_GET_STATIC_METHODS => "Reflect.getStaticMethods",

        // Reflect - Object Creation
        REFLECT_CONSTRUCT => "Reflect.construct",
        REFLECT_ALLOCATE => "Reflect.allocate",
        REFLECT_CLONE => "Reflect.clone",

        _ => "unknown",
    }
}
