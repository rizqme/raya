//! Logger native implementations
//!
//! Provides the Rust-side handlers for `std:logger` native calls.
//! Supports level filtering, structured data logging, JSON output, timestamps, and prefixes.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, RwLock};

/// Log levels: debug(0) < info(1) < warn(2) < error(3) < silent(4)
const LEVEL_DEBUG: u8 = 0;
const LEVEL_INFO: u8 = 1;
const LEVEL_WARN: u8 = 2;
const LEVEL_ERROR: u8 = 3;
const LEVEL_SILENT: u8 = 4;

/// Current log level (default: debug — show everything)
static LOG_LEVEL: AtomicU8 = AtomicU8::new(LEVEL_DEBUG);

/// Output format: "text" or "json"
static FORMAT: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new("text".to_string()));

/// Whether to include timestamps
static TIMESTAMP_ENABLED: AtomicU8 = AtomicU8::new(0); // 0 = false

/// Prefix string (e.g., "[MyApp]")
static PREFIX: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new(String::new()));

fn level_from_name(name: &str) -> u8 {
    match name.to_lowercase().as_str() {
        "debug" => LEVEL_DEBUG,
        "info" => LEVEL_INFO,
        "warn" | "warning" => LEVEL_WARN,
        "error" => LEVEL_ERROR,
        "silent" | "off" => LEVEL_SILENT,
        _ => LEVEL_INFO,
    }
}

fn level_name(level: u8) -> &'static str {
    match level {
        LEVEL_DEBUG => "debug",
        LEVEL_INFO => "info",
        LEVEL_WARN => "warn",
        LEVEL_ERROR => "error",
        LEVEL_SILENT => "silent",
        _ => "info",
    }
}

fn get_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Simple ISO 8601 without chrono dependency
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    // Approximate date calculation (good enough for logging)
    let (year, month, day) = days_to_date(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn format_message(level: u8, message: &str, data: Option<&str>) {
    let format = FORMAT.read().unwrap().clone();
    let timestamp_on = TIMESTAMP_ENABLED.load(Ordering::Relaxed) != 0;
    let prefix = PREFIX.read().unwrap().clone();

    if format == "json" {
        let mut json = format!(
            "{{\"level\":\"{}\",\"message\":\"{}\"",
            level_name(level),
            message.replace('\\', "\\\\").replace('"', "\\\"")
        );
        if !prefix.is_empty() {
            json.push_str(&format!(
                ",\"prefix\":\"{}\"",
                prefix.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        }
        if timestamp_on {
            json.push_str(&format!(",\"timestamp\":\"{}\"", get_timestamp()));
        }
        if let Some(d) = data {
            json.push_str(&format!(",\"data\":{}", d));
        }
        json.push('}');
        if level >= LEVEL_WARN {
            eprintln!("{}", json);
        } else {
            println!("{}", json);
        }
    } else {
        // Text format
        let mut parts = Vec::new();
        if timestamp_on {
            parts.push(get_timestamp());
        }
        if !prefix.is_empty() {
            parts.push(prefix);
        }
        let level_tag = match level {
            LEVEL_DEBUG => "[DEBUG]",
            LEVEL_INFO => "[INFO]",
            LEVEL_WARN => "[WARN]",
            LEVEL_ERROR => "[ERROR]",
            _ => "",
        };
        // For info in text mode, omit the tag for clean output (original behavior)
        if level != LEVEL_INFO || timestamp_on || !PREFIX.read().unwrap().is_empty() {
            parts.push(level_tag.to_string());
        }
        parts.push(message.to_string());
        if let Some(d) = data {
            parts.push(d.to_string());
        }
        let output = parts.join(" ");
        if level >= LEVEL_WARN {
            eprintln!("{}", output);
        } else {
            println!("{}", output);
        }
    }
}

/// Log a debug message to stdout
pub fn debug(message: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) == LEVEL_DEBUG {
        format_message(LEVEL_DEBUG, message, None);
    }
}

/// Log an info message to stdout
pub fn info(message: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) <= LEVEL_INFO {
        format_message(LEVEL_INFO, message, None);
    }
}

/// Log a warning message to stderr
pub fn warn(message: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) <= LEVEL_WARN {
        format_message(LEVEL_WARN, message, None);
    }
}

/// Log an error message to stderr
pub fn error(message: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) <= LEVEL_ERROR {
        format_message(LEVEL_ERROR, message, None);
    }
}

/// Log a debug message with structured data
pub fn debug_data(message: &str, data: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) == LEVEL_DEBUG {
        format_message(LEVEL_DEBUG, message, Some(data));
    }
}

/// Log an info message with structured data
pub fn info_data(message: &str, data: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) <= LEVEL_INFO {
        format_message(LEVEL_INFO, message, Some(data));
    }
}

/// Log a warning message with structured data
pub fn warn_data(message: &str, data: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) <= LEVEL_WARN {
        format_message(LEVEL_WARN, message, Some(data));
    }
}

/// Log an error message with structured data
pub fn error_data(message: &str, data: &str) {
    if LOG_LEVEL.load(Ordering::Relaxed) <= LEVEL_ERROR {
        format_message(LEVEL_ERROR, message, Some(data));
    }
}

/// Set log level
pub fn set_level(name: &str) {
    LOG_LEVEL.store(level_from_name(name), Ordering::Relaxed);
}

/// Get current log level name
pub fn get_level() -> &'static str {
    level_name(LOG_LEVEL.load(Ordering::Relaxed))
}

/// Set output format ("text" or "json")
pub fn set_format(format: &str) {
    *FORMAT.write().unwrap() = format.to_string();
}

/// Enable or disable timestamp
pub fn set_timestamp(enabled: bool) {
    TIMESTAMP_ENABLED.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
}

/// Set prefix string
pub fn set_prefix(prefix: &str) {
    *PREFIX.write().unwrap() = prefix.to_string();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_does_not_panic() {
        info("hello world");
    }

    #[test]
    fn test_debug_does_not_panic() {
        debug("debug msg");
    }

    #[test]
    fn test_warn_does_not_panic() {
        warn("warning msg");
    }

    #[test]
    fn test_error_does_not_panic() {
        error("error msg");
    }

    #[test]
    fn test_level_filtering() {
        set_level("warn");
        // debug and info should be suppressed (no panic, just silently skipped)
        debug("should not appear");
        info("should not appear");
        warn("should appear");
        error("should appear");
        set_level("debug"); // reset
    }

    #[test]
    fn test_level_roundtrip() {
        set_level("error");
        assert_eq!(get_level(), "error");
        set_level("debug");
        assert_eq!(get_level(), "debug");
    }
}
