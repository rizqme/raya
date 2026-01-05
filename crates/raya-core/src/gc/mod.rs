//! Garbage collection system
//!
//! This module provides a mark-sweep garbage collector for the Raya VM.
//!
//! # Architecture
//!
//! The GC system consists of several components:
//!
//! - **Value**: Tagged pointer representation (8 bytes)
//! - **GcHeader**: Metadata for each allocated object (16 bytes)
//! - **GcPtr**: Smart pointer to GC-managed objects
//! - **Heap**: Memory allocator with GC integration
//! - **RootSet**: Tracking of GC roots (stack, globals)
//! - **GarbageCollector**: Mark-sweep collection algorithm
//!
//! # Memory Layout
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ GcHeader (16 bytes, 8-byte aligned)     │
//! │  - marked: bool                         │
//! │  - type_id: TypeId                      │
//! ├─────────────────────────────────────────┤  ← GcPtr points here
//! │ Object data (variable size)             │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```no_run
//! use raya_core::gc::GarbageCollector;
//!
//! let mut gc = GarbageCollector::default();
//!
//! // Allocate objects
//! let ptr1 = gc.allocate(42i32);
//! let ptr2 = gc.allocate(String::from("hello"));
//!
//! // Run collection
//! gc.collect();
//! ```

mod collector;
mod header;
mod heap;
mod ptr;
mod roots;

// Re-export public types
pub use collector::{GarbageCollector, GcStats, HeapStats};
pub use header::GcHeader;
pub use heap::Heap;
pub use ptr::GcPtr;
pub use roots::RootSet;

// Legacy Gc type for backwards compatibility
// TODO: Remove this once all code is updated to use GarbageCollector
#[deprecated(note = "Use GarbageCollector instead")]
/// Alias for GarbageCollector
pub type Gc = GarbageCollector;
