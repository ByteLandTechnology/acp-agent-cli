//! Optional daemon app-server overlay for the generated package layout.
//! This module is package-local to generated skills when daemon support is
//! enabled. It provides one binary that can act as both daemon server and
//! daemon client while preserving the generated runtime conventions.

use crate::{Format, StructuredError};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
#[cfg(not(unix))]
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

struct DaemonAgentConnection {
    client: crate::acp::AcpClient,
    sessions: std::collections::HashMap<String, u64>,
    default_session: Option<String>,
}
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::context::{RuntimeLocations, path_to_string};

const DAEMON_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DAEMON_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonLifecycleState {
    Stopped,
    Starting,
    Running,
    Degraded,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRuntimeArtifacts {
    pub directory: String,
    pub pid_file: String,
    pub socket_metadata_file: String,
    pub state_file: String,
    pub log_file: String,
    pub lock_file: String,
    pub auth_token_file: String,
}

#[derive(Debug, Clone)]
pub struct DaemonFiles {
    pub directory: PathBuf,
    pub pid_file: PathBuf,
    pub socket_path: PathBuf,
    pub socket_metadata_file: PathBuf,
    pub state_file: PathBuf,
    pub log_file: PathBuf,
    pub lock_file: PathBuf,
    pub auth_token_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub state: String,
    pub readiness: String,
    pub instance_id: String,
    pub pid: Option<u32>,
    pub transport: String,
    pub endpoint: String,
    pub uptime_sec: u64,
    pub active_requests: u64,
    pub queue_depth: u64,
    pub last_error: String,
    pub recommended_next_action: String,
    pub runtime_artifacts: DaemonRuntimeArtifacts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonLifecycleResponse {
    pub status: String,
    pub action: String,
    pub daemon_status: DaemonStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonStateFile {
    state: DaemonLifecycleState,
    readiness: String,
    instance_id: String,
    pid: Option<u32>,
    transport: String,
    endpoint: String,
    started_at_epoch_sec: u64,
    active_requests: u64,
    queue_depth: u64,
    last_error: String,
    recommended_next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonRpcRequest {
    Health,
    Status,
    Shutdown,
    SessionCreate {
        agent: String,
    },
    SessionList,
    SessionClose {
        agent: String,
        session_id: String,
    },
    ExecuteRun {
        input: String,
        agent: String,
        session_id: Option<String>,
        effective_context: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonRpcResponse {
    Health {
        ok: bool,
    },
    Status {
        status: Box<DaemonStatus>,
    },
    Shutdown {
        accepted: bool,
    },
    SessionCreate {
        agent: String,
        session_id: String,
    },
    SessionList {
        agents: Vec<AgentSessionInfo>,
    },
    SessionClose {
        agent: String,
        session_id: String,
    },
    RunResult {
        output: crate::AcpAgentCliOutput,
    },
    Error {
        code: String,
        message: String,
        recommended_next_action: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionInfo {
    pub agent: String,
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub turn_count: u64,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonInfo {
    pub enabled: bool,
    pub mode: String,
    pub instance_model: String,
    pub lifecycle: Vec<String>,
    pub routing_flags: Vec<String>,
    pub local_only_commands: Vec<String>,
    pub transports: Vec<String>,
    pub auth: Vec<String>,
    pub runtime_artifacts: Vec<String>,
}

pub fn daemon_info(runtime: &RuntimeLocations) -> DaemonInfo {
    let files = daemon_files(runtime);
    DaemonInfo {
        enabled: true,
        mode: "app_server".to_string(),
        instance_model: "single".to_string(),
        lifecycle: vec![
            "daemon run".to_string(),
            "daemon start".to_string(),
            "daemon stop".to_string(),
            "daemon restart".to_string(),
            "daemon status".to_string(),
        ],
        routing_flags: vec!["--via".to_string(), "--ensure-daemon".to_string()],
        local_only_commands: vec![
            "help".to_string(),
            "paths".to_string(),
            "context show".to_string(),
            "context use".to_string(),
            "agent status".to_string(),
            "agent candidates".to_string(),
            "agent select".to_string(),
            "agent set-executable".to_string(),
            "agent set-args".to_string(),
            "workspace browse".to_string(),
            "workspace mkdir".to_string(),
            "session use".to_string(),
            "repl".to_string(),
            "daemon".to_string(),
        ],
        transports: vec![
            "unix_socket_or_named_pipe".to_string(),
            "opt_in_loopback_tcp".to_string(),
        ],
        auth: vec![
            "os_permissions".to_string(),
            "required_when_tcp_enabled".to_string(),
        ],
        runtime_artifacts: vec![
            path_to_string(&files.pid_file),
            path_to_string(&files.socket_metadata_file),
            path_to_string(&files.state_file),
            path_to_string(&files.log_file),
            path_to_string(&files.lock_file),
            path_to_string(&files.auth_token_file),
        ],
    }
}

pub fn daemon_files(runtime: &RuntimeLocations) -> DaemonFiles {
    let directory = runtime.state_dir.join("daemon");
    let socket_path = directory.join("daemon.sock");
    DaemonFiles {
        pid_file: directory.join("daemon.pid"),
        socket_metadata_file: directory.join("daemon.sock_or_pipe_metadata"),
        state_file: directory.join("daemon-state.json"),
        log_file: directory.join("daemon.log"),
        lock_file: directory.join("daemon.lock"),
        auth_token_file: directory.join("auth.token_when_tcp_enabled"),
        directory,
        socket_path,
    }
}

pub fn ensure_daemon_directories(runtime: &RuntimeLocations) -> Result<DaemonFiles> {
    runtime.ensure_exists()?;
    let files = daemon_files(runtime);
    fs::create_dir_all(&files.directory)
        .with_context(|| format!("failed to create {}", files.directory.display()))?;
    Ok(files)
}

pub fn daemon_runtime_artifacts(runtime: &RuntimeLocations) -> DaemonRuntimeArtifacts {
    let files = daemon_files(runtime);
    DaemonRuntimeArtifacts {
        directory: path_to_string(&files.directory),
        pid_file: path_to_string(&files.pid_file),
        socket_metadata_file: path_to_string(&files.socket_metadata_file),
        state_file: path_to_string(&files.state_file),
        log_file: path_to_string(&files.log_file),
        lock_file: path_to_string(&files.lock_file),
        auth_token_file: path_to_string(&files.auth_token_file),
    }
}

fn now_epoch_sec() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(unix)]
fn pid_is_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_is_running(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains(&pid.to_string())
        })
        .unwrap_or(false)
}

fn daemon_transport_label() -> &'static str {
    if cfg!(unix) {
        "unix_socket_or_named_pipe"
    } else {
        "loopback_tcp"
    }
}

fn daemon_default_endpoint(runtime: &RuntimeLocations) -> String {
    if cfg!(unix) {
        path_to_string(&daemon_files(runtime).socket_path)
    } else {
        "127.0.0.1:0".to_string()
    }
}

fn default_state(runtime: &RuntimeLocations) -> DaemonStateFile {
    DaemonStateFile {
        state: DaemonLifecycleState::Stopped,
        readiness: "not_running".to_string(),
        instance_id: "default".to_string(),
        pid: None,
        transport: daemon_transport_label().to_string(),
        endpoint: daemon_default_endpoint(runtime),
        started_at_epoch_sec: now_epoch_sec(),
        active_requests: 0,
        queue_depth: 0,
        last_error: String::new(),
        recommended_next_action: "daemon start".to_string(),
    }
}

fn save_state(runtime: &RuntimeLocations, state: &DaemonStateFile) -> Result<()> {
    let files = ensure_daemon_directories(runtime)?;
    let serialized =
        serde_json::to_string_pretty(state).context("failed to serialize daemon state")?;
    fs::write(&files.state_file, serialized)
        .with_context(|| format!("failed to write {}", files.state_file.display()))?;
    if let Some(pid) = state.pid {
        fs::write(&files.pid_file, pid.to_string())
            .with_context(|| format!("failed to write {}", files.pid_file.display()))?;
    } else if files.pid_file.exists() {
        fs::remove_file(&files.pid_file)
            .with_context(|| format!("failed to remove {}", files.pid_file.display()))?;
    }
    if matches!(
        state.state,
        DaemonLifecycleState::Running
            | DaemonLifecycleState::Starting
            | DaemonLifecycleState::Stopping
            | DaemonLifecycleState::Degraded
    ) && !state.endpoint.trim().is_empty()
    {
        fs::write(&files.socket_metadata_file, &state.endpoint)
            .with_context(|| format!("failed to write {}", files.socket_metadata_file.display()))?;
    } else if files.socket_metadata_file.exists() {
        fs::remove_file(&files.socket_metadata_file).with_context(|| {
            format!("failed to remove {}", files.socket_metadata_file.display())
        })?;
    }
    Ok(())
}

fn load_state(runtime: &RuntimeLocations) -> Result<DaemonStateFile> {
    let files = daemon_files(runtime);
    if !files.state_file.exists() {
        return Ok(default_state(runtime));
    }

    let raw = fs::read_to_string(&files.state_file)
        .with_context(|| format!("failed to read {}", files.state_file.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", files.state_file.display()))
}

fn cleanup_daemon_artifacts(runtime: &RuntimeLocations) -> Result<()> {
    let files = daemon_files(runtime);
    if files.socket_path.exists() {
        fs::remove_file(&files.socket_path)
            .with_context(|| format!("failed to remove {}", files.socket_path.display()))?;
    }
    if files.lock_file.exists() {
        fs::remove_file(&files.lock_file)
            .with_context(|| format!("failed to remove {}", files.lock_file.display()))?;
    }
    Ok(())
}

fn write_failed_state(
    runtime: &RuntimeLocations,
    message: impl Into<String>,
    pid: Option<u32>,
) -> Result<()> {
    let mut failed_state = default_state(runtime);
    failed_state.state = DaemonLifecycleState::Failed;
    failed_state.readiness = "failed".to_string();
    failed_state.pid = pid;
    failed_state.last_error = message.into();
    failed_state.recommended_next_action = "daemon start".to_string();
    save_state(runtime, &failed_state)
}

fn materialize_status(runtime: &RuntimeLocations, mut state: DaemonStateFile) -> DaemonStatus {
    if let Some(pid) = state.pid
        && !pid_is_running(pid)
    {
        state.state = DaemonLifecycleState::Stopped;
        state.readiness = "not_running".to_string();
        state.pid = None;
        state.recommended_next_action = "daemon start".to_string();
    }

    let uptime_sec = state
        .pid
        .map(|_| now_epoch_sec().saturating_sub(state.started_at_epoch_sec))
        .unwrap_or(0);

    DaemonStatus {
        state: match state.state {
            DaemonLifecycleState::Stopped => "stopped",
            DaemonLifecycleState::Starting => "starting",
            DaemonLifecycleState::Running => "running",
            DaemonLifecycleState::Degraded => "degraded",
            DaemonLifecycleState::Stopping => "stopping",
            DaemonLifecycleState::Failed => "failed",
        }
        .to_string(),
        readiness: state.readiness,
        instance_id: state.instance_id,
        pid: state.pid,
        transport: state.transport,
        endpoint: state.endpoint,
        uptime_sec,
        active_requests: state.active_requests,
        queue_depth: state.queue_depth,
        last_error: state.last_error,
        recommended_next_action: state.recommended_next_action,
        runtime_artifacts: daemon_runtime_artifacts(runtime),
    }
}

pub fn daemon_status(runtime: &RuntimeLocations) -> Result<DaemonStatus> {
    let state = load_state(runtime)?;
    if matches!(
        state.state,
        DaemonLifecycleState::Running
            | DaemonLifecycleState::Starting
            | DaemonLifecycleState::Stopping
            | DaemonLifecycleState::Degraded
    ) {
        if let Ok(status) = daemon_live_status(runtime) {
            return Ok(status);
        }

        let stopped_state = default_state(runtime);
        cleanup_daemon_artifacts(runtime)?;
        save_state(runtime, &stopped_state)?;
        return Ok(materialize_status(runtime, stopped_state));
    }
    Ok(materialize_status(runtime, state))
}

pub fn daemon_status_response(runtime: &RuntimeLocations) -> Result<DaemonLifecycleResponse> {
    Ok(DaemonLifecycleResponse {
        status: "ok".to_string(),
        action: "status".to_string(),
        daemon_status: daemon_status(runtime)?,
    })
}

pub fn ensure_daemon_running(runtime: &RuntimeLocations) -> Result<DaemonStatus> {
    let status = daemon_status(runtime)?;
    if status.state == "running" {
        return Ok(status);
    }

    daemon_start(runtime).map(|response| response.daemon_status)
}

#[allow(dead_code)]
fn read_rpc_line(stream: &mut impl Read) -> Result<String> {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream
            .read(&mut byte)
            .context("failed to read RPC response")?
        {
            0 => break,
            _ => {
                if byte[0] == b'\n' {
                    break;
                }
                buffer.push(byte[0]);
            }
        }
    }

    let line = String::from_utf8(buffer).context("daemon returned non-utf8 RPC data")?;
    if line.trim().is_empty() {
        bail!("daemon returned an empty RPC response");
    }
    Ok(line)
}

fn exchange_rpc<S>(stream: &mut S, request: &DaemonRpcRequest) -> Result<DaemonRpcResponse>
where
    S: Read + Write,
{
    let serialized = serde_json::to_string(request).context("failed to serialize RPC request")?;
    stream
        .write_all(serialized.as_bytes())
        .context("failed to write RPC request")?;
    stream
        .write_all(b"\n")
        .context("failed to finish RPC request")?;
    stream.flush().context("failed to flush RPC request")?;

    let line = read_rpc_line(stream)?;
    serde_json::from_str(line.trim()).context("failed to parse RPC response")
}

#[cfg(unix)]
fn invoke_daemon_rpc(
    runtime: &RuntimeLocations,
    request: &DaemonRpcRequest,
) -> Result<DaemonRpcResponse> {
    let files = daemon_files(runtime);
    if !files.socket_path.exists() {
        bail!("daemon endpoint is not available");
    }

    let mut stream = UnixStream::connect(&files.socket_path)
        .with_context(|| format!("failed to connect to {}", files.socket_path.display()))?;
    exchange_rpc(&mut stream, request)
}

#[cfg(not(unix))]
fn invoke_daemon_rpc(
    runtime: &RuntimeLocations,
    request: &DaemonRpcRequest,
) -> Result<DaemonRpcResponse> {
    let files = daemon_files(runtime);
    if !files.socket_metadata_file.exists() {
        bail!("daemon endpoint is not available");
    }

    let endpoint = fs::read_to_string(&files.socket_metadata_file)
        .with_context(|| format!("failed to read {}", files.socket_metadata_file.display()))?;
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        bail!("daemon endpoint is not available");
    }

    let mut stream =
        TcpStream::connect(endpoint).with_context(|| format!("failed to connect to {endpoint}"))?;
    exchange_rpc(&mut stream, request)
}

fn daemon_live_status(runtime: &RuntimeLocations) -> Result<DaemonStatus> {
    match invoke_daemon_rpc(runtime, &DaemonRpcRequest::Status)? {
        DaemonRpcResponse::Status { status } => Ok(*status),
        other => bail!(
            "daemon returned unexpected RPC response for status: {}",
            serde_json::to_string(&other).unwrap_or_else(|_| "<unserializable>".to_string())
        ),
    }
}

fn daemon_healthcheck(runtime: &RuntimeLocations) -> Result<()> {
    match invoke_daemon_rpc(runtime, &DaemonRpcRequest::Health)? {
        DaemonRpcResponse::Health { ok: true } => Ok(()),
        DaemonRpcResponse::Health { ok: false } => bail!("daemon health check reported not ready"),
        other => bail!(
            "daemon returned unexpected RPC response for health: {}",
            serde_json::to_string(&other).unwrap_or_else(|_| "<unserializable>".to_string())
        ),
    }
}

fn spawn_daemon_process(runtime: &RuntimeLocations) -> Result<Child> {
    let files = ensure_daemon_directories(runtime)?;
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&files.log_file)
        .with_context(|| format!("failed to open {}", files.log_file.display()))?;
    let stderr = log_file
        .try_clone()
        .context("failed to clone daemon log handle")?;
    let executable = std::env::current_exe().context("failed to locate current executable")?;

    let mut command = Command::new(executable);
    command
        .arg("--config-dir")
        .arg(&runtime.config_dir)
        .arg("--data-dir")
        .arg(&runtime.data_dir)
        .arg("--state-dir")
        .arg(&runtime.state_dir)
        .arg("--cache-dir")
        .arg(&runtime.cache_dir)
        .args(["--format", "json", "daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr));

    if let Some(log_dir) = &runtime.log_dir {
        command.arg("--log-dir").arg(log_dir);
    }

    command.spawn().context("failed to spawn daemon process")
}

fn wait_for_daemon_ready(runtime: &RuntimeLocations, child: &mut Child) -> Result<DaemonStatus> {
    let deadline = Instant::now() + DAEMON_STARTUP_TIMEOUT;
    loop {
        if daemon_healthcheck(runtime).is_ok() {
            return daemon_live_status(runtime);
        }

        if let Some(status) = child
            .try_wait()
            .context("failed to inspect daemon child process")?
        {
            bail!("daemon exited before becoming ready with status {status}");
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "daemon did not become ready within {}s",
                DAEMON_STARTUP_TIMEOUT.as_secs()
            );
        }

        thread::sleep(DAEMON_POLL_INTERVAL);
    }
}

fn wait_for_daemon_shutdown(runtime: &RuntimeLocations) -> Result<()> {
    let deadline = Instant::now() + DAEMON_SHUTDOWN_TIMEOUT;
    loop {
        if daemon_healthcheck(runtime).is_err() {
            return Ok(());
        }

        if Instant::now() >= deadline {
            bail!(
                "daemon did not stop within {}s",
                DAEMON_SHUTDOWN_TIMEOUT.as_secs()
            );
        }

        thread::sleep(DAEMON_POLL_INTERVAL);
    }
}

pub fn daemon_start(runtime: &RuntimeLocations) -> Result<DaemonLifecycleResponse> {
    ensure_daemon_directories(runtime)?;
    let current_status = daemon_status(runtime)?;
    if current_status.state == "running" {
        return Ok(DaemonLifecycleResponse {
            status: "running".to_string(),
            action: "start".to_string(),
            daemon_status: current_status,
        });
    }

    cleanup_daemon_artifacts(runtime)?;

    let mut starting_state = default_state(runtime);
    starting_state.state = DaemonLifecycleState::Starting;
    starting_state.readiness = "starting".to_string();
    starting_state.started_at_epoch_sec = now_epoch_sec();
    starting_state.recommended_next_action = "daemon status".to_string();
    save_state(runtime, &starting_state)?;

    let mut child = match spawn_daemon_process(runtime) {
        Ok(child) => child,
        Err(error) => {
            write_failed_state(runtime, error.to_string(), None)?;
            return Err(error);
        }
    };

    let live_status = match wait_for_daemon_ready(runtime, &mut child) {
        Ok(status) => status,
        Err(error) => {
            cleanup_daemon_artifacts(runtime)?;
            write_failed_state(runtime, error.to_string(), None)?;
            return Err(error);
        }
    };

    Ok(DaemonLifecycleResponse {
        status: "running".to_string(),
        action: "start".to_string(),
        daemon_status: live_status,
    })
}

pub fn daemon_stop(runtime: &RuntimeLocations) -> Result<DaemonLifecycleResponse> {
    if daemon_healthcheck(runtime).is_err() {
        cleanup_daemon_artifacts(runtime)?;
        let stopped_state = default_state(runtime);
        save_state(runtime, &stopped_state)?;
        return Ok(DaemonLifecycleResponse {
            status: "stopped".to_string(),
            action: "stop".to_string(),
            daemon_status: materialize_status(runtime, stopped_state),
        });
    }

    match invoke_daemon_rpc(runtime, &DaemonRpcRequest::Shutdown)? {
        DaemonRpcResponse::Shutdown { accepted: true } => {
            wait_for_daemon_shutdown(runtime)?;
        }
        DaemonRpcResponse::Shutdown { accepted: false } => {
            bail!("daemon rejected shutdown request");
        }
        other => bail!(
            "daemon returned unexpected RPC response for shutdown: {}",
            serde_json::to_string(&other).unwrap_or_else(|_| "<unserializable>".to_string())
        ),
    }

    cleanup_daemon_artifacts(runtime)?;
    let stopped_state = default_state(runtime);
    save_state(runtime, &stopped_state)?;
    Ok(DaemonLifecycleResponse {
        status: "stopped".to_string(),
        action: "stop".to_string(),
        daemon_status: materialize_status(runtime, stopped_state),
    })
}

pub fn daemon_restart(runtime: &RuntimeLocations) -> Result<DaemonLifecycleResponse> {
    let _ = daemon_stop(runtime)?;
    daemon_start(runtime).map(|mut response| {
        response.action = "restart".to_string();
        response
    })
}

fn running_state(endpoint: String, transport: &str) -> DaemonStateFile {
    DaemonStateFile {
        state: DaemonLifecycleState::Running,
        readiness: "ready".to_string(),
        instance_id: "default".to_string(),
        pid: Some(std::process::id()),
        transport: transport.to_string(),
        endpoint,
        started_at_epoch_sec: now_epoch_sec(),
        active_requests: 0,
        queue_depth: 0,
        last_error: String::new(),
        recommended_next_action: "daemon status".to_string(),
    }
}

fn finalize_stopped_server(
    runtime: &RuntimeLocations,
    files: &DaemonFiles,
    server_state: &Arc<Mutex<DaemonStateFile>>,
) -> Result<DaemonLifecycleResponse> {
    let mut stopped_state = server_state.lock().unwrap();
    stopped_state.state = DaemonLifecycleState::Stopped;
    stopped_state.readiness = "not_running".to_string();
    stopped_state.pid = None;
    stopped_state.recommended_next_action = "daemon start".to_string();
    save_state(runtime, &stopped_state.clone())?;
    if files.socket_path.exists() {
        let _ = fs::remove_file(&files.socket_path);
    }
    if files.lock_file.exists() {
        let _ = fs::remove_file(&files.lock_file);
    }

    Ok(DaemonLifecycleResponse {
        status: "stopped".to_string(),
        action: "run".to_string(),
        daemon_status: materialize_status(runtime, stopped_state.clone()),
    })
}

fn serve_listener_loop<S, F>(
    runtime: &RuntimeLocations,
    files: &DaemonFiles,
    mut accept_once: F,
    endpoint: String,
    transport: &str,
) -> Result<DaemonLifecycleResponse>
where
    S: Read + Write + Send + 'static,
    F: FnMut() -> std::io::Result<S>,
{
    fs::write(&files.lock_file, std::process::id().to_string())
        .with_context(|| format!("failed to write {}", files.lock_file.display()))?;

    let server_state = Arc::new(Mutex::new(running_state(endpoint, transport)));
    save_state(runtime, &server_state.lock().unwrap().clone())?;

    let should_stop = Arc::new(AtomicBool::new(false));
    let agent_sessions: Arc<Mutex<std::collections::HashMap<String, DaemonAgentConnection>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    while !should_stop.load(Ordering::SeqCst) {
        match accept_once() {
            Ok(stream) => {
                let state = server_state.clone();
                let stop_flag = should_stop.clone();
                let session = agent_sessions.clone();
                let rt = runtime.clone();
                thread::spawn(move || {
                    let _ = handle_connection(&rt, stream, state, stop_flag, session);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(DAEMON_POLL_INTERVAL);
            }
            Err(error) => {
                let mut failed_state = server_state.lock().unwrap();
                failed_state.state = DaemonLifecycleState::Failed;
                failed_state.readiness = "failed".to_string();
                failed_state.last_error = error.to_string();
                failed_state.recommended_next_action = "daemon restart".to_string();
                save_state(runtime, &failed_state.clone())?;
                return Err(error).context("daemon listener failed");
            }
        }
    }

    finalize_stopped_server(runtime, files, &server_state)
}

#[cfg(unix)]
pub fn daemon_run(runtime: &RuntimeLocations) -> Result<DaemonLifecycleResponse> {
    let files = ensure_daemon_directories(runtime)?;
    if files.socket_path.exists() {
        match UnixStream::connect(&files.socket_path) {
            Ok(_) => bail!("daemon is already running"),
            Err(_) => {
                fs::remove_file(&files.socket_path)
                    .with_context(|| format!("failed to remove {}", files.socket_path.display()))?;
            }
        }
    }

    let listener = UnixListener::bind(&files.socket_path)
        .with_context(|| format!("failed to bind {}", files.socket_path.display()))?;
    listener
        .set_nonblocking(true)
        .context("failed to configure daemon listener")?;

    serve_listener_loop::<UnixStream, _>(
        runtime,
        &files,
        || listener.accept().map(|(stream, _)| stream),
        path_to_string(&files.socket_path),
        "unix_socket_or_named_pipe",
    )
}

#[cfg(not(unix))]
pub fn daemon_run(runtime: &RuntimeLocations) -> Result<DaemonLifecycleResponse> {
    let files = ensure_daemon_directories(runtime)?;
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to bind loopback daemon")?;
    listener
        .set_nonblocking(true)
        .context("failed to configure daemon listener")?;
    let endpoint = listener
        .local_addr()
        .context("failed to read loopback daemon address")?
        .to_string();

    serve_listener_loop::<TcpStream, _>(
        runtime,
        &files,
        || listener.accept().map(|(stream, _)| stream),
        endpoint,
        "loopback_tcp",
    )
}

fn handle_connection(
    runtime: &RuntimeLocations,
    mut stream: impl Read + Write,
    state: Arc<Mutex<DaemonStateFile>>,
    should_stop: Arc<AtomicBool>,
    agent_sessions: Arc<Mutex<std::collections::HashMap<String, DaemonAgentConnection>>>,
) -> Result<()> {
    let line = read_rpc_line(&mut stream).context("failed to read daemon request")?;
    if line.trim().is_empty() {
        return Ok(());
    }

    let request: DaemonRpcRequest =
        serde_json::from_str(line.trim()).context("failed to parse daemon request")?;
    let response = match request {
        DaemonRpcRequest::Health => DaemonRpcResponse::Health { ok: true },
        DaemonRpcRequest::Status => {
            let snapshot = state.lock().unwrap().clone();
            DaemonRpcResponse::Status {
                status: Box::new(materialize_status(runtime, snapshot)),
            }
        }
        DaemonRpcRequest::Shutdown => {
            should_stop.store(true, Ordering::SeqCst);
            {
                let mut current = state.lock().unwrap();
                current.state = DaemonLifecycleState::Stopping;
                current.readiness = "stopping".to_string();
                current.recommended_next_action = "daemon start".to_string();
                save_state(runtime, &current.clone())?;
            }
            DaemonRpcResponse::Shutdown { accepted: true }
        }
        DaemonRpcRequest::SessionCreate { agent } => {
            execute_session_create(runtime, &agent, agent_sessions)
        }
        DaemonRpcRequest::SessionList => execute_session_list(agent_sessions),
        DaemonRpcRequest::SessionClose { agent, session_id } => {
            execute_session_close(&agent, &session_id, agent_sessions)
        }
        DaemonRpcRequest::ExecuteRun {
            input,
            agent,
            session_id,
            effective_context,
        } => execute_daemon_run(
            runtime,
            &input,
            &agent,
            session_id,
            &effective_context,
            agent_sessions,
        ),
    };

    let serialized =
        serde_json::to_string(&response).context("failed to serialize daemon response")?;
    stream
        .write_all(serialized.as_bytes())
        .context("failed to write daemon response")?;
    stream
        .write_all(b"\n")
        .context("failed to finish daemon response")?;
    stream.flush().context("failed to flush daemon response")?;
    Ok(())
}

fn connect_agent(runtime: &RuntimeLocations, agent_id: &str) -> Result<DaemonAgentConnection> {
    let executable = crate::acp::resolve_agent_executable(agent_id, &runtime.data_dir)?;
    let args = crate::acp::resolve_agent_args(agent_id, &runtime.data_dir);
    let client = crate::acp::AcpClient::connect(&executable, &args)?;
    Ok(DaemonAgentConnection {
        client,
        sessions: std::collections::HashMap::new(),
        default_session: None,
    })
}

fn execute_session_create(
    runtime: &RuntimeLocations,
    agent_id: &str,
    agent_sessions: Arc<Mutex<std::collections::HashMap<String, DaemonAgentConnection>>>,
) -> DaemonRpcResponse {
    let mut guard = agent_sessions.lock().unwrap();
    if !guard.contains_key(agent_id) {
        match connect_agent(runtime, agent_id) {
            Ok(conn) => {
                guard.insert(agent_id.to_string(), conn);
            }
            Err(e) => {
                return DaemonRpcResponse::Error {
                    code: "daemon.agent_connect_failed".to_string(),
                    message: e.to_string(),
                    recommended_next_action: "verify agent is installed and executable".to_string(),
                };
            }
        }
    }
    let conn = guard.get_mut(agent_id).unwrap();
    let session_id = match conn.client.create_session_id() {
        Ok(id) => id,
        Err(e) => {
            return DaemonRpcResponse::Error {
                code: "daemon.session_create_failed".to_string(),
                message: e.to_string(),
                recommended_next_action: "daemon restart".to_string(),
            };
        }
    };
    let is_first = conn.sessions.is_empty();
    conn.sessions.insert(session_id.clone(), 0);
    if is_first || conn.default_session.is_none() {
        conn.default_session = Some(session_id.clone());
    }
    DaemonRpcResponse::SessionCreate {
        agent: agent_id.to_string(),
        session_id,
    }
}

fn execute_session_list(
    agent_sessions: Arc<Mutex<std::collections::HashMap<String, DaemonAgentConnection>>>,
) -> DaemonRpcResponse {
    let guard = agent_sessions.lock().unwrap();
    let agents: Vec<AgentSessionInfo> = guard
        .iter()
        .map(|(agent_id, conn)| {
            let default = conn.default_session.as_deref().unwrap_or("");
            AgentSessionInfo {
                agent: agent_id.clone(),
                sessions: conn
                    .sessions
                    .iter()
                    .map(|(sid, turns)| SessionInfo {
                        session_id: sid.clone(),
                        turn_count: *turns,
                        is_default: sid == default,
                    })
                    .collect(),
            }
        })
        .collect();
    DaemonRpcResponse::SessionList { agents }
}

fn execute_session_close(
    agent_id: &str,
    session_id: &str,
    agent_sessions: Arc<Mutex<std::collections::HashMap<String, DaemonAgentConnection>>>,
) -> DaemonRpcResponse {
    let mut guard = agent_sessions.lock().unwrap();
    let Some(conn) = guard.get_mut(agent_id) else {
        return DaemonRpcResponse::Error {
            code: "daemon.agent_not_found".to_string(),
            message: format!("no connection for agent '{agent_id}'"),
            recommended_next_action: "use daemon session create first".to_string(),
        };
    };
    if conn.sessions.remove(session_id).is_none() {
        return DaemonRpcResponse::Error {
            code: "daemon.session_not_found".to_string(),
            message: format!("session '{session_id}' not found for agent '{agent_id}'"),
            recommended_next_action: "use daemon session list to see active sessions".to_string(),
        };
    }
    if conn.default_session.as_deref() == Some(session_id) {
        conn.default_session = conn.sessions.keys().next().cloned();
    }
    if conn.sessions.is_empty() {
        guard.remove(agent_id);
    }
    DaemonRpcResponse::SessionClose {
        agent: agent_id.to_string(),
        session_id: session_id.to_string(),
    }
}

fn execute_daemon_run(
    runtime: &RuntimeLocations,
    input: &str,
    agent_id: &str,
    session_id: Option<String>,
    effective_context: &BTreeMap<String, String>,
    agent_sessions: Arc<Mutex<std::collections::HashMap<String, DaemonAgentConnection>>>,
) -> DaemonRpcResponse {
    // Phase 1: Prepare — ensure agent connection, resolve session, update turn count.
    // Temporarily take the connection out of the map so we can use it without holding the lock.
    let mut conn = {
        let mut guard = agent_sessions.lock().unwrap();
        if !guard.contains_key(agent_id) {
            match connect_agent(runtime, agent_id) {
                Ok(c) => {
                    guard.insert(agent_id.to_string(), c);
                }
                Err(e) => {
                    return DaemonRpcResponse::Error {
                        code: "daemon.agent_connect_failed".to_string(),
                        message: e.to_string(),
                        recommended_next_action: "verify agent is installed and executable"
                            .to_string(),
                    };
                }
            }
        }
        guard.remove(agent_id).unwrap()
    };

    let target_session = match &session_id {
        Some(sid) => {
            if !conn.sessions.contains_key(sid) {
                // Put connection back before returning error
                agent_sessions
                    .lock()
                    .unwrap()
                    .insert(agent_id.to_string(), conn);
                return DaemonRpcResponse::Error {
                    code: "daemon.session_not_found".to_string(),
                    message: format!("session '{sid}' not found for agent '{agent_id}'"),
                    recommended_next_action: "use daemon session list to see active sessions"
                        .to_string(),
                };
            }
            sid.clone()
        }
        None => match &conn.default_session {
            Some(sid) => sid.clone(),
            None => match conn.client.create_session_id() {
                Ok(id) => {
                    conn.sessions.insert(id.clone(), 0);
                    conn.default_session = Some(id.clone());
                    id
                }
                Err(e) => {
                    agent_sessions
                        .lock()
                        .unwrap()
                        .insert(agent_id.to_string(), conn);
                    return DaemonRpcResponse::Error {
                        code: "daemon.session_create_failed".to_string(),
                        message: e.to_string(),
                        recommended_next_action: "daemon restart".to_string(),
                    };
                }
            },
        },
    };

    *conn.sessions.get_mut(&target_session).unwrap() += 1;

    // Phase 2: Execute agent I/O without holding the mutex.
    let prompt_result = conn.client.send_prompt_with_session(&target_session, input);

    // Phase 3: Put the connection back and return result.
    agent_sessions
        .lock()
        .unwrap()
        .insert(agent_id.to_string(), conn);

    match prompt_result {
        Ok(result) => {
            let text = crate::acp::extract_text(&result);
            DaemonRpcResponse::RunResult {
                output: crate::AcpAgentCliOutput {
                    status: "ok".to_string(),
                    message: text,
                    input: input.to_string(),
                    effective_context: effective_context.clone(),
                },
            }
        }
        Err(e) => DaemonRpcResponse::Error {
            code: "daemon.agent_error".to_string(),
            message: e.to_string(),
            recommended_next_action: "daemon restart".to_string(),
        },
    }
}

pub fn daemon_routing_error(format: Format, command: &str) -> StructuredError {
    StructuredError::new(
        "daemon.local_only_command",
        format!("the command '{command}' must run locally; daemon routing is unavailable"),
        "daemon_routing",
        format,
    )
    .with_detail("recommended_next_action", "retry without --via daemon")
}

pub fn execute_run_via_daemon(
    runtime: &RuntimeLocations,
    input: &str,
    agent: &str,
    session_id: Option<String>,
    effective_context: BTreeMap<String, String>,
    ensure_daemon: bool,
) -> Result<crate::AcpAgentCliOutput> {
    if ensure_daemon {
        let _ = ensure_daemon_running(runtime)?;
    }

    let status = daemon_status(runtime)?;
    if status.state != "running" {
        bail!("daemon is not running; start it first or pass --ensure-daemon");
    }

    match invoke_daemon_rpc(
        runtime,
        &DaemonRpcRequest::ExecuteRun {
            input: input.to_string(),
            agent: agent.to_string(),
            session_id,
            effective_context,
        },
    )? {
        DaemonRpcResponse::RunResult { output } => Ok(output),
        DaemonRpcResponse::Error {
            code: _,
            message,
            recommended_next_action: _,
        } => bail!("{message}"),
        other => bail!(
            "daemon returned unexpected RPC response for run: {}",
            serde_json::to_string(&other).unwrap_or_else(|_| "<unserializable>".to_string())
        ),
    }
}

pub fn daemon_session_create(runtime: &RuntimeLocations, agent: &str) -> Result<(String, String)> {
    let status = daemon_status(runtime)?;
    if status.state != "running" {
        bail!("daemon is not running; start it first");
    }
    match invoke_daemon_rpc(
        runtime,
        &DaemonRpcRequest::SessionCreate {
            agent: agent.to_string(),
        },
    )? {
        DaemonRpcResponse::SessionCreate { agent, session_id } => Ok((agent, session_id)),
        DaemonRpcResponse::Error { message, .. } => bail!("{message}"),
        other => bail!(
            "daemon returned unexpected RPC response for session create: {}",
            serde_json::to_string(&other).unwrap_or_else(|_| "<unserializable>".to_string())
        ),
    }
}

pub fn daemon_session_list(runtime: &RuntimeLocations) -> Result<Vec<AgentSessionInfo>> {
    let status = daemon_status(runtime)?;
    if status.state != "running" {
        bail!("daemon is not running; start it first");
    }
    match invoke_daemon_rpc(runtime, &DaemonRpcRequest::SessionList)? {
        DaemonRpcResponse::SessionList { agents } => Ok(agents),
        DaemonRpcResponse::Error { message, .. } => bail!("{message}"),
        other => bail!(
            "daemon returned unexpected RPC response for session list: {}",
            serde_json::to_string(&other).unwrap_or_else(|_| "<unserializable>".to_string())
        ),
    }
}

pub fn daemon_session_close(
    runtime: &RuntimeLocations,
    agent: &str,
    session_id: &str,
) -> Result<(String, String)> {
    let status = daemon_status(runtime)?;
    if status.state != "running" {
        bail!("daemon is not running; start it first");
    }
    match invoke_daemon_rpc(
        runtime,
        &DaemonRpcRequest::SessionClose {
            agent: agent.to_string(),
            session_id: session_id.to_string(),
        },
    )? {
        DaemonRpcResponse::SessionClose { agent, session_id } => Ok((agent, session_id)),
        DaemonRpcResponse::Error { message, .. } => bail!("{message}"),
        other => bail!(
            "daemon returned unexpected RPC response for session close: {}",
            serde_json::to_string(&other).unwrap_or_else(|_| "<unserializable>".to_string())
        ),
    }
}
