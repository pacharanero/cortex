---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["cli", "tasks", "roadmap"]
spec: spec.md
design: design.md
---

# 200 CLI - Tasks

> Implementation tasks for the `cortex` command-line surface. Phase 1 is done; phase 2 is the `--format` and `--schema` contract; phase 3 is the planned commands (pending the client layer, zone 150); phase 4 is the house-style polish.

## Phase 1 - Core CLI (done)

### 1.1 Binary scaffold

- [x] Create `crates/cortex-cli/` with `[[bin]] name = "cortex"`.
- [x] Depend on `cortex-rs` (workspace path), `anyhow`, `clap` (derive), `clap_complete`, `prost`.
- [x] Thin `main.rs`: `main()` -> `run()` -> `ExitCode`, `anyhow::Result` throughout.

### 1.2 Command tree

- [x] `Cli` struct with `#[derive(Parser)]`, `arg_required_else_help = true`, `propagate_version`.
- [x] `Command` enum with `#[derive(Subcommand)]`: `Version`, `Completions`.
- [x] Every command carries a short `///` doc-comment used as the clap `about`.
- [x] `None` arm in `run()` calls `Cli::command().print_help()`.

### 1.3 `version` command

- [x] `cmd_version()`: `Transport::open(DeviceKind::QuadCortex)`, build `VersionMessage{action: READ}`, `prost::Message::encode_to_vec`, `Transport::request`, `prost::Message::decode`.
- [x] `print_version()`: YAML-like `label: value` lines, oneof-wrapped strings extracted via Debug-strip.
- [x] 10s timeout (firmware reply can be slow on first connect).

### 1.4 `completions` command

- [x] `Command::Completions { shell: clap_complete::Shell }`.
- [x] `clap_complete::generate(shell, &mut Cli::command(), name, &mut std::io::stdout())`.

### 1.5 House-style essentials

- [x] SIGPIPE reset on Unix (`libc_sigpipe_reset()` in `main()`).
- [x] `cortex --version` / `-V` via `#[command(version)]` + `propagate_version`.
- [x] Errors to stderr as `cortex: {e:#}`, exit `ExitCode::FAILURE`.
- [x] Data on stdout only.

## Phase 2 - Output contract (planned, next)

### 2.1 Global `--format` flag

- [ ] Add `--format text|json` global arg to `Cli` (default `text`).
- [ ] `cmd_version()` branches on format: text (current YAML-like output) vs json (`serde_json::to_string_pretty` on a serialisable struct).
- [ ] Define a `VersionOutput` serde struct that mirrors the `VersionMessage` fields worth surfacing.
- [ ] Every future command honours `--format` from the start.

### 2.2 `--schema` / `--print-schema`

- [ ] Emit a JSON Schema of command inputs from the live `Cli::command()` tree.
- [ ] Decide mechanism: a `clap` -> JSON Schema bridge, or a hand-maintained schema validated against the tree.
- [ ] Test: schema round-trips (parse the schema, confirm it covers every command).

### 2.3 Contract enforcement

- [ ] Walk `Cli::command()` recursively and assert every leaf command honours `--format` (introspection test).
- [ ] Assert every command has a non-empty `about` (no blank rows in `--help`).

## Phase 3 - Planned commands (blocked on client layer, zone 150)

### 3.1 `cortex recall --setlist <path> --slot <slot>`

- [ ] Delegate to `QuadCortex::recall_preset(setlist_path, position, is_factory, request_id)`.
- [ ] Print confirmation (setlist, slot, preset name) on stdout; `--format json` for structured output.
- [ ] Document that recall changes what is heard but nothing persistent is lost.

### 3.2 `cortex scene --index <n>`

- [ ] Delegate to `QuadCortex::switch_scene(scene)`.
- [ ] Print the active scene on stdout.
- [ ] Note: scenes are 0-based in the API, 1-4 on screen (the row-numbering trap).

### 3.3 `cortex dump-preset --setlist <path> --slot <slot>`

- [ ] Delegate to `QuadCortex::read_preset(setlist_path, position, is_factory, timeout)`.
- [ ] Print the full `BinaryPreset` (text summary by default, JSON with `--format json`).
- [ ] Note: `read_preset` RECALLS the slot (side effect) - document this in `--help`.

### 3.4 `cortex list-presets --setlist <path>`

- [ ] Delegate to `QuadCortex::list_presets(setlist, timeout, include_empty)`.
- [ ] Print one preset per line (text) or a JSON array (`--format json`).

### 3.5 `cortex list-folders`

- [ ] Delegate to `QuadCortex::list_folders(seconds)`.
- [ ] Print one folder per line (text) or a JSON array (`--format json`).

### 3.6 Switch `version` to the client layer

- [ ] Once `QuadCortex::version()` lands (zone 150), switch `cmd_version()` from `Transport::request` to `QuadCortex::version()`.
- [ ] Remove the direct `Transport::open` / `prost` encode/decode from `main.rs`.

## Phase 4 - House-style polish (planned)

### 4.1 `completions install`

- [ ] Add `cortex completions install [--shell <shell>] [--dir <path>]` subcommand.
- [ ] Detect current shell from `$SHELL` when `--shell` is omitted.
- [ ] Write the correctly named file to the standard user completion directory.
- [ ] Print any one-time shell config the user still needs; never edit `.bashrc`/`.zshrc`/profiles.

### 4.2 `--dry-run` on mutating commands

- [ ] Add a global `-n` / `--dry-run` flag.
- [ ] `cortex recall --dry-run` prints the plan (setlist, slot, preset name) without touching the device.
- [ ] `cortex scene --dry-run` prints the target scene without switching.
- [ ] Read-only commands accept and ignore `--dry-run` so scripts can pass it uniformly.

### 4.3 Progress on long-running commands

- [ ] `dump-preset` and `list-presets` may take >1s over USB; add an `indicatif` spinner on stderr.
- [ ] Auto-hide when not a TTY (pipe, redirect, CI, dumb TERM).
- [ ] Centralise the widget styling in one module if multiple commands need it.

### 4.4 Path argument handling

- [ ] Add a `value_parser` that expands `~/` to `$HOME` on every `PathBuf` argument (`--setlist`, etc.).
- [ ] Add `value_hint = ValueHint::FilePath` / `ValueHint::DirPath` so completions offer files/dirs.

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1-1.5 | Implemented `cortex` CLI (version, completions, SIGPIPE, arg_required_else_help) | `crates/cortex-cli/src/main.rs`, `crates/cortex-cli/Cargo.toml` | [x] | [x] |
| 2026-08-01 | 1.5 | Wrote spec/design/tasks for this zone | `spec/200-cli/{spec,design,tasks}.md` | [x] | [x] |