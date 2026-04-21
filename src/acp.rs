//! ACP (Agent Client Protocol) client — JSON-RPC 2.0 over stdio.
//!
//! Handles the full ACP lifecycle: initialize → session/new → session/prompt
//! with streaming notification support.

use crate::registry;
use crate::transport::Transport;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// ACP client managing a JSON-RPC connection to an agent process.
pub struct AcpClient {
    transport: Transport,
    agent_info: Value,
    session_id: Option<String>,
}

/// Result from a prompt turn.
pub struct PromptResult {
    pub stop_reason: String,
    pub messages: Value,
    pub notifications: Vec<Value>,
}

impl AcpClient {
    /// Spawn an agent and perform the ACP initialize handshake.
    pub fn connect(executable: &str, args: &[String]) -> Result<Self> {
        let mut transport = Transport::spawn(executable, args)?;

        let (result, _) = transport.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": true, "writeTextFile": true },
                    "terminal": true
                },
                "clientInfo": {
                    "name": "acp-agent-cli",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )?;

        Ok(Self {
            transport,
            agent_info: result,
            session_id: None,
        })
    }

    pub fn agent_info(&self) -> &Value {
        &self.agent_info
    }

    /// Create a new agent session.
    pub fn create_session(&mut self) -> Result<String> {
        let (result, _) = self.transport.request(
            "session/new",
            json!({
                "cwd": std::env::current_dir().unwrap_or_default().display().to_string(),
                "mcpServers": []
            }),
        )?;
        let session_id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .context("agent did not return sessionId")?
            .to_string();
        self.session_id = Some(session_id.clone());
        Ok(session_id)
    }

    /// Send a prompt and wait for the final result.
    pub fn send_prompt(&mut self, message: &str) -> Result<PromptResult> {
        let (result, notifications) = self.send_prompt_raw(message)?;
        Ok(parse_prompt_result(result, notifications))
    }

    /// Create a new ACP session and return its ID without storing it as the default.
    pub fn create_session_id(&mut self) -> Result<String> {
        let (result, _) = self.transport.request(
            "session/new",
            json!({
                "cwd": std::env::current_dir().unwrap_or_default().display().to_string(),
                "mcpServers": []
            }),
        )?;
        result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .context("agent did not return sessionId")
            .map(|s| s.to_string())
    }

    /// Send a prompt targeting a specific session ID, ignoring the stored default.
    pub fn send_prompt_with_session(
        &mut self,
        session_id: &str,
        message: &str,
    ) -> Result<PromptResult> {
        let (result, notifications) = self
            .transport
            .request("session/prompt", prompt_params(session_id, message))?;
        Ok(parse_prompt_result(result, notifications))
    }

    /// Send a prompt with a streaming callback for notifications.
    pub fn send_prompt_streaming<F>(
        &mut self,
        message: &str,
        mut on_notification: F,
    ) -> Result<PromptResult>
    where
        F: FnMut(&Value),
    {
        let id = self.transport.send_request(
            "session/prompt",
            prompt_params(self.session_id.as_deref().unwrap_or(""), message),
        )?;

        let mut notifications = Vec::new();
        let final_result;

        loop {
            let (msg_id, msg) = self.transport.read_message()?;
            match msg_id {
                Some(response_id) if response_id == id => {
                    if let Some(error) = msg.get("error") {
                        let code = error.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
                        let message = error
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown error");
                        bail!("agent error (code {}): {}", code, message);
                    }
                    final_result = Some(msg.get("result").cloned().unwrap_or(Value::Null));
                    break;
                }
                Some(_) => continue,
                None => {
                    on_notification(&msg);
                    notifications.push(msg);
                }
            }
        }

        let result = final_result.context("no result from agent")?;
        Ok(parse_prompt_result(result, notifications))
    }

    /// Cancel the current in-progress prompt.
    pub fn cancel(&mut self) -> Result<()> {
        self.transport
            .send_notification("session/cancel", json!({}))
    }

    /// Shut down the agent process.
    pub fn shutdown(mut self) -> Result<()> {
        self.transport.shutdown()
    }

    pub fn raw_request(&mut self, method: &str, params: Value) -> Result<(Value, Vec<Value>)> {
        self.transport.request(method, params)
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    fn send_prompt_raw(&mut self, message: &str) -> Result<(Value, Vec<Value>)> {
        let sid = self.session_id.as_deref().unwrap_or("");
        self.transport
            .request("session/prompt", prompt_params(sid, message))
    }
}

fn prompt_params(session_id: &str, message: &str) -> Value {
    json!({
        "sessionId": session_id,
        "prompt": [
            { "type": "text", "text": message }
        ]
    })
}

fn parse_prompt_result(result: Value, notifications: Vec<Value>) -> PromptResult {
    PromptResult {
        stop_reason: result
            .get("stopReason")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        messages: result.get("messages").cloned().unwrap_or(json!([])),
        notifications,
    }
}

/// Extract all text content from a prompt result's notifications.
pub fn extract_text(result: &PromptResult) -> String {
    let mut text = String::new();
    for notification in &result.notifications {
        if let Some(chunk) = extract_chunk_text(notification) {
            text.push_str(&chunk);
        }
    }
    text
}

/// Extract streaming text chunk from a session/update notification.
pub fn extract_chunk_text(notification: &Value) -> Option<String> {
    let update = notification.get("params")?.get("update")?;
    match update.get("sessionUpdate")?.as_str()? {
        "agent_message_chunk" => update
            .get("content")?
            .get("text")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// Resolve the executable path for an agent identifier.
///
/// Resolution order:
/// 1. Direct path (starts with `/`, `./`, or `~`)
/// 2. Installed agent ID from registry
/// 3. Command name in system PATH
pub fn resolve_agent_executable(agent: &str, data_dir: &Path) -> Result<String> {
    // Direct filesystem path
    if agent.starts_with('/') || agent.starts_with("./") || agent.starts_with('~') {
        let expanded = shellexpand_agent(agent);
        if Path::new(&expanded).exists() {
            return Ok(expanded);
        }
        bail!("agent executable not found at: {}", expanded);
    }

    // Installed agent from registry
    let installed = registry::scan_installed_agents(data_dir)?;
    if let Some(installed_agent) = installed.iter().find(|a| a.id == agent) {
        if let Some(ref exe) = installed_agent.executable {
            let exe_name = exe.trim_start_matches("./");
            let exe_path = installed_agent.install_path.join(exe_name);
            if exe_path.exists() {
                return Ok(exe_path.display().to_string());
            }
        }
        bail!(
            "installed agent '{}' has no valid executable at {}",
            agent,
            installed_agent.install_path.display()
        );
    }

    // System PATH lookup
    if let Ok(path) = which::which(agent) {
        let path_str = path.display().to_string();
        if !path_str.is_empty() {
            return Ok(path_str);
        }
    }

    bail!(
        "agent '{}' not found — install it with: acp-agent-cli registry install {}",
        agent,
        agent
    );
}

/// Resolve agent args from installed metadata or registry.
pub fn resolve_agent_args(agent: &str, data_dir: &Path) -> Vec<String> {
    registry::resolve_agent_args(agent, data_dir, None)
}

fn shellexpand_agent(agent: &str) -> String {
    if agent.starts_with('~') {
        let home = std::env::var("HOME").unwrap_or_default();
        agent.replacen('~', &home, 1)
    } else {
        agent.to_string()
    }
}

/// Run a single prompt against an agent. Streaming chunks go to stderr.
pub fn run_agent_prompt(
    agent: &str,
    input: &str,
    data_dir: &Path,
    _effective_context: &BTreeMap<String, String>,
) -> Result<(String, String)> {
    let executable = resolve_agent_executable(agent, data_dir)?;
    let args = resolve_agent_args(agent, data_dir);
    let mut client = AcpClient::connect(&executable, &args)?;
    let _session_id = client.create_session()?;

    let result = client.send_prompt_streaming(input, |notification| {
        if let Some(chunk) = extract_chunk_text(notification) {
            eprint!("{}", chunk);
        }
    })?;

    if !result.notifications.is_empty() {
        let _ = writeln!(std::io::stderr());
    }

    let text = extract_text(&result);
    client.shutdown()?;

    Ok((result.stop_reason, text))
}
