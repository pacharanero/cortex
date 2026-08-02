---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["mcp", "tasks", "roadmap", "provisional"]
spec: spec.md
design: design.md
---

# 300 MCP - Tasks (stub)

> Implementation tasks for the `cortex-mcp` MCP server. Phase 0 (scaffold) is done; everything else is planned and blocked on the client layer (zone 150). The safety surface is designed but not yet enforced.

## Phase 0 - Scaffold (done)

### 0.1 Binary scaffold

- [x] Create `crates/cortex-mcp/` with `[[bin]] name = "cortex-mcp"`.
- [x] Depend on `cortex-rs` (workspace path), `rmcp` (server + transport-io), `tokio` (macros, net, rt-multi-thread), `anyhow`, `serde_json`.
- [x] `main.rs` prints "not yet implemented" and points at AGENTS.md -> MCP safety surface.

### 0.2 Spec tree

- [x] Write spec/design/tasks for this zone (this file).

## Phase 1 - Safety surface (planned, next, blocked on 150)

### 1.1 Confirmation gate

- [ ] Define a confirmation mechanism for `save_preset` (explicit `confirm: true` flag, or a token from a prior `propose_save` call).
- [ ] Reject `save_preset` calls without the confirmation; return a structured error.
- [ ] Log every confirmation to stderr.

### 1.2 Factory-setlist refusal

- [ ] Detect FACTORY setlist in `save_preset` and refuse with a structured error naming the safe alternative.
- [ ] No override for the factory setlist; the refusal is absolute.

### 1.3 Scratch range

- [ ] Define a default scratch range of USER slots (e.g. 900-999).
- [ ] Reject `save_preset` outside the scratch range unless an explicit override is supplied.
- [ ] Log every override to stderr.

### 1.4 Slot backup

- [ ] Before overwriting, call `read_preset` on the target slot and retain the blob (in memory for the session).
- [ ] (Future) Optionally write the blob to a configurable backup path for cross-session rollback.
- [ ] Log every backup to stderr (slot, timestamp, blob size).

### 1.5 Trap-surfacing in tool descriptions

- [ ] Row-numbering trap: every row-accepting tool (`set_param`, `set_block`, `set_bypass`, `set_chain_input`) notes "rows are 0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently" in `inputSchema` description.
- [ ] `read_preset` side effect: `read_preset` description notes the recall side effect; `read_current_preset` notes no side effect.
- [ ] `set_block` DSP refusal: `set_block` description notes the silent-absence trap; `verify` defaults to `true`.
- [ ] `set_param(scene=)` 3-message rule: `set_param` description notes the 3-message rule and the visible side effect (leaves unit on target scene).

## Phase 2 - Server lifecycle (planned, blocked on 140 + 150)

### 2.1 Single owning process

- [ ] `main()` constructs one `Transport::open(DeviceKind::QuadCortex)` at startup.
- [ ] Construct `Session` over the transport; construct `QuadCortex` over the session.
- [ ] Hold the `QuadCortex` for the process lifetime; every tool call borrows it.
- [ ] No per-tool-call open, ever.

### 2.2 rmcp server on stdio

- [ ] Wrap the `QuadCortex` in a `CortexMcpServer` struct implementing the `rmcp` server trait.
- [ ] `main()` hands the server to `rmcp::serve_stdio` on a `tokio` runtime.
- [ ] Test: a real MCP client can list tools and call a read tool.

### 2.3 Device-glitch recovery

- [ ] Surface a read timeout (dead device) as a structured error to the agent.
- [ ] (Future) Offer a `reconnect` tool for manual recovery; no silent reconnect.

## Phase 3 - Read tools (planned, blocked on 150)

### 3.1 `list_presets(setlist)`

- [ ] Delegate to `QuadCortex::list_presets(setlist, timeout, include_empty)`.
- [ ] Return a JSON array of preset entries (slot, name, empty flag).

### 3.2 `read_preset(setlist, slot)`

- [ ] Delegate to `QuadCortex::read_preset(setlist_path, position, is_factory, timeout)`.
- [ ] Return the full `BinaryPreset` as JSON.
- [ ] Description notes the recall side effect (FR-7).

### 3.3 `read_current_preset()`

- [ ] Delegate to `QuadCortex::read_current_preset(timeout)`.
- [ ] Return the live grid as JSON; no side effect.

### 3.4 `list_blocks(preset)`

- [ ] Derived from `read_current_preset`; list the blocks in the live grid.
- [ ] Return a JSON array of block entries (row, column, model, bypassed).

### 3.5 `list_folders()`

- [ ] Delegate to `QuadCortex::list_folders(seconds)`.
- [ ] Return a JSON array of folder paths.

### 3.6 `get_device_version()`

- [ ] Delegate to `QuadCortex::version(timeout)`.
- [ ] Return the `VersionMessage` fields as JSON (same struct as `cortex version --format json`).

## Phase 4 - Transient write tools (planned, blocked on 150)

### 4.1 `recall_preset(setlist, slot)`

- [ ] Delegate to `QuadCortex::recall_preset(setlist_path, position, is_factory, request_id)`.
- [ ] Return confirmation (setlist, slot, preset name).

### 4.2 `switch_scene(scene)`

- [ ] Delegate to `QuadCortex::switch_scene(scene)`.
- [ ] Return the active scene.
- [ ] Description notes scenes are 0-based in the API, 1-4 on screen.

## Phase 5 - Working-copy write tools (planned, blocked on 150)

### 5.1 `set_block(row, column, model, verify)`

- [ ] Delegate to `QuadCortex::set_block(row, column, model, verify, timeout)`.
- [ ] `verify` defaults to `true`; on `BlockRefused`, return a structured error.
- [ ] Description notes the DSP-capacity silent-absence trap.

### 5.2 `set_param(row, column, param_index, value, scene, ...)`

- [ ] Delegate to `QuadCortex::set_param(...)`.
- [ ] Description notes the 3-message rule for `scene=` and the visible side effect.
- [ ] Description notes the row-numbering trap.

### 5.3 `set_routing(row, in_portid, out_portid)`

- [ ] Delegate to `QuadCortex::set_chain_input` / `set_chain_output`.
- [ ] Description notes the row-numbering trap.

## Phase 6 - Destructive write tool (planned, blocked on 150 + safety surface)

### 6.1 `save_preset(setlist, slot, confirm)`

- [ ] Gate on the safety surface (Phase 1): confirmation, factory refusal, scratch range, backup.
- [ ] Delegate to `QuadCortex::save_current_preset(...)` after the gates pass.
- [ ] Return confirmation (setlist, slot, backup blob id).

## Phase 7 - Shared safety module (future)

### 7.1 Extract safety rules into a shared module

- [ ] Move the factory-setlist refusal, scratch range, and backup logic into a shared module the CLI and GUI can reuse.
- [ ] The server is the first consumer, not the owner.

## Phase 8 - Verification (planned)

### 8.1 Hardware smoke

- [ ] Exercise every tool against a real Quad Cortex from this crate.
- [ ] Confirm the safety surface refuses the dangerous cases (factory save, unconfirmed save, out-of-scratch save without override).
- [ ] Confirm a backed-up slot can be rolled back.

### 8.2 Agent-generated test review

- [ ] Cross-check agent-generated safety-surface tests against the client layer (150) and a real device.
- [ ] Agent tests must not be the sole basis for accepting safety-surface behaviour (AGENTS.md assurance).

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 0.1 | Scaffolded `cortex-mcp` binary | `crates/cortex-mcp/src/main.rs`, `crates/cortex-mcp/Cargo.toml` | [x] | [x] |
| 2026-08-01 | 0.2 | Wrote spec/design/tasks (stub) for this zone | `spec/300-mcp/{spec,design,tasks}.md` | [x] | [x] |