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

# 300 MCP - Design

> Design for the `cortex-mcp` MCP server. The interesting part is not the tool list - it is the **safety surface** that gates destructive operations and surfaces the silent-failure traps an agent would not notice. The tool model is a thin wrapper over the client layer (150).

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [AGENTS.md](../../AGENTS.md) - the MCP safety surface design (the source of truth).
- [Public protocol reference](../../docs/protocol.md) - the wire invariants the tool surface must preserve.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at (`[Flow.MCP]`).
- [100-transport design](../100-transport/design.md) [DES-EXCLUSIVE] - the exclusive-HID-access invariant.
- [150-client design](../150-client/design.md) - the `QuadCortex` API the tools delegate to.
- Owned source: `crates/cortex-mcp/src/{main,server,transport}.rs`, process tests; shared host and safety implementations: `crates/cortex-host/src/`, `crates/cortex-rs/src/safety.rs`.

## [DES-SAFETY] The Safety Surface

### Behaviour

The safety surface is the set of rules that gate destructive operations and surface silent-failure traps. It is the core of this zone. An agent with a `save_preset` tool can overwrite a factory preset or clobber a slot the user cared about, and the device will not stop it. The MCP server must be the boundary that refuses the dangerous case.

The five invariants (from AGENTS.md and the protocol research note):

1. **Read and recall are free; saving is always explicitly confirmed.**
2. **Never write to the factory setlist; require one explicitly named USER target.**
3. **Prepare every target before editing, and retain any backup.** Listings cannot prove emptiness because file state is eventually consistent; a target first chosen after unsaved edits exist cannot be prepared without losing those edits.
4. **Surface the row-numbering trap (0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently) in tool descriptions.**
5. **Single owning process for the USB interface.**

### Design choice: classify tools by destructive tier

The proposed tool surface has four tiers, each with a different safety posture:

| Tier | Tools | Safety posture |
| --- | --- | --- |
| Read | `list_presets`, `read_preset`, `list_blocks`, `get_device_version`, `read_current_preset`, `list_folders` | Unrestricted. (But note: `read_preset` has a recall side effect - surface it in the description.) |
| Transient write | `recall_preset`, `switch_scene` | Free. Changes what is heard; nothing persistent is lost. |
| Working-copy scene write | `set_scene_label`, `unlabel_scene`, `set_scene_color`, `copy_scene`, `swap_scenes` | Free. Changes device RAM only; copy/swap move labels and colours with sound state and force a live-preset refresh. |
| Working-copy write | `set_block`, `set_param`, `set_bypass`, `remove_block`, `set_chain_input`, `set_chain_output`, `set_split` | Free. Edits device RAM; persists only on save. Routing uses closed typed names; parameter, bypass, removal, routing, and split require complete live-grid read-back before success. |
| Destructive | `save_preset` | **Gated.** Require explicit slot, refuse FACTORY, require confirmation, and consume a matching pre-edit backup or empty-target preparation. |

This tiering is the design that matters. The tool list is a thin wrapper over the client layer; the tiering is what stops an agent from destroying a patch.

### Design choice: confirmation token, not default-yes

`save_preset` requires an explicit confirmation token/flag from the caller. A default-yes (the agent calls `save_preset` and it saves unless `confirm=false`) is unsafe: an agent that calls every tool with defaults will destroy a slot. The confirmation must be an active opt-in: the agent must set `confirm: true` (or supply a token from a prior `read_preset` / `propose_save` call).

### Design choice: refuse FACTORY, always

`save_preset` refuses a slot in the FACTORY setlist regardless of the caller's request. The refusal is a structured error naming the safe alternative ("save to a USER slot"). There is no override for the factory setlist.

### Design choice: exact target, not a range

Each preparation names one USER slot within the unit's actual 1A-32H range. The host authorises only that slot for the resulting opaque token; neither the MCP schema nor the daemon request accepts a range or broad override. This keeps the agent's authority equal to the destination the user reviewed.

### Design choice: backup before overwrite

The old wording called `read_preset` immediately before the overwrite. That cannot work: `read_preset` recalls the target, replacing the unsaved working grid that `save_current_preset` was about to commit. The ordering is load-bearing.

The safe workflow has a preparation phase before editing starts:

1. List the target setlist and bind the preparation to its exact setlist, slot, listing entry, physical-session generation, and stored-preset mutation epoch.
2. Recall/read the target now, while replacing the working grid is still acceptable, and retain the returned `BinaryPreset`. Do this even when the listing says empty because listings are eventually consistent and cannot prove emptiness.
3. A host that needs immediately restorable backups may additionally copy the retained preset to a configured empty backup slot once that restoration path is verified.
4. Recall/build the intended source and perform working-copy edits.
5. `save_preset` re-lists the target, rejects any reconnect, stored-preset mutation, invalidated stream, or changed listing entry, then consumes the matching preparation plus an explicit confirmation. A stale preparation, a preparation for another slot, or no preparation for an occupied target is refused.

If the user chooses an occupied target only after the grid is dirty, the safe choices are to select and prepare another target or abandon/replay the edits after preparing the original target. Silently recalling it to make a backup is not a safety feature; it loses the work being saved.

### Design choice: surface traps in tool descriptions

The traps are not errors - the device does not refuse a wrong-row edit or a too-big block. They are silent failures an agent would not notice. The tool descriptions (`inputSchema`) surface them:

- **Row-numbering trap**: every row-accepting tool notes "rows are 0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently."
- **`read_preset` side effect**: `read_preset` notes "RECALLS the slot (loads onto the grid, discards unsaved edits, resets active scene); use `read_current_preset` for inspection during editing."
- **`set_block` DSP refusal**: `set_block` notes "a block that exceeds the preset's processing budget is accepted on the wire and silently absent; `verify=true` (default) catches this."
- **`set_param(scene=)` 3-message rule**: `set_param` notes "a scene-targeted write issues 3 messages (promote scene_mode, switch scene, write) and leaves the unit on the target scene (visible side effect)."
- **Routing-id trap**: the device stores meaningless output integers and reads them back cleanly, so MCP exposes only closed input/output string enums.
- **Dispatch is not confirmation**: a subscribed cache can still show the old state immediately after a write, so affected tools explicitly read the complete live grid before reporting success.

### Alternatives considered

- **Gate every write, not just saves.** Rejected: recall and scene-switch are transient (nothing persistent lost). Gating them would make the server unusable for exploration. Working-copy writes (`set_block`, `set_param`, `set_routing`) edit device RAM but do not persist; they are free, with traps surfaced.
- **Allow FACTORY saves with a stronger confirmation.** Rejected: a factory preset is never the right target for an agent experiment. The refusal is absolute, not a higher bar.
- **Describe retained bytes as rollback.** Rejected: an unkeyed whole-preset write is ignored. Retention supports audit and a future verified restoration path, not automatic rollback today.
- **Trap-surfacing as runtime warnings, not descriptions.** Rejected: the agent does not read warnings it does not ask for. The description is what an agent reads to decide how to call the tool.

## [DES-OWNER] Single Owning Process

### Behaviour

The MCP process opens no `Transport`. It connects to the held `cortex session` daemon through the reusable host boundary. Every tool call uses that typed request client, so the daemon remains the one process holding HID. A missing socket makes the MCP process locate the installed sibling `cortex` binary and invoke the auto-managed `cortex session start` contract before opening stdio. An existing incompatible daemon remains an explicit refusal rather than being killed behind another user's back.

### Design choice: reuse the daemon, do not become another owner

`cortex-host` owns the extracted host-facing request boundary; `cortex-rs` remains free of host IPC and async-runtime dependencies. The MCP process constructs one supervised host client at startup and serves stdio through `rmcp`. The supervisor probes status without bypassing the daemon protocol-version gate, starts only when no owner exists, and rechecks before every tool so a long-lived MCP process recovers after request-idle release.

### Design choice: agent-triggered daemon lifecycle, not systemd

The installed stdio lifecycle starts a missing daemon on demand. `cortex-mcp` resolves `cortex` beside its own executable, invokes `cortex session start --auto-managed --idle-timeout-seconds 60`, and waits for a compatible status response before serving or dispatching a tool. Concurrent MCP processes may both reach the start command, but only one daemon wins the endpoint claim; losing callers accept that race only after the winner serves the expected protocol. Every completed request resets the idle timeout, in-flight requests prevent exit, and a later tool call restarts an owner that has already released the device. This avoids a systemd user-service dependency and does not tie ownership to one MCP process.

### Design choice: no per-tool-call reconnect

The shared `cortex session` owner reconnects with a surfaced health state, generation invalidation, and bounded backoff. MCP uses that contract rather than implementing a second manager. Tool calls made while replacement is in progress receive the reconnect attempt and last error rather than racing another HID owner.

### Alternatives considered

- **MCP owns a second long-lived transport.** Rejected for the first milestone: it prevents CLI and GUI sharing and duplicates the reconnect manager.
- **Pool or reopen per tool call.** Rejected: the HID interface does not multiplex, and a second open wedges the existing owner.
- **Rely on `hidapi` open to enforce ownership.** Rejected by hardware: a second process opens without error and wedges the held session on its next request. The owner claims its socket before opening the interface and every other surface routes through it or refuses.

## [DES-TOOLS] Tool Model

### Behaviour

Each tool is a thin wrapper over a `QuadCortex` method. The tool parses its input (validated against `inputSchema`), calls the client method, and returns the result as JSON. The safety surface (above) gates the destructive tier.

The implemented first milestone deliberately omits the destructive tier. Read, transient and working-copy tools are sufficient to research and build a preset in the live grid, and a recall reverses the experiment. `save_preset` appears only after MCP-specific policy, token lifecycle, restoration semantics and hardware smoke are complete.

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
| `get_status`, `get_active_scene`, `get_cpu_load`, `search_catalog` | daemon/cache/client reads | Read |
| `recall_preset` | `QuadCortex::recall_preset` | Transient write |
| `switch_scene` | `QuadCortex::switch_scene` | Transient write |
| `set_scene_label`, `unlabel_scene`, `set_scene_color` | typed daemon scene requests | Working-copy write |
| `copy_scene`, `swap_scenes` | `QuadCortex::copy_scene` plus full live read-back | Working-copy write |
| `set_block` | `QuadCortex::set_block` | Working-copy write |
| `set_param`, `set_bypass`, `remove_block` | corresponding read-back-verified `QuadCortex` methods | Working-copy write |
| `set_chain_input`, `set_chain_output`, `set_split` | typed, read-back-verified `QuadCortex` methods | Working-copy write |
| `save_preset` | `QuadCortex::prepare_save_before_editing` + `QuadCortex::save_prepared` | Destructive |

### Design choice: `inputSchema` matches CLI `--schema`

Tool schemas are explicit, bounded JSON Schemas maintained in the MCP registry. Routing schemas derive their closed string values from `GridInputPort::ALL` and `GridOutputPort::ALL`. CLI `--schema` does not exist yet; CLI-007.1 will introduce one typed registry and migrate both surfaces to it.

### Design choice: `save_preset` is a distinct tool, not a flag

`save_preset` is a separate tool, not a `confirm=true` flag on `set_param` or `set_block`. This makes the destructive tier visible in the tool list: an agent enumerating tools sees `save_preset` as a distinct, gated operation, not an option on a safer tool.

### Alternatives considered

- **One generic `edit_preset` tool.** Rejected: the surface must be enumerable and the tiering visible. A generic tool hides the safety posture from an agent reading the tool list.
- **Combine `set_block`/`set_param`/`set_routing` into one `edit_grid` tool.** Rejected for the same reason: the traps are per-operation (row numbering, DSP refusal, 3-message rule), and surfacing them in a combined tool's description buries them.

## [DES-FRAMEWORK] rmcp

### Behaviour

The server uses the `rmcp` crate on a `tokio` runtime. Stable `rmcp` 3.1.0 still buffers an unterminated stdio line without a finite read limit, so `BoundedStdioTransport` supplies a 16 MiB line cap and limits aggregate work to eight in-flight requests until each response has been transmitted. The binary constructs the reusable daemon host client and `CortexMcp`, then serves that bounded transport through the official SDK.

### Design choice: rmcp over a hand-rolled JSON-RPC server

`rmcp` is the selected Rust MCP server framework. Using it avoids hand-rolling the JSON-RPC transport and keeps the server on the standard MCP protocol. The workspace pins `rmcp` with `default-features = false` and only the required server/transport features.

### Design choice: tokio runtime

`rmcp` requires an async runtime. `tokio` (macros, net, rt-multi-thread) is the workspace standard. The server is the only surface in this project that needs async; the crate stays sync (leaf-crate discipline, no async runtime in `cortex-rs`).

### Alternatives considered

- **Hand-rolled JSON-RPC over stdio.** Rejected: the MCP protocol is standard; `rmcp` keeps us on it without the maintenance burden.
- **A different MCP framework.** None selected; `rmcp` currently meets the protocol and integration needs.

## [DES-LIMITS] Known Limitations

- **The non-persistent slice is hardware-verified.** Read, recall, scene and unsaved live-grid tools passed an official-client hardware smoke against CorOS 4.0.1 on 2026-08-06. Persistent writes remain absent.
- **The client, persistent-session foundations and non-persistent tool wiring exist.** Wider operations and destructive tools remain incremental.
- **No destructive MCP token registry yet.** The daemon can retain an exact-slot preparation, but the MCP process still needs a proposal/confirmation flow and its own hardware smoke before exposing save.
- **Prepared-save token and backup retention are implemented in the shared crate.** The MCP server still needs an opaque token registry so it can retain `SavePreparation` between tool calls without serialising raw backups.
- **Automatic restoration is not implemented.** The retained `BinaryPreset` can be persisted, but the device ignores an unkeyed whole-preset grid write. Restoration needs a separately verified device-side copy, import, or keyed replay path; hosts must not present the retained blob as one-click rollback yet.
- **The reusable host boundary is extracted.** `cortex-host` owns the typed daemon protocol and synchronous short-lived local IPC client. It has no HID feature; CLI and MCP share it without putting host IPC into the leaf crate. Platform details stay behind `LocalEndpoint`, `LocalListener` and `LocalConnection`.
- **Daemon failures remain typed across both boundaries.** Protocol v11 carries a `DaemonErrorCode` beside every human diagnostic and serialises typed routing names in `SetRouting`. `cortex-host` turns error envelopes into a downcastable `DaemonError`; MCP converts them to structured tool content with top-level `code` and `error` fields. The mapping matches `cortex_rs::Error` variants and explicit daemon lifecycle state, never display strings. JSON-RPC errors remain reserved for malformed MCP traffic and server failures rather than model-correctable tool outcomes.
- **Destructive MCP save is intentionally deferred.** The former core PROT-009 blockers and typed daemon failures are closed; MCP still needs configured policy, opaque token lifecycle, restoration semantics and its own hardware smoke.
