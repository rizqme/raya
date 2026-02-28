//! std:http2 — pragmatic native-backed subset

use crate::handles::HandleRegistry;
use raya_sdk::{NativeCallResult, NativeContext, NativeValue};
use std::net;
use std::sync::LazyLock;

static HTTP2_SERVERS: LazyLock<HandleRegistry<net::TcpListener>> =
    LazyLock::new(HandleRegistry::new);
static HTTP2_CLIENTS: LazyLock<HandleRegistry<net::TcpStream>> = LazyLock::new(HandleRegistry::new);

pub fn server_create(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("http2.serverCreate: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    let addr = format!("{}:{}", host, port);
    match net::TcpListener::bind(&addr) {
        Ok(listener) => NativeCallResult::f64(HTTP2_SERVERS.insert(listener) as f64),
        Err(e) => NativeCallResult::Error(format!("http2.serverCreate: {}", e)),
    }
}

pub fn server_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    HTTP2_SERVERS.remove(handle);
    NativeCallResult::null()
}

pub fn server_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP2_SERVERS.get(handle) {
        Some(server) => match server.local_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.ip().to_string())),
            Err(e) => NativeCallResult::Error(format!("http2.serverAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("http2.serverAddr: invalid handle {}", handle)),
    }
}

pub fn server_port(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP2_SERVERS.get(handle) {
        Some(server) => match server.local_addr() {
            Ok(addr) => NativeCallResult::f64(addr.port() as f64),
            Err(e) => NativeCallResult::Error(format!("http2.serverPort: {}", e)),
        },
        None => NativeCallResult::Error(format!("http2.serverPort: invalid handle {}", handle)),
    }
}

pub fn client_connect(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let host = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("http2.clientConnect: {}", e)),
    };
    let port = args
        .get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u16;
    let addr = format!("{}:{}", host, port);
    match net::TcpStream::connect(&addr) {
        Ok(stream) => NativeCallResult::f64(HTTP2_CLIENTS.insert(stream) as f64),
        Err(e) => NativeCallResult::Error(format!("http2.clientConnect: {}", e)),
    }
}

pub fn client_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    HTTP2_CLIENTS.remove(handle);
    NativeCallResult::null()
}

pub fn client_addr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match HTTP2_CLIENTS.get(handle) {
        Some(client) => match client.peer_addr() {
            Ok(addr) => NativeCallResult::Value(ctx.create_string(&addr.to_string())),
            Err(e) => NativeCallResult::Error(format!("http2.clientAddr: {}", e)),
        },
        None => NativeCallResult::Error(format!("http2.clientAddr: invalid handle {}", handle)),
    }
}
