use crate::context::{
    InvocationContextOverrides, RuntimeOverrides, load_active_context, resolve_effective_context,
    resolve_runtime_locations,
};
use anyhow::{Result, anyhow};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::FileHistory;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};
use std::path::PathBuf;

const SLASH_COMMANDS: &[&str] = &[
    "/prompt", "/approve", "/reject", "/edit", "/events", "/files", "/stop", "/help", "/exit",
];

#[derive(Debug, Clone)]
pub struct ReplConfig {
    pub session: Option<String>,
    pub workspace: Option<PathBuf>,
    pub agent: Option<String>,
    pub selectors: std::collections::BTreeMap<String, String>,
    pub current_directory: Option<PathBuf>,
}

struct ReplHelper;

impl Helper for ReplHelper {}
impl Highlighter for ReplHelper {}
impl Validator for ReplHelper {}
impl Hinter for ReplHelper {
    type Hint = String;
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];
        if !prefix.starts_with('/') {
            return Ok((0, Vec::new()));
        }

        let matches = SLASH_COMMANDS
            .iter()
            .filter(|command| command.starts_with(prefix))
            .map(|command| Pair {
                display: (*command).to_string(),
                replacement: (*command).to_string(),
            })
            .collect::<Vec<_>>();
        Ok((0, matches))
    }
}

fn send_prompt(client: &mut crate::acp::AcpClient, text: &str) {
    match client.send_prompt_streaming(text, |notification| {
        if let Some(chunk) = crate::acp::extract_chunk_text(notification) {
            eprint!("{}", chunk);
        }
    }) {
        Ok(result) => {
            if !result.notifications.is_empty() {
                eprintln!();
            }
            if result.notifications.is_empty() {
                let text = crate::acp::extract_text(&result);
                if !text.is_empty() {
                    println!("{}", text);
                }
            }
            if result.stop_reason != "end_turn" {
                println!("[stopped: {}]", result.stop_reason);
            }
        }
        Err(e) => eprintln!("error: {e}"),
    }
}

pub fn run_repl(runtime_overrides: &RuntimeOverrides, config: ReplConfig) -> Result<()> {
    let runtime = resolve_runtime_locations(runtime_overrides, true)?;
    runtime.ensure_exists()?;
    let persisted = load_active_context(&runtime)?;
    let _effective = resolve_effective_context(
        persisted.as_ref(),
        &InvocationContextOverrides {
            agent: config.agent.clone(),
            workspace: config.workspace.clone(),
            session_id: config.session.clone(),
            selectors: config.selectors,
            current_directory: config.current_directory,
        },
    );

    // Resolve agent
    let agent = config
        .agent
        .as_deref()
        .or(persisted.as_ref().and_then(|c| c.agent.as_deref()))
        .ok_or_else(|| {
            anyhow!(
                "repl requires an agent; pass --agent <name> or set one with `context use --agent`"
            )
        })?;

    let executable = crate::acp::resolve_agent_executable(agent, &runtime.data_dir)?;
    let args = crate::acp::resolve_agent_args(agent, &runtime.data_dir);

    // Connect to agent and create session
    let mut client = crate::acp::AcpClient::connect(&executable, &args)?;
    let session_id = client.create_session()?;

    let mut editor = Editor::<ReplHelper, FileHistory>::new()?;
    editor.set_helper(Some(ReplHelper));
    let history_path = runtime.history_file();
    if history_path.exists() {
        let _ = editor.load_history(&history_path);
    }

    println!("Connected to agent `{agent}` — session `{session_id}`");
    println!("Type text to send a prompt, `/help` for commands, or `/exit` to leave.");

    loop {
        let line = match editor.readline("acp> ") {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("Interrupted. Use `/exit` to leave.");
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(error) => return Err(error.into()),
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        editor.add_history_entry(trimmed)?;
        let _ = editor.save_history(&history_path);

        match trimmed {
            "/exit" | "quit" => break,
            "/help" => print_repl_help(),
            "/stop" => {
                let _ = client.cancel();
                println!("[cancelled]");
            }
            _ if trimmed.starts_with("/approve ") || trimmed == "/approve" => {
                let args = trimmed.strip_prefix("/approve").unwrap().trim();
                match client
                    .raw_request("proposal/approve", serde_json::json!({"proposalId": args}))
                {
                    Ok((result, _)) => println!(
                        "{}",
                        serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| format!("{result:?}"))
                    ),
                    Err(e) => eprintln!("error: {e}"),
                }
            }
            _ if trimmed.starts_with("/reject ") || trimmed == "/reject" => {
                let args = trimmed.strip_prefix("/reject").unwrap().trim();
                match client.raw_request("proposal/reject", serde_json::json!({"proposalId": args}))
                {
                    Ok((result, _)) => println!(
                        "{}",
                        serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| format!("{result:?}"))
                    ),
                    Err(e) => eprintln!("error: {e}"),
                }
            }
            _ if trimmed.starts_with("/edit ") => {
                let args = trimmed.strip_prefix("/edit").unwrap().trim();
                let parts: Vec<&str> = args.splitn(2, ' ').collect();
                let (proposal_id, replacement) = if parts.len() >= 2 {
                    (parts[0], parts[1])
                } else {
                    (parts[0].trim(), "")
                };
                match client.raw_request(
                    "proposal/edit",
                    serde_json::json!({"proposalId": proposal_id, "replacement": replacement}),
                ) {
                    Ok((result, _)) => println!(
                        "{}",
                        serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| format!("{result:?}"))
                    ),
                    Err(e) => eprintln!("error: {e}"),
                }
            }
            "/events" => match client.raw_request("events/follow", serde_json::json!({})) {
                Ok((result, _)) => println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| format!("{result:?}"))
                ),
                Err(e) => eprintln!("error: {e}"),
            },
            "/files" => match client.raw_request("file/list", serde_json::json!({})) {
                Ok((result, _)) => println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| format!("{result:?}"))
                ),
                Err(e) => eprintln!("error: {e}"),
            },
            _ if trimmed.starts_with("/prompt ") => {
                let text = trimmed.strip_prefix("/prompt").unwrap().trim();
                if text.is_empty() {
                    println!("usage: /prompt <text>");
                } else {
                    send_prompt(&mut client, text);
                }
            }
            _ if trimmed.starts_with('/') => {
                println!("unknown command `{trimmed}`; type `/help` for available commands");
            }
            _ => send_prompt(&mut client, trimmed),
        }
    }

    client.shutdown()?;
    Ok(())
}

fn print_repl_help() {
    println!("REPL commands");
    println!("  <text>            Send a prompt to the agent");
    println!("  /prompt <text>    Send a prompt (explicit slash form)");
    println!("  /approve <id>     Approve a pending proposal");
    println!("  /reject <id>      Reject a pending proposal");
    println!("  /edit <id> <text> Edit and approve a proposal");
    println!("  /events           Stream session events");
    println!("  /files            List session files");
    println!("  /stop             Cancel the current prompt");
    println!("  /help             Show this help");
    println!("  /exit             Exit the REPL");
}
