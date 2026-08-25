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
- Owned source: `crates/cortex-cli/src/{main,connect,decode}.rs`; shared host boundary: `crates/cortex-host/src/`.

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

The `cortex` binary, the `cortex-mcp` server, and the Tauri backend are three surfaces over one crate. If protocol or domain logic lived in `main.rs`, the sibling surfaces would have to reimplement it or depend on the CLI binary (which is wrong). Keeping `main.rs` thin is what makes the crate the single implementation.

Ordinary commands call `QuadCortex` directly or send typed requests to the persistent daemon. Direct transport access is reserved for deliberately unconnected diagnostics and trace decoding; it is never used to duplicate domain behaviour.

### Alternatives considered

- **Builder API for clap.** Rejected: house style is `#[derive(Parser)]` / `#[derive(Subcommand)]`, never the builder. Derive keeps the command tree declarative and lets `clap_complete` generate from it.
- **A reusable CLI library.** Rejected: reusable daemon protocol and IPC moved into `cortex-host`, while clap parsing remains binary-specific.

## [DES-CLI] Command tree

### Behaviour

The clap derive tree is noun-then-verb: `session`, `preset`, `setlist`, `grid`, `block`, `row` and `device` group the operations a player recognises. Explicitly invoking a mutating command executes it by default; global `-n`/`--dry-run` classifies the command and returns before any IPC, HID, process or filesystem boundary. Read-only commands accept and ignore the flag. `preset move` names exact source and destination slots and refuses an occupied destination from a fresh listing. `preset copy` exposes both exact slots and typed instrument metadata; `setlist create|delete|duplicate` accept safe USER names and surface partial duplication. `scene` is a deliberate direct action; `catalog`, `completions` and trace decoding complete the current surface. The generated [CLI reference](../../docs/cli-reference.md) is the authoritative command inventory.

### Design choice: `arg_required_else_help = true`

A bare `cortex` prints help and exits successfully. This is the house-style rule: a bare invocation must be helpful, not an error. `subcommand_required = false` + `command: Option<Command>` lets the `None` arm in `run()` call `Cli::command().print_help()`.

### Design choice: `Option<Command>` over `Required`

clap's `subcommand_required = true` exits with a non-zero code and a "missing subcommand" error. The house-style wants a bare invocation to be helpful, which means printing help and exiting successfully. `Option<Command>` with `arg_required_else_help = true` gives both: clap prints help before `parse()` returns, and the `None` arm in `run()` covers the case where `--help` was not triggered.

### Device routing

`cortex session start` owns one subscribed connection for the selected product and serves typed line-delimited requests over product-scoped local IPC. Quad retains the legacy `cortex.sock`; Nano uses `cortex-nano.sock`, with derived lock and log paths. `session status` and `session stop` default to Quad for compatibility and accept `--device nano` for the Nano endpoint. Ordinary Quad commands use the Quad daemon when available and otherwise open one bounded direct session; Nano commands route only to the Nano daemon. Each endpoint is claimed before its handshake so a second current-version process cannot race startup and wedge that physical device. A Nano start also probes the legacy Quad endpoint and refuses to open HID when an older Nano daemon already owns it. Simultaneously launching old and new binaries is unsupported because the older process cannot participate in the product-scoped ownership contract; stop all pre-upgrade daemons before starting the new version. Distinct Quad and Nano daemons may coexist because they own distinct USB interfaces; the invariant is one owner per physical device, not one owner for the whole host. `cortex-host` owns the platform seam and concurrent accept/request lifecycle: owner-only Unix domain sockets today, and future current-user Windows named pipes. Cached values are served only while their generation is usable; missing values fall back to explicit reads.

### Request-based lifecycle

An ordinary explicit `cortex session start`, foreground or detached, is persistent and exits only through shutdown, terminal/process termination, or failure. A host that starts the installed sibling binary uses the deliberately separate hidden contract `cortex session start --foreground --auto-managed --idle-timeout-seconds N`. Requiring both arguments prevents a user-started daemon from unexpectedly inheriting host lifecycle.

The host server accepts connections concurrently. Activity starts only when one complete line parses as a typed request, stays active through handler execution and response writing, and restarts the full timeout on completion even when the response reports an operation error. This makes status/version-gate requests real activity while excluding connected-but-silent clients, blank lines, and malformed traffic. The idle verdict requires zero in-flight requests, so one slow operation is protected while another client can still query status or request shutdown. Device-backed requests share one fail-fast mutex with reconnect: a second device operation is refused rather than queued, because a queued mutation could outlive its caller's IPC timeout and execute unexpectedly. Shutdown stops admission, interrupts reconnect backoff and waits for that mutex before closing HID and acknowledging. A three-second watchdog handles a genuinely hung operation by exiting without an acknowledgement; this bounds HID ownership but can interrupt the active operation, so it is not described as a graceful drain.

On idle expiry, normal daemon teardown is used rather than process kill: reconnect waits are interrupted, the operation write gate drains requests, `Session::close()` explicitly releases the old HID link, and only then is the endpoint removed. A protocol-skewed client receives status, refuses further work with the explicit stop/restart instruction, and leaves the daemon running until its ordinary timeout; only cross-version `Shutdown` bypasses the compatibility refusal.

### Design choice: `--format` as a global flag

A global `--format text|json` flag on `Cli` (not per-command) is the house-style pattern. Every command honours it; `text` is the default for humans, `json` is for scripts and agents. The flag is propagated to each `run()` function, which branches on it for output formatting. This keeps the output contract uniform across the surface.

### Alternatives considered

- **Per-command `--format`.** Rejected: the contract must be uniform. A user piping `cortex preset recall --format json | jq` should not have to remember which commands support it.
- **A `--json` boolean.** Rejected: `--format` is extensible (yaml, table) without adding a flag per format.

## [DES-VERSION] The `version` command

### Behaviour

`cortex device version` is the cheapest hardware diagnostic. It can read the version without the full subscribed handshake, while ordinary commands use the client/daemon paths.

### Design choice: minimal unconnected diagnostic

The device answers a plain Version READ without the full connect handshake. Keeping a minimal diagnostic path distinguishes connection/permission failures from handshake failures and avoids paying several seconds when only identity is needed. It still refuses to open while the daemon owns HID.

### Design choice: YAML-like text, not structured YAML

Text prints flat `label: value` lines from the shared `cortex_rs::view::DeviceVersion`; JSON serialises the same stable view. Hosts do not expose prost wire structs or depend on vendor field names as their public contract.

### Alternatives considered

- **Wait for the client layer before shipping `version`.** Rejected: `cortex device version` is the hardware smoke test. Shipping it early proved the transport, framing, and proto layers against a real device before the session layer existed.
- **Use `serde_yaml` for the text path.** Rejected: a dependency for a flat `label: value` print. The Debug-strip trick is cheap and dependency-free.

## [DES-COMPLETIONS] Shell completions

### Behaviour

`cortex completions <shell>` calls `clap_complete::generate(shell, &mut Cli::command(), name, &mut std::io::stdout())`. The shell is a `clap_complete::Shell` enum, which covers bash, zsh, fish, powershell, and elvish.

### Design choice: generate from the live command tree

Completions are generated from `Cli::command()` - the same tree clap parses against. This means completions cannot drift from the actual command surface. The moment a new subcommand is added to the `Command` enum, completions include it with no manual step.

### Design choice: print and install from the live tree

`completions <shell>` prints to stdout for package managers and scripting. `completions install` detects or accepts a shell and writes the conventional user completion filename without editing shell startup files.

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

This is the single most important composability rule. Text and JSON results go to stdout; connection phases, progress and warnings go to stderr. The 42-check hardware smoke parses JSON output through `jq`, so this contract is exercised end to end.

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

- **No `--schema` yet.** Machine discoverability is planned but not implemented.
- **Device-resolved facts are deferred in `--dry-run`.** A no-IPC plan cannot resolve a named parameter to a wire index, inspect slot occupancy or validate a daemon-held save token. Plans distinguish those execution-time checks instead of contacting the daemon and pretending the run is side-effect-free.
- **Only the Unix local-IPC adapter is operational.** The daemon/client no longer expose Unix socket types outside `cortex-host::ipc`; `cortex-host` and `cortex-mcp` cross-check for Windows. A reviewed safe named-pipe adapter, current-user pipe ACLs, Windows detached-process lifecycle and hardware verification remain before Windows is supported.
- **The command implementation remains concentrated in `main.rs`.** Behaviour is in the crate, but command-family modules may become worthwhile as the surface grows.
