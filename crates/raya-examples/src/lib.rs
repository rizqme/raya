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

/// Absolute path to the todo/kv fixture root.
pub fn todo_kv_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/todo-kv")
}

/// Absolute path to the todo/kv fixture entrypoint.
pub fn todo_kv_entry() -> PathBuf {
    todo_kv_fixture_dir().join("src/main.raya")
}

/// Absolute path to the worker queue fixture root.
pub fn worker_queue_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/worker-queue")
}

/// Absolute path to the worker queue fixture entrypoint.
pub fn worker_queue_entry() -> PathBuf {
    worker_queue_fixture_dir().join("src/main.raya")
}

/// Absolute path to the TCP chat fixture root.
pub fn tcp_chat_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/tcp-chat")
}

/// Absolute path to the TCP chat fixture entrypoint.
pub fn tcp_chat_entry() -> PathBuf {
    tcp_chat_fixture_dir().join("src/main.raya")
}

/// Absolute path to the template/archive pipeline fixture root.
pub fn template_pipeline_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/template-pipeline")
}

/// Absolute path to the template/archive pipeline fixture entrypoint.
pub fn template_pipeline_entry() -> PathBuf {
    template_pipeline_fixture_dir().join("src/main.raya")
}

/// Absolute path to the package workflow fixture root.
pub fn pkg_workflow_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/pkg-workflow")
}

/// Absolute path to the package workflow fixture entrypoint.
pub fn pkg_workflow_entry() -> PathBuf {
    pkg_workflow_fixture_dir().join("src/main.raya")
}

/// Absolute path to the fault injection fixture root.
pub fn fault_injection_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/fault-injection")
}

/// Absolute path to the fault injection fixture entrypoint.
pub fn fault_injection_entry() -> PathBuf {
    fault_injection_fixture_dir().join("src/main.raya")
}
