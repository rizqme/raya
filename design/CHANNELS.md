# Channel Design for Raya

**Status:** Design Document
**Date:** 2026-01-27

---

## 1. Overview

Channels provide a type-safe mechanism for communication between concurrent Tasks in Raya. Inspired by Go's channels and Rust's `std::sync::mpsc`, Raya channels enable safe data transfer between Tasks without explicit locking.

### Design Goals

1. **Type Safety**: Channels are generic (`Channel<T>`) and only accept values of type `T`
2. **Synchronous Blocking**: Send and receive operations block the current Task directly (no `await` needed)
3. **Unbounded by Default**: No capacity limit, sends never block
4. **Optional Bounded**: Capacity-limited channels for backpressure
5. **Closeable**: Channels can be closed to signal completion
6. **Go-like API**: Familiar syntax for developers coming from Go

---

## 2. Language Syntax

### 2.1 Channel Creation

```typescript
// Unbounded channel (default)
let ch = new Channel<number>();

// Bounded channel with capacity
let bounded = new Channel<string>(10);
```

### 2.2 Sending Values

```typescript
// Blocking send (may block if bounded channel is full)
// NOTE: No await needed - blocks the current Task directly
ch.send(42);

// Try send (non-blocking, returns boolean)
let sent: boolean = ch.trySend(42);
```

### 2.3 Receiving Values

```typescript
// Blocking receive (suspends Task until value available or channel closed)
// NOTE: No await needed - blocks the current Task directly
let value: number | null = ch.receive();  // null if channel closed

// Try receive (non-blocking)
let result: number | null = ch.tryReceive();  // null if empty or closed
```

### 2.4 Closing Channels

```typescript
ch.close();

// Check if closed
let closed: boolean = ch.isClosed();
```

### 2.5 Channel Length and Capacity

```typescript
let len: number = ch.length();      // Current buffered items
let cap: number = ch.capacity();    // Max capacity (0 for unbounded)
```

---

## 3. Type System

### 3.1 Channel Type

```typescript
// Built-in generic type
class Channel<T> {
    constructor(capacity?: number);

    // Blocking operations (suspend Task, no await needed)
    send(value: T): void;
    receive(): T | null;

    // Non-blocking operations
    trySend(value: T): boolean;
    tryReceive(): T | null;

    close(): void;
    isClosed(): boolean;

    length(): number;
    capacity(): number;
}
```

### 3.2 Type Checking Rules

1. **Generic Instantiation**: `Channel<T>` must be instantiated with a concrete type
2. **Send Type Check**: `ch.send(v)` requires `v: T`
3. **Receive Type**: `ch.receive()` returns `T | null` directly (blocking, no `await`)
4. **Monomorphization**: Each channel instantiation generates specialized code

### 3.3 Blocking Semantics

Channel operations block the current Task **without** using `await`:

```typescript
// This is CORRECT - no await needed
let value = ch.receive();  // Blocks Task until value available

// This is WRONG - receive() is not async
let value = await ch.receive();  // ERROR: receive() doesn't return Task<T>
```

The blocking is cooperative - the Task yields to the scheduler while waiting, allowing other Tasks to run. This is similar to Go's channel semantics.

### 3.4 Discriminated Union Pattern for Receive

For complex types, use discriminated unions to distinguish closed channel from null values:

```typescript
type ReceiveResult<T> =
    | { status: "value"; value: T }
    | { status: "closed" };

// Wrap receive to return discriminated union
function safeReceive<T>(ch: Channel<T>): ReceiveResult<T> {
    let value = ch.receive();  // Blocking, no await
    if (ch.isClosed() && value === null) {
        return { status: "closed" };
    }
    return { status: "value", value: value as T };
}
```

---

## 4. Bytecode Instructions

### 4.1 Opcode Definitions

Using available opcodes in the 0xED-0xEF range:

| Opcode | Hex | Description | Stack Effect |
|--------|-----|-------------|--------------|
| `NewChannel` | 0xED | Create channel with capacity | `[capacity] -> [channel]` |
| `ChanSend` | 0xEE | Send value to channel | `[channel, value] -> []` |
| `ChanRecv` | 0xEF | Receive from channel (blocking) | `[channel] -> [value]` |

Additional opcodes (can use 0xFD-0xFE if needed):

| Opcode | Hex | Description | Stack Effect |
|--------|-----|-------------|--------------|
| `ChanClose` | 0xFD | Close a channel | `[channel] -> []` |
| `ChanTryRecv` | 0xFE | Non-blocking receive | `[channel] -> [value_or_null]` |

### 4.2 Opcode Semantics

#### NewChannel (0xED)
```
Stack: [capacity: i32] -> [channel: Channel<T>]

- capacity > 0: bounded channel
- capacity = 0: unbounded channel
- Allocates channel object on heap
- Returns channel reference
```

#### ChanSend (0xEE)
```
Stack: [channel: Channel<T>, value: T] -> []

- If bounded and full: suspend current Task until space available
- If unbounded: always succeeds immediately
- If channel closed: throw exception
- Wakes one waiting receiver (if any)
```

#### ChanRecv (0xEF)
```
Stack: [channel: Channel<T>] -> [value: T | null]

- If value available: return value immediately
- If empty and open: suspend Task until value or close
- If empty and closed: return null
- Task suspension: add to channel's wait queue
```

#### ChanClose (0xFD)
```
Stack: [channel: Channel<T>] -> []

- Mark channel as closed
- Wake all waiting receivers (they get null)
- Wake all waiting senders (they throw exception)
- Idempotent: closing twice is no-op
```

#### ChanTryRecv (0xFE)
```
Stack: [channel: Channel<T>] -> [value: T | null]

- Non-blocking: never suspends
- If value available: return value
- If empty (open or closed): return null
```

---

## 5. VM Implementation

### 5.1 Channel Data Structure

```rust
pub struct Channel {
    /// Unique channel ID
    id: ChannelId,

    /// Type info for values (for GC and type checking)
    value_type: TypeId,

    /// Capacity (0 = unbounded)
    capacity: usize,

    /// Buffered values (FIFO queue)
    buffer: VecDeque<Value>,

    /// Tasks waiting to receive
    receivers: VecDeque<TaskId>,

    /// Tasks waiting to send (only for bounded channels)
    senders: VecDeque<(TaskId, Value)>,

    /// Channel state
    closed: AtomicBool,

    /// Mutex for internal synchronization
    lock: Mutex<()>,
}
```

### 5.2 Channel Operations Implementation

#### Send Operation
```rust
impl Channel {
    pub fn send(&self, value: Value, task: &Task, scheduler: &Scheduler) -> SendResult {
        let _guard = self.lock.lock();

        if self.closed.load(Ordering::SeqCst) {
            return SendResult::ChannelClosed;
        }

        // Bounded channel full check
        if self.capacity > 0 && self.buffer.len() >= self.capacity {
            // Add to sender wait queue
            self.senders.push_back((task.id(), value));
            return SendResult::WouldBlock;
        }

        // Try to hand off directly to waiting receiver
        if let Some(receiver_id) = self.receivers.pop_front() {
            scheduler.wake_task_with_value(receiver_id, value);
            return SendResult::Sent;
        }

        // Buffer the value
        self.buffer.push_back(value);
        SendResult::Sent
    }
}

pub enum SendResult {
    Sent,
    WouldBlock,
    ChannelClosed,
}
```

#### Receive Operation
```rust
impl Channel {
    pub fn receive(&self, task: &Task, scheduler: &Scheduler) -> ReceiveResult {
        let _guard = self.lock.lock();

        // Try to get from buffer
        if let Some(value) = self.buffer.pop_front() {
            // Wake a waiting sender if bounded
            if let Some((sender_id, send_value)) = self.senders.pop_front() {
                self.buffer.push_back(send_value);
                scheduler.wake_task(sender_id);
            }
            return ReceiveResult::Value(value);
        }

        // Check if closed
        if self.closed.load(Ordering::SeqCst) {
            return ReceiveResult::Closed;
        }

        // Add to receiver wait queue
        self.receivers.push_back(task.id());
        ReceiveResult::WouldBlock
    }
}

pub enum ReceiveResult {
    Value(Value),
    WouldBlock,
    Closed,
}
```

### 5.3 Task Suspension/Resumption

When a channel operation would block:

1. **ChanRecv blocks**:
   - Add TaskId to channel's `receivers` queue
   - Set Task status to `BLOCKED_CHANNEL`
   - Return control to scheduler

2. **ChanSend blocks** (bounded channel full):
   - Add (TaskId, Value) to channel's `senders` queue
   - Set Task status to `BLOCKED_CHANNEL`
   - Return control to scheduler

3. **Wake on value available**:
   - Pop TaskId from receivers queue
   - Set wake value on Task
   - Set Task status to `READY`
   - Add to scheduler's run queue

### 5.4 Channel Registry

```rust
pub struct ChannelRegistry {
    channels: RwLock<HashMap<ChannelId, Arc<Channel>>>,
    next_id: AtomicU64,
}

impl ChannelRegistry {
    pub fn create(&self, capacity: usize, value_type: TypeId) -> ChannelId {
        let id = ChannelId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let channel = Arc::new(Channel::new(id, capacity, value_type));
        self.channels.write().insert(id, channel);
        id
    }

    pub fn get(&self, id: ChannelId) -> Option<Arc<Channel>> {
        self.channels.read().get(&id).cloned()
    }

    pub fn close(&self, id: ChannelId) {
        if let Some(channel) = self.get(id) {
            channel.close();
        }
    }
}
```

---

## 6. Garbage Collection

### 6.1 Channel GC Integration

Channels are GC-managed objects:

1. **Channel reference**: Traced like other heap objects
2. **Buffered values**: All values in buffer are traced
3. **Pending sends**: Values in sender queue are traced
4. **Channel lifetime**: Channel lives while any reference exists

### 6.2 GC Roots from Channels

```rust
impl Channel {
    pub fn trace(&self, tracer: &mut GcTracer) {
        // Trace all buffered values
        for value in &self.buffer {
            tracer.trace_value(value);
        }

        // Trace pending send values
        for (_, value) in &self.senders {
            tracer.trace_value(value);
        }
    }
}
```

### 6.3 Channel Cleanup on GC

When a channel becomes unreachable:

1. Wake all waiting receivers with `null`
2. Wake all waiting senders with exception
3. Clear buffer
4. Remove from registry

---

## 7. Select Statement (Future)

A future enhancement could add Go-style select for multiple channels:

```typescript
// Future syntax (not implemented yet)
select {
    case value = ch1.receive():
        console.log("Received from ch1:", value);
    case value = ch2.receive():
        console.log("Received from ch2:", value);
    case ch3.send(42):
        console.log("Sent to ch3");
    default:
        console.log("No channel ready");
}
```

This would require:
- New `Select` opcode
- Channel operation registration
- Fair selection algorithm

---

## 8. Compilation Examples

### 8.1 Simple Send/Receive

```typescript
// Source
let ch = new Channel<number>();
ch.send(42);
let value = ch.receive();  // Blocking, no await needed
```

```
// Bytecode
CONST_I32 0           // capacity = 0 (unbounded)
NEW_CHANNEL           // Create channel, push reference
STORE_LOCAL 0         // ch = channel

LOAD_LOCAL 0          // Push channel
CONST_I32 42          // Push value
CHAN_SEND             // Send value

LOAD_LOCAL 0          // Push channel
CHAN_RECV             // Receive (may suspend)
STORE_LOCAL 1         // value = received
```

### 8.2 Producer-Consumer Pattern

```typescript
// Source
async function producer(ch: Channel<number>): Task<void> {
    for (let i = 0; i < 10; i = i + 1) {
        ch.send(i);  // Blocking send
    }
    ch.close();
}

async function consumer(ch: Channel<number>): Task<number> {
    let sum = 0;
    let value = ch.receive();  // Blocking receive, no await
    while (value !== null) {
        sum = sum + value;
        value = ch.receive();
    }
    return sum;
}

async function main(): Task<number> {
    let ch = new Channel<number>();
    let p = producer(ch);
    let c = consumer(ch);
    await p;
    return await c;
}
```

---

## 9. Implementation Roadmap

### Phase 1: Core Implementation

1. **Add Opcodes** (0.5 day)
   - Add `NewChannel`, `ChanSend`, `ChanRecv`, `ChanClose`, `ChanTryRecv` to opcode.rs
   - Update bytecode verifier
   - Update emit.rs for opcode sizes

2. **Channel Data Structure** (1 day)
   - Implement `Channel` struct in `raya-core/src/vm/`
   - Implement `ChannelRegistry`
   - Add channel operations

3. **Task Suspension** (1 day)
   - Add `BLOCKED_CHANNEL` task status
   - Implement suspend/resume for channel operations
   - Integrate with scheduler

4. **VM Interpreter** (0.5 day)
   - Implement opcode handlers
   - Handle blocking and non-blocking paths

### Phase 2: Language Integration

5. **Type Checker** (0.5 day)
   - Add `Channel<T>` as built-in generic type
   - Type check send/receive operations

6. **IR Instructions** (0.5 day)
   - Add IR instructions for channel operations
   - Update pretty printer

7. **Lowering** (0.5 day)
   - Recognize `Channel` constructor and methods
   - Emit appropriate IR

8. **Codegen** (0.5 day)
   - Generate bytecode for channel operations

### Phase 3: Testing & Polish

9. **Tests** (1 day)
   - Unit tests for channel operations
   - Integration tests for producer-consumer
   - Concurrency stress tests

10. **Documentation** (0.5 day)
    - Update LANG.md
    - Update OPCODE.md

**Total Estimated Effort:** 7 days

---

## 10. Error Handling

### 10.1 Send on Closed Channel

```typescript
try {
    ch.send(42);
} catch (e) {
    console.log("Channel closed");
}
```

Throws: `"ChannelClosed: cannot send on closed channel"`

### 10.2 Null Check on Receive

```typescript
let value = ch.receive();  // Blocking, no await
if (value === null) {
    console.log("Channel closed");
} else {
    console.log("Received:", value);
}
```

### 10.3 Exception Propagation

If a Task is blocked on a channel and an exception occurs:
1. Task is removed from channel's wait queue
2. Exception propagates normally
3. Channel remains open for other Tasks

---

## 11. Memory Model

### 11.1 Happens-Before Relationships

1. **Send happens-before receive**: A send to a channel happens-before the corresponding receive completes
2. **Close happens-before receive of null**: Closing a channel happens-before any receive that returns null due to closure
3. **Sequential consistency**: All channel operations appear to occur in some global total order

### 11.2 Thread Safety

All channel operations are thread-safe:
- Internal mutex protects buffer and wait queues
- Atomic flag for closed state
- Lock-free fast path for unbounded send when receiver waiting

---

## 12. Comparison with Go Channels

| Feature | Raya | Go |
|---------|------|-----|
| Syntax | `new Channel<T>()` | `make(chan T)` |
| Generics | Type parameter `<T>` | Built-in |
| Default | Unbounded | Unbounded (capacity 0) |
| Close | `ch.close()` | `close(ch)` |
| Receive | `ch.receive()` (blocking) | `<-ch` |
| Send | `ch.send(v)` (blocking) | `ch <- v` |
| Select | Future feature | Built-in `select` |
| Range | Not supported | `for v := range ch` |

---

## 13. Test Cases

### 13.1 Basic Operations
- `test_channel_send_receive` - Simple send and receive
- `test_channel_multiple_values` - Multiple sends, multiple receives
- `test_channel_close` - Close channel, receive returns null
- `test_channel_send_on_closed` - Send on closed throws

### 13.2 Blocking Behavior
- `test_channel_receive_blocks` - Receive blocks until send
- `test_channel_bounded_send_blocks` - Send blocks when bounded full
- `test_channel_wakeup_on_send` - Blocked receiver wakes on send

### 13.3 Concurrency
- `test_channel_producer_consumer` - Classic pattern
- `test_channel_multiple_producers` - Multiple senders
- `test_channel_multiple_consumers` - Multiple receivers
- `test_channel_stress` - High throughput stress test

### 13.4 Edge Cases
- `test_channel_empty_close` - Close empty channel
- `test_channel_double_close` - Close twice is no-op
- `test_channel_gc_collection` - Channel collected when unreachable

---

## 14. References

- [Go Channel Specification](https://golang.org/ref/spec#Channel_types)
- [Rust std::sync::mpsc](https://doc.rust-lang.org/std/sync/mpsc/)
- [CSP (Communicating Sequential Processes)](https://en.wikipedia.org/wiki/Communicating_sequential_processes)

---

**Last Updated:** 2026-01-27
**Status:** Design Document - Ready for Implementation
