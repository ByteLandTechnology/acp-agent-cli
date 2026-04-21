//! Baseline CLI contract and integration tests for the generated package
//! layout. Overlay-specific fixtures stay package-local to the generated
//! project when enabled.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

fn assert_man_like_help(stdout: &str) {
    let required_sections = [
        "NAME",
        "SYNOPSIS",
        "DESCRIPTION",
        "OPTIONS",
        "FORMATS",
        "EXAMPLES",
        "EXIT CODES",
    ];

    let lines: Vec<&str> = stdout.lines().collect();
    let mut next_line_index = 0;
    for section in required_sections {
        let relative_index = lines[next_line_index..]
            .iter()
            .position(|line| *line == section)
            .unwrap_or_else(|| panic!("missing help section {section:?}"));
        next_line_index += relative_index + 1;
    }
}

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

/// Create a mock ACP agent script that responds to JSON-RPC protocol.
fn create_mock_agent(temp_dir: &TempDir) -> std::path::PathBuf {
    let script_path = temp_dir.path().join("mock-agent");
    let script_content = r#"#!/usr/bin/env python3
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    method = msg.get("method", "")
    mid = msg.get("id")
    if method == "initialize":
        r = {"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": "0.1.0", "agentCapabilities": {}, "agentInfo": {"name": "mock", "version": "0.0.0"}}}
        print(json.dumps(r), flush=True)
    elif method == "notifications/initialized":
        pass
    elif method == "session/new":
        r = {"jsonrpc": "2.0", "id": mid, "result": {"sessionId": "test-1"}}
        print(json.dumps(r), flush=True)
    elif method == "session/prompt":
        msgs = msg.get("params", {}).get("messages", [])
        content = msgs[0]["parts"][0]["content"] if msgs else ""
        r = {"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn", "messages": [{"role": "agent", "parts": [{"type": "text", "content": "echo: " + content}]}]}}
        print(json.dumps(r), flush=True)
"#;
    std::fs::write(&script_path, script_content).expect("write mock agent");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod mock agent");
    }
    script_path
}

#[test]
fn test_acp_agent_cli_version_prints_semver() {
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"\d+\.\d+\.\d+").unwrap());
}

#[test]
fn test_acp_agent_cli_top_level_auto_help_exits_zero() {
    let output = cmd().output().expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    assert_man_like_help(&stdout);
    assert!(stdout.contains("Available subcommands"));
    assert!(stdout.contains("--version"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("daemon"));
}

#[test]
fn test_acp_agent_cli_non_leaf_auto_help_exits_zero() {
    let output = cmd().arg("context").output().expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    assert_man_like_help(&stdout);
    assert!(stdout.contains("Available subcommands"));
    assert!(stdout.contains("show"));
    assert!(stdout.contains("use"));
}

#[test]
fn test_acp_agent_cli_help_flag_stays_human_readable_even_with_json_format() {
    let output = cmd()
        .args(["run", "--help", "--format", "json"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    assert_man_like_help(&stdout);
    assert!(!stdout.trim_start().starts_with('{'));
}

#[test]
fn test_acp_agent_cli_structured_help_yaml() {
    let output = cmd()
        .args(["help", "run", "--format", "yaml"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: serde_yaml::Value =
        serde_yaml::from_str(&stdout).expect("stdout should be valid YAML");

    assert_eq!(value["command_path"][0], "run");
    assert!(value["runtime_directories"].is_mapping());
    assert!(value["active_context"].is_mapping());
}

#[test]
fn test_acp_agent_cli_structured_help_json() {
    let output = cmd()
        .args(["help", "context", "use", "--format", "json"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(value["command_path"][0], "context");
    assert_eq!(value["command_path"][1], "use");
}

#[test]
fn test_acp_agent_cli_structured_help_toml() {
    let output = cmd()
        .args(["help", "paths", "--format", "toml"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: toml::Value = stdout.parse().expect("stdout should be valid TOML");

    assert_eq!(
        value
            .get("command_path")
            .and_then(|path| path.as_array())
            .and_then(|path| path.first())
            .and_then(|entry| entry.as_str()),
        Some("paths")
    );
}

#[test]
fn test_acp_agent_cli_missing_leaf_input_returns_structured_yaml_error() {
    let output = cmd().arg("run").output().expect("failed to execute");

    assert!(!output.status.success(), "expected non-zero exit");

    let stderr = String::from_utf8(output.stderr).expect("non-utf8 stderr");
    let value: serde_yaml::Value =
        serde_yaml::from_str(&stderr).expect("stderr should be valid YAML");

    assert_eq!(value["code"], "run.missing_input");
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("requires <INPUT>")
    );
    assert!(!stderr.contains("NAME\n"));
}

#[test]
fn test_acp_agent_cli_missing_leaf_input_returns_structured_json_error() {
    let output = cmd()
        .args(["run", "--format", "json"])
        .output()
        .expect("failed to execute");

    assert!(!output.status.success(), "expected non-zero exit");

    let stderr = String::from_utf8(output.stderr).expect("non-utf8 stderr");
    let value: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be valid JSON");

    assert_eq!(value["code"], "run.missing_input");
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("requires <INPUT>")
    );
    assert!(!stderr.contains("NAME\n"));
}

#[test]
fn test_acp_agent_cli_paths_reports_user_scoped_defaults() {
    let output = cmd().arg("paths").output().expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: serde_yaml::Value =
        serde_yaml::from_str(&stdout).expect("stdout should be valid YAML");

    assert_eq!(value["scope"], "user_scoped_default");
    assert!(!value["config_dir"].as_str().unwrap().is_empty());
}

#[test]
fn test_acp_agent_cli_context_use_and_show_round_trip() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sandbox = sandbox_args(&temp_dir);

    cmd()
        .args(&sandbox)
        .args([
            "context",
            "use",
            "--selector",
            "workspace=demo",
            "--selector",
            "provider=staging",
        ])
        .assert()
        .success();

    let output = cmd()
        .args(&sandbox)
        .args(["context", "show"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: serde_yaml::Value =
        serde_yaml::from_str(&stdout).expect("stdout should be valid YAML");

    assert_eq!(
        value["persisted_context"]["selectors"]["workspace"],
        serde_yaml::Value::from("demo")
    );
    assert_eq!(
        value["effective_context"]["effective_values"]["provider"],
        serde_yaml::Value::from("staging")
    );
}

#[test]
fn test_acp_agent_cli_explicit_run_override_does_not_mutate_persisted_context() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mock_agent = create_mock_agent(&temp_dir);
    let sandbox = sandbox_args(&temp_dir);

    cmd()
        .args(&sandbox)
        .args([
            "context",
            "use",
            "--selector",
            "workspace=demo",
            "--selector",
            "provider=staging",
        ])
        .assert()
        .success();

    let agent_flag = format!("--agent={}", mock_agent.display());
    let run_output = cmd()
        .args(&sandbox)
        .args([
            "run",
            "demo-input",
            agent_flag.as_str(),
            "--selector",
            "provider=preview",
        ])
        .output()
        .expect("failed to execute");

    assert!(run_output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(run_output.stdout).expect("non-utf8 output");
    let run_value: serde_yaml::Value =
        serde_yaml::from_str(&stdout).expect("stdout should be valid YAML");
    assert_eq!(
        run_value["effective_context"]["provider"],
        serde_yaml::Value::from("preview")
    );

    let show_output = cmd()
        .args(&sandbox)
        .args(["context", "show"])
        .output()
        .expect("failed to execute");

    assert!(show_output.status.success(), "expected exit 0");

    let show_stdout = String::from_utf8(show_output.stdout).expect("non-utf8 output");
    let show_value: serde_yaml::Value =
        serde_yaml::from_str(&show_stdout).expect("stdout should be valid YAML");
    assert_eq!(
        show_value["effective_context"]["effective_values"]["provider"],
        serde_yaml::Value::from("staging")
    );
}

#[test]
fn test_acp_agent_cli_help_daemon_json() {
    let output = cmd()
        .args(["help", "daemon", "--format", "json"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(value["command_path"][0], "daemon");
    assert_eq!(value["daemon_contract"]["mode"], "app_server");
    assert_eq!(
        value["daemon_contract"]["runtime_artifact_files"]
            .as_array()
            .and_then(|files| files.last())
            .and_then(|file| file.as_str()),
        Some("auth.token_when_tcp_enabled")
    );
}

#[test]
fn test_acp_agent_cli_daemon_start_status_run_stop() {
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
    let start_status = start_value["status"].as_str().unwrap();
    assert_eq!(start_status, "running");
    assert!(
        start_value["daemon_status"]["runtime_artifacts"]["auth_token_file"]
            .as_str()
            .unwrap()
            .ends_with("auth.token_when_tcp_enabled")
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
    assert_eq!(status_value["status"], "ok");
    assert!(status_value["daemon_status"]["endpoint"].is_string());
    assert!(status_value["daemon_status"]["transport"].is_string());
    assert!(status_value["daemon_status"]["readiness"].is_string());

    let run_output = cmd()
        .args(&sandbox)
        .args([
            "run",
            "demo-input",
            "--via",
            "daemon",
            "--ensure-daemon",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to execute daemon-routed run");
    // Daemon run without an agent configured — expect missing_agent or execution_failed error
    let run_stderr = String::from_utf8(run_output.stderr).expect("non-utf8 stderr");
    assert!(
        run_stderr.contains("daemon.missing_agent")
            || run_stderr.contains("daemon.execution_failed")
            || run_stderr.contains("no agent specified"),
        "expected daemon agent error, got: {run_stderr}"
    );

    let stop_output = cmd()
        .args(&sandbox)
        .args(["daemon", "stop", "--format", "json"])
        .output()
        .expect("failed to execute daemon stop");
    assert!(stop_output.status.success(), "expected daemon stop exit 0");

    let stop_stdout = String::from_utf8(stop_output.stdout).expect("non-utf8 output");
    let stop_value: serde_json::Value =
        serde_json::from_str(&stop_stdout).expect("stdout should be valid JSON");
    let stop_status = stop_value["status"].as_str().unwrap();
    assert_eq!(stop_status, "stopped");
}

#[test]
fn test_acp_agent_cli_structured_help_for_planned_command_path() {
    let output = cmd()
        .args(["help", "agent", "status", "--format", "json"])
        .output()
        .expect("failed to execute planned help");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(value["command_path"][0], "agent");
    assert_eq!(value["command_path"][1], "status");
}

#[test]
fn test_acp_agent_cli_planned_command_accepts_global_format_after_path() {
    let output = cmd()
        .args(["agent", "status", "--format", "json"])
        .output()
        .expect("failed to execute planned command");

    // Without --agent, external commands now attempt real ACP dispatch and
    // produce a structured error on stderr (exit 2).
    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8(output.stderr).expect("non-utf8 stderr");
    let value: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be valid JSON");
    assert_eq!(value["code"], "external.missing_agent");
}

#[test]
fn test_acp_agent_cli_repl_help_is_plain_text() {
    let output = cmd()
        .args(["repl", "--help"])
        .output()
        .expect("failed to execute repl help");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    assert_man_like_help(&stdout);
    assert!(stdout.contains("REPL model"));
}

#[test]
fn test_acp_agent_cli_daemonizable_external_command_requires_running_daemon() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sandbox = sandbox_args(&temp_dir);

    let output = cmd()
        .args(&sandbox)
        .args([
            "--via",
            "daemon",
            "session",
            "create",
            "/tmp/demo",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to execute daemon-routed external command");

    assert!(!output.status.success(), "expected non-zero exit");

    let stderr = String::from_utf8(output.stderr).expect("non-utf8 stderr");
    let value: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be valid JSON");
    assert_eq!(value["code"], "daemon.execution_failed");
}

#[test]
fn test_acp_agent_cli_daemonizable_external_command_supports_ensure_daemon() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sandbox = sandbox_args(&temp_dir);

    let output = cmd()
        .args(&sandbox)
        .args([
            "--via",
            "daemon",
            "--ensure-daemon",
            "session",
            "create",
            "/tmp/demo",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to execute daemon-routed external command with ensure");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(value["command"], "session create");
    assert_eq!(value["data"]["routing"], "daemon");
}

#[test]
fn test_acp_agent_cli_ensure_daemon_requires_via_daemon() {
    let output = cmd()
        .args(["run", "demo-input", "--ensure-daemon", "--format", "json"])
        .output()
        .expect("failed to execute invalid daemon routing");

    assert!(!output.status.success(), "expected non-zero exit");

    let stderr = String::from_utf8(output.stderr).expect("non-utf8 stderr");
    let value: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be valid JSON");
    assert_eq!(value["code"], "daemon.ensure_requires_via_daemon");
}

#[test]
fn test_acp_agent_cli_gitignore_contains_cli_forge_and_target() {
    let gitignore =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".gitignore"))
            .expect("failed to read .gitignore");
    assert!(gitignore.contains(".cli-forge/"));
    assert!(gitignore.contains("/target/"));
}
