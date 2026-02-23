use raya_examples::{
    systems_concurrency_entry, systems_fault_entry, systems_pipeline_entry, systems_stateful_entry,
    systems_tcp_entry,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

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

fn run_cli_script(workspace: &Path, script: &Path, tmp_dir: &Path) -> Output {
    Command::new(raya_cli_bin(workspace))
        .current_dir(workspace)
        .arg("run")
        .arg(script)
        .env("RAYA_EXAMPLES_TMPDIR", tmp_dir)
        .output()
        .expect("run raya CLI")
}

fn systems_dir(tmp: &Path) -> PathBuf {
    tmp.join("raya-examples-systems")
}

#[test]
fn e2e_systems_stateful_data_suite() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("systems-stateful");
    let script = systems_stateful_entry();

    let first = run_cli_script(&workspace, &script, &tmp_dir);
    assert!(
        first.status.success(),
        "first run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run_cli_script(&workspace, &script, &tmp_dir);
    assert!(
        second.status.success(),
        "second run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    let summary = std::fs::read_to_string(systems_dir(&tmp_dir).join("stateful.result.txt"))
        .expect("stateful.result.txt");
    assert!(summary.contains("ok=true"), "bad summary: {summary}");
    assert!(summary.contains("rows="), "missing rows in summary: {summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_systems_concurrency_cancel_suite() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("systems-concurrency");
    let script = systems_concurrency_entry();

    let out = run_cli_script(&workspace, &script, &tmp_dir);
    assert!(
        out.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let summary = std::fs::read_to_string(systems_dir(&tmp_dir).join("concurrency.result.txt"))
        .expect("concurrency.result.txt");
    assert!(summary.contains("ok=true"), "bad summary: {summary}");
    assert!(summary.contains("cancelled=true"), "task cancellation not observed: {summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_systems_tcp_protocol_suite() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("systems-tcp");
    let script = systems_tcp_entry();

    let out = run_cli_script(&workspace, &script, &tmp_dir);
    assert!(
        out.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let summary =
        std::fs::read_to_string(systems_dir(&tmp_dir).join("tcp.result.txt")).expect("tcp.result.txt");
    assert!(summary.contains("ok=true"), "tcp protocol summary: {summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_systems_template_archive_suite() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("systems-pipeline");
    let script = systems_pipeline_entry();

    let out = run_cli_script(&workspace, &script, &tmp_dir);
    assert!(
        out.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let summary = std::fs::read_to_string(systems_dir(&tmp_dir).join("pipeline.result.txt"))
        .expect("pipeline.result.txt");
    assert!(summary.contains("ok=true"), "pipeline summary: {summary}");
    assert!(
        systems_dir(&tmp_dir).join("report.tar").exists(),
        "report.tar missing"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_systems_package_workflow_suite() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("systems-pkgflow");
    let project = tmp_dir.join("pkgflow");
    std::fs::create_dir_all(project.join("src")).expect("create src");
    std::fs::write(
        project.join("raya.toml"),
        r#"[package]
name = "pkgflow"
version = "0.1.0"
main = "src/main.raya"

[scripts]
smoke = "src/main.raya"
"#,
    )
    .expect("write manifest");
    std::fs::write(
        project.join("src/main.raya"),
        r#"import env from "std:env";
import fs from "std:fs";
import path from "std:path";
const root = env.get("RAYA_EXAMPLES_TMPDIR");
const dir = path.join(root, "raya-examples-systems");
const out = path.join(dir, "pkgflow.result.txt");
if (!fs.exists(dir)) {
    fs.mkdirRecursive(dir);
}
fs.writeTextFile(out, "ok=true");
"#,
    )
    .expect("write main");

    let out = Command::new(raya_cli_bin(&workspace))
        .current_dir(&project)
        .arg("run")
        .arg("smoke")
        .env("RAYA_EXAMPLES_TMPDIR", &tmp_dir)
        .output()
        .expect("run script alias");
    assert!(
        out.status.success(),
        "pkg workflow failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let summary = std::fs::read_to_string(systems_dir(&tmp_dir).join("pkgflow.result.txt"))
        .expect("pkgflow.result.txt");
    assert_eq!(summary.trim(), "ok=true");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn e2e_systems_fault_injection_suite() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("systems-fault");
    let script = systems_fault_entry();

    let out = run_cli_script(&workspace, &script, &tmp_dir);
    assert!(
        out.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let summary = std::fs::read_to_string(systems_dir(&tmp_dir).join("fault.result.txt"))
        .expect("fault.result.txt");
    assert!(summary.contains("ok=true"), "fault summary: {summary}");
    assert!(summary.contains("json=true"), "json fault not captured: {summary}");
    assert!(summary.contains("hex=true"), "hex fault not captured: {summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
