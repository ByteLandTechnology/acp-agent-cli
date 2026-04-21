# acp-agent-cli

Official full-function ACP CLI for developers and automation, exposing the complete ACP protocol surface and an integrated agent registry for discovering, managing, and installing ACP agents through one unified command-line workflow.

This CLI reuses the approved description contract across Cargo metadata,
`SKILL.md`, README, and help text, and now exposes the planned ACP command
families through one contract-driven help and routing surface.

## Build

```sh
cargo build --release
```

The compiled binary will be at `./target/release/acp-agent-cli`.

## Invocation Layers

The generated CLI uses three different invocation contexts. Keep them distinct:

- Final installed skill contract: `acp-agent-cli ...`
- Local development from repo root: `cargo run -- ...`
- Built release binary from repo root: `./target/release/acp-agent-cli ...`

`SKILL.md` documents the final installed contract with the bare command name.
This README may also show the development and release-binary forms for local
verification.

## Runtime Conventions

This scaffold follows the shared cli-forge runtime contract:

- leaf commands never auto-render help; validation failures stay structured
- top-level and non-leaf command paths auto-render human-readable help
- `--help` stays human-readable only
- `help` returns structured help in YAML, JSON, or TOML
- human-readable help is man-like and must keep the section order `NAME`,
  `SYNOPSIS`, `DESCRIPTION`, `OPTIONS`, `FORMATS`, `EXAMPLES`, `EXIT CODES`
- runtime directories are separated into `config`, `data`, `state`, `cache`,
  and optional `logs`
- `Active Context` is inspectable and can be persisted or overridden per
  invocation
- daemon lifecycle commands and runtime artifacts live under `state/daemon`

## Package Boundary

The generated package includes the baseline skill files plus any package-local
support files required by enabled capabilities. Repository-owned CI workflows,
release scripts, and release automation are not scaffolded into the generated
project by default. If a target repository later adopts the `cli-forge-publish`
stage's bundled release asset pack, those files live at the target repository
root rather than inside the shipped CLI skill package.

Package-local packaging-ready metadata or support fixtures should appear only
when a supported capability or packaging path explicitly requires them.

## Commands

### Human-Readable Help

Top-level invocation and non-leaf invocation automatically print man-like
human-readable help and exit `0`:

```sh
acp-agent-cli
acp-agent-cli context
acp-agent-cli daemon
```

### Structured Help

```sh
acp-agent-cli help session prompt --format yaml
acp-agent-cli help agent status --format json
acp-agent-cli help daemon status --format json
```

### `--help`

`--help` always renders the same man-like human-readable help surface, even if
`--format json` or `--format toml` is present.

### Runtime Directories

```sh
acp-agent-cli paths
acp-agent-cli paths --log-enabled
```

### Active Context

```sh
acp-agent-cli context show
acp-agent-cli context use --workspace /tmp/demo --session local-1 --selector provider=staging
```

### Planned ACP Command Families

The CLI now publishes the planned command tree for:

- `agent`
- `workspace`
- `session`
- `command`
- `skill`
- `proposal`
- `file`
- `code`
- `terminal`
- `port-forward`
- `events`
- `state`
- `passthrough`
- `repl`
- `daemon`

The contract surface for these paths is available through `help ...`. Local
runtime-backed commands such as `paths`, `context`, `repl`, and daemon control
already execute directly. The remaining ACP workflow commands currently return
stable structured envelopes while the ACP backend transport is being wired in.

### Diagnostic Run Command

Default YAML output:

```sh
acp-agent-cli run demo-input
```

JSON output:

```sh
acp-agent-cli run demo-input --format json
```

Per-invocation context override:

```sh
acp-agent-cli run demo-input --selector provider=preview
```

Daemon-routed execution:

```sh
acp-agent-cli run demo-input --via daemon --ensure-daemon
```

Missing required leaf input stays a structured failure on `stderr`; it does
not auto-render help.

### Daemon Lifecycle

```sh
acp-agent-cli daemon start
acp-agent-cli daemon status --format json
acp-agent-cli daemon restart
acp-agent-cli daemon stop
```

The daemon runs in a single-instance app-server mode. Local IPC is the default
transport. Runtime artifacts live beneath `state/daemon`, including the
optional TCP auth token file `auth.token_when_tcp_enabled`.

### REPL

```sh
acp-agent-cli repl --session local-1
acp-agent-cli repl --help
```

The REPL keeps plain-text help, persisted history, and slash-command
completion for the common control-plane shortcuts.

### Local Development

Use `cargo run -- ...` while iterating without a release build:

```sh
cargo run -- help run
cargo run -- run demo-input --format json
cargo run -- daemon status
```

### Built Release Binary

After `cargo build --release`, you can verify the compiled binary directly:

```sh
./target/release/acp-agent-cli help run
./target/release/acp-agent-cli run demo-input --selector provider=preview
./target/release/acp-agent-cli daemon status
```

### Optional Features

Streaming and REPL are planned as first-class surfaces. Structured help already
documents the streaming-oriented command paths, and REPL help remains plain
text only.

## Development

Run tests:

```sh
cargo test
```

Lint:

```sh
cargo clippy -- -D warnings
```

Format check:

```sh
cargo fmt --check
```

---

*Generated: 2026-04-16*
