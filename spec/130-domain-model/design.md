---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["domain-model", "design", "device", "message", "preset", "grid", "catalog"]
spec: spec.md
---

# cortex-rs - Domain Model Design (Zone 130)

## [DES-OVR] Overview

This layer wraps the generated `cortex_protobuf_v2` types (zone 120) into ergonomic, documented Rust types. The wire-shape contract lives in the proto module; the navigation safety, the row-numbering trap, and the human-readable model catalog live here. The implemented surface is `DeviceKind` and `Message`; the planned surface is `Preset`, `Grid`, `Block`, `Scene`, `Catalog`, and the navigation helpers.

The design principle is: never let a caller touch a raw `oneof` accessor without a doc-comment that explains the trap. The proto types are correct; they are just not safe to navigate blind.

## [DES-FILES] Owned Files

| File | Role | Status |
| --- | --- | --- |
| `crates/cortex-rs/src/device.rs` | `DeviceKind` enum, `vid_pid()` | Implemented |
| `crates/cortex-rs/src/message.rs` | `Message` struct, `parse()`, `TRAILER_LEN` | Implemented |
| `crates/cortex-rs/src/preset.rs` (planned) | `Preset` wrapper around `BinaryPreset` | Planned |
| `crates/cortex-rs/src/grid.rs` (planned) | `Grid`, `Block`, `Scene`, row/column helpers | Planned |
| `crates/cortex-rs/src/catalog.rs` (planned) | `Catalog` parsed from `ModelRepo` blob | Planned |
| `crates/cortex-rs/src/helpers.rs` (planned) | `UNITY_LEVEL`, `input_level_db`, scaling conversions | Planned |

## [DES-DEVICE] DeviceKind Design

`DeviceKind` is a two-variant enum with a `vid_pid()` method. The variants express the project's current hardware targets, not a claim that both use one protocol: the recovered schema names `QC=0` and `ATMA=1`, while Nano transport compatibility remains unestablished. Adding another supported device would require an enum variant and transport evidence; it need not first appear in this recovered Quad Cortex schema.

```rust
pub enum DeviceKind {
    QuadCortex,
    NanoCortex, // provisional
}
```

`NanoCortex` is labelled provisional in its doc-comment because transport compatibility itself is unverified. Third-party observation reports VID:PID `152A:88E7` and 65-byte HID reports, but no protobuf/trailer exchange. The placeholder product ID `0xFFFF` is a sentinel that will never match a real device, so a `NanoCortex` open fails closed until this project verifies the transport and deliberately replaces it.

The `vid_pid()` method is `const fn` so it can be used in `const` contexts (e.g. a static lookup table in the transport layer).

## [DES-MESSAGE] Message Design

`Message` is the parsed envelope: the protobuf body and the message-type tag read from the trailer. It is the boundary between zone 110 (framing, which produces the reassembled buffer) and this zone (which hands the body to a proto decode).

```rust
pub struct Message {
    pub message_type: u16,
    pub body: Bytes,
}
```

Design choices:

- **`Bytes`, not `Vec<u8>`.** The body is a zero-copy view into the reassembled buffer; `Bytes` lets downstream proto decode avoid a copy.
- **`message_type: u16`, not the generated enum.** The proto-generated `CortexMessageType::Enum` is a Rust enum, but a CorOS update can add variants we do not know about. Storing the raw `u16` means an unknown type is preserved, not silently coerced to `Undefined`. The domain layer (this zone, in a future `Message::message_type_enum()` helper) maps known values to the enum and leaves unknown values as `None` or a catch-all.
- **`TRAILER_LEN` is public.** The framing layer (110) and the tests both need it; hiding it behind a private constant would force duplication.
- **The remaining 6 trailer bytes are currently unused.** The recovered schema does not document them; `Message::parse` reads only the first two. If a future CorOS version uses them (e.g. a sequence number), this is where they are exposed.

## [DES-PRESET] Preset Wrapper Design (planned)

`Preset` wraps `proto::BinaryPreset` and exposes ergonomic accessors. The design choice is a newtype wrapper, not a re-implementation:

```rust
pub struct Preset {
    inner: proto::BinaryPreset,
}
```

- **Why a wrapper, not a re-implementation?** The proto type is the wire shape; re-implementing it would create a second source of truth for the preset structure. The wrapper adds ergonomics (named scene access, typed block iteration) without duplicating the wire layout.
- **Why not `Deref`?** `Deref` hides the inner type and lets callers reach the raw `oneof` accessors without the doc-comments. Explicit accessor methods keep the row-numbering trap visible.
- **Scenes are parallel arrays in the proto** (`scene_labels`, `scene_colors`, `scene_tempo`, `bypass`). The wrapper presents them as a `Vec<Scene>` indexed by scene index, hiding the parallel-array layout.
- **A recalled preset carries no explicit row.** The wrapper documents this; the client layer (150) sets the row before writing. See [DES-ROW-TRAP].

## [DES-GRID] Grid, Block, Scene Design (planned)

The grid is a 4-row by N-column model. Each row is a `Chain` in the proto. The design:

```rust
pub struct Grid {
    chains: Vec<Chain>, // 4 entries, one per row
}
pub struct Block {
    model_hash: u32,
    column: u32,
    params: Vec<Param>,
}
pub struct Scene {
    index: u32,
    label: Option<String>,
    color: Option<u32>,
    bypass: Vec<bool>, // per-block
}
```

- **Rows are 0-based in the API, 1-4 on screen.** Every accessor that takes or returns a row documents this in its doc-comment. See [DES-ROW-TRAP].
- **Splitters and mixers exist only on rows 0 and 2.** The `Grid` accessor for splitter/mixer returns `None` for rows 1 and 3, with a doc-comment explaining why.
- **`Block` is a view, not an owner.** A `Block` borrows from the `Chain` it came from; it does not own a copy of the params. This keeps preset editing cheap.

## [DES-CATALOG] Catalog Design (planned)

The catalog is parsed from the `ModelRepo` message's `model_repo_payload` `bytes` field - a ~47KB gzip blob. The design:

```rust
pub struct Catalog {
    models: HashMap<u32, ModelInfo>,
}
pub struct ModelInfo {
    pub name: String,
    pub category: String,
    pub parameters: Vec<ParameterInfo>,
}
```

- **Field-level gzip, not frame-level.** The blob is decompressed once with `flate2`, then the inner protobuf is decoded. This is the same field-level gzip pattern the framing layer (110) handles at the frame level.
- **Covers purchased models and captures, not just factory.** The catalog is the union of factory, purchased, and user-captured models. The GUI uses it to render human-readable block names from the model hash.
- **Lazy where possible.** If the catalog is large and only a few lookups are needed, the design may expose a `Catalog::lookup(hash)` that decompresses on demand. This is a future optimisation; the initial implementation decodes the whole blob.

## [DES-HELPERS] Helper Functions Design (planned)

Ported from `pyquadcortex` (MIT) with attribution:

- `blocks(preset, row) -> Vec<Block>` - the model instances on a given row.
- `splits(preset, row) -> Option<(SplitPoint, SplitPoint)>` - the split/mix control points; `None` for rows 1 and 3.
- `slot_to_position(slot) -> (row, column)` - convert a linear slot index to a (row, column) pair.
- `position_to_slot(row, column) -> u32` - the inverse.

Each helper's doc-comment surfaces the row-numbering trap. The helpers are free functions, not methods on `Preset`, because they are pure functions of the preset structure and are easier to test in isolation.

## [DES-ROW-TRAP] The Row-Numbering Trap (authoritative design note)

This is the single most important safety invariant in the domain model, repeated here so it is not lost:

- **Rows are 0-based in the API, 1-4 on screen.** A `row = 0` in a `GridMove` or `DefaultParameters` message targets the on-screen row 1.
- **A wrong-row edit succeeds silently.** The device does not push back; the edit is accepted and applied to the wrong row. The result is a preset that looks correct in the editor but is wrong on the device.
- **A recalled preset carries no explicit row.** Writing the recalled blob back wholesale does nothing. To move or edit a block, the client layer (150) must set the `row` (and `column`) explicitly before sending.
- **Every helper that takes or returns a row documents this.** Not just one place - every signature.
- **The MCP safety surface (300) repeats it in tool descriptions.** An agentic patch editor must not silently edit the wrong row.

## [DES-SCALING] Parameter Scaling Design (planned)

Two constants/helpers for parameter scaling:

- `UNITY_LEVEL = 10.0 / 13.0` (= `0.76923077`). This is the parameter value that represents 0 dB on the -100..+30 dB span used by level parameters. The GUI uses it to render a "0 dB" tick.
- `input_level_db(level) = -12.0 + 72.0 * level`. Input ports span -12 to +60 dB; this converts the normalised `level` (0.0..1.0) to dB. The IO-settings surface (a future message wrapper) uses it.

These are free functions in `helpers.rs`, not methods, because they are pure conversions.

## [DES-LAYERS] Layer Map (cross-reference)

```text
Layer 4: Domain (130)      - DeviceKind, Message, Preset, Grid, Block, Scene, Catalog
       ^
       |  (cortex_rs::proto::* types, reassembled buffer)
       |
Layer 3: Proto (120)       - prost-generated types
Layer 2: Framing (110)     - reassembled buffer production
```

This zone is Layer 4. It depends on Layer 3 (proto types) and Layer 2 (the reassembled buffer that `Message::parse` consumes). Nothing above it (session, client, CLI, MCP, GUI) touches the proto types directly; they go through this layer.

## [DES-TEST] Testing Strategy

- **Unit tests for `Message::parse`** (implemented): body/trailer split, LE `u16` read, short-buffer rejection.
- **Unit tests for `DeviceKind::vid_pid`** (implemented): the Quad Cortex pair is asserted; the Nano Cortex placeholder is asserted as `0xFFFF`.
- **Unit tests for the helpers** (planned): `slot_to_position` / `position_to_slot` round-trip; `blocks` / `splits` against a fixture preset.
- **Catalog parsing test** (planned): against a captured ~47KB blob (fixture, not checked in if it contains Neural DSP strings - see AGENTS.md legal hygiene).
- **Hardware smoke test** (manual runbook): recall a preset, parse it, edit a block, write it back, recall again and confirm the edit landed on the right row. CI has no hardware.

Agent-generated tests are not the sole basis for accepting the row-numbering-trap behaviour; the trap is cross-checked against `pyquadcortex` and a real device.
