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
