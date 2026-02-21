//! Debug state coordination between debugger (parent VM) and debuggee (child interpreter).
//!
//! This module provides the shared state that enables the debugger to control
//! execution of the debuggee. The key mechanism is condvar ping-pong: the parent
//! VM blocks while the child runs, and the child blocks while the parent inspects.

use crate::compiler::Module;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Condvar, Mutex};
use parking_lot::RwLock;

/// Shared between debugger (parent VM) and debuggee (child interpreter thread).
pub struct DebugState {
    /// Fast check — false = zero overhead in interpreter hot path
    pub active: AtomicBool,

    /// Breakpoints: func_id → set of bytecode offsets
    pub breakpoints: RwLock<FxHashMap<usize, FxHashSet<u32>>>,

    /// Current step mode (set by debugger, checked by interpreter)
    pub step_mode: Mutex<StepMode>,

    /// Info about the current pause (written by interpreter, read by debugger)
    pub pause_info: Mutex<Option<PauseInfo>>,

    /// Line at last pause (for step-over "line changed" detection)
    pub pause_line: AtomicU32,

    /// Frame depth at last pause (for step-over/out depth comparison)
    pub pause_depth: AtomicU32,

    /// Condvar signaling between interpreter and debugger
    pub phase_lock: Mutex<DebugPhase>,
    pub phase_changed: Condvar,

    /// Break at entry flag (break before first instruction)
    pub break_at_entry: AtomicBool,

    /// Next breakpoint ID counter
    pub next_bp_id: AtomicU32,

    /// Breakpoint registry: bp_id → entry (for listing/removal)
    pub bp_registry: RwLock<FxHashMap<u32, BreakpointEntry>>,

    /// Source text cache: file path → source string (for getSource)
    pub source_cache: RwLock<FxHashMap<String, String>>,
}

/// Current phase of the debug session.
pub enum DebugPhase {
    /// Child is running, parent is blocked waiting
    Running,
    /// Child is paused, parent can inspect
    Paused,
    /// Child finished execution (result value raw bits)
    Completed(i64),
    /// Child errored
    Failed(String),
}

/// Information about the current pause point.
pub struct PauseInfo {
    pub func_id: usize,
    pub bytecode_offset: u32,
    pub source_file: String,
    pub line: u32,
    pub column: u32,
    pub reason: PauseReason,
    pub function_name: String,
}

/// Why execution was paused.
pub enum PauseReason {
    /// Hit a breakpoint (with its ID)
    Breakpoint(u32),
    /// Step completed
    Step,
    /// Hit a `debugger;` statement
    DebuggerStatement,
    /// Break at entry point
    Entry,
}

/// Stepping mode — set by the debugger, checked by the interpreter at each instruction.
pub enum StepMode {
    /// Only break at breakpoints
    None,
    /// Step over: same or lower depth + line changed
    Over { target_depth: usize, start_line: u32 },
    /// Step into: any depth + line changed
    Into { start_line: u32 },
    /// Step out: depth < target
    Out { target_depth: usize },
}

/// Registered breakpoint entry.
pub struct BreakpointEntry {
    pub id: u32,
    pub file: String,
    pub line: u32,
    pub func_id: usize,
    pub bytecode_offset: u32,
    pub enabled: bool,
    pub condition: Option<String>,
    pub hit_count: u32,
}

impl Default for DebugState {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugState {
    /// Create a new debug state (inactive by default).
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            breakpoints: RwLock::new(FxHashMap::default()),
            step_mode: Mutex::new(StepMode::None),
            pause_info: Mutex::new(None),
            pause_line: AtomicU32::new(0),
            pause_depth: AtomicU32::new(0),
            phase_lock: Mutex::new(DebugPhase::Running),
            phase_changed: Condvar::new(),
            break_at_entry: AtomicBool::new(false),
            next_bp_id: AtomicU32::new(1),
            bp_registry: RwLock::new(FxHashMap::default()),
            source_cache: RwLock::new(FxHashMap::default()),
        }
    }

    /// Called by interpreter at each instruction. Must be fast.
    ///
    /// Returns `Some(reason)` if execution should pause, `None` to continue.
    pub fn should_break(
        &self,
        func_id: usize,
        offset: u32,
        frame_depth: usize,
        current_line: u32,
    ) -> Option<PauseReason> {
        // 1. Check breakpoints
        {
            let bps = self.breakpoints.read();
            if let Some(offsets) = bps.get(&func_id) {
                if offsets.contains(&offset) {
                    // Find bp_id for this offset
                    if let Some(bp) = self.find_bp(func_id, offset) {
                        if bp.enabled {
                            return Some(PauseReason::Breakpoint(bp.id));
                        }
                    }
                }
            }
        }

        // 2. Check step mode
        let step = self.step_mode.lock().unwrap();
        match *step {
            StepMode::None => None,
            StepMode::Over { target_depth, start_line } => {
                if frame_depth <= target_depth && current_line != start_line && current_line != 0 {
                    Some(PauseReason::Step)
                } else {
                    None
                }
            }
            StepMode::Into { start_line } => {
                if current_line != start_line && current_line != 0 {
                    Some(PauseReason::Step)
                } else {
                    None
                }
            }
            StepMode::Out { target_depth } => {
                if frame_depth < target_depth {
                    Some(PauseReason::Step)
                } else {
                    None
                }
            }
        }
    }

    /// Interpreter calls this on breakpoint hit — blocks until the debugger resumes.
    pub fn signal_pause(&self, info: PauseInfo) {
        // Store pause metadata for debugger to read
        self.pause_line.store(info.line, Ordering::Release);
        self.pause_depth.store(info.func_id as u32, Ordering::Release);
        *self.pause_info.lock().unwrap() = Some(info);

        // Signal pause and wait for resume
        let mut phase = self.phase_lock.lock().unwrap();
        *phase = DebugPhase::Paused;
        self.phase_changed.notify_all();

        // Block until debugger resumes us
        while matches!(*phase, DebugPhase::Paused) {
            phase = self.phase_changed.wait(phase).unwrap();
        }
    }

    /// Debugger calls this — blocks until the child pauses or completes.
    ///
    /// Returns a reference-like description of the current phase. The caller
    /// should inspect `phase_lock` after this returns.
    pub fn wait_for_pause(&self) -> DebugPhaseSnapshot {
        let mut phase = self.phase_lock.lock().unwrap();
        while matches!(*phase, DebugPhase::Running) {
            phase = self.phase_changed.wait(phase).unwrap();
        }
        // Snapshot the phase
        match &*phase {
            DebugPhase::Paused => DebugPhaseSnapshot::Paused,
            DebugPhase::Completed(v) => DebugPhaseSnapshot::Completed(*v),
            DebugPhase::Failed(msg) => DebugPhaseSnapshot::Failed(msg.clone()),
            DebugPhase::Running => unreachable!(),
        }
    }

    /// Debugger calls this to resume the child interpreter.
    pub fn signal_resume(&self, mode: StepMode) {
        *self.step_mode.lock().unwrap() = mode;
        let mut phase = self.phase_lock.lock().unwrap();
        *phase = DebugPhase::Running;
        self.phase_changed.notify_all();
    }

    /// Signal that execution completed (called by interpreter on task finish).
    pub fn signal_completed(&self, result_bits: i64) {
        let mut phase = self.phase_lock.lock().unwrap();
        *phase = DebugPhase::Completed(result_bits);
        self.phase_changed.notify_all();
    }

    /// Signal that execution failed (called by interpreter on task error).
    pub fn signal_failed(&self, message: String) {
        let mut phase = self.phase_lock.lock().unwrap();
        *phase = DebugPhase::Failed(message);
        self.phase_changed.notify_all();
    }

    /// Resolve source file:line → (func_id, bytecode_offset) via LineEntry table.
    pub fn resolve_breakpoint(
        &self,
        module: &Module,
        file: &str,
        target_line: u32,
    ) -> Result<(usize, u32), String> {
        let debug_info = module.debug_info.as_ref()
            .ok_or("Module compiled without sourcemap — recompile with --sourcemap")?;

        let file_idx = debug_info.source_files.iter()
            .position(|f| f.ends_with(file) || f == file)
            .ok_or_else(|| format!("Source file '{}' not found in debug info", file))?;

        let mut best: Option<(usize, u32, u32)> = None; // (func_id, offset, actual_line)

        for (func_id, func_dbg) in debug_info.functions.iter().enumerate() {
            if func_dbg.source_file_index != file_idx as u32 {
                continue;
            }

            for entry in &func_dbg.line_table {
                if entry.line >= target_line {
                    match &best {
                        None => best = Some((func_id, entry.bytecode_offset, entry.line)),
                        Some((_, _, bl)) if entry.line < *bl => {
                            best = Some((func_id, entry.bytecode_offset, entry.line));
                        }
                        _ => {}
                    }
                    break; // line_table sorted by offset within function
                }
            }
        }

        best.map(|(f, o, _)| (f, o))
            .ok_or_else(|| format!("No executable code at line {}", target_line))
    }

    /// Add a breakpoint. Returns the breakpoint ID.
    pub fn add_breakpoint(
        &self,
        func_id: usize,
        offset: u32,
        file: String,
        line: u32,
    ) -> u32 {
        let bp_id = self.next_bp_id.fetch_add(1, Ordering::Relaxed);

        // Add to fast-lookup map
        self.breakpoints.write()
            .entry(func_id)
            .or_default()
            .insert(offset);

        // Add to registry
        self.bp_registry.write().insert(bp_id, BreakpointEntry {
            id: bp_id,
            file,
            line,
            func_id,
            bytecode_offset: offset,
            enabled: true,
            condition: None,
            hit_count: 0,
        });

        bp_id
    }

    /// Remove a breakpoint by ID.
    pub fn remove_breakpoint(&self, bp_id: u32) -> bool {
        let mut registry = self.bp_registry.write();
        if let Some(entry) = registry.remove(&bp_id) {
            // Remove from fast-lookup map
            let mut bps = self.breakpoints.write();
            if let Some(offsets) = bps.get_mut(&entry.func_id) {
                offsets.remove(&entry.bytecode_offset);
                if offsets.is_empty() {
                    bps.remove(&entry.func_id);
                }
            }
            true
        } else {
            false
        }
    }

    /// Increment hit count for a breakpoint.
    pub fn increment_hit_count(&self, bp_id: u32) {
        let mut registry = self.bp_registry.write();
        if let Some(entry) = registry.get_mut(&bp_id) {
            entry.hit_count += 1;
        }
    }

    /// Find a breakpoint entry by func_id and offset.
    fn find_bp(&self, func_id: usize, offset: u32) -> Option<BreakpointSnapshot> {
        let registry = self.bp_registry.read();
        for entry in registry.values() {
            if entry.func_id == func_id && entry.bytecode_offset == offset {
                return Some(BreakpointSnapshot {
                    id: entry.id,
                    enabled: entry.enabled,
                });
            }
        }
        None
    }
}

/// Snapshot of a breakpoint for fast checks (avoids holding the lock).
struct BreakpointSnapshot {
    id: u32,
    enabled: bool,
}

/// Snapshot of the debug phase (owned, so no borrow issues).
pub enum DebugPhaseSnapshot {
    Paused,
    Completed(i64),
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_debug_state_new() {
        let state = DebugState::new();
        assert!(!state.active.load(Ordering::Relaxed));
        assert!(state.breakpoints.read().is_empty());
        assert!(state.pause_info.lock().unwrap().is_none());
    }

    #[test]
    fn test_add_remove_breakpoint() {
        let state = DebugState::new();

        let bp_id = state.add_breakpoint(0, 10, "test.raya".into(), 5);
        assert_eq!(bp_id, 1);

        // Verify it's in the fast-lookup map
        assert!(state.breakpoints.read().get(&0).unwrap().contains(&10));

        // Verify it's in the registry
        assert!(state.bp_registry.read().contains_key(&bp_id));

        // Remove it
        assert!(state.remove_breakpoint(bp_id));
        assert!(state.breakpoints.read().get(&0).is_none());
        assert!(!state.bp_registry.read().contains_key(&bp_id));
    }

    #[test]
    fn test_should_break_on_breakpoint() {
        let state = DebugState::new();
        state.add_breakpoint(0, 10, "test.raya".into(), 5);

        // Should break at the breakpoint
        let result = state.should_break(0, 10, 0, 5);
        assert!(matches!(result, Some(PauseReason::Breakpoint(1))));

        // Should not break at a different offset
        let result = state.should_break(0, 20, 0, 6);
        assert!(result.is_none());
    }

    #[test]
    fn test_step_over_mode() {
        let state = DebugState::new();
        *state.step_mode.lock().unwrap() = StepMode::Over {
            target_depth: 1,
            start_line: 5,
        };

        // Same line — no break
        assert!(state.should_break(0, 0, 1, 5).is_none());

        // Different line, same depth — break
        assert!(matches!(
            state.should_break(0, 0, 1, 6),
            Some(PauseReason::Step)
        ));

        // Different line, deeper — no break
        assert!(state.should_break(0, 0, 2, 6).is_none());
    }

    #[test]
    fn test_step_into_mode() {
        let state = DebugState::new();
        *state.step_mode.lock().unwrap() = StepMode::Into { start_line: 5 };

        // Same line — no break
        assert!(state.should_break(0, 0, 0, 5).is_none());

        // Different line — break regardless of depth
        assert!(matches!(
            state.should_break(0, 0, 5, 6),
            Some(PauseReason::Step)
        ));
    }

    #[test]
    fn test_step_out_mode() {
        let state = DebugState::new();
        *state.step_mode.lock().unwrap() = StepMode::Out { target_depth: 2 };

        // Same depth — no break
        assert!(state.should_break(0, 0, 2, 5).is_none());

        // Deeper — no break
        assert!(state.should_break(0, 0, 3, 6).is_none());

        // Shallower — break
        assert!(matches!(
            state.should_break(0, 0, 1, 7),
            Some(PauseReason::Step)
        ));
    }

    #[test]
    fn test_condvar_ping_pong() {
        let state = Arc::new(DebugState::new());
        state.active.store(true, Ordering::Release);

        let state_clone = state.clone();

        // Spawn "interpreter" thread that signals pause
        let handle = std::thread::spawn(move || {
            let info = PauseInfo {
                func_id: 0,
                bytecode_offset: 10,
                source_file: "test.raya".into(),
                line: 5,
                column: 1,
                reason: PauseReason::Breakpoint(1),
                function_name: "main".into(),
            };
            state_clone.signal_pause(info);
            // After resume, continue to completion
            state_clone.signal_completed(0);
        });

        // "Debugger" thread waits for pause
        let phase = state.wait_for_pause();
        assert!(matches!(phase, DebugPhaseSnapshot::Paused));

        // Verify pause info
        {
            let info = state.pause_info.lock().unwrap();
            let info = info.as_ref().unwrap();
            assert_eq!(info.line, 5);
            assert_eq!(info.source_file, "test.raya");
        }

        // Resume
        state.signal_resume(StepMode::None);

        // Wait for completion
        let phase = state.wait_for_pause();
        assert!(matches!(phase, DebugPhaseSnapshot::Completed(0)));

        handle.join().unwrap();
    }
}
