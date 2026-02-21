//! std:ws — WebSocket client and server
//!
//! Provides WebSocket connectivity via the tungstenite library.
//! Supports both `ws://` (plain TCP) and `wss://` (TLS) connections.

use crate::handles::HandleRegistry;
use crate::tls;
use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{LazyLock, Mutex, MutexGuard};
use tungstenite::protocol::{CloseFrame, WebSocket};
use tungstenite::Message;

// ── Type-erased stream for ws:// and wss:// ──

/// Combined Read+Write trait for type-erased stream objects.
/// Rust does not allow `dyn Read + Write` directly; we need a supertrait.
trait ReadWrite: Read + Write + Send {}
impl<T: Read + Write + Send> ReadWrite for T {}

type WsSocket = WebSocket<Box<dyn ReadWrite>>;

struct WsHandle {
    ws: Mutex<WsSocket>,
    /// Peer address captured at connect/accept time
    peer_addr: String,
    /// Negotiated subprotocol (if any)
    protocol: String,
}

static WS_HANDLES: LazyLock<HandleRegistry<WsHandle>> = LazyLock::new(HandleRegistry::new);

struct WsServerHandle {
    listener: TcpListener,
}

static WS_SERVER_HANDLES: LazyLock<HandleRegistry<WsServerHandle>> =
    LazyLock::new(HandleRegistry::new);

// ── Helpers ──

fn read_handle(args: &[NativeValue]) -> u64 {
    args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64
}

/// Parse a WebSocket URL into (host, port, path, use_tls).
fn parse_ws_url(url: &str) -> Result<(String, u16, String, bool), String> {
    let use_tls = if url.starts_with("wss://") {
        true
    } else if url.starts_with("ws://") {
        false
    } else {
        return Err(format!("ws.connect: unsupported scheme in URL '{}'", url));
    };

    let without_scheme = if use_tls { &url[6..] } else { &url[5..] };
    let (host_port, path) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None => (without_scheme, "/"),
    };

    let (host, port) = if let Some(colon) = host_port.rfind(':') {
        let h = &host_port[..colon];
        let p = host_port[colon + 1..]
            .parse::<u16>()
            .map_err(|e| format!("ws.connect: invalid port: {}", e))?;
        (h.to_string(), p)
    } else {
        (host_port.to_string(), if use_tls { 443 } else { 80 })
    };

    Ok((host, port, path.to_string(), use_tls))
}

/// Perform the WebSocket client handshake on a boxed stream.
fn ws_client_handshake(
    url: &str,
    stream: Box<dyn ReadWrite>,
    extra_headers: Option<Vec<(String, String)>>,
) -> Result<(WsSocket, String), String> {
    use tungstenite::handshake::client::generate_key;
    use tungstenite::http::Request;

    let mut builder = Request::builder()
        .uri(url)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key());

    if let Some(headers) = &extra_headers {
        for (name, value) in headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
    }

    let request = builder
        .body(())
        .map_err(|e| format!("ws.connect: failed to build request: {}", e))?;

    let (ws, response) = tungstenite::client(request, stream)
        .map_err(|e| format!("ws.connect: handshake failed: {}", e))?;

    // Extract negotiated subprotocol from response headers
    let protocol = response
        .headers()
        .get("Sec-WebSocket-Protocol")
        .and_then(|v: &tungstenite::http::HeaderValue| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    Ok((ws, protocol))
}

// ── 1. ws.connect(url) ──

/// Connect to a WebSocket server (blocking -> IO pool)
pub fn connect(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let url = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("ws.connect: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let (host, port, _path, use_tls) = match parse_ws_url(&url) {
                Ok(v) => v,
                Err(e) => return IoCompletion::Error(e),
            };

            let addr = format!("{}:{}", host, port);
            let tcp = match TcpStream::connect(&addr) {
                Ok(s) => s,
                Err(e) => return IoCompletion::Error(format!("ws.connect: {}", e)),
            };

            let peer_addr = tcp
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_default();

            let stream: Box<dyn ReadWrite> = if use_tls {
                let config = tls::default_client_config();
                match tls::connect_tls(tcp, &host, config) {
                    Ok(tls_stream) => Box::new(tls_stream),
                    Err(e) => return IoCompletion::Error(format!("ws.connect: TLS: {}", e)),
                }
            } else {
                Box::new(tcp)
            };

            let (ws, protocol) = match ws_client_handshake(&url, stream, None) {
                Ok(v) => v,
                Err(e) => return IoCompletion::Error(e),
            };

            let handle = WS_HANDLES.insert(WsHandle {
                ws: Mutex::new(ws),
                peer_addr,
                protocol,
            });
            IoCompletion::Primitive(NativeValue::f64(handle as f64))
        }),
    })
}

// ── 2. ws.connectWithProtocols(url, protocols) ──

/// Connect with subprotocol negotiation (blocking -> IO pool)
pub fn connect_with_protocols(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let url = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("ws.connectWithProtocols: {}", e)),
    };
    let protocols = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("ws.connectWithProtocols: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let (host, port, _path, use_tls) = match parse_ws_url(&url) {
                Ok(v) => v,
                Err(e) => return IoCompletion::Error(e),
            };

            let addr = format!("{}:{}", host, port);
            let tcp = match TcpStream::connect(&addr) {
                Ok(s) => s,
                Err(e) => {
                    return IoCompletion::Error(format!("ws.connectWithProtocols: {}", e))
                }
            };

            let peer_addr = tcp
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_default();

            let stream: Box<dyn ReadWrite> = if use_tls {
                let config = tls::default_client_config();
                match tls::connect_tls(tcp, &host, config) {
                    Ok(tls_stream) => Box::new(tls_stream),
                    Err(e) => {
                        return IoCompletion::Error(format!(
                            "ws.connectWithProtocols: TLS: {}",
                            e
                        ))
                    }
                }
            } else {
                Box::new(tcp)
            };

            let headers = vec![(
                "Sec-WebSocket-Protocol".to_string(),
                protocols.clone(),
            )];

            let (ws, protocol) = match ws_client_handshake(&url, stream, Some(headers)) {
                Ok(v) => v,
                Err(e) => return IoCompletion::Error(e),
            };

            let handle = WS_HANDLES.insert(WsHandle {
                ws: Mutex::new(ws),
                peer_addr,
                protocol,
            });
            IoCompletion::Primitive(NativeValue::f64(handle as f64))
        }),
    })
}

// ── 3. ws.serverCreate(host, port) ──

/// Create a WebSocket server (TCP listener)
pub fn server_create(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("ws.serverCreate: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    let addr = format!("{}:{}", host, port);
    match TcpListener::bind(&addr) {
        Ok(listener) => {
            let handle = WS_SERVER_HANDLES.insert(WsServerHandle { listener });
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("ws.serverCreate: {}", e)),
    }
}

// ── 4. ws.serverAccept(handle) ──

/// Accept a WebSocket connection (blocking -> IO pool)
pub fn server_accept(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let listener_ref = match WS_SERVER_HANDLES.get(handle) {
                Some(r) => r,
                None => {
                    return IoCompletion::Error(format!(
                        "ws.serverAccept: invalid handle {}",
                        handle
                    ))
                }
            };

            let (tcp_stream, addr) = match listener_ref.listener.accept() {
                Ok(v) => v,
                Err(e) => return IoCompletion::Error(format!("ws.serverAccept: {}", e)),
            };
            let peer_addr = addr.to_string();

            // Drop the listener ref before doing the handshake to avoid holding the DashMap lock
            drop(listener_ref);

            let stream: Box<dyn ReadWrite> = Box::new(tcp_stream);
            let ws: WsSocket = match tungstenite::accept(stream) {
                Ok(ws) => ws,
                Err(e) => return IoCompletion::Error(format!("ws.serverAccept: {}", e)),
            };

            let ws_handle = WS_HANDLES.insert(WsHandle {
                ws: Mutex::new(ws),
                peer_addr,
                protocol: String::new(),
            });
            IoCompletion::Primitive(NativeValue::f64(ws_handle as f64))
        }),
    })
}

// ── 5. ws.serverClose(handle) ──

/// Close a WebSocket server
pub fn server_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    WS_SERVER_HANDLES.remove(handle);
    NativeCallResult::null()
}

// ── 6. ws.serverAddr(handle) ──

/// Get the local address of a WebSocket server
pub fn server_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    match WS_SERVER_HANDLES.get(handle) {
        Some(server) => match server.listener.local_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
            Err(e) => NativeCallResult::Error(format!("ws.serverAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("ws.serverAddr: invalid handle {}", handle)),
    }
}

// ── 7. ws.send(handle, message) ──

/// Send a text message (blocking -> IO pool)
pub fn send(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    let message = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("ws.send: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match WS_HANDLES.get(handle) {
                Some(ws_handle) => {
                    let mut ws: MutexGuard<'_, WsSocket> = match ws_handle.ws.lock() {
                        Ok(g) => g,
                        Err(e) => return IoCompletion::Error(format!("ws.send: lock: {}", e)),
                    };
                    match ws.send(Message::Text(message)) {
                        Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                        Err(e) => IoCompletion::Error(format!("ws.send: {}", e)),
                    }
                }
                None => IoCompletion::Error(format!("ws.send: invalid handle {}", handle)),
            }
        }),
    })
}

// ── 8. ws.sendBytes(handle, data) ──

/// Send a binary message (blocking -> IO pool)
pub fn send_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    let data = match ctx.read_buffer(args[1]) {
        Ok(d) => d,
        Err(e) => return NativeCallResult::Error(format!("ws.sendBytes: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match WS_HANDLES.get(handle) {
                Some(ws_handle) => {
                    let mut ws: MutexGuard<'_, WsSocket> = match ws_handle.ws.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            return IoCompletion::Error(format!("ws.sendBytes: lock: {}", e))
                        }
                    };
                    match ws.send(Message::Binary(data)) {
                        Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                        Err(e) => IoCompletion::Error(format!("ws.sendBytes: {}", e)),
                    }
                }
                None => IoCompletion::Error(format!("ws.sendBytes: invalid handle {}", handle)),
            }
        }),
    })
}

// ── 9. ws.receive(handle) ──

/// Receive a message as string (blocking -> IO pool)
pub fn receive(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match WS_HANDLES.get(handle) {
                Some(ws_handle) => {
                    let mut ws: MutexGuard<'_, WsSocket> = match ws_handle.ws.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            return IoCompletion::Error(format!("ws.receive: lock: {}", e))
                        }
                    };
                    match ws.read() {
                        Ok(msg) => match msg {
                            Message::Text(text) => IoCompletion::String(text),
                            Message::Binary(data) => {
                                IoCompletion::String(String::from_utf8_lossy(&data).into_owned())
                            }
                            Message::Close(_) => IoCompletion::String(String::new()),
                            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
                                // Control frames: return empty string.
                                // The Raya-level wrapper can loop if needed.
                                IoCompletion::String(String::new())
                            }
                        },
                        Err(e) => IoCompletion::Error(format!("ws.receive: {}", e)),
                    }
                }
                None => IoCompletion::Error(format!("ws.receive: invalid handle {}", handle)),
            }
        }),
    })
}

// ── 10. ws.receiveBytes(handle) ──

/// Receive a message as bytes (blocking -> IO pool)
pub fn receive_bytes(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match WS_HANDLES.get(handle) {
                Some(ws_handle) => {
                    let mut ws: MutexGuard<'_, WsSocket> = match ws_handle.ws.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            return IoCompletion::Error(format!("ws.receiveBytes: lock: {}", e))
                        }
                    };
                    match ws.read() {
                        Ok(msg) => match msg {
                            Message::Binary(data) => IoCompletion::Bytes(data),
                            Message::Text(text) => IoCompletion::Bytes(text.into_bytes()),
                            Message::Close(_) => IoCompletion::Bytes(Vec::new()),
                            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
                                IoCompletion::Bytes(Vec::new())
                            }
                        },
                        Err(e) => IoCompletion::Error(format!("ws.receiveBytes: {}", e)),
                    }
                }
                None => {
                    IoCompletion::Error(format!("ws.receiveBytes: invalid handle {}", handle))
                }
            }
        }),
    })
}

// ── 11. ws.close(handle) ──

/// Close a WebSocket connection
pub fn close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    if let Some(ws_handle) = WS_HANDLES.get(handle) {
        if let Ok(mut ws) = ws_handle.ws.lock() {
            let ws: &mut WsSocket = &mut ws;
            let _ = ws.close(None);
            let _ = ws.flush();
        }
    }
    WS_HANDLES.remove(handle);
    NativeCallResult::null()
}

// ── 12. ws.closeWithCode(handle, code, reason) ──

/// Close with a specific close code and reason
pub fn close_with_code(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    let code = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(1000.0) as u16;
    let reason = ctx.read_string(args[2]).unwrap_or_default();

    if let Some(ws_handle) = WS_HANDLES.get(handle) {
        if let Ok(mut ws) = ws_handle.ws.lock() {
            let ws: &mut WsSocket = &mut ws;
            let frame = CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::from(code),
                reason: reason.into(),
            };
            let _ = ws.close(Some(frame));
            let _ = ws.flush();
        }
    }
    WS_HANDLES.remove(handle);
    NativeCallResult::null()
}

// ── 13. ws.isOpen(handle) ──

/// Check if the WebSocket is still open (synchronous)
pub fn is_open(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    match WS_HANDLES.get(handle) {
        Some(ws_handle) => match ws_handle.ws.lock() {
            Ok(ws) => {
                let ws: &WsSocket = &ws;
                NativeCallResult::bool(ws.can_read())
            }
            Err(_) => NativeCallResult::bool(false),
        },
        None => NativeCallResult::bool(false),
    }
}

// ── 14. ws.remoteAddr(handle) ──

/// Get remote address of a WebSocket connection
pub fn remote_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    match WS_HANDLES.get(handle) {
        Some(ws_handle) => {
            NativeCallResult::Value(ctx.create_string(&ws_handle.peer_addr))
        }
        None => NativeCallResult::Error(format!("ws.remoteAddr: invalid handle {}", handle)),
    }
}

// ── 15. ws.protocol(handle) ──

/// Get the negotiated subprotocol
pub fn protocol(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = read_handle(args);
    match WS_HANDLES.get(handle) {
        Some(ws_handle) => {
            NativeCallResult::Value(ctx.create_string(&ws_handle.protocol))
        }
        None => NativeCallResult::Error(format!("ws.protocol: invalid handle {}", handle)),
    }
}
