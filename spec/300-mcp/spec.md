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

# 300 MCP - Spec (stub)

> The `cortex-mcp` MCP server: an agentic surface over the Quad Cortex (and Nano Cortex) for patch editing. **Greenfield.** No MCP server for any Neural DSP hardware exists. The design that matters is the **safety surface**, not the tool list. This is a stub spec: the server is scaffolded, the safety surface is designed in, the tools are not yet wired.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at (`[Flow.MCP]`).
- [AGENTS.md](../../AGENTS.md) - the MCP safety surface design (the source of truth for the requirements below).
- [Protocol research note](../../quad-cortex-linux-editor-and-protocol.md) - the proposed tool surface and safety rules (at the parent workspace root).
- [100-transport spec](../100-transport/spec.md) [DES-EXCLUSIVE] - the exclusive-HID-access invariant the MCP server must honour (single owning process).
- [150-client spec](../150-client/spec.md) - the `QuadCortex` client API the MCP tools will delegate to.
- [140-session spec](../140-session/spec.md) - the session layer that owns the background RX thread and correlation.
- [house-style rust-cli.md](https://github.com/marcus-pacharanero/house-style/blob/main/rust-cli.md) `--schema` / machine-discoverability section - the schema contract the MCP tool `inputSchema` should match.
- Owned source: `crates/cortex-mcp/src/main.rs`, `crates/cortex-mcp/Cargo.toml`.

## Problem Statement

An MCP server exposes the Quad Cortex to an AI coding agent for patch editing: recall a preset, switch a scene, set a block parameter, save the result. The state is small and structured, the operations are enumerable, and verification is possible (read back what you wrote). This is a genuinely good MCP use case.

The hard part is not the tool list. It is the **safety surface**. An agent with a `save_preset` tool can overwrite a factory preset or clobber a slot the user cared about, and the device will not stop it - a wrong-row edit succeeds silently. The MCP server must be the boundary that refuses the dangerous case, backs up the target before overwriting, and surfaces the traps an agent would not notice.

This zone owns that safety surface and the thin tool wrappers over the client layer (150). The server holds a single `Transport` for its lifetime (one owning process for the USB interface), constructs a `QuadCortex` client over the session layer, and exposes tools gated by the safety classification below.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| The safety surface rules (gated saves, factory-setlist refusal, slot backups, row-numbering trap, single owning process) | Designed (not yet implemented) | AGENTS.md -> MCP safety surface; protocol research note -> The MCP Server |
| The proposed tool surface (read/transient/working-copy/destructive tiers) | Designed (not yet implemented) | Protocol research note -> Proposed tool surface |
| The server is scaffolded (binary exists, prints "not yet implemented") | Implemented | `crates/cortex-mcp/src/main.rs` |
| The `rmcp` crate is the MCP server framework | Provisional | Pinned in `Cargo.toml`; not yet exercised against a real MCP client |

Everything in this zone is provisional until the server is implemented and exercised against a real Quad Cortex from this crate. The safety surface design is hardware-informed (the row-numbering trap, the `read_preset` side effect, the `set_block` DSP-capacity refusal are all observed behaviours) but the MCP server itself is greenfield.

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
| FR-3 | **Restrict saves to a designated scratch range of USER slots unless overridden.** The default scratch range is configurable; an override requires an explicit, logged opt-in. | Must Have |
| FR-4 | **Back up the target slot before overwriting.** `save_preset` calls `read_preset` on the target slot first and retains the blob (in memory or to a configurable backup path) so a bad save can be rolled back. | Must Have |
| FR-5 | **Surface the row-numbering trap in tool descriptions.** Rows are 0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently. Every row-accepting tool (`set_param`, `set_block`, `set_bypass`, `set_chain_input`) notes this in its `inputSchema` description. | Must Have |
| FR-6 | **Single owning process for the USB interface.** The server constructs one `Transport` at startup and holds it for the process lifetime; every tool call reuses it. Opening a transport per tool call is a bug. | Must Have |
| FR-7 | **Surface the `read_preset` side-effect trap in the tool description.** `read_preset` RECALLS the slot (loads it onto the grid, discarding unsaved edits, resetting the active scene). `read_current_preset` does not. The tool descriptions make this distinction explicit. | Must Have |
| FR-8 | **Surface the `set_block` DSP-capacity trap.** A block that exceeds the preset's processing budget is accepted on the wire and silently absent afterwards. The `set_block` tool description and the `verify` parameter (default `true`) surface this. | Must Have |
| FR-9 | **Surface the `set_param(scene=)` 3-message trap.** The scene-following flag and a value cannot travel in the same message. The `set_param` tool description notes that a scene-targeted write issues 3 messages and leaves the unit on the target scene (visible side effect). | Should Have |

#### Tool surface (proposed, not yet implemented)

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-20 | `list_presets(setlist)` - list presets in a setlist. Read, unrestricted. | Must Have |
| FR-21 | `read_preset(setlist, slot)` - recall a slot and return the full `BinaryPreset`. Read, unrestricted (but note the recall side effect, FR-7). | Must Have |
| FR-22 | `list_blocks(preset)` - list the blocks in a preset. Read, unrestricted. | Must Have |
| FR-23 | `get_device_version()` - read the device firmware version. Read, unrestricted. | Must Have |
| FR-24 | `recall_preset(setlist, slot)` - recall a preset (changes what is heard; nothing persistent lost). Write, transient. | Must Have |
| FR-25 | `switch_scene(scene)` - switch the active scene. Write, transient. | Must Have |
| FR-26 | `set_block(row, column, model, verify)` - place a model in a cell. Write, working copy only (edits the recalled preset in device RAM). | Must Have |
| FR-27 | `set_param(row, column, param_index, value, scene, ...)` - set one block parameter. Write, working copy only. | Must Have |
| FR-28 | `set_routing(row, in_portid, out_portid)` - re-point a row's input/output. Write, working copy only. | Must Have |
| FR-29 | `save_preset(setlist, slot, confirm)` - save the working copy to a slot. **Destructive.** Gate this: require explicit slot, refuse FACTORY setlist (FR-2), require confirmation (FR-1), back up target first (FR-4). | Must Have |
| FR-30 | `read_current_preset()` - read the live grid without recalling (no side effect). Read, unrestricted. | Should Have |
| FR-31 | `list_folders()` - list all folders the device knows. Read, unrestricted. | Should Have |

#### Server lifecycle

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-40 | The server constructs a single `Transport` at startup and holds it for the process lifetime. | Must Have |
| FR-41 | The server constructs a `QuadCortex` client over the session layer and reuses it for every tool call. | Must Have |
| FR-42 | The server runs on stdio (the MCP transport) via `rmcp`. | Must Have |
| FR-43 | The server logs safety-relevant events (refused save, slot backup, scratch-range override) to stderr. | Should Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | The server holds exactly one `Transport` for its lifetime; no per-tool-call open. | Review-enforced |
| NFR-2 | A `save_preset` refusal is a structured error, not a silent no-op. The agent sees the refusal and the safe alternative. | Review-enforced |
| NFR-3 | Tool `inputSchema` matches the CLI `--schema` output for shared commands, so humans, scripts, and agents share one contract. | Design-enforced |
| NFR-4 | The server is provisional until exercised against a real Quad Cortex from this crate. Safety-surface behaviour is labelled provisional in docs and release notes. | Docs-enforced |
| NFR-5 | Agent-generated tests must not be the sole basis for accepting safety-surface behaviour. Cross-check against the client layer (150) and a real device. | AGENTS.md assurance |

## Acceptance Criteria

- [ ] `save_preset` refuses a FACTORY setlist slot with a structured error naming the safe alternative.
- [ ] `save_preset` requires an explicit confirmation token; a default-yes does not save.
- [ ] `save_preset` backs up the target slot (`read_preset`) before overwriting and retains the blob.
- [ ] `save_preset` is restricted to a designated scratch range of USER slots unless an explicit override is supplied.
- [ ] Every row-accepting tool (`set_param`, `set_block`, `set_bypass`, `set_chain_input`) notes the row-numbering trap (0-based API, 1-4 on screen) in its `inputSchema` description.
- [ ] `read_preset` tool description notes the recall side effect; `read_current_preset` tool description notes it has no side effect.
- [ ] `set_block` tool description notes the DSP-capacity refusal trap and exposes `verify` (default `true`).
- [ ] The server holds one `Transport` for its lifetime; no per-tool-call open.
- [ ] The server runs on stdio via `rmcp` and answers a real MCP client.
- [ ] All tools delegate to the `QuadCortex` client (zone 150); none reimplement protocol or domain logic.
- [ ] Safety-surface behaviour is labelled provisional in release notes until verified against real hardware.

## Non-Goals

- **Protocol or domain logic.** Owned by the crate (zones 100-150). The MCP server delegates to `QuadCortex`; it does not reimplement.
- **The CLI.** Owned by zone 200. The CLI and MCP server are sibling surfaces over the same crate.
- **The Tauri GUI.** Owned by zone 400 (deferred).
- **On-device / rooting tools.** Out of scope; this project uses the USB HID route exclusively.
- **A second implementation of the safety surface.** The safety rules live in the crate (or a shared safety module the server calls), so the CLI and GUI can reuse them. The server is the first consumer, not the owner.

## Dependencies

- **`cortex-rs`** (workspace path) - the crate whose `QuadCortex` client the tools delegate to.
- **`rmcp`** (workspace) - the MCP server framework (server + transport-io features).
- **`tokio`** (workspace) - the async runtime `rmcp` requires.
- **`anyhow`** - error handling in the binary.
- **`serde_json`** - JSON for tool inputs/outputs.
- **Zone 150 (client)** - the `QuadCortex` API the tools call. This zone is blocked on 150 landing.
- **Zone 140 (session)** - the session layer that owns the background RX thread and correlation. The server holds one session for its lifetime.
- **Zone 100 (transport) [DES-EXCLUSIVE]** - the exclusive-HID-access invariant the server must honour.

## Future

- **Configurable scratch range.** The default USER scratch range for saves should be configurable (env var, config file, or MCP tool) so a user can carve out a safe area for agent experimentation.
- **Backup retention.** The slot-backup blob (FR-4) could be written to a configurable backup directory with a timestamp, so a user can audit and roll back agent saves after the server has exited.
- **Read-back verification.** After a `save_preset`, the server could `read_preset` the same slot and confirm it matches the working copy, surfacing a mismatch as a structured error.
- **Session persistence.** The server could keep the session alive across tool calls (it must - FR-40) and expose a `disconnect`/`reconnect` tool for recovery after a device glitch.
- **Shared safety module.** The safety rules (factory-setlist refusal, scratch range, backup) belong in a shared module the CLI and GUI can reuse, not duplicated in the server.

## Glossary

| Term | Definition |
| --- | --- |
| Safety surface | The set of rules that gate destructive operations (saves) and surface silent-failure traps (row numbering, `read_preset` side effect, `set_block` refusal) |
| Scratch range | A designated set of USER slots where `save_preset` is allowed by default; outside it requires an explicit override |
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
