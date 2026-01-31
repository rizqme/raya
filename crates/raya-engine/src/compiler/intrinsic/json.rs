//! JSON Compile-Time Intrinsics
//!
//! Provides compile-time code generation for type-safe JSON encoding/decoding.
//!
//! ## API
//!
//! ```typescript
//! // Global built-in - no import needed
//! JSON.stringify(value)          // any -> string (runtime)
//! JSON.parse(jsonString)         // string -> any (runtime)
//! JSON.encode<T>(value: T)       // T -> Result<string, Error> (compile-time codegen)
//! JSON.decode<T>(json: string)   // string -> Result<T, Error> (compile-time codegen)
//! ```
//!
//! Note: Field mapping with @json decorator will be added in a future milestone.

/// JSON intrinsic handler for compile-time code generation
pub struct JsonIntrinsic;

impl JsonIntrinsic {
    /// Check if this is a JSON intrinsic call
    ///
    /// Returns the intrinsic type if matched:
    /// - "stringify" - runtime JSON.stringify
    /// - "parse" - runtime JSON.parse
    /// - "encode" - compile-time JSON.encode<T>
    /// - "decode" - compile-time JSON.decode<T>
    pub fn detect_intrinsic(object_name: &str, method_name: &str) -> Option<&'static str> {
        if object_name != "JSON" {
            return None;
        }
        match method_name {
            "stringify" => Some("stringify"),
            "parse" => Some("parse"),
            "encode" => Some("encode"),
            "decode" => Some("decode"),
            _ => None,
        }
    }

    /// Check if the method requires compile-time code generation
    pub fn is_compile_time(method_name: &str) -> bool {
        matches!(method_name, "encode" | "decode")
    }

    /// Check if the method is a runtime call (delegates to VM)
    pub fn is_runtime(method_name: &str) -> bool {
        matches!(method_name, "stringify" | "parse")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_json_intrinsic() {
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "stringify"), Some("stringify"));
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "parse"), Some("parse"));
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "encode"), Some("encode"));
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "decode"), Some("decode"));
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "unknown"), None);
        assert_eq!(JsonIntrinsic::detect_intrinsic("Other", "stringify"), None);
    }

    #[test]
    fn test_compile_time_vs_runtime() {
        assert!(JsonIntrinsic::is_compile_time("encode"));
        assert!(JsonIntrinsic::is_compile_time("decode"));
        assert!(!JsonIntrinsic::is_compile_time("stringify"));
        assert!(!JsonIntrinsic::is_compile_time("parse"));

        assert!(JsonIntrinsic::is_runtime("stringify"));
        assert!(JsonIntrinsic::is_runtime("parse"));
        assert!(!JsonIntrinsic::is_runtime("encode"));
        assert!(!JsonIntrinsic::is_runtime("decode"));
    }
}
