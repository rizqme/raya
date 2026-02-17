//! Native stream operations for std:stream
//!
//! Provides optimized native implementations for hot-path stream operations:
//! - `forward`: tight channel-to-channel forwarding loop
//! - `collect`: drain channel into an array
//! - `count`: drain channel and count items
//!
//! Channel objects are passed as their `channelId` field (field 0 of the
//! Channel<T> Raya object), which is a GC pointer to a ChannelObject.

use raya_engine::vm::{
    NativeCallResult, NativeContext, NativeValue,
    array_allocate, object_get_field,
};
use raya_engine::vm::object::ChannelObject;

/// Extract a ChannelObject reference from a NativeValue.
///
/// Accepts either:
/// - A direct channel pointer (from channelId field)
/// - A Channel<T> object (extracts field 0 = channelId)
fn extract_channel(val: &NativeValue) -> Result<&ChannelObject, String> {
    if !val.is_ptr() {
        return Err("Expected channel, got non-pointer".to_string());
    }

    // Try direct ChannelObject pointer first
    let inner_val = val.into_value();
    if let Some(ptr) = unsafe { inner_val.as_ptr::<ChannelObject>() } {
        return Ok(unsafe { &*ptr.as_ptr() });
    }

    // Try as Object (Channel<T> class) — field 0 is channelId
    match object_get_field(*val, 0) {
        Ok(field_val) => {
            if !field_val.is_ptr() {
                return Err("Channel object field 0 is not a pointer".to_string());
            }
            let ptr = unsafe { field_val.into_value().as_ptr::<ChannelObject>() }
                .ok_or_else(|| "Channel field 0 is not a ChannelObject".to_string())?;
            Ok(unsafe { &*ptr.as_ptr() })
        }
        Err(e) => Err(format!("Cannot extract channel: {}", e)),
    }
}

/// Forward all values from source channel to destination channel.
///
/// Tight native loop: receive from src -> send to dst, until src is closed.
/// Closes dst when done. Returns the number of items forwarded.
pub fn forward(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("stream.forward requires 2 arguments (src, dst)".to_string());
    }

    let src = match extract_channel(&args[0]) {
        Ok(ch) => ch,
        Err(e) => return NativeCallResult::Error(format!("stream.forward src: {}", e)),
    };

    let dst = match extract_channel(&args[1]) {
        Ok(ch) => ch,
        Err(e) => return NativeCallResult::Error(format!("stream.forward dst: {}", e)),
    };

    let mut count: i64 = 0;

    loop {
        // Try non-blocking receive first
        if let Some(val) = src.try_receive() {
            if dst.try_send(val) {
                count += 1;
                continue;
            }
            // dst full, use blocking send
            match dst.send(val) {
                Ok(()) => { count += 1; }
                Err(_) => break, // dst closed
            }
            continue;
        }

        // Nothing available - check if closed
        if src.is_closed() {
            break;
        }

        // Blocking receive
        match src.receive() {
            Ok(val) => {
                match dst.send(val) {
                    Ok(()) => { count += 1; }
                    Err(_) => break, // dst closed
                }
            }
            Err(_) => break, // src closed
        }
    }

    dst.close();
    NativeCallResult::f64(count as f64)
}

/// Drain all values from a channel into an array.
///
/// Reads until the channel is closed and empty. Returns the array.
pub fn collect(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("stream.collect requires 1 argument (channel)".to_string());
    }

    let ch = match extract_channel(&args[0]) {
        Ok(ch) => ch,
        Err(e) => return NativeCallResult::Error(format!("stream.collect: {}", e)),
    };

    let mut items: Vec<NativeValue> = Vec::new();

    loop {
        if let Some(val) = ch.try_receive() {
            items.push(NativeValue::from_value(val));
            continue;
        }

        if ch.is_closed() {
            break;
        }

        match ch.receive() {
            Ok(val) => {
                items.push(NativeValue::from_value(val));
            }
            Err(_) => break,
        }
    }

    NativeCallResult::Value(array_allocate(ctx, &items))
}

/// Blocking receive from a channel.
///
/// Blocks the current thread until a value is available or the channel is closed.
/// Returns the value, or null if the channel is closed and empty.
/// Unlike Channel.receive() in Raya (which uses TRY_RECEIVE and is non-blocking),
/// this uses the OS-level blocking receive via condvar.
pub fn receive(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("stream.receive requires 1 argument (channel)".to_string());
    }

    let ch = match extract_channel(&args[0]) {
        Ok(ch) => ch,
        Err(e) => return NativeCallResult::Error(format!("stream.receive: {}", e)),
    };

    // Try non-blocking first
    if let Some(val) = ch.try_receive() {
        return NativeCallResult::Value(NativeValue::from_value(val));
    }

    if ch.is_closed() {
        return NativeCallResult::null();
    }

    // Block until value available or channel closed
    match ch.receive() {
        Ok(val) => NativeCallResult::Value(NativeValue::from_value(val)),
        Err(_) => NativeCallResult::null(), // closed
    }
}

/// Blocking send to a channel.
///
/// Blocks until the value can be sent (backpressure).
/// Returns true if sent, false if channel is closed.
pub fn send(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("stream.send requires 2 arguments (channel, value)".to_string());
    }

    let ch = match extract_channel(&args[0]) {
        Ok(ch) => ch,
        Err(e) => return NativeCallResult::Error(format!("stream.send: {}", e)),
    };

    let value = args[1].into_value();

    match ch.send(value) {
        Ok(()) => NativeCallResult::bool(true),
        Err(_) => NativeCallResult::bool(false), // closed
    }
}

/// Drain a channel and count items.
///
/// Reads until closed and empty. Returns the count.
pub fn count(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("stream.count requires 1 argument (channel)".to_string());
    }

    let ch = match extract_channel(&args[0]) {
        Ok(ch) => ch,
        Err(e) => return NativeCallResult::Error(format!("stream.count: {}", e)),
    };

    let mut count: i64 = 0;

    loop {
        if ch.try_receive().is_some() {
            count += 1;
            continue;
        }

        if ch.is_closed() {
            break;
        }

        match ch.receive() {
            Ok(_) => { count += 1; }
            Err(_) => break,
        }
    }

    NativeCallResult::f64(count as f64)
}
