//! Fast JSON parser that directly creates JsonValue
//!
//! This parser is optimized for performance:
//! - Single-pass parsing
//! - Minimal allocations
//! - Direct GC allocation during parsing
//! - No intermediate representations

use super::JsonValue;
use crate::gc::GarbageCollector;
use crate::object::RayaString;
use crate::{VmError, VmResult};
use rustc_hash::FxHashMap;

/// Parse a JSON string into a JsonValue
///
/// This is a fast, single-pass parser that directly creates JsonValue
/// with GC-managed allocations.
pub fn parse(input: &str, gc: &mut GarbageCollector) -> VmResult<JsonValue> {
    let mut parser = Parser::new(input, gc);
    parser.parse_value()
}

/// JSON parser state
struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    gc: &'a mut GarbageCollector,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, gc: &'a mut GarbageCollector) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            gc,
        }
    }

    /// Parse a JSON value (entry point)
    fn parse_value(&mut self) -> VmResult<JsonValue> {
        self.skip_whitespace();

        if self.pos >= self.bytes.len() {
            return Err(VmError::RuntimeError("Unexpected end of JSON".to_string()));
        }

        match self.bytes[self.pos] {
            b'n' => self.parse_null(),
            b't' | b'f' => self.parse_bool(),
            b'"' => self.parse_string(),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            c => Err(VmError::RuntimeError(format!(
                "Unexpected character '{}' at position {}",
                c as char, self.pos
            ))),
        }
    }

    /// Parse null
    fn parse_null(&mut self) -> VmResult<JsonValue> {
        if self.consume_literal("null") {
            Ok(JsonValue::Null)
        } else {
            Err(VmError::RuntimeError(format!(
                "Invalid null literal at position {}",
                self.pos
            )))
        }
    }

    /// Parse boolean
    fn parse_bool(&mut self) -> VmResult<JsonValue> {
        if self.consume_literal("true") {
            Ok(JsonValue::Bool(true))
        } else if self.consume_literal("false") {
            Ok(JsonValue::Bool(false))
        } else {
            Err(VmError::RuntimeError(format!(
                "Invalid boolean literal at position {}",
                self.pos
            )))
        }
    }

    /// Parse string
    fn parse_string(&mut self) -> VmResult<JsonValue> {
        if self.bytes[self.pos] != b'"' {
            return Err(VmError::RuntimeError(format!(
                "Expected '\"' at position {}",
                self.pos
            )));
        }
        self.pos += 1; // Skip opening quote

        let start = self.pos;
        let mut has_escapes = false;

        // Find end of string and check for escapes
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'"' => {
                    // Found closing quote
                    let end = self.pos;
                    self.pos += 1;

                    let string_data = if has_escapes {
                        // Need to unescape
                        self.unescape_string(&self.input[start..end])?
                    } else {
                        // No escapes, use slice directly
                        self.input[start..end].to_string()
                    };

                    let raya_str = RayaString { data: string_data };
                    let str_ptr = self.gc.allocate(raya_str);
                    return Ok(JsonValue::String(str_ptr));
                }
                b'\\' => {
                    has_escapes = true;
                    self.pos += 1; // Skip backslash
                    if self.pos >= self.bytes.len() {
                        return Err(VmError::RuntimeError(
                            "Unexpected end of string escape".to_string(),
                        ));
                    }
                    self.pos += 1; // Skip escaped character
                }
                b'\x00'..=b'\x1F' => {
                    return Err(VmError::RuntimeError(format!(
                        "Unescaped control character in string at position {}",
                        self.pos
                    )));
                }
                _ => {
                    self.pos += 1;
                }
            }
        }

        Err(VmError::RuntimeError("Unterminated string".to_string()))
    }

    /// Unescape a JSON string
    fn unescape_string(&self, s: &str) -> VmResult<String> {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars();

        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('"') => result.push('"'),
                    Some('\\') => result.push('\\'),
                    Some('/') => result.push('/'),
                    Some('b') => result.push('\x08'),
                    Some('f') => result.push('\x0C'),
                    Some('n') => result.push('\n'),
                    Some('r') => result.push('\r'),
                    Some('t') => result.push('\t'),
                    Some('u') => {
                        // Unicode escape \uXXXX
                        let hex: String = chars.by_ref().take(4).collect();
                        if hex.len() != 4 {
                            return Err(VmError::RuntimeError(
                                "Invalid unicode escape".to_string(),
                            ));
                        }
                        let code = u32::from_str_radix(&hex, 16).map_err(|_| {
                            VmError::RuntimeError("Invalid unicode hex digits".to_string())
                        })?;
                        if let Some(unicode_char) = char::from_u32(code) {
                            result.push(unicode_char);
                        } else {
                            return Err(VmError::RuntimeError(
                                "Invalid unicode code point".to_string(),
                            ));
                        }
                    }
                    Some(c) => {
                        return Err(VmError::RuntimeError(format!(
                            "Invalid escape sequence: \\{}",
                            c
                        )))
                    }
                    None => {
                        return Err(VmError::RuntimeError(
                            "Unexpected end of string".to_string(),
                        ))
                    }
                }
            } else {
                result.push(ch);
            }
        }

        Ok(result)
    }

    /// Parse number
    fn parse_number(&mut self) -> VmResult<JsonValue> {
        let start = self.pos;

        // Optional minus
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'-' {
            self.pos += 1;
        }

        // Integer part
        if self.pos >= self.bytes.len() {
            return Err(VmError::RuntimeError(
                "Unexpected end of number".to_string(),
            ));
        }

        if self.bytes[self.pos] == b'0' {
            self.pos += 1;
        } else if self.bytes[self.pos].is_ascii_digit() {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        } else {
            return Err(VmError::RuntimeError(format!(
                "Invalid number at position {}",
                self.pos
            )));
        }

        // Fractional part
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' {
            self.pos += 1;
            if self.pos >= self.bytes.len() || !self.bytes[self.pos].is_ascii_digit() {
                return Err(VmError::RuntimeError(
                    "Invalid number: digit expected after '.'".to_string(),
                ));
            }
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }

        // Exponent part
        if self.pos < self.bytes.len()
            && (self.bytes[self.pos] == b'e' || self.bytes[self.pos] == b'E')
        {
            self.pos += 1;
            if self.pos < self.bytes.len()
                && (self.bytes[self.pos] == b'+' || self.bytes[self.pos] == b'-')
            {
                self.pos += 1;
            }
            if self.pos >= self.bytes.len() || !self.bytes[self.pos].is_ascii_digit() {
                return Err(VmError::RuntimeError(
                    "Invalid number: digit expected in exponent".to_string(),
                ));
            }
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }

        let num_str = &self.input[start..self.pos];
        let num = num_str
            .parse::<f64>()
            .map_err(|_| VmError::RuntimeError(format!("Invalid number: {}", num_str)))?;

        Ok(JsonValue::Number(num))
    }

    /// Parse array
    fn parse_array(&mut self) -> VmResult<JsonValue> {
        if self.bytes[self.pos] != b'[' {
            return Err(VmError::RuntimeError(format!(
                "Expected '[' at position {}",
                self.pos
            )));
        }
        self.pos += 1;
        self.skip_whitespace();

        let mut elements = Vec::new();

        // Empty array
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b']' {
            self.pos += 1;
            let arr_ptr = self.gc.allocate(elements);
            return Ok(JsonValue::Array(arr_ptr));
        }

        loop {
            // Parse element
            let value = self.parse_value()?;
            elements.push(value);

            self.skip_whitespace();

            if self.pos >= self.bytes.len() {
                return Err(VmError::RuntimeError("Unterminated array".to_string()));
            }

            match self.bytes[self.pos] {
                b',' => {
                    self.pos += 1;
                    self.skip_whitespace();
                }
                b']' => {
                    self.pos += 1;
                    let arr_ptr = self.gc.allocate(elements);
                    return Ok(JsonValue::Array(arr_ptr));
                }
                c => {
                    return Err(VmError::RuntimeError(format!(
                        "Expected ',' or ']' in array, got '{}' at position {}",
                        c as char, self.pos
                    )))
                }
            }
        }
    }

    /// Parse object
    fn parse_object(&mut self) -> VmResult<JsonValue> {
        if self.bytes[self.pos] != b'{' {
            return Err(VmError::RuntimeError(format!(
                "Expected '{{' at position {}",
                self.pos
            )));
        }
        self.pos += 1;
        self.skip_whitespace();

        let mut object = FxHashMap::default();

        // Empty object
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'}' {
            self.pos += 1;
            let obj_ptr = self.gc.allocate(object);
            return Ok(JsonValue::Object(obj_ptr));
        }

        loop {
            // Parse key (must be string)
            self.skip_whitespace();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'"' {
                return Err(VmError::RuntimeError(format!(
                    "Expected string key at position {}",
                    self.pos
                )));
            }

            let key_value = self.parse_string()?;
            let key = match key_value {
                JsonValue::String(s_ptr) => {
                    let s = unsafe { &*s_ptr.as_ptr() };
                    s.data.clone()
                }
                _ => unreachable!("parse_string always returns String"),
            };

            // Expect colon
            self.skip_whitespace();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b':' {
                return Err(VmError::RuntimeError(format!(
                    "Expected ':' after object key at position {}",
                    self.pos
                )));
            }
            self.pos += 1;

            // Parse value
            self.skip_whitespace();
            let value = self.parse_value()?;
            object.insert(key, value);

            self.skip_whitespace();

            if self.pos >= self.bytes.len() {
                return Err(VmError::RuntimeError("Unterminated object".to_string()));
            }

            match self.bytes[self.pos] {
                b',' => {
                    self.pos += 1;
                    self.skip_whitespace();
                }
                b'}' => {
                    self.pos += 1;
                    let obj_ptr = self.gc.allocate(object);
                    return Ok(JsonValue::Object(obj_ptr));
                }
                c => {
                    return Err(VmError::RuntimeError(format!(
                        "Expected ',' or '}}' in object, got '{}' at position {}",
                        c as char, self.pos
                    )))
                }
            }
        }
    }

    /// Skip whitespace
    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    /// Try to consume a literal string
    fn consume_literal(&mut self, literal: &str) -> bool {
        let literal_bytes = literal.as_bytes();
        if self.pos + literal_bytes.len() > self.bytes.len() {
            return false;
        }

        if &self.bytes[self.pos..self.pos + literal_bytes.len()] == literal_bytes {
            self.pos += literal_bytes.len();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::GarbageCollector;

    #[test]
    fn test_parse_null() {
        let mut gc = GarbageCollector::default();
        let result = parse("null", &mut gc).unwrap();
        assert!(result.is_null());
    }

    #[test]
    fn test_parse_bool() {
        let mut gc = GarbageCollector::default();

        let result = parse("true", &mut gc).unwrap();
        assert_eq!(result.as_bool(), Some(true));

        let result = parse("false", &mut gc).unwrap();
        assert_eq!(result.as_bool(), Some(false));
    }

    #[test]
    fn test_parse_number() {
        let mut gc = GarbageCollector::default();

        let result = parse("42", &mut gc).unwrap();
        assert_eq!(result.as_number(), Some(42.0));

        let result = parse("-17.5", &mut gc).unwrap();
        assert_eq!(result.as_number(), Some(-17.5));

        let result = parse("3.14e2", &mut gc).unwrap();
        assert_eq!(result.as_number(), Some(314.0));
    }

    #[test]
    fn test_parse_string() {
        let mut gc = GarbageCollector::default();

        let result = parse("\"hello\"", &mut gc).unwrap();
        assert!(result.is_string());

        let result = parse("\"hello\\nworld\"", &mut gc).unwrap();
        let s_ptr = result.as_string().unwrap();
        let s = unsafe { &*s_ptr.as_ptr() };
        assert_eq!(s.data, "hello\nworld");
    }

    #[test]
    fn test_parse_array() {
        let mut gc = GarbageCollector::default();

        let result = parse("[1, 2, 3]", &mut gc).unwrap();
        assert!(result.is_array());

        let arr_ptr = result.as_array().unwrap();
        let arr = unsafe { &*arr_ptr.as_ptr() };
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_number(), Some(1.0));
        assert_eq!(arr[1].as_number(), Some(2.0));
        assert_eq!(arr[2].as_number(), Some(3.0));
    }

    #[test]
    fn test_parse_object() {
        let mut gc = GarbageCollector::default();

        let result = parse("{\"name\": \"Alice\", \"age\": 30}", &mut gc).unwrap();
        assert!(result.is_object());

        let name = result.get_property("name");
        assert!(name.is_string());

        let age = result.get_property("age");
        assert_eq!(age.as_number(), Some(30.0));
    }

    #[test]
    fn test_parse_nested() {
        let mut gc = GarbageCollector::default();

        let json = r#"
        {
            "user": {
                "name": "Alice",
                "tags": ["admin", "user"]
            },
            "count": 42
        }
        "#;

        let result = parse(json, &mut gc).unwrap();
        assert!(result.is_object());

        let user = result.get_property("user");
        assert!(user.is_object());

        let tags = user.get_property("tags");
        assert!(tags.is_array());
    }

    #[test]
    fn test_parse_error() {
        let mut gc = GarbageCollector::default();

        assert!(parse("{invalid}", &mut gc).is_err());
        assert!(parse("[1, 2,]", &mut gc).is_err());
        assert!(parse("nul", &mut gc).is_err());
    }
}
