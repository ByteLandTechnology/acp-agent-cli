---
name: 'acp-agent-cli'
description: 'Official full-function ACP CLI for developers and automation, exposing the complete ACP protocol surface and an integrated agent registry for discovering, managing, and installing ACP agents through one unified command-line workflow.'
---

# acp-agent-cli

## Description

Official full-function ACP CLI for developers and automation, exposing the complete ACP protocol surface and an integrated agent registry for discovering, managing, and installing ACP agents through one unified command-line workflow.

This generated Skill reuses the approved description contract across Cargo
metadata, `SKILL.md`, README, and help text.

## Prerequisites

- A working Rust toolchain (`rustup`, `cargo`) to compile and test the binary.
- No additional system dependencies are required for the default scaffold.
- The managed daemon uses std-only local IPC and writes runtime artifacts under
  `state/daemon`.

## Invocation

```text
acp-agent-cli [OPTIONS] <COMMAND>
acp-agent-cli help [COMMAND_PATH ...] [--format yaml|json|toml]
acp-agent-cli run [OPTIONS] <INPUT>
acp-agent-cli paths [OPTIONS]
acp-agent-cli context <show|use> [OPTIONS]
acp-agent-cli agent <status|candidates|select|set-executable|set-args> [OPTIONS]
acp-agent-cli workspace <browse|mkdir> [OPTIONS]
acp-agent-cli session <create|list|use|show|prompt|cancel-turn|stop|mode|config-option|model|authenticate|remote-sessions> [OPTIONS]
acp-agent-cli command list [OPTIONS]
acp-agent-cli skill list [OPTIONS]
acp-agent-cli proposal <list|approve|reject|edit> [OPTIONS]
acp-agent-cli file <upload|download> [OPTIONS]
acp-agent-cli code <read|list|search> [OPTIONS]
acp-agent-cli terminal <create|list|input|resize|output|close> [OPTIONS]
acp-agent-cli port-forward <create|list|delete> [OPTIONS]
acp-agent-cli events follow [OPTIONS]
acp-agent-cli state show [OPTIONS]
acp-agent-cli passthrough <command|skill> [OPTIONS]
acp-agent-cli repl [OPTIONS]
acp-agent-cli daemon <run|start|stop|restart|status> [OPTIONS]
```

The canonical agent-facing contract uses the bare command name shown above.
`cargo run -- ...` and `./target/release/acp-agent-cli ...` are local
developer execution forms and should be documented in `README.md`, not treated
as the final installed skill interface.

### Global Options

| Flag              | Type                       | Default                   | Description                                                        |
| ----------------- | -------------------------- | ------------------------- | ------------------------------------------------------------------ |
| `--format`, `-f`  | `yaml` \| `json` \| `toml` | `yaml`                    | Structured output format for one-shot commands and structured help |
| `--help`, `-h`    | —                          | —                         | Man-like human-readable help only; never emits YAML/JSON/TOML      |
| `--config-dir`    | `PATH`                     | platform default          | Override the configuration directory                               |
| `--data-dir`      | `PATH`                     | platform default          | Override the durable data directory                                |
| `--state-dir`     | `PATH`                     | derived from data         | Override the runtime state directory                               |
| `--cache-dir`     | `PATH`                     | platform default          | Override the cache directory                                       |
| `--log-dir`       | `PATH`                     | `state/logs` when enabled | Override the optional log directory                                |
| `--agent`         | `STRING`                   | persisted/default         | Override the ACP executable or configured agent profile            |
| `--workspace`     | `PATH`                     | persisted/default         | Override the workspace path                                        |
| `--session`       | `STRING`                   | persisted/default         | Override the active session                                        |
| `--selector`      | `KEY=VALUE`                | none                      | Add or override one Active Context selector                        |
| `--cwd`           | `PATH`                     | none                      | Override the working-directory cue                                 |
| `--via`           | `local` \| `daemon`       | `local`                   | Route the `run` leaf command through the managed daemon            |
| `--ensure-daemon` | —                          | `false`                   | Start or reuse the daemon before `run --via daemon`                |
| `--version`, `-V` | —                          | —                         | Print version and exit                                             |

### Commands

| Command | Kind | Purpose |
| ------- | ---- | ------- |
| `help` | leaf | Return structured help for the requested command path |
| `run` | leaf | Execute the scaffold diagnostic leaf command |
| `paths` | leaf | Inspect config/data/state/cache and optional log directories |
| `context show` / `context use` | leaf | Inspect and persist Active Context defaults |
| `agent ...` | group | Manage ACP executable selection and launch configuration |
| `workspace ...` | group | Browse and prepare local workspaces |
| `session ...` | group | Create, inspect, and control ACP sessions |
| `command list` / `skill list` | leaf | Discover ACP command and skill surfaces |
| `proposal ...` | group | Inspect and resolve approval proposals |
| `file ...` / `code ...` | group | Upload, download, read, list, and search workspace files |
| `terminal ...` / `port-forward ...` | group | Control ACP-backed terminals and forwarded ports |
| `events follow` / `state show` | leaf | Stream events and fetch one-shot state snapshots |
| `passthrough command` / `passthrough skill` | leaf | Forward raw ACP command and skill invocations |
| `repl` | leaf | Open the local ACP session console |
| `daemon ...` | group | Control the managed daemon lifecycle |

## Input

- The scaffolded CLI does not read default-mode input from `stdin`.
- `run` requires one positional `<INPUT>` argument.
- `context use` accepts one or more of `--agent`, `--workspace`, `--session`,
  `--selector KEY=VALUE`, and `--cwd PATH`.
- `help` accepts an optional command path such as `run`, `paths`, `context use`,
  `session prompt`, or `daemon status`.

## Output

Standard command results are written to `stdout`. Errors and diagnostics are
written to `stderr`.

### Help Channels

- Leaf commands never auto-display help. Missing required input stays a
  structured validation failure in the selected output format.
- Top-level invocation and non-leaf invocation (for example `context` or
  `daemon`) display man-like human-readable help automatically and exit `0`.
- `--help` is the man-like human-readable help channel. It always prints text
  and exits `0`, regardless of `--format`.
- `help` is the structured help channel. It supports `yaml`, `json`, and
  `toml`, with YAML as the default.
- Human-readable help is a required man-like surface with these sections in
  order: `NAME`, `SYNOPSIS`, `DESCRIPTION`, `OPTIONS`, `FORMATS`, `EXAMPLES`,
  `EXIT CODES`.

### Structured Results

The default one-shot result format is YAML.

Example `run` result:

```yaml
status: ok
message: Hello from acp-agent-cli
input: demo-input
effective_context:
  workspace: demo
```

Example `paths` result:

```yaml
config_dir: /home/user/.config/acp-agent-cli
data_dir: /home/user/.local/share/acp-agent-cli
state_dir: /home/user/.local/share/acp-agent-cli/state
cache_dir: /home/user/.cache/acp-agent-cli
scope: user_scoped_default
override_mechanisms:
  - --config-dir
  - --data-dir
  - --state-dir
  - --cache-dir
  - --log-dir
```

### Runtime Directories and Active Context

- `paths` exposes the runtime directory family: `config`, `data`, `state`,
  `cache`, and optional `logs`.
- Defaults are user-scoped unless explicitly overridden.
- `context show` exposes the persisted and effective Active Context.
- Explicit per-invocation selectors on `run` override the persisted Active
  Context for that invocation only.
- `repl` is enabled and keeps plain-text help, persisted history, and slash
  command completion.
- When daemon is enabled, runtime artifacts live beneath `state/daemon`.

### Streaming Mode

- Activate streaming with `--stream`.
- `--stream --format yaml` writes YAML multi-document output using `---` before each record and `...` at the end.
- `--stream --format json` writes NDJSON, one compact JSON object per line.
- `--stream --format toml` is unsupported and must fail with a non-zero exit code and an explanation on stderr.

### Managed Daemon Contract

- Public daemon control uses `daemon start`, `daemon stop`, `daemon restart`,
  and `daemon status`.
- `daemon run` is the foreground app-server entrypoint used for supervision and
  for `daemon start` to spawn the background worker.
- `run` is daemonizable and accepts `--via local|daemon` plus `--ensure-daemon`.
- Local-only surfaces such as `help`, `paths`, `context`, and `daemon` reject
  daemon routing with structured errors.
- Local IPC is the default transport. Loopback TCP is opt-in only. OS
  permissions protect local IPC, and TCP requires explicit authentication when
  enabled.
- Runtime artifact inspection includes `daemon.pid`,
  `daemon.sock_or_pipe_metadata`, `daemon-state.json`, `daemon.log`,
  `daemon.lock`, and `auth.token_when_tcp_enabled`.

### Optional Features

- Streaming is enabled for `run` output via `--stream`, with YAML multi-doc and NDJSON framing.
- REPL is enabled as a local session console. REPL help remains plain text only
  and the default REPL presentation is human-oriented.
- Package-local packaging-ready metadata or support fixtures may be added by a
  supported capability later, but repository-owned CI workflows and release
  automation are not copied into generated skill packages by default.

## Errors

| Exit Code | Meaning                              |
| --------- | ------------------------------------ |
| `0`       | Success or human-readable help       |
| `1`       | Unexpected runtime failure           |
| `2`       | Structured usage or validation error |

Structured errors preserve the selected output format and include at least
stable `code` and `message` fields.

Example structured error (`--format json`):

```json
{
  "code": "run.missing_input",
  "message": "the run command requires <INPUT>; use --help for man-like human-readable help",
  "source": "leaf_validation",
  "format": "json"
}
```

## Examples

Human-readable discovery:

```text
$ acp-agent-cli
NAME
  acp-agent-cli - Official full-function ACP CLI for developers and automation, exposing the complete ACP protocol surface and an integrated agent registry for discovering, managing, and installing ACP agents through one unified command-line workflow.
```

`--help` discovery:

```text
$ acp-agent-cli run --help
NAME
  acp-agent-cli run - Execute the generated leaf command
```

Structured help:

```text
$ acp-agent-cli help session prompt --format yaml
```

Persist Active Context:

```text
$ acp-agent-cli context use --workspace /tmp/demo --session local-1 --selector provider=staging
```

Run with daemon routing:

```text
$ acp-agent-cli run demo-input --via daemon --ensure-daemon
```

Open the REPL:

```text
$ acp-agent-cli repl --session local-1
```

---

_Created: 2026-04-16_
