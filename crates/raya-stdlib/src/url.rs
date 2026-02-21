//! URL module implementation (std:url)
//!
//! Native implementation using the `url` crate (WHATWG URL parser)
//! for URL parsing, component access, encoding/decoding, and
//! URLSearchParams manipulation.

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

// ============================================================================
// Handle Registries
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

    fn with_mut<F, R>(&self, id: u64, f: F) -> Option<R>
    where
        F: FnOnce(&mut T) -> R,
    {
        self.map.lock().get_mut(&id).map(f)
    }
}

/// Global registry for parsed URL objects
static URL_HANDLES: LazyLock<HandleRegistry<url::Url>> =
    LazyLock::new(HandleRegistry::new);

/// Global registry for URLSearchParams objects
static PARAMS_HANDLES: LazyLock<HandleRegistry<Vec<(String, String)>>> =
    LazyLock::new(HandleRegistry::new);

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

/// Handle URL method calls by numeric ID
pub fn call_url_method(
    ctx: &dyn NativeContext,
    method_id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match method_id {
        // URL Parsing
        0x9000 => parse(ctx, args),
        0x9001 => parse_with_base(ctx, args),

        // URL Components
        0x9010 => protocol(ctx, args),
        0x9011 => hostname(ctx, args),
        0x9012 => port(ctx, args),
        0x9013 => host(ctx, args),
        0x9014 => pathname(ctx, args),
        0x9015 => search(ctx, args),
        0x9016 => hash(ctx, args),
        0x9017 => origin(ctx, args),
        0x9018 => href(ctx, args),
        0x9019 => username(ctx, args),
        0x901A => password(ctx, args),
        0x901B => search_params(args),
        0x901C => to_string(ctx, args),

        // Mutators (return new handle)
        0x901D => with_protocol(ctx, args),
        0x901E => with_hostname(ctx, args),
        0x901F => with_port(ctx, args),
        0x9022 => with_pathname(ctx, args),
        0x9023 => with_search(ctx, args),
        0x9024 => with_hash(ctx, args),

        // Encoding
        0x9020 => encode(ctx, args),
        0x9021 => decode(ctx, args),
        0x9025 => encode_path(ctx, args),
        0x9026 => decode_path(ctx, args),

        // URLSearchParams
        0x9030 => params_new(),
        0x9031 => params_from_string(ctx, args),
        0x9032 => params_get(ctx, args),
        0x9033 => params_get_all(ctx, args),
        0x9034 => params_has(ctx, args),
        0x9035 => params_set(ctx, args),
        0x9036 => params_append(ctx, args),
        0x9037 => params_delete(ctx, args),
        0x9038 => params_keys(ctx, args),
        0x9039 => params_values(ctx, args),
        0x903A => params_entries(ctx, args),
        0x903B => params_sort(args),
        0x903C => params_to_string(ctx, args),
        0x903D => params_size(args),

        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// URL Parsing
// ============================================================================

/// url.parse(input: string) -> handle (f64)
fn parse(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("url.parse requires 1 argument".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.parse: invalid input: {}", e)),
    };
    match url::Url::parse(&input) {
        Ok(parsed) => {
            let handle = URL_HANDLES.insert(parsed);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("url.parse: {}", e)),
    }
}

/// url.parseWithBase(input: string, base: string) -> handle (f64)
fn parse_with_base(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("url.parseWithBase requires 2 arguments".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.parseWithBase: invalid input: {}", e)),
    };
    let base_str = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.parseWithBase: invalid base: {}", e)),
    };
    let base = match url::Url::parse(&base_str) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("url.parseWithBase: invalid base URL: {}", e)),
    };
    match base.join(&input) {
        Ok(parsed) => {
            let handle = URL_HANDLES.insert(parsed);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("url.parseWithBase: {}", e)),
    }
}

// ============================================================================
// URL Component Accessors
// ============================================================================

/// url.protocol(handle) -> string (e.g., "https:")
fn protocol(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| format!("{}:", u.scheme())) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.protocol: invalid handle".to_string()),
    }
}

/// url.hostname(handle) -> string
fn hostname(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| u.host_str().unwrap_or("").to_string()) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.hostname: invalid handle".to_string()),
    }
}

/// url.port(handle) -> string
fn port(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| {
        u.port().map(|p| p.to_string()).unwrap_or_default()
    }) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.port: invalid handle".to_string()),
    }
}

/// url.host(handle) -> string (hostname:port or just hostname)
fn host(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| {
        match (u.host_str(), u.port()) {
            (Some(h), Some(p)) => format!("{}:{}", h, p),
            (Some(h), None) => h.to_string(),
            _ => String::new(),
        }
    }) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.host: invalid handle".to_string()),
    }
}

/// url.pathname(handle) -> string
fn pathname(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| u.path().to_string()) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.pathname: invalid handle".to_string()),
    }
}

/// url.search(handle) -> string (including "?" prefix, or empty)
fn search(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| {
        u.query().map(|q| format!("?{}", q)).unwrap_or_default()
    }) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.search: invalid handle".to_string()),
    }
}

/// url.hash(handle) -> string (including "#" prefix, or empty)
fn hash(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| {
        u.fragment().map(|f| format!("#{}", f)).unwrap_or_default()
    }) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.hash: invalid handle".to_string()),
    }
}

/// url.origin(handle) -> string
fn origin(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| u.origin().ascii_serialization()) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.origin: invalid handle".to_string()),
    }
}

/// url.href(handle) -> string
fn href(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| u.as_str().to_string()) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.href: invalid handle".to_string()),
    }
}

/// url.username(handle) -> string
fn username(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| u.username().to_string()) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.username: invalid handle".to_string()),
    }
}

/// url.password(handle) -> string
fn password(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| {
        u.password().unwrap_or("").to_string()
    }) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.password: invalid handle".to_string()),
    }
}

/// url.searchParams(handle) -> paramsHandle (f64)
///
/// Extracts the query parameters from a URL into a new UrlSearchParams handle.
fn search_params(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match URL_HANDLES.with(handle, |u| {
        let pairs: Vec<(String, String)> = u
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        pairs
    }) {
        Some(pairs) => {
            let params_handle = PARAMS_HANDLES.insert(pairs);
            NativeCallResult::f64(params_handle as f64)
        }
        None => NativeCallResult::Error("url.searchParams: invalid handle".to_string()),
    }
}

/// url.toString(handle) -> string
fn to_string(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    href(ctx, args)
}

// ============================================================================
// Encoding / Decoding
// ============================================================================

/// Characters that should NOT be percent-encoded in encodeURIComponent
/// (matches JS behavior: unreserved chars per RFC 3986 + !, ', (, ), *)
const URI_COMPONENT_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// url.encode(component: string) -> string
///
/// Percent-encodes a URI component (equivalent to JS encodeURIComponent).
fn encode(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("url.encode requires 1 argument".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.encode: invalid input: {}", e)),
    };
    let encoded = percent_encoding::utf8_percent_encode(&input, URI_COMPONENT_SET).to_string();
    NativeCallResult::Value(ctx.create_string(&encoded))
}

/// url.decode(component: string) -> string
///
/// Percent-decodes a URI component (equivalent to JS decodeURIComponent).
fn decode(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("url.decode requires 1 argument".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.decode: invalid input: {}", e)),
    };
    let decoded = percent_encoding::percent_decode_str(&input)
        .decode_utf8_lossy()
        .to_string();
    NativeCallResult::Value(ctx.create_string(&decoded))
}

// ============================================================================
// URL Mutators (return new URL handle with one component changed)
// ============================================================================

fn with_protocol(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    let new_val = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.withProtocol: {}", e)),
    };
    let scheme = new_val.trim_end_matches(':');
    match URL_HANDLES.with(handle, |u| {
        let mut cloned = u.clone();
        let _ = cloned.set_scheme(scheme);
        cloned
    }) {
        Some(new_url) => {
            let new_handle = URL_HANDLES.insert(new_url);
            NativeCallResult::f64(new_handle as f64)
        }
        None => NativeCallResult::Error("url.withProtocol: invalid handle".to_string()),
    }
}

fn with_hostname(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    let new_val = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.withHostname: {}", e)),
    };
    match URL_HANDLES.with(handle, |u| {
        let mut cloned = u.clone();
        let _ = cloned.set_host(Some(&new_val));
        cloned
    }) {
        Some(new_url) => {
            let new_handle = URL_HANDLES.insert(new_url);
            NativeCallResult::f64(new_handle as f64)
        }
        None => NativeCallResult::Error("url.withHostname: invalid handle".to_string()),
    }
}

fn with_port(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    let new_val = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.withPort: {}", e)),
    };
    let port: Option<u16> = if new_val.is_empty() {
        None
    } else {
        new_val.parse().ok()
    };
    match URL_HANDLES.with(handle, |u| {
        let mut cloned = u.clone();
        let _ = cloned.set_port(port);
        cloned
    }) {
        Some(new_url) => {
            let new_handle = URL_HANDLES.insert(new_url);
            NativeCallResult::f64(new_handle as f64)
        }
        None => NativeCallResult::Error("url.withPort: invalid handle".to_string()),
    }
}

fn with_pathname(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    let new_val = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.withPathname: {}", e)),
    };
    match URL_HANDLES.with(handle, |u| {
        let mut cloned = u.clone();
        cloned.set_path(&new_val);
        cloned
    }) {
        Some(new_url) => {
            let new_handle = URL_HANDLES.insert(new_url);
            NativeCallResult::f64(new_handle as f64)
        }
        None => NativeCallResult::Error("url.withPathname: invalid handle".to_string()),
    }
}

fn with_search(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    let new_val = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.withSearch: {}", e)),
    };
    let query = if new_val.is_empty() {
        None
    } else {
        Some(new_val.strip_prefix('?').unwrap_or(&new_val).to_string())
    };
    match URL_HANDLES.with(handle, |u| {
        let mut cloned = u.clone();
        cloned.set_query(query.as_deref());
        cloned
    }) {
        Some(new_url) => {
            let new_handle = URL_HANDLES.insert(new_url);
            NativeCallResult::f64(new_handle as f64)
        }
        None => NativeCallResult::Error("url.withSearch: invalid handle".to_string()),
    }
}

fn with_hash(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    let new_val = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.withHash: {}", e)),
    };
    let frag = if new_val.is_empty() {
        None
    } else {
        Some(new_val.strip_prefix('#').unwrap_or(&new_val).to_string())
    };
    match URL_HANDLES.with(handle, |u| {
        let mut cloned = u.clone();
        cloned.set_fragment(frag.as_deref());
        cloned
    }) {
        Some(new_url) => {
            let new_handle = URL_HANDLES.insert(new_url);
            NativeCallResult::f64(new_handle as f64)
        }
        None => NativeCallResult::Error("url.withHash: invalid handle".to_string()),
    }
}

// ============================================================================
// Path Encoding
// ============================================================================

/// Characters for path segment encoding (less aggressive than URI component)
const PATH_SEGMENT_SET: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

fn encode_path(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("url.encodePath requires 1 argument".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.encodePath: {}", e)),
    };
    let encoded = percent_encoding::utf8_percent_encode(&input, PATH_SEGMENT_SET).to_string();
    NativeCallResult::Value(ctx.create_string(&encoded))
}

fn decode_path(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("url.decodePath requires 1 argument".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.decodePath: {}", e)),
    };
    let decoded = percent_encoding::percent_decode_str(&input)
        .decode_utf8_lossy()
        .to_string();
    NativeCallResult::Value(ctx.create_string(&decoded))
}

// ============================================================================
// URLSearchParams
// ============================================================================

/// url.paramsNew() -> handle (f64)
fn params_new() -> NativeCallResult {
    let handle = PARAMS_HANDLES.insert(Vec::new());
    NativeCallResult::f64(handle as f64)
}

/// url.paramsFromString(init: string) -> handle (f64)
fn params_from_string(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("url.paramsFromString requires 1 argument".to_string());
    }
    let input = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsFromString: invalid input: {}", e)),
    };
    // Strip leading "?" if present
    let query = input.strip_prefix('?').unwrap_or(&input);
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let handle = PARAMS_HANDLES.insert(pairs);
    NativeCallResult::f64(handle as f64)
}

/// url.paramsGet(handle, name: string) -> string | null
fn params_get(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("url.paramsGet requires 2 arguments".to_string());
    }
    let handle = get_handle(args, 0);
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsGet: invalid name: {}", e)),
    };
    match PARAMS_HANDLES.with(handle, |pairs| {
        pairs.iter().find(|(k, _)| k == &name).map(|(_, v)| v.clone())
    }) {
        Some(Some(val)) => NativeCallResult::Value(ctx.create_string(&val)),
        Some(None) => NativeCallResult::null(),
        None => NativeCallResult::Error("url.paramsGet: invalid handle".to_string()),
    }
}

/// url.paramsGetAll(handle, name: string) -> string[]
fn params_get_all(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("url.paramsGetAll requires 2 arguments".to_string());
    }
    let handle = get_handle(args, 0);
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsGetAll: invalid name: {}", e)),
    };
    match PARAMS_HANDLES.with(handle, |pairs| {
        pairs.iter()
            .filter(|(k, _)| k == &name)
            .map(|(_, v)| v.clone())
            .collect::<Vec<String>>()
    }) {
        Some(values) => {
            let items: Vec<NativeValue> = values
                .iter()
                .map(|v| ctx.create_string(v))
                .collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error("url.paramsGetAll: invalid handle".to_string()),
    }
}

/// url.paramsHas(handle, name: string) -> boolean
fn params_has(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("url.paramsHas requires 2 arguments".to_string());
    }
    let handle = get_handle(args, 0);
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsHas: invalid name: {}", e)),
    };
    match PARAMS_HANDLES.with(handle, |pairs| {
        pairs.iter().any(|(k, _)| k == &name)
    }) {
        Some(found) => NativeCallResult::bool(found),
        None => NativeCallResult::Error("url.paramsHas: invalid handle".to_string()),
    }
}

/// url.paramsSet(handle, name: string, value: string) -> void
fn params_set(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 3 {
        return NativeCallResult::Error("url.paramsSet requires 3 arguments".to_string());
    }
    let handle = get_handle(args, 0);
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsSet: invalid name: {}", e)),
    };
    let value = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsSet: invalid value: {}", e)),
    };
    match PARAMS_HANDLES.with_mut(handle, |pairs| {
        // Remove all existing entries for this name, then add one
        pairs.retain(|(k, _)| k != &name);
        pairs.push((name, value));
    }) {
        Some(()) => NativeCallResult::null(),
        None => NativeCallResult::Error("url.paramsSet: invalid handle".to_string()),
    }
}

/// url.paramsAppend(handle, name: string, value: string) -> void
fn params_append(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 3 {
        return NativeCallResult::Error("url.paramsAppend requires 3 arguments".to_string());
    }
    let handle = get_handle(args, 0);
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsAppend: invalid name: {}", e)),
    };
    let value = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsAppend: invalid value: {}", e)),
    };
    match PARAMS_HANDLES.with_mut(handle, |pairs| {
        pairs.push((name, value));
    }) {
        Some(()) => NativeCallResult::null(),
        None => NativeCallResult::Error("url.paramsAppend: invalid handle".to_string()),
    }
}

/// url.paramsDelete(handle, name: string) -> void
fn params_delete(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("url.paramsDelete requires 2 arguments".to_string());
    }
    let handle = get_handle(args, 0);
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("url.paramsDelete: invalid name: {}", e)),
    };
    match PARAMS_HANDLES.with_mut(handle, |pairs| {
        pairs.retain(|(k, _)| k != &name);
    }) {
        Some(()) => NativeCallResult::null(),
        None => NativeCallResult::Error("url.paramsDelete: invalid handle".to_string()),
    }
}

/// url.paramsKeys(handle) -> string[]
fn params_keys(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match PARAMS_HANDLES.with(handle, |pairs| {
        pairs.iter().map(|(k, _)| k.clone()).collect::<Vec<String>>()
    }) {
        Some(keys) => {
            let items: Vec<NativeValue> = keys
                .iter()
                .map(|k| ctx.create_string(k))
                .collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error("url.paramsKeys: invalid handle".to_string()),
    }
}

/// url.paramsValues(handle) -> string[]
fn params_values(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match PARAMS_HANDLES.with(handle, |pairs| {
        pairs.iter().map(|(_, v)| v.clone()).collect::<Vec<String>>()
    }) {
        Some(values) => {
            let items: Vec<NativeValue> = values
                .iter()
                .map(|v| ctx.create_string(v))
                .collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error("url.paramsValues: invalid handle".to_string()),
    }
}

/// url.paramsEntries(handle) -> string[] (flat: [key1, val1, key2, val2, ...])
fn params_entries(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match PARAMS_HANDLES.with(handle, |pairs| {
        pairs.iter()
            .flat_map(|(k, v)| vec![k.clone(), v.clone()])
            .collect::<Vec<String>>()
    }) {
        Some(entries) => {
            let items: Vec<NativeValue> = entries
                .iter()
                .map(|s| ctx.create_string(s))
                .collect();
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error("url.paramsEntries: invalid handle".to_string()),
    }
}

/// url.paramsSort(handle) -> void
fn params_sort(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match PARAMS_HANDLES.with_mut(handle, |pairs| {
        pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
    }) {
        Some(()) => NativeCallResult::null(),
        None => NativeCallResult::Error("url.paramsSort: invalid handle".to_string()),
    }
}

/// url.paramsToString(handle) -> string (serialized query string without "?")
fn params_to_string(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match PARAMS_HANDLES.with(handle, |pairs| {
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs.iter())
            .finish()
    }) {
        Some(s) => NativeCallResult::Value(ctx.create_string(&s)),
        None => NativeCallResult::Error("url.paramsToString: invalid handle".to_string()),
    }
}

/// url.paramsSize(handle) -> number
fn params_size(args: &[NativeValue]) -> NativeCallResult {
    let handle = get_handle(args, 0);
    match PARAMS_HANDLES.with(handle, |pairs| pairs.len()) {
        Some(n) => NativeCallResult::f64(n as f64),
        None => NativeCallResult::Error("url.paramsSize: invalid handle".to_string()),
    }
}
