---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["mcp", "cortex-mcp", "safety-surface", "agentic", "provisional"]
---

# 300 MCP - Spec

> The `cortex-mcp` MCP server is a hardware-verified agentic surface over the Quad Cortex for reading, recall, scene switching and unsaved live-grid editing. Nano Cortex support is deferred until its transport is established, and destructive saving remains deferred.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at (`[Flow.MCP]`).
- [AGENTS.md](../../AGENTS.md) - the MCP safety surface design (the source of truth for the requirements below).
- [Public protocol reference](../../docs/protocol.md) - the wire invariants the tools must preserve.
- [100-transport spec](../100-transport/spec.md) [DES-EXCLUSIVE] - the exclusive-HID-access invariant the MCP server must honour (single owning process).
- [150-client spec](../150-client/spec.md) - the `QuadCortex` client API the MCP tools will delegate to.
- [140-session spec](../140-session/spec.md) - the session layer that owns the background RX thread and correlation.
- [house-style rust-cli.md](https://github.com/marcus-pacharanero/house-style/blob/main/rust-cli.md) `--schema` / machine-discoverability section - the schema contract the MCP tool `inputSchema` should match.
- Owned source: `crates/cortex-mcp/src/main.rs`, `crates/cortex-mcp/Cargo.toml`; shared safety implementation: `crates/cortex-rs/src/safety.rs`.

## Problem Statement

The MCP server exposes structured device reads, recall, scene switching and unsaved live-grid editing to an agent. Persistent save/delete tools are deliberately absent. The implemented surface lets an agent inspect the actual grid, perform typed edits and read the result back without becoming a second HID owner.

The hard part is not the tool list. It is the **safety surface**. An agent with a `save_preset` tool can overwrite a factory preset or clobber a slot the user cared about, and the device will not stop it - a wrong-row edit succeeds silently. The MCP server must be the boundary that refuses the dangerous case, backs up the target before overwriting, and surfaces the traps an agent would not notice.

This zone owns the thin MCP tool wrappers and enforces the shared safety surface. The first milestone reuses the held daemon through a host boundary extracted from `cortex-cli`; it does not open HID itself. This preserves one owning process while allowing CLI, MCP and the future GUI backend to share the same session and typed request contract.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| Shared save policy (explicit target and confirmation, absolute factory refusal, pre-edit preparation/backup, stale-target refusal) | Implemented, fake-session tested, and prepare/edit/commit ordering hardware-verified through the CLI; MCP save remains deliberately absent | `crates/cortex-rs/src/safety.rs`; ENG-006.3 |
| MCP tool descriptions and single owning process | Implemented and hardware-verified | `crates/cortex-mcp/src/server.rs`; 2026-08-06 hardware smoke |
| Read, transient and working-copy tiers | Implemented and hardware-verified, including typed routing and mandatory per-call read-back | `crates/cortex-mcp/src/server.rs`; official-client process test and hardware smokes on 2026-08-06 and 2026-08-11 |
| Destructive tier | Deliberately absent | Core correctness blockers are closed; MCP-specific policy configuration, token lifecycle, restoration semantics, typed errors and hardware smoke remain |
| Bounded official-SDK stdio transport | Implemented and tested | `crates/cortex-mcp/src/transport.rs`; stable `rmcp` 3.1.0 remains affected by upstream #1030 |
| The `rmcp` crate is the MCP server framework | Implemented | Process-tested with the official SDK client and hardware-smoked through that client |

The non-persistent MCP host, including typed routing and mandatory per-call read-back, is hardware-verified against a real Quad Cortex through the official SDK client. Persistent save remains absent until the MCP token registry, exact-target confirmation flow, and its own hardware smoke exist.

## Current Boundary

The non-persistent milestone is complete and hardware-verified through an official SDK client. The installed MCP server starts and lazily restarts an auto-managed sibling daemon when needed; an explicit compatible daemon is reused, typed failures survive both host boundaries, routing uses closed names, and affected working-copy writes require complete live-grid read-back. Destructive save is a later independent milestone.

## User Stories

### Primary Users

AI coding agents editing patches via MCP, and the maintainers who gate what an agent can do.

### Stories

**As an** AI agent
**I want** a `read_preset` tool that returns the full preset structure
**So that** I can inspect a patch before editing it.

**As a** maintainer
**I want** `save_preset` to refuse the FACTORY setlist and require an explicit USER slot
**So that** an agent cannot overwrite a factory preset, ever.

**As a** user
**I want** the MCP server to back up a slot before overwriting it
**So that** if an agent saves a bad patch, I can restore the previous one.

**As an** AI agent
**I want** the row-numbering trap surfaced in the `set_param` tool description
**So that** I do not silently edit the wrong row (rows are 0-based in the API, 1-4 on screen).

**As a** maintainer
**I want** the MCP server to hold a single USB connection for its lifetime
**So that** it does not deadlock against itself or against a concurrently-running CLI.

## Requirements

### Functional Requirements

#### Safety surface (the core of this zone)

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | Read and recall are free; **saving is always explicitly confirmed.** A `save_preset` tool call must include an explicit confirmation token/flag from the caller, not a default-yes. | Must Have |
| FR-2 | **Never write to the factory setlist.** `save_preset` refuses a slot in the FACTORY setlist regardless of the caller's request. Refusal message names the safe alternative (save to a USER slot). | Must Have |
| FR-3 | **Name the exact USER target.** A persistent tool accepts one explicit 1A-32H slot and authorises only that target for the prepared operation. | Must Have |
| FR-4 | **Prepare every target before editing, and retain its backup.** `read_preset` recalls the target, so doing this immediately before save would destroy the unsaved working grid. A listing can be stale even when it reports an empty slot, so preparation always recalls/reads the target before edits begin. The preparation is bound to the physical-session generation, stored-preset mutation epoch, and exact listing entry; reconnects or target changes make it stale. | Must Have |
| FR-5 | **Surface the row-numbering trap in tool descriptions.** Rows are 0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently. Every row-accepting tool (`set_param`, `set_block`, `set_bypass`, `set_chain_input`) notes this in its `inputSchema` description. | Must Have |
| FR-6 | **Single owning process for the USB interface.** The first MCP milestone uses the held daemon through the shared host boundary and never opens HID. Every tool call reuses that owner. | Must Have |
| FR-7 | **Surface the `read_preset` side-effect trap in the tool description.** `read_preset` RECALLS the slot (loads it onto the grid, discarding unsaved edits, resetting the active scene). `read_current_preset` does not. The tool descriptions make this distinction explicit. | Must Have |
| FR-8 | **Surface the `set_block` DSP-capacity trap.** A block that exceeds the preset's processing budget is accepted on the wire and silently absent afterwards. The `set_block` tool description and the `verify` parameter (default `true`) surface this. | Must Have |
| FR-9 | **Surface the `set_param(scene=)` 3-message trap.** The scene-following flag and a value cannot travel in the same message. The `set_param` tool description notes that a scene-targeted write issues 3 messages and leaves the unit on the target scene (visible side effect). | Should Have |

#### Tool surface

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-20 | `list_presets(setlist)` - list presets in a setlist. Read, unrestricted. | Must Have |
| FR-21 | `read_preset(setlist, slot)` - recall a slot and return the full `BinaryPreset`. Read, unrestricted (but note the recall side effect, FR-7). | Must Have |
| FR-22 | `list_blocks(preset)` - list the blocks in a preset. Read, unrestricted. | Must Have |
| FR-23 | `get_device_version()` - read the device firmware version. Read, unrestricted. | Must Have |
| FR-24 | `recall_preset(setlist, slot)` - recall a preset (changes what is heard; nothing persistent lost). Write, transient. | Must Have |
| FR-25 | `switch_scene(scene)` - switch the active scene. Write, transient. | Must Have |
| FR-26 | `set_block(row, column, model, verify)` - place a model in a cell. Write, working copy only (edits the recalled preset in device RAM). | Must Have |
| FR-27 | `set_param(row, column, param_index, value, scene, ...)` sets one block parameter and reports success only after a complete live-grid read confirms the target value and scene. Write, working copy only. | Must Have |
| FR-28 | `set_chain_input` and `set_chain_output` accept closed string enums backed by `GridInputPort` and `GridOutputPort`; raw integers and unknown names are rejected before daemon dispatch. `set_split` remains separate. All three report success only after complete live-grid read-back confirms the requested state. | Must Have |
| FR-29 | `save_preset(setlist, slot, preparation, confirm)` - save the working copy to a slot. **Destructive and deferred beyond the first MCP milestone.** Gate this: require explicit slot, refuse FACTORY (FR-2), require confirmation (FR-1), and require a matching pre-edit preparation (FR-4). | Must Have |
| FR-30 | `read_current_preset()` - read the live grid without recalling (no side effect). Read, unrestricted. | Should Have |
| FR-31 | `list_folders()` - list all folders the device knows. Read, unrestricted. | Should Have |
| FR-32 | `set_scene_label(scene, label)` and `unlabel_scene(scene)` edit scene metadata on the unsaved working copy. | Must Have |
| FR-33 | `set_scene_color(scene, color)` writes an ARGB `uint32` to the unsaved working copy. | Must Have |
| FR-34 | `copy_scene(from_scene, to_scene)` copies parameter, bypass, label and colour state, then refreshes the live preset before returning. | Must Have |
| FR-35 | `swap_scenes(first_scene, second_scene)` exchanges complete scene state and refreshes the live preset before returning. | Must Have |

#### Server lifecycle

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-40 | The server connects once to the reusable host/daemon client and never opens HID directly. | Must Have |
| FR-41 | Every tool delegates through the typed daemon request contract to the sole held `QuadCortex` session. | Must Have |
| FR-42 | The server runs on stdio (the MCP transport) via `rmcp`. | Must Have |
| FR-43 | The server logs safety-relevant events (refused save, prepared target, and slot backup) to stderr. | Should Have |
| FR-44 | When no daemon endpoint exists, the installed MCP server starts the sibling `cortex session` owner before serving stdio. It never silently replaces a running incompatible daemon, and concurrent starts converge on the local IPC claim. | Should Have |
| FR-45 | Daemon failures retain a stable machine-readable code across local IPC and MCP structured tool results while preserving a human-readable diagnostic. Model-correctable failures and retryable session states never require message parsing. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | The MCP process opens zero HID transports; the held daemon remains the sole owner. | Review-enforced |
| NFR-2 | A `save_preset` refusal is a structured error, not a silent no-op. The agent sees the refusal and the safe alternative. | Review-enforced |
| NFR-3 | Tool schemas are explicit and bounded today. They converge with a future shared CLI/MCP registry under CLI-007.1; no CLI `--schema` contract exists yet. | Design-enforced |
| NFR-4 | The non-persistent server is hardware-verified per operation. A changed host contract remains provisional until its own hardware smoke, even when the underlying wire operation was already verified. | Docs-enforced |
| NFR-5 | Agent-generated tests must not be the sole basis for accepting safety-surface behaviour. Cross-check against the client layer (150) and a real device. | AGENTS.md assurance |
| NFR-6 | Tool execution failures use MCP's model-correctable structured-error path with top-level `code` and `error` fields; transport and server faults remain JSON-RPC errors. | Protocol-enforced |

## Acceptance Criteria

- [ ] `save_preset` refuses a FACTORY setlist slot with a structured error naming the safe alternative.
- [ ] `save_preset` requires an explicit confirmation token; a default-yes does not save.
- [ ] An occupied save target is read and retained before working-copy edits begin; attempting to create that backup after edits is refused because recall would discard them.
- [ ] Every target is recalled/read before edits begin even when its listing says empty; the retained preparation is bound to that exact setlist and slot.
- [ ] A reconnect, intervening stored-preset mutation, or changed target listing makes the preparation stale and refuses the save.
- [ ] `save_preset` names and authorises one exact USER target; its schema exposes no range or broad override.
- [x] Every row-accepting tool notes the row-numbering trap (0-based API, 1-4 on screen) in its description and row schema.
- [x] `read_preset` notes the recall side effect; `read_current_preset` notes it has no side effect.
- [x] `set_block` notes the DSP-capacity refusal trap and exposes `verify` (default `true`).
- [x] The server uses the held daemon and opens no HID transport itself.
- [x] The server runs on bounded stdio via `rmcp` and answers a real official-SDK MCP client.
- [x] All tools delegate through the daemon to `QuadCortex`; none reimplement protocol or domain logic.
- [x] The non-persistent tool surface passed hardware smoke on 2026-08-06 against CorOS 4.0.1.
- [x] A normally installed `cortex-mcp` resolves the sibling `cortex`, starts an auto-managed owner without a separate manual session, and writes no non-MCP data to stdout. Process tests cover two concurrent missing-daemon launches converging on one endpoint and a long-lived MCP process restarting after request-idle release.
- [x] Typed daemon error codes survive the process boundary and appear in MCP structured tool errors. Tests cover leaf-error classification, a downcastable host error from a real daemon process, daemon reconnect/validation categories, and an official-SDK MCP call receiving `dsp_refused`.
- [x] Routing schemas expose closed string enums, require only `row` and `port`, reject numeric/unknown values, and preserve typed routes through MCP JSON, daemon protocol v11 and core conversion.
- [x] Parameter, bypass, removal, routing, and split success requires a complete matching live-grid read; mismatches become `GridWriteUnconfirmed` and daemon `outcome_unconfirmed`.
- [x] The updated official-client hardware smoke passed typed routing and mandatory per-call read-back against the real daemon/core path on CorOS 4.0.1 on 2026-08-11, with USER `6A` restored by recall.

## Non-Goals

- **Protocol or domain logic.** Owned by the crate (zones 100-150). The MCP server delegates to `QuadCortex`; it does not reimplement.
- **The CLI.** Owned by zone 200. The CLI and MCP server are sibling surfaces over the same crate.
- **The Tauri GUI.** Owned by zone 400; it is a sibling consumer of the same daemon/core contract.
- **On-device / rooting tools.** Out of scope; this project uses the USB HID route exclusively.
- **A second implementation of the safety surface.** The safety rules live in the crate (or a shared safety module the server calls), so the CLI and GUI can reuse them. The server is the first consumer, not the owner.

## Dependencies

- **`cortex-rs`** (workspace path) - the crate whose `QuadCortex` client the tools delegate to.
- **`cortex-host`** - extracted typed daemon requests and lifecycle access with platform IPC behind one facade.
- **`rmcp`** (workspace) - the MCP server framework (server + transport-io features).
- **`tokio`** (workspace) - the async runtime `rmcp` requires.
- **`anyhow`** - error handling in the binary.
- **`serde_json`** - JSON for tool inputs/outputs.
- **Zone 150 (client)** - the implemented `QuadCortex` API the tools call; wider device operations remain incremental.
- **Zone 140 (session)** - the session layer that owns the background RX thread and correlation. The server holds one session for its lifetime.
- **Zone 100 (transport) [DES-EXCLUSIVE]** - the exclusive-HID-access invariant the server must honour.

## Future

- **Scratch-range configuration.** Choose the host mechanism (GUI setting, config file, or MCP startup argument). `SavePolicy` validates supplied ranges but deliberately provides no default because the crate cannot know which slots are disposable.
- **Backup retention and restoration.** The slot-backup blob (FR-4) could be written to a configurable backup directory with a timestamp. Restoring it still needs a verified device-side copy, import, or keyed replay path because an unkeyed whole-preset grid write is ignored; retention alone must not be presented as one-click rollback.
- **Read-back verification.** After a `save_preset`, the server could `read_preset` the same slot and confirm it matches the working copy, surfacing a mismatch as a structured error.
- **Prepared-token registry.** MCP calls cannot carry the opaque Rust `SavePreparation` directly. The server must retain preparations under short-lived opaque IDs and expose only `SavePreparationView`; raw backups never cross into tool arguments.

## Glossary

| Term | Definition |
| --- | --- |
| Safety surface | The set of rules that gate destructive operations (saves) and surface silent-failure traps (row numbering, `read_preset` side effect, `set_block` refusal) |
| Scratch range | A host-configured set of USER slots where `save_preset` is allowed by default; outside it requires an explicit override |
| Factory setlist | The read-only set of presets shipped with the device; `save_preset` refuses it |
| Row-numbering trap | Rows are 0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently |
| `read_preset` side effect | `read_preset` RECALLS the slot (loads it onto the grid, discarding unsaved edits); `read_current_preset` does not |
| `set_block` DSP-capacity trap | A block that exceeds the preset's processing budget is accepted on the wire and silently absent afterwards |
| Transient write | Changes what is heard (recall, scene switch); nothing persistent is lost |
| Working-copy write | Edits the recalled preset in device RAM (set_block, set_param, set_routing); persists only on save |
| Destructive write | Overwrites a slot (save_preset); gated by the safety surface |
| Provisional | Not yet verified against real hardware by this project; may work but is not confirmed |
## Future: audio feedback (FUTURE-007)

An agent editing a patch through this server can already establish **structural** ground truth by read-back: `read_current_preset` returns the actual grid, so "did my edit land on the intended block" is answerable today, and the row-numbering trap is detectable rather than silent.

What read-back cannot answer is whether the result **sounds** any good. That is the gap [FUTURE-007](../roadmap.md) proposes to close by playing a standardised stimulus through the unit's USB audio interfaces and analysing the result.

Two points matter for this zone's safety surface:

1. **It is not a safety mechanism and must not be relied on as one.** Audio analysis yields inference; the grid read yields fact. A save must continue to be gated on explicit confirmation and verified by read-back, never by "it sounded fine".
2. **It does not contend for the HID connection.** Confirmed on hardware: interfaces 0-4 of the device are USB Audio class and only interface 5 is HID. An audio capture process and this server can therefore run concurrently without violating the single-owning-process rule for the HID interface.
