//! Unified Reactor — single control thread for scheduling + event loop
//!
//! The reactor merges the scheduler and event loop into one thread running a single
//! loop. It dispatches tasks to VM workers and handles async IO completions via a
//! shared Tokio runtime, while also managing timers, channel waiters, and
//! preemption.

use crate::vm::abi::native_to_value;
use crate::vm::interpreter::{
    ExecutionResult, Interpreter, PromiseHandle, PromiseMicrotask, SharedVmState,
};
use crate::vm::object::{Buffer, ChannelObject, Class, Object, RayaString};
use crate::vm::scheduler::{SuspendReason, Task, TaskId, TaskState};
use crate::vm::value::Value;
use crossbeam::channel::{self, Receiver, Sender, TryRecvError, TrySendError};
use raya_sdk::{IoCompletion, IoRequest};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tokio::runtime::{Builder, Handle, Runtime};

const VM_WORKER_RECV_TIMEOUT: Duration = Duration::from_millis(5);

// ============================================================================
// Channel message types
// ============================================================================

/// Work sent from reactor to a VM worker
struct VmWork {
    task: Arc<Task>,
}

/// Result sent from a VM worker back to the reactor
struct VmResult {
    task: Arc<Task>,
    result: ExecutionResult,
}

/// IO submission from a VM worker (when NativeCallResult::Suspend)
pub struct IoSubmission {
    pub task_id: TaskId,
    pub request: IoRequest,
}

/// IO completion sent from an IO worker back to the reactor
struct IoPoolCompletion {
    task_id: TaskId,
    result: IoCompletion,
}

// ============================================================================
// Reactor-internal state types
// ============================================================================

/// A task waiting on a channel operation
struct ChannelWaiter {
    task_id: TaskId,
    channel_handle: u64,
    is_send: bool,
    value: Option<Value>,
}

/// Timer entry for sleeping tasks (min-heap by wake_at)
struct SleepEntry {
    wake_at: Instant,
    task: Arc<Task>,
}

impl Ord for SleepEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.wake_at.cmp(&self.wake_at) // reverse for min-heap
    }
}

impl PartialOrd for SleepEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for SleepEntry {
    fn eq(&self, other: &Self) -> bool {
        self.wake_at == other.wake_at
    }
}

impl Eq for SleepEntry {}

// ============================================================================
// Reactor
// ============================================================================

/// Unified reactor that runs scheduling + event loop in a single thread.
pub struct Reactor {
    /// VM worker pool size
    vm_worker_count: usize,
    /// Maximum Tokio blocking worker threads (used for io-style work like fs/process).
    io_worker_count: usize,

    /// Shared VM state
    shared_state: Arc<SharedVmState>,

    /// Channel: reactor → VM workers (bounded for backpressure)
    vm_task_tx: Option<Sender<VmWork>>,
    /// Channel: VM workers → reactor (execution results)
    vm_result_rx: Option<Receiver<VmResult>>,
    /// Channel: VM workers → reactor (IO submissions from NativeCallResult::Suspend)
    io_submit_rx: Option<Receiver<IoSubmission>>,
    /// Sender cloned into VM workers for IO submissions
    io_submit_tx: Option<Sender<IoSubmission>>,

    /// Channel: Tokio IO completions → reactor.
    io_completion_rx: Option<Receiver<IoPoolCompletion>>,

    /// Tokio runtime for asynchronous IO execution
    tokio_runtime: Option<Runtime>,

    /// Shutdown signal
    shutdown: Arc<AtomicBool>,

    /// Thread handles
    reactor_handle: Option<JoinHandle<()>>,
    vm_worker_handles: Vec<JoinHandle<()>>,

    /// Whether the reactor has been started
    started: bool,
}

impl Reactor {
    /// Create a new reactor (not yet started).
    pub fn new(
        vm_worker_count: usize,
        io_worker_count: usize,
        shared_state: Arc<SharedVmState>,
    ) -> Self {
        Self {
            vm_worker_count,
            io_worker_count,
            shared_state,
            vm_task_tx: None,
            vm_result_rx: None,
            io_submit_rx: None,
            io_submit_tx: None,
            io_completion_rx: None,
            tokio_runtime: None,
            shutdown: Arc::new(AtomicBool::new(false)),
            reactor_handle: None,
            vm_worker_handles: Vec::new(),
            started: false,
        }
    }

    /// Get the IO submit sender (cloned into Interpreter for NativeCallResult::Suspend)
    pub fn io_submit_tx(&self) -> Option<&Sender<IoSubmission>> {
        self.io_submit_tx.as_ref()
    }

    /// Start the reactor thread and all worker pools.
    pub fn start(&mut self) {
        if self.started {
            return;
        }

        // Create channels
        let (vm_task_tx, vm_task_rx) = channel::bounded::<VmWork>(self.vm_worker_count);
        let (vm_result_tx, vm_result_rx) = channel::unbounded::<VmResult>();
        let (io_submit_tx, io_submit_rx) = channel::unbounded::<IoSubmission>();
        let (io_completion_tx, io_completion_rx) = channel::unbounded::<IoPoolCompletion>();

        self.vm_task_tx = Some(vm_task_tx.clone());
        self.vm_result_rx = Some(vm_result_rx.clone());
        self.io_submit_rx = Some(io_submit_rx.clone());
        self.io_submit_tx = Some(io_submit_tx.clone());
        self.io_completion_rx = Some(io_completion_rx.clone());

        let tokio_runtime = match Builder::new_multi_thread()
            .thread_name("raya-tokio-io")
            .thread_stack_size(2 * 1024 * 1024)
            .worker_threads(self.vm_worker_count.max(1))
            .max_blocking_threads(self.io_worker_count.max(1))
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(e) => {
                eprintln!(
                    "[runtime] failed to initialize tokio runtime, using default runtime: {e}"
                );
                Runtime::new().expect("tokio runtime construction failed")
            }
        };
        let tokio_handle = tokio_runtime.handle().clone();
        self.tokio_runtime = Some(tokio_runtime);

        // Store io_submit_tx in shared state so Interpreter can access it
        *self.shared_state.io_submit_tx.lock() = Some(io_submit_tx.clone());

        let shutdown = self.shutdown.clone();

        // --- Spawn VM workers ---
        for i in 0..self.vm_worker_count {
            let rx = vm_task_rx.clone();
            let result_tx = vm_result_tx.clone();
            let io_sub_tx = io_submit_tx.clone();
            let state = self.shared_state.clone();
            let shutdown = shutdown.clone();

            let handle = thread::Builder::new()
                .name(format!("raya-vm-worker-{}", i))
                .spawn(move || {
                    Self::vm_worker_loop(rx, result_tx, io_sub_tx, state, shutdown);
                })
                .expect("Failed to spawn VM worker thread");
            self.vm_worker_handles.push(handle);
        }

        // --- Spawn reactor thread ---
        let shared_state = self.shared_state.clone();
        let vm_worker_count = self.vm_worker_count;
        let shutdown_clone = shutdown.clone();

        let reactor_handle = thread::Builder::new()
            .name("raya-reactor".to_string())
            .spawn(move || {
                Self::reactor_loop(
                    shared_state,
                    vm_task_tx,
                    vm_result_rx,
                    io_submit_rx,
                    io_completion_tx,
                    io_completion_rx,
                    tokio_handle,
                    vm_worker_count,
                    shutdown_clone,
                );
            })
            .expect("Failed to spawn reactor thread");

        self.reactor_handle = Some(reactor_handle);
        self.started = true;
    }

    /// Shutdown the reactor and all workers.
    pub fn shutdown(&mut self) {
        if !self.started {
            return;
        }
        let debug_teardown = std::env::var("RAYA_DEBUG_VM_TEARDOWN").is_ok();
        if debug_teardown {
            eprintln!("[reactor-shutdown] start");
        }

        self.shutdown.store(true, AtomicOrdering::Release);

        // Cancel all tasks so interpreters exit quickly
        {
            let tasks = self.shared_state.tasks.read();
            if debug_teardown {
                eprintln!("[reactor-shutdown] cancel_tasks count={}", tasks.len());
            }
            for task in tasks.values() {
                task.cancel();
            }
        }

        // Drop only the VM task sender immediately to unblock idle workers.
        // Keep IO submission/runtime state alive until worker/reactor threads
        // have actually exited; otherwise Tokio worker threads can still be
        // running against dropped runtime state during teardown.
        self.vm_task_tx.take();

        // Join VM workers and the reactor thread synchronously. Detaching live
        // VM threads during teardown leaves them running against runtime state
        // that the caller is about to drop, which can surface later as
        // allocator corruption in unrelated tests.
        for handle in self.vm_worker_handles.drain(..) {
            if debug_teardown {
                eprintln!("[reactor-shutdown] join_vm_worker:start");
            }
            let _ = handle.join();
            if debug_teardown {
                eprintln!("[reactor-shutdown] join_vm_worker:done");
            }
        }

        if let Some(handle) = self.reactor_handle.take() {
            if debug_teardown {
                eprintln!("[reactor-shutdown] join_reactor:start");
            }
            let _ = handle.join();
            if debug_teardown {
                eprintln!("[reactor-shutdown] join_reactor:done");
            }
        }

        // It is now safe to drop IO submission/runtime state; no VM worker or
        // reactor thread should still touch it after the joins above.
        self.io_submit_tx.take();
        if let Some(runtime) = self.tokio_runtime.take() {
            if debug_teardown {
                eprintln!("[reactor-shutdown] tokio_shutdown:start");
            }
            runtime.shutdown_timeout(Duration::from_secs(5));
            if debug_teardown {
                eprintln!("[reactor-shutdown] tokio_shutdown:done");
            }
        }
        *self.shared_state.io_submit_tx.lock() = None;

        self.started = false;
        self.shared_state.tasks.write().clear();
        if debug_teardown {
            eprintln!("[reactor-shutdown] done");
        }
    }

    // ========================================================================
    // VM Worker Loop
    // ========================================================================

    fn vm_worker_loop(
        work_rx: Receiver<VmWork>,
        result_tx: Sender<VmResult>,
        io_submit_tx: Sender<IoSubmission>,
        state: Arc<SharedVmState>,
        shutdown: Arc<AtomicBool>,
    ) {
        let debug_teardown = std::env::var("RAYA_DEBUG_VM_TEARDOWN").is_ok();
        while !shutdown.load(AtomicOrdering::Acquire) {
            let work = match work_rx.recv_timeout(VM_WORKER_RECV_TIMEOUT) {
                Ok(w) => w,
                Err(channel::RecvTimeoutError::Timeout) => continue,
                Err(channel::RecvTimeoutError::Disconnected) => break,
            };

            let task = work.task;
            // Defensive ownership gate: duplicated queue entries must not allow
            // the same task to execute concurrently on multiple VM workers.
            if !task.try_enter_running() {
                continue;
            }
            task.set_start_time(Instant::now());

            let mut interpreter = Interpreter::new(
                &state.gc,
                &state.classes,
                &state.layouts,
                &state.mutex_registry,
                &state.semaphore_registry,
                &state.safepoint,
                &state.globals_by_index,
                &state.builtin_global_slots,
                &state.js_global_bindings,
                &state.js_global_binding_slots,
                &state.constant_string_cache,
                &state.ephemeral_gc_roots,
                &state.pinned_handles,
                &state.tasks,
                &state.injector,
                &state.promise_microtasks,
                &state.test262_async_state,
                &state.test262_async_failure,
                &state.metadata,
                &state.class_metadata,
                &state.native_handler,
                &state.module_layouts,
                &state.module_registry,
                &state.structural_shape_adapters,
                &state.structural_shape_names,
                &state.structural_layout_shapes,
                &state.type_handles,
                &state.class_value_slots,
                &state.prop_keys,
                &state.aot_profile,
                Some(&io_submit_tx),
                state.max_preemptions,
                &state.stack_pool,
            );

            // Wire JIT code cache and profiling for native dispatch
            #[cfg(feature = "jit")]
            {
                let cache = state.code_cache.lock().clone();
                interpreter.set_code_cache(cache);

                // Wire profiling for on-the-fly compilation
                let module = task.module();
                let profiles = state.module_profiles.read();
                if let Some(profile) = profiles.get(&module.checksum) {
                    interpreter.set_module_profile(Some(profile.clone()));
                }
                drop(profiles);
                interpreter.set_module_profiles_map(Some(&state.module_profiles));

                let compiler = state.background_compiler.lock().clone();
                if let Some(ref c) = compiler {
                    interpreter.set_background_compiler(Some(c.clone()));
                }

                let policy = state.jit_compilation_policy.lock().clone();
                interpreter.set_compilation_policy(policy);

                interpreter.set_jit_telemetry(Some(state.jit_telemetry.clone()));
            }

            // Wire profiler for CPU/wall-clock sampling
            if let Some(ref profiler) = *state.profiler.lock() {
                interpreter.set_profiler(Some(profiler.clone()));
            }

            // Wire debug state for debugger coordination
            if let Some(ref ds) = *state.debug_state.lock() {
                interpreter.set_debug_state(Some(ds.clone()));
            }

            let result = interpreter.run(&task);

            // Signal debug state for terminal results (completion/failure)
            interpreter.signal_debug_result(&result);

            task.clear_start_time();

            let _ = result_tx.send(VmResult { task, result });
        }
        if debug_teardown {
            eprintln!("[vm-worker] exit");
        }
    }

    // ========================================================================
    // Reactor Loop
    // ========================================================================

    #[allow(clippy::too_many_arguments)]
    fn reactor_loop(
        shared_state: Arc<SharedVmState>,
        vm_task_tx: Sender<VmWork>,
        vm_result_rx: Receiver<VmResult>,
        io_submit_rx: Receiver<IoSubmission>,
        io_completion_tx: Sender<IoPoolCompletion>,
        io_completion_rx: Receiver<IoPoolCompletion>,
        tokio_runtime: Handle,
        vm_worker_count: usize,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut timer_heap: BinaryHeap<SleepEntry> = BinaryHeap::new();
        let mut channel_waiters: Vec<ChannelWaiter> = Vec::new();
        let mut ready_queue: VecDeque<Arc<Task>> = VecDeque::new();
        let mut active_vm_tasks: usize = 0;
        let preempt_threshold = Duration::from_millis(shared_state.preempt_threshold_ms);

        loop {
            if shutdown.load(AtomicOrdering::Acquire) {
                shared_state.set_reactor_quiescent(false);
                break;
            }

            // === STEP 1: Drain VM results ===
            loop {
                match vm_result_rx.try_recv() {
                    Ok(vr) => {
                        active_vm_tasks = active_vm_tasks.saturating_sub(1);
                        Self::handle_vm_result(
                            vr,
                            &shared_state,
                            &mut timer_heap,
                            &mut channel_waiters,
                            &mut ready_queue,
                        );
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }

            // === STEP 2: Drain IO submissions from VM workers ===
            loop {
                match io_submit_rx.try_recv() {
                    Ok(sub) => {
                        Self::handle_io_submission(
                            sub,
                            &tokio_runtime,
                            &io_completion_tx,
                            &shared_state,
                            &mut channel_waiters,
                            &mut ready_queue,
                        );
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }

            // === STEP 3: Drain IO completions ===
            loop {
                match io_completion_rx.try_recv() {
                    Ok(completion) => {
                        Self::handle_io_completion(completion, &shared_state, &mut ready_queue);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }

            // === STEP 4: Check timers ===
            let now = Instant::now();
            while let Some(entry) = timer_heap.peek() {
                if entry.wake_at <= now {
                    let entry = timer_heap.pop().unwrap();
                    if entry.task.state() == TaskState::Suspended {
                        entry.task.set_state(TaskState::Resumed);
                        entry.task.clear_suspend_reason();
                        ready_queue.push_back(entry.task);
                    }
                } else {
                    break;
                }
            }

            // === STEP 5: Retry channel waiters (3-phase) ===
            // Phase 1: Try buffer operations (try_send / try_receive)
            let mut unresolved: Vec<ChannelWaiter> = Vec::new();
            for waiter in channel_waiters.drain(..) {
                if waiter.channel_handle == 0
                    || !shared_state.is_valid_pinned_handle(waiter.channel_handle)
                {
                    // Stale/invalid channel handle: wake waiter with a neutral value
                    // instead of dereferencing unknown memory.
                    Self::complete_task(
                        &shared_state,
                        waiter.task_id,
                        Value::null(),
                        &mut ready_queue,
                    );
                    continue;
                }
                let ch_ptr = waiter.channel_handle as *const ChannelObject;
                if ch_ptr.is_null() {
                    Self::complete_task(
                        &shared_state,
                        waiter.task_id,
                        Value::null(),
                        &mut ready_queue,
                    );
                    continue;
                }
                let channel = unsafe { &*ch_ptr };

                if !waiter.is_send {
                    // Receiver: try buffer first
                    if let Some(val) = channel.try_receive() {
                        Self::complete_task(&shared_state, waiter.task_id, val, &mut ready_queue);
                        continue;
                    }
                    // Closed channel with empty buffer → return null
                    if channel.is_closed() {
                        Self::complete_task(
                            &shared_state,
                            waiter.task_id,
                            Value::null(),
                            &mut ready_queue,
                        );
                        continue;
                    }
                    unresolved.push(waiter);
                } else {
                    // Sender: try buffer first
                    if let Some(val) = waiter.value {
                        if channel.try_send(val) {
                            Self::complete_task(
                                &shared_state,
                                waiter.task_id,
                                Value::null(),
                                &mut ready_queue,
                            );
                            continue;
                        }
                        if channel.is_closed() {
                            Self::complete_task(
                                &shared_state,
                                waiter.task_id,
                                Value::null(),
                                &mut ready_queue,
                            );
                            continue;
                        }
                    }
                    unresolved.push(waiter);
                }
            }

            // Phase 2: Pair matching — match senders with receivers on the same channel
            // This is critical for unbuffered channels (capacity=0) where try_send always fails.
            let mut unresolved_senders: Vec<ChannelWaiter> = Vec::new();
            let mut unresolved_receivers: Vec<ChannelWaiter> = Vec::new();
            for w in unresolved.drain(..) {
                if w.is_send {
                    unresolved_senders.push(w);
                } else {
                    unresolved_receivers.push(w);
                }
            }

            let mut remaining_senders: Vec<ChannelWaiter> = Vec::new();
            for sender in unresolved_senders.drain(..) {
                // Find a matching receiver on the same channel
                let mut matched = false;
                for i in 0..unresolved_receivers.len() {
                    if unresolved_receivers[i].channel_handle == sender.channel_handle {
                        let receiver = unresolved_receivers.swap_remove(i);
                        // Transfer value from sender to receiver
                        if let Some(val) = sender.value {
                            Self::complete_task(
                                &shared_state,
                                receiver.task_id,
                                val,
                                &mut ready_queue,
                            );
                            Self::complete_task(
                                &shared_state,
                                sender.task_id,
                                Value::null(),
                                &mut ready_queue,
                            );
                        }
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    remaining_senders.push(sender);
                }
            }

            // Phase 3: Put unresolved waiters back
            channel_waiters.extend(remaining_senders);
            channel_waiters.extend(unresolved_receivers);

            // === STEP 6: Drain injector (spawned/woken tasks) ===
            loop {
                match shared_state.injector.steal() {
                    crossbeam_deque::Steal::Success(task) => {
                        ready_queue.push_back(task);
                    }
                    crossbeam_deque::Steal::Empty => break,
                    crossbeam_deque::Steal::Retry => continue,
                }
            }

            // === STEP 7: Check preemption ===
            {
                let tasks = shared_state.tasks.read();
                for task in tasks.values() {
                    if task.state() == TaskState::Running {
                        if let Some(start) = task.start_time() {
                            if now.duration_since(start) >= preempt_threshold {
                                task.request_preempt();
                            }
                        }
                    }
                }
            }

            // === STEP 8: Dispatch ready tasks to VM workers ===
            while !ready_queue.is_empty() && active_vm_tasks < vm_worker_count {
                if let Some(task) = ready_queue.pop_front() {
                    match vm_task_tx.try_send(VmWork { task }) {
                        Ok(()) => {
                            active_vm_tasks += 1;
                        }
                        Err(TrySendError::Full(work)) => {
                            ready_queue.push_front(work.task);
                            break;
                        }
                        Err(TrySendError::Disconnected(_)) => return,
                    }
                }
            }

            // === STEP 8.5: Promise microtask checkpoint ===
            // Only surface unhandled rejections once the scheduler reaches a
            // genuinely quiescent checkpoint. A one-tick delay is not enough
            // for patterns like `let g = good(); let b = bad(); await g; try { await b; }`
            // where the rejection is observed later in the same user flow.
            let quiescent_checkpoint = active_vm_tasks == 0
                && ready_queue.is_empty()
                && timer_heap.is_empty()
                && channel_waiters.is_empty();
            shared_state.set_reactor_quiescent(quiescent_checkpoint);
            Self::drain_promise_microtasks(&shared_state, quiescent_checkpoint);

            // === STEP 9: Block briefly with select! ===
            let timeout = timer_heap
                .peek()
                .map(|e| {
                    if e.wake_at > now {
                        e.wake_at - now
                    } else {
                        Duration::ZERO
                    }
                })
                .unwrap_or(Duration::from_millis(1))
                .min(Duration::from_millis(1));

            crossbeam::channel::select! {
                recv(vm_result_rx) -> msg => {
                    if let Ok(vr) = msg {
                        active_vm_tasks = active_vm_tasks.saturating_sub(1);
                        Self::handle_vm_result(
                            vr,
                            &shared_state,
                            &mut timer_heap,
                            &mut channel_waiters,
                            &mut ready_queue,
                        );
                    }
                },
                recv(io_submit_rx) -> msg => {
                    if let Ok(sub) = msg {
                        Self::handle_io_submission(
                            sub,
                            &tokio_runtime,
                            &io_completion_tx,
                            &shared_state,
                            &mut channel_waiters,
                            &mut ready_queue,
                        );
                    }
                },
                recv(io_completion_rx) -> msg => {
                    if let Ok(completion) = msg {
                        Self::handle_io_completion(
                            completion,
                            &shared_state,
                            &mut ready_queue,
                        );
                    }
                },
                default(timeout) => {
                    // Timeout — loop back for timer/preempt checks
                },
            }

            // === STEP 9.5: Promise microtask checkpoint (post-select boundary) ===
            let quiescent_checkpoint = active_vm_tasks == 0
                && ready_queue.is_empty()
                && timer_heap.is_empty()
                && channel_waiters.is_empty();
            shared_state.set_reactor_quiescent(quiescent_checkpoint);
            Self::drain_promise_microtasks(&shared_state, quiescent_checkpoint);
        }

        shared_state.set_reactor_quiescent(false);
    }

    // ========================================================================
    // Reactor helpers
    // ========================================================================

    fn build_reaction_interpreter<'a>(
        shared_state: &'a Arc<SharedVmState>,
        io_submit_tx: Option<&'a Sender<IoSubmission>>,
    ) -> Interpreter<'a> {
        Interpreter::new(
            &shared_state.gc,
            &shared_state.classes,
            &shared_state.layouts,
            &shared_state.mutex_registry,
            &shared_state.semaphore_registry,
            &shared_state.safepoint,
            &shared_state.globals_by_index,
            &shared_state.builtin_global_slots,
            &shared_state.js_global_bindings,
            &shared_state.js_global_binding_slots,
            &shared_state.constant_string_cache,
            &shared_state.ephemeral_gc_roots,
            &shared_state.pinned_handles,
            &shared_state.tasks,
            &shared_state.injector,
            &shared_state.promise_microtasks,
            &shared_state.test262_async_state,
            &shared_state.test262_async_failure,
            &shared_state.metadata,
            &shared_state.class_metadata,
            &shared_state.native_handler,
            &shared_state.module_layouts,
            &shared_state.module_registry,
            &shared_state.structural_shape_adapters,
            &shared_state.structural_shape_names,
            &shared_state.structural_layout_shapes,
            &shared_state.type_handles,
            &shared_state.class_value_slots,
            &shared_state.prop_keys,
            &shared_state.aot_profile,
            io_submit_tx,
            shared_state.max_preemptions,
            &shared_state.stack_pool,
        )
    }

    fn handle_vm_result(
        vr: VmResult,
        shared_state: &Arc<SharedVmState>,
        timer_heap: &mut BinaryHeap<SleepEntry>,
        channel_waiters: &mut Vec<ChannelWaiter>,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        match vr.result {
            ExecutionResult::Completed(value) => {
                if Self::try_adopt_async_completion(shared_state, &vr.task, value, ready_queue) {
                    shared_state.stack_pool.release(vr.task.take_stack());
                    return;
                }
                vr.task.complete(value);
                Self::propagate_terminal_settlement(shared_state, &vr.task, ready_queue);
                // Return the stack to the pool for reuse by future tasks
                shared_state.stack_pool.release(vr.task.take_stack());
            }
            ExecutionResult::Suspended(reason) => {
                if matches!(
                    reason,
                    SuspendReason::MutexLock { .. }
                        | SuspendReason::MutexLockCall { .. }
                        | SuspendReason::SemaphoreAcquire { .. }
                        | SuspendReason::AwaitTask(_)
                ) {
                    // These suspends can race with a wakeup performed by another
                    // worker. Park the task only if it is still Running, then
                    // check whether the awaited resource was already transferred
                    // before the suspend result reached the reactor.
                    let parked = vr.task.try_suspend(reason.clone());
                    if parked {
                        match reason {
                            SuspendReason::MutexLock { mutex_id } => {
                                if let Some(mutex) = shared_state.mutex_registry.get(mutex_id) {
                                    if mutex.owner() == Some(vr.task.id()) && vr.task.try_resume() {
                                        vr.task.add_held_mutex(mutex_id);
                                        vr.task.clear_suspend_reason();
                                        ready_queue.push_back(vr.task);
                                    }
                                }
                            }
                            SuspendReason::MutexLockCall { mutex_id } => {
                                if let Some(mutex) = shared_state.mutex_registry.get(mutex_id) {
                                    if mutex.owner() == Some(vr.task.id()) && vr.task.try_resume() {
                                        vr.task.add_held_mutex(mutex_id);
                                        vr.task.set_resume_value(Value::null());
                                        vr.task.clear_suspend_reason();
                                        ready_queue.push_back(vr.task);
                                    }
                                }
                            }
                            SuspendReason::SemaphoreAcquire { .. }
                            | SuspendReason::AwaitTask(_) => {}
                            _ => unreachable!(),
                        }
                    }
                } else {
                    vr.task.suspend(reason.clone());
                    match reason {
                        SuspendReason::AwaitTask(_) => {
                            // Waiter registration already done by Interpreter
                        }
                        SuspendReason::Sleep { wake_at } => {
                            timer_heap.push(SleepEntry {
                                wake_at,
                                task: vr.task,
                            });
                        }
                        SuspendReason::MutexLock { .. } => unreachable!(),
                        SuspendReason::MutexLockCall { .. } => unreachable!(),
                        SuspendReason::SemaphoreAcquire { .. } => unreachable!(),
                        SuspendReason::ChannelSend { channel_id, value } => {
                            channel_waiters.push(ChannelWaiter {
                                task_id: vr.task.id(),
                                channel_handle: channel_id,
                                is_send: true,
                                value: Some(value),
                            });
                        }
                        SuspendReason::ChannelReceive { channel_id } => {
                            channel_waiters.push(ChannelWaiter {
                                task_id: vr.task.id(),
                                channel_handle: channel_id,
                                is_send: false,
                                value: None,
                            });
                        }
                        SuspendReason::IoWait => {
                            // IoRequest was already sent via io_submit_tx by the VM worker
                        }
                        SuspendReason::JsGeneratorYield { .. } | SuspendReason::JsGeneratorInit => {
                            // Live JS generators are usually advanced synchronously via
                            // Generator.prototype.next/return, but the yielded task may still
                            // re-enter the regular scheduler path after an earlier suspension or
                            // resume. Keep the task parked at the yield point instead of
                            // panicking the reactor; the iterator wrapper will resume it on the
                            // next synchronous .next()/.return() call.
                        }
                    }
                }
            }
            ExecutionResult::Failed(e) => {
                // Preserve any explicit JS exception object already attached by the
                // interpreter so rejected async tasks surface the original reason.
                if !vr.task.has_exception() {
                    let msg = e.to_string();
                    let exc = shared_state.allocate_ephemerally_rooted_string(msg);
                    vr.task.set_exception(exc);
                    shared_state.release_ephemeral_gc_root(exc);
                }
                vr.task.fail();
                Self::propagate_terminal_settlement(shared_state, &vr.task, ready_queue);
                shared_state.schedule_unhandled_rejection_check(&vr.task, 1);
                // Return the stack to the pool for reuse by future tasks
                shared_state.stack_pool.release(vr.task.take_stack());
            }
        }
    }

    fn task_uses_async_js_promise_semantics(task: &Arc<Task>) -> bool {
        task.current_module()
            .functions
            .get(task.current_func_id())
            .is_some_and(|function| {
                function.is_async && function.uses_js_runtime_semantics && !function.is_generator
            })
    }

    fn try_adopt_async_completion(
        shared_state: &Arc<SharedVmState>,
        task: &Arc<Task>,
        value: Value,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) -> bool {
        if !Self::task_uses_async_js_promise_semantics(task) {
            return false;
        }

        let Some(source_task_id) = PromiseHandle::from_value(value).map(PromiseHandle::task_id)
        else {
            return false;
        };

        if source_task_id == task.id() {
            let exc = shared_state.allocate_ephemerally_rooted_string(
                "Promise cannot be resolved with itself".to_string(),
            );
            task.set_exception(exc);
            task.fail();
            Self::propagate_terminal_settlement(shared_state, task, ready_queue);
            shared_state.schedule_unhandled_rejection_check(task, 1);
            shared_state.release_ephemeral_gc_root(exc);
            return true;
        }

        let Some(source_task) = shared_state.tasks.read().get(&source_task_id).cloned() else {
            return false;
        };

        if source_task.add_adopter_if_incomplete(task.id()) {
            task.set_state(TaskState::Created);
            return true;
        }

        match source_task.state() {
            TaskState::Completed => {
                task.complete(source_task.result().unwrap_or(Value::undefined()));
            }
            TaskState::Failed => {
                task.set_exception(
                    source_task
                        .observe_rejection_reason()
                        .unwrap_or(Value::null()),
                );
                task.fail();
            }
            TaskState::Created | TaskState::Running | TaskState::Suspended | TaskState::Resumed => {
                task.set_state(TaskState::Created);
                return true;
            }
        }

        Self::propagate_terminal_settlement(shared_state, task, ready_queue);
        if task.state() == TaskState::Failed {
            shared_state.schedule_unhandled_rejection_check(task, 1);
        }
        true
    }

    fn drain_promise_microtasks(shared_state: &Arc<SharedVmState>, quiescent_checkpoint: bool) {
        let drained = shared_state.take_promise_microtasks();
        if drained.is_empty() {
            return;
        }
        let mut requeue = Vec::new();
        for job in drained {
            match job {
                PromiseMicrotask::ReportUnhandledRejection(task_id, delay) => {
                    let Some(task) = shared_state.tasks.read().get(&task_id).cloned() else {
                        continue;
                    };
                    if !shared_state.should_track_unhandled_rejection(&task) {
                        continue;
                    }
                    if !quiescent_checkpoint {
                        requeue.push(PromiseMicrotask::ReportUnhandledRejection(task_id, delay));
                        continue;
                    }
                    if delay > 0 {
                        requeue.push(PromiseMicrotask::ReportUnhandledRejection(
                            task_id,
                            delay - 1,
                        ));
                        continue;
                    }
                    let reason = task.current_exception().unwrap_or(Value::null());
                    shared_state.report_unhandled_rejection(task_id, reason);
                }
                PromiseMicrotask::RunReaction {
                    source_task_id,
                    reaction,
                } => {
                    let Some(source_task) = shared_state.tasks.read().get(&source_task_id).cloned()
                    else {
                        continue;
                    };
                    let io_submit_guard = shared_state.io_submit_tx.lock();
                    let io_submit_tx = io_submit_guard.as_ref();
                    let mut interpreter =
                        Self::build_reaction_interpreter(shared_state, io_submit_tx);
                    interpreter.run_promise_reaction(&source_task, reaction);
                }
            }
        }
        if !requeue.is_empty() {
            shared_state.requeue_promise_microtasks(requeue);
        }
    }

    fn handle_io_submission(
        sub: IoSubmission,
        tokio_runtime: &Handle,
        io_completion_tx: &Sender<IoPoolCompletion>,
        shared_state: &Arc<SharedVmState>,
        channel_waiters: &mut Vec<ChannelWaiter>,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        match sub.request {
            IoRequest::BlockingWork { work } => {
                let task_id = sub.task_id;
                let tx = io_completion_tx.clone();
                Self::spawn_blocking_io(
                    tokio_runtime,
                    task_id,
                    move || {
                        panic::catch_unwind(panic::AssertUnwindSafe(work))
                            .unwrap_or(IoCompletion::Error("io work panicked".to_string()))
                    },
                    tx,
                );
            }
            IoRequest::ChannelReceive { channel } => {
                let v = native_to_value(channel);
                let ch_handle = v.as_u64().unwrap_or(0);
                if ch_handle == 0 || !shared_state.is_valid_pinned_handle(ch_handle) {
                    Self::complete_task(shared_state, sub.task_id, Value::null(), ready_queue);
                    return;
                }
                let ch_ptr = ch_handle as *const ChannelObject;
                if !ch_ptr.is_null() {
                    let ch = unsafe { &*ch_ptr };
                    // Try immediate receive
                    if let Some(val) = ch.try_receive() {
                        Self::complete_task(shared_state, sub.task_id, val, ready_queue);
                        return;
                    }
                    if ch.is_closed() {
                        Self::complete_task(shared_state, sub.task_id, Value::null(), ready_queue);
                        return;
                    }
                }
                // Register as waiter
                channel_waiters.push(ChannelWaiter {
                    task_id: sub.task_id,
                    channel_handle: ch_handle,
                    is_send: false,
                    value: None,
                });
            }
            IoRequest::ChannelSend { channel, value } => {
                let v = native_to_value(channel);
                let send_val = native_to_value(value);
                let ch_handle = v.as_u64().unwrap_or(0);
                if ch_handle == 0 || !shared_state.is_valid_pinned_handle(ch_handle) {
                    Self::complete_task(shared_state, sub.task_id, Value::bool(false), ready_queue);
                    return;
                }
                let ch_ptr = ch_handle as *const ChannelObject;
                if !ch_ptr.is_null() {
                    let ch = unsafe { &*ch_ptr };
                    if ch.try_send(send_val) {
                        Self::complete_task(
                            shared_state,
                            sub.task_id,
                            Value::bool(true),
                            ready_queue,
                        );
                        return;
                    }
                    if ch.is_closed() {
                        Self::complete_task(
                            shared_state,
                            sub.task_id,
                            Value::bool(false),
                            ready_queue,
                        );
                        return;
                    }
                }
                channel_waiters.push(ChannelWaiter {
                    task_id: sub.task_id,
                    channel_handle: ch_handle,
                    is_send: true,
                    value: Some(send_val),
                });
            }
            // Network IO — dispatch as blocking work for now (Phase 4 will use polling)
            IoRequest::NetAccept { .. }
            | IoRequest::NetRead { .. }
            | IoRequest::NetWrite { .. }
            | IoRequest::NetConnect { .. } => {
                let task_id = sub.task_id;
                let tx = io_completion_tx.clone();
                Self::spawn_blocking_io(
                    tokio_runtime,
                    task_id,
                    || {
                        IoCompletion::Error(
                            "Network IO request variants are handled in stdlib layer".into(),
                        )
                    },
                    tx,
                );
            }
            // Sleep is dispatched as BlockingWork (thread::sleep on IO pool).
            // No special reactor handling needed — the IO pool returns the completion.
            IoRequest::Sleep { .. } => unreachable!("Sleep should be dispatched as BlockingWork"),
        }
    }

    fn spawn_blocking_io(
        tokio_runtime: &Handle,
        task_id: TaskId,
        work: impl FnOnce() -> IoCompletion + Send + 'static,
        completion_tx: Sender<IoPoolCompletion>,
    ) {
        tokio_runtime.spawn_blocking(move || {
            let result = work();
            let _ = completion_tx.send(IoPoolCompletion { task_id, result });
        });
    }

    fn handle_io_completion(
        completion: IoPoolCompletion,
        shared_state: &Arc<SharedVmState>,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        let task_cancelled = {
            let tasks = shared_state.tasks.read();
            tasks
                .get(&completion.task_id)
                .map(|t| t.is_cancelled())
                .unwrap_or(false)
        };

        let value = match completion.result {
            IoCompletion::Bytes(data) => {
                // Create raw Buffer on heap (same as BUFFER_NEW native call)
                let mut buffer = Buffer::new(data.len());
                for (i, &byte) in data.iter().enumerate() {
                    let _ = buffer.set_byte(i, byte);
                }
                let handle = shared_state.allocate_pinned_handle(buffer);

                // Wrap in a proper Object with Buffer nominal_type_id so vtable dispatch works
                let (nominal_type_id, field_count, layout_id) = {
                    let mut classes = shared_state.classes.write();
                    if let Some(id) = classes.get_class_by_name("Buffer").map(|class| class.id) {
                        let (layout_id, field_count) = shared_state
                            .layouts
                            .read()
                            .nominal_allocation(id)
                            .expect("registered Buffer allocation");
                        (id, field_count.max(2), layout_id)
                    } else {
                        drop(classes);
                        let id = shared_state.register_runtime_class_with_layout_names(
                            Class::new(0, "Buffer".to_string(), 2),
                            Some(crate::vm::object::BUFFER_LAYOUT_FIELDS),
                        );
                        let (layout_id, field_count) = shared_state
                            .layouts
                            .read()
                            .nominal_allocation(id)
                            .expect("registered Buffer allocation");
                        (id, field_count.max(2), layout_id)
                    }
                };
                let mut obj = Object::new_nominal(layout_id, nominal_type_id as u32, field_count);
                obj.fields[0] = Value::u64(handle); // bufferPtr field
                if field_count > 1 {
                    obj.fields[1] = Value::i32(data.len() as i32); // length field
                }
                let mut gc = shared_state.gc.lock();
                let obj_ptr = gc.allocate(obj);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) }
            }
            IoCompletion::String(s) => {
                let raya_str = RayaString::new(s);
                let gc_ptr = shared_state.gc.lock().allocate(raya_str);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }
            IoCompletion::StringArray(strings) => {
                use crate::vm::object::{Array, RayaString as RS};
                let mut gc = shared_state.gc.lock();
                let mut string_values: Vec<Value> = Vec::with_capacity(strings.len());
                for s in strings {
                    let raya_str = RS::new(s);
                    let gc_ptr = gc.allocate(raya_str);
                    let val = unsafe {
                        Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                    };
                    string_values.push(val);
                }
                let mut arr = Array::new(0, string_values.len());
                for (i, v) in string_values.iter().enumerate() {
                    let _ = arr.set(i, *v);
                }
                let gc_ptr = gc.allocate(arr);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }
            IoCompletion::Primitive(native_val) => native_to_value(native_val),
            IoCompletion::Error(msg) => {
                if task_cancelled {
                    // Ignore late IO errors for cancelled tasks and resume them
                    // as cancelled completion instead.
                    Self::complete_task(
                        shared_state,
                        completion.task_id,
                        Value::null(),
                        ready_queue,
                    );
                    return;
                }
                eprintln!(
                    "[reactor] IO error for task {:?}: {}",
                    completion.task_id, msg
                );
                let err_val = shared_state.allocate_ephemerally_rooted_string(msg);
                Self::complete_task_with_exception(
                    shared_state,
                    completion.task_id,
                    err_val,
                    ready_queue,
                );
                shared_state.release_ephemeral_gc_root(err_val);
                return;
            }
        };

        Self::complete_task(shared_state, completion.task_id, value, ready_queue);
    }

    /// Complete a task with a value and add it to the ready queue.
    fn complete_task(
        shared_state: &Arc<SharedVmState>,
        task_id: TaskId,
        value: Value,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        let tasks = shared_state.tasks.read();
        if let Some(task) = tasks.get(&task_id) {
            task.set_resume_value(value);
            if task.resume_if_pending() {
                task.clear_suspend_reason();
                ready_queue.push_back(task.clone());
            }
        }
    }

    /// Resume a suspended task with a pending exception.
    fn complete_task_with_exception(
        shared_state: &Arc<SharedVmState>,
        task_id: TaskId,
        exception: Value,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        let tasks = shared_state.tasks.read();
        if let Some(task) = tasks.get(&task_id) {
            if task.is_cancelled() {
                // Cancellation wins over delayed IO exceptions.
                task.set_resume_value(Value::null());
                if task.resume_if_pending() {
                    task.clear_suspend_reason();
                    ready_queue.push_back(task.clone());
                }
                return;
            }
            // Ensure stale resume values are not materialized when resuming with exception.
            let _ = task.take_resume_value();
            task.set_exception(exception);
            if task.resume_if_pending() {
                task.clear_suspend_reason();
                ready_queue.push_back(task.clone());
            }
        }
    }

    /// Wake tasks waiting on a terminal task and cascade any adopted promises/tasks.
    fn propagate_terminal_settlement(
        shared_state: &Arc<SharedVmState>,
        task: &Arc<Task>,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        let mut settled = vec![task.clone()];
        while let Some(source) = settled.pop() {
            Self::wake_waiters(shared_state, &source, ready_queue);
            for reaction in source.take_reactions() {
                shared_state
                    .promise_microtasks
                    .lock()
                    .push_back(PromiseMicrotask::RunReaction {
                        source_task_id: source.id(),
                        reaction,
                    });
            }

            let adopters = source.take_adopters();
            if adopters.is_empty() {
                continue;
            }

            let source_failed = source.state() == TaskState::Failed;
            let source_result = source.result();
            let source_exception = if source_failed {
                Some(source.observe_rejection_reason().unwrap_or(Value::null()))
            } else {
                None
            };

            let tasks_map = shared_state.tasks.read();
            let adopter_tasks: Vec<_> = adopters
                .into_iter()
                .filter_map(|task_id| tasks_map.get(&task_id).cloned())
                .collect();
            drop(tasks_map);

            for adopter in adopter_tasks {
                match adopter.state() {
                    TaskState::Completed | TaskState::Failed => continue,
                    _ => {}
                }
                if let Some(exception) = source_exception {
                    adopter.set_exception(exception);
                    adopter.fail();
                } else if let Some(result) = source_result {
                    adopter.complete(result);
                }
                settled.push(adopter);
            }
        }
    }

    /// Wake tasks waiting for a completed/failed task.
    fn wake_waiters(
        shared_state: &Arc<SharedVmState>,
        task: &Arc<Task>,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        let waiters = task.take_waiters();
        if waiters.is_empty() {
            return;
        }

        let tasks_map = shared_state.tasks.read();
        let task_failed = task.state() == TaskState::Failed;
        let exception = if task_failed {
            task.observe_rejection_reason()
        } else {
            None
        };

        for waiter_id in waiters {
            if let Some(waiter_task) = tasks_map.get(&waiter_id) {
                if task_failed {
                    // Ensure failure path does not keep an older resume payload.
                    let _ = waiter_task.take_resume_value();
                    if let Some(exc) = exception {
                        waiter_task.set_exception(exc);
                    } else {
                        waiter_task.set_exception(Value::null());
                    }
                } else if let Some(result) = task.result() {
                    waiter_task.set_resume_value(result);
                }
                if waiter_task.resume_if_pending() {
                    waiter_task.clear_suspend_reason();
                    ready_queue.push_back(waiter_task.clone());
                }
            }
        }
    }
}

impl Drop for Reactor {
    fn drop(&mut self) {
        self.shutdown();
    }
}
