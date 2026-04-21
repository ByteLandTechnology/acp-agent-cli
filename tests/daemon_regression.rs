use assert_cmd::Command;
use serde_json::json;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn cmd() -> Command {
    Command::cargo_bin(env!("CARGO_PKG_NAME")).expect("binary should exist")
}

fn sandbox_args(temp_dir: &TempDir) -> Vec<String> {
    vec![
        "--config-dir".to_string(),
        temp_dir.path().join("config").display().to_string(),
        "--data-dir".to_string(),
        temp_dir.path().join("data").display().to_string(),
        "--state-dir".to_string(),
        temp_dir.path().join("state").display().to_string(),
        "--cache-dir".to_string(),
        temp_dir.path().join("cache").display().to_string(),
    ]
}

#[cfg(unix)]
#[test]
fn daemon_start_spawns_background_worker_and_creates_live_endpoint() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sandbox = sandbox_args(&temp_dir);

    let start_output = cmd()
        .args(&sandbox)
        .args(["daemon", "start", "--format", "json"])
        .output()
        .expect("failed to execute daemon start");
    assert!(
        start_output.status.success(),
        "expected daemon start exit 0"
    );

    let start_stdout = String::from_utf8(start_output.stdout).expect("non-utf8 output");
    let start_value: serde_json::Value =
        serde_json::from_str(&start_stdout).expect("stdout should be valid JSON");
    let daemon_pid = start_value["daemon_status"]["pid"]
        .as_u64()
        .expect("daemon pid should be present");
    let endpoint = start_value["daemon_status"]["endpoint"]
        .as_str()
        .expect("daemon endpoint should be present");

    assert_ne!(
        daemon_pid,
        u64::from(std::process::id()),
        "daemon start should spawn a distinct background worker"
    );
    assert!(
        Path::new(endpoint).exists(),
        "daemon start should wait for a live endpoint"
    );

    let stop_output = cmd()
        .args(&sandbox)
        .args(["daemon", "stop", "--format", "json"])
        .output()
        .expect("failed to execute daemon stop");
    assert!(stop_output.status.success(), "expected daemon stop exit 0");
    assert!(
        !Path::new(endpoint).exists(),
        "daemon stop should remove the live endpoint"
    );
}

#[test]
fn run_via_daemon_does_not_fallback_to_local_execution_when_endpoint_is_missing() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sandbox = sandbox_args(&temp_dir);
    let daemon_dir = temp_dir.path().join("state").join("daemon");
    fs::create_dir_all(&daemon_dir).expect("failed to create daemon runtime dir");

    fs::write(
        daemon_dir.join("daemon-state.json"),
        serde_json::to_vec_pretty(&json!({
            "state": "running",
            "readiness": "ready",
            "instance_id": "default",
            "pid": 424242,
            "transport": if cfg!(unix) { "unix_socket_or_named_pipe" } else { "loopback_tcp" },
            "endpoint": if cfg!(unix) { daemon_dir.join("daemon.sock").display().to_string() } else { "127.0.0.1:65535".to_string() },
            "started_at_epoch_sec": 1,
            "active_requests": 0,
            "queue_depth": 0,
            "last_error": "",
            "recommended_next_action": "daemon status"
        }))
        .expect("failed to serialize fake daemon state"),
    )
    .expect("failed to write fake daemon state");

    let output = cmd()
        .args(&sandbox)
        .args(["run", "demo-input", "--via", "daemon", "--format", "json"])
        .output()
        .expect("failed to execute daemon-routed run");

    assert!(
        !output.status.success(),
        "run --via daemon should fail when no live daemon endpoint exists"
    );

    let stderr = String::from_utf8(output.stderr).expect("non-utf8 stderr");
    let value: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be valid JSON");
    assert!(
        value["code"] == "daemon.execution_failed" || value["code"] == "run.missing_agent",
        "unexpected error code: {:?}",
        value["code"]
    );

    let status_output = cmd()
        .args(&sandbox)
        .args(["daemon", "status", "--format", "json"])
        .output()
        .expect("failed to execute daemon status");
    assert!(
        status_output.status.success(),
        "expected daemon status exit 0"
    );

    let status_stdout = String::from_utf8(status_output.stdout).expect("non-utf8 output");
    let status_value: serde_json::Value =
        serde_json::from_str(&status_stdout).expect("stdout should be valid JSON");
    assert_eq!(status_value["daemon_status"]["state"], "stopped");
}
