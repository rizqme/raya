//! std:fetch — HTTP/1.1 client with TLS support (rustls)

use crate::handles::HandleRegistry;
use crate::tls;
use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net;
use std::sync::LazyLock;

static RESPONSES: LazyLock<HandleRegistry<HttpResponseData>> =
    LazyLock::new(HandleRegistry::new);

/// Parsed HTTP response data
struct HttpResponseData {
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

/// Make HTTP request, return response handle (blocking → IO pool)
pub fn request(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let method = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fetch.request: {}", e)),
    };
    let url = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fetch.request: {}", e)),
    };
    let body = ctx.read_string(args[2]).unwrap_or_default();
    let extra_headers = ctx.read_string(args[3]).unwrap_or_default();

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match do_http_request(&method, &url, &body, &extra_headers) {
                Ok(resp) => {
                    let handle = RESPONSES.insert(resp);
                    IoCompletion::Primitive(NativeValue::f64(handle as f64))
                }
                Err(e) => IoCompletion::Error(format!("fetch.request: {}", e)),
            }
        }),
    })
}

// ── Response accessors (sync — data already in memory) ──

/// Get response status code
pub fn res_status(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => NativeCallResult::f64(resp.status as f64),
        None => NativeCallResult::Error(format!("fetch.resStatus: invalid handle {}", handle)),
    }
}

/// Get response status text
pub fn res_status_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => NativeCallResult::Value(ctx.create_string(&resp.status_text)),
        None => {
            NativeCallResult::Error(format!("fetch.resStatusText: invalid handle {}", handle))
        }
    }
}

/// Get specific response header
pub fn res_header(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let name = match ctx.read_string(args[1]) {
        Ok(s) => s.to_lowercase(),
        Err(e) => return NativeCallResult::Error(format!("fetch.resHeader: {}", e)),
    };
    match RESPONSES.get(handle) {
        Some(resp) => {
            let val = resp.headers.get(&name).cloned().unwrap_or_default();
            NativeCallResult::Value(ctx.create_string(&val))
        }
        None => NativeCallResult::Error(format!("fetch.resHeader: invalid handle {}", handle)),
    }
}

/// Get all response headers as flat [key, value, ...] array
pub fn res_headers(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => {
            let mut items = Vec::new();
            for (k, v) in &resp.headers {
                items.push(ctx.create_string(k));
                items.push(ctx.create_string(v));
            }
            NativeCallResult::Value(ctx.create_array(&items))
        }
        None => NativeCallResult::Error(format!("fetch.resHeaders: invalid handle {}", handle)),
    }
}

/// Get response body as text
pub fn res_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => {
            let text = String::from_utf8_lossy(&resp.body).into_owned();
            NativeCallResult::Value(ctx.create_string(&text))
        }
        None => NativeCallResult::Error(format!("fetch.resText: invalid handle {}", handle)),
    }
}

/// Get response body as bytes
pub fn res_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => NativeCallResult::Value(ctx.create_buffer(&resp.body)),
        None => NativeCallResult::Error(format!("fetch.resBytes: invalid handle {}", handle)),
    }
}

/// Release response handle
pub fn res_release(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    RESPONSES.remove(handle);
    NativeCallResult::null()
}

/// Check if response status is 200-299
pub fn res_ok(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => NativeCallResult::bool(resp.status >= 200 && resp.status < 300),
        None => NativeCallResult::Error(format!("fetch.resOk: invalid handle {}", handle)),
    }
}

/// Check if response was redirected (status 3xx)
pub fn res_redirected(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match RESPONSES.get(handle) {
        Some(resp) => NativeCallResult::bool(resp.status >= 300 && resp.status < 400),
        None => NativeCallResult::Error(format!("fetch.resRedirected: invalid handle {}", handle)),
    }
}

// ── Internal: raw HTTP/1.1 client with TLS support ──

/// Parsed URL components
struct ParsedUrl {
    host: String,
    port: u16,
    path: String,
    is_tls: bool,
}

fn do_http_request(
    method: &str,
    url: &str,
    body: &str,
    extra_headers: &str,
) -> Result<HttpResponseData, String> {
    let url = url.trim();
    let parsed = parse_url(url)?;

    // Connect TCP
    let addr = format!("{}:{}", parsed.host, parsed.port);
    let stream = net::TcpStream::connect(&addr).map_err(|e| e.to_string())?;

    // Build HTTP request payload
    let request = build_request(method, &parsed.host, &parsed.path, body, extra_headers);

    if parsed.is_tls {
        // HTTPS path: wrap in TLS
        let config = tls::default_client_config();
        let mut tls_stream = tls::connect_tls(stream, &parsed.host, config)?;
        tls_stream
            .write_all(request.as_bytes())
            .map_err(|e| e.to_string())?;
        tls_stream.flush().map_err(|e| e.to_string())?;
        let reader = BufReader::new(tls_stream);
        read_http_response(reader)
    } else {
        // HTTP path: plain TCP
        let mut stream_ref = &stream;
        stream_ref
            .write_all(request.as_bytes())
            .map_err(|e| e.to_string())?;
        stream_ref.flush().map_err(|e| e.to_string())?;
        let reader = BufReader::new(stream);
        read_http_response(reader)
    }
}

fn build_request(
    method: &str,
    host: &str,
    path: &str,
    body: &str,
    extra_headers: &str,
) -> String {
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
    request
}

fn read_http_response<R: Read>(reader: BufReader<R>) -> Result<HttpResponseData, String> {
    let mut reader = reader;

    // Status line
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|e| e.to_string())?;
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

fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (remainder, is_tls) = if url.starts_with("https://") {
        (&url[8..], true)
    } else if url.starts_with("http://") {
        (&url[7..], false)
    } else {
        (url, false)
    };

    let (host_port, path) = if let Some(idx) = remainder.find('/') {
        (&remainder[..idx], &remainder[idx..])
    } else {
        (remainder, "/")
    };

    let (host, port) = if let Some(idx) = host_port.find(':') {
        let h = &host_port[..idx];
        let p: u16 = host_port[idx + 1..]
            .parse()
            .map_err(|_| "Invalid port".to_string())?;
        (h.to_string(), p)
    } else {
        let default_port = if is_tls { 443 } else { 80 };
        (host_port.to_string(), default_port)
    };

    Ok(ParsedUrl {
        host,
        port,
        path: path.to_string(),
        is_tls,
    })
}

fn parse_status_line(line: &str) -> Result<(u16, String), String> {
    // HTTP/1.1 200 OK
    let parts: Vec<&str> = line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err("Invalid HTTP status line".to_string());
    }
    let status: u16 = parts[1].parse().map_err(|_| "Invalid status code")?;
    let text = if parts.len() >= 3 {
        parts[2].to_string()
    } else {
        String::new()
    };
    Ok((status, text))
}

fn read_chunked_body<R: Read>(reader: &mut BufReader<R>) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        reader
            .read_line(&mut size_line)
            .map_err(|e| e.to_string())?;
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
