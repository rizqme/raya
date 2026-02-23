use std::path::PathBuf;

/// Fixed loopback port used by the CLI HTTP E2E example.
pub const WEBAPP_PORT: u16 = 38081;

/// Absolute path to the fixture web application root.
pub fn webapp_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/webapp")
}

/// Absolute path to the fixture server entrypoint.
pub fn webapp_server_entry() -> PathBuf {
    webapp_fixture_dir().join("src/server.raya")
}

/// Absolute path to the fixture client entrypoint.
pub fn webapp_client_entry() -> PathBuf {
    webapp_fixture_dir().join("src/client.raya")
}

/// Absolute path to the fixture stress client entrypoint.
pub fn webapp_stress_client_entry() -> PathBuf {
    webapp_fixture_dir().join("src/stress_client.raya")
}

/// Absolute path to the systems fixture root.
pub fn systems_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/systems")
}

/// Absolute path to the systems stateful data suite entrypoint.
pub fn systems_stateful_entry() -> PathBuf {
    systems_fixture_dir().join("src/stateful_data.raya")
}

/// Absolute path to the systems concurrency/cancellation suite entrypoint.
pub fn systems_concurrency_entry() -> PathBuf {
    systems_fixture_dir().join("src/concurrency_cancel.raya")
}

/// Absolute path to the systems TCP protocol suite entrypoint.
pub fn systems_tcp_entry() -> PathBuf {
    systems_fixture_dir().join("src/tcp_protocol.raya")
}

/// Absolute path to the systems template/archive suite entrypoint.
pub fn systems_pipeline_entry() -> PathBuf {
    systems_fixture_dir().join("src/template_archive.raya")
}

/// Absolute path to the systems fault-injection suite entrypoint.
pub fn systems_fault_entry() -> PathBuf {
    systems_fixture_dir().join("src/fault_injection.raya")
}
