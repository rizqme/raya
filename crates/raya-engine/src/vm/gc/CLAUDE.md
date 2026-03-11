# VM GC

This folder owns heap allocation and garbage collection. If an object should stay alive, be collected, or be traversed, the logic belongs here.

## What This Folder Owns

- Heap allocation and bookkeeping.
- Object headers and pointer layout.
- Root tracking.
- Collection orchestration and statistics.
- Nursery support for young allocations.
- External root provider hooks for other VM subsystems.

## File Guide

- `collector.rs`: collector entrypoints, stats, and external root provider integration.
- `heap.rs`: heap allocation and storage.
- `nursery.rs`: nursery logic.
- `roots.rs`: root-set tracking.
- `ptr.rs`: `GcPtr` abstraction.
- `header.rs`: object header layout and header/value pointer conversion.

## Start Here When

- Objects are collected too early or never collected.
- GC stats or heap accounting are wrong.
- A subsystem needs to expose additional roots during collection.
- Object layout changes require header or traversal changes.

## Read Next

- Main runtime consumer: [`../interpreter/CLAUDE.md`](../interpreter/CLAUDE.md)
- Scheduler root registration: [`../scheduler/CLAUDE.md`](../scheduler/CLAUDE.md)
- Snapshot interaction: [`../snapshot/CLAUDE.md`](../snapshot/CLAUDE.md)
