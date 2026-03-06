//! JSON intrinsic surface
//!
//! Only JavaScript-compatible JSON methods are supported:
//! - `JSON.stringify(value)`
//! - `JSON.parse(jsonString)`

/// JSON intrinsic handler for runtime JSON operations.
pub struct JsonIntrinsic;

impl JsonIntrinsic {
    /// Check if this is a supported JSON intrinsic call.
    pub fn detect_intrinsic(object_name: &str, method_name: &str) -> Option<&'static str> {
        if object_name != "JSON" {
            return None;
        }
        match method_name {
            "stringify" => Some("stringify"),
            "parse" => Some("parse"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_json_intrinsic() {
        assert_eq!(
            JsonIntrinsic::detect_intrinsic("JSON", "stringify"),
            Some("stringify")
        );
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "parse"), Some("parse"));
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "encode"), None);
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "decode"), None);
        assert_eq!(JsonIntrinsic::detect_intrinsic("JSON", "unknown"), None);
        assert_eq!(JsonIntrinsic::detect_intrinsic("Other", "stringify"), None);
    }
}
