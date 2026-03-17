//! Fast JSON parser that directly creates native VM Values
//!
//! This parser is optimized for performance:
//! - Single-pass parsing
//! - Minimal allocations
//! - Direct GC allocation of native types (Object, Array, RayaString)
//! - No intermediate representations

use crate::vm::gc::GarbageCollector;
use crate::vm::object::{
    layout_id_from_ordered_names, register_global_layout_names, Array, Object, PropKeyId,
    RayaString,
};
use crate::vm::value::Value;
use crate::vm::{VmError, VmResult};

/// Parse a JSON string into a native VM `Value`.
///
/// The result uses native heap types:
/// - null → `Value::null()`
/// - bool → `Value::bool(b)`
/// - number → `Value::f64(n)` (JSON numbers are floating-point)
/// - string → `GcPtr<RayaString>` → `Value`
/// - array → `GcPtr<Array>` with elements as `Value` → `Value`
/// - object → `GcPtr<Object>` with structural fields and/or dynamic props as `Value` → `Value`
pub fn parse(input: &str, gc: &mut GarbageCollector) -> VmResult<Value> {
    let mut parser = Parser::new(input, gc, None);
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

/// Parse JSON directly into unified `Object + dyn_props` carriers by interning
/// dynamic property keys through the provided callback.
pub fn parse_with_prop_key_interner(
    input: &str,
    gc: &mut GarbageCollector,
    intern_prop_key: &mut dyn FnMut(&str) -> PropKeyId,
) -> VmResult<Value> {
    let mut parser = Parser::new(input, gc, Some(intern_prop_key));
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
    intern_prop_key: Option<&'a mut dyn FnMut(&str) -> PropKeyId>,
}

impl<'a> Parser<'a> {
    fn new(
        input: &'a str,
        gc: &'a mut GarbageCollector,
        intern_prop_key: Option<&'a mut dyn FnMut(&str) -> PropKeyId>,
    ) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            gc,
            intern_prop_key,
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
        let n = num_str
            .parse::<f64>()
            .map_err(|_| VmError::RuntimeError(format!("Invalid number: {}", num_str)))?;
        Ok(Value::f64(n))
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
                length: 0,
                elements,
                present: vec![true; 0],
                sparse_elements: rustc_hash::FxHashMap::default(),
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
                    let len = elements.len();
                    let arr = Array {
                        type_id: 0,
                        length: len,
                        elements,
                        present: vec![true; len],
                        sparse_elements: rustc_hash::FxHashMap::default(),
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

        let mut entries = Vec::new();

        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'}' {
            self.pos += 1;
            return self.allocate_object(entries);
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
            entries.push((key, value));

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
                    return self.allocate_object(entries);
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

    fn allocate_object(&mut self, entries: Vec<(String, Value)>) -> VmResult<Value> {
        if let Some(intern_prop_key) = self.intern_prop_key.as_deref_mut() {
            let mut obj = Object::new_dynamic(layout_id_from_ordered_names(&[]), 0);
            {
                let dyn_props = obj.ensure_dyn_props();
                for (key, value) in entries {
                    dyn_props.insert(intern_prop_key(&key), crate::vm::object::DynProp::data(value));
                }
            }
            let obj_ptr = self.gc.allocate(obj);
            Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) })
        } else {
            let mut field_names = entries
                .iter()
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            field_names.sort_unstable();
            field_names.dedup();
            let layout_id = layout_id_from_ordered_names(&field_names);
            register_global_layout_names(layout_id, &field_names);
            let mut obj = Object::new_structural(layout_id, field_names.len());
            for (key, value) in entries {
                if let Some(index) = field_names.iter().position(|name| name == &key) {
                    let _ = obj.set_field(index, value);
                }
            }
            let obj_ptr = self.gc.allocate(obj);
            Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) })
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
        assert_eq!(result.as_f64(), Some(42.0));
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
                assert_eq!(arr.get(0).and_then(|v| v.as_f64()), Some(1.0));
                assert_eq!(arr.get(1).and_then(|v| v.as_f64()), Some(2.0));
                assert_eq!(arr.get(2).and_then(|v| v.as_f64()), Some(3.0));
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
            JSView::Struct { ptr, layout_id, .. } => {
                let obj = unsafe { &*ptr };
                let names =
                    crate::vm::object::global_layout_names(layout_id).expect("layout names");
                let name_index = names
                    .iter()
                    .position(|name| name == "name")
                    .expect("name field");
                let age_index = names
                    .iter()
                    .position(|name| name == "age")
                    .expect("age field");
                assert!(obj.get_field(name_index).is_some());
                assert_eq!(
                    obj.get_field(age_index).and_then(|v| v.as_f64()),
                    Some(30.0)
                );
            }
            _ => panic!("Expected Object"),
        }
    }

    #[test]
    fn test_parse_empty_object() {
        let mut gc = GarbageCollector::default();
        let result = parse("{}", &mut gc).unwrap();
        match js_classify(result) {
            JSView::Struct { ptr, .. } => assert_eq!(unsafe { &*ptr }.field_count(), 0),
            _ => panic!("Expected Object"),
        }
    }

    #[test]
    fn test_parse_nested() {
        let mut gc = GarbageCollector::default();
        let json = r#"{"user": {"name": "Alice", "tags": ["admin", "user"]}, "count": 42}"#;
        let result = parse(json, &mut gc).unwrap();
        match js_classify(result) {
            JSView::Struct { ptr, layout_id, .. } => {
                let obj = unsafe { &*ptr };
                let names =
                    crate::vm::object::global_layout_names(layout_id).expect("layout names");
                let user_val = obj
                    .get_field(names.iter().position(|name| name == "user").expect("user"))
                    .unwrap();
                match js_classify(user_val) {
                    JSView::Struct {
                        ptr: user_ptr,
                        layout_id: user_layout,
                        ..
                    } => {
                        let user_obj = unsafe { &*user_ptr };
                        let user_names = crate::vm::object::global_layout_names(user_layout)
                            .expect("user names");
                        let tags_val = user_obj
                            .get_field(
                                user_names
                                    .iter()
                                    .position(|name| name == "tags")
                                    .expect("tags"),
                            )
                            .unwrap();
                        match js_classify(tags_val) {
                            JSView::Arr(tags_ptr) => {
                                assert_eq!(unsafe { &*tags_ptr }.len(), 2);
                            }
                            _ => panic!("Expected array for tags"),
                        }
                    }
                    _ => panic!("Expected Object for user"),
                }
                let count_val = obj
                    .get_field(
                        names
                            .iter()
                            .position(|name| name == "count")
                            .expect("count"),
                    )
                    .unwrap();
                assert_eq!(count_val.as_f64(), Some(42.0));
            }
            _ => panic!("Expected Object"),
        }
    }

    #[test]
    fn test_parse_error() {
        let mut gc = GarbageCollector::default();
        assert!(parse("{invalid}", &mut gc).is_err());
        assert!(parse("nul", &mut gc).is_err());
    }
}
