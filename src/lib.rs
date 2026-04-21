//! Baseline shared library for the generated package layout.
//! Optional feature overlays may add package-local support modules on top of
//! this baseline, but repository-owned CI and release automation remain
//! external to generated skill packages.

pub mod acp;
pub mod context;
pub mod daemon;
pub mod help;
pub mod plan;
pub mod registry;
pub mod repl;
pub mod stream;
pub mod transport;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// Output format for serialization.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Yaml,
    Json,
    Toml,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yaml => "yaml",
            Self::Json => "json",
            Self::Toml => "toml",
        }
    }
}

/// Minimal stable structured error contract.
#[derive(Debug, Clone, Serialize)]
pub struct StructuredError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<BTreeMap<String, String>>,
    pub source: String,
    pub format: String,
}

impl StructuredError {
    pub fn new(code: &str, message: impl Into<String>, source: &str, format: Format) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            details: None,
            source: source.to_string(),
            format: format.as_str().to_string(),
        }
    }

    pub fn with_detail(mut self, key: &str, value: impl Into<String>) -> Self {
        self.details
            .get_or_insert_with(BTreeMap::new)
            .insert(key.to_string(), value.into());
        self
    }
}

/// Primary output structure for acp_agent_cli.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpAgentCliOutput {
    pub status: String,
    pub message: String,
    pub input: String,
    pub effective_context: BTreeMap<String, String>,
}

/// Run a prompt against an ACP agent and return the output.
pub fn run(
    input: &str,
    agent: &str,
    data_dir: &Path,
    effective_context: BTreeMap<String, String>,
) -> Result<AcpAgentCliOutput> {
    let (stop_reason, text) = acp::run_agent_prompt(agent, input, data_dir, &effective_context)?;
    Ok(AcpAgentCliOutput {
        status: if stop_reason == "end_turn" {
            "ok"
        } else {
            "partial"
        }
        .to_string(),
        message: text,
        input: input.to_string(),
        effective_context,
    })
}

pub use stream::stream_value;

/// Serialize a value to the given format and write to the writer.
pub fn serialize_value<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
    format: Format,
) -> Result<()> {
    match format {
        Format::Yaml => {
            let serialized = serde_yaml::to_string(value).context("failed to serialize as YAML")?;
            writer.write_all(serialized.as_bytes())?;
        }
        Format::Json => {
            serde_json::to_writer_pretty(&mut *writer, value)
                .context("failed to serialize as JSON")?;
            writeln!(writer)?;
        }
        Format::Toml => {
            let serialized =
                toml::to_string_pretty(value).context("failed to serialize as TOML")?;
            writer.write_all(serialized.as_bytes())?;
            writeln!(writer)?;
        }
    }

    Ok(())
}

/// Serialize a structured error in the selected output format.
pub fn write_structured_error<W: Write>(
    writer: &mut W,
    error: &StructuredError,
    format: Format,
) -> Result<()> {
    serialize_value(writer, error, format)
}
