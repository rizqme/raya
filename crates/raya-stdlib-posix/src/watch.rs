//! std:watch — OS-native file system watching
//!
//! Uses the `notify` crate (v7) for cross-platform file watching.
//! Events are queued via an mpsc channel and consumed one at a time
//! through `watch.nextEvent`, which suspends the calling goroutine
//! on the IO thread pool until an event arrives.

use crate::handles::HandleRegistry;
use notify::{recommended_watcher, EventKind, RecursiveMode, Watcher};
use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};

/// Internal watcher state held behind a handle.
struct WatcherHandle {
    /// The underlying OS watcher (kept alive to receive events).
    _watcher: notify::RecommendedWatcher,
    /// Shared receiver for file-system events.
    receiver: Arc<Mutex<mpsc::Receiver<notify::Result<notify::Event>>>>,
}

/// Global handle registry for watcher instances.
static WATCHER_HANDLES: LazyLock<HandleRegistry<Mutex<WatcherHandle>>> =
    LazyLock::new(HandleRegistry::new);

/// Helper: extract a handle ID from a `NativeValue`.
fn extract_handle(args: &[NativeValue]) -> u64 {
    args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64
}

/// Create a file watcher for the given paths (non-recursive).
///
/// Args: `paths: string[]`
/// Returns: handle (f64)
pub fn watch_create(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    create_watcher_inner(ctx, args, RecursiveMode::NonRecursive)
}

/// Create a file watcher for the given paths with explicit recursive flag.
///
/// Args: `paths: string[], recursive: boolean`
/// Returns: handle (f64)
pub fn watch_create_recursive(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let recursive = args
        .get(1)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };

    create_watcher_inner(ctx, args, mode)
}

/// Shared implementation for `watch_create` and `watch_create_recursive`.
fn create_watcher_inner(
    ctx: &dyn NativeContext,
    args: &[NativeValue],
    mode: RecursiveMode,
) -> NativeCallResult {
    // Read paths array
    let arr = match args.first() {
        Some(v) => *v,
        None => return NativeCallResult::Error("watch.create: missing paths argument".into()),
    };
    let len = match ctx.array_len(arr) {
        Ok(n) => n,
        Err(e) => return NativeCallResult::Error(format!("watch.create: {}", e)),
    };
    let mut paths = Vec::with_capacity(len);
    for i in 0..len {
        let elem = match ctx.array_get(arr, i) {
            Ok(v) => v,
            Err(e) => return NativeCallResult::Error(format!("watch.create: {}", e)),
        };
        let s = match ctx.read_string(elem) {
            Ok(s) => s,
            Err(e) => return NativeCallResult::Error(format!("watch.create: {}", e)),
        };
        paths.push(s);
    }

    // Create mpsc channel for events
    let (tx, rx) = mpsc::channel();

    // Create the OS watcher with callback that sends to channel
    let mut watcher = match recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => return NativeCallResult::Error(format!("watch.create: {}", e)),
    };

    // Watch all requested paths
    for path_str in &paths {
        if let Err(e) = watcher.watch(Path::new(path_str), mode) {
            return NativeCallResult::Error(format!("watch.create: failed to watch '{}': {}", path_str, e));
        }
    }

    let handle = WatcherHandle {
        _watcher: watcher,
        receiver: Arc::new(Mutex::new(rx)),
    };

    let id = WATCHER_HANDLES.insert(Mutex::new(handle));
    NativeCallResult::f64(id as f64)
}

/// Block until the next file-system event arrives.
///
/// Args: `handle: number`
/// Returns: event string in `"kind:path"` format
///
/// This suspends the calling goroutine on the IO thread pool,
/// blocking on the mpsc receiver until an event is available.
pub fn watch_next_event(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);

    // Clone the Arc<Mutex<Receiver>> so we can move it into the closure
    // without holding the DashMap guard during the blocking recv().
    let receiver = {
        let guard = match WATCHER_HANDLES.get(handle) {
            Some(g) => g,
            None => return NativeCallResult::Error("watch.nextEvent: invalid handle".into()),
        };
        let wh = guard.lock().unwrap();
        Arc::clone(&wh.receiver)
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let rx = receiver.lock().unwrap();
            match rx.recv() {
                Ok(Ok(event)) => {
                    let kind = match event.kind {
                        EventKind::Create(_) => "create",
                        EventKind::Modify(_) => "modify",
                        EventKind::Remove(_) => "remove",
                        EventKind::Access(_) => "access",
                        _ => "other",
                    };
                    let path = event
                        .paths
                        .first()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    IoCompletion::String(format!("{}:{}", kind, path))
                }
                Ok(Err(e)) => IoCompletion::Error(format!("watch error: {}", e)),
                Err(_) => IoCompletion::Error("watcher closed".into()),
            }
        }),
    })
}

/// Add a path to an existing watcher (non-recursive).
///
/// Args: `handle: number, path: string`
pub fn watch_add_path(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);
    let path_str = match args.get(1).map(|v| ctx.read_string(*v)) {
        Some(Ok(s)) => s,
        Some(Err(e)) => return NativeCallResult::Error(format!("watch.addPath: {}", e)),
        None => return NativeCallResult::Error("watch.addPath: missing path argument".into()),
    };

    match WATCHER_HANDLES.get(handle) {
        Some(guard) => {
            let mut wh = guard.lock().unwrap();
            if let Err(e) = wh._watcher.watch(Path::new(&path_str), RecursiveMode::NonRecursive) {
                return NativeCallResult::Error(format!("watch.addPath: {}", e));
            }
            NativeCallResult::null()
        }
        None => NativeCallResult::Error("watch.addPath: invalid handle".into()),
    }
}

/// Remove a path from an existing watcher.
///
/// Args: `handle: number, path: string`
pub fn watch_remove_path(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);
    let path_str = match args.get(1).map(|v| ctx.read_string(*v)) {
        Some(Ok(s)) => s,
        Some(Err(e)) => return NativeCallResult::Error(format!("watch.removePath: {}", e)),
        None => return NativeCallResult::Error("watch.removePath: missing path argument".into()),
    };

    match WATCHER_HANDLES.get(handle) {
        Some(guard) => {
            let mut wh = guard.lock().unwrap();
            if let Err(e) = wh._watcher.unwatch(Path::new(&path_str)) {
                return NativeCallResult::Error(format!("watch.removePath: {}", e));
            }
            NativeCallResult::null()
        }
        None => NativeCallResult::Error("watch.removePath: invalid handle".into()),
    }
}

/// Close a watcher and release its resources.
///
/// Args: `handle: number`
pub fn watch_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);
    WATCHER_HANDLES.remove(handle);
    NativeCallResult::null()
}
