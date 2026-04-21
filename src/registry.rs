//! ACP agent registry: discover, install, and manage agents.
//!
//! Integrates with the official Agent Client Protocol registry hosted at
//! <https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json>.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types matching the real ACP registry JSON schema
// ---------------------------------------------------------------------------

/// Registry configuration loaded from config_dir/registry.yml.
#[derive(Debug, Deserialize)]
pub struct RegistryConfig {
    #[serde(default = "default_registry_url")]
    pub url: String,
}

fn default_registry_url() -> String {
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json".into()
}

/// Top-level registry index returned by the CDN.
#[derive(Debug, Deserialize)]
struct RegistryIndex {
    #[allow(dead_code)]
    version: String,
    agents: Vec<AgentEntry>,
    #[allow(dead_code)]
    extensions: Vec<serde_json::Value>,
}

/// An agent entry from the ACP registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub distribution: Distribution,
}

/// Agent distribution — either npx, binary, or both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    #[serde(default)]
    pub npx: Option<NpxDistribution>,
    #[serde(default)]
    pub binary: Option<BTreeMap<String, PlatformBinary>>,
}

/// NPX-based distribution: invokes `npx <package>` with optional args/env.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpxDistribution {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Platform-specific binary distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformBinary {
    pub archive: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Metadata for a locally installed agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledAgent {
    pub id: String,
    pub name: String,
    pub version: String,
    pub install_path: PathBuf,
    #[serde(default)]
    pub executable: Option<String>,
    /// Arguments to pass when spawning the agent (e.g., `["acp"]`).
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub distribution_type: Option<String>,
    #[serde(default)]
    pub installed_at: String,
    #[serde(default)]
    pub source_registry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy: Option<bool>,
}

// ---------------------------------------------------------------------------
// Config helpers
// ---------------------------------------------------------------------------

pub fn load_registry_config(
    config_dir: &Path,
    override_path: Option<&Path>,
) -> Result<RegistryConfig> {
    let path = override_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| config_dir.join("registry.yml"));

    if path.exists() {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("reading registry config from {}", path.display()))?;
        let config: RegistryConfig = serde_yaml::from_str(&content)
            .with_context(|| format!("parsing registry config from {}", path.display()))?;
        Ok(config)
    } else {
        Ok(RegistryConfig {
            url: default_registry_url(),
        })
    }
}

pub fn registry_url_with_override(config: &RegistryConfig, override_url: Option<&str>) -> String {
    override_url
        .map(|u| u.to_string())
        .unwrap_or_else(|| config.url.clone())
        .trim_end_matches('/')
        .to_string()
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

pub fn agents_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("agents")
}

pub fn registry_cache_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("registry")
}

pub fn installed_agent_dir(data_dir: &Path, agent_id: &str) -> PathBuf {
    agents_dir(data_dir).join(agent_id)
}

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

fn current_platform_key() -> String {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "unknown"
    };
    format!("{}-{}", os, arch)
}

// ---------------------------------------------------------------------------
// Remote registry fetch
// ---------------------------------------------------------------------------

fn fetch_registry_index(registry_url: &str) -> Result<RegistryIndex> {
    let body = ureq::get(registry_url)
        .header("Accept", "application/json")
        .call()
        .with_context(|| format!("fetching registry from {}", registry_url))?
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading registry response from {}", registry_url))?;

    serde_json::from_str::<RegistryIndex>(&body)
        .with_context(|| format!("parsing registry JSON from {}", registry_url))
}

// ---------------------------------------------------------------------------
// Local operations
// ---------------------------------------------------------------------------

pub fn scan_installed_agents(data_dir: &Path) -> Result<Vec<InstalledAgent>> {
    let dir = agents_dir(data_dir);
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut agents = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let meta_path = entry.path().join("agent.json");
        if meta_path.exists() {
            let content = fs::read_to_string(&meta_path)?;
            if let Ok(agent) = serde_json::from_str::<InstalledAgent>(&content) {
                agents.push(agent);
            }
        }
    }
    agents.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(agents)
}

fn persist_agent_meta(agent: &InstalledAgent) -> Result<()> {
    let meta_path = agent.install_path.join("agent.json");
    let content = serde_json::to_string_pretty(agent)?;
    fs::write(&meta_path, content)
        .with_context(|| format!("writing agent metadata to {}", meta_path.display()))
}

/// Download a tar.gz or zip archive and extract into target_dir.
fn download_and_extract(archive_url: &str, target_dir: &Path) -> Result<()> {
    let mut response = ureq::get(archive_url)
        .call()
        .with_context(|| format!("downloading archive from {}", archive_url))?;

    let bytes = response
        .body_mut()
        .with_config()
        .limit(200 * 1024 * 1024)
        .read_to_vec()?;

    let archive_path = target_dir.join("download.tmp");
    fs::write(&archive_path, &bytes).with_context(|| "writing archive to temp file")?;

    let url_lower = archive_url.to_lowercase();
    let extract_result: Result<()> = if url_lower.ends_with(".tar.gz")
        || url_lower.ends_with(".tgz")
    {
        let file = std::fs::File::open(&archive_path).with_context(|| "opening tar.gz archive")?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        archive
            .unpack(target_dir)
            .with_context(|| "extracting tar.gz archive")
    } else if url_lower.ends_with(".zip") {
        let file = std::fs::File::open(&archive_path).with_context(|| "opening zip archive")?;
        let mut archive = zip::ZipArchive::new(file).with_context(|| "reading zip archive")?;
        archive
            .extract(target_dir)
            .with_context(|| "extracting zip archive")
    } else {
        bail!("unsupported archive format: {}", archive_url);
    };

    let _ = fs::remove_file(&archive_path);

    extract_result
}

/// Create a shell wrapper script that invokes `npx` with the right package, args, and env.
fn create_npx_wrapper(target_dir: &Path, npx: &NpxDistribution) -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let wrapper_name = "run.bat";
    #[cfg(not(target_os = "windows"))]
    let wrapper_name = "run";

    let wrapper_path = target_dir.join(wrapper_name);

    let mut script = String::new();

    #[cfg(not(target_os = "windows"))]
    {
        script.push_str("#!/bin/sh\n");
        for (key, value) in &npx.env {
            script.push_str(&format!("export {}=\"{}\"\n", key, value));
        }
        script.push_str(&format!("exec npx {}", npx.package));
        for arg in &npx.args {
            script.push_str(&format!(" {}", arg));
        }
        script.push('\n');
    }

    #[cfg(target_os = "windows")]
    {
        for (key, value) in &npx.env {
            script.push_str(&format!("set {}={}\n", key, value));
        }
        script.push_str(&format!("npx {}", npx.package));
        for arg in &npx.args {
            script.push_str(&format!(" {}", arg));
        }
        script.push('\n');
    }

    fs::write(&wrapper_path, &script)
        .with_context(|| format!("writing wrapper script to {}", wrapper_path.display()))?;

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper_path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("setting permissions on {}", wrapper_path.display()))?;
    }

    Ok(wrapper_path)
}

/// Install an agent using either binary or npx distribution.
/// Prefers binary for the current platform; falls back to npx.
fn install_agent_files(
    agent: &AgentEntry,
    target_dir: &Path,
) -> Result<(String, Option<String>, Vec<String>)> {
    let platform = current_platform_key();

    // Try binary distribution for current platform first
    if let Some(ref binaries) = agent.distribution.binary
        && let Some(platform_binary) = binaries.get(&platform)
    {
        download_and_extract(&platform_binary.archive, target_dir)?;

        let cmd_name = platform_binary.cmd.trim_start_matches("./");
        let cmd_path = target_dir.join(cmd_name);
        if cmd_path.exists() {
            #[cfg(not(target_os = "windows"))]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&cmd_path, fs::Permissions::from_mode(0o755));
            }
        }

        return Ok((
            "binary".to_string(),
            Some(platform_binary.cmd.clone()),
            platform_binary.args.clone(),
        ));
    }

    // Fall back to npx
    if let Some(ref npx) = agent.distribution.npx {
        let wrapper = create_npx_wrapper(target_dir, npx)?;
        let exe_name = wrapper
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        return Ok(("npx".to_string(), Some(exe_name), npx.args.clone()));
    }

    if agent.distribution.binary.is_some() {
        bail!(
            "no binary distribution available for platform '{}' and no npx fallback",
            platform
        );
    }
    bail!("agent '{}' has no distribution information", agent.id);
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub query: String,
    pub matches: Vec<AgentEntry>,
    pub total: usize,
}

pub fn cmd_search(
    registry_url: &str,
    query: &str,
    tags: &[String],
    category: Option<&str>,
    verified_only: bool,
    page: u32,
    page_size: u32,
) -> Result<SearchResult> {
    let index = fetch_registry_index(registry_url)?;
    let mut agents = index.agents;

    let query_lower = query.to_lowercase();
    if !query_lower.is_empty() {
        agents.retain(|a| {
            let name_match = a.name.to_lowercase().contains(&query_lower);
            let desc_match = a.description.to_lowercase().contains(&query_lower);
            let id_match = a.id.to_lowercase().contains(&query_lower);
            name_match || desc_match || id_match
        });
    }

    // Tags, category, and verified_only are not part of the current registry
    // schema. Kept for forward compatibility.
    let _ = (tags, category, verified_only);

    let total = agents.len();
    let page = page.max(1) as usize;
    let page_size = page_size.max(1) as usize;
    let start = (page - 1) * page_size;
    let matches: Vec<_> = agents.into_iter().skip(start).take(page_size).collect();
    Ok(SearchResult {
        query: query.to_string(),
        matches,
        total,
    })
}

pub fn cmd_list(
    registry_url: &str,
    tags: &[String],
    category: Option<&str>,
    verified_only: bool,
) -> Result<Vec<AgentEntry>> {
    let index = fetch_registry_index(registry_url)?;
    let mut agents = index.agents;

    let _ = (tags, category, verified_only);

    agents.sort_by_key(|a| a.name.to_lowercase());
    Ok(agents)
}

pub fn cmd_show(registry_url: &str, agent_id: &str, version: Option<&str>) -> Result<AgentEntry> {
    let index = fetch_registry_index(registry_url)?;
    let agent = index
        .agents
        .into_iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| anyhow::anyhow!("agent '{}' not found in registry", agent_id))?;

    let _ = version; // registry always returns latest
    Ok(agent)
}

/// An agent detected on the system (not installed via the registry).
#[derive(Debug, Clone, Serialize)]
pub struct DetectedAgent {
    pub id: String,
    pub name: String,
    pub version: String,
    pub executable_path: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct InstalledResult {
    pub agents: Vec<InstalledAgent>,
    pub detected: Vec<DetectedAgent>,
    pub total: usize,
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

pub fn cmd_installed(
    data_dir: &Path,
    check_health: bool,
    verbose: bool,
) -> Result<InstalledResult> {
    let mut agents = scan_installed_agents(data_dir)?;

    if check_health {
        for agent in &mut agents {
            if let Some(exe) = &agent.executable {
                let exe_path = agent.install_path.join(exe);
                agent.healthy = Some(exe_path.exists());
            }
        }
    }

    let _ = verbose;

    let detected = Vec::new();

    let total = agents.len() + detected.len();
    Ok(InstalledResult {
        agents,
        detected,
        total,
    })
}

#[derive(Debug, Serialize)]
pub struct InstallResult {
    pub agent_id: String,
    pub version: String,
    pub install_path: String,
    pub distribution_type: String,
    pub status: String,
}

pub fn cmd_install(
    registry_url: &str,
    data_dir: &Path,
    agent_id: &str,
    version: Option<&str>,
    set_active: bool,
    _config_dir: &Path,
) -> Result<InstallResult> {
    let index = fetch_registry_index(registry_url)?;
    let agent = index
        .agents
        .into_iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| anyhow::anyhow!("agent '{}' not found in registry", agent_id))?;

    let target_dir = installed_agent_dir(data_dir, agent_id);
    if target_dir.exists() {
        bail!(
            "agent '{}' is already installed at {}",
            agent_id,
            target_dir.display()
        );
    }
    fs::create_dir_all(&target_dir)?;

    let ver = version.unwrap_or(&agent.version).to_string();
    let (dist_type, executable, agent_args) = install_agent_files(&agent, &target_dir)?;

    let now = chrono_now();
    let installed = InstalledAgent {
        id: agent_id.to_string(),
        name: agent.name.clone(),
        version: ver.clone(),
        install_path: target_dir.clone(),
        executable,
        args: agent_args,
        distribution_type: Some(dist_type.clone()),
        installed_at: now,
        source_registry: Some(registry_url.to_string()),
        healthy: None,
    };
    persist_agent_meta(&installed)?;

    let _ = set_active;

    Ok(InstallResult {
        agent_id: agent_id.to_string(),
        version: ver,
        install_path: target_dir.display().to_string(),
        distribution_type: dist_type,
        status: "installed".to_string(),
    })
}

#[derive(Debug, Serialize)]
pub struct UninstallResult {
    pub agent_id: String,
    pub status: String,
    pub removed_path: String,
}

pub fn cmd_uninstall(
    data_dir: &Path,
    agent_id: &str,
    purge_config: bool,
    _config_dir: &Path,
) -> Result<UninstallResult> {
    let target_dir = installed_agent_dir(data_dir, agent_id);
    if !target_dir.exists() {
        bail!("agent '{}' is not installed", agent_id);
    }

    fs::remove_dir_all(&target_dir)
        .with_context(|| format!("removing agent directory {}", target_dir.display()))?;

    if purge_config {
        let agent_config = _config_dir.join("agents").join(agent_id);
        if agent_config.exists() {
            let _ = fs::remove_dir_all(&agent_config);
        }
    }

    Ok(UninstallResult {
        agent_id: agent_id.to_string(),
        status: "removed".to_string(),
        removed_path: target_dir.display().to_string(),
    })
}

#[derive(Debug, Serialize)]
pub struct UpdateResult {
    pub agent_id: Option<String>,
    pub updates: Vec<UpdateEntry>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct UpdateEntry {
    pub agent_id: String,
    pub previous_version: String,
    pub new_version: String,
    pub status: String,
}

pub fn cmd_update(
    registry_url: &str,
    data_dir: &Path,
    agent_id: Option<&str>,
    check_only: bool,
    version: Option<&str>,
) -> Result<UpdateResult> {
    let installed = scan_installed_agents(data_dir)?;

    let targets: Vec<&InstalledAgent> = if let Some(id) = agent_id {
        installed.iter().filter(|a| a.id == id).collect()
    } else {
        installed.iter().collect()
    };

    if targets.is_empty() {
        return Ok(UpdateResult {
            agent_id: agent_id.map(|s| s.to_string()),
            updates: vec![],
            status: "no_updates_available".to_string(),
        });
    }

    let index = fetch_registry_index(registry_url)?;
    let mut updates = Vec::new();

    for installed_agent in &targets {
        let remote = match index.agents.iter().find(|a| a.id == installed_agent.id) {
            Some(a) => a,
            None => continue,
        };

        let target_version = version.unwrap_or(&remote.version);
        if target_version == installed_agent.version {
            continue;
        }

        if check_only {
            updates.push(UpdateEntry {
                agent_id: installed_agent.id.clone(),
                previous_version: installed_agent.version.clone(),
                new_version: target_version.to_string(),
                status: "update_available".to_string(),
            });
            continue;
        }

        // Remove old installation and reinstall
        let target_dir = &installed_agent.install_path;
        if target_dir.exists() {
            fs::remove_dir_all(target_dir)?;
        }
        fs::create_dir_all(target_dir)?;

        let (dist_type, executable, agent_args) = match install_agent_files(remote, target_dir) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let mut updated = (*installed_agent).clone();
        updated.version = target_version.to_string();
        updated.executable = executable;
        updated.args = agent_args;
        updated.distribution_type = Some(dist_type);
        updated.installed_at = chrono_now();
        updated.source_registry = Some(registry_url.to_string());
        persist_agent_meta(&updated)?;

        updates.push(UpdateEntry {
            agent_id: installed_agent.id.clone(),
            previous_version: installed_agent.version.clone(),
            new_version: target_version.to_string(),
            status: "updated".to_string(),
        });
    }

    let status = if updates.is_empty() {
        "no_updates_available".to_string()
    } else if check_only {
        "updates_available".to_string()
    } else {
        "updated".to_string()
    };

    Ok(UpdateResult {
        agent_id: agent_id.map(|s| s.to_string()),
        updates,
        status,
    })
}

/// Resolve the args needed to spawn an agent in ACP mode.
/// Checks installed metadata first, then falls back to the registry index.
pub fn resolve_agent_args(agent: &str, data_dir: &Path, registry_url: Option<&str>) -> Vec<String> {
    // Check installed agent metadata
    let installed = scan_installed_agents(data_dir).unwrap_or_default();
    if let Some(a) = installed.iter().find(|a| a.id == agent)
        && !a.args.is_empty()
    {
        return a.args.clone();
    }

    // Fall back to registry index
    let url = registry_url
        .unwrap_or("https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json");
    if let Ok(index) = fetch_registry_index(url) {
        let platform = current_platform_key();
        if let Some(entry) = index.agents.iter().find(|a| a.id == agent) {
            // Check binary distribution args first
            if let Some(ref binaries) = entry.distribution.binary
                && let Some(pb) = binaries.get(&platform)
                && !pb.args.is_empty()
            {
                return pb.args.clone();
            }
            // Fall back to npx distribution args
            if let Some(ref npx) = entry.distribution.npx
                && !npx.args.is_empty()
            {
                return npx.args.clone();
            }
        }
    }

    vec![]
}

fn chrono_now() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%z")
        .to_string()
}
