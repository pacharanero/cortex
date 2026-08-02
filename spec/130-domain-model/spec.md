---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["domain-model", "device", "message", "preset", "grid", "block", "scene", "catalog", "cortex-rs"]
---

# cortex-rs - Domain Model (Zone 130)

> Owns the typed domain model that sits above the generated protobuf types (zone 120). This is the layer callers actually use: `DeviceKind`, `Message`, and the planned `Preset`, `Grid`, `Block`, `Scene`, `Catalog`, plus the helper functions that navigate the grid's row/column trap. The domain model is the ergonomic surface; the proto types are the wire-shape contract underneath.

## References

- [001-overview/spec.md](../001-overview/spec.md) - taxonomy, traceability rules, routing index.
- [120-proto-schema/spec.md](../120-proto-schema/spec.md) - the generated `cortex_protobuf_v2` types this layer wraps.
- [110-framing/spec.md](../110-framing/spec.md) - the reassembled-message envelope this layer's `Message::parse` consumes.
- [Protocol research note](../../../quad-cortex-linux-editor-and-protocol.md) (parent workspace root) - authoritative protocol facts, including the row-numbering trap.
- [pyquadcortex](https://github.com/stokes-audio/pyquadcortex) - the MIT-licensed reference implementation whose domain helpers (`blocks`, `splits`, `slot_to_position`, `position_to_slot`) we port.
- [AGENTS.md](../../AGENTS.md) - protocol invariants, the row-numbering trap, the MCP safety surface.

## Problem Statement

The generated protobuf types are correct but unergonomic: `BinaryPreset` is a flat bag of `oneof` fields, `Chain` carries rows as optional `uint32`, and the grid's row numbering is 0-based in the API but 1-4 on the device screen. A wrong-row edit succeeds silently - the device does not push back. This zone owns the typed wrappers and helper functions that make the domain safe to navigate, and that surface the row-numbering trap to every caller (CLI, MCP server, GUI) rather than letting each one re-derive it.

The model is split into implemented surfaces (`DeviceKind`, `Message`) and planned surfaces (`Preset`, `Grid`, `Block`, `Scene`, `Catalog`, and the navigation helpers). The implemented surfaces are hardware-verified; the planned surfaces are tracked as tasks and labelled provisional until verified against a real Quad Cortex.

## User Stories

### Primary Users

Maintainers, AI coding agents, and the CLI/MCP/GUI surfaces that consume the crate.

### Stories

**As an** AI agent
**I want** a `Preset` wrapper that gives me the chains, scenes, and blocks without me touching `BinaryPreset`'s `oneof` accessors directly
**So that** I can write a patch-editing tool without re-learning the proto layout each session.

**As an** MCP server author
**I want** the row-numbering trap surfaced in the helper function signatures and doc-comments
**So that** a tool that edits row 2 (API) does not silently hit the on-screen row 3.

**As a** CLI user
**I want** `cortex version` to resolve the `DeviceType` from a `VersionMessage` into a `DeviceKind`
**So that** the output says "Quad Cortex" or "Nano Cortex" instead of `QC`/`ATMA`.

**As a** crate consumer
**I want** the domain model to build with `default-features = false` (no HID)
**So that** I can decode a captured preset blob offline.

**As a** maintainer
**I want** the catalog parsing to handle the ~47KB gzip blob the device pushes
**So that** I can resolve model hashes to human-readable names in the GUI.

## Requirements

### Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | `crates/cortex-rs/src/device.rs` exposes `DeviceKind::{QuadCortex, NanoCortex}` with a `vid_pid()` method returning `(u16, u16)`. | Must Have |
| FR-2 | `DeviceKind::QuadCortex` returns `(0x152A, 0x880A)` (verified against CorOS 4.0.1). `DeviceKind::NanoCortex` returns `(0x152A, 0xFFFF)` as a placeholder until the real product ID is verified. | Must Have |
| FR-3 | `DeviceKind::NanoCortex` is labelled provisional in its doc-comment (Nano-specific behaviour is not hardware-verified). | Must Have |
| FR-4 | `crates/cortex-rs/src/message.rs` exposes `Message { message_type: u16, body: Bytes }` and a `Message::parse(&[u8])` constructor that splits a reassembled buffer into body + 8-byte trailer. | Must Have |
| FR-5 | `message.rs` exposes `TRAILER_LEN = 8` as a public constant. | Must Have |
| FR-6 | `Message::parse` reads the message-type tag as a little-endian `u16` from the first two bytes of the trailer; the remaining 6 bytes are currently unused and undocumented. | Must Have |
| FR-7 | `Message::parse` returns `Error::Trailer` if the buffer is shorter than `TRAILER_LEN`. | Must Have |
| FR-8 | A `Preset` wrapper around `proto::BinaryPreset` exposes ergonomic access to: name, hash, chains, scenes (labels, colors, tempo), bypass, tags, metadata (created/modified/oldest-compatible versions). | Should Have |
| FR-9 | A `Grid` model exposes the row/column layout and documents the row-numbering trap: rows are 0-based in the API, 1-4 on screen. | Should Have |
| FR-10 | A `Block` model exposes a model's hash, column position, and params. | Should Have |
| FR-11 | A `Scene` model exposes index, label, color, and per-block bypass. | Should Have |
| FR-12 | A `Catalog` parses the `ModelRepo` gzip blob (~47KB) the device pushes into a `model_id -> {name, category, parameters}` map. The catalog covers purchased models and captures, not just factory. | Should Have |
| FR-13 | Helper functions `blocks()`, `splits()`, `slot_to_position()`, `position_to_slot()` are ported from `pyquadcortex` with attribution, and their doc-comments surface the row-numbering trap. | Should Have |
| FR-14 | A recalled preset carries NO explicit row; writing it back wholesale does nothing. The `Preset` wrapper documents this and the client layer (150) must set the row before writing. | Should Have |
| FR-15 | Splitters and mixers exist only on rows 0 and 2. The `Grid` model enforces or at least documents this invariant. | Should Have |
| FR-16 | The `UNITY_LEVEL` constant (10/13 = 0.76923077, representing 0 dB on the -100..+30 dB span) is exposed for parameter scaling. | Should Have |
| FR-17 | The `input_level_db` conversion (`-12 + 72 * level`, input ports span -12 to +60 dB) is exposed as a helper for the IO-settings surface. | Should Have |
| FR-18 | All domain types implement `Debug` and `Clone`; the read-only views implement `Serialize`/`Deserialize` for Tauri and MCP transport. | Should Have |
| FR-19 | All domain source files carry `@see` doc-comments linking to this spec and its `design.md`. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | The domain model builds with `default-features = false` (no `hidapi`). | CI-enforced |
| NFR-2 | No `async` runtime dependency in this layer; the domain model is synchronous. | Review-enforced |
| NFR-3 | The row-numbering trap is documented in every helper that takes or returns a row, not just one place. | Review-enforced |
| NFR-4 | Catalog parsing handles the ~47KB gzip blob without unbounded allocation; field-level gzip inside `bytes` is decompressed lazily where possible. | Review-enforced |
| NFR-5 | Provisional surfaces (`NanoCortex`, unverified message-type decodes, the catalog shape) are labelled provisional in doc-comments and in the GUI/release notes downstream. | Review-enforced |

## Acceptance Criteria

- [x] `DeviceKind` exists with `vid_pid()` and the provisional `NanoCortex` label.
- [x] `Message::parse` splits body and trailer, reads the LE `u16` type, and rejects short buffers.
- [x] `TRAILER_LEN = 8` is public.
- [x] `device.rs` and `message.rs` carry `@see` links to this spec.
- [ ] `Preset` wrapper exists with ergonomic access to chains, scenes, bypass, metadata.
- [ ] `Grid` model exists and documents the row-numbering trap.
- [ ] `Block`, `Scene` models exist.
- [ ] `Catalog` parses the `ModelRepo` blob.
- [ ] `blocks()`, `splits()`, `slot_to_position()`, `position_to_slot()` are ported with attribution.
- [ ] `UNITY_LEVEL` and `input_level_db` helpers are exposed.

## Non-Goals

- The generated protobuf types themselves (owned by [120-proto-schema](../120-proto-schema/spec.md)).
- The connect handshake, keepalive, and request correlation (owned by [140-session](../140-session/spec.md)).
- The ergonomic `QuadCortex` client API that ties session + domain together (owned by [150-client](../150-client/spec.md)).
- The MCP safety surface (owned by [300-mcp](../300-mcp/spec.md)), though this layer's helpers feed its tool descriptions.
- On-device builds or the footswitch/rotary ioctl route (out of scope; see AGENTS.md).

## Dependencies

- **Downward**: [120-proto-schema](../120-proto-schema/spec.md) - the generated `cortex_protobuf_v2` types this layer wraps.
- **Downward**: [110-framing](../110-framing/spec.md) - the reassembled buffer that `Message::parse` consumes.
- **Sideways**: `bytes`, `serde`, `flate2` (workspace deps) for `Bytes`, serialization, and gzip decompression of the catalog blob.
- **Upward**: [140-session](../140-session/spec.md) and [150-client](../150-client/spec.md) consume the domain types; [200-cli](../200-cli/spec.md), [300-mcp](../300-mcp/spec.md), and [400-gui](../400-gui/spec.md) render them.
- **Prior art**: `pyquadcortex` (MIT) for the `blocks`/`splits`/`slot_to_position`/`position_to_slot` helpers; attribution recorded in `NOTICE`.

## Appendix

### Routing Index entry

| Zone | Spec | Owns (primary source) | Status |
| --- | --- | --- | --- |
| [130-domain-model](./spec.md) | Domain model | `crates/cortex-rs/src/{device,message}.rs` (and future `preset.rs`, `grid.rs`, `block.rs`, `scene.rs`, `catalog.rs`, `helpers.rs`) | Partial |

### The row-numbering trap (authoritative statement)

Rows are **0-based in the API**, **1-4 on the device screen**. A `GridMove` or `DefaultParameters` message with `row = 1` targets the on-screen row 2. A wrong-row edit succeeds silently - the device does not push back, and the result is a preset that looks correct in the editor but is wrong on the device. Every helper in this layer that takes or returns a row documents this in its doc-comment, and the MCP safety surface (300) repeats it in tool descriptions.

A recalled preset carries NO explicit `row` field on its chains; writing the recalled blob back wholesale does nothing. To move or edit a block, the client layer (150) must set the `row` (and `column`) explicitly before sending.

### Splitters and mixers

Splitters and mixers exist only on rows 0 and 2 (the rows that feed the two parallel signal paths). The `Grid` model documents this; a future enforcement pass may return `None` or an error for splitter/mixer access on rows 1 and 3.

### Parameter scaling constants

| Constant | Value | Meaning |
| --- | --- | --- |
| `UNITY_LEVEL` | `10.0 / 13.0` = `0.76923077` | 0 dB on the -100..+30 dB parameter span. |
| `input_level_db(level)` | `-12.0 + 72.0 * level` | Input port level in dB; input ports span -12 to +60 dB. |

### Catalog provenance

The catalog comes FROM the device as a `ModelRepo` message containing a ~47KB gzip blob (`model_repo_payload` `bytes` field). It is field-level gzip, not frame-level. It covers purchased models and captures, not just factory content. Parsing it yields a `model_id -> {name, category, parameters}` map that the GUI uses to render human-readable block names.

### Glossary

| Term | Definition |
| --- | --- |
| `DeviceKind` | Enum distinguishing Quad Cortex from Nano Cortex; carries the USB VID:PID. |
| `Message` | A reassembled envelope: protobuf body + the message-type tag read from the 8-byte trailer. |
| `Preset` | Typed wrapper around `proto::BinaryPreset`; the ergonomic preset surface. |
| `Grid` | The row/column model of the current preset's signal chains. |
| `Block` | A single model instance on the grid: hash, column, params. |
| `Scene` | A preset scene: index, label, color, per-block bypass. |
| `Catalog` | Parsed `ModelRepo`: model id to name/category/parameters. |
| Row-numbering trap | Rows are 0-based in the API, 1-4 on screen; a wrong-row edit succeeds silently. |
| `UNITY_LEVEL` | 10/13, the parameter value representing 0 dB on the -100..+30 dB span. |
| Provisional | Not yet verified against real hardware by this project. |