---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["mcp", "cortex-mcp", "safety-surface", "tool-model", "rmcp", "provisional"]
spec: spec.md
---

# 300 MCP - Design (stub)

> Design for the `cortex-mcp` MCP server. The interesting part is not the tool list - it is the **safety surface** that gates destructive operations and surfaces the silent-failure traps an agent would not notice. The tool model is a thin wrapper over the client layer (150).

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [AGENTS.md](../../AGENTS.md) - the MCP safety surface design (the source of truth).
- [Protocol research note](../../quad-cortex-linux-editor-and-protocol.md) - the proposed tool surface (at the parent workspace root).
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at (`[Flow.MCP]`).
- [100-transport design](../100-transport/design.md) [DES-EXCLUSIVE] - the exclusive-HID-access invariant.
- [150-client design](../150-client/design.md) - the `QuadCortex` API the tools delegate to.
- Owned source: `crates/cortex-mcp/src/main.rs`.

## [DES-SAFETY] The Safety Surface

### Behaviour

The safety surface is the set of rules that gate destructive operations and surface silent-failure traps. It is the core of this zone. An agent with a `save_preset` tool can overwrite a factory preset or clobber a slot the user cared about, and the device will not stop it. The MCP server must be the boundary that refuses the dangerous case.

The five invariants (from AGENTS.md and the protocol research note):

1. **Read and recall are free; saving is always explicitly confirmed.**
2. **Never write to the factory setlist; restrict saves to a designated scratch range of USER slots unless overridden.**
3. **Back up the target slot (`read_preset`) before overwriting, and keep the blob.**
4. **Surface the row-numbering trap (0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently) in tool descriptions.**
5. **Single owning process for the USB interface.**

### Design choice: classify tools by destructive tier

The proposed tool surface has four tiers, each with a different safety posture:

| Tier | Tools | Safety posture |
| --- | --- | --- |
| Read | `list_presets`, `read_preset`, `list_blocks`, `get_device_version`, `read_current_preset`, `list_folders` | Unrestricted. (But note: `read_preset` has a recall side effect - surface it in the description.) |
| Transient write | `recall_preset`, `switch_scene` | Free. Changes what is heard; nothing persistent is lost. |
| Working-copy write | `set_block`, `set_param`, `set_routing` | Free. Edits the recalled preset in device RAM; persists only on save. Surface the traps (`set_block` DSP refusal, `set_param(scene=)` 3-message rule, row numbering). |
| Destructive | `save_preset` | **Gated.** Require explicit slot, refuse FACTORY setlist, require confirmation, back up target first. |

This tiering is the design that matters. The tool list is a thin wrapper over the client layer; the tiering is what stops an agent from destroying a patch.

### Design choice: confirmation token, not default-yes

`save_preset` requires an explicit confirmation token/flag from the caller. A default-yes (the agent calls `save_preset` and it saves unless `confirm=false`) is unsafe: an agent that calls every tool with defaults will destroy a slot. The confirmation must be an active opt-in: the agent must set `confirm: true` (or supply a token from a prior `read_preset` / `propose_save` call).

### Design choice: refuse FACTORY, always

`save_preset` refuses a slot in the FACTORY setlist regardless of the caller's request. The refusal is a structured error naming the safe alternative ("save to a USER slot"). There is no override for the factory setlist; the scratch-range override (next) applies only to USER slots.

### Design choice: scratch range with explicit override

The default scratch range for saves is a configurable set of USER slots (e.g. slots 900-999 in the USER setlist). Saving outside the scratch range requires an explicit, logged override. This keeps an agent's default behaviour inside a safe sandbox while allowing a knowledgeable user to direct it elsewhere.

### Design choice: backup before overwrite

`save_preset` calls `read_preset` on the target slot before overwriting and retains the blob (in memory for the session, or to a configurable backup path for cross-session rollback). A bad save can be rolled back by restoring the blob. The backup is logged to stderr.

### Design choice: surface traps in tool descriptions

The traps are not errors - the device does not refuse a wrong-row edit or a too-big block. They are silent failures an agent would not notice. The tool descriptions (`inputSchema`) surface them:

- **Row-numbering trap**: every row-accepting tool notes "rows are 0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently."
- **`read_preset` side effect**: `read_preset` notes "RECALLS the slot (loads onto the grid, discards unsaved edits, resets active scene); use `read_current_preset` for inspection during editing."
- **`set_block` DSP refusal**: `set_block` notes "a block that exceeds the preset's processing budget is accepted on the wire and silently absent; `verify=true` (default) catches this."
- **`set_param(scene=)` 3-message rule**: `set_param` notes "a scene-targeted write issues 3 messages (promote scene_mode, switch scene, write) and leaves the unit on the target scene (visible side effect)."

### Alternatives considered

- **Gate every write, not just saves.** Rejected: recall and scene-switch are transient (nothing persistent lost). Gating them would make the server unusable for exploration. Working-copy writes (`set_block`, `set_param`, `set_routing`) edit device RAM but do not persist; they are free, with traps surfaced.
- **Allow FACTORY saves with a stronger confirmation.** Rejected: a factory preset is never the right target for an agent experiment. The refusal is absolute, not a higher bar.
- **Backup to a file, always.** Rejected for the default: in-memory backup is enough for session-scoped rollback. A configurable backup path is a future option for cross-session audit.
- **Trap-surfacing as runtime warnings, not descriptions.** Rejected: the agent does not read warnings it does not ask for. The description is what an agent reads to decide how to call the tool.

## [DES-OWNER] Single Owning Process

### Behaviour

The server constructs one `Transport` at startup and holds it for the process lifetime. Every tool call reuses it. Opening a transport per tool call is a bug, not a pattern (see [100-transport design](../100-transport/design.md) [DES-EXCLUSIVE]).

### Design choice: construct at startup, hold for lifetime

```rust
fn main() -> anyhow::Result<()> {
    let transport = Transport::open(DeviceKind::QuadCortex)?;
    let session = Session::new(transport);
    let client = QuadCortex::new(session);
    let server = CortexMcpServer::new(client);
    rmcp::serve_stdio(server)
}
```

The `Transport`, `Session`, and `QuadCortex` are all held by the server for its lifetime. `rmcp` drives the event loop; each tool call borrows the `QuadCortex` and calls a method.

### Design choice: no per-tool-call reconnect

If the device glitches (read timeout), the server does not silently reconnect. It surfaces the error to the agent and offers a `reconnect` tool (or a manual restart). Silent reconnect hides a flaky device and can race against a concurrently-running CLI.

### Alternatives considered

- **Pool of transports.** Rejected: the HID interface does not multiplex. A pool of one is a held transport.
- **Reopen per tool call.** Rejected: deadlocks against a concurrently-running CLI or against the server's own prior tool calls.
- **Lockfile-based exclusive lock.** Not needed: `hidapi` open already takes the interface; a second open fails. We rely on that.

## [DES-TOOLS] Tool Model

### Behaviour

Each tool is a thin wrapper over a `QuadCortex` method. The tool parses its input (validated against `inputSchema`), calls the client method, and returns the result as JSON. The safety surface (above) gates the destructive tier.

### Design choice: one tool per client method, grouped by tier

The tool list mirrors the client API (zone 150), grouped by the destructive tier. This keeps the surface discoverable: an agent reading the tool list sees the tier in the tool name or description, and the safety posture follows.

| Tool | Client method | Tier |
| --- | --- | --- |
| `list_presets` | `QuadCortex::list_presets` | Read |
| `read_preset` | `QuadCortex::read_preset` | Read (recall side effect) |
| `read_current_preset` | `QuadCortex::read_current_preset` | Read |
| `list_blocks` | (derived from `read_current_preset`) | Read |
| `list_folders` | `QuadCortex::list_folders` | Read |
| `get_device_version` | `QuadCortex::version` | Read |
| `recall_preset` | `QuadCortex::recall_preset` | Transient write |
| `switch_scene` | `QuadCortex::switch_scene` | Transient write |
| `set_block` | `QuadCortex::set_block` | Working-copy write |
| `set_param` | `QuadCortex::set_param` | Working-copy write |
| `set_routing` | `QuadCortex::set_chain_input` / `set_chain_output` | Working-copy write |
| `save_preset` | `QuadCortex::save_current_preset` (gated) | Destructive |

### Design choice: `inputSchema` matches CLI `--schema`

Where a tool corresponds to a CLI command (e.g. `list_presets`, `recall_preset`, `get_device_version`), the `inputSchema` is the same contract the CLI's `--schema` emits. This keeps humans, scripts, and agents on one contract rather than three drifting copies (house-style rust-cli.md).

### Design choice: `save_preset` is a distinct tool, not a flag

`save_preset` is a separate tool, not a `confirm=true` flag on `set_param` or `set_block`. This makes the destructive tier visible in the tool list: an agent enumerating tools sees `save_preset` as a distinct, gated operation, not an option on a safer tool.

### Alternatives considered

- **One generic `edit_preset` tool.** Rejected: the surface must be enumerable and the tiering visible. A generic tool hides the safety posture from an agent reading the tool list.
- **Combine `set_block`/`set_param`/`set_routing` into one `edit_grid` tool.** Rejected for the same reason: the traps are per-operation (row numbering, DSP refusal, 3-message rule), and surfacing them in a combined tool's description buries them.

## [DES-FRAMEWORK] rmcp

### Behaviour

The server uses the `rmcp` crate (workspace dependency, `server` + `transport-io` features) on a `tokio` runtime. The binary's `main()` constructs the `Transport` -> `Session` -> `QuadCortex` -> `CortexMcpServer` and hands it to `rmcp::serve_stdio`.

### Design choice: rmcp over a hand-rolled JSON-RPC server

`rmcp` is the Rust MCP server framework. Using it avoids hand-rolling the JSON-RPC transport and keeps the server on the standard MCP protocol. The workspace pins `rmcp` with `default-features = false` and only the `server` + `transport-io` features, to keep the dependency surface minimal.

### Design choice: tokio runtime

`rmcp` requires an async runtime. `tokio` (macros, net, rt-multi-thread) is the workspace standard. The server is the only surface in this project that needs async; the crate stays sync (leaf-crate discipline, no async runtime in `cortex-rs`).

### Alternatives considered

- **Hand-rolled JSON-RPC over stdio.** Rejected: the MCP protocol is standard; `rmcp` keeps us on it without the maintenance burden.
- **A different MCP framework.** None considered; `rmcp` is the Rust standard.

## [DES-LIMITS] Known Limitations

- **Everything in this zone is provisional.** The server is scaffolded (prints "not yet implemented"); no tools are wired. The safety surface is designed but not yet enforced.
- **Blocked on the client layer (150).** The tools delegate to `QuadCortex`; until 150 lands, the server has nothing to call.
- **No configurable scratch range yet.** The design assumes a configurable default; the config mechanism (env var, config file, MCP tool) is undecided.
- **No backup retention.** The in-memory backup design is session-scoped; cross-session rollback via a backup path is a future option.
- **No `reconnect` tool.** Device-glitch recovery is manual (restart the server) for now.
- **No shared safety module.** The safety rules live in the server for now; a shared module the CLI and GUI can reuse is a future extraction.