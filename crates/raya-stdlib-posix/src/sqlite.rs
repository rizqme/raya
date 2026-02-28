//! std:sqlite — SQLite native bindings (rusqlite-backed subset)

use crate::handles::HandleRegistry;
use parking_lot::Mutex;
use raya_sdk::{NativeCallResult, NativeContext, NativeValue};
use rusqlite::{types::ValueRef, Connection};
use std::sync::{Arc, LazyLock};

static SQLITE_DBS: LazyLock<HandleRegistry<Arc<Mutex<Connection>>>> =
    LazyLock::new(HandleRegistry::new);

pub fn open(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("sqlite.open: {}", e)),
    };
    match Connection::open(path) {
        Ok(conn) => {
            let handle = SQLITE_DBS.insert(Arc::new(Mutex::new(conn)));
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("sqlite.open: {}", e)),
    }
}

pub fn close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    SQLITE_DBS.remove(handle);
    NativeCallResult::null()
}

pub fn exec(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let sql = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("sqlite.exec: {}", e)),
    };

    let Some(db) = SQLITE_DBS.get(handle) else {
        return NativeCallResult::Error(format!("sqlite.exec: invalid handle {}", handle));
    };

    let conn = db.lock();
    match conn.execute_batch(&sql) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("sqlite.exec: {}", e)),
    }
}

pub fn query(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let sql = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("sqlite.query: {}", e)),
    };

    let Some(db) = SQLITE_DBS.get(handle) else {
        return NativeCallResult::Error(format!("sqlite.query: invalid handle {}", handle));
    };

    let conn = db.lock();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("sqlite.query: {}", e)),
    };

    let col_count = stmt.column_count();
    let rows = match stmt.query_map([], |row| {
        let mut cols: Vec<String> = Vec::with_capacity(col_count);
        for i in 0..col_count {
            let s = match row.get_ref(i)? {
                ValueRef::Null => "null".to_string(),
                ValueRef::Integer(v) => v.to_string(),
                ValueRef::Real(v) => v.to_string(),
                ValueRef::Text(v) => String::from_utf8_lossy(v).into_owned(),
                ValueRef::Blob(v) => format!("<blob:{}>", v.len()),
            };
            cols.push(s);
        }
        Ok(cols.join("\t"))
    }) {
        Ok(r) => r,
        Err(e) => return NativeCallResult::Error(format!("sqlite.query: {}", e)),
    };

    let mut out = Vec::new();
    for row in rows {
        match row {
            Ok(s) => out.push(ctx.create_string(&s)),
            Err(e) => return NativeCallResult::Error(format!("sqlite.query: {}", e)),
        }
    }

    NativeCallResult::Value(ctx.create_array(&out))
}
