//! std:fetch — HTTP/1.1 client (minimal, built on std::net)

use crate::handles::HandleRegistry;
use raya_engine::vm::{NativeCallResult, NativeContext, NativeValue, string_read, string_allocate, buffer_allocate, array_allocate};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net;
use std::sync::LazyLock;

static RESPONSES: LazyLock<HandleRegistry<HttpResponseData>> = LazyLock::new(HandleRegistry::new);

/// Parsed HTTP response data
struct HttpResponseData {
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

/// Make HTTP request, return response handle
pub fn request(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let method = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fetch.request: {}", e)),
    };
    let url = match string_read(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fetch.request: {}", e)),
    };
    let body = match string_read(args[2]) {
        Ok(s) => s,
        Err(_) => String::new(),
    };
    let extra_headers = match string_read(args[3]) {
        Ok(s) => s,
        Err(_) => String::new(),
    };

    match do_http_request(&method, &url, &body, &extra_headers) {
        Ok(resp) => {
            let handle = RESPONSES.insert(resp);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("fetch.request: {}", e)),
    }
}

// ── Response accessors ──

/// Get response status code
pub fn res_status(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => NativeCallResult::f64(resp.status as f64),
        None => NativeCallResult::Error(format!("fetch.resStatus: invalid handle {}", handle)),
    }
}

/// Get response status text
pub fn res_status_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => NativeCallResult::Value(string_allocate(ctx, resp.status_text.clone())),
        None => NativeCallResult::Error(format!("fetch.resStatusText: invalid handle {}", handle)),
    }
}

/// Get specific response header
pub fn res_header(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let name = match string_read(args[1]) {
        Ok(s) => s.to_lowercase(),
        Err(e) => return NativeCallResult::Error(format!("fetch.resHeader: {}", e)),
    };
    match RESPONSES.get(handle) {
        Some(resp) => {
            let val = resp.headers.get(&name).cloned().unwrap_or_default();
            NativeCallResult::Value(string_allocate(ctx, val))
        }
        None => NativeCallResult::Error(format!("fetch.resHeader: invalid handle {}", handle)),
    }
}

/// Get all response headers as flat [key, value, ...] array
pub fn res_headers(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => {
            let mut items = Vec::new();
            for (k, v) in &resp.headers {
                items.push(string_allocate(ctx, k.clone()));
                items.push(string_allocate(ctx, v.clone()));
            }
            NativeCallResult::Value(array_allocate(ctx, &items))
        }
        None => NativeCallResult::Error(format!("fetch.resHeaders: invalid handle {}", handle)),
    }
}

/// Get response body as text
pub fn res_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => {
            let text = String::from_utf8_lossy(&resp.body).into_owned();
            NativeCallResult::Value(string_allocate(ctx, text))
        }
        None => NativeCallResult::Error(format!("fetch.resText: invalid handle {}", handle)),
    }
}

/// Get response body as bytes
pub fn res_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => NativeCallResult::Value(buffer_allocate(ctx, &resp.body)),
        None => NativeCallResult::Error(format!("fetch.resBytes: invalid handle {}", handle)),
    }
}

// ── Internal: raw HTTP/1.1 client ──

fn do_http_request(method: &str, url: &str, body: &str, extra_headers: &str) -> Result<HttpResponseData, String> {
    // Parse URL: http://host:port/path
    let url = url.trim();
    let (host, port, path) = parse_url(url)?;

    // Connect
    let addr = format!("{}:{}", host, port);
    let mut stream = net::TcpStream::connect(&addr).map_err(|e| e.to_string())?;

    // Build request
    let mut request = format!("{} {} HTTP/1.1\r\n", method, path);
    request.push_str(&format!("Host: {}\r\n", host));
    request.push_str("Connection: close\r\n");
    if !body.is_empty() {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    // Parse extra headers (newline-separated "Key: Value" pairs)
    if !extra_headers.is_empty() {
        for line in extra_headers.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                request.push_str(trimmed);
                request.push_str("\r\n");
            }
        }
    }
    request.push_str("\r\n");
    if !body.is_empty() {
        request.push_str(body);
    }

    // Send
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    // Read response
    let mut reader = BufReader::new(stream);

    // Status line
    let mut status_line = String::new();
    reader.read_line(&mut status_line).map_err(|e| e.to_string())?;
    let (status, status_text) = parse_status_line(&status_line)?;

    // Headers
    let mut headers = HashMap::new();
    let mut content_length: Option<usize> = None;
    let mut chunked = false;
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
                content_length = val.parse().ok();
            }
            if key == "transfer-encoding" && val.to_lowercase().contains("chunked") {
                chunked = true;
            }
            headers.insert(key, val);
        }
    }

    // Body
    let resp_body = if chunked {
        read_chunked_body(&mut reader)?
    } else if let Some(len) = content_length {
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
        buf
    } else {
        // Read until EOF
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    };

    Ok(HttpResponseData {
        status,
        status_text,
        headers,
        body: resp_body,
    })
}

fn parse_url(url: &str) -> Result<(String, u16, String), String> {
    let url = if url.starts_with("http://") {
        &url[7..]
    } else if url.starts_with("https://") {
        return Err("HTTPS not supported (use HTTP)".to_string());
    } else {
        url
    };

    let (host_port, path) = if let Some(idx) = url.find('/') {
        (&url[..idx], &url[idx..])
    } else {
        (url, "/")
    };

    let (host, port) = if let Some(idx) = host_port.find(':') {
        let h = &host_port[..idx];
        let p: u16 = host_port[idx + 1..].parse().map_err(|_| "Invalid port")?;
        (h.to_string(), p)
    } else {
        (host_port.to_string(), 80)
    };

    Ok((host, port, path.to_string()))
}

fn parse_status_line(line: &str) -> Result<(u16, String), String> {
    // HTTP/1.1 200 OK
    let parts: Vec<&str> = line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err("Invalid HTTP status line".to_string());
    }
    let status: u16 = parts[1].parse().map_err(|_| "Invalid status code")?;
    let text = if parts.len() >= 3 { parts[2].to_string() } else { String::new() };
    Ok((status, text))
}

fn read_chunked_body(reader: &mut BufReader<net::TcpStream>) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line).map_err(|e| e.to_string())?;
        let size = usize::from_str_radix(size_line.trim(), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk).map_err(|e| e.to_string())?;
        body.extend_from_slice(&chunk);
        // Read trailing CRLF
        let mut crlf = String::new();
        let _ = reader.read_line(&mut crlf);
    }
    Ok(body)
}
