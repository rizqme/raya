//! Logger native implementations
//!
//! Provides the Rust-side handlers for `std:logger` native calls.
//! Each method takes a message string and writes to stdout or stderr.

/// Log a debug message to stdout
pub fn debug(message: &str) {
    println!("[DEBUG] {}", message);
}

/// Log an info message to stdout
pub fn info(message: &str) {
    println!("{}", message);
}

/// Log a warning message to stderr
pub fn warn(message: &str) {
    eprintln!("[WARN] {}", message);
}

/// Log an error message to stderr
pub fn error(message: &str) {
    eprintln!("[ERROR] {}", message);
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
}
