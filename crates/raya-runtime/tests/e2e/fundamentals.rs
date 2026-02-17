//! Fundamental capability tests
//!
//! These tests verify that core language features work correctly when composed.
//! Each category builds on the previous one, isolating exactly where things break.
//!
//! If a test in category N fails but all tests in category N-1 pass,
//! the bug is in the feature tested by category N.

use super::harness::*;

// ============================================================================
// Category 1: Method calls on objects with nested field access
// Verify: obj.method() works when method accesses this.field.subfield
// ============================================================================

#[test]
fn test_nested_field_method_direct() {
    // Inner object holds a number, outer object holds inner.
    // Outer method accesses this.inner.val
    expect_i32(
        "class Inner {
             val: number;
             constructor(v: number) { this.val = v; }
             get(): number { return this.val; }
         }
         class Outer {
             inner: Inner;
             constructor(i: Inner) { this.inner = i; }
             getInnerVal(): number { return this.inner.get(); }
         }
         let o = new Outer(new Inner(42));
         return o.getInnerVal();",
        42,
    );
}

#[test]
fn test_method_calls_method_on_field() {
    // obj.process() internally calls this.worker.compute(this.data)
    expect_i32(
        "class Worker {
             id: number;
             constructor(i: number) { this.id = i; }
             compute(x: number): number { return this.id * 10 + x; }
         }
         class Manager {
             worker: Worker;
             data: number;
             constructor(w: Worker, d: number) { this.worker = w; this.data = d; }
             process(): number { return this.worker.compute(this.data); }
         }
         let m = new Manager(new Worker(7), 3);
         return m.process();",
        73,
    );
}

// ============================================================================
// Category 2: Closure captures object, calls method that chains through fields
// Verify: captured_obj.method() works when method accesses this.field.subfield
// ============================================================================

#[test]
fn test_closure_captures_object_calls_nested_method() {
    // Closure captures 'mgr', calls mgr.process() which chains through mgr.worker.compute()
    expect_i32(
        "class Worker {
             id: number;
             constructor(i: number) { this.id = i; }
             compute(x: number): number { return this.id * 10 + x; }
         }
         class Manager {
             worker: Worker;
             data: number;
             constructor(w: Worker, d: number) { this.worker = w; this.data = d; }
             process(): number { return this.worker.compute(this.data); }
         }
         let mgr = new Manager(new Worker(7), 3);
         let f = (): number => mgr.process();
         return f();",
        73,
    );
}

#[test]
fn test_closure_captures_object_calls_method_with_arg() {
    // Closure captures obj and val, calls obj.method(val)
    // The method accesses this.field internally
    expect_i32(
        "class Processor {
             base: number;
             constructor(b: number) { this.base = b; }
             run(x: number): number { return this.base * 10 + x; }
         }
         let obj = new Processor(7);
         let val = 3;
         let f = (): number => obj.run(val);
         return f();",
        73,
    );
}

// ============================================================================
// Category 3: Async block captures object, calls method that chains through fields
// Same as Category 2 but inside async { ... }
// ============================================================================

#[test]
fn test_async_captures_object_calls_nested_method() {
    expect_i32_with_builtins(
        "class Worker {
             id: number;
             constructor(i: number) { this.id = i; }
             compute(x: number): number { return this.id * 10 + x; }
         }
         class Manager {
             worker: Worker;
             data: number;
             constructor(w: Worker, d: number) { this.worker = w; this.data = d; }
             process(): number { return this.worker.compute(this.data); }
         }
         let mgr = new Manager(new Worker(7), 3);
         let t = async { return mgr.process(); };
         return await t;",
        73,
    );
}

#[test]
fn test_async_captures_two_objects_calls_method() {
    // Async block captures obj and val, calls obj.method(val)
    expect_i32_with_builtins(
        "class Processor {
             base: number;
             constructor(b: number) { this.base = b; }
             run(x: number): number { return this.base * 10 + x; }
         }
         let obj = new Processor(7);
         let val = 3;
         let t = async { return obj.run(val); };
         return await t;",
        73,
    );
}

// ============================================================================
// Category 4: Method on captured object calls ANOTHER method on captured object
// Verify: closure calls capturedA.method(capturedB)
// ============================================================================

#[test]
fn test_closure_calls_method_with_captured_object_arg() {
    // Closure captures src and dst, calls src.forward(dst)
    // forward() accesses this.val and calls dst.receive(val)
    expect_i32(
        "class Sink {
             result: number;
             constructor() { this.result = 0; }
             receive(v: number): void { this.result = v; }
             getResult(): number { return this.result; }
         }
         class Source {
             val: number;
             constructor(v: number) { this.val = v; }
             forward(sink: Sink): void { sink.receive(this.val); }
         }
         let src = new Source(42);
         let dst = new Sink();
         let f = (): void => src.forward(dst);
         f();
         return dst.getResult();",
        42,
    );
}

#[test]
fn test_async_calls_method_with_captured_object_arg() {
    // Same but in async block, fire-and-forget, use Channel to get result
    expect_i32_with_builtins(
        "class Source {
             val: number;
             constructor(v: number) { this.val = v; }
             sendTo(ch: Channel<number>): void {
                 ch.send(this.val);
                 ch.close();
             }
         }
         let src = new Source(42);
         let ch = new Channel<number>(1);
         async { src.sendTo(ch); };
         return ch.receive();",
        42,
    );
}

// ============================================================================
// Category 5: Class method spawns async block that calls method on captured obj
// This is the pipeThrough pattern
// ============================================================================

#[test]
fn test_method_spawns_async_calling_method_on_captured() {
    // Class method captures a local (from this.field), spawns async that calls method
    expect_i32_with_builtins(
        "class Worker {
             val: number;
             constructor(v: number) { this.val = v; }
             sendTo(ch: Channel<number>): void {
                 ch.send(this.val);
                 ch.close();
             }
         }
         class Coordinator {
             worker: Worker;
             constructor(w: Worker) { this.worker = w; }
             run(): number {
                 let w = this.worker;
                 let ch = new Channel<number>(1);
                 async { w.sendTo(ch); };
                 return ch.receive();
             }
         }
         let coord = new Coordinator(new Worker(42));
         return coord.run();",
        42,
    );
}

#[test]
fn test_method_spawns_async_calling_method_on_param() {
    // Method receives an object param, spawns async that calls method on it
    expect_i32_with_builtins(
        "class Worker {
             val: number;
             constructor(v: number) { this.val = v; }
             sendTo(ch: Channel<number>): void {
                 ch.send(this.val);
                 ch.close();
             }
         }
         class Coordinator {
             run(w: Worker): number {
                 let ch = new Channel<number>(1);
                 async { w.sendTo(ch); };
                 return ch.receive();
             }
         }
         let coord = new Coordinator();
         return coord.run(new Worker(42));",
        42,
    );
}

// ============================================================================
// Category 6: Calling a method (A.method(B)) from async block where the method
// itself accesses B's fields through B's methods
// This tests the full chain: async -> method call -> nested method call
// ============================================================================

#[test]
fn test_async_method_chain_through_fields() {
    // A.pipe(B) where pipe accesses this.inner and B.inner
    expect_i32_with_builtins(
        "class InnerA {
             val: number;
             constructor(v: number) { this.val = v; }
             get(): number { return this.val; }
         }
         class InnerB {
             result: number;
             constructor() { this.result = 0; }
             set(v: number): void { this.result = v; }
         }
         class A {
             inner: InnerA;
             constructor(ia: InnerA) { this.inner = ia; }
             pipe(b: B): void {
                 let val = this.inner.get();
                 b.inner.set(val);
             }
         }
         class B {
             inner: InnerB;
             constructor(ib: InnerB) { this.inner = ib; }
             getResult(): number { return this.inner.result; }
         }
         let a = new A(new InnerA(42));
         let b = new B(new InnerB());
         let t = async {
             a.pipe(b);
         };
         await t;
         return b.getResult();",
        42,
    );
}

#[test]
fn test_async_in_method_calls_method_on_captured_that_chains() {
    // Exact pipeThrough pattern: class method captures locals from this/param,
    // spawns async, async calls method on captured, that method chains through fields
    expect_i32_with_builtins(
        "class Store {
             val: number;
             constructor(v: number) { this.val = v; }
             get(): number { return this.val; }
         }
         class Reader {
             store: Store;
             constructor(s: Store) { this.store = s; }
             readTo(ch: Channel<number>): void {
                 let v = this.store.get();
                 ch.send(v);
                 ch.close();
             }
         }
         class Pipeline {
             reader: Reader;
             constructor(r: Reader) { this.reader = r; }
             run(): number {
                 let r = this.reader;
                 let ch = new Channel<number>(1);
                 async { r.readTo(ch); };
                 return ch.receive();
             }
         }
         let pipeline = new Pipeline(new Reader(new Store(99)));
         return pipeline.run();",
        99,
    );
}

// ============================================================================
// Category 7: Channel as a field of a class — fundamental Channel operations
// This tests whether Channel objects work correctly when stored as fields
// ============================================================================

#[test]
fn test_channel_in_class_field_basic() {
    // Channel stored as a class field, send/receive through the field
    expect_i32_with_builtins(
        "class Holder {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
             sendVal(v: number): void { this.ch.send(v); }
             recvVal(): number { return this.ch.receive(); }
         }
         let h = new Holder();
         h.sendVal(42);
         return h.recvVal();",
        42,
    );
}

#[test]
fn test_channel_field_accessed_from_closure() {
    // Channel field loaded into local, closure captures the local, uses it
    expect_i32_with_builtins(
        "class Holder {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
             getChannel(): Channel<number> { return this.ch; }
         }
         let h = new Holder();
         let ch = h.getChannel();
         let f = (): number => {
             ch.send(42);
             return ch.receive();
         };
         return f();",
        42,
    );
}

#[test]
fn test_channel_field_accessed_from_async() {
    // Channel field loaded into local, async block captures it
    expect_i32_with_builtins(
        "class Holder {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
             getChannel(): Channel<number> { return this.ch; }
         }
         let h = new Holder();
         let ch = h.getChannel();
         async { ch.send(42); };
         return ch.receive();",
        42,
    );
}

#[test]
fn test_channel_field_method_accesses_channel_field() {
    // A class has a Channel field. A method on the class calls channel methods.
    // Then call that method from an async block via a captured reference.
    expect_i32_with_builtins(
        "class Pipe {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
             push(v: number): void { this.ch.send(v); }
             pull(): number { return this.ch.receive(); }
         }
         let p = new Pipe();
         async { p.push(42); };
         return p.pull();",
        42,
    );
}

// ============================================================================
// Category 8: Two objects with channels — one reads, forwards to other
// This is the simplified pipe() pattern
// ============================================================================

#[test]
fn test_pipe_pattern_direct() {
    // src.pipe(dst) reads from src.ch and sends to dst.ch — called directly
    expect_i32_with_builtins(
        "class Src {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
             pipe(dst: Dst): void {
                 let val = this.ch.receive();
                 dst.ch.send(val);
             }
         }
         class Dst {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
         }
         let src = new Src();
         let dst = new Dst();
         src.ch.send(42);
         src.pipe(dst);
         return dst.ch.receive();",
        42,
    );
}

#[test]
fn test_pipe_pattern_from_async() {
    // Same pipe pattern but called from async block — the pipeThrough pattern
    expect_i32_with_builtins(
        "class Src {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
             pipe(dst: Dst): void {
                 let val = this.ch.receive();
                 dst.ch.send(val);
             }
         }
         class Dst {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
         }
         let src = new Src();
         let dst = new Dst();
         src.ch.send(42);
         let s = src;
         let d = dst;
         async { s.pipe(d); };
         return dst.ch.receive();",
        42,
    );
}

#[test]
fn test_pipe_pattern_from_method_async() {
    // pipe called from async block inside a class method
    expect_i32_with_builtins(
        "class Src {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
             pipe(dst: Dst): void {
                 let val = this.ch.receive();
                 dst.ch.send(val);
             }
         }
         class Dst {
             ch: Channel<number>;
             constructor() { this.ch = new Channel<number>(1); }
         }
         class Orchestrator {
             src: Src;
             constructor(s: Src) { this.src = s; }
             run(dst: Dst): number {
                 let s = this.src;
                 async { s.pipe(dst); };
                 return dst.ch.receive();
             }
         }
         let src = new Src();
         src.ch.send(42);
         let dst = new Dst();
         let orch = new Orchestrator(src);
         return orch.run(dst);",
        42,
    );
}

// ============================================================================
// Category 9: GENERIC classes with Channel<T> fields
// This isolates whether monomorphization of Channel<T> inside generic classes works
// ============================================================================

#[test]
fn test_generic_class_with_channel_field() {
    // Generic class holds Channel<T>, method sends/receives
    expect_i32_with_builtins(
        "class Holder<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
             push(v: T): void { this.ch.send(v); }
             pull(): T { return this.ch.receive(); }
         }
         let h = new Holder<number>(1);
         h.push(42);
         return h.pull();",
        42,
    );
}

#[test]
fn test_generic_class_channel_field_from_closure() {
    // Capture a generic class instance, call method that uses Channel<T>
    expect_i32_with_builtins(
        "class Holder<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
             push(v: T): void { this.ch.send(v); }
             pull(): T { return this.ch.receive(); }
         }
         let h = new Holder<number>(1);
         h.push(42);
         let f = (): number => h.pull();
         return f();",
        42,
    );
}

#[test]
fn test_generic_class_channel_field_from_async() {
    // Capture generic class instance in async block, call method using Channel<T>
    expect_i32_with_builtins(
        "class Holder<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
             push(v: T): void { this.ch.send(v); }
             pull(): T { return this.ch.receive(); }
         }
         let h = new Holder<number>(1);
         async { h.push(42); };
         return h.pull();",
        42,
    );
}

// ============================================================================
// Category 10: Generic pipe pattern — two generic classes with channels
// This mimics ReadableStream.pipe(WritableStream) with generics
// ============================================================================

#[test]
fn test_generic_pipe_direct() {
    // Generic Src<T>.pipe(GenericDst<T>) called directly
    expect_i32_with_builtins(
        "class GSrc<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
             pipe(dst: GDst<T>): void {
                 let val = this.ch.receive();
                 dst.ch.send(val);
             }
         }
         class GDst<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
         }
         let src = new GSrc<number>(1);
         let dst = new GDst<number>(1);
         src.ch.send(42);
         src.pipe(dst);
         return dst.ch.receive();",
        42,
    );
}

#[test]
fn test_generic_pipe_from_async() {
    // Generic pipe called from async block
    expect_i32_with_builtins(
        "class GSrc<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
             pipe(dst: GDst<T>): void {
                 let val = this.ch.receive();
                 dst.ch.send(val);
             }
         }
         class GDst<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
         }
         let src = new GSrc<number>(1);
         let dst = new GDst<number>(1);
         src.ch.send(42);
         let s = src;
         let d = dst;
         async { s.pipe(d); };
         return dst.ch.receive();",
        42,
    );
}

#[test]
fn test_generic_pipe_from_method_async() {
    // Generic pipe called from async in a class method — full pipeThrough pattern
    expect_i32_with_builtins(
        "class GSrc<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
             pipe(dst: GDst<T>): void {
                 let val = this.ch.receive();
                 dst.ch.send(val);
             }
         }
         class GDst<T> {
             ch: Channel<T>;
             constructor(cap: number) { this.ch = new Channel<T>(cap); }
         }
         class Orch<T> {
             src: GSrc<T>;
             constructor(s: GSrc<T>) { this.src = s; }
             run(dst: GDst<T>): T {
                 let s = this.src;
                 async { s.pipe(dst); };
                 return dst.ch.receive();
             }
         }
         let src = new GSrc<number>(1);
         src.ch.send(42);
         let dst = new GDst<number>(1);
         let orch = new Orch<number>(src);
         return orch.run(dst);",
        42,
    );
}

// ============================================================================
// Category 11: Generic method with additional type parameter
// pipeThrough<O> introduces a new type parameter on top of class <T>
// ============================================================================

#[test]
fn test_method_with_extra_type_param() {
    // Class<T> has method<O> that creates Channel<O>
    expect_i32_with_builtins(
        "class Box<T> {
             val: T;
             constructor(v: T) { this.val = v; }
             transform<O>(f: (v: T) => O): O {
                 return f(this.val);
             }
         }
         let b = new Box<number>(7);
         return b.transform<number>((v: number): number => v * 6);",
        42,
    );
}

// ============================================================================
// Category 12: Std:stream specific test — minimal pipeThrough reproduction
// Uses the actual stream module imports
// ============================================================================

#[test]
fn test_stream_pipe_method_from_async_minimal() {
    // Uses the actual stream module with a minimal pipeThrough-like pattern
    expect_i32_with_builtins(
        "let source = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(42);
             ctrl.close();
         }, 4);

         let sink = new WritableStream<number>((ctrl: WritableController<number>): void => {
             let val = ctrl.pull();
             while (val != null) {
                 val = ctrl.pull();
             }
         }, 4);

         // This is what pipeThrough does internally:
         let readable = source;
         let w = sink;
         async { readable.pipe(w); };
         // Give time for async pipe to complete
         __NATIVE_CALL<number>(\"time.sleep\", 100);
         return 42;",
        42,
    );
}

// ============================================================================
// Category 13: TransformStream-specific tests
// Incrementally reproduce the failing pipeThrough pattern
// ============================================================================

#[test]
fn test_transform_stream_construct() {
    // Just construct a TransformStream — no pipe
    expect_i32_with_builtins(
        "let ts = new TransformStream<number, number>(
             (input: number, ctrl: TransformController<number>): void => {
                 ctrl.push(input * 2);
             },
             16
         );
         return 1;",
        1,
    );
}

#[test]
fn test_transform_stream_get_writable() {
    // Construct TransformStream, get writable
    expect_i32_with_builtins(
        "let ts = new TransformStream<number, number>(
             (input: number, ctrl: TransformController<number>): void => {
                 ctrl.push(input * 2);
             },
             16
         );
         let w = ts.getWritable();
         return 1;",
        1,
    );
}

#[test]
fn test_transform_stream_get_readable() {
    // Construct TransformStream, get readable
    expect_i32_with_builtins(
        "let ts = new TransformStream<number, number>(
             (input: number, ctrl: TransformController<number>): void => {
                 ctrl.push(input * 2);
             },
             16
         );
         let r = ts.getReadable();
         return 1;",
        1,
    );
}

#[test]
fn test_pipethrough_manual_inline() {
    // Manually inline what pipeThrough does, step by step
    // This is source.pipeThrough(transform) inlined:
    expect_i32_with_builtins(
        "let source = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(10);
             ctrl.push(20);
             ctrl.close();
         }, 16);

         let transform = new TransformStream<number, number>(
             (input: number, ctrl: TransformController<number>): void => {
                 ctrl.push(input * 2);
             },
             16
         );

         // Manually inline pipeThrough:
         let readable = source;
         let w = transform.getWritable();
         async {
             readable.pipe(w);
         };
         let output = transform.getReadable();
         let result = output.collect();
         let sum = 0;
         let i = 0;
         while (i < result.length()) {
             sum = sum + result[i];
             i = i + 1;
         }
         return sum;",
        60,  // (10*2 + 20*2) = 60
    );
}

#[test]
fn test_pipethrough_method_call() {
    // Call pipeThrough as a method — exactly what the failing test does
    expect_i32_with_builtins(
        "let source = new ReadableStream<number>((ctrl: ReadableController<number>): void => {
             ctrl.push(10);
             ctrl.push(20);
             ctrl.close();
         }, 16);

         let transform = new TransformStream<number, number>(
             (input: number, ctrl: TransformController<number>): void => {
                 ctrl.push(input * 2);
             },
             16
         );

         let output = source.pipeThrough<number>(transform);
         let result = output.collect();
         let sum = 0;
         let i = 0;
         while (i < result.length()) {
             sum = sum + result[i];
             i = i + 1;
         }
         return sum;",
        60,
    );
}
