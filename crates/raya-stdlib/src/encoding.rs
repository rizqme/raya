//! Encoding module implementation (std:encoding)
//!
//! Native implementation for CSV parsing/serialization, XML parsing/serialization,
//! and Base32 encoding/decoding. Uses handle-based patterns for structured data
//! (CSV tables, XML nodes) and direct value returns for encoding operations.

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

// ============================================================================
// Handle Registry (local to this module)
// ============================================================================

/// Thread-safe registry mapping numeric handles to values.
struct HandleRegistry<T> {
    map: Mutex<HashMap<u64, T>>,
    next_id: AtomicU64,
}

impl<T> HandleRegistry<T> {
    fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn insert(&self, value: T) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.map.lock().insert(id, value);
        id
    }

    fn with<F, R>(&self, id: u64, f: F) -> Option<R>
    where
        F: FnOnce(&T) -> R,
    {
        self.map.lock().get(&id).map(f)
    }

    fn remove(&self, id: u64) -> Option<T> {
        self.map.lock().remove(&id)
    }
}

// ============================================================================
// Data Types
// ============================================================================

/// Parsed CSV table data stored behind a handle.
struct CsvTableData {
    /// Column headers (empty vec if no headers)
    headers: Vec<String>,
    /// Rows of string values
    rows: Vec<Vec<String>>,
}

/// Parsed XML node data stored behind a handle.
struct XmlNodeData {
    /// Element tag name
    tag: String,
    /// Text content (concatenated from all text children)
    text: String,
    /// Attributes as (name, value) pairs
    attributes: Vec<(String, String)>,
    /// Child node handles
    children: Vec<u64>,
}

// ============================================================================
// Global Handle Registries
// ============================================================================

/// Global registry for CSV table handles
static CSV_TABLES: LazyLock<HandleRegistry<CsvTableData>> =
    LazyLock::new(|| HandleRegistry::new());

/// Global registry for XML node handles
static XML_NODES: LazyLock<HandleRegistry<XmlNodeData>> =
    LazyLock::new(|| HandleRegistry::new());

// ============================================================================
// Helper
// ============================================================================

/// Extract a handle (u64) from a NativeValue argument
fn get_handle(args: &[NativeValue], index: usize) -> u64 {
    args.get(index)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64
}

// ============================================================================
// Public API
// ============================================================================

/// Handle encoding method calls by numeric ID
pub fn call_encoding_method(
    ctx: &dyn NativeContext,
    method_id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match method_id {
        // CSV operations (0xA000-0xA00F)
        0xA000 => csv_parse(ctx, args),
        0xA001 => csv_parse_headers(ctx, args),
        0xA002 => csv_stringify(ctx, args),
        0xA003 => csv_stringify_headers(ctx, args),
        0xA004 => csv_table_headers(ctx, args),
        0xA005 => csv_table_rows(ctx, args),
        0xA006 => csv_table_row(ctx, args),
        0xA007 => csv_table_column(ctx, args),
        0xA008 => csv_table_row_count(args),
        0xA009 => csv_table_release(args),

        // XML operations (0xA010-0xA01F)
        0xA010 => xml_parse(ctx, args),
        0xA011 => xml_stringify(ctx, args),
        0xA012 => xml_tag(ctx, args),
        0xA013 => xml_text(ctx, args),
        0xA014 => xml_attr(ctx, args),
        0xA015 => xml_attrs(ctx, args),
        0xA016 => xml_children(ctx, args),
        0xA017 => xml_child(ctx, args),
        0xA018 => xml_children_by_tag(ctx, args),
        0xA019 => xml_release(args),

        // Base32 operations (0xA020-0xA02F)
        0xA020 => base32_encode(ctx, args),
        0xA021 => base32_decode(ctx, args),
        0xA022 => base32_hex_encode(ctx, args),
        0xA023 => base32_hex_decode(ctx, args),

        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// CSV Operations
// ============================================================================

/// encoding.csvParse(input: string) -> handle
///
/// Parse a CSV string without headers. Returns a handle to a CsvTable
/// where all rows are treated as data rows (no header row).
fn csv_parse(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("encoding.csvParse requires 1 argument".to_string());
    }

    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("encoding.csvParse: invalid input: {}", e)),
    };

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(input.as_bytes());

    let mut rows = Vec::new();
    for result in reader.records() {
        match result {
            Ok(record) => {
                let row: Vec<String> = record.iter().map(|s| s.to_string()).collect();
                rows.push(row);
            }
            Err(e) => {
                return NativeCallResult::Error(format!("encoding.csvParse: {}", e));
            }
        }
    }

    let table = CsvTableData {
        headers: Vec::new(),
        rows,
    };
    let handle = CSV_TABLES.insert(table);
    NativeCallResult::f64(handle as f64)
}

/// encoding.csvParseHeaders(input: string) -> handle
///
/// Parse a CSV string with the first row as headers. Returns a handle
/// to a CsvTable with headers and data rows.
fn csv_parse_headers(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "encoding.csvParseHeaders requires 1 argument".to_string(),
        );
    }

    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "encoding.csvParseHeaders: invalid input: {}",
                e
            ))
        }
    };

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(input.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map(|h| h.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();

    let mut rows = Vec::new();
    for result in reader.records() {
        match result {
            Ok(record) => {
                let row: Vec<String> = record.iter().map(|s| s.to_string()).collect();
                rows.push(row);
            }
            Err(e) => {
                return NativeCallResult::Error(format!("encoding.csvParseHeaders: {}", e));
            }
        }
    }

    let table = CsvTableData { headers, rows };
    let handle = CSV_TABLES.insert(table);
    NativeCallResult::f64(handle as f64)
}

/// encoding.csvStringify(handle) -> string
///
/// Serialize all rows of a CSV table to a CSV string (no header row).
fn csv_stringify(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match CSV_TABLES.with(handle, |table| {
        let mut wtr = csv::Writer::from_writer(Vec::new());
        for row in &table.rows {
            if let Err(e) = wtr.write_record(row) {
                return Err(format!("encoding.csvStringify: {}", e));
            }
        }
        wtr.flush().ok();
        match wtr.into_inner() {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).to_string()),
            Err(e) => Err(format!("encoding.csvStringify: {}", e)),
        }
    }) {
        Some(Ok(s)) => NativeCallResult::Value(ctx.create_string(&s)),
        Some(Err(e)) => NativeCallResult::Error(e),
        None => NativeCallResult::Error("encoding.csvStringify: invalid handle".to_string()),
    }
}

/// encoding.csvStringifyHeaders(handle) -> string
///
/// Serialize a CSV table to a CSV string with headers as the first row.
fn csv_stringify_headers(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match CSV_TABLES.with(handle, |table| {
        let mut wtr = csv::Writer::from_writer(Vec::new());
        if !table.headers.is_empty() {
            if let Err(e) = wtr.write_record(&table.headers) {
                return Err(format!("encoding.csvStringifyHeaders: {}", e));
            }
        }
        for row in &table.rows {
            if let Err(e) = wtr.write_record(row) {
                return Err(format!("encoding.csvStringifyHeaders: {}", e));
            }
        }
        wtr.flush().ok();
        match wtr.into_inner() {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).to_string()),
            Err(e) => Err(format!("encoding.csvStringifyHeaders: {}", e)),
        }
    }) {
        Some(Ok(s)) => NativeCallResult::Value(ctx.create_string(&s)),
        Some(Err(e)) => NativeCallResult::Error(e),
        None => NativeCallResult::Error(
            "encoding.csvStringifyHeaders: invalid handle".to_string(),
        ),
    }
}

/// encoding.csvTableHeaders(handle) -> string[]
///
/// Get the headers from a CSV table handle.
fn csv_table_headers(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match CSV_TABLES.with(handle, |table| table.headers.clone()) {
        Some(headers) => {
            let items: Vec<NativeValue> =
                headers.iter().map(|h| ctx.create_string(h)).collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error("encoding.csvTableHeaders: invalid handle".to_string()),
    }
}

/// encoding.csvTableRows(handle) -> string[]
///
/// Get all rows from a CSV table as a flat array. Each row is separated
/// by a null value, and columns within a row are string values.
/// Format: [col1, col2, ..., null, col1, col2, ..., null, ...]
fn csv_table_rows(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match CSV_TABLES.with(handle, |table| {
        let mut items = Vec::new();
        for row in &table.rows {
            for cell in row {
                items.push(ctx.create_string(cell));
            }
            items.push(NativeValue::null());
        }
        items
    }) {
        Some(items) => NativeCallResult::Value(ctx.create_array(&items)),
        None => NativeCallResult::Error("encoding.csvTableRows: invalid handle".to_string()),
    }
}

/// encoding.csvTableRow(handle, index: number) -> string[]
///
/// Get a single row from a CSV table by index.
fn csv_table_row(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "encoding.csvTableRow requires 2 arguments".to_string(),
        );
    }

    let handle = get_handle(args, 0);
    let index = args
        .get(1)
        .and_then(|v| v.as_i32().or_else(|| v.as_f64().map(|f| f as i32)))
        .unwrap_or(-1);

    if index < 0 {
        return NativeCallResult::Error("encoding.csvTableRow: invalid index".to_string());
    }
    let index = index as usize;

    match CSV_TABLES.with(handle, |table| {
        table.rows.get(index).cloned()
    }) {
        Some(Some(row)) => {
            let items: Vec<NativeValue> =
                row.iter().map(|cell| ctx.create_string(cell)).collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        Some(None) => NativeCallResult::Error(format!(
            "encoding.csvTableRow: index {} out of bounds",
            index
        )),
        None => NativeCallResult::Error("encoding.csvTableRow: invalid handle".to_string()),
    }
}

/// encoding.csvTableColumn(handle, name: string) -> string[]
///
/// Get all values from a column by header name.
fn csv_table_column(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "encoding.csvTableColumn requires 2 arguments".to_string(),
        );
    }

    let handle = get_handle(args, 0);
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "encoding.csvTableColumn: invalid name: {}",
                e
            ))
        }
    };

    match CSV_TABLES.with(handle, |table| {
        let col_index = table.headers.iter().position(|h| h == &name);
        match col_index {
            Some(idx) => {
                let values: Vec<String> = table
                    .rows
                    .iter()
                    .map(|row| row.get(idx).cloned().unwrap_or_default())
                    .collect();
                Ok(values)
            }
            None => Err(format!(
                "encoding.csvTableColumn: column '{}' not found",
                name
            )),
        }
    }) {
        Some(Ok(values)) => {
            let items: Vec<NativeValue> =
                values.iter().map(|v| ctx.create_string(v)).collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        Some(Err(e)) => NativeCallResult::Error(e),
        None => NativeCallResult::Error(
            "encoding.csvTableColumn: invalid handle".to_string(),
        ),
    }
}

/// encoding.csvTableRowCount(handle) -> number
///
/// Get the number of data rows in a CSV table.
fn csv_table_row_count(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match CSV_TABLES.with(handle, |table| table.rows.len()) {
        Some(count) => NativeCallResult::f64(count as f64),
        None => NativeCallResult::Error(
            "encoding.csvTableRowCount: invalid handle".to_string(),
        ),
    }
}

/// encoding.csvTableRelease(handle) -> void
///
/// Release the memory associated with a CSV table handle.
fn csv_table_release(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    CSV_TABLES.remove(handle);
    NativeCallResult::null()
}

// ============================================================================
// XML Operations
// ============================================================================

/// Parse an XML string into a tree of XmlNodeData handles.
///
/// Returns the root element handle. Child nodes are recursively stored
/// in the global XML_NODES registry.
fn parse_xml_tree(xml_str: &str) -> Result<u64, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml_str);

    // Stack of (handle, children_handles) for building the tree
    let mut stack: Vec<(u64, Vec<u64>)> = Vec::new();
    let mut root_handle: Option<u64> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attributes: Vec<(String, String)> = e
                    .attributes()
                    .filter_map(|a| {
                        a.ok().map(|attr| {
                            let key =
                                String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            (key, val)
                        })
                    })
                    .collect();

                let node = XmlNodeData {
                    tag,
                    text: String::new(),
                    attributes,
                    children: Vec::new(),
                };
                let handle = XML_NODES.insert(node);
                stack.push((handle, Vec::new()));
            }
            Ok(Event::End(_)) => {
                if let Some((handle, child_handles)) = stack.pop() {
                    // Update this node's children
                    XML_NODES.with(handle, |node| {
                        // We need mutable access; use a workaround
                        let _ = node;
                    });
                    // Actually set children via direct map access
                    {
                        let mut map = XML_NODES.map.lock();
                        if let Some(node) = map.get_mut(&handle) {
                            node.children = child_handles;
                        }
                    }

                    if let Some(parent) = stack.last_mut() {
                        parent.1.push(handle);
                    } else {
                        root_handle = Some(handle);
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attributes: Vec<(String, String)> = e
                    .attributes()
                    .filter_map(|a| {
                        a.ok().map(|attr| {
                            let key =
                                String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            (key, val)
                        })
                    })
                    .collect();

                let node = XmlNodeData {
                    tag,
                    text: String::new(),
                    attributes,
                    children: Vec::new(),
                };
                let handle = XML_NODES.insert(node);

                if let Some(parent) = stack.last_mut() {
                    parent.1.push(handle);
                } else {
                    root_handle = Some(handle);
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if !text.trim().is_empty() {
                    if let Some((parent_handle, _)) = stack.last() {
                        let mut map = XML_NODES.map.lock();
                        if let Some(node) = map.get_mut(parent_handle) {
                            if !node.text.is_empty() {
                                node.text.push(' ');
                            }
                            node.text.push_str(text.trim());
                        }
                    }
                }
            }
            Ok(Event::CData(ref e)) => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                if let Some((parent_handle, _)) = stack.last() {
                    let mut map = XML_NODES.map.lock();
                    if let Some(node) = map.get_mut(parent_handle) {
                        if !node.text.is_empty() {
                            node.text.push(' ');
                        }
                        node.text.push_str(&text);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {} // Skip comments, PI, decl, etc.
            Err(e) => return Err(format!("XML parse error: {}", e)),
        }
    }

    root_handle.ok_or_else(|| "XML parse error: no root element found".to_string())
}

/// Stringify an XML node tree back to an XML string.
fn stringify_xml_node(handle: u64) -> Result<String, String> {
    let (tag, text, attributes, children) = {
        let map = XML_NODES.map.lock();
        match map.get(&handle) {
            Some(node) => (
                node.tag.clone(),
                node.text.clone(),
                node.attributes.clone(),
                node.children.clone(),
            ),
            None => return Err("xml.stringify: invalid handle".to_string()),
        }
    };

    let mut result = String::new();
    result.push('<');
    result.push_str(&tag);
    for (key, val) in &attributes {
        result.push(' ');
        result.push_str(key);
        result.push_str("=\"");
        result.push_str(&xml_escape(val));
        result.push('"');
    }

    if children.is_empty() && text.is_empty() {
        result.push_str("/>");
    } else {
        result.push('>');
        result.push_str(&xml_escape(&text));
        for child_handle in &children {
            result.push_str(&stringify_xml_node(*child_handle)?);
        }
        result.push_str("</");
        result.push_str(&tag);
        result.push('>');
    }

    Ok(result)
}

/// Escape special XML characters in text content
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// encoding.xmlParse(input: string) -> handle
///
/// Parse an XML string into a tree of nodes. Returns a handle to the
/// root element node.
fn xml_parse(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("encoding.xmlParse requires 1 argument".to_string());
    }

    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!("encoding.xmlParse: invalid input: {}", e))
        }
    };

    match parse_xml_tree(&input) {
        Ok(handle) => NativeCallResult::f64(handle as f64),
        Err(e) => NativeCallResult::Error(e),
    }
}

/// encoding.xmlStringify(handle) -> string
///
/// Serialize an XML node tree back to an XML string.
fn xml_stringify(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match stringify_xml_node(handle) {
        Ok(s) => NativeCallResult::Value(ctx.create_string(&s)),
        Err(e) => NativeCallResult::Error(e),
    }
}

/// encoding.xmlTag(handle) -> string
///
/// Get the tag name of an XML node.
fn xml_tag(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match XML_NODES.with(handle, |node| node.tag.clone()) {
        Some(tag) => NativeCallResult::Value(ctx.create_string(&tag)),
        None => NativeCallResult::Error("encoding.xmlTag: invalid handle".to_string()),
    }
}

/// encoding.xmlText(handle) -> string
///
/// Get the text content of an XML node.
fn xml_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match XML_NODES.with(handle, |node| node.text.clone()) {
        Some(text) => NativeCallResult::Value(ctx.create_string(&text)),
        None => NativeCallResult::Error("encoding.xmlText: invalid handle".to_string()),
    }
}

/// encoding.xmlAttr(handle, name: string) -> string | null
///
/// Get the value of an attribute by name. Returns null if not found.
fn xml_attr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("encoding.xmlAttr requires 2 arguments".to_string());
    }

    let handle = get_handle(args, 0);
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!("encoding.xmlAttr: invalid name: {}", e))
        }
    };

    match XML_NODES.with(handle, |node| {
        node.attributes
            .iter()
            .find(|(k, _)| k == &name)
            .map(|(_, v)| v.clone())
    }) {
        Some(Some(val)) => NativeCallResult::Value(ctx.create_string(&val)),
        Some(None) => NativeCallResult::null(),
        None => NativeCallResult::Error("encoding.xmlAttr: invalid handle".to_string()),
    }
}

/// encoding.xmlAttrs(handle) -> string[]
///
/// Get all attributes as a flat array [key1, val1, key2, val2, ...].
fn xml_attrs(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match XML_NODES.with(handle, |node| node.attributes.clone()) {
        Some(attrs) => {
            let items: Vec<NativeValue> = attrs
                .iter()
                .flat_map(|(k, v)| vec![ctx.create_string(k), ctx.create_string(v)])
                .collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error("encoding.xmlAttrs: invalid handle".to_string()),
    }
}

/// encoding.xmlChildren(handle) -> number[]
///
/// Get all child node handles as an array of numbers.
fn xml_children(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);

    match XML_NODES.with(handle, |node| node.children.clone()) {
        Some(children) => {
            let items: Vec<NativeValue> = children
                .iter()
                .map(|&h| NativeValue::f64(h as f64))
                .collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error("encoding.xmlChildren: invalid handle".to_string()),
    }
}

/// encoding.xmlChild(handle, tag: string) -> number
///
/// Get the first child node with a matching tag name. Returns the handle
/// as a number, or -1 if not found.
fn xml_child(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("encoding.xmlChild requires 2 arguments".to_string());
    }

    let handle = get_handle(args, 0);
    let tag_name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!("encoding.xmlChild: invalid tag: {}", e))
        }
    };

    match XML_NODES.with(handle, |node| node.children.clone()) {
        Some(children) => {
            let map = XML_NODES.map.lock();
            for &child_handle in &children {
                if let Some(child) = map.get(&child_handle) {
                    if child.tag == tag_name {
                        return NativeCallResult::f64(child_handle as f64);
                    }
                }
            }
            NativeCallResult::f64(-1.0)
        }
        None => NativeCallResult::Error("encoding.xmlChild: invalid handle".to_string()),
    }
}

/// encoding.xmlChildrenByTag(handle, tag: string) -> number[]
///
/// Get all child nodes with a matching tag name as an array of handles.
fn xml_children_by_tag(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "encoding.xmlChildrenByTag requires 2 arguments".to_string(),
        );
    }

    let handle = get_handle(args, 0);
    let tag_name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "encoding.xmlChildrenByTag: invalid tag: {}",
                e
            ))
        }
    };

    match XML_NODES.with(handle, |node| node.children.clone()) {
        Some(children) => {
            let map = XML_NODES.map.lock();
            let matching: Vec<NativeValue> = children
                .iter()
                .filter(|&&h| {
                    map.get(&h)
                        .map(|child| child.tag == tag_name)
                        .unwrap_or(false)
                })
                .map(|&h| NativeValue::f64(h as f64))
                .collect();
            NativeCallResult::Value(ctx.create_array(&matching))
        }
        None => NativeCallResult::Error(
            "encoding.xmlChildrenByTag: invalid handle".to_string(),
        ),
    }
}

/// encoding.xmlRelease(handle) -> void
///
/// Release an XML node and all its children recursively.
fn xml_release(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    release_xml_recursive(handle);
    NativeCallResult::null()
}

/// Recursively release an XML node and all its descendants.
fn release_xml_recursive(handle: u64) {
    if let Some(node) = XML_NODES.remove(handle) {
        for child_handle in node.children {
            release_xml_recursive(child_handle);
        }
    }
}

// ============================================================================
// Base32 Operations
// ============================================================================

/// encoding.base32Encode(data: Buffer) -> string
///
/// Encode a buffer to a Base32 string (RFC 4648).
fn base32_encode(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "encoding.base32Encode requires 1 argument".to_string(),
        );
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "encoding.base32Encode: invalid buffer: {}",
                e
            ))
        }
    };

    let encoded = data_encoding::BASE32.encode(&data);
    NativeCallResult::Value(ctx.create_string(&encoded))
}

/// encoding.base32Decode(input: string) -> Buffer
///
/// Decode a Base32 string (RFC 4648) to a buffer.
fn base32_decode(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "encoding.base32Decode requires 1 argument".to_string(),
        );
    }

    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "encoding.base32Decode: invalid input: {}",
                e
            ))
        }
    };

    match data_encoding::BASE32.decode(input.as_bytes()) {
        Ok(bytes) => NativeCallResult::Value(ctx.create_buffer(&bytes)),
        Err(e) => NativeCallResult::Error(format!("encoding.base32Decode: {}", e)),
    }
}

/// encoding.base32HexEncode(data: Buffer) -> string
///
/// Encode a buffer to a Base32hex string (RFC 4648 extended hex alphabet).
fn base32_hex_encode(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "encoding.base32HexEncode requires 1 argument".to_string(),
        );
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "encoding.base32HexEncode: invalid buffer: {}",
                e
            ))
        }
    };

    let encoded = data_encoding::BASE32HEX.encode(&data);
    NativeCallResult::Value(ctx.create_string(&encoded))
}

/// encoding.base32HexDecode(input: string) -> Buffer
///
/// Decode a Base32hex string (RFC 4648 extended hex alphabet) to a buffer.
fn base32_hex_decode(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "encoding.base32HexDecode requires 1 argument".to_string(),
        );
    }

    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!(
                "encoding.base32HexDecode: invalid input: {}",
                e
            ))
        }
    };

    match data_encoding::BASE32HEX.decode(input.as_bytes()) {
        Ok(bytes) => NativeCallResult::Value(ctx.create_buffer(&bytes)),
        Err(e) => NativeCallResult::Error(format!("encoding.base32HexDecode: {}", e)),
    }
}
