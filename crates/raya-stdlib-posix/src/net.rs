//! std:net — TCP/UDP sockets

use crate::handles::HandleRegistry;
use raya_sdk::{NativeCallResult, NativeContext, NativeValue, IoRequest, IoCompletion};
use std::io::{BufRead, BufReader, Read, Write};
use std::net;
use std::sync::LazyLock;

static TCP_LISTENERS: LazyLock<HandleRegistry<net::TcpListener>> = LazyLock::new(HandleRegistry::new);
static TCP_STREAMS: LazyLock<HandleRegistry<net::TcpStream>> = LazyLock::new(HandleRegistry::new);
static UDP_SOCKETS: LazyLock<HandleRegistry<net::UdpSocket>> = LazyLock::new(HandleRegistry::new);

// ── TCP Listener ──

/// Bind a TCP listener (fast syscall — stays sync)
pub fn tcp_listen(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tcpListen: {}", e)),
    };
    let port = args.get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    let addr = format!("{}:{}", host, port);
    match net::TcpListener::bind(&addr) {
        Ok(listener) => {
            let handle = TCP_LISTENERS.insert(listener);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("net.tcpListen: {}", e)),
    }
}

/// Accept a TCP connection (blocking → IO pool)
pub fn tcp_accept(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match TCP_LISTENERS.get(handle) {
                Some(listener) => match listener.accept() {
                    Ok((stream, _addr)) => {
                        let stream_handle = TCP_STREAMS.insert(stream);
                        IoCompletion::Primitive(NativeValue::f64(stream_handle as f64))
                    }
                    Err(e) => IoCompletion::Error(format!("net.tcpAccept: {}", e)),
                },
                None => IoCompletion::Error(format!("net.tcpAccept: invalid handle {}", handle)),
            }
        }),
    })
}

/// Close TCP listener
pub fn tcp_listener_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    TCP_LISTENERS.remove(handle);
    NativeCallResult::null()
}

/// Get TCP listener local address
pub fn tcp_listener_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match TCP_LISTENERS.get(handle) {
        Some(listener) => match listener.local_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
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
    let port = args.get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    let addr = format!("{}:{}", host, port);
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match net::TcpStream::connect(&addr) {
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
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let size = args.get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(4096.0) as usize;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match TCP_STREAMS.get_mut(handle) {
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
            }
        }),
    })
}

/// Read all bytes from TCP stream until EOF (blocking → IO pool)
pub fn tcp_read_all(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match TCP_STREAMS.get_mut(handle) {
                Some(mut stream) => {
                    let mut buf = Vec::new();
                    match stream.read_to_end(&mut buf) {
                        Ok(_) => IoCompletion::Bytes(buf),
                        Err(e) => IoCompletion::Error(format!("net.tcpReadAll: {}", e)),
                    }
                }
                None => IoCompletion::Error(format!("net.tcpReadAll: invalid handle {}", handle)),
            }
        }),
    })
}

/// Read a line from TCP stream (blocking → IO pool)
pub fn tcp_read_line(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match TCP_STREAMS.get_mut(handle) {
                Some(stream) => {
                    match stream.try_clone() {
                        Ok(clone) => {
                            let mut reader = BufReader::new(clone);
                            let mut line = String::new();
                            match reader.read_line(&mut line) {
                                Ok(_) => {
                                    if line.ends_with('\n') { line.pop(); }
                                    if line.ends_with('\r') { line.pop(); }
                                    IoCompletion::String(line)
                                }
                                Err(e) => IoCompletion::Error(format!("net.tcpReadLine: {}", e)),
                            }
                        }
                        Err(e) => IoCompletion::Error(format!("net.tcpReadLine: {}", e)),
                    }
                }
                None => IoCompletion::Error(format!("net.tcpReadLine: invalid handle {}", handle)),
            }
        }),
    })
}

/// Write bytes to TCP stream (blocking → IO pool)
pub fn tcp_write(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_buffer(args[1]) {
        Ok(d) => d,
        Err(e) => return NativeCallResult::Error(format!("net.tcpWrite: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match TCP_STREAMS.get_mut(handle) {
                Some(mut stream) => match stream.write(&data) {
                    Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                    Err(e) => IoCompletion::Error(format!("net.tcpWrite: {}", e)),
                },
                None => IoCompletion::Error(format!("net.tcpWrite: invalid handle {}", handle)),
            }
        }),
    })
}

/// Write string to TCP stream (blocking → IO pool)
pub fn tcp_write_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("net.tcpWriteText: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match TCP_STREAMS.get_mut(handle) {
                Some(mut stream) => match stream.write(data.as_bytes()) {
                    Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                    Err(e) => IoCompletion::Error(format!("net.tcpWriteText: {}", e)),
                },
                None => IoCompletion::Error(format!("net.tcpWriteText: invalid handle {}", handle)),
            }
        }),
    })
}

/// Close TCP stream
pub fn tcp_stream_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    TCP_STREAMS.remove(handle);
    NativeCallResult::null()
}

/// Get remote address of TCP stream
pub fn tcp_remote_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
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
    let handle = args.first()
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
    let port = args.get(1)
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
    let handle = args.first()
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
        work: Box::new(move || {
            match UDP_SOCKETS.get(handle) {
                Some(socket) => match socket.send_to(&data, &addr) {
                    Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                    Err(e) => IoCompletion::Error(format!("net.udpSendTo: {}", e)),
                },
                None => IoCompletion::Error(format!("net.udpSendTo: invalid handle {}", handle)),
            }
        }),
    })
}

/// Send text to UDP address (blocking → IO pool)
pub fn udp_send_text(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
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
        work: Box::new(move || {
            match UDP_SOCKETS.get(handle) {
                Some(socket) => match socket.send_to(data.as_bytes(), &addr) {
                    Ok(n) => IoCompletion::Primitive(NativeValue::f64(n as f64)),
                    Err(e) => IoCompletion::Error(format!("net.udpSendText: {}", e)),
                },
                None => IoCompletion::Error(format!("net.udpSendText: invalid handle {}", handle)),
            }
        }),
    })
}

/// Receive data from UDP socket (blocking → IO pool)
pub fn udp_receive(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let size = args.get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(4096.0) as usize;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match UDP_SOCKETS.get(handle) {
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
            }
        }),
    })
}

/// Close UDP socket
pub fn udp_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    UDP_SOCKETS.remove(handle);
    NativeCallResult::null()
}

/// Get local address of UDP socket
pub fn udp_local_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
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
