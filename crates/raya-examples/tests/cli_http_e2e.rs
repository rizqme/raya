use raya_examples::{webapp_client_entry, webapp_server_entry, webapp_stress_client_entry};
use serde_json::Value;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
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
        if let Some(path) = std::env::var_os("RAYA_CLI_BIN") {
            let p = PathBuf::from(path);
            assert!(p.exists(), "RAYA_CLI_BIN does not exist: {}", p.display());
            return p;
        }

        let bin = workspace.join("target").join("debug").join("raya");
        if bin.exists() {
            return bin;
        }

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

        bin
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

fn run_cli_and_capture_env(
    workspace: &Path,
    script: &Path,
    tmp_dir: &Path,
    envs: &[(&str, &str)],
) -> Output {
    let mut cmd = Command::new(raya_cli_bin(workspace));
    cmd.current_dir(workspace)
        .arg("run")
        .arg(script)
        .env("RAYA_EXAMPLES_TMPDIR", tmp_dir);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("run raya CLI binary")
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

fn wait_for_ready_addr(path: &Path, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(raw) = std::fs::read_to_string(path) {
            let addr = raw.trim();
            if !addr.is_empty() && addr.parse::<SocketAddr>().is_ok() {
                return Some(addr.to_string());
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    None
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

fn app_dir(tmp_dir: &Path) -> PathBuf {
    tmp_dir.join("raya-examples-webapp")
}

fn boot_server(workspace: &Path, tmp_dir: &Path) -> Child {
    let ready_file = tmp_dir.join("server.ready");
    let mut server = spawn_cli_run(workspace, &webapp_server_entry(), tmp_dir);
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
    if wait_for_ready_addr(&ready_file, Duration::from_secs(120)).is_none() {
        let _ = server.kill();
        let output = server.wait_with_output().expect("server output");
        let ready_contents = std::fs::read_to_string(&ready_file).unwrap_or_default();
        panic!(
            "server readiness address not valid in file: {}\ncontents: {:?}\nstdout:\n{}\nstderr:\n{}",
            ready_file.display(),
            ready_contents,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    server
}

fn shutdown_server(workspace: &Path, tmp_dir: &Path, mut server: Child) -> Output {
    let shutdown = run_cli_and_capture_env(
        workspace,
        &webapp_client_entry(),
        tmp_dir,
        &[("RAYA_EXAMPLES_CLIENT_ROUTE", "/shutdown")],
    );
    assert!(
        shutdown.status.success(),
        "shutdown client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&shutdown.stdout),
        String::from_utf8_lossy(&shutdown.stderr)
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
    server_output
}

#[test]
fn e2e_cli_http_stress_workflow() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("cli-http-stress");
    let mut server = boot_server(&workspace, &tmp_dir);

    let stress = run_cli_and_capture_env(&workspace, &webapp_stress_client_entry(), &tmp_dir, &[]);
    assert!(
        stress.status.success(),
        "stress client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stress.stdout),
        String::from_utf8_lossy(&stress.stderr)
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

    let app_dir = app_dir(&tmp_dir);
    assert!(app_dir.join("health.csv").exists(), "health.csv missing");
    assert!(app_dir.join("health.tar").exists(), "health.tar missing");
    assert!(app_dir.join("health.ok").exists(), "health.ok missing");
    assert!(
        app_dir.join("diag.result.json").exists(),
        "diag.result.json missing"
    );
    assert!(app_dir.join("request.log").exists(), "request.log missing");
    assert!(
        app_dir.join("stress.result.txt").exists(),
        "stress.result.txt missing"
    );

    let stress_result =
        std::fs::read_to_string(app_dir.join("stress.result.txt")).expect("read stress.result.txt");
    assert!(
        stress_result.contains("health=true")
            && stress_result.contains("diag=true")
            && stress_result.contains("echo=true")
            && stress_result.contains("missing=true")
            && stress_result.contains("shutdown=true"),
        "stress result indicates failure: {stress_result}"
    );

    let diag_text =
        std::fs::read_to_string(app_dir.join("diag.result.json")).expect("read diag.result.json");
    let diag: Value = serde_json::from_str(&diag_text).expect("parse diag json");
    assert_eq!(diag["ok"], Value::Bool(true));
    assert_eq!(diag["allPass"], Value::Bool(true));
    assert_eq!(diag["rowCount"].as_f64(), Some(2.0));
    assert!(diag["hash"]
        .as_str()
        .map(|s| s.len() == 64)
        .unwrap_or(false));

    let checks = diag["checks"].as_object().expect("checks object");
    for key in [
        "csv", "math", "crypto", "time", "archive", "glob", "base32", "semver", "url", "template",
        "compress", "env", "process", "os", "stream",
    ] {
        assert_eq!(
            checks.get(key),
            Some(&Value::Bool(true)),
            "check {key} failed"
        );
    }

    let request_log = std::fs::read_to_string(app_dir.join("request.log")).expect("request log");
    assert!(request_log.contains("GET /health"));
    assert!(request_log.contains("GET /diag"));
    assert!(request_log.contains("POST /echo"));
    assert!(request_log.contains("GET /missing"));
    assert!(request_log.contains("GET /shutdown"));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_cli_http_diag_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("cli-http-diag");
    let mut server = boot_server(&workspace, &tmp_dir);

    let diag = run_cli_and_capture_env(
        &workspace,
        &webapp_client_entry(),
        &tmp_dir,
        &[("RAYA_EXAMPLES_CLIENT_ROUTE", "/diag?mode=contract")],
    );
    assert!(
        diag.status.success(),
        "diag client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&diag.stdout),
        String::from_utf8_lossy(&diag.stderr)
    );

    shutdown_server(&workspace, &tmp_dir, server);

    let app_dir = app_dir(&tmp_dir);
    let diag_result =
        std::fs::read_to_string(app_dir.join("client.diag.txt")).expect("client.diag.txt");
    assert!(diag_result.starts_with("200:OK:"));
    assert!(diag_result.contains("\"allPass\": true"));
    assert!(diag_result.contains("\"query\": \"mode=contract\""));
    let diag_raw = std::fs::read_to_string(app_dir.join("diag.client.body.json"))
        .expect("diag.client.body.json");
    assert!(diag_raw.contains("\"ok\": true"));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_cli_http_echo_and_not_found_contracts() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("cli-http-echo404");
    let mut server = boot_server(&workspace, &tmp_dir);

    let echo = run_cli_and_capture_env(
        &workspace,
        &webapp_client_entry(),
        &tmp_dir,
        &[
            ("RAYA_EXAMPLES_CLIENT_ROUTE", "/echo"),
            ("RAYA_EXAMPLES_CLIENT_METHOD", "POST"),
            ("RAYA_EXAMPLES_CLIENT_BODY", "payload-e2e"),
            ("RAYA_EXAMPLES_CLIENT_HEADERS", "X-Trace: e2e-123"),
        ],
    );
    assert!(
        echo.status.success(),
        "echo client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&echo.stdout),
        String::from_utf8_lossy(&echo.stderr)
    );

    let not_found = run_cli_and_capture_env(
        &workspace,
        &webapp_client_entry(),
        &tmp_dir,
        &[("RAYA_EXAMPLES_CLIENT_ROUTE", "/missing")],
    );
    assert!(
        not_found.status.success(),
        "missing client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&not_found.stdout),
        String::from_utf8_lossy(&not_found.stderr)
    );

    shutdown_server(&workspace, &tmp_dir, server);

    let app_dir = app_dir(&tmp_dir);
    let echo_out =
        std::fs::read_to_string(app_dir.join("client.echo.txt")).expect("client.echo.txt");
    assert!(echo_out.starts_with("201:Created:"));
    let echo_json = echo_out.splitn(3, ':').nth(2).expect("echo body json");
    let echo: Value = serde_json::from_str(echo_json).expect("parse echo json");
    assert_eq!(echo["method"], Value::from("POST"));
    assert_eq!(echo["path"], Value::from("/echo"));
    assert_eq!(echo["body"], Value::from("payload-e2e"));
    assert_eq!(echo["trace"], Value::from("e2e-123"));
    assert!(echo["bodyHash"]
        .as_str()
        .map(|s| !s.is_empty())
        .unwrap_or(false));

    let missing_out =
        std::fs::read_to_string(app_dir.join("client.missing.txt")).expect("client.missing.txt");
    assert_eq!(missing_out.trim(), "404:Not Found:not-found");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_cli_http_health_contract_and_artifacts() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("cli-http-health");
    let server = boot_server(&workspace, &tmp_dir);

    let health = run_cli_and_capture_env(&workspace, &webapp_client_entry(), &tmp_dir, &[]);
    assert!(
        health.status.success(),
        "health client failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&health.stdout),
        String::from_utf8_lossy(&health.stderr)
    );

    shutdown_server(&workspace, &tmp_dir, server);

    let app_dir = app_dir(&tmp_dir);
    let health_out =
        std::fs::read_to_string(app_dir.join("client.health.txt")).expect("client.health.txt");
    assert_eq!(health_out.trim(), "200:OK:OK");
    assert!(app_dir.join("health.csv").exists(), "health.csv missing");
    assert!(app_dir.join("health.tar").exists(), "health.tar missing");
    assert!(app_dir.join("health.ok").exists(), "health.ok missing");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_cli_http_echo_method_not_allowed_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("cli-http-echo405");
    let server = boot_server(&workspace, &tmp_dir);

    let echo_get = run_cli_and_capture_env(
        &workspace,
        &webapp_client_entry(),
        &tmp_dir,
        &[("RAYA_EXAMPLES_CLIENT_ROUTE", "/echo")],
    );
    assert!(
        echo_get.status.success(),
        "client process failed unexpectedly\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&echo_get.stdout),
        String::from_utf8_lossy(&echo_get.stderr)
    );

    shutdown_server(&workspace, &tmp_dir, server);

    let app_dir = app_dir(&tmp_dir);
    let echo_out =
        std::fs::read_to_string(app_dir.join("client.echo.txt")).expect("client.echo.txt");
    assert!(echo_out.starts_with("405:"));
    assert!(echo_out.ends_with(":method-not-allowed"));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
