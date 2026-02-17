//! E2E tests for std:stream module
//!
//! Tests ReadableStream, WritableStream, TransformStream, and pipe.

use super::harness::*;

// ============================================================================
// ReadableStream basics
// ============================================================================

#[test]
fn test_readable_stream_from_array() {
    expect_i32_with_builtins(
        "let stream = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(1);
             ctrl.push(2);
             ctrl.push(3);
             ctrl.close();
         }, 16);
         let result = stream.collect();
         return result.length;",
        3,
    );
}

#[test]
fn test_readable_stream_from_array_values() {
    expect_i32_with_builtins(
        "let stream = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(10);
             ctrl.push(20);
             ctrl.push(30);
             ctrl.close();
         }, 16);
         let a = stream.read();
         let b = stream.read();
         let c = stream.read();
         let d = stream.read();
         let sum = 0;
         if (a != null) { sum = sum + a; }
         if (b != null) { sum = sum + b; }
         if (c != null) { sum = sum + c; }
         // d should be null (stream ended)
         if (d != null) { sum = sum + 999; }
         return sum;",
        60,
    );
}

#[test]
fn test_readable_stream_empty() {
    expect_i32_with_builtins(
        "let stream = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.close();
         }, 1);
         let val = stream.read();
         if (val == null) { return 1; }
         return 0;",
        1,
    );
}

#[test]
fn test_readable_stream_producer() {
    expect_i32_with_builtins(
        "let stream = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(5);
             ctrl.push(10);
             ctrl.push(15);
             ctrl.close();
         }, 16);
         let result = stream.collect();
         let sum = 0;
         let i = 0;
         while (i < result.length) {
             sum = sum + result[i];
             i = i + 1;
         }
         return sum;",
        30,
    );
}

// ============================================================================
// WritableStream basics
// ============================================================================

#[test]
fn test_writable_stream_consumer() {
    // Use array to accumulate values (arrays are heap objects, shared by reference in closures)
    expect_i32_with_builtins(
        "let acc: number[] = [];
         let sink = new WritableStream<number>((ctrl: WritableController<number>): void => {
             let val = ctrl.pull();
             while (val != null) {
                 acc.push(val);
                 val = ctrl.pull();
             }
         }, 16);
         sink.write(10);
         sink.write(20);
         sink.write(30);
         sink.close();
         // Wait for consumer to finish
         __NATIVE_CALL<number>(\"time.sleep\", 50);
         let total = 0;
         let i = 0;
         while (i < acc.length) {
             total = total + acc[i];
             i = i + 1;
         }
         return total;",
        60,
    );
}

// ============================================================================
// Pipe
// ============================================================================

#[test]
fn test_readable_pipe_to_writable() {
    // Use array to accumulate piped values (arrays shared by reference in closures)
    expect_i32_with_builtins(
        "let acc: number[] = [];
         let source = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(1);
             ctrl.push(2);
             ctrl.push(3);
             ctrl.push(4);
             ctrl.push(5);
             ctrl.close();
         }, 16);
         let sink = new WritableStream<number>((ctrl: WritableController<number>): void => {
             let val = ctrl.pull();
             while (val != null) {
                 acc.push(val);
                 val = ctrl.pull();
             }
         }, 16);
         source.pipe(sink);
         // Wait for consumer
         __NATIVE_CALL<number>(\"time.sleep\", 50);
         let total = 0;
         let i = 0;
         while (i < acc.length) {
             total = total + acc[i];
             i = i + 1;
         }
         return total;",
        15,
    );
}

// ============================================================================
// TransformStream
// ============================================================================

#[test]
fn test_transform_stream_map() {
    expect_i32_with_builtins(
        "let source = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(1);
             ctrl.push(2);
             ctrl.push(3);
             ctrl.close();
         }, 16);

         let doubler = new TransformStream<number, number>(
             (input: number, ctrl: TransformController<number>): void => {
                 ctrl.push(input * 2);
             },
             16
         );

         let output = source.pipeThrough<number>(doubler);
         let result = output.collect();
         let sum = 0;
         let i = 0;
         while (i < result.length) {
             sum = sum + result[i];
             i = i + 1;
         }
         return sum;",
        12,  // (1*2 + 2*2 + 3*2) = 12
    );
}

#[test]
fn test_transform_stream_filter() {
    expect_i32_with_builtins(
        "let source = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(1);
             ctrl.push(2);
             ctrl.push(3);
             ctrl.push(4);
             ctrl.push(5);
             ctrl.push(6);
             ctrl.close();
         }, 16);

         let evens = new TransformStream<number, number>(
             (input: number, ctrl: TransformController<number>): void => {
                 if (input % 2 == 0) {
                     ctrl.push(input);
                 }
             },
             16
         );

         let output = source.pipeThrough<number>(evens);
         let result = output.collect();
         return result.length;",
        3,  // 2, 4, 6
    );
}

// ============================================================================
// Pipe (replaces pipeline function)
// ============================================================================

#[test]
fn test_pipe_basic() {
    // Use array to accumulate piped values (arrays shared by reference in closures)
    expect_i32_with_builtins(
        "let acc: number[] = [];
         let source = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(10);
             ctrl.push(20);
             ctrl.push(30);
             ctrl.close();
         }, 16);
         let sink = new WritableStream<number>((ctrl: WritableController<number>): void => {
             let val = ctrl.pull();
             while (val != null) {
                 acc.push(val);
                 val = ctrl.pull();
             }
         }, 16);
         source.pipe(sink);
         // Wait for consumer
         __NATIVE_CALL<number>(\"time.sleep\", 50);
         let total = 0;
         let i = 0;
         while (i < acc.length) {
             total = total + acc[i];
             i = i + 1;
         }
         return total;",
        60,
    );
}

// ============================================================================
// Collect
// ============================================================================

#[test]
fn test_collect_values() {
    expect_i32_with_builtins(
        "let stream = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(5);
             ctrl.push(10);
             ctrl.push(15);
             ctrl.push(20);
             ctrl.close();
         }, 16);
         let arr = stream.collect();
         return arr[0] + arr[1] + arr[2] + arr[3];",
        50,
    );
}

// ============================================================================
// Diagnostic tests to isolate collect() issue
// ============================================================================

#[test]
fn test_diag_array_push_loop() {
    // Test array push in a simple loop
    expect_i32_with_builtins(
        "let arr: number[] = [];
         arr.push(10);
         arr.push(20);
         arr.push(30);
         return arr.length;",
        3,
    );
}

#[test]
fn test_diag_channel_receive_loop() {
    // Test raw channel receive in a loop (no stream classes)
    // Uses null check guards to avoid type errors
    expect_i32_with_builtins(
        "let ch = new Channel<number>(16);
         ch.send(1);
         ch.send(2);
         ch.send(3);
         ch.close();
         let sum = 0;
         let val = ch.tryReceive();
         while (val != null) {
             if (val != null) { sum = sum + val; }
             val = ch.tryReceive();
         }
         return sum;",
        6,
    );
}

#[test]
fn test_diag_channel_collect_manual() {
    // Test manual collect pattern with channel (no stream classes)
    expect_i32_with_builtins(
        "let ch = new Channel<number>(16);
         ch.send(5);
         ch.send(10);
         ch.send(15);
         ch.close();
         let result: number[] = [];
         let val = ch.tryReceive();
         while (val != null) {
             if (val != null) { result.push(val); }
             val = ch.tryReceive();
         }
         return result.length;",
        3,
    );
}

#[test]
fn test_diag_async_channel_collect() {
    // Test channel with async producer + collect pattern
    expect_i32_with_builtins(
        "let ch = new Channel<number>(16);
         async {
             ch.send(1);
             ch.send(2);
             ch.send(3);
             ch.close();
         };
         // Collect from channel
         let result: number[] = [];
         let val = ch.tryReceive();
         if (val == null && !ch.isClosed()) {
             val = ch.receive();
         }
         while (val != null) {
             if (val != null) { result.push(val); }
             val = ch.tryReceive();
             if (val == null && !ch.isClosed()) {
                 val = ch.receive();
             }
         }
         return result.length;",
        3,
    );
}
