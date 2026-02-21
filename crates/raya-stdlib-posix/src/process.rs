//! std:process — Process management

use crate::handles::HandleRegistry;
use raya_sdk::{NativeCallResult, NativeContext, NativeValue, IoRequest, IoCompletion};
use std::io::{Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

/// Cached result of a process execution
struct ExecResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

static EXEC_HANDLES: LazyLock<HandleRegistry<ExecResult>> = LazyLock::new(HandleRegistry::new);

/// Child process handle — needs interior mutability since Child.wait() takes &mut
struct ChildProcessHandle {
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Mutex<Option<ChildStdout>>,
    stderr: Mutex<Option<ChildStderr>>,
}

static CHILD_PROCESSES: LazyLock<HandleRegistry<ChildProcessHandle>> =
    LazyLock::new(HandleRegistry::new);

/// Exit the process
pub fn exit(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let code = args.first()
        .and_then(|v| v.as_i32().or_else(|| v.as_f64().map(|f| f as i32)))
        .unwrap_or(0);
    std::process::exit(code);
}

/// Get current process ID
pub fn pid(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(std::process::id() as f64)
}

/// Get command-line arguments
pub fn argv(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let args: Vec<NativeValue> = std::env::args()
        .map(|a| ctx.create_string(&a))
        .collect();
    NativeCallResult::Value(ctx.create_array(&args))
}

/// Get path to current executable
pub fn exec_path(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    match std::env::current_exe() {
        Ok(path) => NativeCallResult::Value(ctx.create_string(&path.to_string_lossy())),
        Err(e) => NativeCallResult::Error(format!("process.execPath: {}", e)),
    }
}

/// Execute shell command, return handle to read results (blocking → IO pool)
pub fn exec(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let command = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.exec: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let output = if cfg!(target_os = "windows") {
                std::process::Command::new("cmd").args(["/C", &command]).output()
            } else {
                std::process::Command::new("sh").args(["-c", &command]).output()
            };
            match output {
                Ok(out) => {
                    let result = ExecResult {
                        exit_code: out.status.code().unwrap_or(-1),
                        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                    };
                    let handle = EXEC_HANDLES.insert(result);
                    IoCompletion::Primitive(NativeValue::f64(handle as f64))
                }
                Err(e) => IoCompletion::Error(format!("process.exec: {}", e)),
            }
        }),
    })
}

/// Get exit code from exec handle
pub fn exec_get_code(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match EXEC_HANDLES.get(handle) {
        Some(r) => NativeCallResult::i32(r.exit_code),
        None => NativeCallResult::Error(format!("process.execGetCode: invalid handle {}", handle)),
    }
}

/// Get stdout from exec handle
pub fn exec_get_stdout(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match EXEC_HANDLES.get(handle) {
        Some(r) => NativeCallResult::Value(ctx.create_string(&r.stdout)),
        None => NativeCallResult::Error(format!("process.execGetStdout: invalid handle {}", handle)),
    }
}

/// Get stderr from exec handle
pub fn exec_get_stderr(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match EXEC_HANDLES.get(handle) {
        Some(r) => NativeCallResult::Value(ctx.create_string(&r.stderr)),
        None => NativeCallResult::Error(format!("process.execGetStderr: invalid handle {}", handle)),
    }
}

/// Release exec handle
pub fn exec_release(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    EXEC_HANDLES.remove(handle);
    NativeCallResult::null()
}

/// Get parent process ID
pub fn ppid(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: getppid() is always safe to call, no arguments
    let ppid = unsafe { libc::getppid() };
    NativeCallResult::f64(ppid as f64)
}

/// Get runtime version
pub fn version(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Value(ctx.create_string(env!("CARGO_PKG_VERSION")))
}

/// Process start time for uptime calculation
static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Get process uptime in seconds
pub fn uptime(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(PROCESS_START.elapsed().as_secs_f64())
}

/// Get process resident memory usage in bytes
pub fn memory_usage(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(get_memory_usage() as f64)
}

/// Get CPU usage as [user_micros, system_micros]
pub fn cpu_usage(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: getrusage is safe to call with a zeroed struct
    let usage = unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage
    };
    let user_micros = usage.ru_utime.tv_sec as f64 * 1_000_000.0 + usage.ru_utime.tv_usec as f64;
    let system_micros = usage.ru_stime.tv_sec as f64 * 1_000_000.0 + usage.ru_stime.tv_usec as f64;
    let items = [NativeValue::f64(user_micros), NativeValue::f64(system_micros)];
    NativeCallResult::Value(ctx.create_array(&items))
}

/// Change current working directory
pub fn chdir(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.chdir: {}", e)),
    };
    match std::env::set_current_dir(&path) {
        Ok(()) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("process.chdir: {}", e)),
    }
}

/// Get current umask without changing it
pub fn umask_get(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: umask is safe; we call it twice to read-then-restore
    let old = unsafe { libc::umask(0) };
    unsafe { libc::umask(old) };
    NativeCallResult::f64(old as f64)
}

/// Set umask, return old umask
pub fn umask_set(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let mask = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as libc::mode_t;
    // SAFETY: umask is always safe to call
    let old = unsafe { libc::umask(mask) };
    NativeCallResult::f64(old as f64)
}

/// Get real user ID
pub fn uid(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: getuid() is always safe
    NativeCallResult::f64(unsafe { libc::getuid() } as f64)
}

/// Get real group ID
pub fn gid(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: getgid() is always safe
    NativeCallResult::f64(unsafe { libc::getgid() } as f64)
}

/// Get effective user ID
pub fn euid(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: geteuid() is always safe
    NativeCallResult::f64(unsafe { libc::geteuid() } as f64)
}

/// Get effective group ID
pub fn egid(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: getegid() is always safe
    NativeCallResult::f64(unsafe { libc::getegid() } as f64)
}

/// Get supplementary group IDs
pub fn groups(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: getgroups with 0 returns the count, then we call again with buffer
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    if count < 0 {
        return NativeCallResult::Value(ctx.create_array(&[]));
    }
    let mut gids = vec![0 as libc::gid_t; count as usize];
    // SAFETY: buffer is correctly sized
    let actual = unsafe { libc::getgroups(count, gids.as_mut_ptr()) };
    if actual < 0 {
        return NativeCallResult::Value(ctx.create_array(&[]));
    }
    let items: Vec<NativeValue> = gids[..actual as usize]
        .iter()
        .map(|&g| NativeValue::f64(g as f64))
        .collect();
    NativeCallResult::Value(ctx.create_array(&items))
}

// ── Process title ──

use std::sync::RwLock;

static PROCESS_TITLE: LazyLock<RwLock<String>> = LazyLock::new(|| {
    RwLock::new(
        std::env::current_exe()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .unwrap_or_default(),
    )
});

/// Get process title
pub fn title(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let t = PROCESS_TITLE.read().unwrap().clone();
    NativeCallResult::Value(ctx.create_string(&t))
}

/// Set process title
pub fn set_title(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let new_title = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.setTitle: {}", e)),
    };
    *PROCESS_TITLE.write().unwrap() = new_title;
    NativeCallResult::null()
}

/// Get Raya GC heap used in bytes (stub — returns 0 until GC stats are wired)
pub fn heap_used(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // TODO: Wire to actual GC heap stats via NativeContext when available
    NativeCallResult::f64(0.0)
}

/// Get Raya GC heap total in bytes (stub — returns 0 until GC stats are wired)
pub fn heap_total(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // TODO: Wire to actual GC heap stats via NativeContext when available
    NativeCallResult::f64(0.0)
}

// ── Signal handling ──

use std::sync::atomic::{AtomicBool, Ordering};

/// Signal flags — indexed by signal number. Max signal number on most Unix is 64.
#[allow(clippy::declare_interior_mutable_const)] // Const used to initialize static array
static SIGNAL_FLAGS: [AtomicBool; 64] = {
    const INIT: AtomicBool = AtomicBool::new(false);
    [INIT; 64]
};

/// Which signals we are trapping (to distinguish "not received" from "not trapped")
#[allow(clippy::declare_interior_mutable_const)] // Const used to initialize static array
static SIGNAL_TRAPPED: [AtomicBool; 64] = {
    const INIT: AtomicBool = AtomicBool::new(false);
    [INIT; 64]
};

fn signal_name_to_number(name: &str) -> Option<i32> {
    match name.to_uppercase().as_str() {
        "SIGINT" | "INT" => Some(libc::SIGINT),
        "SIGTERM" | "TERM" => Some(libc::SIGTERM),
        "SIGHUP" | "HUP" => Some(libc::SIGHUP),
        "SIGUSR1" | "USR1" => Some(libc::SIGUSR1),
        "SIGUSR2" | "USR2" => Some(libc::SIGUSR2),
        _ => None,
    }
}

fn signal_number_to_name(sig: usize) -> &'static str {
    match sig as i32 {
        libc::SIGINT => "SIGINT",
        libc::SIGTERM => "SIGTERM",
        libc::SIGHUP => "SIGHUP",
        libc::SIGUSR1 => "SIGUSR1",
        libc::SIGUSR2 => "SIGUSR2",
        _ => "UNKNOWN",
    }
}

/// C-compatible signal handler — sets atomic flag
extern "C" fn signal_handler(sig: libc::c_int) {
    let sig = sig as usize;
    if sig < SIGNAL_FLAGS.len() {
        SIGNAL_FLAGS[sig].store(true, Ordering::SeqCst);
    }
}

/// Register a signal to be trapped (caught and stored in atomic flag)
pub fn trap_signal(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let name = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.trapSignal: {}", e)),
    };
    let sig = match signal_name_to_number(&name) {
        Some(s) => s,
        None => return NativeCallResult::Error(format!("process.trapSignal: unknown signal '{}'", name)),
    };
    let sig_usize = sig as usize;
    if sig_usize >= SIGNAL_FLAGS.len() {
        return NativeCallResult::Error("process.trapSignal: signal number out of range".to_string());
    }
    // Mark as trapped and install handler
    SIGNAL_TRAPPED[sig_usize].store(true, Ordering::SeqCst);
    SIGNAL_FLAGS[sig_usize].store(false, Ordering::SeqCst);
    // SAFETY: signal_handler is a valid C signal handler that only sets an atomic flag
    unsafe {
        libc::signal(sig, signal_handler as libc::sighandler_t);
    }
    NativeCallResult::null()
}

/// Remove signal trap and restore default handler
pub fn untrap_signal(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let name = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.untrapSignal: {}", e)),
    };
    let sig = match signal_name_to_number(&name) {
        Some(s) => s,
        None => return NativeCallResult::Error(format!("process.untrapSignal: unknown signal '{}'", name)),
    };
    let sig_usize = sig as usize;
    if sig_usize >= SIGNAL_FLAGS.len() {
        return NativeCallResult::Error("process.untrapSignal: signal number out of range".to_string());
    }
    SIGNAL_TRAPPED[sig_usize].store(false, Ordering::SeqCst);
    SIGNAL_FLAGS[sig_usize].store(false, Ordering::SeqCst);
    // SAFETY: restoring default signal handler
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
    }
    NativeCallResult::null()
}

/// Check if a trapped signal has been received (non-blocking)
pub fn has_signal(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let name = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.hasSignal: {}", e)),
    };
    let sig = match signal_name_to_number(&name) {
        Some(s) => s,
        None => return NativeCallResult::bool(false),
    };
    let sig_usize = sig as usize;
    if sig_usize >= SIGNAL_FLAGS.len() {
        return NativeCallResult::bool(false);
    }
    NativeCallResult::bool(SIGNAL_FLAGS[sig_usize].load(Ordering::SeqCst))
}

/// Clear a signal flag (acknowledge receipt)
pub fn clear_signal(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let name = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.clearSignal: {}", e)),
    };
    let sig = match signal_name_to_number(&name) {
        Some(s) => s,
        None => return NativeCallResult::null(),
    };
    let sig_usize = sig as usize;
    if sig_usize < SIGNAL_FLAGS.len() {
        SIGNAL_FLAGS[sig_usize].store(false, Ordering::SeqCst);
    }
    NativeCallResult::null()
}

/// Wait until any trapped signal is received (blocking).
/// Returns the name of the signal that was received, and clears its flag.
pub fn wait_signal(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            // Poll every 50ms for a received signal
            loop {
                for i in 0..SIGNAL_FLAGS.len() {
                    if SIGNAL_TRAPPED[i].load(Ordering::SeqCst)
                        && SIGNAL_FLAGS[i].swap(false, Ordering::SeqCst)
                    {
                        return IoCompletion::String(signal_number_to_name(i).to_string());
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }),
    })
}

// ── Platform-specific helpers ──

fn get_memory_usage() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use std::mem;
        // SAFETY: mach_task_basic_info is safe with properly zeroed struct
        let mut info: libc::mach_task_basic_info_data_t = unsafe { mem::zeroed() };
        let mut count = (mem::size_of::<libc::mach_task_basic_info_data_t>()
            / mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
        #[allow(deprecated)] // libc deprecates in favor of mach2 crate, but we use libc
        let ret = unsafe {
            libc::task_info(
                libc::mach_task_self(),
                libc::MACH_TASK_BASIC_INFO,
                &mut info as *mut _ as *mut _,
                &mut count,
            )
        };
        if ret == libc::KERN_SUCCESS {
            info.resident_size as u64
        } else {
            0
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Parse /proc/self/statm — second field is resident pages
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
            if let Some(resident) = statm.split_whitespace().nth(1) {
                if let Ok(pages) = resident.parse::<u64>() {
                    return pages * page_size;
                }
            }
        }
        0
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

// ── Child Process (subprocess spawning) ──

/// Helper: configure pipes and spawn a Command, returning handle as f64
fn spawn_child(mut cmd: Command) -> NativeCallResult {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    match cmd.spawn() {
        Ok(mut child) => {
            let stdin = child.stdin.take();
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let handle = ChildProcessHandle {
                child: Mutex::new(child),
                stdin: Mutex::new(stdin),
                stdout: Mutex::new(stdout),
                stderr: Mutex::new(stderr),
            };
            let id = CHILD_PROCESSES.insert(handle);
            NativeCallResult::f64(id as f64)
        }
        Err(e) => NativeCallResult::Error(format!("process.spawn: {}", e)),
    }
}

/// Spawn command via shell (sh -c)
pub fn process_spawn(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let command = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.spawn: {}", e)),
    };
    let mut cmd = Command::new("sh");
    cmd.args(["-c", &command]);
    spawn_child(cmd)
}

/// Spawn command with explicit args (no shell)
pub fn process_spawn_with_args(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let command = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.spawnWithArgs: {}", e)),
    };
    // args[1] is an array of strings
    let arr = args[1];
    let len = match ctx.array_len(arr) {
        Ok(n) => n,
        Err(e) => return NativeCallResult::Error(format!("process.spawnWithArgs: {}", e)),
    };
    let mut cmd_args = Vec::with_capacity(len);
    for i in 0..len {
        let elem = match ctx.array_get(arr, i) {
            Ok(v) => v,
            Err(e) => return NativeCallResult::Error(format!("process.spawnWithArgs: {}", e)),
        };
        match ctx.read_string(elem) {
            Ok(s) => cmd_args.push(s),
            Err(e) => return NativeCallResult::Error(format!("process.spawnWithArgs: {}", e)),
        }
    }
    let mut cmd = Command::new(&command);
    cmd.args(&cmd_args);
    spawn_child(cmd)
}

/// Spawn with custom working directory and environment
pub fn process_spawn_with_options(
    ctx: &dyn NativeContext,
    args: &[NativeValue],
) -> NativeCallResult {
    let command = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.spawnWithOptions: {}", e)),
    };
    // args[1] = string[] (command args)
    let arr = args[1];
    let len = match ctx.array_len(arr) {
        Ok(n) => n,
        Err(e) => return NativeCallResult::Error(format!("process.spawnWithOptions: {}", e)),
    };
    let mut cmd_args = Vec::with_capacity(len);
    for i in 0..len {
        let elem = match ctx.array_get(arr, i) {
            Ok(v) => v,
            Err(e) => return NativeCallResult::Error(format!("process.spawnWithOptions: {}", e)),
        };
        match ctx.read_string(elem) {
            Ok(s) => cmd_args.push(s),
            Err(e) => return NativeCallResult::Error(format!("process.spawnWithOptions: {}", e)),
        }
    }
    // args[2] = cwd string
    let cwd = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.spawnWithOptions: {}", e)),
    };
    // args[3] = env string[] (flat: [KEY, VALUE, KEY, VALUE, ...])
    let env_arr = args[3];
    let env_len = match ctx.array_len(env_arr) {
        Ok(n) => n,
        Err(e) => return NativeCallResult::Error(format!("process.spawnWithOptions: {}", e)),
    };
    let mut env_pairs = Vec::with_capacity(env_len);
    for i in 0..env_len {
        let elem = match ctx.array_get(env_arr, i) {
            Ok(v) => v,
            Err(e) => return NativeCallResult::Error(format!("process.spawnWithOptions: {}", e)),
        };
        match ctx.read_string(elem) {
            Ok(s) => env_pairs.push(s),
            Err(e) => return NativeCallResult::Error(format!("process.spawnWithOptions: {}", e)),
        }
    }

    let mut cmd = Command::new(&command);
    cmd.args(&cmd_args);
    if !cwd.is_empty() {
        cmd.current_dir(&cwd);
    }
    // env_pairs is flat [KEY, VALUE, KEY, VALUE, ...]
    let mut i = 0;
    while i + 1 < env_pairs.len() {
        cmd.env(&env_pairs[i], &env_pairs[i + 1]);
        i += 2;
    }
    spawn_child(cmd)
}

/// Wait for child to exit (blocking), return exit code
pub fn child_wait(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            if let Some(entry) = CHILD_PROCESSES.get(handle) {
                match entry.child.lock().unwrap().wait() {
                    Ok(status) => {
                        IoCompletion::Primitive(NativeValue::f64(status.code().unwrap_or(-1) as f64))
                    }
                    Err(_) => IoCompletion::Primitive(NativeValue::f64(-1.0)),
                }
            } else {
                IoCompletion::Error("Invalid child process handle".to_string())
            }
        }),
    })
}

/// Non-blocking check — returns exit code or -1 if still running
pub fn child_try_wait(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    if let Some(entry) = CHILD_PROCESSES.get(handle) {
        match entry.child.lock().unwrap().try_wait() {
            Ok(Some(status)) => NativeCallResult::f64(status.code().unwrap_or(-1) as f64),
            Ok(None) => NativeCallResult::f64(-1.0), // still running
            Err(_) => NativeCallResult::f64(-1.0),
        }
    } else {
        NativeCallResult::Error("Invalid child process handle".to_string())
    }
}

/// Check if child process is still running
pub fn child_is_alive(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    if let Some(entry) = CHILD_PROCESSES.get(handle) {
        let alive = match entry.child.lock().unwrap().try_wait() {
            Ok(Some(_)) => false,
            Ok(None) => true,
            Err(_) => false,
        };
        NativeCallResult::bool(alive)
    } else {
        NativeCallResult::bool(false)
    }
}

/// Write to child's stdin pipe (blocking)
pub fn child_write_stdin(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.childWriteStdin: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            if let Some(entry) = CHILD_PROCESSES.get(handle) {
                if let Some(ref mut stdin) = *entry.stdin.lock().unwrap() {
                    match stdin.write_all(data.as_bytes()).and_then(|_| stdin.flush()) {
                        Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                        Err(e) => IoCompletion::Error(format!("childWriteStdin: {}", e)),
                    }
                } else {
                    IoCompletion::Error("stdin not available".to_string())
                }
            } else {
                IoCompletion::Error("Invalid child process handle".to_string())
            }
        }),
    })
}

/// Read chunk from child's stdout pipe (blocking)
pub fn child_read_stdout(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            if let Some(entry) = CHILD_PROCESSES.get(handle) {
                if let Some(ref mut stdout) = *entry.stdout.lock().unwrap() {
                    let mut buf = vec![0u8; 8192];
                    match stdout.read(&mut buf) {
                        Ok(0) => IoCompletion::String(String::new()),
                        Ok(n) => {
                            IoCompletion::String(String::from_utf8_lossy(&buf[..n]).to_string())
                        }
                        Err(e) => IoCompletion::Error(format!("childReadStdout: {}", e)),
                    }
                } else {
                    IoCompletion::Error("stdout not available".to_string())
                }
            } else {
                IoCompletion::Error("Invalid child process handle".to_string())
            }
        }),
    })
}

/// Read chunk from child's stderr pipe (blocking)
pub fn child_read_stderr(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            if let Some(entry) = CHILD_PROCESSES.get(handle) {
                if let Some(ref mut stderr) = *entry.stderr.lock().unwrap() {
                    let mut buf = vec![0u8; 8192];
                    match stderr.read(&mut buf) {
                        Ok(0) => IoCompletion::String(String::new()),
                        Ok(n) => {
                            IoCompletion::String(String::from_utf8_lossy(&buf[..n]).to_string())
                        }
                        Err(e) => IoCompletion::Error(format!("childReadStderr: {}", e)),
                    }
                } else {
                    IoCompletion::Error("stderr not available".to_string())
                }
            } else {
                IoCompletion::Error("Invalid child process handle".to_string())
            }
        }),
    })
}

/// Send SIGKILL to child process
pub fn child_kill(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    if let Some(entry) = CHILD_PROCESSES.get(handle) {
        match entry.child.lock().unwrap().kill() {
            Ok(_) => NativeCallResult::null(),
            Err(e) => NativeCallResult::Error(format!("childKill: {}", e)),
        }
    } else {
        NativeCallResult::Error("Invalid child process handle".to_string())
    }
}

/// Send named signal to child process
pub fn child_signal(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    let signal_name = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.childSignal: {}", e)),
    };

    let sig = match signal_name.to_uppercase().as_str() {
        "SIGTERM" | "TERM" => libc::SIGTERM,
        "SIGINT" | "INT" => libc::SIGINT,
        "SIGHUP" | "HUP" => libc::SIGHUP,
        "SIGUSR1" | "USR1" => libc::SIGUSR1,
        "SIGUSR2" | "USR2" => libc::SIGUSR2,
        "SIGKILL" | "KILL" => libc::SIGKILL,
        _ => {
            return NativeCallResult::Error(format!("Unknown signal: {}", signal_name));
        }
    };

    if let Some(entry) = CHILD_PROCESSES.get(handle) {
        let pid = entry.child.lock().unwrap().id() as i32;
        // SAFETY: kill() is safe to call with a valid pid and signal number
        let result = unsafe { libc::kill(pid, sig) };
        if result == 0 {
            NativeCallResult::null()
        } else {
            NativeCallResult::Error("Failed to send signal".to_string())
        }
    } else {
        NativeCallResult::Error("Invalid child process handle".to_string())
    }
}

/// Get child process ID
pub fn child_pid(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args
        .first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    if let Some(entry) = CHILD_PROCESSES.get(handle) {
        let pid = entry.child.lock().unwrap().id();
        NativeCallResult::f64(pid as f64)
    } else {
        NativeCallResult::Error("Invalid child process handle".to_string())
    }
}
