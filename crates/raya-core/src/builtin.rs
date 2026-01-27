//! Built-in method definitions
//!
//! This module defines constants for built-in methods on primitive types
//! (arrays, strings, etc.) that are handled specially by the VM.

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
