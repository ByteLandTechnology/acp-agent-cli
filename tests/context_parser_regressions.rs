use assert_cmd::Command;
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
fn run_honors_explicit_agent_workspace_and_session_overrides() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mock_agent = create_mock_agent(&temp_dir);
    let sandbox = sandbox_args(&temp_dir);
    let persisted_workspace = temp_dir.path().join("persisted-workspace");
    let override_workspace = temp_dir.path().join("override-workspace");
    let persisted_cwd = temp_dir.path().join("persisted-cwd");

    cmd()
        .args(&sandbox)
        .args([
            "context",
            "use",
            "--agent",
            "persisted-agent",
            "--workspace",
            &persisted_workspace.display().to_string(),
            "--session",
            "persisted-session",
            "--selector",
            "provider=staging",
            "--cwd",
            &persisted_cwd.display().to_string(),
            "--format",
            "json",
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
            "--workspace",
            &override_workspace.display().to_string(),
            "--session",
            "override-session",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to execute run");

    assert!(run_output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(run_output.stdout).expect("non-utf8 output");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(
        value["effective_context"]["agent"],
        mock_agent.display().to_string()
    );
    assert_eq!(
        value["effective_context"]["workspace"],
        override_workspace.display().to_string()
    );
    assert_eq!(value["effective_context"]["session_id"], "override-session");
    assert_eq!(value["effective_context"]["provider"], "staging");
    assert_eq!(
        value["effective_context"]["current_directory"],
        persisted_cwd.display().to_string()
    );

    let show_output = cmd()
        .args(&sandbox)
        .args(["context", "show", "--format", "json"])
        .output()
        .expect("failed to execute context show");

    assert!(show_output.status.success(), "expected exit 0");

    let show_stdout = String::from_utf8(show_output.stdout).expect("non-utf8 output");
    let show_value: serde_json::Value =
        serde_json::from_str(&show_stdout).expect("stdout should be valid JSON");

    assert_eq!(show_value["persisted_context"]["agent"], "persisted-agent");
    assert_eq!(
        show_value["persisted_context"]["workspace"],
        persisted_workspace.display().to_string()
    );
    assert_eq!(
        show_value["persisted_context"]["session_id"],
        "persisted-session"
    );
}

#[test]
fn external_commands_honor_explicit_agent_workspace_and_session_overrides() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sandbox = sandbox_args(&temp_dir);
    let persisted_workspace = temp_dir.path().join("persisted-workspace");
    let override_workspace = temp_dir.path().join("override-workspace");

    cmd()
        .args(&sandbox)
        .args([
            "context",
            "use",
            "--agent",
            "persisted-agent",
            "--workspace",
            &persisted_workspace.display().to_string(),
            "--session",
            "persisted-session",
            "--selector",
            "provider=staging",
            "--format",
            "json",
        ])
        .assert()
        .success();

    let output = cmd()
        .args(&sandbox)
        .args([
            "--via",
            "daemon",
            "--ensure-daemon",
            "session",
            "list",
            "--agent",
            "override-agent",
            "--workspace",
            &override_workspace.display().to_string(),
            "--session",
            "override-session",
            "--selector",
            "provider=staging",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to execute external command");

    assert!(output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 output");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(value["status"], "accepted_daemon");
    assert_eq!(value["context"]["agent"], "override-agent");
    assert_eq!(
        value["context"]["workspace"],
        override_workspace.display().to_string()
    );
    assert_eq!(value["context"]["session_id"], "override-session");
    assert_eq!(value["context"]["provider"], "staging");
}

#[test]
fn context_use_preserves_existing_values_until_clear_is_requested() {
    let temp_dir = TempDir::new().expect("temp dir");
    let sandbox = sandbox_args(&temp_dir);
    let workspace = temp_dir.path().join("workspace-a");
    let current_directory = temp_dir.path().join("cwd-a");

    cmd()
        .args(&sandbox)
        .args([
            "context",
            "use",
            "--agent",
            "persisted-agent",
            "--workspace",
            &workspace.display().to_string(),
            "--selector",
            "provider=staging",
            "--cwd",
            &current_directory.display().to_string(),
            "--format",
            "json",
        ])
        .assert()
        .success();

    cmd()
        .args(&sandbox)
        .args([
            "context",
            "use",
            "--session",
            "next-session",
            "--format",
            "json",
        ])
        .assert()
        .success();

    let show_output = cmd()
        .args(&sandbox)
        .args(["context", "show", "--format", "json"])
        .output()
        .expect("failed to execute context show");

    assert!(show_output.status.success(), "expected exit 0");

    let stdout = String::from_utf8(show_output.stdout).expect("non-utf8 output");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert_eq!(value["persisted_context"]["agent"], "persisted-agent");
    assert_eq!(
        value["persisted_context"]["workspace"],
        workspace.display().to_string()
    );
    assert_eq!(value["persisted_context"]["session_id"], "next-session");
    assert_eq!(
        value["persisted_context"]["selectors"]["provider"],
        "staging"
    );
    assert_eq!(
        value["persisted_context"]["ambient_cues"]["current_directory"],
        current_directory.display().to_string()
    );

    cmd()
        .args(&sandbox)
        .args([
            "context",
            "use",
            "--clear",
            "--session",
            "cleared-session",
            "--format",
            "json",
        ])
        .assert()
        .success();

    let cleared_output = cmd()
        .args(&sandbox)
        .args(["context", "show", "--format", "json"])
        .output()
        .expect("failed to execute cleared context show");

    assert!(cleared_output.status.success(), "expected exit 0");

    let cleared_stdout = String::from_utf8(cleared_output.stdout).expect("non-utf8 output");
    let cleared_value: serde_json::Value =
        serde_json::from_str(&cleared_stdout).expect("stdout should be valid JSON");

    assert_eq!(
        cleared_value["persisted_context"]["session_id"],
        "cleared-session"
    );
    assert!(cleared_value["persisted_context"]["agent"].is_null());
    assert!(cleared_value["persisted_context"]["workspace"].is_null());
    assert!(
        cleared_value["persisted_context"]["selectors"]
            .as_object()
            .is_none()
    );
    assert!(
        cleared_value["persisted_context"]["ambient_cues"]
            .as_object()
            .is_none()
    );
}

#[test]
fn fixed_arity_planned_commands_reject_unexpected_extra_positionals() {
    let output = cmd()
        .args(["agent", "status", "unexpected", "--format", "json"])
        .output()
        .expect("failed to execute planned command");

    assert!(!output.status.success(), "expected non-zero exit");

    let stderr = String::from_utf8(output.stderr).expect("non-utf8 stderr");
    let value: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be valid JSON");

    assert_eq!(value["code"], "command.unexpected_input");
    assert_eq!(value["source"], "help_usage");
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("unexpected positional input `unexpected`")
    );
}
