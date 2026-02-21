//! std:http — HTTP/1.1 server with optional TLS (built on std::net + rustls)

use crate::handles::HandleRegistry;
use crate::tls;
use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net;
use std::sync::{Arc, LazyLock, Mutex};

/// Server entry: TcpListener + optional TLS config
struct HttpServerEntry {
    listener: net::TcpListener,
    tls_config: Option<Arc<rustls::ServerConfig>>,
}

static HTTP_SERVERS: LazyLock<HandleRegistry<HttpServerEntry>> =
    LazyLock::new(HandleRegistry::new);
static HTTP_REQUESTS: LazyLock<HandleRegistry<HttpRequestData>> =
    LazyLock::new(HandleRegistry::new);

/// Parsed HTTP request data
struct HttpRequestData {
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    /// The underlying stream for sending the response (either TCP or TLS)
    stream: Mutex<Box<dyn Write + Send>>,
    /// Remote client address (IP:port)
    remote_addr: String,
}

/// Create HTTP server (bind to host:port — fast syscall, stays sync)
pub fn server_create(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("http.serverCreate: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    let addr = format!("{}:{}", host, port);
    match net::TcpListener::bind(&addr) {
        Ok(listener) => {
            let entry = HttpServerEntry {
                listener,
                tls_config: None,
            };
            let handle = HTTP_SERVERS.insert(entry);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("http.serverCreate: {}", e)),
    }
}

/// Create HTTPS server with TLS certificate and key
pub fn server_create_tls(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("http.serverCreateTls: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(443.0) as u16;
    let cert_pem = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("http.serverCreateTls: cert: {}", e)),
    };
    let key_pem = match ctx.read_string(args[3]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("http.serverCreateTls: key: {}", e)),
    };

    let tls_config = match tls::server_config(&cert_pem, &key_pem) {
        Ok(c) => c,
        Err(e) => return NativeCallResult::Error(format!("http.serverCreateTls: {}", e)),
    };

    let addr = format!("{}:{}", host, port);
    match net::TcpListener::bind(&addr) {
        Ok(listener) => {
            let entry = HttpServerEntry {
                listener,
                tls_config: Some(tls_config),
            };
            let handle = HTTP_SERVERS.insert(entry);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("http.serverCreateTls: {}", e)),
    }
}

/// Accept next HTTP request (blocking → IO pool). Parses request, returns handle.
/// Automatically performs TLS handshake if server was created with TLS.
pub fn server_accept(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match HTTP_SERVERS.get(handle) {
                Some(entry) => match entry.listener.accept() {
                    Ok((stream, addr)) => {
                        let remote = addr.to_string();
                        let result = if let Some(ref tls_config) = entry.tls_config {
                            accept_tls_request(stream, tls_config.clone(), &remote)
                        } else {
                            accept_plain_request(stream, &remote)
                        };
                        match result {
                            Ok(req) => {
                                let req_handle = HTTP_REQUESTS.insert(req);
                                IoCompletion::Primitive(NativeValue::f64(req_handle as f64))
                            }
                            Err(e) => {
                                IoCompletion::Error(format!("http.serverAccept: {}", e))
                            }
                        }
                    }
                    Err(e) => IoCompletion::Error(format!("http.serverAccept: {}", e)),
                },
                None => {
                    IoCompletion::Error(format!("http.serverAccept: invalid handle {}", handle))
                }
            }
        }),
    })
}

/// Send text response
pub fn server_respond(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let _server_handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let req_handle = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let status = args
        .get(2)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(200.0) as u16;
    let body = match ctx.read_string(args[3]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("http.serverRespond: {}", e)),
    };

    if let Some((_, req)) = HTTP_REQUESTS.remove(req_handle) {
        let status_text = http_status_text(status);
        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
            status, status_text, body.len(), body
        );
        let mut stream = req.stream.into_inner().unwrap();
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        NativeCallResult::null()
    } else {
        NativeCallResult::Error(format!(
            "http.serverRespond: invalid request handle {}",
            req_handle
        ))
    }
}

/// Send binary response
pub fn server_respond_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let _server_handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let req_handle = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let status = args
        .get(2)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(200.0) as u16;
    let body = match ctx.read_buffer(args[3]) {
        Ok(d) => d,
        Err(e) => return NativeCallResult::Error(format!("http.serverRespondBytes: {}", e)),
    };

    if let Some((_, req)) = HTTP_REQUESTS.remove(req_handle) {
        let status_text = http_status_text(status);
        let header = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
            status, status_text, body.len()
        );
        let mut stream = req.stream.into_inner().unwrap();
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
        NativeCallResult::null()
    } else {
        NativeCallResult::Error(format!(
            "http.serverRespondBytes: invalid request handle {}",
            req_handle
        ))
    }
}

/// Send response with custom headers
pub fn server_respond_headers(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let _server_handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let req_handle = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let status = args
        .get(2)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(200.0) as u16;
    // headers is a string[] of alternating key-value pairs
    let header_count = ctx.array_len(args[3]).unwrap_or(0);
    let mut custom_headers = Vec::new();
    let mut i = 0;
    while i + 1 < header_count {
        if let (Ok(k), Ok(v)) = (
            ctx.array_get(args[3], i).and_then(|v| ctx.read_string(v)),
            ctx.array_get(args[3], i + 1)
                .and_then(|v| ctx.read_string(v)),
        ) {
            custom_headers.push((k, v));
        }
        i += 2;
    }
    let body = match ctx.read_string(args[4]) {
        Ok(s) => s,
        Err(e) => {
            return NativeCallResult::Error(format!("http.serverRespondHeaders: {}", e))
        }
    };

    if let Some((_, req)) = HTTP_REQUESTS.remove(req_handle) {
        let status_text = http_status_text(status);
        let mut response = format!("HTTP/1.1 {} {}\r\n", status, status_text);
        response.push_str(&format!("Content-Length: {}\r\n", body.len()));
        for (k, v) in &custom_headers {
            response.push_str(&format!("{}: {}\r\n", k, v));
        }
        response.push_str("Connection: close\r\n\r\n");
        response.push_str(&body);
        let mut stream = req.stream.into_inner().unwrap();
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        NativeCallResult::null()
    } else {
        NativeCallResult::Error(format!(
            "http.serverRespondHeaders: invalid request handle {}",
            req_handle
        ))
    }
}

/// Close HTTP server
pub fn server_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    HTTP_SERVERS.remove(handle);
    NativeCallResult::null()
}

/// Get HTTP server local address
pub fn server_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_SERVERS.get(handle) {
        Some(entry) => match entry.listener.local_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
            Err(e) => NativeCallResult::Error(format!("http.serverAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("http.serverAddr: invalid handle {}", handle)),
    }
}

// ── Request accessors ──

/// Get request method
pub fn req_method(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_REQUESTS.get(handle) {
        Some(req) => NativeCallResult::Value(ctx.create_string(&req.method)),
        None => NativeCallResult::Error(format!("http.reqMethod: invalid handle {}", handle)),
    }
}

/// Get request path
pub fn req_path(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_REQUESTS.get(handle) {
        Some(req) => NativeCallResult::Value(ctx.create_string(&req.path)),
        None => NativeCallResult::Error(format!("http.reqPath: invalid handle {}", handle)),
    }
}

/// Get request query string
pub fn req_query(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_REQUESTS.get(handle) {
        Some(req) => NativeCallResult::Value(ctx.create_string(&req.query)),
        None => NativeCallResult::Error(format!("http.reqQuery: invalid handle {}", handle)),
    }
}

/// Get specific request header
pub fn req_header(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s.to_lowercase(),
        Err(e) => return NativeCallResult::Error(format!("http.reqHeader: {}", e)),
    };
    match HTTP_REQUESTS.get(handle) {
        Some(req) => {
            let val = req.headers.get(&name).cloned().unwrap_or_default();
            NativeCallResult::Value(ctx.create_string(&val))
        }
        None => NativeCallResult::Error(format!("http.reqHeader: invalid handle {}", handle)),
    }
}

/// Get all request headers as flat [key, value, ...] array
pub fn req_headers(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_REQUESTS.get(handle) {
        Some(req) => {
            let mut items = Vec::new();
            for (k, v) in &req.headers {
                items.push(ctx.create_string(k));
                items.push(ctx.create_string(v));
            }
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error(format!("http.reqHeaders: invalid handle {}", handle)),
    }
}

/// Get request body as text
pub fn req_body(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_REQUESTS.get(handle) {
        Some(req) => {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            NativeCallResult::Value(ctx.create_string(&body))
        }
        None => NativeCallResult::Error(format!("http.reqBody: invalid handle {}", handle)),
    }
}

/// Get request body as bytes
pub fn req_body_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_REQUESTS.get(handle) {
        Some(req) => NativeCallResult::Value(ctx.create_buffer(&req.body)),
        None => NativeCallResult::Error(format!("http.reqBodyBytes: invalid handle {}", handle)),
    }
}

/// Get full request URL (path + query)
pub fn req_url(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_REQUESTS.get(handle) {
        Some(req) => {
            let url = if req.query.is_empty() {
                req.path.clone()
            } else {
                format!("{}?{}", req.path, req.query)
            };
            NativeCallResult::Value(ctx.create_string(&url))
        }
        None => NativeCallResult::Error(format!("http.reqUrl: invalid handle {}", handle)),
    }
}

/// Get remote client address (IP:port)
pub fn req_remote_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP_REQUESTS.get(handle) {
        Some(req) => NativeCallResult::Value(ctx.create_string(&req.remote_addr)),
        None => NativeCallResult::Error(format!("http.reqRemoteAddr: invalid handle {}", handle)),
    }
}

// ── Internal helpers ──

fn accept_plain_request(stream: net::TcpStream, remote_addr: &str) -> Result<HttpRequestData, String> {
    let clone = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(clone);
    let (method, path, query, headers, body) = parse_http_request_from_reader(&mut reader)?;
    Ok(HttpRequestData {
        method,
        path,
        query,
        headers,
        body,
        stream: Mutex::new(Box::new(stream)),
        remote_addr: remote_addr.to_string(),
    })
}

fn accept_tls_request(
    stream: net::TcpStream,
    config: Arc<rustls::ServerConfig>,
    remote_addr: &str,
) -> Result<HttpRequestData, String> {
    let tls_stream = tls::accept_tls(stream, config)?;
    // We need to split the stream for reading (BufReader) and keep it for writing.
    // Since rustls StreamOwned doesn't support try_clone, we read first, then store for writes.
    // Use a single-pass approach: read the request from the TLS stream, then store the stream.
    //
    // We have to be careful here — we need to consume just the request and leave the
    // stream usable for writing the response. We'll read into a buffer first.
    parse_tls_request(tls_stream, remote_addr)
}

fn parse_tls_request(
    mut tls_stream: tls::ServerTlsStream,
    #[allow(unused)] remote_addr: &str,
) -> Result<HttpRequestData, String> {
    // Read the request bytes in a buffer — we read until we find \r\n\r\n (end of headers)
    let mut buf = Vec::with_capacity(8192);
    let mut byte = [0u8; 1];
    let mut header_end = false;

    // Read headers byte-by-byte
    while !header_end {
        match tls_stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                buf.push(byte[0]);
                let len = buf.len();
                if len >= 4
                    && buf[len - 4] == b'\r'
                    && buf[len - 3] == b'\n'
                    && buf[len - 2] == b'\r'
                    && buf[len - 1] == b'\n'
                {
                    header_end = true;
                }
            }
            Err(e) => return Err(format!("TLS read error: {}", e)),
        }
    }

    let header_str = String::from_utf8_lossy(&buf).to_string();
    let mut lines = header_str.lines();

    // Parse request line
    let request_line = lines.next().ok_or("Empty request")?;
    let parts: Vec<&str> = request_line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err("Invalid HTTP request line".to_string());
    }
    let method = parts[0].to_string();
    let full_path = parts[1].to_string();
    let (path, query) = if let Some(idx) = full_path.find('?') {
        (
            full_path[..idx].to_string(),
            full_path[idx + 1..].to_string(),
        )
    } else {
        (full_path, String::new())
    };

    // Parse headers
    let mut headers = HashMap::new();
    let mut content_length: usize = 0;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(idx) = trimmed.find(':') {
            let key = trimmed[..idx].trim().to_lowercase();
            let val = trimmed[idx + 1..].trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            }
            headers.insert(key, val);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        tls_stream
            .read_exact(&mut body)
            .map_err(|e| e.to_string())?;
    }

    Ok(HttpRequestData {
        method,
        path,
        query,
        headers,
        body,
        stream: Mutex::new(Box::new(tls_stream)),
        remote_addr: remote_addr.to_string(),
    })
}

#[allow(clippy::type_complexity)] // Returns parsed HTTP request components
fn parse_http_request_from_reader<R: Read>(
    reader: &mut BufReader<R>,
) -> Result<
    (
        String,
        String,
        String,
        HashMap<String, String>,
        Vec<u8>,
    ),
    String,
> {
    // Read request line: METHOD /path?query HTTP/1.1
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| e.to_string())?;
    let parts: Vec<&str> = request_line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err("Invalid HTTP request line".to_string());
    }
    let method = parts[0].to_string();
    let full_path = parts[1].to_string();

    // Split path and query
    let (path, query) = if let Some(idx) = full_path.find('?') {
        (
            full_path[..idx].to_string(),
            full_path[idx + 1..].to_string(),
        )
    } else {
        (full_path, String::new())
    };

    // Read headers
    let mut headers = HashMap::new();
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(idx) = trimmed.find(':') {
            let key = trimmed[..idx].trim().to_lowercase();
            let val = trimmed[idx + 1..].trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            }
            headers.insert(key, val);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    }

    Ok((method, path, query, headers, body))
}

fn http_status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}
