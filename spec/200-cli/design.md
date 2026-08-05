---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["cli", "cortex", "clap", "completions", "version", "format", "sigpipe"]
spec: spec.md
---

# 200 CLI - Design

> Design for the `cortex` binary. The interesting parts are not the argument parsing but the house-style rules that make the CLI composable and agent-friendly: thin main, clap derive, data on stdout, SIGPIPE reset, completions from the live tree, and a `--format` global flag honoured everywhere.

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at the top of (`[Flow.CLI]`).
- [house-style rust-cli.md](https://github.com/marcus-pacharanero/house-style/blob/main/rust-cli.md) - the shape rules.
- Owned source: `crates/cortex-cli/src/main.rs`.

## [DES-MAIN] Thin main.rs

### Behaviour

The binary is a thin wrapper: `main()` resets SIGPIPE, parses args via `Cli::parse()`, dispatches to a `run()` function, and maps `Result` to an `ExitCode`. All protocol and domain behaviour lives in the crate.

```rust
fn main() -> ExitCode {
    #[cfg(unix)]
    unsafe { libc_sigpipe_reset(); }
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cortex: {e:#}");
            ExitCode::FAILURE
        }
    }
}
```

### Design choice: behaviour in the crate, not the binary

The `cortex` binary, the `cortex-mcp` server, and the future Tauri backend are three surfaces over one crate. If protocol or domain logic lived in `main.rs`, the MCP server would have to reimplement it or depend on the CLI binary (which is wrong). Keeping `main.rs` thin is what makes the crate the single implementation.

Today `cmd_version()` calls `Transport::open` and `Transport::request` directly because the client layer (150) is not yet implemented. Once `QuadCortex::version()` lands, `cmd_version()` becomes a one-liner delegation. This is the only place the CLI reaches below the client layer, and it is a temporary state.

### Alternatives considered

- **Builder API for clap.** Rejected: house style is `#[derive(Parser)]` / `#[derive(Subcommand)]`, never the builder. Derive keeps the command tree declarative and lets `clap_complete` generate from it.
- **A `lib.rs` in `cortex-cli`.** Rejected for now: the CLI has no behaviour worth embedding (the crate already owns it). If a host CLI ever needs to embed `cortex` as a subcommand (like `gitehr calc` embeds `clincalc`), a `cli::run()` module will appear; until then, `main.rs` is enough.

## [DES-CLI] Command tree

### Behaviour

The command tree is a clap derive enum:

```rust
#[derive(Parser, Debug)]
#[command(
    name = "cortex",
    version,
    about,
    long_about = None,
    propagate_version = true,
    subcommand_required = false,
    arg_required_else_help = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Read the device firmware version (CorOS, app, bootloader, zencoder).
    Version,
    /// Print shell completions to stdout.
    Completions {
        /// The shell to generate completions for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}
```

### Design choice: `arg_required_else_help = true`

A bare `cortex` prints help and exits successfully. This is the house-style rule: a bare invocation must be helpful, not an error. `subcommand_required = false` + `command: Option<Command>` lets the `None` arm in `run()` call `Cli::command().print_help()`.

### Design choice: `Option<Command>` over `Required`

clap's `subcommand_required = true` exits with a non-zero code and a "missing subcommand" error. The house-style wants a bare invocation to be helpful, which means printing help and exiting successfully. `Option<Command>` with `arg_required_else_help = true` gives both: clap prints help before `parse()` returns, and the `None` arm in `run()` covers the case where `--help` was not triggered.

### Planned commands

The planned commands (`recall`, `scene`, `dump-preset`, `list-presets`, `list-folders`) are variants on the same pattern: a clap struct with path/slot/index args, a `run()` that opens a `QuadCortex` client and calls the corresponding method, and a print function that honours `--format`. They will be added as variants to `Command` once the client layer (150) lands.

### Design choice: `--format` as a global flag

A global `--format text|json` flag on `Cli` (not per-command) is the house-style pattern. Every command honours it; `text` is the default for humans, `json` is for scripts and agents. The flag is propagated to each `run()` function, which branches on it for output formatting. This keeps the output contract uniform across the surface.

### Alternatives considered

- **Per-command `--format`.** Rejected: the contract must be uniform. A user piping `cortex preset recall --format json | jq` should not have to remember which commands support it.
- **A `--json` boolean.** Rejected: `--format` is extensible (yaml, table) without adding a flag per format.

## [DES-VERSION] The `version` command

### Behaviour

`cortex device version` is the one implemented command that touches the device. It opens a `Transport`, builds a `VersionMessage{action: READ}`, encodes it with `prost`, calls `Transport::request`, decodes the reply, and prints every field as YAML-like text to stdout.

### Design choice: direct transport call (temporary)

The `version` command works without the full connect handshake - a plain Version READ gets a reply. This is why it can call `Transport::request` directly today, before the session (140) and client (150) layers exist. Once `QuadCortex::version()` lands, `cmd_version()` switches to:

```rust
fn cmd_version(client: &QuadCortex, format: Format) -> Result<()> {
    let v = client.version(Duration::from_secs(10))?;
    match format {
        Format::Text => print_version_text(&v),
        Format::Json => print_version_json(&v),
    }
}
```

### Design choice: YAML-like text, not structured YAML

The current `print_version` function prints `label: value` lines, one per field, extracting oneof-wrapped strings by Debug-formatting the variant and stripping the wrapper (`AppFwVersion("d14e")` -> `d14e`). This is human-readable and does not pull in a YAML serializer. Once `--format json` lands, the JSON path uses `serde_json::to_string_pretty` on a serialisable struct.

### Alternatives considered

- **Wait for the client layer before shipping `version`.** Rejected: `cortex device version` is the hardware smoke test. Shipping it early proved the transport, framing, and proto layers against a real device before the session layer existed.
- **Use `serde_yaml` for the text path.** Rejected: a dependency for a flat `label: value` print. The Debug-strip trick is cheap and dependency-free.

## [DES-COMPLETIONS] Shell completions

### Behaviour

`cortex completions <shell>` calls `clap_complete::generate(shell, &mut Cli::command(), name, &mut std::io::stdout())`. The shell is a `clap_complete::Shell` enum, which covers bash, zsh, fish, powershell, and elvish.

### Design choice: generate from the live command tree

Completions are generated from `Cli::command()` - the same tree clap parses against. This means completions cannot drift from the actual command surface. The moment a new subcommand is added to the `Command` enum, completions include it with no manual step.

### Design choice: print to stdout, not install

The current interface is `completions <shell>` (print to stdout), which is the stable interface for package managers and scripting. The house-style ideal adds `completions install [--shell <shell>] [--dir <path>]` as the human interface. That is a planned follow-up (see [Future](spec.md#future)).

## [DES-SIGPIPE] SIGPIPE reset

### Behaviour

On Unix, `main()` calls `libc_sigpipe_reset()` before `Cli::parse()`. This calls `signal(SIGPIPE, SIG_DFL)` via an `extern "C"` declaration, resetting SIGPIPE to its default disposition. Without this, a Rust process piping into `head -1` panics when `head` closes the pipe after the first line.

### Design choice: raw `extern "C"` over a crate

The `signal` call is a one-liner; pulling in `nix` or `libc` for a single `signal(SIGPIPE, SIG_DFL)` is a dependency the binary does not need. The `unsafe extern "C"` block is documented with the SAFETY reasoning.

```rust
#[cfg(unix)]
unsafe fn libc_sigpipe_reset() {
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;
    let _ = unsafe { signal(SIGPIPE, SIG_DFL) };
}
```

### Alternatives considered

- **`libc` crate.** Rejected: a dependency for one syscall. The raw declaration is smaller and equally clear.
- **`nix` crate.** Rejected: same, plus `nix` is a larger surface than needed.
- **Ignore SIGPIPE.** Rejected: a CLI that panics when piped into `head` is not composable. This is a house-style rule.

## [DES-OUTPUT] Output contract

### Behaviour

Data goes on stdout; everything else (hints, progress, errors) goes on stderr. `eprintln!("cortex: {e:#}")` is the error path; `println!` is the data path. A user piping `cortex device version | head -1` gets the first data line on stdout and never sees an error mixed in.

### Design choice: `eprintln` for errors, `println` for data

This is the single most important composability rule. The current `print_version` function uses `println!` exclusively; errors in `run()` go through `eprintln!`. Once `--format json` lands, the JSON output goes on stdout and any human-readable hints ("opening device...", "request timed out") go on stderr.

### Alternatives considered

- **A logging crate (`tracing`/`log`).** Rejected for the binary: the CLI is short-lived and `eprintln` is enough. The crate may use `tracing` internally; the binary does not need to.

## [DES-SCHEMA] Machine discoverability (planned)

### Behaviour

`cortex --schema` / `cortex --print-schema` emits a JSON Schema of command inputs. This is the house-style pattern for making the surface discoverable by scripts and LLMs without scraping `--help`.

### Design choice: schema from the clap tree

The schema is generated from the same `Cli::command()` tree that drives parsing and completions. This keeps the three surfaces (parsing, completions, schema) in sync. The exact mechanism (a `clap` -> JSON Schema bridge, or a hand-maintained schema validated against the tree) is a design decision for when the feature lands.

### Alternatives considered

- **Scrape `--help`.** Rejected: brittle, not structured, and drifts from the tree.
- **Hand-maintain a schema.** Rejected: drifts from the tree the moment a command is added.

## [DES-LIMITS] Known Limitations

- **No `--format` flag yet.** The current `version` command prints text only. The global `--format text|json` flag and JSON output are planned.
- **No `--schema` yet.** Machine discoverability is planned but not implemented.
- **Direct transport call in `version`.** `cmd_version()` calls `Transport::request` directly, bypassing the session and client layers. This is temporary; it switches to `QuadCortex::version()` once 150 lands.
- **No `completions install`.** Only `completions <shell>` (print-to-stdout) is implemented. The human-friendly `install` subcommand is planned.
- **No `--dry-run`.** No mutating commands are implemented yet; `--dry-run` follows when they land.
- **No progress bars.** No long-running commands are implemented yet; `indicatif` on stderr follows when `dump-preset` / `list-presets` land.
