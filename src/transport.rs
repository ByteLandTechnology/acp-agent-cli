//! JSON-RPC 2.0 transport over stdio with a child process.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

/// JSON-RPC 2.0 transport over stdio.
///
/// Manages a child process where each JSON-RPC message is a single
/// newline-delimited JSON line on stdin/stdout.
pub struct Transport {
    child: Child,
    stdin: Option<ChildStdin>,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Transport {
    /// Spawn a child process and set up stdio pipes for JSON-RPC.
    pub fn spawn(executable: &str, args: &[String]) -> Result<Self> {
        let mut child = Command::new(executable)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn agent: {}", executable))?;

        let stdin = child.stdin.take().context("failed to open agent stdin")?;
        let stdout = child.stdout.take().context("failed to open agent stdout")?;

        Ok(Self {
            child,
            stdin: Some(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn write_json(&mut self, value: &Value) -> Result<()> {
        let stdin = self.stdin.as_mut().context("agent stdin closed")?;
        let mut line = serde_json::to_string(value)?;
        line.push('\n');
        stdin.write_all(line.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    fn read_json(&mut self) -> Result<(Option<u64>, Value)> {
        let mut line = String::new();
        let bytes = self.reader.read_line(&mut line)?;
        if bytes == 0 {
            bail!("agent closed stdout unexpectedly");
        }
        let msg: Value = serde_json::from_str(line.trim())
            .with_context(|| format!("invalid JSON from agent: {}", line.trim()))?;
        let id = msg.get("id").and_then(|v| v.as_u64());
        Ok((id, msg))
    }

    /// Send a JSON-RPC request and return the assigned ID.
    pub fn send_request(&mut self, method: &str, params: Value) -> Result<u64> {
        let id = self.allocate_id();
        self.write_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        Ok(id)
    }

    /// Send a JSON-RPC notification (no response expected).
    pub fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        self.write_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    /// Wait for a response with the given ID, collecting any notifications.
    pub fn wait_for_response(&mut self, expected_id: u64) -> Result<(Value, Vec<Value>)> {
        let mut notifications = Vec::new();
        loop {
            let (id, msg) = self.read_json()?;
            match id {
                Some(response_id) if response_id == expected_id => {
                    if let Some(error) = msg.get("error") {
                        let code = error.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
                        let message = error
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown error");
                        bail!("agent error (code {}): {}", code, message);
                    }
                    let result = msg.get("result").cloned().unwrap_or(Value::Null);
                    return Ok((result, notifications));
                }
                Some(_) => continue,
                None => notifications.push(msg),
            }
        }
    }

    /// Send a request and wait for the matching response.
    pub fn request(&mut self, method: &str, params: Value) -> Result<(Value, Vec<Value>)> {
        let id = self.send_request(method, params)?;
        self.wait_for_response(id)
    }

    /// Read the next raw message from the agent stdout.
    pub fn read_message(&mut self) -> Result<(Option<u64>, Value)> {
        self.read_json()
    }

    /// Shut down the agent process.
    pub fn shutdown(&mut self) -> Result<()> {
        drop(self.stdin.take());
        let _ = self.child.try_wait();
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
