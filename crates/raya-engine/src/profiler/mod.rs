//! Sampling-based profiler for the Raya VM.
//!
//! Zero-cost when disabled (single `AtomicBool` check in hot path).
//! Piggybacks on existing backward-jump safepoints and preemption checks.
//!
//! # Output formats
//!
//! - **cpuprofile**: Chrome DevTools JSON (viewable in Chrome, VS Code, speedscope.app)
//! - **flamegraph**: Brendan Gregg folded stacks (for flamegraph.pl / speedscope)

pub mod output;

use crate::compiler::Module;
use crate::vm::scheduler::Task;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Profiling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileMode {
    /// Sample only running tasks (CPU time).
    Cpu,
    /// Sample all tasks including suspended (wall-clock time).
    WallClock,
}

/// Output format for the profile data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Chrome DevTools `.cpuprofile` JSON.
    CpuProfile,
    /// Brendan Gregg folded stacks (one line per stack trace).
    Flamegraph,
}

/// Profiler configuration.
#[derive(Debug, Clone)]
pub struct ProfileConfig {
    /// Profiling mode.
    pub mode: ProfileMode,
    /// Sampling interval in microseconds (default: 10 000 = 100 Hz).
    pub interval_us: u64,
    /// Maximum stack depth to capture per sample (default: 128).
    pub max_depth: usize,
    /// Output format.
    pub format: OutputFormat,
    /// Output file path (if `None`, caller decides).
    pub output_path: Option<String>,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            mode: ProfileMode::Cpu,
            interval_us: 10_000,
            max_depth: 128,
            format: OutputFormat::CpuProfile,
            output_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Sample types
// ---------------------------------------------------------------------------

/// A raw frame captured during sampling — minimal work in hot path.
#[derive(Debug, Clone, Copy)]
pub struct RawFrame {
    /// Function index in the module.
    pub func_id: u32,
    /// Bytecode offset within the function at sample time.
    pub bytecode_offset: u32,
}

/// A single stack sample captured at a point in time.
#[derive(Debug, Clone)]
pub struct StackSample {
    /// Microseconds since profiling started.
    pub timestamp_us: u64,
    /// Task that was sampled.
    pub task_id: u64,
    /// Stack frames (bottom to top: outermost caller → leaf).
    pub frames: Vec<RawFrame>,
}

// ---------------------------------------------------------------------------
// Resolved types (after source mapping)
// ---------------------------------------------------------------------------

/// A frame resolved to source location via `DebugInfo`.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ResolvedFrame {
    pub function_name: String,
    pub source_file: String,
    pub line_number: u32,
    pub column_number: u32,
}

/// A sample with resolved source-level frames.
#[derive(Debug, Clone)]
pub struct ResolvedSample {
    pub timestamp_us: u64,
    pub task_id: u64,
    pub frames: Vec<ResolvedFrame>,
}

/// Aggregated profiling data returned by [`Profiler::stop`].
#[derive(Debug)]
pub struct ProfileData {
    pub samples: Vec<StackSample>,
    pub start_time_us: u64,
    pub end_time_us: u64,
}

/// Resolved profiling data ready for output.
#[derive(Debug)]
pub struct ResolvedProfileData {
    pub samples: Vec<ResolvedSample>,
    pub start_time_us: u64,
    pub end_time_us: u64,
}

// ---------------------------------------------------------------------------
// Profiler
// ---------------------------------------------------------------------------

/// Sampling-based profiler. Shared via `Arc` between VM, interpreter, and scheduler.
pub struct Profiler {
    /// Fast check — single atomic load in hot path.
    enabled: AtomicBool,
    /// Configuration (immutable after creation).
    config: ProfileConfig,
    /// Monotonic start time.
    start_time: Instant,
    /// Last sample timestamp in microseconds (for rate limiting).
    last_sample_time: AtomicU64,
    /// Sample buffer (lock-free sender).
    pub(crate) tx: crossbeam::channel::Sender<StackSample>,
    /// Sample buffer (receiver — drained by `stop()`).
    rx: crossbeam::channel::Receiver<StackSample>,
}

impl Profiler {
    /// Create a new profiler with the given configuration.
    ///
    /// The profiler is created in the **disabled** state; call [`start`] to begin sampling.
    pub fn new(config: ProfileConfig) -> Self {
        let (tx, rx) = crossbeam::channel::bounded(65_536);
        Self {
            enabled: AtomicBool::new(false),
            config,
            start_time: Instant::now(),
            last_sample_time: AtomicU64::new(0),
            tx,
            rx,
        }
    }

    /// Start profiling.
    pub fn start(&self) {
        self.enabled.store(true, Ordering::Release);
    }

    /// Stop profiling and drain all captured samples.
    pub fn stop(&self) -> ProfileData {
        self.enabled.store(false, Ordering::Release);
        let end_time_us = self.elapsed_us();

        let mut samples = Vec::new();
        while let Ok(sample) = self.rx.try_recv() {
            samples.push(sample);
        }

        ProfileData {
            samples,
            start_time_us: 0,
            end_time_us,
        }
    }

    /// Whether profiling is currently active.
    #[inline(always)]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Access the profiler configuration.
    pub fn config(&self) -> &ProfileConfig {
        &self.config
    }

    // ── Hot-path sampling ──────────────────────────────────────────────

    /// Called from the interpreter hot path. Checks the rate limit and captures
    /// a stack sample if enough time has elapsed.
    ///
    /// Designed to be as cheap as possible when it is not time to sample:
    /// one atomic load (`enabled`) + one atomic load (`last_sample_time`) +
    /// a timestamp comparison.
    #[inline(always)]
    pub fn maybe_sample(
        &self,
        task: &Arc<Task>,
        current_func_id: usize,
        current_ip: usize,
    ) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let now_us = self.elapsed_us();
        let last = self.last_sample_time.load(Ordering::Relaxed);
        if now_us.wrapping_sub(last) < self.config.interval_us {
            return;
        }
        // CAS to avoid duplicate samples from concurrent workers
        if self
            .last_sample_time
            .compare_exchange(last, now_us, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        self.capture_stack(task, current_func_id, current_ip, now_us);
    }

    /// Capture the current call stack from a task.
    fn capture_stack(
        &self,
        task: &Arc<Task>,
        current_func_id: usize,
        current_ip: usize,
        timestamp_us: u64,
    ) {
        let max = self.config.max_depth;
        let mut frames = Vec::with_capacity(max.min(32));

        // 1. Saved execution frames (caller chain, bottom to top)
        let exec_frames = task.get_execution_frames();
        for frame in exec_frames.iter() {
            if frames.len() >= max {
                break;
            }
            frames.push(RawFrame {
                func_id: frame.func_id as u32,
                bytecode_offset: frame.ip as u32,
            });
        }

        // 2. Current frame (the leaf — where execution is right now)
        if frames.len() < max {
            frames.push(RawFrame {
                func_id: current_func_id as u32,
                bytecode_offset: current_ip as u32,
            });
        }

        // Non-blocking send — drop sample if buffer is full
        let _ = self.tx.try_send(StackSample {
            timestamp_us,
            task_id: task.id().as_u64(),
            frames,
        });
    }

    #[inline(always)]
    fn elapsed_us(&self) -> u64 {
        self.start_time.elapsed().as_micros() as u64
    }
}

// ---------------------------------------------------------------------------
// Source resolution
// ---------------------------------------------------------------------------

impl ProfileData {
    /// Resolve raw bytecode frames to source locations using the module's [`DebugInfo`].
    pub fn resolve(&self, module: &Module) -> ResolvedProfileData {
        let resolved_samples = self
            .samples
            .iter()
            .map(|sample| {
                let frames = sample
                    .frames
                    .iter()
                    .map(|raw| resolve_frame(module, raw.func_id, raw.bytecode_offset))
                    .collect();
                ResolvedSample {
                    timestamp_us: sample.timestamp_us,
                    task_id: sample.task_id,
                    frames,
                }
            })
            .collect();

        ResolvedProfileData {
            samples: resolved_samples,
            start_time_us: self.start_time_us,
            end_time_us: self.end_time_us,
        }
    }
}

/// Resolve a single raw frame to a source location.
fn resolve_frame(module: &Module, func_id: u32, bytecode_offset: u32) -> ResolvedFrame {
    let func_name = module
        .functions
        .get(func_id as usize)
        .map(|f| f.name.clone())
        .unwrap_or_else(|| format!("func_{}", func_id));

    if let Some(ref debug_info) = module.debug_info {
        if let Some(func_debug) = debug_info.functions.get(func_id as usize) {
            let source_file = debug_info
                .source_files
                .get(func_debug.source_file_index as usize)
                .cloned()
                .unwrap_or_default();

            let (line, col) = find_line_for_offset(
                &func_debug.line_table,
                bytecode_offset,
                func_debug.start_line,
                func_debug.start_column,
            );

            return ResolvedFrame {
                function_name: func_name,
                source_file,
                line_number: line,
                column_number: col,
            };
        }
    }

    // Fallback: no debug info
    ResolvedFrame {
        function_name: func_name,
        source_file: String::new(),
        line_number: 0,
        column_number: 0,
    }
}

/// Binary-search the line table for the largest offset ≤ `target`.
fn find_line_for_offset(
    table: &[crate::compiler::bytecode::module::LineEntry],
    offset: u32,
    default_line: u32,
    default_col: u32,
) -> (u32, u32) {
    if table.is_empty() {
        return (default_line, default_col);
    }
    match table.binary_search_by_key(&offset, |e| e.bytecode_offset) {
        Ok(i) => (table[i].line, table[i].column),
        Err(0) => (default_line, default_col),
        Err(i) => (table[i - 1].line, table[i - 1].column),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiler_disabled_by_default() {
        let profiler = Profiler::new(ProfileConfig::default());
        assert!(!profiler.is_enabled());
    }

    #[test]
    fn test_profiler_start_stop_lifecycle() {
        let profiler = Profiler::new(ProfileConfig::default());
        profiler.start();
        assert!(profiler.is_enabled());
        let data = profiler.stop();
        assert!(!profiler.is_enabled());
        assert_eq!(data.start_time_us, 0);
    }

    #[test]
    fn test_profiler_stop_drains_samples() {
        let profiler = Profiler::new(ProfileConfig::default());
        profiler.start();

        // Manually push a sample
        let _ = profiler.tx.try_send(StackSample {
            timestamp_us: 1000,
            task_id: 1,
            frames: vec![RawFrame {
                func_id: 0,
                bytecode_offset: 10,
            }],
        });

        let data = profiler.stop();
        assert_eq!(data.samples.len(), 1);
        assert_eq!(data.samples[0].frames.len(), 1);
        assert_eq!(data.samples[0].frames[0].func_id, 0);
    }

    #[test]
    fn test_resolve_frame_without_debug_info() {
        let mut module = Module::new("test".to_string());
        module.functions.push(crate::compiler::Function {
            name: "my_func".to_string(),
            param_count: 0,
            local_count: 0,
            code: vec![],
        });

        let resolved = resolve_frame(&module, 0, 10);
        assert_eq!(resolved.function_name, "my_func");
        assert_eq!(resolved.source_file, "");
        assert_eq!(resolved.line_number, 0);
    }

    #[test]
    fn test_resolve_frame_with_debug_info() {
        use crate::compiler::bytecode::module::{DebugInfo, FunctionDebugInfo, LineEntry};

        let mut module = Module::new("test".to_string());
        module.functions.push(crate::compiler::Function {
            name: "fibonacci".to_string(),
            param_count: 1,
            local_count: 1,
            code: vec![],
        });

        let mut debug_info = DebugInfo::new();
        debug_info.add_source_file("app.raya".to_string());
        let mut func_debug = FunctionDebugInfo::new(0, 10, 1, 20, 1);
        func_debug.line_table.push(LineEntry {
            bytecode_offset: 0,
            line: 10,
            column: 1,
        });
        func_debug.line_table.push(LineEntry {
            bytecode_offset: 5,
            line: 12,
            column: 5,
        });
        func_debug.line_table.push(LineEntry {
            bytecode_offset: 15,
            line: 18,
            column: 1,
        });
        debug_info.functions.push(func_debug);
        module.debug_info = Some(debug_info);
        module.flags |= crate::compiler::bytecode::module::flags::HAS_DEBUG_INFO;

        // Exact match
        let resolved = resolve_frame(&module, 0, 5);
        assert_eq!(resolved.function_name, "fibonacci");
        assert_eq!(resolved.source_file, "app.raya");
        assert_eq!(resolved.line_number, 12);

        // Between entries — should pick previous
        let resolved = resolve_frame(&module, 0, 8);
        assert_eq!(resolved.line_number, 12);

        // Before first entry — use function default
        let resolved = resolve_frame(&module, 0, 0);
        assert_eq!(resolved.line_number, 10);
    }

    #[test]
    fn test_find_line_for_offset_empty_table() {
        let (line, col) = find_line_for_offset(&[], 5, 42, 1);
        assert_eq!(line, 42);
        assert_eq!(col, 1);
    }

    #[test]
    fn test_config_defaults() {
        let config = ProfileConfig::default();
        assert_eq!(config.mode, ProfileMode::Cpu);
        assert_eq!(config.interval_us, 10_000);
        assert_eq!(config.max_depth, 128);
        assert_eq!(config.format, OutputFormat::CpuProfile);
        assert!(config.output_path.is_none());
    }
}
