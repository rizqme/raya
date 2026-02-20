//! Template module implementation (std:template)
//!
//! A simple Mustache-like template engine implemented in pure Rust.
//! Supports variable interpolation, sections, inverted sections,
//! and raw (unescaped) variables. Data is provided as JSON strings
//! parsed with serde_json.

use parking_lot::Mutex;
use raya_sdk::{NativeCallResult, NativeContext, NativeValue};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

// ============================================================================
// Template token types
// ============================================================================

/// A parsed template token
#[derive(Clone, Debug)]
enum TemplateToken {
    /// Literal text
    Text(String),
    /// Variable substitution: `{{name}}` (HTML-escaped)
    Variable(String),
    /// Raw variable substitution: `{{{name}}}` (unescaped)
    RawVariable(String),
    /// Section start: `{{#name}}`
    SectionStart(String),
    /// Section end: `{{/name}}`
    SectionEnd(String),
    /// Inverted section start: `{{^name}}`
    InvertedStart(String),
}

// ============================================================================
// Compiled template storage
// ============================================================================

/// Next handle ID for compiled templates
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Global store of compiled templates keyed by handle ID
static TEMPLATES: LazyLock<Mutex<HashMap<u64, Vec<TemplateToken>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ============================================================================
// Public dispatch
// ============================================================================

/// Handle template method calls by ID
///
/// Routes native call IDs (0xB000-0xB003) to the appropriate handler.
pub fn call_template_method(
    ctx: &dyn NativeContext,
    id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match id {
        0xB000 => template_compile(ctx, args),
        0xB001 => template_render(ctx, args),
        0xB002 => compiled_render(ctx, args),
        0xB003 => compiled_release(ctx, args),
        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// Native function implementations
// ============================================================================

/// Compile a template source string and store it, returning a handle ID
///
/// Args: (source: string)
/// Returns: number (handle ID)
fn template_compile(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "template.compile requires 1 argument (source)".to_string(),
        );
    }

    let source = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!("template.compile: invalid source: {}", e))
        }
    };

    let tokens = match parse_template(&source) {
        Ok(t) => t,
        Err(e) => return NativeCallResult::Error(format!("template.compile: {}", e)),
    };

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    TEMPLATES.lock().insert(id, tokens);
    NativeCallResult::f64(id as f64)
}

/// Render a template source string with JSON data (one-shot, no compilation)
///
/// Args: (source: string, data: string)
/// Returns: string (rendered output)
fn template_render(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "template.render requires 2 arguments (source, data)".to_string(),
        );
    }

    let source = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!("template.render: invalid source: {}", e))
        }
    };

    let data_str = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!("template.render: invalid data: {}", e))
        }
    };

    let tokens = match parse_template(&source) {
        Ok(t) => t,
        Err(e) => return NativeCallResult::Error(format!("template.render: {}", e)),
    };

    let data: JsonValue = match serde_json::from_str(&data_str) {
        Ok(v) => v,
        Err(e) => {
            return NativeCallResult::Error(format!("template.render: invalid JSON: {}", e))
        }
    };

    match render_tokens(&tokens, &data, &[&data]) {
        Ok(output) => NativeCallResult::Value(ctx.create_string(&output)),
        Err(e) => NativeCallResult::Error(format!("template.render: {}", e)),
    }
}

/// Render a pre-compiled template with JSON data
///
/// Args: (handle: number, data: string)
/// Returns: string (rendered output)
fn compiled_render(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "template.compiledRender requires 2 arguments (handle, data)".to_string(),
        );
    }

    let handle = match args[0].as_f64().or_else(|| args[0].as_i32().map(|i| i as f64)) {
        Some(f) => f as u64,
        None => {
            return NativeCallResult::Error(
                "template.compiledRender: first argument must be a number (handle)".to_string(),
            )
        }
    };

    let data_str = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "template.compiledRender: invalid data: {}",
                e
            ))
        }
    };

    let data: JsonValue = match serde_json::from_str(&data_str) {
        Ok(v) => v,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "template.compiledRender: invalid JSON: {}",
                e
            ))
        }
    };

    let templates = TEMPLATES.lock();
    let tokens = match templates.get(&handle) {
        Some(t) => t,
        None => {
            return NativeCallResult::Error(format!(
                "template.compiledRender: invalid handle {}",
                handle
            ))
        }
    };

    match render_tokens(tokens, &data, &[&data]) {
        Ok(output) => NativeCallResult::Value(ctx.create_string(&output)),
        Err(e) => NativeCallResult::Error(format!("template.compiledRender: {}", e)),
    }
}

/// Release a compiled template handle
///
/// Args: (handle: number)
/// Returns: null
fn compiled_release(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "template.compiledRelease requires 1 argument (handle)".to_string(),
        );
    }

    let handle = match args[0].as_f64().or_else(|| args[0].as_i32().map(|i| i as f64)) {
        Some(f) => f as u64,
        None => {
            return NativeCallResult::Error(
                "template.compiledRelease: argument must be a number (handle)".to_string(),
            )
        }
    };

    TEMPLATES.lock().remove(&handle);
    NativeCallResult::null()
}

// ============================================================================
// Template parser
// ============================================================================

/// Parse a template source string into a list of tokens
fn parse_template(source: &str) -> Result<Vec<TemplateToken>, String> {
    let mut tokens = Vec::new();
    let mut pos = 0;
    let bytes = source.as_bytes();
    let len = bytes.len();

    while pos < len {
        // Look for the start of a tag: {{ or {{{
        if let Some(tag_start) = find_substr(bytes, pos, b"{{") {
            // Emit any text before the tag
            if tag_start > pos {
                tokens.push(TemplateToken::Text(source[pos..tag_start].to_string()));
            }

            // Check for triple-brace raw variable: {{{name}}}
            if tag_start + 2 < len && bytes[tag_start + 2] == b'{' {
                // Find closing }}}
                let content_start = tag_start + 3;
                if let Some(close) = find_substr(bytes, content_start, b"}}}") {
                    let name = source[content_start..close].trim().to_string();
                    if name.is_empty() {
                        return Err("empty raw variable name".to_string());
                    }
                    tokens.push(TemplateToken::RawVariable(name));
                    pos = close + 3;
                } else {
                    return Err(format!(
                        "unclosed raw variable tag starting at position {}",
                        tag_start
                    ));
                }
            } else {
                // Regular {{ ... }} tag
                let content_start = tag_start + 2;
                if let Some(close) = find_substr(bytes, content_start, b"}}") {
                    let content = source[content_start..close].trim();
                    if content.is_empty() {
                        return Err(format!(
                            "empty tag at position {}",
                            tag_start
                        ));
                    }

                    let first_char = content.as_bytes()[0];
                    match first_char {
                        b'#' => {
                            let name = content[1..].trim().to_string();
                            if name.is_empty() {
                                return Err("empty section name".to_string());
                            }
                            tokens.push(TemplateToken::SectionStart(name));
                        }
                        b'/' => {
                            let name = content[1..].trim().to_string();
                            if name.is_empty() {
                                return Err("empty section end name".to_string());
                            }
                            tokens.push(TemplateToken::SectionEnd(name));
                        }
                        b'^' => {
                            let name = content[1..].trim().to_string();
                            if name.is_empty() {
                                return Err("empty inverted section name".to_string());
                            }
                            tokens.push(TemplateToken::InvertedStart(name));
                        }
                        _ => {
                            tokens.push(TemplateToken::Variable(content.to_string()));
                        }
                    }
                    pos = close + 2;
                } else {
                    return Err(format!(
                        "unclosed tag starting at position {}",
                        tag_start
                    ));
                }
            }
        } else {
            // No more tags — emit the rest as text
            tokens.push(TemplateToken::Text(source[pos..].to_string()));
            break;
        }
    }

    Ok(tokens)
}

/// Find a byte substring starting from a given position
fn find_substr(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || start + needle.len() > haystack.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + start)
}

// ============================================================================
// Template renderer
// ============================================================================

/// Render a list of tokens with a JSON data context
///
/// `data` is the root data object. `context_stack` provides scoped
/// lookup for nested sections (closest scope first when searching).
fn render_tokens(
    tokens: &[TemplateToken],
    data: &JsonValue,
    context_stack: &[&JsonValue],
) -> Result<String, String> {
    let mut output = String::new();
    let mut i = 0;

    while i < tokens.len() {
        match &tokens[i] {
            TemplateToken::Text(text) => {
                output.push_str(text);
                i += 1;
            }
            TemplateToken::Variable(name) => {
                if name == "." {
                    // Current context value
                    if let Some(ctx) = context_stack.last() {
                        output.push_str(&html_escape(&json_to_string(ctx)));
                    }
                } else {
                    let val = lookup_value(name, context_stack);
                    output.push_str(&html_escape(&json_to_string(val)));
                }
                i += 1;
            }
            TemplateToken::RawVariable(name) => {
                if name == "." {
                    if let Some(ctx) = context_stack.last() {
                        output.push_str(&json_to_string(ctx));
                    }
                } else {
                    let val = lookup_value(name, context_stack);
                    output.push_str(&json_to_string(val));
                }
                i += 1;
            }
            TemplateToken::SectionStart(name) => {
                // Find matching SectionEnd
                let (section_tokens, end_idx) = extract_section(tokens, i, name)?;

                let val = lookup_value(name, context_stack);

                match val {
                    JsonValue::Array(arr) => {
                        // Iterate over array elements
                        for item in arr {
                            let mut new_stack: Vec<&JsonValue> = context_stack.to_vec();
                            new_stack.push(item);
                            output.push_str(&render_tokens(
                                &section_tokens,
                                data,
                                &new_stack,
                            )?);
                        }
                    }
                    JsonValue::Object(_) => {
                        // Push object as context
                        let mut new_stack: Vec<&JsonValue> = context_stack.to_vec();
                        new_stack.push(val);
                        output.push_str(&render_tokens(
                            &section_tokens,
                            data,
                            &new_stack,
                        )?);
                    }
                    JsonValue::Bool(true) => {
                        output.push_str(&render_tokens(
                            &section_tokens,
                            data,
                            context_stack,
                        )?);
                    }
                    JsonValue::String(s) if !s.is_empty() => {
                        output.push_str(&render_tokens(
                            &section_tokens,
                            data,
                            context_stack,
                        )?);
                    }
                    JsonValue::Number(_) => {
                        output.push_str(&render_tokens(
                            &section_tokens,
                            data,
                            context_stack,
                        )?);
                    }
                    _ => {
                        // null, false, empty string — do not render
                    }
                }

                i = end_idx + 1;
            }
            TemplateToken::InvertedStart(name) => {
                // Find matching SectionEnd
                let (section_tokens, end_idx) = extract_section(tokens, i, name)?;

                let val = lookup_value(name, context_stack);
                let render = is_falsy(val);

                if render {
                    output.push_str(&render_tokens(
                        &section_tokens,
                        data,
                        context_stack,
                    )?);
                }

                i = end_idx + 1;
            }
            TemplateToken::SectionEnd(name) => {
                return Err(format!("unexpected section end: {{{{/{}}}}}", name));
            }
        }
    }

    Ok(output)
}

/// Extract tokens between a section start and its matching section end
///
/// Returns the inner tokens and the index of the SectionEnd token.
fn extract_section(
    tokens: &[TemplateToken],
    start: usize,
    name: &str,
) -> Result<(Vec<TemplateToken>, usize), String> {
    let mut depth = 1;
    let mut j = start + 1;

    while j < tokens.len() {
        match &tokens[j] {
            TemplateToken::SectionStart(n) | TemplateToken::InvertedStart(n) if n == name => {
                depth += 1;
            }
            TemplateToken::SectionEnd(n) if n == name => {
                depth -= 1;
                if depth == 0 {
                    let section_tokens = tokens[start + 1..j].to_vec();
                    return Ok((section_tokens, j));
                }
            }
            _ => {}
        }
        j += 1;
    }

    Err(format!("unclosed section: {{{{#{}}}}}", name))
}

/// Look up a value by dot-notation path in the context stack
///
/// Searches from the top (most recent) context down to the root.
fn lookup_value<'a>(path: &str, context_stack: &[&'a JsonValue]) -> &'a JsonValue {
    static NULL: JsonValue = JsonValue::Null;

    // Search from top of stack down
    for ctx in context_stack.iter().rev() {
        let val = resolve_path(ctx, path);
        if !val.is_null() {
            return val;
        }
    }

    &NULL
}

/// Resolve a dot-notation path against a single JSON value
fn resolve_path<'a>(value: &'a JsonValue, path: &str) -> &'a JsonValue {
    static NULL: JsonValue = JsonValue::Null;

    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in parts {
        match current {
            JsonValue::Object(map) => {
                if let Some(v) = map.get(part) {
                    current = v;
                } else {
                    return &NULL;
                }
            }
            JsonValue::Array(arr) => {
                // Allow numeric index access on arrays
                if let Ok(idx) = part.parse::<usize>() {
                    if let Some(v) = arr.get(idx) {
                        current = v;
                    } else {
                        return &NULL;
                    }
                } else {
                    return &NULL;
                }
            }
            _ => return &NULL,
        }
    }

    current
}

/// Check if a JSON value is falsy (for inverted sections)
fn is_falsy(val: &JsonValue) -> bool {
    match val {
        JsonValue::Null => true,
        JsonValue::Bool(false) => true,
        JsonValue::Array(arr) => arr.is_empty(),
        JsonValue::String(s) => s.is_empty(),
        _ => false,
    }
}

/// Convert a JSON value to its string representation for output
fn json_to_string(val: &JsonValue) -> String {
    match val {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Null => String::new(),
        _ => val.to_string(),
    }
}

/// HTML-escape a string for safe insertion into HTML
fn html_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#x27;"),
            _ => result.push(ch),
        }
    }
    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_only() {
        let tokens = parse_template("Hello, world!").unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0], TemplateToken::Text(s) if s == "Hello, world!"));
    }

    #[test]
    fn test_parse_variable() {
        let tokens = parse_template("Hello, {{name}}!").unwrap();
        assert_eq!(tokens.len(), 3);
        assert!(matches!(&tokens[0], TemplateToken::Text(s) if s == "Hello, "));
        assert!(matches!(&tokens[1], TemplateToken::Variable(s) if s == "name"));
        assert!(matches!(&tokens[2], TemplateToken::Text(s) if s == "!"));
    }

    #[test]
    fn test_parse_raw_variable() {
        let tokens = parse_template("{{{raw}}}").unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(&tokens[0], TemplateToken::RawVariable(s) if s == "raw"));
    }

    #[test]
    fn test_parse_section() {
        let tokens = parse_template("{{#items}}item{{/items}}").unwrap();
        assert_eq!(tokens.len(), 3);
        assert!(matches!(&tokens[0], TemplateToken::SectionStart(s) if s == "items"));
        assert!(matches!(&tokens[1], TemplateToken::Text(s) if s == "item"));
        assert!(matches!(&tokens[2], TemplateToken::SectionEnd(s) if s == "items"));
    }

    #[test]
    fn test_parse_inverted() {
        let tokens = parse_template("{{^empty}}content{{/empty}}").unwrap();
        assert_eq!(tokens.len(), 3);
        assert!(matches!(&tokens[0], TemplateToken::InvertedStart(s) if s == "empty"));
        assert!(matches!(&tokens[1], TemplateToken::Text(s) if s == "content"));
        assert!(matches!(&tokens[2], TemplateToken::SectionEnd(s) if s == "empty"));
    }

    #[test]
    fn test_parse_unclosed_tag() {
        assert!(parse_template("Hello {{name").is_err());
    }

    #[test]
    fn test_parse_empty_tag() {
        assert!(parse_template("Hello {{}}").is_err());
    }

    #[test]
    fn test_render_variable() {
        let tokens = parse_template("Hello, {{name}}!").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"name": "World"}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_render_html_escape() {
        let tokens = parse_template("{{value}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"value": "<b>bold</b>"}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "&lt;b&gt;bold&lt;/b&gt;");
    }

    #[test]
    fn test_render_raw_variable() {
        let tokens = parse_template("{{{value}}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"value": "<b>bold</b>"}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "<b>bold</b>");
    }

    #[test]
    fn test_render_section_array() {
        let tokens = parse_template("{{#items}}{{.}} {{/items}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"items": ["a", "b", "c"]}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "a b c ");
    }

    #[test]
    fn test_render_section_bool_true() {
        let tokens = parse_template("{{#show}}visible{{/show}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"show": true}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "visible");
    }

    #[test]
    fn test_render_section_bool_false() {
        let tokens = parse_template("{{#show}}visible{{/show}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"show": false}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_render_inverted_null() {
        let tokens = parse_template("{{^val}}empty{{/val}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"val": null}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "empty");
    }

    #[test]
    fn test_render_inverted_missing() {
        let tokens = parse_template("{{^val}}empty{{/val}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "empty");
    }

    #[test]
    fn test_render_inverted_empty_array() {
        let tokens = parse_template("{{^items}}none{{/items}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"items": []}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "none");
    }

    #[test]
    fn test_render_inverted_nonempty() {
        let tokens = parse_template("{{^val}}empty{{/val}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"val": "hello"}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_render_dot_notation() {
        let tokens = parse_template("{{user.name}}").unwrap();
        let data: JsonValue =
            serde_json::from_str(r#"{"user": {"name": "Alice"}}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "Alice");
    }

    #[test]
    fn test_render_section_object() {
        let tokens = parse_template("{{#user}}{{name}}{{/user}}").unwrap();
        let data: JsonValue =
            serde_json::from_str(r#"{"user": {"name": "Bob"}}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "Bob");
    }

    #[test]
    fn test_render_number() {
        let tokens = parse_template("Count: {{count}}").unwrap();
        let data: JsonValue = serde_json::from_str(r#"{"count": 42}"#).unwrap();
        let result = render_tokens(&tokens, &data, &[&data]).unwrap();
        assert_eq!(result, "Count: 42");
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("&"), "&amp;");
        assert_eq!(html_escape("<"), "&lt;");
        assert_eq!(html_escape(">"), "&gt;");
        assert_eq!(html_escape("\""), "&quot;");
        assert_eq!(html_escape("'"), "&#x27;");
        assert_eq!(html_escape("safe text"), "safe text");
    }

    #[test]
    fn test_is_falsy() {
        assert!(is_falsy(&JsonValue::Null));
        assert!(is_falsy(&JsonValue::Bool(false)));
        assert!(is_falsy(&JsonValue::Array(vec![])));
        assert!(is_falsy(&JsonValue::String(String::new())));
        assert!(!is_falsy(&JsonValue::Bool(true)));
        assert!(!is_falsy(&JsonValue::String("x".to_string())));
        assert!(!is_falsy(&serde_json::json!(42)));
        assert!(!is_falsy(&serde_json::json!([1])));
    }
}
