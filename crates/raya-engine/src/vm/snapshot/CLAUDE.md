# VM Snapshot

This folder serializes and restores VM state for pause/resume and transfer scenarios. It defines what a snapshot contains and how to validate and reconstruct it.

## What This Folder Owns

- Snapshot file format.
- Segment headers and checksums.
- Serialized heap, task, and value representations.
- Reader/writer logic for snapshot round-trips.

## File Guide

- `format.rs`: headers, segment types, endianness, checksum rules.
- `writer.rs`: snapshot creation.
- `reader.rs`: snapshot loading and verification.
- `heap.rs`: serialized heap structures.
- `task.rs`: serialized task and frame structures.
- `value.rs`: serialized value encoding.

## Start Here When

- Snapshot files are unreadable, invalid, or incompatible.
- Pause/resume state does not reconstruct correctly.
- You need to add a new kind of runtime state to snapshots.

## Read Next

- Runtime state sources: [`../interpreter/CLAUDE.md`](../interpreter/CLAUDE.md), [`../gc/CLAUDE.md`](../gc/CLAUDE.md), [`../scheduler/CLAUDE.md`](../scheduler/CLAUDE.md)
