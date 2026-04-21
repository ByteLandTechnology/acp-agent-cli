use serde::Deserialize;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
pub struct CliPlan {
    pub design_inheritance: DesignInheritance,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub shared_flag_sets: BTreeMap<String, SharedFlagSet>,
    pub daemon_contract: DaemonContract,
    pub commands: CommandsSection,
    #[serde(default)]
    pub repl_contract: Option<ReplContract>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DesignInheritance {
    pub purpose_summary: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub stream: CapabilityStatus,
    #[serde(default)]
    pub repl: CapabilityStatus,
    #[serde(default)]
    pub daemon: CapabilityStatus,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CapabilityStatus {
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SharedFlagSet {
    pub description: String,
    #[serde(default)]
    pub flags: Vec<PlannedFlag>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandsSection {
    #[serde(default)]
    pub top_level: Vec<PlannedCommand>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlannedCommand {
    pub name: String,
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub positionals: Vec<PlannedPositional>,
    #[serde(default)]
    pub flags: Vec<PlannedFlag>,
    #[serde(default)]
    pub shared_flag_sets: Vec<String>,
    #[serde(default)]
    pub subcommands: Vec<PlannedCommand>,
    #[serde(default)]
    pub supported_output_formats: Option<SupportedFormats>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlannedPositional {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlannedFlag {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repeatable: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SupportedFormats {
    List(Vec<String>),
    Text(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct DaemonContract {
    #[serde(default)]
    pub local_only_command_paths: Vec<String>,
    #[serde(default)]
    pub daemonizable_command_paths: Vec<String>,
    pub mode: String,
    pub instance_model: String,
    pub transports: DaemonTransports,
    pub auth: DaemonAuth,
    pub rpc: DaemonRpc,
    pub runtime_artifacts: DaemonRuntimeArtifactsPlan,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DaemonTransports {
    pub local_default: String,
    pub tcp: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DaemonAuth {
    pub local_ipc: String,
    pub tcp: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DaemonRpc {
    pub protocol: String,
    #[serde(default)]
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DaemonRuntimeArtifactsPlan {
    pub directory: String,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReplContract {
    pub model: String,
    pub requires_active_session: bool,
    pub session_resolution: String,
    pub default_input_behavior: String,
    #[serde(default)]
    pub slash_commands: Vec<String>,
    pub help_channel: String,
    #[serde(default)]
    pub exit_triggers: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedCommand {
    pub path: Vec<String>,
    pub command: PlannedCommand,
}

static PLAN: OnceLock<CliPlan> = OnceLock::new();

pub fn cli_plan() -> &'static CliPlan {
    PLAN.get_or_init(|| {
        serde_yaml::from_str(include_str!("plan_data.yml"))
            .expect("embedded cli-plan.yml must stay parseable")
    })
}

pub fn purpose_summary() -> &'static str {
    &cli_plan().design_inheritance.purpose_summary
}

pub fn capability_in_scope(name: &str) -> bool {
    let capabilities = &cli_plan().capabilities;
    match name {
        "stream" => capabilities.stream.status == "in_scope",
        "repl" => capabilities.repl.status == "in_scope",
        "daemon" => capabilities.daemon.status == "in_scope",
        _ => false,
    }
}

pub fn top_level_commands() -> &'static [PlannedCommand] {
    &cli_plan().commands.top_level
}

pub fn find_command(path: &[String]) -> Option<ResolvedCommand> {
    find_in_commands(&cli_plan().commands.top_level, path).map(|command| ResolvedCommand {
        path: path.to_vec(),
        command,
    })
}

fn find_in_commands(commands: &[PlannedCommand], path: &[String]) -> Option<PlannedCommand> {
    let head = path.first()?;
    let command = commands
        .iter()
        .find(|command| command.name == *head)?
        .clone();
    if path.len() == 1 {
        return Some(command);
    }
    find_in_commands(&command.subcommands, &path[1..])
}

pub fn match_command_tokens(tokens: &[String]) -> Option<(ResolvedCommand, Vec<String>)> {
    if tokens.is_empty() {
        return None;
    }

    if tokens.len() >= 2 {
        let two = vec![tokens[0].clone(), tokens[1].clone()];
        if let Some(command) = find_command(&two) {
            return Some((command, tokens[2..].to_vec()));
        }
    }

    let one = vec![tokens[0].clone()];
    find_command(&one).map(|command| (command, tokens[1..].to_vec()))
}

pub fn shared_flags(names: &[String]) -> Vec<PlannedFlag> {
    let plan = cli_plan();
    let mut flags = Vec::new();
    for name in names {
        if let Some(set) = plan.shared_flag_sets.get(name) {
            flags.extend(set.flags.clone());
        }
    }
    flags
}

pub fn is_local_only(path: &[String]) -> bool {
    cli_plan()
        .daemon_contract
        .local_only_command_paths
        .iter()
        .any(|entry| entry == &path.join(" "))
}

pub fn is_daemonizable(path: &[String]) -> bool {
    cli_plan()
        .daemon_contract
        .daemonizable_command_paths
        .iter()
        .any(|entry| entry == &path.join(" "))
}

pub fn planned_output_formats(command: &PlannedCommand) -> Vec<String> {
    match &command.supported_output_formats {
        Some(SupportedFormats::List(values)) => values.clone(),
        Some(SupportedFormats::Text(value)) if value == "inherited_global_format" => {
            vec!["yaml".to_string(), "json".to_string(), "toml".to_string()]
        }
        Some(SupportedFormats::Text(value)) => vec![value.clone()],
        None => vec!["yaml".to_string(), "json".to_string(), "toml".to_string()],
    }
}

pub fn value_to_string(value: &Option<Value>, fallback: &str) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Sequence(values)) => values
            .iter()
            .map(|value| match value {
                Value::String(value) => value.clone(),
                _ => serde_yaml::to_string(value)
                    .unwrap_or_else(|_| fallback.to_string())
                    .trim()
                    .to_string(),
            })
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => serde_yaml::to_string(other)
            .unwrap_or_else(|_| fallback.to_string())
            .trim()
            .to_string(),
        None => fallback.to_string(),
    }
}
