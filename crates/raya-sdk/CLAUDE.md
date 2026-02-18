# raya-sdk

SDK for writing Raya native modules and stdlib implementations.

## Overview

This crate provides types and traits needed to write Raya native modules **without** depending on the full `raya-engine`. Both the core stdlib (`raya-stdlib`) and platform-specific stdlib (`raya-stdlib-posix`) depend on this crate.

## Key Types

### NativeValue

Opaque value type for native handlers. Uses a tagged union internally.

```rust
NativeValue::null() / .bool(v) / .i32(v) / .i64(v) / .f64(v)
value.is_null() / .as_bool() / .as_i32() / .as_f64()
```

### NativeContext (trait)

VM context passed to native handlers — provides GC allocation, string/buffer access, channel operations, and scheduler access without engine dependency.

```rust
ctx.read_string(val) -> Result<String>    // Read a GC string
ctx.create_string(&s) -> NativeValue      // Allocate a GC string
ctx.create_buffer(&[u8]) -> NativeValue   // Allocate a GC buffer
ctx.create_array(&[NativeValue]) -> NativeValue
ctx.channel_send(ch, val) / ctx.channel_receive(ch)
```

### NativeCallResult

```rust
pub enum NativeCallResult {
    Value(NativeValue),       // Synchronous return
    Unhandled,                // ID not recognized
    Error(String),            // Runtime error
    Suspend(IoRequest),       // Yield to reactor (non-blocking IO)
}
```

### IoRequest / IoCompletion

For non-blocking IO via the reactor's worker pool:

```rust
pub enum IoRequest {
    BlockingWork { work: Box<dyn FnOnce() -> IoCompletion + Send> },
    ChannelReceive { channel }, ChannelSend { channel, value },
    NetAccept { handle }, NetRead { handle, max_bytes }, NetWrite { handle, data },
    NetConnect { addr }, Sleep { duration_nanos },
}

pub enum IoCompletion {
    Bytes(Vec<u8>), String(String), Primitive(NativeValue),
    StringArray(Vec<String>), Error(String),
}
```

### NativeHandler (trait) / NativeFunctionRegistry

```rust
trait NativeHandler: Send + Sync {
    fn call(&self, ctx: &dyn NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult;
}

// Name-based dispatch for ModuleNativeCall
registry.register("fs.readFile", |ctx, args| { ... });
```

## BlockingWork Pattern

Handlers that do IO return `Suspend(BlockingWork)` so the reactor runs the work on the IO pool, keeping VM workers free:

```rust
fn read_file(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = ctx.read_string(args[0]).unwrap();  // Extract BEFORE closure
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {  // ctx is NOT available here (not Send)
            match std::fs::read(&path) {
                Ok(data) => IoCompletion::Bytes(data),
                Err(e) => IoCompletion::Error(e.to_string()),
            }
        })
    })
}
```

## For AI Assistants

- This crate is the dependency boundary — `raya-stdlib` and `raya-stdlib-posix` depend on this, NOT on `raya-engine`
- `NativeContext` is NOT Send — extract all data from it BEFORE creating BlockingWork closures
- `IoCompletion::StringArray` is for operations like `readDir` that return `string[]`
- `IoRequest::Sleep` exists but currently sleep uses `BlockingWork` with `thread::sleep`
