//! Fast JSON parser that directly creates native VM Values
//!
//! This parser is optimized for performance:
//! - Single-pass parsing
//! - Minimal allocations
//! - Direct GC allocation of native types (DynObject, Array, RayaString)
//! - No intermediate representations

use crate::vm::gc::GarbageCollector;
use crate::vm::object::{Array, DynObject, RayaString};
use crate::vm::value::Value;
use crate::vm::{VmError, VmResult};

/// Parse a JSON string into a native VM `Value`.
///
/// The result uses native heap types:
/// - null → `Value::null()`
/// - bool → `Value::bool(b)`
/// - number → `Value::i32(n)` for integers, `Value::f64(n)` otherwise
/// - string → `GcPtr<RayaString>` → `Value`
/// - array → `GcPtr<Array>` with elements as `Value` → `Value`
/// - object → `GcPtr<DynObject>` with props as `Value` → `Value`
pub fn parse(input: &str, gc: &mut GarbageCollector) -> VmResult<Value> {
    let mut parser = Parser::new(input, gc);
    let val = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.pos < parser.bytes.len() {
        return Err(VmError::RuntimeError(format!(
            "Unexpected trailing characters at position {}",
            parser.pos
        )));
    }
    Ok(val)
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

    /// Parse a JSON value
    fn parse_value(&mut self) -> VmResult<Value> {
        self.skip_whitespace();

        if self.pos >= self.bytes.len() {
            return Err(VmError::RuntimeError("Unexpected end of JSON".to_string()));
        }

        match self.bytes[self.pos] {
            b'n' => self.parse_null(),
            b't' | b'f' => self.parse_bool(),
            b'"' => self.parse_string_value(),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            c => Err(VmError::RuntimeError(format!(
                "Unexpected character '{}' at position {}",
                c as char, self.pos
            ))),
        }
    }

    fn parse_null(&mut self) -> VmResult<Value> {
        if self.consume_literal("null") {
            Ok(Value::null())
        } else {
            Err(VmError::RuntimeError(format!(
                "Invalid null literal at position {}",
                self.pos
            )))
        }
    }

    fn parse_bool(&mut self) -> VmResult<Value> {
        if self.consume_literal("true") {
            Ok(Value::bool(true))
        } else if self.consume_literal("false") {
            Ok(Value::bool(false))
        } else {
            Err(VmError::RuntimeError(format!(
                "Invalid boolean literal at position {}",
                self.pos
            )))
        }
    }

    fn parse_number(&mut self) -> VmResult<Value> {
        let start = self.pos;

        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'-' {
            self.pos += 1;
        }

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

        let mut is_float = false;

        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' {
            is_float = true;
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

        if self.pos < self.bytes.len()
            && (self.bytes[self.pos] == b'e' || self.bytes[self.pos] == b'E')
        {
            is_float = true;
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
        let n = num_str
            .parse::<f64>()
            .map_err(|_| VmError::RuntimeError(format!("Invalid number: {}", num_str)))?;

        // Represent integers as i32 when possible (faster VM operations)
        if !is_float && n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64 {
            Ok(Value::i32(n as i32))
        } else {
            Ok(Value::f64(n))
        }
    }

    /// Parse a JSON string and return it as a VM `Value` (allocates RayaString).
    fn parse_string_value(&mut self) -> VmResult<Value> {
        let data = self.read_string_data()?;
        let raya_str = RayaString::new(data);
        let gc_ptr = self.gc.allocate(raya_str);
        Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) })
    }

    /// Parse a JSON string and return the raw Rust `String` (used for object keys).
    fn read_string_data(&mut self) -> VmResult<String> {
        if self.bytes[self.pos] != b'"' {
            return Err(VmError::RuntimeError(format!(
                "Expected '\"' at position {}",
                self.pos
            )));
        }
        self.pos += 1;

        let start = self.pos;
        let mut has_escapes = false;

        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'"' => {
                    let end = self.pos;
                    self.pos += 1;
                    if has_escapes {
                        return self.unescape_string(&self.input[start..end]);
                    } else {
                        return Ok(self.input[start..end].to_string());
                    }
                }
                b'\\' => {
                    has_escapes = true;
                    self.pos += 1;
                    if self.pos >= self.bytes.len() {
                        return Err(VmError::RuntimeError(
                            "Unexpected end of string escape".to_string(),
                        ));
                    }
                    self.pos += 1;
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

    fn parse_array(&mut self) -> VmResult<Value> {
        debug_assert_eq!(self.bytes[self.pos], b'[');
        self.pos += 1;
        self.skip_whitespace();

        let mut elements: Vec<Value> = Vec::new();

        if self.pos < self.bytes.len() && self.bytes[self.pos] == b']' {
            self.pos += 1;
            let arr = Array {
                type_id: 0,
                elements,
            };
            let arr_ptr = self.gc.allocate(arr);
            return Ok(unsafe {
                Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap())
            });
        }

        loop {
            let elem = self.parse_value()?;
            elements.push(elem);
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
                    let arr = Array {
                        type_id: 0,
                        elements,
                    };
                    let arr_ptr = self.gc.allocate(arr);
                    return Ok(unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(arr_ptr.as_ptr()).unwrap())
                    });
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

    fn parse_object(&mut self) -> VmResult<Value> {
        debug_assert_eq!(self.bytes[self.pos], b'{');
        self.pos += 1;
        self.skip_whitespace();

        let mut obj = DynObject::new();

        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'}' {
            self.pos += 1;
            let obj_ptr = self.gc.allocate(obj);
            return Ok(unsafe {
                Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap())
            });
        }

        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'"' {
                return Err(VmError::RuntimeError(format!(
                    "Expected string key at position {}",
                    self.pos
                )));
            }

            let key = self.read_string_data()?;

            self.skip_whitespace();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b':' {
                return Err(VmError::RuntimeError(format!(
                    "Expected ':' after object key at position {}",
                    self.pos
                )));
            }
            self.pos += 1;

            self.skip_whitespace();
            let value = self.parse_value()?;
            obj.set(key, value);

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
                    let obj_ptr = self.gc.allocate(obj);
                    return Ok(unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap())
                    });
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

    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn consume_literal(&mut self, literal: &str) -> bool {
        let lb = literal.as_bytes();
        if self.pos + lb.len() > self.bytes.len() {
            return false;
        }
        if &self.bytes[self.pos..self.pos + lb.len()] == lb {
            self.pos += lb.len();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::gc::GarbageCollector;
    use crate::vm::json::view::{js_classify, JSView};

    #[test]
    fn test_parse_null() {
        let mut gc = GarbageCollector::default();
        let result = parse("null", &mut gc).unwrap();
        assert!(result.is_null());
    }

    #[test]
    fn test_parse_bool() {
        let mut gc = GarbageCollector::default();
        assert_eq!(parse("true", &mut gc).unwrap().as_bool(), Some(true));
        assert_eq!(parse("false", &mut gc).unwrap().as_bool(), Some(false));
    }

    #[test]
    fn test_parse_number_integer() {
        let mut gc = GarbageCollector::default();
        let result = parse("42", &mut gc).unwrap();
        assert_eq!(result.as_i32(), Some(42));
    }

    #[test]
    fn test_parse_number_float() {
        let mut gc = GarbageCollector::default();
        let result = parse("-17.5", &mut gc).unwrap();
        assert_eq!(result.as_f64(), Some(-17.5));
        let result = parse("3.14e2", &mut gc).unwrap();
        assert_eq!(result.as_f64(), Some(314.0));
    }

    #[test]
    fn test_parse_string() {
        let mut gc = GarbageCollector::default();
        let result = parse("\"hello\"", &mut gc).unwrap();
        match js_classify(result) {
            JSView::Str(ptr) => assert_eq!(unsafe { &*ptr }.data, "hello"),
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_parse_string_escape() {
        let mut gc = GarbageCollector::default();
        let result = parse("\"hello\\nworld\"", &mut gc).unwrap();
        match js_classify(result) {
            JSView::Str(ptr) => assert_eq!(unsafe { &*ptr }.data, "hello\nworld"),
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_parse_array() {
        let mut gc = GarbageCollector::default();
        let result = parse("[1, 2, 3]", &mut gc).unwrap();
        match js_classify(result) {
            JSView::Arr(ptr) => {
                let arr = unsafe { &*ptr };
                assert_eq!(arr.len(), 3);
                assert_eq!(arr.get(0).and_then(|v| v.as_i32()), Some(1));
                assert_eq!(arr.get(1).and_then(|v| v.as_i32()), Some(2));
                assert_eq!(arr.get(2).and_then(|v| v.as_i32()), Some(3));
            }
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parse_empty_array() {
        let mut gc = GarbageCollector::default();
        let result = parse("[]", &mut gc).unwrap();
        match js_classify(result) {
            JSView::Arr(ptr) => assert_eq!(unsafe { &*ptr }.len(), 0),
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parse_object() {
        let mut gc = GarbageCollector::default();
        let result = parse(r#"{"name": "Alice", "age": 30}"#, &mut gc).unwrap();
        match js_classify(result) {
            JSView::Dyn(ptr) => {
                let obj = unsafe { &*ptr };
                assert!(obj.has("name"));
                assert_eq!(obj.get("age").and_then(|v| v.as_i32()), Some(30));
            }
            _ => panic!("Expected DynObject"),
        }
    }

    #[test]
    fn test_parse_empty_object() {
        let mut gc = GarbageCollector::default();
        let result = parse("{}", &mut gc).unwrap();
        match js_classify(result) {
            JSView::Dyn(ptr) => assert!(unsafe { &*ptr }.props.is_empty()),
            _ => panic!("Expected DynObject"),
        }
    }

    #[test]
    fn test_parse_nested() {
        let mut gc = GarbageCollector::default();
        let json = r#"{"user": {"name": "Alice", "tags": ["admin", "user"]}, "count": 42}"#;
        let result = parse(json, &mut gc).unwrap();
        match js_classify(result) {
            JSView::Dyn(ptr) => {
                let obj = unsafe { &*ptr };
                let user_val = obj.get("user").unwrap();
                match js_classify(user_val) {
                    JSView::Dyn(user_ptr) => {
                        let user_obj = unsafe { &*user_ptr };
                        assert!(user_obj.has("name"));
                        let tags_val = user_obj.get("tags").unwrap();
                        match js_classify(tags_val) {
                            JSView::Arr(tags_ptr) => {
                                assert_eq!(unsafe { &*tags_ptr }.len(), 2);
                            }
                            _ => panic!("Expected array for tags"),
                        }
                    }
                    _ => panic!("Expected DynObject for user"),
                }
                assert_eq!(obj.get("count").and_then(|v| v.as_i32()), Some(42));
            }
            _ => panic!("Expected DynObject"),
        }
    }

    #[test]
    fn test_parse_error() {
        let mut gc = GarbageCollector::default();
        assert!(parse("{invalid}", &mut gc).is_err());
        assert!(parse("nul", &mut gc).is_err());
    }
}
