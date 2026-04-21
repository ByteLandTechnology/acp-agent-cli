use crate::plan::{
    capability_in_scope, cli_plan, find_command, planned_output_formats, purpose_summary,
    shared_flags, top_level_commands, value_to_string,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HelpOption {
    pub name: String,
    pub value_name: String,
    pub default_value: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HelpSubcommand {
    pub name: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HelpExample {
    pub command: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExitCodeSpec {
    pub code: i32,
    pub meaning: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDirectoryHelp {
    pub config: String,
    pub data: String,
    pub state: String,
    pub cache: String,
    pub logs: String,
    pub scope: String,
    pub overrides: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveContextHelp {
    pub persisted_values: Vec<String>,
    pub ambient_cues: Vec<String>,
    pub inspection_command: String,
    pub switch_command: String,
    pub precedence_rule: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureAvailability {
    pub streaming: String,
    pub repl: String,
    pub daemon: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplContractHelp {
    pub model: String,
    pub requires_active_session: bool,
    pub session_resolution: String,
    pub default_input_behavior: String,
    pub help_channel: String,
    pub slash_commands: Vec<String>,
    pub exit_triggers: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonContractHelp {
    pub mode: String,
    pub instance_model: String,
    pub local_default_transport: String,
    pub tcp_transport: String,
    pub local_ipc_auth: String,
    pub tcp_auth: String,
    pub rpc_protocol: String,
    pub rpc_methods: Vec<String>,
    pub runtime_artifact_directory: String,
    pub runtime_artifact_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HelpDocument {
    pub command_path: Vec<String>,
    pub purpose: String,
    pub usage: String,
    pub arguments: Vec<String>,
    pub options: Vec<HelpOption>,
    pub subcommands: Vec<HelpSubcommand>,
    pub output_formats: Vec<String>,
    pub exit_behavior: Vec<ExitCodeSpec>,
    pub runtime_directories: RuntimeDirectoryHelp,
    pub active_context: ActiveContextHelp,
    pub feature_availability: FeatureAvailability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repl_contract: Option<ReplContractHelp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_contract: Option<DaemonContractHelp>,
    #[serde(skip_serializing)]
    pub description: Vec<String>,
    #[serde(skip_serializing)]
    pub examples: Vec<HelpExample>,
}

fn runtime_directory_help() -> RuntimeDirectoryHelp {
    RuntimeDirectoryHelp {
        config: "User-authored configuration (user-scoped by default)".to_string(),
        data: "Durable CLI-managed business data".to_string(),
        state: "Recoverable runtime state, history, persisted Active Context, and state/daemon artifacts".to_string(),
        cache: "Disposable or reproducible artifacts".to_string(),
        logs: "Optional log path beneath state by default when logging is enabled".to_string(),
        scope: "user_scoped_default".to_string(),
        overrides: vec![
            "--config-dir".to_string(),
            "--data-dir".to_string(),
            "--state-dir".to_string(),
            "--cache-dir".to_string(),
            "--log-dir".to_string(),
        ],
    }
}

fn active_context_help() -> ActiveContextHelp {
    ActiveContextHelp {
        persisted_values: vec![
            "agent".to_string(),
            "workspace".to_string(),
            "session_id".to_string(),
            "selector key/value pairs".to_string(),
        ],
        ambient_cues: vec!["current_directory".to_string()],
        inspection_command: "acp-agent-cli context show".to_string(),
        switch_command: "acp-agent-cli context use --workspace /abs/path --session 42"
            .to_string(),
        precedence_rule: "explicit invocation values override the persisted Active Context for one invocation only".to_string(),
    }
}

fn feature_availability() -> FeatureAvailability {
    FeatureAvailability {
        streaming: if capability_in_scope("stream") {
            "enabled for session prompt, events follow, terminal output, and passthrough streaming"
                .to_string()
        } else {
            "not enabled".to_string()
        },
        repl: if capability_in_scope("repl") {
            "enabled as a local session console".to_string()
        } else {
            "not enabled".to_string()
        },
        daemon: if capability_in_scope("daemon") {
            "enabled in single-instance app-server mode".to_string()
        } else {
            "not enabled".to_string()
        },
    }
}

fn top_level_help() -> HelpDocument {
    HelpDocument {
        command_path: Vec::new(),
        purpose: purpose_summary().to_string(),
        usage: "acp-agent-cli [OPTIONS] <COMMAND>".to_string(),
        arguments: Vec::new(),
        options: vec![
            HelpOption {
                name: "--format".to_string(),
                value_name: "yaml|json|toml".to_string(),
                default_value: "yaml".to_string(),
                description: "Select the structured output format for one-shot commands and structured help".to_string(),
            },
            HelpOption {
                name: "--config-dir".to_string(),
                value_name: "PATH".to_string(),
                default_value: "platform default".to_string(),
                description: "Override the default configuration directory".to_string(),
            },
            HelpOption {
                name: "--data-dir".to_string(),
                value_name: "PATH".to_string(),
                default_value: "platform default".to_string(),
                description: "Override the default durable data directory".to_string(),
            },
            HelpOption {
                name: "--state-dir".to_string(),
                value_name: "PATH".to_string(),
                default_value: "derived from data".to_string(),
                description: "Override the runtime state directory".to_string(),
            },
            HelpOption {
                name: "--cache-dir".to_string(),
                value_name: "PATH".to_string(),
                default_value: "platform default".to_string(),
                description: "Override the cache directory".to_string(),
            },
            HelpOption {
                name: "--log-dir".to_string(),
                value_name: "PATH".to_string(),
                default_value: "state/logs when enabled".to_string(),
                description: "Override the optional log directory".to_string(),
            },
            HelpOption {
                name: "--help".to_string(),
                value_name: "-".to_string(),
                default_value: "false".to_string(),
                description: "Render man-like human-readable help for the selected command path".to_string(),
            },
            HelpOption {
                name: "--version, -V".to_string(),
                value_name: "-".to_string(),
                default_value: "false".to_string(),
                description: "Print version and exit".to_string(),
            },
        ],
        subcommands: top_level_subcommands(),
        output_formats: vec!["yaml".to_string(), "json".to_string(), "toml".to_string()],
        exit_behavior: vec![
            ExitCodeSpec {
                code: 0,
                meaning: "Success or human-readable help".to_string(),
            },
            ExitCodeSpec {
                code: 2,
                meaning: "Structured usage or validation error".to_string(),
            },
        ],
        runtime_directories: runtime_directory_help(),
        active_context: active_context_help(),
        feature_availability: feature_availability(),
        repl_contract: None,
        daemon_contract: None,
        description: vec![
            purpose_summary().to_string(),
            "This command surface reuses the approved description contract across Cargo metadata, SKILL.md, README, and help text.".to_string(),
            "This binary exposes the approved ACP workflow groups for agent control, sessions, proposals, files, code, terminals, events, passthrough, REPL, and daemon lifecycle.".to_string(),
            "Package-local packaging-ready support may appear only when enabled capabilities require them; repository-owned CI automation remains outside generated skill packages.".to_string(),
        ],
        examples: vec![
            HelpExample {
                command: "acp-agent-cli help session prompt --format yaml".to_string(),
                description: "Inspect structured help for a planned ACP session command".to_string(),
            },
            HelpExample {
                command: "acp-agent-cli repl --session local-1".to_string(),
                description: "Open the local ACP session console".to_string(),
            },
        ],
    }
}

fn help_subcommand_help() -> HelpDocument {
    HelpDocument {
        command_path: vec!["help".to_string()],
        purpose: "Return machine-readable help for a command path".to_string(),
        usage: "acp-agent-cli help [COMMAND_PATH ...] [--format yaml|json|toml]".to_string(),
        arguments: vec![
            "COMMAND_PATH: optional command path such as run, paths, context use, or daemon start"
                .to_string(),
        ],
        options: Vec::new(),
        subcommands: Vec::new(),
        output_formats: vec!["yaml".to_string(), "json".to_string(), "toml".to_string()],
        exit_behavior: vec![
            ExitCodeSpec {
                code: 0,
                meaning: "The structured help document was returned".to_string(),
            },
            ExitCodeSpec {
                code: 2,
                meaning: "The requested help path was unknown".to_string(),
            },
        ],
        runtime_directories: runtime_directory_help(),
        active_context: active_context_help(),
        feature_availability: feature_availability(),
        repl_contract: None,
        daemon_contract: None,
        description: vec![
            "Use this subcommand when you need machine-readable command metadata.".to_string(),
        ],
        examples: vec![
            HelpExample {
                command: "acp-agent-cli help run --format yaml".to_string(),
                description: "Inspect the run command as structured YAML".to_string(),
            },
            HelpExample {
                command: "acp-agent-cli help daemon status --format json".to_string(),
                description: "Inspect a daemon lifecycle command as structured JSON".to_string(),
            },
        ],
    }
}

fn paths_help() -> HelpDocument {
    HelpDocument {
        command_path: vec!["paths".to_string()],
        purpose: "Inspect runtime directory defaults and explicit overrides".to_string(),
        usage: "acp-agent-cli paths [OPTIONS]".to_string(),
        arguments: Vec::new(),
        options: vec![HelpOption {
            name: "--log-enabled".to_string(),
            value_name: "-".to_string(),
            default_value: "false".to_string(),
            description: "Include the optional log directory in the output".to_string(),
        }],
        subcommands: Vec::new(),
        output_formats: vec!["yaml".to_string(), "json".to_string(), "toml".to_string()],
        exit_behavior: vec![ExitCodeSpec {
            code: 0,
            meaning: "The runtime path summary was returned".to_string(),
        }],
        runtime_directories: runtime_directory_help(),
        active_context: active_context_help(),
        feature_availability: feature_availability(),
        repl_contract: None,
        daemon_contract: None,
        description: vec![
            "Use this command to inspect config, data, state, cache, and optional log locations."
                .to_string(),
        ],
        examples: vec![
            HelpExample {
                command: "acp-agent-cli paths".to_string(),
                description: "Inspect the standard runtime directory family".to_string(),
            },
            HelpExample {
                command: "acp-agent-cli paths --log-enabled".to_string(),
                description: "Inspect the optional log directory as well".to_string(),
            },
        ],
    }
}

pub fn structured_help(path: &[String]) -> Option<HelpDocument> {
    match path {
        [] => Some(top_level_help()),
        [one] if one == "help" => Some(help_subcommand_help()),
        [one] if one == "paths" => Some(paths_help()),
        _ if path.starts_with(&["context".to_string()])
            || path.starts_with(&["daemon".to_string()]) =>
        {
            plan_help(path)
        }
        _ => plan_help(path),
    }
}

fn top_level_subcommands() -> Vec<HelpSubcommand> {
    top_level_commands()
        .iter()
        .map(|command| HelpSubcommand {
            name: command.name.clone(),
            summary: command.summary.clone(),
        })
        .collect()
}

fn plan_help(path: &[String]) -> Option<HelpDocument> {
    let resolved = find_command(path)?;
    let shared_options = shared_flags(&resolved.command.shared_flag_sets);
    let mut options = resolved
        .command
        .flags
        .iter()
        .chain(shared_options.iter())
        .map(|flag| HelpOption {
            name: flag.name.clone(),
            value_name: flag_value_name(flag),
            default_value: value_to_string(&flag.default, "none"),
            description: flag.description.clone(),
        })
        .collect::<Vec<_>>();

    if path.len() == 1 && resolved.command.name == "repl" {
        options.push(HelpOption {
            name: "--help".to_string(),
            value_name: "-".to_string(),
            default_value: "false".to_string(),
            description: "Render plain-text help for the REPL command".to_string(),
        });
    }

    let arguments = resolved
        .command
        .positionals
        .iter()
        .map(|positional| {
            format!(
                "{}: {}",
                positional.name.to_uppercase(),
                positional.description
            )
        })
        .collect::<Vec<_>>();

    Some(HelpDocument {
        command_path: path.to_vec(),
        purpose: resolved.command.summary.clone(),
        usage: command_usage(path, &resolved.command),
        arguments,
        options,
        subcommands: resolved
            .command
            .subcommands
            .iter()
            .map(|command| HelpSubcommand {
                name: command.name.clone(),
                summary: command.summary.clone(),
            })
            .collect(),
        output_formats: planned_output_formats(&resolved.command),
        exit_behavior: generic_exit_behavior(&resolved.command.kind),
        runtime_directories: runtime_directory_help(),
        active_context: active_context_help(),
        feature_availability: feature_availability(),
        repl_contract: repl_contract_help(path),
        daemon_contract: daemon_contract_help(path),
        description: plan_description(path, &resolved.command),
        examples: plan_examples(path, &resolved.command),
    })
}

fn repl_contract_help(path: &[String]) -> Option<ReplContractHelp> {
    if path != ["repl"] {
        return None;
    }

    let repl = cli_plan().repl_contract.as_ref()?;
    Some(ReplContractHelp {
        model: repl.model.clone(),
        requires_active_session: repl.requires_active_session,
        session_resolution: repl.session_resolution.clone(),
        default_input_behavior: repl.default_input_behavior.clone(),
        help_channel: repl.help_channel.clone(),
        slash_commands: repl.slash_commands.clone(),
        exit_triggers: repl.exit_triggers.clone(),
        note: repl.note.clone(),
    })
}

fn daemon_contract_help(path: &[String]) -> Option<DaemonContractHelp> {
    if path.first().is_none_or(|segment| segment != "daemon") {
        return None;
    }

    let daemon = &cli_plan().daemon_contract;
    Some(DaemonContractHelp {
        mode: daemon.mode.clone(),
        instance_model: daemon.instance_model.clone(),
        local_default_transport: daemon.transports.local_default.clone(),
        tcp_transport: daemon.transports.tcp.clone(),
        local_ipc_auth: daemon.auth.local_ipc.clone(),
        tcp_auth: daemon.auth.tcp.clone(),
        rpc_protocol: daemon.rpc.protocol.clone(),
        rpc_methods: daemon.rpc.methods.clone(),
        runtime_artifact_directory: daemon.runtime_artifacts.directory.clone(),
        runtime_artifact_files: daemon.runtime_artifacts.files.clone(),
    })
}

fn command_usage(path: &[String], command: &crate::plan::PlannedCommand) -> String {
    let mut parts = vec!["acp-agent-cli".to_string()];
    parts.extend(path.iter().cloned());
    if command.kind == "group" {
        parts.push("<COMMAND>".to_string());
        return parts.join(" ");
    }

    if !command.flags.is_empty() || !command.shared_flag_sets.is_empty() {
        parts.push("[OPTIONS]".to_string());
    }

    for positional in &command.positionals {
        let rendered = if positional.value_type == "string_list" {
            format!("<{}...>", positional.name.to_uppercase())
        } else {
            format!("<{}>", positional.name.to_uppercase())
        };
        if positional.required {
            parts.push(rendered);
        } else {
            parts.push(format!("[{}]", rendered));
        }
    }

    parts.join(" ")
}

fn generic_exit_behavior(kind: &str) -> Vec<ExitCodeSpec> {
    if kind == "group" {
        vec![
            ExitCodeSpec {
                code: 0,
                meaning: "Help was displayed or a subcommand succeeded".to_string(),
            },
            ExitCodeSpec {
                code: 2,
                meaning: "Structured usage error".to_string(),
            },
        ]
    } else {
        vec![
            ExitCodeSpec {
                code: 0,
                meaning: "The command completed successfully".to_string(),
            },
            ExitCodeSpec {
                code: 2,
                meaning: "Structured validation or runtime error".to_string(),
            },
        ]
    }
}

fn plan_description(path: &[String], command: &crate::plan::PlannedCommand) -> Vec<String> {
    let mut description = vec![command.summary.clone()];

    if path == ["daemon"] {
        let daemon = &cli_plan().daemon_contract;
        description.push(format!(
            "The daemon runs in {} {} mode.",
            daemon.instance_model.replace('_', "-"),
            daemon.mode.replace('_', "-")
        ));
        description.push(format!(
            "Transport defaults: local `{}` and TCP `{}`.",
            daemon.transports.local_default, daemon.transports.tcp
        ));
        description.push(format!(
            "Auth rules: local IPC `{}` and TCP `{}`.",
            daemon.auth.local_ipc, daemon.auth.tcp
        ));
        description.push(format!(
            "RPC protocol `{}` with methods: {}.",
            daemon.rpc.protocol,
            daemon.rpc.methods.join(", ")
        ));
        description.push(format!(
            "Runtime artifacts live beneath `{}`: {}.",
            daemon.runtime_artifacts.directory,
            daemon.runtime_artifacts.files.join(", ")
        ));
    } else if path.first().is_some_and(|segment| segment == "daemon") {
        description.push("Daemon runtime artifacts live beneath state/daemon.".to_string());
        description.push(
            "Client routing uses --via local|daemon and --ensure-daemon only when daemon execution is requested."
                .to_string(),
        );
    } else if path == ["repl"] {
        if let Some(repl) = &cli_plan().repl_contract {
            description.push(format!("REPL model: {}.", repl.model.replace('_', "-")));
            description.push(format!("Session resolution: {}.", repl.session_resolution));
            description.push(format!(
                "Default input behavior: {}.",
                repl.default_input_behavior
            ));
            description.push(format!("Plain-text help channel: {}.", repl.help_channel));
            description.push(format!(
                "Slash commands: {}.",
                repl.slash_commands.join(", ")
            ));
        }
    } else if command.kind == "leaf" {
        description.push(
            "This command path is part of the approved ACP CLI contract and uses structured stdout/stderr surfaces."
                .to_string(),
        );
    }

    description
}

fn plan_examples(path: &[String], command: &crate::plan::PlannedCommand) -> Vec<HelpExample> {
    let joined = format!("acp-agent-cli {}", path.join(" "));
    let mut examples = vec![HelpExample {
        command: joined.clone(),
        description: command.summary.clone(),
    }];

    if path == ["repl"] {
        examples.push(HelpExample {
            command: "acp-agent-cli repl --session local-1".to_string(),
            description: "Open the local session console for an explicit session".to_string(),
        });
    } else if path.first().is_some_and(|segment| segment == "daemon") {
        examples.push(HelpExample {
            command: "acp-agent-cli daemon status --format json".to_string(),
            description: "Inspect daemon health, endpoint, and recommended next action".to_string(),
        });
    } else if command.kind == "group" {
        examples.push(HelpExample {
            command: format!("{} --help", joined),
            description: "Render the human-readable help surface for this command group"
                .to_string(),
        });
    } else {
        examples.push(HelpExample {
            command: format!("acp-agent-cli help {} --format yaml", path.join(" ")),
            description: "Inspect the structured help contract for this command path".to_string(),
        });
    }

    examples
}

fn flag_value_name(flag: &crate::plan::PlannedFlag) -> String {
    match flag.value_type.as_str() {
        "bool" => "-".to_string(),
        "enum" if !flag.values.is_empty() => flag.values.join("|"),
        "path" => "PATH".to_string(),
        "int" => "INT".to_string(),
        "string" => "VALUE".to_string(),
        _ => "VALUE".to_string(),
    }
}

pub fn plain_text_help(path: &[String]) -> Option<String> {
    structured_help(path).map(|doc| render_plain_text_help(&doc))
}

pub fn render_plain_text_help(doc: &HelpDocument) -> String {
    let command_name = if doc.command_path.is_empty() {
        "acp-agent-cli".to_string()
    } else {
        format!("acp-agent-cli {}", doc.command_path.join(" "))
    };

    let mut out = String::new();
    // Human-readable help is intentionally man-like so top-level/non-leaf
    // auto-help and explicit `--help` share one stable contract.
    out.push_str("NAME\n");
    out.push_str(&format!("  {} - {}\n\n", command_name, doc.purpose));

    out.push_str("SYNOPSIS\n");
    out.push_str(&format!("  {}\n\n", doc.usage));

    out.push_str("DESCRIPTION\n");
    for paragraph in &doc.description {
        out.push_str(&format!("  {}\n", paragraph));
    }
    if !doc.subcommands.is_empty() {
        out.push_str("  Available subcommands:\n");
        for subcommand in &doc.subcommands {
            out.push_str(&format!(
                "    {:<12} {}\n",
                subcommand.name, subcommand.summary
            ));
        }
    }
    out.push('\n');

    out.push_str("OPTIONS\n");
    if !doc.arguments.is_empty() {
        for argument in &doc.arguments {
            out.push_str(&format!("  {}\n", argument));
        }
    }
    if doc.options.is_empty() && doc.arguments.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for option in &doc.options {
            out.push_str(&format!(
                "  {:<18} {:<16} default: {:<18} {}\n",
                option.name, option.value_name, option.default_value, option.description
            ));
        }
    }
    out.push('\n');

    out.push_str("FORMATS\n");
    out.push_str(&format!(
        "  Structured formats: {}\n",
        doc.output_formats.join(", ")
    ));
    out.push_str(&format!(
        "  Streaming: {}\n",
        doc.feature_availability.streaming
    ));
    out.push_str(&format!("  REPL: {}\n", doc.feature_availability.repl));
    out.push_str(&format!(
        "  Daemon: {}\n\n",
        doc.feature_availability.daemon
    ));

    out.push_str("EXAMPLES\n");
    for example in &doc.examples {
        out.push_str(&format!("  {}\n", example.command));
        out.push_str(&format!("    {}\n", example.description));
    }
    out.push('\n');

    out.push_str("EXIT CODES\n");
    for exit_code in &doc.exit_behavior {
        out.push_str(&format!("  {}  {}\n", exit_code.code, exit_code.meaning));
    }

    out
}
