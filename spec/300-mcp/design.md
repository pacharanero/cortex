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
- [Public protocol reference](../../docs/protocol.md) - the wire invariants the tool surface must preserve.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at (`[Flow.MCP]`).
- [100-transport design](../100-transport/design.md) [DES-EXCLUSIVE] - the exclusive-HID-access invariant.
- [150-client design](../150-client/design.md) - the `QuadCortex` API the tools delegate to.
- Owned source: `crates/cortex-mcp/src/main.rs`; shared safety implementation: `crates/cortex-rs/src/safety.rs`.

## [DES-SAFETY] The Safety Surface

### Behaviour

The safety surface is the set of rules that gate destructive operations and surface silent-failure traps. It is the core of this zone. An agent with a `save_preset` tool can overwrite a factory preset or clobber a slot the user cared about, and the device will not stop it. The MCP server must be the boundary that refuses the dangerous case.

The five invariants (from AGENTS.md and the protocol research note):

1. **Read and recall are free; saving is always explicitly confirmed.**
2. **Never write to the factory setlist; restrict saves to a designated scratch range of USER slots unless overridden.**
3. **Prepare and back up an occupied target before editing, and keep the blob.** A target first chosen after unsaved edits exist must be empty, because reading an occupied target recalls it and discards those edits.
4. **Surface the row-numbering trap (0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently) in tool descriptions.**
5. **Single owning process for the USB interface.**

### Design choice: classify tools by destructive tier

The proposed tool surface has four tiers, each with a different safety posture:

| Tier | Tools | Safety posture |
| --- | --- | --- |
| Read | `list_presets`, `read_preset`, `list_blocks`, `get_device_version`, `read_current_preset`, `list_folders` | Unrestricted. (But note: `read_preset` has a recall side effect - surface it in the description.) |
| Transient write | `recall_preset`, `switch_scene` | Free. Changes what is heard; nothing persistent is lost. |
| Working-copy write | `set_block`, `set_param`, `set_routing` | Free. Edits the recalled preset in device RAM; persists only on save. Surface the traps (`set_block` DSP refusal, `set_param(scene=)` 3-message rule, row numbering). |
| Destructive | `save_preset` | **Gated.** Require explicit slot, refuse FACTORY, require confirmation, and consume a matching pre-edit backup or empty-target preparation. |

This tiering is the design that matters. The tool list is a thin wrapper over the client layer; the tiering is what stops an agent from destroying a patch.

### Design choice: confirmation token, not default-yes

`save_preset` requires an explicit confirmation token/flag from the caller. A default-yes (the agent calls `save_preset` and it saves unless `confirm=false`) is unsafe: an agent that calls every tool with defaults will destroy a slot. The confirmation must be an active opt-in: the agent must set `confirm: true` (or supply a token from a prior `read_preset` / `propose_save` call).

### Design choice: refuse FACTORY, always

`save_preset` refuses a slot in the FACTORY setlist regardless of the caller's request. The refusal is a structured error naming the safe alternative ("save to a USER slot"). There is no override for the factory setlist; the scratch-range override (next) applies only to USER slots.

### Design choice: scratch range with explicit override

The scratch range is a host-configured set of valid USER slots within the unit's actual 1A-32H range. The shared library deliberately has no built-in default: only the user knows which slots are disposable. Saving outside the configured range requires an explicit, logged override.

### Design choice: backup before overwrite

The old wording called `read_preset` immediately before the overwrite. That cannot work: `read_preset` recalls the target, replacing the unsaved working grid that `save_current_preset` was about to commit. The ordering is load-bearing.

The safe workflow has a preparation phase before editing starts:

1. List the target setlist and bind the preparation to its exact setlist, slot, listing entry, physical-session generation, and stored-preset mutation epoch.
2. If the target is empty, record an empty-target preparation without recalling anything.
3. If occupied, read it now, while replacing the working grid is still acceptable, and retain the returned `BinaryPreset`. A host that needs immediately restorable backups may additionally copy it to a configured empty backup slot.
4. Recall/build the intended source and perform working-copy edits.
5. `save_preset` re-lists the target, rejects any reconnect, stored-preset mutation, invalidated stream, or changed listing entry, then consumes the matching preparation plus an explicit confirmation. A stale preparation, a preparation for another slot, or no preparation for an occupied target is refused.

If the user chooses an occupied target only after the grid is dirty, the safe choices are to select an empty scratch slot or abandon/replay the edits after preparing that target. Silently recalling it to make a backup is not a safety feature; it loses the work being saved.

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

The shared `cortex session` owner now reconnects with a surfaced health state, generation invalidation, and bounded backoff. An MCP server may use that daemon contract or apply the same manager around its one owned session; it must not reopen per tool call. Tool calls made while replacement is in progress receive the reconnect attempt and last error rather than racing another HID owner.

### Alternatives considered

- **Pool of transports.** Rejected: the HID interface does not multiplex. A pool of one is a held transport.
- **Reopen per tool call.** Rejected: deadlocks against a concurrently-running CLI or against the server's own prior tool calls.
- **Rely on `hidapi` open to enforce ownership.** Rejected by hardware: a second process opens without error and wedges the held session on its next request. The owner claims its socket before opening the interface and every other surface routes through it or refuses.

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
| `save_preset` | `QuadCortex::prepare_save_before_editing` + `QuadCortex::save_prepared` | Destructive |

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
- **The client and persistent-session foundations now exist.** Tool wiring remains unimplemented, but it is no longer blocked on zone 150.
- **No scratch-range host configuration mechanism yet.** `SavePolicy` validates host-supplied 1A-32H ranges without guessing a default; the MCP process still needs to obtain that policy from the user.
- **Prepared-save token and backup retention are implemented in the shared crate.** The MCP server still needs an opaque token registry so it can retain `SavePreparation` between tool calls without serialising raw backups.
- **Automatic restoration is not implemented.** The retained `BinaryPreset` can be persisted, but the device ignores an unkeyed whole-preset grid write. Restoration needs a separately verified device-side copy, import, or keyed replay path; hosts must not present the retained blob as one-click rollback yet.
- **Reconnect policy for the MCP host is not selected.** The CLI daemon now has the correct generation-invalidating manager; the MCP process can reuse its socket contract or own one equivalent session, never both.
- **Safety hardware smoke remains outstanding.** Fake-session tests pin empty-target, occupied-target, explicit-discard, confirmation, stale-generation, mutation-epoch, and changed-entry behaviour, but the save reason and listing refresh must still be observed on the unit.
