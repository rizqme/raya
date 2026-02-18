//! Native stream operations for std:stream
//!
//! Provides optimized native implementations for hot-path stream operations:
//! - `forward`: tight channel-to-channel forwarding loop
//! - `collect`: drain channel into an array
//! - `count`: drain channel and count items
//!
//! All channel operations go through the NativeContext trait, so this module
//! has no dependency on engine internals.

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

/// Forward all values from source channel to destination channel.
///
/// Tight native loop: receive from src -> send to dst, until src is closed.
/// Closes dst when done. Returns the number of items forwarded.
pub fn forward(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("stream.forward requires 2 arguments (src, dst)".to_string());
    }

    let src = args[0];
    let dst = args[1];
    let mut count: i64 = 0;

    loop {
        // Try non-blocking receive first
        if let Some(val) = ctx.channel_try_receive(src) {
            if ctx.channel_try_send(dst, val) {
                count += 1;
                continue;
            }
            // dst full, use blocking send
            match ctx.channel_send(dst, val) {
                Ok(true) => { count += 1; }
                _ => break, // dst closed or error
            }
            continue;
        }

        // Nothing available - check if closed
        if ctx.channel_is_closed(src) {
            break;
        }

        // Blocking receive
        match ctx.channel_receive(src) {
            Ok(Some(val)) => {
                match ctx.channel_send(dst, val) {
                    Ok(true) => { count += 1; }
                    _ => break, // dst closed or error
                }
            }
            _ => break, // src closed or error
        }
    }

    ctx.channel_close(dst);
    NativeCallResult::f64(count as f64)
}

/// Drain all values from a channel into an array.
///
/// Reads until the channel is closed and empty. Returns the array.
pub fn collect(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("stream.collect requires 1 argument (channel)".to_string());
    }

    let ch = args[0];
    let mut items: Vec<NativeValue> = Vec::new();

    loop {
        if let Some(val) = ctx.channel_try_receive(ch) {
            items.push(val);
            continue;
        }

        if ctx.channel_is_closed(ch) {
            break;
        }

        match ctx.channel_receive(ch) {
            Ok(Some(val)) => {
                items.push(val);
            }
            _ => break,
        }
    }

    NativeCallResult::Value(ctx.create_array(&items))
}

/// Blocking receive from a channel.
///
/// Blocks the current thread until a value is available or the channel is closed.
/// Returns the value, or null if the channel is closed and empty.
pub fn receive(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("stream.receive requires 1 argument (channel)".to_string());
    }

    let ch = args[0];

    // Try non-blocking first
    if let Some(val) = ctx.channel_try_receive(ch) {
        return NativeCallResult::Value(val);
    }

    if ctx.channel_is_closed(ch) {
        return NativeCallResult::null();
    }

    // Block until value available or channel closed
    match ctx.channel_receive(ch) {
        Ok(Some(val)) => NativeCallResult::Value(val),
        _ => NativeCallResult::null(), // closed or error
    }
}

/// Blocking send to a channel.
///
/// Blocks until the value can be sent (backpressure).
/// Returns true if sent, false if channel is closed.
pub fn send(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("stream.send requires 2 arguments (channel, value)".to_string());
    }

    let ch = args[0];
    let value = args[1];

    match ctx.channel_send(ch, value) {
        Ok(true) => NativeCallResult::bool(true),
        _ => NativeCallResult::bool(false), // closed or error
    }
}

/// Drain a channel and count items.
///
/// Reads until closed and empty. Returns the count.
pub fn count(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("stream.count requires 1 argument (channel)".to_string());
    }

    let ch = args[0];
    let mut count: i64 = 0;

    loop {
        if ctx.channel_try_receive(ch).is_some() {
            count += 1;
            continue;
        }

        if ctx.channel_is_closed(ch) {
            break;
        }

        match ctx.channel_receive(ch) {
            Ok(Some(_)) => { count += 1; }
            _ => break,
        }
    }

    NativeCallResult::f64(count as f64)
}
