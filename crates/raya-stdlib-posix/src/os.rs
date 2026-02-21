//! std:os — OS information

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

/// Get OS platform name
pub fn platform(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
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
    NativeCallResult::Value(ctx.create_string(name))
}

/// Get CPU architecture
pub fn arch(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
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
    NativeCallResult::Value(ctx.create_string(name))
}

/// Get machine hostname
pub fn hostname(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // Use gethostname via libc-compatible approach
    let name = get_hostname().unwrap_or_else(|| "unknown".to_string());
    NativeCallResult::Value(ctx.create_string(&name))
}

/// Get number of logical CPUs
pub fn cpus(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    NativeCallResult::f64(count as f64)
}

/// Get total system memory in bytes
pub fn total_memory(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(get_total_memory() as f64)
}

/// Get free system memory in bytes
pub fn free_memory(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(get_free_memory() as f64)
}

/// Get system uptime in seconds
pub fn uptime(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(get_uptime())
}

/// Get OS line ending
pub fn eol(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let ending = if cfg!(target_os = "windows") { "\r\n" } else { "\n" };
    NativeCallResult::Value(ctx.create_string(ending))
}

/// Get OS temp directory
pub fn tmpdir(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let dir = std::env::temp_dir();
    NativeCallResult::Value(ctx.create_string(&dir.to_string_lossy()))
}

/// Get OS kernel release string (e.g. "23.4.0")
pub fn release(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Value(ctx.create_string(&uname_field(UnameField::Release)))
}

/// Get OS type / sysname (e.g. "Darwin", "Linux")
pub fn os_type(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Value(ctx.create_string(&uname_field(UnameField::Sysname)))
}

/// Get machine hardware name (e.g. "arm64", "x86_64")
pub fn machine(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Value(ctx.create_string(&uname_field(UnameField::Machine)))
}

/// Get current username from passwd database
pub fn username(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let name = get_pw_field(PwField::Name)
        .unwrap_or_else(|| std::env::var("USER").unwrap_or_default());
    NativeCallResult::Value(ctx.create_string(&name))
}

/// Get user info as flat string array [uid, gid, username, homedir, shell]
pub fn user_info(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let info = get_user_info();
    let items: Vec<NativeValue> = info.iter().map(|s| ctx.create_string(s)).collect();
    NativeCallResult::Value(ctx.create_array(&items))
}

/// Get user's login shell
pub fn shell(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let sh = get_pw_field(PwField::Shell)
        .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_default());
    NativeCallResult::Value(ctx.create_string(&sh))
}

/// Get system load averages [1min, 5min, 15min]
pub fn loadavg(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let mut avg = [0.0f64; 3];
    // SAFETY: getloadavg is safe with a properly sized buffer
    let ret = unsafe { libc::getloadavg(avg.as_mut_ptr(), 3) };
    if ret < 0 {
        let items = [NativeValue::f64(0.0), NativeValue::f64(0.0), NativeValue::f64(0.0)];
        return NativeCallResult::Value(ctx.create_array(&items));
    }
    let items = [
        NativeValue::f64(avg[0]),
        NativeValue::f64(avg[1]),
        NativeValue::f64(avg[2]),
    ];
    NativeCallResult::Value(ctx.create_array(&items))
}

/// Get network interfaces as flat string array [name, addr, family, name, addr, family, ...]
pub fn network_interfaces(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let entries = get_network_interfaces();
    let items: Vec<NativeValue> = entries.iter().map(|s| ctx.create_string(s)).collect();
    NativeCallResult::Value(ctx.create_array(&items))
}

/// Get byte order: "LE" or "BE"
pub fn endianness(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let s = if cfg!(target_endian = "little") { "LE" } else { "BE" };
    NativeCallResult::Value(ctx.create_string(s))
}

/// Get system page size in bytes
pub fn page_size(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: sysconf is safe with _SC_PAGESIZE
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    NativeCallResult::f64(size as f64)
}

// ── Platform-specific helpers ──

enum UnameField { Release, Sysname, Machine }

fn uname_field(field: UnameField) -> String {
    #[cfg(unix)]
    {
        use std::ffi::CStr;
        // SAFETY: uname is safe with a properly zeroed struct
        let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::uname(&mut uts) };
        if ret != 0 {
            return "unknown".to_string();
        }
        let ptr = match field {
            UnameField::Release => uts.release.as_ptr(),
            UnameField::Sysname => uts.sysname.as_ptr(),
            UnameField::Machine => uts.machine.as_ptr(),
        };
        // SAFETY: uname null-terminates all fields
        unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .unwrap_or("unknown")
            .to_string()
    }
    #[cfg(not(unix))]
    {
        let _ = field;
        "unknown".to_string()
    }
}

enum PwField { Name, Shell }

fn get_pw_field(field: PwField) -> Option<String> {
    #[cfg(unix)]
    {
        use std::ffi::CStr;
        // SAFETY: getuid is always safe; getpwuid returns a static pointer valid until next call
        let pw = unsafe { libc::getpwuid(libc::getuid()) };
        if pw.is_null() {
            return None;
        }
        let ptr = match field {
            PwField::Name => unsafe { (*pw).pw_name },
            PwField::Shell => unsafe { (*pw).pw_shell },
        };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: pw_name and pw_shell are null-terminated C strings
        Some(unsafe { CStr::from_ptr(ptr) }.to_str().ok()?.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = field;
        None
    }
}

fn get_user_info() -> Vec<String> {
    #[cfg(unix)]
    {
        use std::ffi::CStr;
        // SAFETY: getuid/getpwuid are safe; pointer is valid until next passwd call
        let uid = unsafe { libc::getuid() };
        let pw = unsafe { libc::getpwuid(uid) };
        if pw.is_null() {
            return vec![
                uid.to_string(),
                "0".to_string(),
                std::env::var("USER").unwrap_or_default(),
                std::env::var("HOME").unwrap_or_default(),
                std::env::var("SHELL").unwrap_or_default(),
            ];
        }
        unsafe {
            let gid = (*pw).pw_gid;
            let name = if (*pw).pw_name.is_null() { String::new() }
                else { CStr::from_ptr((*pw).pw_name).to_str().unwrap_or("").to_string() };
            let dir = if (*pw).pw_dir.is_null() { String::new() }
                else { CStr::from_ptr((*pw).pw_dir).to_str().unwrap_or("").to_string() };
            let shell = if (*pw).pw_shell.is_null() { String::new() }
                else { CStr::from_ptr((*pw).pw_shell).to_str().unwrap_or("").to_string() };
            vec![
                uid.to_string(),
                gid.to_string(),
                name,
                dir,
                shell,
            ]
        }
    }
    #[cfg(not(unix))]
    {
        vec![
            "0".to_string(),
            "0".to_string(),
            std::env::var("USERNAME").unwrap_or_default(),
            std::env::var("USERPROFILE").unwrap_or_default(),
            String::new(),
        ]
    }
}

fn get_network_interfaces() -> Vec<String> {
    #[cfg(unix)]
    {
        use std::ffi::CStr;
        use std::net::{Ipv4Addr, Ipv6Addr};

        let mut entries = Vec::new();
        let mut addrs: *mut libc::ifaddrs = std::ptr::null_mut();

        // SAFETY: getifaddrs allocates and fills the linked list; we free it later
        if unsafe { libc::getifaddrs(&mut addrs) } != 0 {
            return entries;
        }

        let mut cursor = addrs;
        while !cursor.is_null() {
            // SAFETY: cursor is a valid ifaddrs node from getifaddrs
            let ifa = unsafe { &*cursor };
            let name = if ifa.ifa_name.is_null() {
                String::new()
            } else {
                // SAFETY: ifa_name is a null-terminated C string
                unsafe { CStr::from_ptr(ifa.ifa_name) }
                    .to_str()
                    .unwrap_or("")
                    .to_string()
            };

            if !ifa.ifa_addr.is_null() {
                // SAFETY: ifa_addr is valid as checked above
                let family = unsafe { (*ifa.ifa_addr).sa_family } as i32;
                if family == libc::AF_INET {
                    // SAFETY: We checked sa_family == AF_INET, so it's a sockaddr_in
                    let sa = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in) };
                    let ip = Ipv4Addr::from(u32::from_be(sa.sin_addr.s_addr));
                    entries.push(name);
                    entries.push(ip.to_string());
                    entries.push("IPv4".to_string());
                } else if family == libc::AF_INET6 {
                    // SAFETY: We checked sa_family == AF_INET6, so it's a sockaddr_in6
                    let sa = unsafe { &*(ifa.ifa_addr as *const libc::sockaddr_in6) };
                    let ip = Ipv6Addr::from(sa.sin6_addr.s6_addr);
                    entries.push(name);
                    entries.push(ip.to_string());
                    entries.push("IPv6".to_string());
                }
            }

            cursor = ifa.ifa_next;
        }

        // SAFETY: addrs was allocated by getifaddrs and must be freed
        unsafe { libc::freeifaddrs(addrs) };
        entries
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

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
        #[allow(deprecated)] // libc deprecates in favor of mach2 crate, but we use libc
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
