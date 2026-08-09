---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["cli", "cortex", "clap", "completions", "version", "format", "rust-cli"]
---

# 200 CLI - Spec

> The `cortex` command-line surface: a thin, usable pre-alpha wrapper over the `cortex-rs` crate. It provides noun-then-verb read/edit/save commands and a persistent held-session daemon; all protocol, domain and save-policy behaviour remains in the library.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at the top of (`[Flow.CLI]`).
- [100-transport spec](../100-transport/spec.md) - the HID transport, including exclusive ownership.
- [130-domain-model spec](../130-domain-model/spec.md) - typed catalog, preset, cache and output views.
- [150-client spec](../150-client/spec.md) - the implemented `QuadCortex` API the command surface calls.
- [house-style rust-cli.md](https://github.com/marcus-pacharanero/house-style/blob/main/rust-cli.md) - the CLI shape rules this surface follows.
- [pyquadcortex qcctl](https://github.com/stokes-audio/pyquadcortex) - the MIT-licensed CLI whose command surface informs the planned commands.
- Owned source: `crates/cortex-cli/src/{main,connect,decode}.rs`, `crates/cortex-cli/Cargo.toml`; shared host contract and IPC: `crates/cortex-host/src/`.

## Problem Statement

The CLI is one of three surfaces (CLI, MCP server, Tauri GUI) over the same `cortex-rs` crate. Its job is to be the scriptable, terminal-first interface for a Linux user with a Quad Cortex on the desk: read the firmware version, recall a preset, switch a scene, dump a preset blob, list what is on the device. It is thin by design - it parses arguments, calls the crate, prints the result - and it must stay thin so the MCP server and the Tauri backend can reuse the same behaviour without a third implementation drifting alongside.

The interesting requirements are not "parse args and print" but the house-style rules that make the CLI composable and agent-friendly: a bare invocation is helpful, every command has a short description, data goes on stdout and hints on stderr, a `--format text|json` global flag is honoured by every command, shell completions are generated from the live clap command tree, and the surface is machine-discoverable via `--schema`.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| `cortex device version` returns the shared typed identity over direct or held-session paths | Hardware-verified | Succeeds against CorOS 4.0.1 and both paths agree |
| `cortex --version` / `-V` prints the crate version | Implemented | clap `#[command(version)]` + `propagate_version` |
| `cortex completions <shell>` prints to stdout for bash, zsh, fish, powershell, elvish | Implemented | `clap_complete::generate` from the live `Cli::command()` |
| SIGPIPE reset on Unix | Implemented | `libc_sigpipe_reset()` in `main()` before `Cli::parse()` |
| `arg_required_else_help = true` | Implemented | Bare `cortex` prints help and exits successfully |
| Noun-then-verb preset, setlist, grid, block, row, scene, catalog and device commands | Implemented; wire operations hardware-verified as listed in the roadmap | All ordinary device commands delegate to `QuadCortex` directly or through the held session |

## User Stories

### Primary Users

Linux users with a Quad Cortex, script writers, AI coding agents driving the CLI via `--format json`, and maintainers running `cortex device version` as a smoke test.

### Stories

**As a** Linux user
**I want** `cortex device version` to read the real device firmware over USB
**So that** I can confirm the device is talking and the protocol version is supported.

**As a** script writer
**I want** `cortex device version --format json` to emit structured JSON on stdout
**So that** I can pipe it into `jq` or another tool without scraping text.

**As an** AI agent
**I want** `cortex --schema` to emit a JSON Schema of command inputs
**So that** I can discover the surface without scraping `--help`.

**As a** shell user
**I want** `cortex completions zsh` to print a completion script I can source
**So that** tab-completion works for every subcommand and flag.

**As a** maintainer
**I want** a bare `cortex` invocation to print help, not error
**So that** a new user running the binary with no arguments learns what it does.

## Requirements

### Functional Requirements

#### Implemented

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | The binary is `cortex` (`[[bin]] name = "cortex"`), built from a thin `main.rs` that delegates all behaviour to the crate. | Must Have |
| FR-2 | `cortex device version` uses the held daemon when available or the bounded minimal direct diagnostic otherwise, and prints the shared typed `DeviceVersion` view. | Must Have |
| FR-3 | `cortex completions <shell>` prints shell completions to stdout via `clap_complete::generate`, supporting bash, zsh, fish, powershell, and elvish. | Must Have |
| FR-4 | `cortex --version` / `cortex -V` prints the crate version (clap `#[command(version)]` + `propagate_version`). | Must Have |
| FR-5 | SIGPIPE is reset to `SIG_DFL` on Unix at startup so output pipes into `head`/`less` without a panic on a closed pipe. | Must Have |
| FR-6 | `arg_required_else_help = true`: a bare `cortex` invocation prints help and exits successfully, rather than erroring on a missing subcommand. | Must Have |
| FR-7 | Every command carries a short `///` doc-comment used as the clap `about`, so `--help`, completions, and generated docs have no blank rows. | Must Have |
| FR-8 | Errors print to stderr as `cortex: {e:#}` and the process exits with `ExitCode::FAILURE`; data never touches stderr. | Must Have |

#### Implemented command surface

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-10 | A global `--format text\|json` flag (default `text`) is honoured by every command. `text` is for humans; `json` is for scripts and agents. `cortex device version --format json` emits structured version output. | Must Have |
| FR-11 | `cortex preset recall --setlist <path> --slot <slot>` recalls a preset on the device (delegates to `QuadCortex::recall_preset`). | Must Have |
| FR-12 | `cortex scene switch|label|unlabel|color|copy|swap` delegates to the shared scene API. Indices are zero-based 0-7; colour accepts `0xAARRGGBB`, `#RRGGBB`, or decimal. The shipped `cortex scene --index <n>` switch shorthand remains accepted. | Must Have |
| FR-13 | `cortex preset show --setlist <path> --slot <slot>` recalls a preset and prints a typed summary (text by default, JSON with `--format json`). | Must Have |
| FR-14 | `cortex preset list --setlist <path>` lists presets in a setlist (delegates to `QuadCortex::list_presets`). | Must Have |
| FR-15 | `cortex setlist list` lists folders the device knows (delegates to `QuadCortex::list_folders`). | Should Have |
| FR-17 | Ordinary commands delegate to `QuadCortex` directly or through the held daemon; the fast unconnected version diagnostic remains deliberately minimal. | Should Have |

#### Planned

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-16 | `cortex --schema` / `cortex --print-schema` emits a JSON Schema of command inputs - the authoritative input contract for scripts and agents. | Should Have |

#### Persistent session

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-18 | `cortex session start` claims its owner-only local IPC endpoint before opening the exclusive HID interface, performs one subscribed handshake, and serves line-delimited JSON requests until stopped. Unix uses a domain socket; Windows will use a current-user named pipe behind the same host facade. | Must Have |
| FR-19 | Every ordinary device command uses the daemon when it is running and falls back to one direct session otherwise. Diagnostics that can use held state, including `device probe`, route through it; no command opens a second HID connection while the daemon owns the interface. | Must Have |
| FR-20 | The daemon serves only responsive `Live` cache entries, falls back to explicit reads for missing state, and reports cache phase/generation/revision and reducer counters in `session status`. | Must Have |
| FR-21 | A background monitor invalidates state before replacing a silent or continuity-invalidated session, excludes and drains device operations, explicitly releases the old handle before opening another, retries the full subscribed handshake with exponential backoff capped at 30 seconds, and exposes connected/reconnecting/failed status. | Must Have |
| FR-22 | Requests received during reconnect fail immediately with the attempt and last error; status and shutdown remain available. | Must Have |
| FR-23 | `cortex preset move --from <slot> --to <slot>` routes through the versioned daemon protocol when held, otherwise uses one direct client session. It executes by default; `-n`/`--dry-run` reports the exact source, destination, and setlist without opening a session or changing the unit. Execution delegates source-path resolution, occupancy refusal, and listing-convergence checks to `QuadCortex::move_preset`; MCP exposes no corresponding tool. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | `main.rs` stays thin: argument parsing, dispatch, and output formatting only. All protocol and domain behaviour lives in the crate. | Review-enforced |
| NFR-2 | clap derive is used, never the builder API. Subcommands are `#[derive(Subcommand)]`, args are `#[derive(Parser)]`. | Review-enforced |
| NFR-3 | `anyhow::Result` is the binary's error type; typed error enums live in the library crate for callers to match on. | Review-enforced |
| NFR-4 | stdout carries the result; stderr carries hints, progress, and errors. A user piping `cortex device version --format json \| jq` gets clean data on stdout. | Review-enforced |
| NFR-5 | Shell completions are generated from the live `Cli::command()` tree, never hand-maintained. | CI-enforced |
| NFR-6 | The binary compiles and runs with `cargo run -p cortex-cli` on a machine with the udev rule installed and a Quad Cortex connected. | Hardware smoke |

## Acceptance Criteria

- [x] `cortex device version` prints all `VersionMessage` fields on stdout against a real Quad Cortex.
- [x] `cortex --version` and `cortex -V` print the crate version.
- [x] `cortex completions <shell>` prints a completion script for each supported shell.
- [x] A bare `cortex` prints help and exits successfully.
- [x] `cortex device version | head -1` does not panic (SIGPIPE reset).
- [x] Errors go to stderr; stdout is clean data only.
- [x] `cortex device version --format json` emits structured JSON on stdout.
- [x] Preset recall/show/list, scene, setlist, grid, block, row, catalog and device operations are implemented and delegate to the client layer.
- [ ] `cortex --schema` emits a JSON Schema of command inputs.
- [x] Every command honours `--format text|json`.
- [x] An exclusivity-aware reconnect test retains an old `Arc<Session>`, proves its lease drops before the first replacement attempt, fails that attempt, succeeds on the second, swaps the session, and advances the retained cache generation even when the old link remained responsive after continuity invalidation.

## Non-Goals

- **Protocol or domain logic.** Owned by the crate (zones 100-150). The CLI calls the crate's public API; it does not reimplement framing, protobuf, or the client surface.
- **The MCP server.** Owned by zone 300. The CLI and MCP server are sibling surfaces over the same crate.
- **The Tauri GUI.** Owned by zone 400; it is a sibling surface over the same crate and daemon contract.
- **Interactive editing.** The CLI is batch-oriented (parse, call, print). A REPL or interactive TUI is out of scope; the GUI owns interactive editing.

## Dependencies

- **`cortex-rs`** (workspace path) - `QuadCortex`, `Session`, state cache, save policy and typed views.
- **`cortex-host`** (workspace path) - daemon protocol, bounded client, local endpoint/listener/connection facade and ownership claim.
- **`clap`** (derive) - argument parsing.
- **`clap_complete`** - shell completions from the live command tree.
- **`anyhow`** - the binary's error type.
- **`prost`** - protobuf encode/decode for direct diagnostics and trace tooling.
- **`serde_json`** - JSON output and the line-delimited daemon contract.
- **house-style rust-cli.md** - the shape rules this surface follows (thin main, clap derive, data on stdout, SIGPIPE reset, completions, `--format`, `--schema`).

## Future

- **`--format yaml`.** A third format option for the global flag, if the need arises. JSON covers the script/agent case; YAML is a human-friendly middle ground.
- **`--dry-run` on mutating commands.** Every device, lifecycle and local-filesystem mutation executes by default and honours global `-n`/`--dry-run` before IPC, HID, process or filesystem access. Read-only commands accept and ignore the flag. Plans report requested targets and identify device-resolved checks deferred until execution.
- **Progress widgets on long-running commands.** The CLI prints phase progress to stderr today; auto-hiding count/byte bars may be worthwhile for operations with a known total.
- **Registry-driven dispatch.** If the command surface grows large, a single registry driving the CLI, schema, and any MCP tool surface keeps them from drifting.

## Glossary

| Term | Definition |
| --- | --- |
| Thin main | `main.rs` parses args and delegates; all behaviour lives in the library crate |
| `--format` | Global flag (`text` default, `json` for scripts/agents) honoured by every command |
| `--schema` | Emits a JSON Schema of command inputs for machine discoverability |
| Completions | Shell completion scripts generated from the live clap command tree via `clap_complete` |
| SIGPIPE reset | `signal(SIGPIPE, SIG_DFL)` at startup so output pipes into `head`/`less` without a panic |
| Data on stdout | stdout carries the result; stderr carries hints, progress, and errors |
