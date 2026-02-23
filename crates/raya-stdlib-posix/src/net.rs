//! std:net — TCP/UDP sockets + TLS streams

use crate::handles::HandleRegistry;
use crate::tls;
use dashmap::{DashMap, DashSet};
use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::io::{BufRead, BufReader, Read, Write};
use std::net;
use std::net::ToSocketAddrs;
use std::os::fd::AsRawFd;
use std::sync::LazyLock;

static TCP_LISTENERS: LazyLock<HandleRegistry<net::TcpListener>> =
    LazyLock::new(HandleRegistry::new);
static TCP_LISTENER_DISPLAY_HOST: LazyLock<DashMap<u64, String>> = LazyLock::new(DashMap::new);
static CLOSED_TCP_LISTENERS: LazyLock<DashSet<u64>> = LazyLock::new(DashSet::new);
static TCP_STREAMS: LazyLock<HandleRegistry<net::TcpStream>> = LazyLock::new(HandleRegistry::new);
static UDP_SOCKETS: LazyLock<HandleRegistry<net::UdpSocket>> = LazyLock::new(HandleRegistry::new);
static TLS_STREAMS: LazyLock<HandleRegistry<tls::ClientTlsStream>> =
    LazyLock::new(HandleRegistry::new);

fn normalize_connect_host(host: &str) -> String {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return "localhost".to_string();
    }
    if trimmed.len() >= 2 && trimmed.starts_with('[') && trimmed.ends_with(']') {
        return trimmed[1..trimmed.len() - 1].trim().to_string();
    }
    // Be forgiving with malformed bracketed hosts from string parsing ("[", "]", "[::1", etc.).
    let debracketed = trimmed.replace(['[', ']'], "").trim().to_string();
    if debracketed.is_empty() || debracketed == ":" {
        "localhost".to_string()
    } else {
        debracketed
    }
}

fn resolve_connect_addrs(host: &str, port: u16) -> Result<Vec<net::SocketAddr>, String> {
    let normalized_host = normalize_connect_host(host);
    let addrs = (normalized_host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("failed to resolve {}:{}: {}", normalized_host, port, e))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(format!(
            "failed to resolve {}:{}: no addresses returned",
            normalized_host, port
        ));
    }
    Ok(addrs)
}

fn connect_first_resolved(addrs: &[net::SocketAddr]) -> Result<net::TcpStream, std::io::Error> {
    let mut last_err: Option<std::io::Error> = None;
    for addr in addrs {
        match net::TcpStream::connect(addr) {
            Ok(stream) => return Ok(stream),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("no resolved addresses")))
}

fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

// ── TCP Listener ──

/// Bind a TCP listener (fast syscall — stays sync)
pub fn tcp_listen(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tcpListen: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    let addr = format!("{}:{}", host, port);
    match net::TcpListener::bind(&addr) {
        Ok(listener) => {
            let handle = TCP_LISTENERS.insert(listener);
            TCP_LISTENER_DISPLAY_HOST.insert(handle, normalize_connect_host(&host));
            CLOSED_TCP_LISTENERS.remove(&handle);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("net.tcpListen: {}", e)),
    }
}

/// Accept a TCP connection (blocking → IO pool)
pub fn tcp_accept(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            if CLOSED_TCP_LISTENERS.contains(&handle) {
                return IoCompletion::Primitive(NativeValue::null());
            }

            // Clone listener so close() can proceed concurrently without blocking on map guards.
            let listener = match TCP_LISTENERS.get(handle) {
                Some(entry) => match entry.try_clone() {
                    Ok(listener) => listener,
                    Err(e) => return IoCompletion::Error(format!("net.tcpAccept: {}", e)),
                },
                None => {
                    return if CLOSED_TCP_LISTENERS.contains(&handle) {
                        IoCompletion::Primitive(NativeValue::null())
                    } else {
                        IoCompletion::Error(format!("net.tcpAccept: invalid handle {}", handle))
                    };
                }
            };

            match listener.accept() {
                Ok((stream, _addr)) => {
                    if CLOSED_TCP_LISTENERS.contains(&handle) {
                        // Listener was closed while accept completed; report graceful shutdown.
                        IoCompletion::Primitive(NativeValue::null())
                    } else {
                        let stream_handle = TCP_STREAMS.insert(stream);
                        IoCompletion::Primitive(NativeValue::f64(stream_handle as f64))
                    }
                }
                Err(e) => {
                    if CLOSED_TCP_LISTENERS.contains(&handle) {
                        IoCompletion::Primitive(NativeValue::null())
                    } else {
                        IoCompletion::Error(format!("net.tcpAccept: {}", e))
                    }
                }
            }
        }),
    })
}

/// Close TCP listener
pub fn tcp_listener_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    CLOSED_TCP_LISTENERS.insert(handle);
    TCP_LISTENER_DISPLAY_HOST.remove(&handle);
    if let Some((_id, listener)) = TCP_LISTENERS.remove(handle) {
        // Wake any concurrent blocking accept() calls immediately.
        let _ = unsafe { libc::shutdown(listener.as_raw_fd(), libc::SHUT_RDWR) };
    }
    NativeCallResult::null()
}

/// Get TCP listener local address
pub fn tcp_listener_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match TCP_LISTENERS.get(handle) {
        Some(listener) => match listener.local_addr() {
            Ok(addr) => {
                let host = TCP_LISTENER_DISPLAY_HOST
                    .get(&handle)
                    .map(|h| h.value().clone())
                    .unwrap_or_else(|| addr.ip().to_string());
                let display = format_host_port(&host, addr.port());
                NativeCallResult::Value(ctx.create_string(&display))
            }
            Err(e) => NativeCallResult::Error(format!("net.tcpListenerAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("net.tcpListenerAddr: invalid handle {}", handle)),
    }
}

// ── TCP Stream ──

/// Connect to TCP server (blocking → IO pool)
pub fn tcp_connect(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tcpConnect: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let addrs = match resolve_connect_addrs(&host, port) {
                Ok(addrs) => addrs,
                Err(e) => return IoCompletion::Error(format!("net.tcpConnect: {}", e)),
            };
            match connect_first_resolved(&addrs) {
                Ok(stream) => {
                    let handle = TCP_STREAMS.insert(stream);
                    IoCompletion::Primitive(NativeValue::f64(handle as f64))
                }
                Err(e) => IoCompletion::Error(format!("net.tcpConnect: {}", e)),
            }
        }),
    })
}

/// Read up to N bytes from TCP stream (blocking → IO pool)
pub fn tcp_read(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let size = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(4096.0) as usize;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TCP_STREAMS.get_mut(handle) {
            Some(mut stream) => {
                let mut buf = vec![0u8; size];
                match stream.read(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        IoCompletion::Bytes(buf)
                    }
                    Err(e) => IoCompletion::Error(format!("net.tcpRead: {}", e)),
                }
            }
            None => IoCompletion::Error(format!("net.tcpRead: invalid handle {}", handle)),
        }),
    })
}

/// Read all bytes from TCP stream until EOF (blocking → IO pool)
pub fn tcp_read_all(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TCP_STREAMS.get_mut(handle) {
            Some(mut stream) => {
                let mut buf = Vec::new();
                match stream.read_to_end(&mut buf) {
                    Ok(_) => IoCompletion::Bytes(buf),
                    Err(e) => IoCompletion::Error(format!("net.tcpReadAll: {}", e)),
                }
            }
            None => IoCompletion::Error(format!("net.tcpReadAll: invalid handle {}", handle)),
        }),
    })
}

/// Read a line from TCP stream (blocking → IO pool)
pub fn tcp_read_line(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TCP_STREAMS.get_mut(handle) {
            Some(stream) => match stream.try_clone() {
                Ok(clone) => {
                    let mut reader = BufReader::new(clone);
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(_) => {
                            if line.ends_with('\n') {
                                line.pop();
                            }
                            if line.ends_with('\r') {
                                line.pop();
                            }
                            IoCompletion::String(line)
                        }
                        Err(e) => IoCompletion::Error(format!("net.tcpReadLine: {}", e)),
                    }
                }
                Err(e) => IoCompletion::Error(format!("net.tcpReadLine: {}", e)),
            },
            None => IoCompletion::Error(format!("net.tcpReadLine: invalid handle {}", handle)),
        }),
    })
}

/// Write bytes to TCP stream (blocking → IO pool)
pub fn tcp_write(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_buffer(args[1]) {
        Ok(d) => d,
        Err(e) => return NativeCallResult::Error(format!("net.tcpWrite: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TCP_STREAMS.get_mut(handle) {
            Some(mut stream) => match stream.write(&data) {
                Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                Err(e) => IoCompletion::Error(format!("net.tcpWrite: {}", e)),
            },
            None => IoCompletion::Error(format!("net.tcpWrite: invalid handle {}", handle)),
        }),
    })
}

/// Write string to TCP stream (blocking → IO pool)
pub fn tcp_write_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tcpWriteText: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TCP_STREAMS.get_mut(handle) {
            Some(mut stream) => match stream.write(data.as_bytes()) {
                Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                Err(e) => IoCompletion::Error(format!("net.tcpWriteText: {}", e)),
            },
            None => IoCompletion::Error(format!("net.tcpWriteText: invalid handle {}", handle)),
        }),
    })
}

/// Close TCP stream
pub fn tcp_stream_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    TCP_STREAMS.remove(handle);
    NativeCallResult::null()
}

/// Get remote address of TCP stream
pub fn tcp_remote_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match TCP_STREAMS.get(handle) {
        Some(stream) => match stream.peer_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
            Err(e) => NativeCallResult::Error(format!("net.tcpRemoteAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("net.tcpRemoteAddr: invalid handle {}", handle)),
    }
}

/// Get local address of TCP stream
pub fn tcp_local_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match TCP_STREAMS.get(handle) {
        Some(stream) => match stream.local_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
            Err(e) => NativeCallResult::Error(format!("net.tcpLocalAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("net.tcpLocalAddr: invalid handle {}", handle)),
    }
}

// ── UDP Socket ──

/// Bind UDP socket (fast syscall — stays sync)
pub fn udp_bind(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.udpBind: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    let addr = format!("{}:{}", host, port);
    match net::UdpSocket::bind(&addr) {
        Ok(socket) => {
            let handle = UDP_SOCKETS.insert(socket);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("net.udpBind: {}", e)),
    }
}

/// Send data to UDP address (blocking → IO pool)
pub fn udp_send_to(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_buffer(args[1]) {
        Ok(d) => d,
        Err(e) => return NativeCallResult::Error(format!("net.udpSendTo: {}", e)),
    };
    let addr = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.udpSendTo: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match UDP_SOCKETS.get(handle) {
            Some(socket) => match socket.send_to(&data, &addr) {
                Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                Err(e) => IoCompletion::Error(format!("net.udpSendTo: {}", e)),
            },
            None => IoCompletion::Error(format!("net.udpSendTo: invalid handle {}", handle)),
        }),
    })
}

/// Send text to UDP address (blocking → IO pool)
pub fn udp_send_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.udpSendText: {}", e)),
    };
    let addr = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.udpSendText: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match UDP_SOCKETS.get(handle) {
            Some(socket) => match socket.send_to(data.as_bytes(), &addr) {
                Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                Err(e) => IoCompletion::Error(format!("net.udpSendText: {}", e)),
            },
            None => IoCompletion::Error(format!("net.udpSendText: invalid handle {}", handle)),
        }),
    })
}

/// Receive data from UDP socket (blocking → IO pool)
pub fn udp_receive(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let size = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(4096.0) as usize;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match UDP_SOCKETS.get(handle) {
            Some(socket) => {
                let mut buf = vec![0u8; size];
                match socket.recv_from(&mut buf) {
                    Ok((n, _addr)) => {
                        buf.truncate(n);
                        IoCompletion::Bytes(buf)
                    }
                    Err(e) => IoCompletion::Error(format!("net.udpReceive: {}", e)),
                }
            }
            None => IoCompletion::Error(format!("net.udpReceive: invalid handle {}", handle)),
        }),
    })
}

/// Close UDP socket
pub fn udp_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    UDP_SOCKETS.remove(handle);
    NativeCallResult::null()
}

/// Get local address of UDP socket
pub fn udp_local_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match UDP_SOCKETS.get(handle) {
        Some(socket) => match socket.local_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
            Err(e) => NativeCallResult::Error(format!("net.udpLocalAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("net.udpLocalAddr: invalid handle {}", handle)),
    }
}

// ── TLS Stream ──

/// Connect to TLS server with default CA roots (blocking → IO pool)
pub fn tls_connect(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tlsConnect: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(443.0) as u16;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let addrs = match resolve_connect_addrs(&host, port) {
                Ok(addrs) => addrs,
                Err(e) => return IoCompletion::Error(format!("net.tlsConnect: {}", e)),
            };
            let sni_host = normalize_connect_host(&host);
            match connect_first_resolved(&addrs) {
                Ok(stream) => {
                    let config = tls::default_client_config();
                    match tls::connect_tls(stream, &sni_host, config) {
                        Ok(tls_stream) => {
                            let handle = TLS_STREAMS.insert(tls_stream);
                            IoCompletion::Primitive(NativeValue::f64(handle as f64))
                        }
                        Err(e) => IoCompletion::Error(format!("net.tlsConnect: {}", e)),
                    }
                }
                Err(e) => IoCompletion::Error(format!("net.tlsConnect: {}", e)),
            }
        }),
    })
}

/// Connect to TLS server with custom CA certificate (blocking → IO pool)
pub fn tls_connect_with_ca(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tlsConnectWithCa: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(443.0) as u16;
    let ca_pem = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tlsConnectWithCa: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let config = match tls::client_config_with_ca(&ca_pem) {
                Ok(c) => c,
                Err(e) => return IoCompletion::Error(format!("net.tlsConnectWithCa: {}", e)),
            };
            let addrs = match resolve_connect_addrs(&host, port) {
                Ok(addrs) => addrs,
                Err(e) => return IoCompletion::Error(format!("net.tlsConnectWithCa: {}", e)),
            };
            let sni_host = normalize_connect_host(&host);
            match connect_first_resolved(&addrs) {
                Ok(stream) => match tls::connect_tls(stream, &sni_host, config) {
                    Ok(tls_stream) => {
                        let handle = TLS_STREAMS.insert(tls_stream);
                        IoCompletion::Primitive(NativeValue::f64(handle as f64))
                    }
                    Err(e) => IoCompletion::Error(format!("net.tlsConnectWithCa: {}", e)),
                },
                Err(e) => IoCompletion::Error(format!("net.tlsConnectWithCa: {}", e)),
            }
        }),
    })
}

/// Read up to N bytes from TLS stream (blocking → IO pool)
pub fn tls_read(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let size = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(4096.0) as usize;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TLS_STREAMS.get_mut(handle) {
            Some(mut stream) => {
                let mut buf = vec![0u8; size];
                match stream.read(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        IoCompletion::Bytes(buf)
                    }
                    Err(e) => IoCompletion::Error(format!("net.tlsRead: {}", e)),
                }
            }
            None => IoCompletion::Error(format!("net.tlsRead: invalid handle {}", handle)),
        }),
    })
}

/// Read all bytes from TLS stream until EOF (blocking → IO pool)
pub fn tls_read_all(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TLS_STREAMS.get_mut(handle) {
            Some(mut stream) => {
                let mut buf = Vec::new();
                match stream.read_to_end(&mut buf) {
                    Ok(_) => IoCompletion::Bytes(buf),
                    Err(e) => IoCompletion::Error(format!("net.tlsReadAll: {}", e)),
                }
            }
            None => IoCompletion::Error(format!("net.tlsReadAll: invalid handle {}", handle)),
        }),
    })
}

/// Read a line from TLS stream (blocking → IO pool)
pub fn tls_read_line(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match TLS_STREAMS.get_mut(handle) {
                Some(mut stream) => {
                    // Read byte-by-byte until newline (TLS streams can't clone)
                    let mut line = Vec::new();
                    let mut byte = [0u8; 1];
                    loop {
                        match stream.read(&mut byte) {
                            Ok(0) => break,
                            Ok(_) => {
                                if byte[0] == b'\n' {
                                    break;
                                }
                                line.push(byte[0]);
                            }
                            Err(e) => {
                                return IoCompletion::Error(format!("net.tlsReadLine: {}", e))
                            }
                        }
                    }
                    // Strip trailing \r if present
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    let s = String::from_utf8_lossy(&line).into_owned();
                    IoCompletion::String(s)
                }
                None => IoCompletion::Error(format!("net.tlsReadLine: invalid handle {}", handle)),
            }
        }),
    })
}

/// Write bytes to TLS stream (blocking → IO pool)
pub fn tls_write(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_buffer(args[1]) {
        Ok(d) => d,
        Err(e) => return NativeCallResult::Error(format!("net.tlsWrite: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TLS_STREAMS.get_mut(handle) {
            Some(mut stream) => match stream.write(&data) {
                Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                Err(e) => IoCompletion::Error(format!("net.tlsWrite: {}", e)),
            },
            None => IoCompletion::Error(format!("net.tlsWrite: invalid handle {}", handle)),
        }),
    })
}

/// Write string to TLS stream (blocking → IO pool)
pub fn tls_write_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tlsWriteText: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match TLS_STREAMS.get_mut(handle) {
            Some(mut stream) => match stream.write(data.as_bytes()) {
                Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                Err(e) => IoCompletion::Error(format!("net.tlsWriteText: {}", e)),
            },
            None => IoCompletion::Error(format!("net.tlsWriteText: invalid handle {}", handle)),
        }),
    })
}

/// Close TLS stream
pub fn tls_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    TLS_STREAMS.remove(handle);
    NativeCallResult::null()
}

/// Get remote address of TLS stream
pub fn tls_remote_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match TLS_STREAMS.get(handle) {
        Some(stream) => match stream.sock.peer_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
            Err(e) => NativeCallResult::Error(format!("net.tlsRemoteAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("net.tlsRemoteAddr: invalid handle {}", handle)),
    }
}

/// Get local address of TLS stream
pub fn tls_local_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match TLS_STREAMS.get(handle) {
        Some(stream) => match stream.sock.local_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
            Err(e) => NativeCallResult::Error(format!("net.tlsLocalAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("net.tlsLocalAddr: invalid handle {}", handle)),
    }
}
