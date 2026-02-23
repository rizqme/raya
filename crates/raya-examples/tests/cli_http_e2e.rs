use raya_examples::{webapp_client_entry, webapp_server_entry};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn unique_tmp_dir(prefix: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("raya-examples-{}-{}", prefix, ts));
    std::fs::create_dir_all(&dir).expect("create tmp dir");
    dir
}

fn raya_cli_bin(workspace: &Path) -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let build = Command::new("cargo")
            .current_dir(workspace)
            .arg("build")
            .arg("-q")
            .arg("-p")
            .arg("raya-cli")
            .env("RUSTFLAGS", "-Awarnings")
            .output()
            .expect("build raya-cli");
        assert!(
            build.status.success(),
            "failed to build raya-cli\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );

        workspace.join("target").join("debug").join("raya")
    })
    .clone()
}

fn spawn_cli_run(workspace: &Path, script: &Path, tmp_dir: &Path) -> Child {
    Command::new(raya_cli_bin(workspace))
        .current_dir(workspace)
        .arg("run")
        .arg(script)
        .env("RAYA_EXAMPLES_TMPDIR", tmp_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn raya CLI")
}

fn run_cli_and_capture(workspace: &Path, script: &Path, tmp_dir: &Path) -> std::process::Output {
    Command::new(raya_cli_bin(workspace))
        .current_dir(workspace)
        .arg("run")
        .arg(script)
        .env("RAYA_EXAMPLES_TMPDIR", tmp_dir)
        .output()
        .expect("run raya CLI binary")
}

fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    false
}

fn wait_for_status(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return Some(status);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn e2e_cli_http_roundtrip() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("cli-http");
    let ready_file = tmp_dir.join("server.ready");

    let mut server = spawn_cli_run(&workspace, &webapp_server_entry(), &tmp_dir);

    if !wait_for_file(&ready_file, Duration::from_secs(120)) {
        let _ = server.kill();
        let output = server.wait_with_output().expect("server output");
        panic!(
            "server readiness file not created: {}\nstdout:\n{}\nstderr:\n{}",
            ready_file.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let client = run_cli_and_capture(&workspace, &webapp_client_entry(), &tmp_dir);
    assert!(
        client.status.success(),
        "client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&client.stdout),
        String::from_utf8_lossy(&client.stderr)
    );

    let server_status = match wait_for_status(&mut server, Duration::from_secs(120)) {
        Some(s) => s,
        None => {
            let _ = server.kill();
            panic!("server did not exit within timeout");
        }
    };

    let server_output = server.wait_with_output().expect("server output");
    assert!(
        server_status.success(),
        "server failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&server_output.stdout),
        String::from_utf8_lossy(&server_output.stderr)
    );

    let app_dir = tmp_dir.join("raya-examples-webapp");
    assert!(app_dir.join("health.csv").exists(), "health.csv missing");
    assert!(app_dir.join("health.tar").exists(), "health.tar missing");
    assert!(app_dir.join("health.ok").exists(), "health.ok missing");
    assert!(
        app_dir.join("client.result.txt").exists(),
        "client.result.txt missing"
    );

    let client_result =
        std::fs::read_to_string(app_dir.join("client.result.txt")).expect("read client.result.txt");
    assert_eq!(client_result.trim(), "200:OK");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
