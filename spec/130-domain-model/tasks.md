---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["domain-model", "tasks", "traceability"]
spec: spec.md
design: design.md
---

# cortex-rs - Domain Model Tasks (Zone 130)

> Implementation tasks for the typed domain model. The implemented surface (`DeviceKind`, `Message`) is done; the planned surface (`Preset`, `Grid`, `Block`, `Scene`, `Catalog`, helpers) is tracked here. Each planned item is labelled provisional until verified against a real Quad Cortex.

## Phase 1 - Implemented surface (DONE)

### 1.1 DeviceKind

- [x] `crates/cortex-rs/src/device.rs` with `DeviceKind::{QuadCortex, NanoCortex}`.
- [x] `vid_pid()` returning `(0x152A, 0x880A)` for QuadCortex, `(0x152A, 0xFFFF)` placeholder for NanoCortex.
- [x] Provisional label on `NanoCortex` doc-comment.
- [x] `@see` link to this spec.

### 1.2 Message envelope

- [x] `crates/cortex-rs/src/message.rs` with `Message { message_type: u16, body: Bytes }`.
- [x] `Message::parse(&[u8])` splitting body + 8-byte trailer, reading LE `u16` type.
- [x] `TRAILER_LEN = 8` public constant.
- [x] `Error::Trailer` on short buffer.
- [x] Unit tests: body/trailer split, short-buffer rejection.
- [x] `@see` link to this spec.

## Phase 2 - Preset and Grid (PLANNED)

### 2.1 Preset wrapper

- [ ] `crates/cortex-rs/src/preset.rs` with `Preset { inner: proto::BinaryPreset }`.
- [ ] Ergonomic accessors: `name()`, `hash()`, `chains()`, `scenes()`, `bypass()`, `tags()`, metadata (`created_version`, `modified_version`, `oldest_compatible_version`).
- [ ] `Scene` view: index, label, color, tempo, per-block bypass - hiding the parallel-array layout.
- [ ] Doc-comment on the "recalled preset carries no row" trap.
- [ ] `Serialize`/`Deserialize` for Tauri/MCP transport.
- [ ] Unit tests against a fixture preset (fixture must not contain Neural DSP strings - see AGENTS.md).
- [ ] `@see` link to this spec and [DES-PRESET].

### 2.2 Grid, Block, Scene

- [ ] `crates/cortex-rs/src/grid.rs` with `Grid`, `Block`, `Scene`.
- [ ] `Grid` exposes 4 rows; row accessors document the 0-based-API / 1-based-screen trap.
- [ ] `Block` exposes model hash, column, params (borrowed, not owned).
- [ ] Splitter/mixer access returns `None` for rows 1 and 3, with a doc-comment.
- [ ] Unit tests for row access and splitter/mixer placement.
- [ ] `@see` link to this spec and [DES-GRID], [DES-ROW-TRAP].

## Phase 3 - Catalog (PLANNED)

### 3.1 Catalog parsing

- [ ] `crates/cortex-rs/src/catalog.rs` with `Catalog { models: HashMap<u32, ModelInfo> }`.
- [ ] `Catalog::parse(model_repo_payload: &[u8])` - field-level gzip decompression via `flate2`, then proto decode.
- [ ] `ModelInfo { name, category, parameters }`.
- [ ] Document that the catalog covers purchased models and captures, not just factory.
- [ ] Unit test against a captured ~47KB blob fixture (do not check in if it contains Neural DSP strings).
- [ ] `@see` link to this spec and [DES-CATALOG].

## Phase 4 - Helpers and scaling (PLANNED)

### 4.1 Navigation helpers

- [ ] `crates/cortex-rs/src/helpers.rs` with `blocks()`, `splits()`, `slot_to_position()`, `position_to_slot()`.
- [ ] Port from `pyquadcortex` (MIT) with attribution in `NOTICE`.
- [ ] Every helper doc-comment surfaces the row-numbering trap.
- [ ] Unit tests: `slot_to_position` / `position_to_slot` round-trip; `blocks` / `splits` against a fixture.
- [ ] `@see` link to this spec and [DES-HELPERS], [DES-ROW-TRAP].

### 4.2 Parameter scaling

- [ ] `UNITY_LEVEL = 10.0 / 13.0` constant in `helpers.rs`.
- [ ] `input_level_db(level) -> f32` helper (`-12.0 + 72.0 * level`).
- [ ] Unit tests for the conversions.
- [ ] `@see` link to this spec and [DES-SCALING].

## Phase 5 - Verification (PLANNED)

### 5.1 Hardware smoke

- [ ] Recall a preset, parse to `Preset`, inspect `Grid`, edit a `Block`, write back, recall again - confirm the edit landed on the right row.
- [ ] Request `ModelRepo`, parse to `Catalog`, confirm a known model hash resolves to its name.
- [ ] Confirm `VersionMessage.DeviceType` maps to `DeviceKind::QuadCortex` on the real device.
- [ ] Record the CorOS version and firmware hash in a Work Sessions row.

### 5.2 Nano Cortex provisional check

- [ ] When a Nano Cortex is available: verify the `ATMA=1` `DeviceType`, record the real product ID, replace the `0xFFFF` placeholder.
- [ ] Remove the provisional label from `NanoCortex` once verified.
- [ ] Record the verification in a Work Sessions row.

## Open Questions

- Should `Preset` expose a `to_binary()` that re-encodes to the wire shape, or is that the client layer's (150) job? Current leaning: client layer, to keep the domain model read-only.
- Should the catalog be cached on disk between sessions, or always re-fetched from the device? Tracked in [150-client](../150-client/spec.md).
- Should `Grid` enforce the splitter/mixer invariant at construction, or just document it? Current leaning: document, return `None`, do not error - enforcement belongs in the client layer.

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1, 1.2 | Implemented `DeviceKind` and `Message` | `crates/cortex-rs/src/{device,message,lib}.rs` | [x] | [x] |