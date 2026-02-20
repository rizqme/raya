//! Unified Reactor — single control thread for scheduling + event loop
//!
//! The reactor merges the scheduler and event loop into one thread running a single loop.
//! Two worker pools (VM + IO) do the actual work. The reactor is the sole decision-maker:
//! it dispatches tasks to VM workers, handles IO submissions, manages timers, retries
//! channel waiters, and checks preemption — all in one loop iteration.

use crate::vm::interpreter::{ExecutionResult, Interpreter, SharedVmState};
use crate::vm::object::{Buffer, ChannelObject, RayaString};
use crate::vm::scheduler::{SuspendReason, Task, TaskId, TaskState};
use crate::vm::value::Value;
use crate::vm::abi::native_to_value;
use crossbeam::channel::{self, Receiver, Sender, TryRecvError, TrySendError};
use raya_sdk::{IoCompletion, IoRequest};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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

/// IO work sent from reactor to an IO worker
#[allow(dead_code)]
struct IoWork {
    task_id: TaskId,
    work: Box<dyn FnOnce() -> IoCompletion + Send>,
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
    /// IO worker pool size
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

    /// Channel: reactor → IO workers
    io_work_tx: Option<Sender<IoWork>>,
    /// Channel: IO workers → reactor (completions)
    io_completion_rx: Option<Receiver<IoPoolCompletion>>,

    /// Shutdown signal
    shutdown: Arc<AtomicBool>,

    /// Thread handles
    reactor_handle: Option<JoinHandle<()>>,
    vm_worker_handles: Vec<JoinHandle<()>>,
    io_worker_handles: Vec<JoinHandle<()>>,

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
            io_work_tx: None,
            io_completion_rx: None,
            shutdown: Arc::new(AtomicBool::new(false)),
            reactor_handle: None,
            vm_worker_handles: Vec::new(),
            io_worker_handles: Vec::new(),
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
        let (io_work_tx, io_work_rx) = channel::unbounded::<IoWork>();
        let (io_completion_tx, io_completion_rx) = channel::unbounded::<IoPoolCompletion>();

        self.vm_task_tx = Some(vm_task_tx.clone());
        self.vm_result_rx = Some(vm_result_rx.clone());
        self.io_submit_rx = Some(io_submit_rx.clone());
        self.io_submit_tx = Some(io_submit_tx.clone());
        self.io_work_tx = Some(io_work_tx.clone());
        self.io_completion_rx = Some(io_completion_rx.clone());

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

        // --- Spawn IO workers ---
        let io_count = self.io_worker_count;
        for i in 0..io_count {
            let rx = io_work_rx.clone();
            let shutdown = shutdown.clone();

            let handle = thread::Builder::new()
                .name(format!("raya-io-worker-{}", i))
                .spawn(move || {
                    Self::io_worker_loop(rx, shutdown);
                })
                .expect("Failed to spawn IO worker thread");
            self.io_worker_handles.push(handle);
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
                    io_work_tx,
                    io_completion_tx,
                    io_completion_rx,
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

        self.shutdown.store(true, AtomicOrdering::Release);

        // Cancel all tasks so interpreters exit quickly
        {
            let tasks = self.shared_state.tasks.read();
            for task in tasks.values() {
                task.cancel();
            }
        }

        // Drop senders to unblock workers
        self.vm_task_tx.take();
        self.io_work_tx.take();
        self.io_submit_tx.take();

        // Clear shared io_submit_tx
        *self.shared_state.io_submit_tx.lock() = None;

        // Join VM workers (2s timeout)
        let timeout = Duration::from_secs(2);
        for handle in self.vm_worker_handles.drain(..) {
            Self::join_with_timeout(handle, timeout);
        }

        // Join IO workers (2s timeout)
        for handle in self.io_worker_handles.drain(..) {
            Self::join_with_timeout(handle, timeout);
        }

        // Join reactor thread
        if let Some(handle) = self.reactor_handle.take() {
            Self::join_with_timeout(handle, timeout);
        }

        self.started = false;

        // Clear task registry
        self.shared_state.tasks.write().clear();
    }

    /// Join a thread with timeout, detach if stuck.
    fn join_with_timeout(handle: JoinHandle<()>, timeout: Duration) {
        let start = Instant::now();
        loop {
            if handle.is_finished() {
                let _ = handle.join();
                return;
            }
            if start.elapsed() > timeout {
                drop(handle);
                return;
            }
            thread::sleep(Duration::from_millis(5));
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
        while !shutdown.load(AtomicOrdering::Acquire) {
            let work = match work_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(w) => w,
                Err(channel::RecvTimeoutError::Timeout) => continue,
                Err(channel::RecvTimeoutError::Disconnected) => break,
            };

            let task = work.task;
            task.set_state(TaskState::Running);
            task.set_start_time(Instant::now());

            let mut interpreter = Interpreter::new(
                &state.gc,
                &state.classes,
                &state.mutex_registry,
                &state.safepoint,
                &state.globals_by_index,
                &state.tasks,
                &state.injector,
                &state.metadata,
                &state.class_metadata,
                &state.native_handler,
                &state.resolved_natives,
                Some(&io_submit_tx),
                state.max_preemptions,
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

                let compiler = state.background_compiler.lock().clone();
                if let Some(ref c) = compiler {
                    interpreter.set_background_compiler(Some(c.clone()));
                }
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
    }

    // ========================================================================
    // IO Worker Loop
    // ========================================================================

    fn io_worker_loop(
        work_rx: Receiver<IoWork>,
        shutdown: Arc<AtomicBool>,
    ) {
        while !shutdown.load(AtomicOrdering::Acquire) {
            let work = match work_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(w) => w,
                Err(channel::RecvTimeoutError::Timeout) => continue,
                Err(channel::RecvTimeoutError::Disconnected) => break,
            };

            // Execute the blocking work and send completion
            // The closure captures its own completion_tx
            (work.work)();
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
        io_work_tx: Sender<IoWork>,
        io_completion_tx: Sender<IoPoolCompletion>,
        io_completion_rx: Receiver<IoPoolCompletion>,
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
                            &io_work_tx,
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
                        Self::handle_io_completion(
                            completion,
                            &shared_state,
                            &mut ready_queue,
                        );
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
                let ch_ptr = waiter.channel_handle as *const ChannelObject;
                if ch_ptr.is_null() {
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
                        Self::complete_task(&shared_state, waiter.task_id, Value::null(), &mut ready_queue);
                        continue;
                    }
                    unresolved.push(waiter);
                } else {
                    // Sender: try buffer first
                    if let Some(val) = waiter.value {
                        if channel.try_send(val) {
                            Self::complete_task(&shared_state, waiter.task_id, Value::null(), &mut ready_queue);
                            continue;
                        }
                        if channel.is_closed() {
                            Self::complete_task(&shared_state, waiter.task_id, Value::null(), &mut ready_queue);
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
                            Self::complete_task(&shared_state, receiver.task_id, val, &mut ready_queue);
                            Self::complete_task(&shared_state, sender.task_id, Value::null(), &mut ready_queue);
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
                            &io_work_tx,
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
        }
    }

    // ========================================================================
    // Reactor helpers
    // ========================================================================

    fn handle_vm_result(
        vr: VmResult,
        shared_state: &Arc<SharedVmState>,
        timer_heap: &mut BinaryHeap<SleepEntry>,
        channel_waiters: &mut Vec<ChannelWaiter>,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        match vr.result {
            ExecutionResult::Completed(value) => {
                vr.task.complete(value);
                Self::wake_waiters(shared_state, &vr.task, ready_queue);
            }
            ExecutionResult::Suspended(reason) => {
                if matches!(reason, SuspendReason::MutexLock { .. }) {
                    // MutexLock wakeup is handled by the MutexUnlock opcode on
                    // VM workers (not by the reactor). Use try_suspend to avoid
                    // overwriting a Resumed state if unlock already woke the task.
                    vr.task.try_suspend(reason);
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
                    }
                }
            }
            ExecutionResult::Failed(e) => {
                vr.task.fail_with_error(&e);
                Self::wake_waiters(shared_state, &vr.task, ready_queue);
            }
        }
    }

    fn handle_io_submission(
        sub: IoSubmission,
        io_work_tx: &Sender<IoWork>,
        io_completion_tx: &Sender<IoPoolCompletion>,
        shared_state: &Arc<SharedVmState>,
        channel_waiters: &mut Vec<ChannelWaiter>,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        match sub.request {
            IoRequest::BlockingWork { work } => {
                let task_id = sub.task_id;
                let tx = io_completion_tx.clone();
                // Wrap the work so it sends the completion back
                let _ = io_work_tx.send(IoWork {
                    task_id,
                    work: Box::new(move || {
                        let result = work();
                        let _ = tx.send(IoPoolCompletion { task_id, result });
                        IoCompletion::Primitive(raya_sdk::NativeValue::null()) // unused
                    }),
                });
            }
            IoRequest::ChannelReceive { channel } => {
                let v = native_to_value(channel);
                let ch_handle = v.as_u64().unwrap_or(0);
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
                let ch_ptr = ch_handle as *const ChannelObject;
                if !ch_ptr.is_null() {
                    let ch = unsafe { &*ch_ptr };
                    if ch.try_send(send_val) {
                        Self::complete_task(shared_state, sub.task_id, Value::bool(true), ready_queue);
                        return;
                    }
                    if ch.is_closed() {
                        Self::complete_task(shared_state, sub.task_id, Value::bool(false), ready_queue);
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
                let _ = io_work_tx.send(IoWork {
                    task_id,
                    work: Box::new(move || {
                        let result = IoCompletion::Error(
                            "Non-blocking network IO not yet implemented".into(),
                        );
                        let _ = tx.send(IoPoolCompletion { task_id, result });
                        IoCompletion::Primitive(raya_sdk::NativeValue::null())
                    }),
                });
            }
            // Sleep is dispatched as BlockingWork (thread::sleep on IO pool).
            // No special reactor handling needed — the IO pool returns the completion.
            IoRequest::Sleep { .. } => unreachable!("Sleep should be dispatched as BlockingWork"),
        }
    }

    fn handle_io_completion(
        completion: IoPoolCompletion,
        shared_state: &Arc<SharedVmState>,
        ready_queue: &mut VecDeque<Arc<Task>>,
    ) {
        let value = match completion.result {
            IoCompletion::Bytes(data) => {
                let mut buffer = Buffer::new(data.len());
                for (i, &byte) in data.iter().enumerate() {
                    let _ = buffer.set_byte(i, byte);
                }
                let gc_ptr = shared_state.gc.lock().allocate(buffer);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
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
                // TODO: Set exception on the task instead of returning null
                eprintln!("[reactor] IO error for task {:?}: {}", completion.task_id, msg);
                Value::null()
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
            task.set_state(TaskState::Resumed);
            task.clear_suspend_reason();
            ready_queue.push_back(task.clone());
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
            task.current_exception()
        } else {
            None
        };

        for waiter_id in waiters {
            if let Some(waiter_task) = tasks_map.get(&waiter_id) {
                if task_failed {
                    if let Some(exc) = exception {
                        waiter_task.set_exception(exc);
                    } else {
                        waiter_task.set_exception(Value::null());
                    }
                } else if let Some(result) = task.result() {
                    waiter_task.set_resume_value(result);
                }
                waiter_task.set_state(TaskState::Resumed);
                waiter_task.clear_suspend_reason();
                ready_queue.push_back(waiter_task.clone());
            }
        }
    }
}

impl Drop for Reactor {
    fn drop(&mut self) {
        self.shutdown();
    }
}
