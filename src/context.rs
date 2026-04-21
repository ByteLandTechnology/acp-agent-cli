//! Runtime-directory and Active Context support for the generated package
//! baseline. Package-local packaging-ready fixtures may reference these
//! locations when a supported capability requires them.

use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct RuntimeOverrides {
    pub config_dir: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
}

impl RuntimeOverrides {
    pub fn has_overrides(&self) -> bool {
        self.config_dir.is_some()
            || self.data_dir.is_some()
            || self.state_dir.is_some()
            || self.cache_dir.is_some()
            || self.log_dir.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeLocations {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: Option<PathBuf>,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDirectorySummary {
    pub config_dir: String,
    pub data_dir: String,
    pub state_dir: String,
    pub cache_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_dir: Option<String>,
    pub scope: String,
    pub override_mechanisms: Vec<String>,
}

impl RuntimeLocations {
    pub fn summary(&self) -> RuntimeDirectorySummary {
        RuntimeDirectorySummary {
            config_dir: self.config_dir.display().to_string(),
            data_dir: self.data_dir.display().to_string(),
            state_dir: self.state_dir.display().to_string(),
            cache_dir: self.cache_dir.display().to_string(),
            log_dir: self.log_dir.as_ref().map(|path| path.display().to_string()),
            scope: self.scope.clone(),
            override_mechanisms: vec![
                "--config-dir".to_string(),
                "--data-dir".to_string(),
                "--state-dir".to_string(),
                "--cache-dir".to_string(),
                "--log-dir".to_string(),
            ],
        }
    }

    pub fn context_file(&self) -> PathBuf {
        self.state_dir.join("active-context.toml")
    }

    pub fn history_file(&self) -> PathBuf {
        self.state_dir.join("repl-history.txt")
    }

    pub fn ensure_exists(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("failed to create {}", self.config_dir.display()))?;
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("failed to create {}", self.data_dir.display()))?;
        fs::create_dir_all(&self.state_dir)
            .with_context(|| format!("failed to create {}", self.state_dir.display()))?;
        fs::create_dir_all(&self.cache_dir)
            .with_context(|| format!("failed to create {}", self.cache_dir.display()))?;
        if let Some(log_dir) = &self.log_dir {
            fs::create_dir_all(log_dir)
                .with_context(|| format!("failed to create {}", log_dir.display()))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveContextState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub selectors: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ambient_cues: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct InvocationContextOverrides {
    pub agent: Option<String>,
    pub workspace: Option<PathBuf>,
    pub session_id: Option<String>,
    pub selectors: BTreeMap<String, String>,
    pub current_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveContextView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub effective_values: BTreeMap<String, String>,
    pub precedence_rule: String,
    pub persisted_context_present: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextInspection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persisted_context: Option<ActiveContextState>,
    pub effective_context: EffectiveContextView,
    pub context_file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPersistenceResult {
    pub status: String,
    pub message: String,
    pub active_context: ActiveContextState,
    pub context_file: String,
}

pub fn resolve_runtime_locations(
    overrides: &RuntimeOverrides,
    log_enabled: bool,
) -> Result<RuntimeLocations> {
    let project_dirs = ProjectDirs::from("com", "cli-forge", "AcpAgentCli")
        .ok_or_else(|| anyhow!("failed to resolve platform project directories"))?;

    let data_dir = overrides
        .data_dir
        .clone()
        .unwrap_or_else(|| project_dirs.data_dir().to_path_buf());
    let state_dir = overrides
        .state_dir
        .clone()
        .unwrap_or_else(|| data_dir.join("state"));

    let log_dir = if overrides.log_dir.is_some() || log_enabled {
        Some(
            overrides
                .log_dir
                .clone()
                .unwrap_or_else(|| state_dir.join("logs")),
        )
    } else {
        None
    };

    Ok(RuntimeLocations {
        config_dir: overrides
            .config_dir
            .clone()
            .unwrap_or_else(|| project_dirs.config_dir().to_path_buf()),
        data_dir,
        state_dir,
        cache_dir: overrides
            .cache_dir
            .clone()
            .unwrap_or_else(|| project_dirs.cache_dir().to_path_buf()),
        log_dir,
        scope: if overrides.has_overrides() {
            "explicit_override".to_string()
        } else {
            "user_scoped_default".to_string()
        },
    })
}

pub fn parse_selector(raw: &str) -> Result<(String, String)> {
    let (key, value) = raw
        .split_once('=')
        .ok_or_else(|| anyhow!("selector '{raw}' must use KEY=VALUE"))?;
    if key.trim().is_empty() || value.trim().is_empty() {
        return Err(anyhow!(
            "selector '{raw}' must include a non-empty key and value"
        ));
    }
    Ok((key.trim().to_string(), value.trim().to_string()))
}

pub fn parse_selectors(values: &[String]) -> Result<BTreeMap<String, String>> {
    let mut selectors = BTreeMap::new();
    for value in values {
        let (key, parsed_value) = parse_selector(value)?;
        selectors.insert(key, parsed_value);
    }
    Ok(selectors)
}

pub fn build_context_state(
    name: Option<String>,
    agent: Option<String>,
    workspace: Option<PathBuf>,
    session_id: Option<String>,
    selectors: BTreeMap<String, String>,
    current_directory: Option<PathBuf>,
) -> ActiveContextState {
    update_active_context_state(
        None,
        true,
        name,
        agent,
        workspace,
        session_id,
        selectors,
        current_directory,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn update_active_context_state(
    existing: Option<&ActiveContextState>,
    clear: bool,
    name: Option<String>,
    agent: Option<String>,
    workspace: Option<PathBuf>,
    session_id: Option<String>,
    selectors: BTreeMap<String, String>,
    current_directory: Option<PathBuf>,
) -> ActiveContextState {
    let mut state = if clear {
        ActiveContextState::default()
    } else {
        existing.cloned().unwrap_or_default()
    };

    if let Some(name) = name {
        state.name = Some(name);
    }
    if let Some(agent) = agent {
        state.agent = Some(agent);
    }
    if let Some(workspace) = workspace {
        state.workspace = Some(workspace.display().to_string());
    }
    if let Some(session_id) = session_id {
        state.session_id = Some(session_id);
    }
    if !selectors.is_empty() {
        state.selectors.extend(selectors);
    }
    if let Some(current_directory) = current_directory {
        state.ambient_cues.insert(
            "current_directory".to_string(),
            current_directory.display().to_string(),
        );
    }

    state
}

pub fn load_active_context(runtime: &RuntimeLocations) -> Result<Option<ActiveContextState>> {
    let context_file = runtime.context_file();
    if !context_file.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&context_file)
        .with_context(|| format!("failed to read {}", context_file.display()))?;
    let state = toml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", context_file.display()))?;
    Ok(Some(state))
}

pub fn persist_active_context(
    runtime: &RuntimeLocations,
    state: &ActiveContextState,
) -> Result<ContextPersistenceResult> {
    runtime.ensure_exists()?;
    let serialized = toml::to_string_pretty(state).context("failed to serialize Active Context")?;
    let context_file = runtime.context_file();
    fs::write(&context_file, serialized)
        .with_context(|| format!("failed to write {}", context_file.display()))?;

    Ok(ContextPersistenceResult {
        status: "ok".to_string(),
        message: "Active Context updated".to_string(),
        active_context: state.clone(),
        context_file: context_file.display().to_string(),
    })
}

pub fn resolve_effective_context(
    persisted: Option<&ActiveContextState>,
    overrides: &InvocationContextOverrides,
) -> EffectiveContextView {
    let mut effective_values = BTreeMap::new();

    if let Some(persisted) = persisted {
        if let Some(agent) = &persisted.agent {
            effective_values.insert("agent".to_string(), agent.clone());
        }
        if let Some(workspace) = &persisted.workspace {
            effective_values.insert("workspace".to_string(), workspace.clone());
        }
        if let Some(session_id) = &persisted.session_id {
            effective_values.insert("session_id".to_string(), session_id.clone());
        }
        effective_values.extend(persisted.selectors.clone());
        effective_values.extend(persisted.ambient_cues.clone());
    }

    if let Some(agent) = &overrides.agent {
        effective_values.insert("agent".to_string(), agent.clone());
    }
    if let Some(workspace) = &overrides.workspace {
        effective_values.insert("workspace".to_string(), workspace.display().to_string());
    }
    if let Some(session_id) = &overrides.session_id {
        effective_values.insert("session_id".to_string(), session_id.clone());
    }

    effective_values.extend(overrides.selectors.clone());

    if let Some(current_directory) = &overrides.current_directory {
        effective_values.insert(
            "current_directory".to_string(),
            current_directory.display().to_string(),
        );
    }

    EffectiveContextView {
        name: persisted.and_then(|state| state.name.clone()),
        effective_values,
        precedence_rule:
            "explicit invocation values override the persisted Active Context for one invocation only"
                .to_string(),
        persisted_context_present: persisted.is_some(),
    }
}

pub fn inspect_context(
    runtime: &RuntimeLocations,
    overrides: &InvocationContextOverrides,
) -> Result<ContextInspection> {
    let persisted_context = load_active_context(runtime)?;
    let effective_context = resolve_effective_context(persisted_context.as_ref(), overrides);

    Ok(ContextInspection {
        persisted_context,
        effective_context,
        context_file: runtime.context_file().display().to_string(),
    })
}

pub fn current_directory_or(path: Option<PathBuf>) -> Result<Option<PathBuf>> {
    match path {
        Some(path) => Ok(Some(path)),
        None => {
            let current_dir =
                std::env::current_dir().context("failed to resolve current directory")?;
            Ok(Some(current_dir))
        }
    }
}

pub fn path_to_string(path: &Path) -> String {
    path.display().to_string()
}
