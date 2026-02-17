//! std:os — OS information

use raya_engine::vm::{NativeCallResult, NativeContext, NativeValue, string_allocate};

/// Get OS platform name
pub fn platform(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let name = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else {
        "unknown"
    };
    NativeCallResult::Value(string_allocate(ctx, name.to_string()))
}

/// Get CPU architecture
pub fn arch(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let name = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else {
        "unknown"
    };
    NativeCallResult::Value(string_allocate(ctx, name.to_string()))
}

/// Get machine hostname
pub fn hostname(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // Use gethostname via libc-compatible approach
    let name = get_hostname().unwrap_or_else(|| "unknown".to_string());
    NativeCallResult::Value(string_allocate(ctx, name))
}

/// Get number of logical CPUs
pub fn cpus(_ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    NativeCallResult::f64(count as f64)
}

/// Get total system memory in bytes
pub fn total_memory(_ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(get_total_memory() as f64)
}

/// Get free system memory in bytes
pub fn free_memory(_ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(get_free_memory() as f64)
}

/// Get system uptime in seconds
pub fn uptime(_ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(get_uptime() as f64)
}

/// Get OS line ending
pub fn eol(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let ending = if cfg!(target_os = "windows") { "\r\n" } else { "\n" };
    NativeCallResult::Value(string_allocate(ctx, ending.to_string()))
}

/// Get OS temp directory
pub fn tmpdir(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let dir = std::env::temp_dir();
    NativeCallResult::Value(string_allocate(ctx, dir.to_string_lossy().into_owned()))
}

// ── Platform-specific helpers ──

fn get_hostname() -> Option<String> {
    #[cfg(unix)]
    {
        use std::ffi::CStr;
        let mut buf = [0u8; 256];
        let ret = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if ret == 0 {
            let cstr = unsafe { CStr::from_ptr(buf.as_ptr() as *const libc::c_char) };
            cstr.to_str().ok().map(|s| s.to_string())
        } else {
            None
        }
    }
    #[cfg(not(unix))]
    {
        std::env::var("COMPUTERNAME").ok()
    }
}

fn get_total_memory() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use std::mem;
        let mut size: u64 = 0;
        let mut len = mem::size_of::<u64>();
        let mib = [libc::CTL_HW, libc::HW_MEMSIZE];
        let ret = unsafe {
            libc::sysctl(
                mib.as_ptr() as *mut _,
                2,
                &mut size as *mut u64 as *mut _,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if ret == 0 { size } else { 0 }
    }
    #[cfg(target_os = "linux")]
    {
        let info = unsafe {
            let mut info: libc::sysinfo = std::mem::zeroed();
            libc::sysinfo(&mut info);
            info
        };
        info.totalram * info.mem_unit as u64
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

fn get_free_memory() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // On macOS, use vm_statistics to get free pages
        use std::mem;
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
        let mut vm_stat: libc::vm_statistics64 = unsafe { mem::zeroed() };
        let mut info_count = (mem::size_of::<libc::vm_statistics64>() / mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
        let ret = unsafe {
            libc::host_statistics64(
                libc::mach_host_self(),
                libc::HOST_VM_INFO64,
                &mut vm_stat as *mut _ as *mut _,
                &mut info_count,
            )
        };
        if ret == 0 {
            (vm_stat.free_count as u64 + vm_stat.inactive_count as u64) * page_size
        } else {
            0
        }
    }
    #[cfg(target_os = "linux")]
    {
        let info = unsafe {
            let mut info: libc::sysinfo = std::mem::zeroed();
            libc::sysinfo(&mut info);
            info
        };
        info.freeram * info.mem_unit as u64
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

fn get_uptime() -> f64 {
    #[cfg(target_os = "macos")]
    {
        use std::mem;
        let mut boottime: libc::timeval = unsafe { mem::zeroed() };
        let mut len = mem::size_of::<libc::timeval>();
        let mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        let ret = unsafe {
            libc::sysctl(
                mib.as_ptr() as *mut _,
                2,
                &mut boottime as *mut _ as *mut _,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if ret == 0 {
            let now = unsafe {
                let mut tv: libc::timeval = mem::zeroed();
                libc::gettimeofday(&mut tv, std::ptr::null_mut());
                tv
            };
            (now.tv_sec - boottime.tv_sec) as f64
        } else {
            0.0
        }
    }
    #[cfg(target_os = "linux")]
    {
        let info = unsafe {
            let mut info: libc::sysinfo = std::mem::zeroed();
            libc::sysinfo(&mut info);
            info
        };
        info.uptime as f64
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0.0
    }
}
