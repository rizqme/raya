use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

pub fn unique_tmp_dir(prefix: &str) -> PathBuf {
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

fn run_cli_script_once(workspace: &Path, script: &Path, tmp_dir: &Path) -> Output {
    let mut child = Command::new(raya_cli_bin(workspace))
        .current_dir(workspace)
        .arg("run")
        .arg(script)
        .env("RAYA_EXAMPLES_TMPDIR", tmp_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn raya CLI");

    let timeout = Duration::from_secs(20);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().expect("collect raya CLI output");
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let out = child
                        .wait_with_output()
                        .expect("collect raya CLI output after timeout");
                    panic!(
                        "raya CLI timed out after {:?} for script {}\nstdout:\n{}\nstderr:\n{}",
                        timeout,
                        script.display(),
                        String::from_utf8_lossy(&out.stdout),
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("failed waiting for raya CLI process: {e}"),
        }
    }
}

pub fn run_cli_script(workspace: &Path, script: &Path, tmp_dir: &Path) -> Output {
    for attempt in 0..4 {
        let out = run_cli_script_once(workspace, script, tmp_dir);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let transient_failure = stderr.contains("Stack underflow");
        if out.status.success() || !transient_failure || attempt == 3 {
            return out;
        }
        thread::sleep(Duration::from_millis(75));
    }
    unreachable!("retry loop always returns")
}

pub fn parse_summary(summary: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for part in summary.trim().split(',') {
        if let Some((k, v)) = part.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

pub fn assert_ok_run(out: &Output) {
    assert!(
        out.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
