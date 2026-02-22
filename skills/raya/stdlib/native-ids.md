# Native ID Ranges and Dispatch

Native functions are identified by 16-bit IDs for fast dispatch.

## ID Allocation

| Range | Module | Count | Location |
|-------|--------|-------|----------|
| 0x0100-0x01FF | String (builtin) | Variable | raya-engine |
| 0x0200-0x02FF | Array (builtin) | Variable | raya-engine |
| 0x0300-0x03FF | Object (builtin) | Variable | raya-engine |
| 0x0400-0x04FF | RegExp (builtin) | Variable | raya-engine |
| 0x0500-0x05FF | Buffer (builtin) | Variable | raya-engine |
| 0x0600-0x06FF | Map (builtin) | Variable | raya-engine |
| 0x0700-0x07FF | Set (builtin) | Variable | raya-engine |
| 0x0D00-0x0E2F | Reflect API | 149+ | raya-engine |
| 0x1000-0x1003 | Logger | 4 | raya-stdlib |
| 0x2000-0x2016 | Math | 23 | raya-stdlib |
| 0x3000-0x30FF | Runtime | Variable | raya-engine |
| 0x4000-0x400B | Crypto | 12 | raya-stdlib |
| 0x5000-0x5004 | Time | 5 | raya-stdlib |
| 0x6000-0x600C | Path | 13 | raya-stdlib |
| 0x8000-0x8005 | Compress | 6 | raya-stdlib |
| 0x9000-0x9026 | URL | 39 | raya-stdlib |

## Dispatch Mechanism

### NativeHandler Trait

```rust
pub trait NativeHandler: Send + Sync {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) 
        -> NativeCallResult;
}
```

### StdNativeHandler

```rust
impl NativeHandler for StdNativeHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) 
        -> NativeCallResult {
        match id {
            0x1000..=0x1003 => self.call_logger(id, args),
            0x2000..=0x2016 => self.call_math(id, args),
            0x4000..=0x400B => crypto::call_crypto_method(ctx, id, args),
            0x5000..=0x5004 => self.call_time(id, args),
            0x6000..=0x600C => path::call_path_method(ctx, id, args),
            0x8000..=0x8005 => compress::call_compress_method(ctx, id, args),
            0x9000..=0x9026 => url::call_url_method(ctx, id, args),
            _ => NativeCallResult::Unhandled,
        }
    }
}
```

## NativeContext

Provides access to VM internals:

```rust
pub struct NativeContext<'a> {
    pub gc: &'a Gc,              // Allocate objects
    pub classes: &'a ClassRegistry,  // Class info
    pub scheduler: &'a Scheduler,    // Task operations
}
```

## NativeValue

Type-safe value wrapper:

```rust
pub enum NativeValue {
    Int(i32),
    Number(f64),
    Bool(bool),
    Null,
    String(GcPtr<String>),
    Object(GcPtr<Object>),
    Array(GcPtr<Array>),
    Buffer(GcPtr<Buffer>),
}

impl NativeValue {
    pub fn as_i32(&self) -> Option<i32> { ... }
    pub fn as_f64(&self) -> Option<f64> { ... }
    pub fn as_string(&self) -> Option<&str> { ... }
    // ...
}
```

## NativeCallResult

```rust
pub enum NativeCallResult {
    Return(NativeValue),
    Suspend(SuspendReason),
    Error(String),
    Unhandled,
}
```

## Adding Native Functions

See [Adding Modules](../development/adding-modules.md) for step-by-step guide.
