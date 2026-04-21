//! Baseline CLI entrypoint for the generated package layout. Optional
//! capabilities may extend the package with package-local support files, but
//! repository-owned CI and release automation stay outside generated outputs.

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::path::PathBuf;

use acp_agent_cli::context::{
    InvocationContextOverrides, RuntimeLocations, RuntimeOverrides, inspect_context,
    parse_selectors, persist_active_context, resolve_effective_context, resolve_runtime_locations,
    update_active_context_state,
};
use acp_agent_cli::daemon::{
    daemon_restart, daemon_routing_error, daemon_run, daemon_session_close, daemon_session_create,
    daemon_session_list, daemon_start, daemon_status, daemon_status_response, daemon_stop,
    ensure_daemon_running, execute_run_via_daemon,
};
use acp_agent_cli::help::{plain_text_help, structured_help};
use acp_agent_cli::plan::{is_daemonizable, is_local_only, match_command_tokens, shared_flags};
use acp_agent_cli::repl::{ReplConfig, run_repl};
use acp_agent_cli::{
    Format, StructuredError, run, serialize_value, stream_value, write_structured_error,
};

#[derive(Debug)]
enum AppExit {
    Usage,
    Failure(anyhow::Error),
}

impl From<anyhow::Error> for AppExit {
    fn from(error: anyhow::Error) -> Self {
        Self::Failure(error)
    }
}

/// Official full-function ACP CLI for developers and automation, exposing the complete ACP protocol surface through one unified command-line workflow.
#[derive(Parser, Debug)]
#[command(
    name = "acp-agent-cli",
    version,
    about = "Official full-function ACP CLI for developers and automation, exposing the complete ACP protocol surface through one unified command-line workflow.",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// Output format
    #[arg(long, short, value_enum, global = true, default_value_t = OutputFormat::Yaml)]
    format: OutputFormat,

    /// Render man-like human-readable help for the selected command path
    #[arg(long, short = 'h', global = true, action = ArgAction::SetTrue)]
    help: bool,

    /// Override the default configuration directory
    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,

    /// Override the default durable data directory
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// Override the runtime state directory
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,

    /// Override the cache directory
    #[arg(long, global = true)]
    cache_dir: Option<PathBuf>,

    /// Override the optional log directory
    #[arg(long, global = true)]
    log_dir: Option<PathBuf>,

    /// Emit run-command records incrementally using YAML multi-doc or NDJSON
    #[arg(long, global = true, default_value_t = false)]
    stream: bool,

    /// Override the agent executable or configured agent profile
    #[arg(long, global = true)]
    agent: Option<String>,

    /// Override the workspace for the current invocation
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,

    /// Override the session for the current invocation
    #[arg(long, global = true)]
    session: Option<String>,

    /// Add or override one Active Context selector
    #[arg(long = "selector", global = true, value_name = "KEY=VALUE")]
    selectors: Vec<String>,

    /// Override the working directory context presented to ACP
    #[arg(long = "cwd", global = true)]
    current_directory: Option<PathBuf>,

    /// Route eligible commands through the managed daemon
    #[arg(long, global = true, value_enum, default_value_t = ExecutionMode::Local)]
    via: ExecutionMode,

    /// Start or reuse the daemon before executing a daemon-routed command
    #[arg(long, global = true, default_value_t = false)]
    ensure_daemon: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Yaml,
    Json,
    Toml,
}

impl From<OutputFormat> for Format {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Yaml => Format::Yaml,
            OutputFormat::Json => Format::Json,
            OutputFormat::Toml => Format::Toml,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ExecutionMode {
    Local,
    Daemon,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Return machine-readable help for a command path
    Help(HelpCommand),
    /// Execute the generated leaf command
    Run(RunCommand),
    /// Inspect runtime directory defaults and overrides
    Paths(PathsCommand),
    /// Inspect or persist the Active Context
    Context(ContextCommand),
    /// Open the local ACP session console
    Repl(ReplCommand),
    /// Control the managed daemon lifecycle
    Daemon(DaemonCommand),
    /// Discover, install, and manage ACP agents from a remote registry
    Registry(RegistryCommand),
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Args)]
struct HelpCommand {
    /// Command path to inspect
    #[arg(value_name = "COMMAND_PATH")]
    path: Vec<String>,
}

#[derive(Debug, Args)]
struct RunCommand {
    /// Required input — kept as Option so missing-input returns a structured error in the selected format instead of clap's raw text
    input: Option<String>,

    /// Include the optional log directory in the resolved runtime paths
    #[arg(long)]
    log_enabled: bool,
}

#[derive(Debug, Args)]
struct PathsCommand {
    /// Include the optional log directory in the resolved runtime paths
    #[arg(long)]
    log_enabled: bool,
}

#[derive(Debug, Args)]
struct ContextCommand {
    #[command(subcommand)]
    command: Option<ContextSubcommand>,
}

#[derive(Debug, Subcommand)]
enum ContextSubcommand {
    /// Display the current persisted and effective context
    Show,
    /// Persist selectors and ambient cues as the Active Context
    Use(ContextUseCommand),
}

#[derive(Debug, Args)]
struct ContextUseCommand {
    /// Optional label for the persisted context
    #[arg(long)]
    name: Option<String>,

    /// Clear the persisted Active Context before applying new values
    #[arg(long, default_value_t = false)]
    clear: bool,
}

#[derive(Debug, Args)]
struct ReplCommand {}

#[derive(Debug, Args)]
struct DaemonCommand {
    #[command(subcommand)]
    command: Option<DaemonSubcommand>,
}

#[derive(Debug, Subcommand)]
enum DaemonSubcommand {
    /// Run the daemon in the foreground
    Run(DaemonRunArgs),
    /// Start the daemon in the background
    Start(DaemonStartArgs),
    /// Stop the background daemon
    Stop(DaemonStopArgs),
    /// Restart the background daemon
    Restart(DaemonRestartArgs),
    /// Inspect daemon health and runtime artifacts
    Status,
    /// Manage daemon agent sessions
    Session(DaemonSessionCommand),
}

#[derive(Debug, Args)]
struct DaemonSessionCommand {
    #[command(subcommand)]
    command: DaemonSessionSubcommand,
}

#[derive(Debug, Subcommand)]
enum DaemonSessionSubcommand {
    /// Create a new ACP session for an agent
    Create {
        /// Agent to connect to
        #[arg(long)]
        agent: String,
    },
    /// List active daemon sessions
    List,
    /// Close a daemon session
    Close {
        /// Agent that owns the session
        #[arg(long)]
        agent: String,
        /// Session ID to close
        #[arg(long)]
        session: String,
    },
}

#[derive(Debug, Args)]
struct DaemonRunArgs {
    #[arg(long, value_enum, default_value_t = DaemonTransport::LocalIpc)]
    transport: DaemonTransport,
    #[arg(long, default_value = "127.0.0.1:0")]
    tcp_bind: String,
    #[arg(long, default_value = "ACP_AGENT_CLI_DAEMON_AUTH_TOKEN")]
    auth_token_env: String,
}

#[derive(Debug, Args)]
struct DaemonStartArgs {
    #[arg(long, value_enum, default_value_t = DaemonTransport::LocalIpc)]
    transport: DaemonTransport,
    #[arg(long, default_value = "127.0.0.1:0")]
    tcp_bind: String,
    #[arg(long, default_value = "ACP_AGENT_CLI_DAEMON_AUTH_TOKEN")]
    auth_token_env: String,
    #[arg(long, default_value_t = 30)]
    startup_timeout_sec: u64,
}

#[derive(Debug, Args)]
struct DaemonStopArgs {
    #[arg(long, default_value_t = 30)]
    shutdown_timeout_sec: u64,
}

#[derive(Debug, Args)]
struct DaemonRestartArgs {
    #[arg(long, value_enum, default_value_t = DaemonTransport::LocalIpc)]
    transport: DaemonTransport,
    #[arg(long, default_value = "127.0.0.1:0")]
    tcp_bind: String,
    #[arg(long, default_value = "ACP_AGENT_CLI_DAEMON_AUTH_TOKEN")]
    auth_token_env: String,
    #[arg(long, default_value_t = 30)]
    startup_timeout_sec: u64,
    #[arg(long, default_value_t = 30)]
    shutdown_timeout_sec: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum DaemonTransport {
    LocalIpc,
    Tcp,
}

#[derive(Debug, Args)]
struct RegistryCommand {
    #[command(subcommand)]
    command: Option<RegistrySubcommand>,
}

#[derive(Debug, Subcommand)]
enum RegistrySubcommand {
    /// Search the remote registry for available ACP agents by keyword
    Search(RegistrySearchCommand),
    /// List all available ACP agents from the remote registry
    List(RegistryListCommand),
    /// Show detailed information about a specific agent from the registry
    Show(RegistryShowCommand),
    /// Scan and list locally installed ACP agents
    Installed(RegistryInstalledCommand),
    /// Download and install an ACP agent from the registry
    Install(RegistryInstallCommand),
    /// Remove a locally installed ACP agent
    Uninstall(RegistryUninstallCommand),
    /// Update one or all installed ACP agents to the latest version
    Update(RegistryUpdateCommand),
}

#[derive(Debug, Args)]
struct RegistrySearchCommand {
    /// Search keyword or phrase
    query: Vec<String>,

    /// Filter results by tag; may be specified multiple times
    #[arg(long)]
    tag: Vec<String>,

    /// Filter results by agent category
    #[arg(long)]
    category: Option<String>,

    /// Only return verified or officially curated agents
    #[arg(long, default_value_t = false)]
    verified_only: bool,

    /// Override the registry URL for this invocation
    #[arg(long)]
    registry: Option<String>,

    /// Path to a custom registry configuration file
    #[arg(long)]
    registry_config: Option<PathBuf>,

    /// 1-based page index to return
    #[arg(long, default_value_t = 1)]
    page: u32,

    /// Maximum number of items to return in one page
    #[arg(long, default_value_t = 20)]
    page_size: u32,
}

#[derive(Debug, Args)]
struct RegistryListCommand {
    /// Filter results by tag; may be specified multiple times
    #[arg(long)]
    tag: Vec<String>,

    /// Filter results by agent category
    #[arg(long)]
    category: Option<String>,

    /// Only return verified or officially curated agents
    #[arg(long, default_value_t = false)]
    verified_only: bool,

    /// Override the registry URL for this invocation
    #[arg(long)]
    registry: Option<String>,

    /// Path to a custom registry configuration file
    #[arg(long)]
    registry_config: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RegistryShowCommand {
    /// Agent identifier from the registry
    agent_id: String,

    /// Show details for a specific version; defaults to latest
    #[arg(long, default_value = "latest")]
    version: String,

    /// Override the registry URL for this invocation
    #[arg(long)]
    registry: Option<String>,

    /// Path to a custom registry configuration file
    #[arg(long)]
    registry_config: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RegistryInstalledCommand {
    /// Verify each installed agent is functional by probing its executable
    #[arg(long, default_value_t = false)]
    check_health: bool,

    /// Include installation paths, metadata, and configuration details
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

#[derive(Debug, Args)]
struct RegistryInstallCommand {
    /// Agent identifier from the registry to install
    agent_id: String,

    /// Specific version to install; defaults to latest
    #[arg(long, default_value = "latest")]
    version: String,

    /// Skip confirmation prompts and proceed with installation
    #[arg(long, short, default_value_t = false)]
    yes: bool,

    /// Set the installed agent as the active agent after installation
    #[arg(long, default_value_t = false)]
    set_active: bool,

    /// Override the registry URL for this invocation
    #[arg(long)]
    registry: Option<String>,

    /// Path to a custom registry configuration file
    #[arg(long)]
    registry_config: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RegistryUninstallCommand {
    /// Locally installed agent identifier to remove
    agent_id: String,

    /// Skip confirmation prompts and proceed with removal
    #[arg(long, short, default_value_t = false)]
    yes: bool,

    /// Also remove agent-specific configuration and data
    #[arg(long, default_value_t = false)]
    purge_config: bool,
}

#[derive(Debug, Args)]
struct RegistryUpdateCommand {
    /// Specific agent to update; omit to update all installed agents
    agent_id: Option<String>,

    /// Specific version to update to; defaults to latest
    #[arg(long, default_value = "latest")]
    version: String,

    /// Check for available updates without installing them
    #[arg(long, default_value_t = false)]
    check_only: bool,

    /// Skip confirmation prompts and proceed with updates
    #[arg(long, short, default_value_t = false)]
    yes: bool,

    /// Override the registry URL for this invocation
    #[arg(long)]
    registry: Option<String>,

    /// Path to a custom registry configuration file
    #[arg(long)]
    registry_config: Option<PathBuf>,
}

#[derive(Debug, serde::Serialize)]
struct RunResponse {
    status: String,
    message: String,
    input: Option<String>,
    effective_context: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, serde::Serialize)]
struct ResultEnvelope {
    command: String,
    status: String,
    summary: String,
    data: serde_json::Value,
    context: std::collections::BTreeMap<String, String>,
    next_steps: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, Default)]
struct ExternalGlobalOptions {
    command_tokens: Vec<String>,
    trailing_help: bool,
    agent: Option<String>,
    workspace: Option<PathBuf>,
    session: Option<String>,
    selectors: Vec<String>,
    current_directory: Option<PathBuf>,
}

fn main() {
    let exit_code = match run_cli() {
        Ok(()) => 0,
        Err(AppExit::Usage) => 2,
        Err(AppExit::Failure(error)) => {
            eprintln!("error: {error:#}");
            1
        }
    };

    std::process::exit(exit_code);
}

fn run_cli() -> std::result::Result<(), AppExit> {
    let raw_args: Vec<String> = std::env::args().collect();
    let detected_format = detect_requested_format(&raw_args);

    let cli = match Cli::try_parse_from(&raw_args) {
        Ok(cli) => cli,
        Err(error) => return handle_parse_error(error, detected_format),
    };

    let mut format: Format = cli.format.into();
    if matches!(cli.command, Some(Command::External(_))) {
        format = detected_format;
    }

    if cli.ensure_daemon && cli.via != ExecutionMode::Daemon {
        let error = StructuredError::new(
            "daemon.ensure_requires_via_daemon",
            "`--ensure-daemon` is only valid together with `--via daemon`",
            "daemon_routing",
            format,
        )
        .with_detail(
            "recommended_next_action",
            "retry with --via daemon or remove --ensure-daemon",
        );
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    }

    if cli.help {
        return render_plain_text_help_for_cli(&cli);
    }

    let runtime_overrides = cli_runtime_overrides(&cli);

    if cli.via == ExecutionMode::Daemon
        && !matches!(
            cli.command,
            Some(Command::Run(_)) | Some(Command::External(_))
        )
    {
        let command = local_only_command_name(&cli.command);
        let error = daemon_routing_error(format, command);
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    }

    match cli.command {
        None => render_plain_text_help_for_path(&[]),
        Some(Command::Help(command)) => render_structured_help(&command.path, format),
        Some(Command::Run(ref command)) => execute_run(
            &cli,
            runtime_overrides,
            command,
            format,
            cli.via,
            cli.ensure_daemon,
            cli.stream,
        ),
        Some(Command::Paths(command)) => execute_paths(runtime_overrides, command, format),
        Some(Command::Context(ref command)) => {
            execute_context(&cli, runtime_overrides, command, format)
        }
        Some(Command::Repl(_)) => execute_repl(&cli, runtime_overrides),
        Some(Command::Daemon(command)) => execute_daemon(runtime_overrides, command, format),
        Some(Command::Registry(ref command)) => {
            execute_registry(&cli, runtime_overrides, command, format)
        }
        Some(Command::External(ref tokens)) => {
            execute_external(&cli, runtime_overrides, tokens, format)
        }
    }
}

fn local_only_command_name(command: &Option<Command>) -> &'static str {
    match command {
        None => "help",
        Some(Command::Help(_)) => "help",
        Some(Command::Paths(_)) => "paths",
        Some(Command::Context(ContextCommand { command: None })) => "context",
        Some(Command::Context(ContextCommand {
            command: Some(ContextSubcommand::Show),
        })) => "context show",
        Some(Command::Context(ContextCommand {
            command: Some(ContextSubcommand::Use(_)),
        })) => "context use",
        Some(Command::Repl(_)) => "repl",
        Some(Command::Daemon(_)) => "daemon",
        Some(Command::Registry(_)) => "registry",
        Some(Command::External(_)) => "command",
        Some(Command::Run(_)) => "run",
    }
}

fn handle_parse_error(error: clap::Error, format: Format) -> std::result::Result<(), AppExit> {
    if error.kind() == clap::error::ErrorKind::DisplayVersion {
        error.print().map_err(|err| AppExit::Failure(err.into()))?;
        return Ok(());
    }

    let structured_error =
        StructuredError::new("usage.parse_error", error.to_string(), "help_usage", format);
    let mut stderr = std::io::stderr().lock();
    write_structured_error(&mut stderr, &structured_error, format).map_err(AppExit::from)?;
    Err(AppExit::Usage)
}

fn cli_runtime_overrides(cli: &Cli) -> RuntimeOverrides {
    RuntimeOverrides {
        config_dir: cli.config_dir.clone(),
        data_dir: cli.data_dir.clone(),
        state_dir: cli.state_dir.clone(),
        cache_dir: cli.cache_dir.clone(),
        log_dir: cli.log_dir.clone(),
    }
}

fn detect_requested_format(args: &[String]) -> Format {
    let mut args = args.iter().peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--format=") {
            return parse_format_token(value).unwrap_or(Format::Yaml);
        }
        if (arg == "--format" || arg == "-f")
            && let Some(value) = args.peek()
        {
            return parse_format_token(value).unwrap_or(Format::Yaml);
        }
    }
    Format::Yaml
}

fn parse_format_token(token: &str) -> Option<Format> {
    match token {
        "yaml" => Some(Format::Yaml),
        "json" => Some(Format::Json),
        "toml" => Some(Format::Toml),
        _ => None,
    }
}

fn render_plain_text_help_for_cli(cli: &Cli) -> std::result::Result<(), AppExit> {
    let path = match &cli.command {
        None => Vec::new(),
        Some(Command::Help(_)) => vec!["help".to_string()],
        Some(Command::Run(_)) => vec!["run".to_string()],
        Some(Command::Paths(_)) => vec!["paths".to_string()],
        Some(Command::Context(ContextCommand { command: None })) => vec!["context".to_string()],
        Some(Command::Context(ContextCommand {
            command: Some(ContextSubcommand::Show),
        })) => vec!["context".to_string(), "show".to_string()],
        Some(Command::Context(ContextCommand {
            command: Some(ContextSubcommand::Use(_)),
        })) => vec!["context".to_string(), "use".to_string()],
        Some(Command::Repl(_)) => vec!["repl".to_string()],
        Some(Command::Daemon(DaemonCommand { command: None })) => vec!["daemon".to_string()],
        Some(Command::Daemon(DaemonCommand {
            command: Some(DaemonSubcommand::Run(_)),
        })) => vec!["daemon".to_string(), "run".to_string()],
        Some(Command::Daemon(DaemonCommand {
            command: Some(DaemonSubcommand::Start(_)),
        })) => vec!["daemon".to_string(), "start".to_string()],
        Some(Command::Daemon(DaemonCommand {
            command: Some(DaemonSubcommand::Stop(_)),
        })) => vec!["daemon".to_string(), "stop".to_string()],
        Some(Command::Daemon(DaemonCommand {
            command: Some(DaemonSubcommand::Restart(_)),
        })) => vec!["daemon".to_string(), "restart".to_string()],
        Some(Command::Daemon(DaemonCommand {
            command: Some(DaemonSubcommand::Status),
        })) => vec!["daemon".to_string(), "status".to_string()],
        Some(Command::Daemon(DaemonCommand {
            command: Some(DaemonSubcommand::Session(_)),
        })) => vec!["daemon".to_string(), "session".to_string()],
        Some(Command::Registry(RegistryCommand { command: None })) => vec!["registry".to_string()],
        Some(Command::Registry(RegistryCommand {
            command: Some(RegistrySubcommand::Search(_)),
        })) => vec!["registry".to_string(), "search".to_string()],
        Some(Command::Registry(RegistryCommand {
            command: Some(RegistrySubcommand::List(_)),
        })) => vec!["registry".to_string(), "list".to_string()],
        Some(Command::Registry(RegistryCommand {
            command: Some(RegistrySubcommand::Show(_)),
        })) => vec!["registry".to_string(), "show".to_string()],
        Some(Command::Registry(RegistryCommand {
            command: Some(RegistrySubcommand::Installed(_)),
        })) => vec!["registry".to_string(), "installed".to_string()],
        Some(Command::Registry(RegistryCommand {
            command: Some(RegistrySubcommand::Install(_)),
        })) => vec!["registry".to_string(), "install".to_string()],
        Some(Command::Registry(RegistryCommand {
            command: Some(RegistrySubcommand::Uninstall(_)),
        })) => vec!["registry".to_string(), "uninstall".to_string()],
        Some(Command::Registry(RegistryCommand {
            command: Some(RegistrySubcommand::Update(_)),
        })) => vec!["registry".to_string(), "update".to_string()],
        Some(Command::External(tokens)) => external_help_path(tokens),
    };

    render_plain_text_help_for_path(&path)
}

fn render_plain_text_help_for_path(path: &[String]) -> std::result::Result<(), AppExit> {
    let Some(help_text) = plain_text_help(path) else {
        let mut stderr = std::io::stderr().lock();
        let error = StructuredError::new(
            "help.unknown_path",
            format!("unknown help path '{}'", path.join(" ")),
            "help_usage",
            Format::Yaml,
        );
        write_structured_error(&mut stderr, &error, Format::Yaml).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    };

    println!("{help_text}");
    Ok(())
}

fn render_structured_help(path: &[String], format: Format) -> std::result::Result<(), AppExit> {
    let Some(help_document) = structured_help(path) else {
        let mut stderr = std::io::stderr().lock();
        let error = StructuredError::new(
            "help.unknown_path",
            format!("unknown help path '{}'", path.join(" ")),
            "help_usage",
            format,
        );
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    };

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serialize_value(&mut stdout, &help_document, format).map_err(AppExit::from)?;
    Ok(())
}

fn execute_paths(
    overrides: RuntimeOverrides,
    command: PathsCommand,
    format: Format,
) -> std::result::Result<(), AppExit> {
    let runtime =
        resolve_runtime_locations(&overrides, command.log_enabled).map_err(AppExit::from)?;
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serialize_value(&mut stdout, &runtime.summary(), format).map_err(AppExit::from)?;
    Ok(())
}

fn execute_context(
    cli: &Cli,
    overrides: RuntimeOverrides,
    command: &ContextCommand,
    format: Format,
) -> std::result::Result<(), AppExit> {
    match &command.command {
        None => render_plain_text_help_for_path(&["context".to_string()]),
        Some(ContextSubcommand::Show) => {
            let runtime = resolve_runtime_locations(&overrides, false).map_err(AppExit::from)?;
            let inspection = inspect_context(&runtime, &InvocationContextOverrides::default())
                .map_err(AppExit::from)?;
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            serialize_value(&mut stdout, &inspection, format).map_err(AppExit::from)?;
            Ok(())
        }
        Some(ContextSubcommand::Use(command)) => {
            let selectors = parse_selectors(&cli.selectors).map_err(AppExit::from)?;
            let current_directory = cli.current_directory.clone();
            if selectors.is_empty()
                && current_directory.is_none()
                && cli.agent.is_none()
                && cli.workspace.is_none()
                && cli.session.is_none()
                && command.name.is_none()
                && !command.clear
            {
                let error = StructuredError::new(
                    "context.missing_values",
                    "provide at least one of --agent, --workspace, --session, --selector, --cwd, --name, or --clear when persisting an Active Context",
                    "runtime_state",
                    format,
                );
                let mut stderr = std::io::stderr().lock();
                write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
                return Err(AppExit::Usage);
            }

            let runtime = resolve_runtime_locations(&overrides, false).map_err(AppExit::from)?;
            let existing =
                acp_agent_cli::context::load_active_context(&runtime).map_err(AppExit::from)?;
            let state = update_active_context_state(
                existing.as_ref(),
                command.clear,
                command.name.clone(),
                cli.agent.clone(),
                cli.workspace.clone(),
                cli.session.clone(),
                selectors,
                current_directory,
            );
            let persisted = persist_active_context(&runtime, &state).map_err(AppExit::from)?;
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            serialize_value(&mut stdout, &persisted, format).map_err(AppExit::from)?;
            Ok(())
        }
    }
}

fn execute_run(
    cli: &Cli,
    overrides: RuntimeOverrides,
    command: &RunCommand,
    format: Format,
    execution_mode: ExecutionMode,
    ensure_daemon: bool,
    stream: bool,
) -> std::result::Result<(), AppExit> {
    let runtime =
        resolve_runtime_locations(&overrides, command.log_enabled).map_err(AppExit::from)?;
    let selectors = parse_selectors(&cli.selectors).map_err(AppExit::from)?;
    let invocation_overrides = InvocationContextOverrides {
        agent: cli.agent.clone(),
        workspace: cli.workspace.clone(),
        session_id: cli.session.clone(),
        selectors,
        current_directory: cli.current_directory.clone(),
    };
    let persisted_context =
        acp_agent_cli::context::load_active_context(&runtime).map_err(AppExit::from)?;
    let effective_context =
        resolve_effective_context(persisted_context.as_ref(), &invocation_overrides);

    let Some(input) = command.input.clone() else {
        let error = StructuredError::new(
            "run.missing_input",
            "the run command requires <INPUT>; use --help for man-like human-readable help",
            "leaf_validation",
            format,
        )
        .with_detail("command", "run");
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    };

    let agent_name = cli
        .agent
        .as_deref()
        .or(persisted_context.as_ref().and_then(|c| c.agent.as_deref()));

    let response = match execution_mode {
        ExecutionMode::Local => {
            let agent = match agent_name {
                Some(a) => a,
                None => {
                    let error = StructuredError::new(
                        "run.missing_agent",
                        "no agent specified — use --agent <name> or set an active agent via context",
                        "agent_resolution",
                        format,
                    );
                    let mut stderr = std::io::stderr().lock();
                    write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
                    return Err(AppExit::Usage);
                }
            };
            run(
                &input,
                agent,
                &runtime.data_dir,
                effective_context.effective_values.clone(),
            )
            .map_err(AppExit::from)?
        }
        ExecutionMode::Daemon => {
            let daemon_agent = match agent_name {
                Some(a) => a.to_string(),
                None => {
                    let error = StructuredError::new(
                        "run.missing_agent",
                        "no agent specified — use --agent <name> or set an active agent via context",
                        "agent_resolution",
                        format,
                    );
                    let mut stderr = std::io::stderr().lock();
                    write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
                    return Err(AppExit::Usage);
                }
            };
            match execute_run_via_daemon(
                &runtime,
                &input,
                &daemon_agent,
                cli.session.clone(),
                effective_context.effective_values.clone(),
                ensure_daemon,
            ) {
                Ok(response) => response,
                Err(error) => {
                    let structured = StructuredError::new(
                        "daemon.execution_failed",
                        error.to_string(),
                        "daemon_routing",
                        format,
                    )
                    .with_detail(
                        "recommended_next_action",
                        "run `acp-agent-cli daemon status` or retry locally",
                    );
                    let mut stderr = std::io::stderr().lock();
                    write_structured_error(&mut stderr, &structured, format)
                        .map_err(AppExit::from)?;
                    return Err(AppExit::Usage);
                }
            }
        }
    };

    let output = RunResponse {
        status: response.status,
        message: response.message,
        input: Some(response.input),
        effective_context: response.effective_context,
    };

    if stream {
        return stream_value(&output, format).map_err(AppExit::from);
    }

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serialize_value(&mut stdout, &output, format).map_err(AppExit::from)?;
    Ok(())
}

fn execute_repl(cli: &Cli, overrides: RuntimeOverrides) -> std::result::Result<(), AppExit> {
    let selectors = parse_selectors(&cli.selectors).map_err(AppExit::from)?;
    run_repl(
        &overrides,
        ReplConfig {
            session: cli.session.clone(),
            workspace: cli.workspace.clone(),
            agent: cli.agent.clone(),
            selectors,
            current_directory: cli.current_directory.clone(),
        },
    )
    .map_err(|error| {
        let structured = anyhow::anyhow!(error);
        AppExit::Failure(structured)
    })
}

fn command_path_to_acp_method(path: &[String]) -> String {
    path.iter()
        .map(|segment| {
            let mut result = String::new();
            let mut capitalize_next = false;
            for ch in segment.chars() {
                if ch == '-' {
                    capitalize_next = true;
                } else if capitalize_next {
                    result.push(ch.to_ascii_uppercase());
                    capitalize_next = false;
                } else {
                    result.push(ch);
                }
            }
            result
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn execute_external(
    cli: &Cli,
    overrides: RuntimeOverrides,
    tokens: &[String],
    format: Format,
) -> std::result::Result<(), AppExit> {
    let external_globals = strip_external_global_flags(tokens, format)?;
    let command_tokens = external_globals.command_tokens;
    if external_globals.trailing_help {
        return render_plain_text_help_for_path(&external_help_path(&command_tokens));
    }

    let Some((resolved, remaining)) = match_command_tokens(&command_tokens) else {
        let error = StructuredError::new(
            "command.unknown_path",
            format!("unknown command path '{}'", command_tokens.join(" ")),
            "help_usage",
            format,
        );
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    };

    if resolved.command.kind == "group" && remaining.is_empty() {
        return render_plain_text_help_for_path(&resolved.path);
    }

    if cli.via == ExecutionMode::Daemon && is_local_only(&resolved.path) {
        let error = daemon_routing_error(format, &resolved.path.join(" "));
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    }

    let parsed = parse_external_invocation(
        &resolved.command,
        &resolved.path.join(" "),
        &remaining,
        format,
    )?;
    let runtime = resolve_runtime_locations(&overrides, false).map_err(AppExit::from)?;
    let persisted_context =
        acp_agent_cli::context::load_active_context(&runtime).map_err(AppExit::from)?;
    let mut selectors = parse_selectors(&cli.selectors).map_err(AppExit::from)?;
    selectors.extend(parse_selectors(&external_globals.selectors).map_err(AppExit::from)?);
    let effective_agent = external_globals.agent.or_else(|| cli.agent.clone());
    let effective_workspace = external_globals.workspace.or_else(|| cli.workspace.clone());
    let effective_session = external_globals.session.or_else(|| cli.session.clone());
    let effective_current_directory = external_globals
        .current_directory
        .or_else(|| cli.current_directory.clone());
    let effective_context = resolve_effective_context(
        persisted_context.as_ref(),
        &InvocationContextOverrides {
            agent: effective_agent.clone(),
            workspace: effective_workspace.clone(),
            session_id: effective_session.clone(),
            selectors,
            current_directory: effective_current_directory,
        },
    );

    if cli.via == ExecutionMode::Daemon && !is_daemonizable(&resolved.path) {
        let error = daemon_routing_error(format, &resolved.path.join(" "));
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    }

    if cli.via == ExecutionMode::Daemon {
        let daemon_status = if cli.ensure_daemon {
            ensure_daemon_running(&runtime)
        } else {
            daemon_status(&runtime)
        };

        match daemon_status {
            Ok(status) if status.state == "running" => {}
            Ok(_) => {
                let error = StructuredError::new(
                    "daemon.execution_failed",
                    format!(
                        "the command '{}' requires a running daemon; start it first or pass --ensure-daemon",
                        resolved.path.join(" ")
                    ),
                    "daemon_routing",
                    format,
                )
                .with_detail(
                    "recommended_next_action",
                    "retry with --ensure-daemon or run `acp-agent-cli daemon start`",
                );
                let mut stderr = std::io::stderr().lock();
                write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
                return Err(AppExit::Usage);
            }
            Err(error) => {
                let structured = StructuredError::new(
                    "daemon.execution_failed",
                    error.to_string(),
                    "daemon_routing",
                    format,
                )
                .with_detail(
                    "recommended_next_action",
                    "run `acp-agent-cli daemon status` or retry with --ensure-daemon",
                );
                let mut stderr = std::io::stderr().lock();
                write_structured_error(&mut stderr, &structured, format).map_err(AppExit::from)?;
                return Err(AppExit::Usage);
            }
        }
    }

    let acp_method = command_path_to_acp_method(&resolved.path);

    if cli.via == ExecutionMode::Daemon {
        let envelope = ResultEnvelope {
            command: resolved.path.join(" "),
            status: "accepted_daemon".to_string(),
            summary: resolved.command.summary.clone(),
            data: json!({
                "method": acp_method,
                "positionals": parsed.positionals,
                "flags": parsed.flags,
                "routing": "daemon",
                "workspace": effective_workspace.as_ref().map(|path| path.display().to_string()),
                "session": effective_session,
                "agent": effective_agent,
            }),
            context: effective_context.effective_values,
            next_steps: vec![
                "Daemon-mode ACP dispatch will be available in a follow-up release.".to_string(),
            ],
            errors: Vec::new(),
        };

        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();
        serialize_value(&mut stdout, &envelope, format).map_err(AppExit::from)?;
        return Ok(());
    }

    // Local mode: connect to agent and send ACP request
    let agent_name = match effective_agent {
        Some(ref a) => a.as_str(),
        None => {
            let structured = StructuredError::new(
                "external.missing_agent",
                "no agent specified; use --agent <name> or set a default with context use --agent <name>".to_string(),
                "external_validation",
                format,
            );
            let mut stderr = std::io::stderr().lock();
            write_structured_error(&mut stderr, &structured, format).map_err(AppExit::from)?;
            return Err(AppExit::Usage);
        }
    };

    let executable = acp_agent_cli::acp::resolve_agent_executable(agent_name, &runtime.data_dir)
        .map_err(|e| {
            let structured = StructuredError::new(
                "external.agent_not_found",
                e.to_string(),
                "agent_resolution",
                format,
            );
            let mut stderr = std::io::stderr().lock();
            let _ = write_structured_error(&mut stderr, &structured, format);
            AppExit::from(e)
        })?;
    let args = acp_agent_cli::acp::resolve_agent_args(agent_name, &runtime.data_dir);

    let mut client = acp_agent_cli::acp::AcpClient::connect(&executable, &args).map_err(|e| {
        let structured = StructuredError::new(
            "external.agent_connect_failed",
            e.to_string(),
            "agent_transport",
            format,
        );
        let mut stderr = std::io::stderr().lock();
        let _ = write_structured_error(&mut stderr, &structured, format);
        AppExit::from(e)
    })?;

    let mut params = json!({});
    let session_id = if let Some(ref sid) = effective_session {
        sid.clone()
    } else {
        client.create_session().map_err(|e| {
            let structured = StructuredError::new(
                "external.session_create_failed",
                e.to_string(),
                "acp_protocol",
                format,
            );
            let mut stderr = std::io::stderr().lock();
            let _ = write_structured_error(&mut stderr, &structured, format);
            AppExit::from(e)
        })?
    };
    params["sessionId"] = json!(session_id);
    if let Some(ref ws) = effective_workspace {
        params["workspace"] = json!(ws.display().to_string());
    }
    for (key, value) in &parsed.positionals {
        params[key] = value.clone();
    }
    for (key, value) in &parsed.flags {
        params[key] = value.clone();
    }

    let (result, _notifications) = client.raw_request(&acp_method, params).map_err(|e| {
        let structured =
            StructuredError::new("external.acp_error", e.to_string(), "acp_protocol", format);
        let mut stderr = std::io::stderr().lock();
        let _ = write_structured_error(&mut stderr, &structured, format);
        AppExit::from(e)
    })?;

    client.shutdown().map_err(AppExit::from)?;

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
    Ok(())
}

fn external_help_path(tokens: &[String]) -> Vec<String> {
    match match_command_tokens(tokens) {
        Some((resolved, _)) => resolved.path,
        None => tokens.iter().take(2).cloned().collect(),
    }
}

fn strip_external_global_flags(
    tokens: &[String],
    format: Format,
) -> std::result::Result<ExternalGlobalOptions, AppExit> {
    let mut stripped = Vec::new();
    let mut options = ExternalGlobalOptions::default();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        if let Some(agent) = token.strip_prefix("--agent=") {
            options.agent = Some(agent.to_string());
            index += 1;
            continue;
        }
        if let Some(workspace) = token.strip_prefix("--workspace=") {
            options.workspace = Some(PathBuf::from(workspace));
            index += 1;
            continue;
        }
        if let Some(session) = token.strip_prefix("--session=") {
            options.session = Some(session.to_string());
            index += 1;
            continue;
        }
        if let Some(selector) = token.strip_prefix("--selector=") {
            options.selectors.push(selector.to_string());
            index += 1;
            continue;
        }
        if let Some(current_directory) = token.strip_prefix("--cwd=") {
            options.current_directory = Some(PathBuf::from(current_directory));
            index += 1;
            continue;
        }

        match token {
            "--format" | "-f" => {
                require_external_global_value(tokens, index, format)?;
                index += 2;
            }
            value if value.starts_with("--format=") => {
                index += 1;
            }
            "--help" | "-h" => {
                options.trailing_help = true;
                index += 1;
            }
            "--agent" => {
                options.agent = Some(require_external_global_value(tokens, index, format)?);
                index += 2;
            }
            "--workspace" => {
                options.workspace = Some(PathBuf::from(require_external_global_value(
                    tokens, index, format,
                )?));
                index += 2;
            }
            "--session" => {
                options.session = Some(require_external_global_value(tokens, index, format)?);
                index += 2;
            }
            "--selector" => {
                options
                    .selectors
                    .push(require_external_global_value(tokens, index, format)?);
                index += 2;
            }
            "--cwd" => {
                options.current_directory = Some(PathBuf::from(require_external_global_value(
                    tokens, index, format,
                )?));
                index += 2;
            }
            _ => {
                stripped.push(tokens[index].clone());
                index += 1;
            }
        }
    }
    options.command_tokens = stripped;
    Ok(options)
}

fn require_external_global_value(
    tokens: &[String],
    index: usize,
    format: Format,
) -> std::result::Result<String, AppExit> {
    let Some(value) = tokens.get(index + 1) else {
        let flag = &tokens[index];
        let error = StructuredError::new(
            "usage.parse_error",
            format!("flag `{flag}` requires a value"),
            "help_usage",
            format,
        );
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    };

    Ok(value.clone())
}

fn execute_daemon(
    overrides: RuntimeOverrides,
    command: DaemonCommand,
    format: Format,
) -> std::result::Result<(), AppExit> {
    let runtime = resolve_runtime_locations(&overrides, true).map_err(AppExit::from)?;

    match &command.command {
        None => return render_plain_text_help_for_path(&["daemon".to_string()]),
        Some(DaemonSubcommand::Session(session_cmd)) => {
            return execute_daemon_session(&runtime, session_cmd, format);
        }
        _ => {}
    }

    let action = match &command.command {
        Some(DaemonSubcommand::Run(_)) => "run",
        Some(DaemonSubcommand::Start(_)) => "start",
        Some(DaemonSubcommand::Stop(_)) => "stop",
        Some(DaemonSubcommand::Restart(_)) => "restart",
        Some(DaemonSubcommand::Status) => "status",
        _ => unreachable!("handled above"),
    };

    let response = match command.command {
        Some(DaemonSubcommand::Run(_command)) => daemon_run(&runtime),
        Some(DaemonSubcommand::Start(_command)) => daemon_start(&runtime),
        Some(DaemonSubcommand::Stop(_command)) => daemon_stop(&runtime),
        Some(DaemonSubcommand::Restart(_command)) => daemon_restart(&runtime),
        Some(DaemonSubcommand::Status) => daemon_status_response(&runtime),
        _ => unreachable!("handled above"),
    }
    .map_err(|error| {
        let structured = StructuredError::new(
            "daemon.lifecycle_failed",
            format!("daemon {action} failed: {error}"),
            "daemon_lifecycle",
            format,
        )
        .with_detail(
            "recommended_next_action",
            "run `acp-agent-cli daemon status` or inspect state/daemon artifacts",
        );
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &structured, format)
            .expect("failed to write daemon lifecycle error");
        AppExit::Usage
    })?;

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serialize_value(&mut stdout, &response, format).map_err(AppExit::from)?;
    Ok(())
}

fn execute_daemon_session(
    runtime: &RuntimeLocations,
    command: &DaemonSessionCommand,
    format: Format,
) -> std::result::Result<(), AppExit> {
    use serde::Serialize;

    #[derive(Serialize)]
    struct SessionCreateResult {
        agent: String,
        session_id: String,
    }

    match &command.command {
        DaemonSessionSubcommand::Create { agent } => {
            let (agent, session_id) =
                daemon_session_create(runtime, agent).map_err(AppExit::Failure)?;
            let result = SessionCreateResult { agent, session_id };
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
            Ok(())
        }
        DaemonSessionSubcommand::List => {
            let agents = daemon_session_list(runtime).map_err(AppExit::Failure)?;
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            serialize_value(&mut stdout, &agents, format).map_err(AppExit::from)?;
            Ok(())
        }
        DaemonSessionSubcommand::Close { agent, session } => {
            let _ = daemon_session_close(runtime, agent, session).map_err(AppExit::Failure)?;
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            serialize_value(
                &mut stdout,
                &serde_json::json!({"status": "ok", "action": "session_close"}),
                format,
            )
            .map_err(AppExit::from)?;
            Ok(())
        }
    }
}

fn execute_registry(
    cli: &Cli,
    overrides: RuntimeOverrides,
    command: &RegistryCommand,
    format: Format,
) -> std::result::Result<(), AppExit> {
    use acp_agent_cli::registry;

    let runtime = resolve_runtime_locations(&overrides, false).map_err(AppExit::from)?;
    let config_dir = runtime.config_dir.as_path();
    let data_dir = runtime.data_dir.as_path();

    let _ = cli; // used for global options

    match &command.command {
        None => render_plain_text_help_for_path(&["registry".to_string()]),
        Some(RegistrySubcommand::Search(cmd)) => {
            let reg_config =
                registry::load_registry_config(config_dir, cmd.registry_config.as_deref())
                    .map_err(|e| registry_error("registry.search", e, format))?;
            let base_url =
                registry::registry_url_with_override(&reg_config, cmd.registry.as_deref());
            let query = cmd.query.join(" ");
            let result = registry::cmd_search(
                &base_url,
                &query,
                &cmd.tag,
                cmd.category.as_deref(),
                cmd.verified_only,
                cmd.page,
                cmd.page_size,
            )
            .map_err(|e| registry_error("registry.search", e, format))?;
            let mut stdout = std::io::stdout();
            serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
            Ok(())
        }
        Some(RegistrySubcommand::List(cmd)) => {
            let reg_config =
                registry::load_registry_config(config_dir, cmd.registry_config.as_deref())
                    .map_err(|e| registry_error("registry.list", e, format))?;
            let base_url =
                registry::registry_url_with_override(&reg_config, cmd.registry.as_deref());
            let result = registry::cmd_list(
                &base_url,
                &cmd.tag,
                cmd.category.as_deref(),
                cmd.verified_only,
            )
            .map_err(|e| registry_error("registry.list", e, format))?;
            let mut stdout = std::io::stdout();
            serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
            Ok(())
        }
        Some(RegistrySubcommand::Show(cmd)) => {
            let reg_config =
                registry::load_registry_config(config_dir, cmd.registry_config.as_deref())
                    .map_err(|e| registry_error("registry.show", e, format))?;
            let base_url =
                registry::registry_url_with_override(&reg_config, cmd.registry.as_deref());
            let result = registry::cmd_show(
                &base_url,
                &cmd.agent_id,
                version_if_not_latest(&cmd.version),
            )
            .map_err(|e| registry_error("registry.show", e, format))?;
            let mut stdout = std::io::stdout();
            serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
            Ok(())
        }
        Some(RegistrySubcommand::Installed(cmd)) => {
            let result = registry::cmd_installed(data_dir, cmd.check_health, cmd.verbose)
                .map_err(|e| registry_error("registry.installed", e, format))?;
            let mut stdout = std::io::stdout();
            serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
            Ok(())
        }
        Some(RegistrySubcommand::Install(cmd)) => {
            let reg_config =
                registry::load_registry_config(config_dir, cmd.registry_config.as_deref())
                    .map_err(|e| registry_error("registry.install", e, format))?;
            let base_url =
                registry::registry_url_with_override(&reg_config, cmd.registry.as_deref());
            let result = registry::cmd_install(
                &base_url,
                data_dir,
                &cmd.agent_id,
                version_if_not_latest(&cmd.version),
                cmd.set_active,
                config_dir,
            )
            .map_err(|e| registry_error("registry.install", e, format))?;

            if cmd.set_active {
                let runtime =
                    resolve_runtime_locations(&overrides, false).map_err(AppExit::from)?;
                let existing =
                    acp_agent_cli::context::load_active_context(&runtime).map_err(AppExit::from)?;
                let state = update_active_context_state(
                    existing.as_ref(),
                    false,
                    None,
                    Some(cmd.agent_id.clone()),
                    None,
                    None,
                    std::collections::BTreeMap::new(),
                    None,
                );
                persist_active_context(&runtime, &state).map_err(AppExit::from)?;
            }

            let mut stdout = std::io::stdout();
            serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
            Ok(())
        }
        Some(RegistrySubcommand::Uninstall(cmd)) => {
            let result =
                registry::cmd_uninstall(data_dir, &cmd.agent_id, cmd.purge_config, config_dir)
                    .map_err(|e| registry_error("registry.uninstall", e, format))?;
            let mut stdout = std::io::stdout();
            serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
            Ok(())
        }
        Some(RegistrySubcommand::Update(cmd)) => {
            let reg_config =
                registry::load_registry_config(config_dir, cmd.registry_config.as_deref())
                    .map_err(|e| registry_error("registry.update", e, format))?;
            let base_url =
                registry::registry_url_with_override(&reg_config, cmd.registry.as_deref());
            let result = registry::cmd_update(
                &base_url,
                data_dir,
                cmd.agent_id.as_deref(),
                cmd.check_only,
                version_if_not_latest(&cmd.version),
            )
            .map_err(|e| registry_error("registry.update", e, format))?;
            let mut stdout = std::io::stdout();
            serialize_value(&mut stdout, &result, format).map_err(AppExit::from)?;
            Ok(())
        }
    }
}

fn version_if_not_latest(version: &str) -> Option<&str> {
    if version == "latest" {
        None
    } else {
        Some(version)
    }
}

fn registry_error(command: &str, err: anyhow::Error, format: Format) -> AppExit {
    let error = StructuredError::new(
        &format!("{}.failed", command),
        err.to_string(),
        command,
        format,
    );
    let mut stderr = std::io::stderr().lock();
    let _ = write_structured_error(&mut stderr, &error, format);
    AppExit::Usage
}

#[derive(Debug)]
struct ParsedExternalInvocation {
    positionals: std::collections::BTreeMap<String, serde_json::Value>,
    flags: std::collections::BTreeMap<String, serde_json::Value>,
}

fn parse_external_invocation(
    command: &acp_agent_cli::plan::PlannedCommand,
    command_path: &str,
    tokens: &[String],
    format: Format,
) -> std::result::Result<ParsedExternalInvocation, AppExit> {
    let mut free = Vec::new();
    let mut flags = std::collections::BTreeMap::new();
    let all_flags = command
        .flags
        .iter()
        .chain(shared_flags(&command.shared_flag_sets).iter())
        .cloned()
        .collect::<Vec<_>>();

    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if !token.starts_with("--") {
            free.push(token.clone());
            index += 1;
            continue;
        }

        let Some(flag_spec) = all_flags.iter().find(|flag| flag.name == *token) else {
            let error = StructuredError::new(
                "command.unknown_flag",
                format!("unknown flag `{token}` for command `{command_path}`"),
                "help_usage",
                format,
            );
            let mut stderr = std::io::stderr().lock();
            write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
            return Err(AppExit::Usage);
        };

        if flag_spec.value_type == "bool" {
            flags.insert(flag_spec.name.clone(), serde_json::Value::Bool(true));
            index += 1;
            continue;
        }

        let Some(value) = tokens.get(index + 1) else {
            let error = StructuredError::new(
                "command.missing_flag_value",
                format!("flag `{}` requires a value", flag_spec.name),
                "leaf_validation",
                format,
            );
            let mut stderr = std::io::stderr().lock();
            write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
            return Err(AppExit::Usage);
        };

        if flag_spec.repeatable {
            let current = flags
                .entry(flag_spec.name.clone())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
            if let serde_json::Value::Array(values) = current {
                values.push(serde_json::Value::String(value.clone()));
            }
        } else {
            flags.insert(
                flag_spec.name.clone(),
                serde_json::Value::String(value.clone()),
            );
        }
        index += 2;
    }

    for flag in &all_flags {
        if flag.required && !flags.contains_key(&flag.name) {
            let error = StructuredError::new(
                "command.missing_required_flag",
                format!(
                    "missing required flag `{}` for command `{command_path}`",
                    flag.name
                ),
                "leaf_validation",
                format,
            );
            let mut stderr = std::io::stderr().lock();
            write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
            return Err(AppExit::Usage);
        }
    }

    let mut positionals = std::collections::BTreeMap::new();
    let mut free_index = 0;
    for (position, positional) in command.positionals.iter().enumerate() {
        let is_last = position + 1 == command.positionals.len();
        if positional.value_type == "string_list" && is_last {
            let remaining = free[free_index..].to_vec();
            if positional.required && remaining.is_empty() {
                let error = StructuredError::new(
                    "command.missing_required_input",
                    format!(
                        "missing required positional `{}` for command `{command_path}`",
                        positional.name
                    ),
                    "leaf_validation",
                    format,
                );
                let mut stderr = std::io::stderr().lock();
                write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
                return Err(AppExit::Usage);
            }
            positionals.insert(positional.name.clone(), json!(remaining));
            free_index = free.len();
            continue;
        }

        match free.get(free_index) {
            Some(value) => {
                positionals.insert(positional.name.clone(), json!(value));
                free_index += 1;
            }
            None if positional.required => {
                let error = StructuredError::new(
                    "command.missing_required_input",
                    format!(
                        "missing required positional `{}` for command `{command_path}`",
                        positional.name
                    ),
                    "leaf_validation",
                    format,
                );
                let mut stderr = std::io::stderr().lock();
                write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
                return Err(AppExit::Usage);
            }
            None => {}
        }
    }

    if free_index < free.len() {
        let unexpected = free[free_index..].join(" ");
        let error = StructuredError::new(
            "command.unexpected_input",
            format!("unexpected positional input `{unexpected}` for command `{command_path}`"),
            "help_usage",
            format,
        );
        let mut stderr = std::io::stderr().lock();
        write_structured_error(&mut stderr, &error, format).map_err(AppExit::from)?;
        return Err(AppExit::Usage);
    }

    Ok(ParsedExternalInvocation { positionals, flags })
}
