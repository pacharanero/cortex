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

This layer turns generated protobuf shapes into typed host views, pure keyed grid updates, catalog metadata, subscribed state and save-safety values. The wire contract remains in `proto`; callers use `view`, `grid`, `state`, `catalog`, and `safety` rather than reproducing those decisions.

The design principle is: never let a caller touch a raw `oneof` accessor without a doc-comment that explains the trap. The proto types are correct; they are just not safe to navigate blind.

## [DES-FILES] Owned Files

| File | Role | Status |
| --- | --- | --- |
| `crates/cortex-rs/src/device.rs` | `DeviceKind` enum, `vid_pid()` | Implemented |
| `crates/cortex-rs/src/message.rs` | `Message` struct, `parse()`, `TRAILER_LEN` | Implemented |
| `crates/cortex-rs/src/catalog.rs` | Bounded `gzip(tar(XML))` parser and model metadata | Implemented, hardware-verified |
| `crates/cortex-rs/src/grid.rs` | Checked `Row` plus pure keyed update builders | Implemented; exposed operations hardware-verified |
| `crates/cortex-rs/src/helpers.rs` | Pure preset topology, MIDI, tempo, dynamic-option and comparison helpers | Implemented from schema and upstream evidence |
| `crates/cortex-rs/src/view.rs` | Stable serialisable host views | Implemented |
| `crates/cortex-rs/src/safety.rs` | Save policy, preparations and validated commit flow | Implemented; core path hardware-verified |

`state.rs` consumes these domain views but is owned by zone 140 because its generation, revision, continuity and wait semantics are part of the held session contract.

## [DES-DEVICE] DeviceKind Design

`DeviceKind` is a two-variant enum with a `vid_pid()` method. The variants express the project's current hardware targets, not a claim that both use one application protocol: hardware established shared HID framing but different Quad and Nano envelopes and domain models. Adding another supported device would require an enum variant and transport evidence; it need not first appear in the recovered Quad Cortex schema.

```rust
pub enum DeviceKind {
    QuadCortex,
    NanoCortex,
}
```

Hardware confirms Nano VID:PID `152A:88E7`, 65-byte HID reports, shared flag framing, and its separate four-byte-footer codec. Typed state, amp, Gate reduction, bypass and raw FX parameter operations are hardware-verified; wider operations remain provisional and are capability-gated rather than making the device kind itself provisional. `DeviceKind` carries the PID and geometry, while the Quad message request and session paths reject Nano before USB I/O so its four-byte footer can never reach the Quad eight-byte parser.

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

## [DES-PRESET] Host View Design

`view::Preset::from_binary` creates an owned serialisable projection for CLI, daemon, MCP and Tauri. `view::Row` and `view::Block` preserve both wire and screen row labels, occupied cells only, parameter values and bypass state. The protobuf remains available to protocol layers; host surfaces should not independently interpret its positional arrays.

## [DES-GRID] Grid and Row Design

The read side is represented by `view::Preset::{rows,blocks}`. The write side uses `grid::Row` and pure protobuf builders. `Row::from_wire` and `Row::from_screen` centralise conversion and refuse invalid values; splitter operations refuse odd wire rows before sending. A separate dense `Grid` type remains optional rather than being created merely to match an old plan.

## [DES-CATALOG] Catalog Design

`Catalog::parse` gunzips the payload through the same `bounded_gunzip` helper `Message::decode` uses (see [100-transport](../100-transport/design.md#des-request-synchronous-requestresponse)), capping decompressed tar size at `MAX_DECOMPRESSED_CATALOG_LEN` (8 MiB) so a malformed or hostile payload cannot allocate without bound before extraction fails. It then opens the tar archive, extracts `ModelRepo.xml`, and eagerly parses model/category/parameter metadata. Positional `empty` and `meter` entries remain in each parameter vector because filtering them shifts all later wire indices. The vendor's `tm` attribution is exposed verbatim as `based_on`; this project does not paraphrase it. Tests construct fictional minimal XML/tar fixtures rather than committing a real device catalog.

## [DES-HELPERS] Helper Functions

Checked slot conversion, inverse conversion, dB scaling and row validation are implemented. `helpers.rs` is the canonical read-side interpretation of protobuf preset topology: explicit row, column and parameter keys override repeated-field position, while absent keys use position. It exposes occupied blocks, branches and reserved lanes, input-chain rows, zero-safe STOMP assignments, typed 10-by-12 MIDI storage, positional tempo values, current-preset dynamic options and list-aware parameter comparison. `view` and client occupancy checks reuse its keyed lookup rather than maintaining a second interpretation.

Dynamic option names come from the current preset rather than the catalog because some lists enumerate the preset's blocks. `option_value` and `option_at` centralise `index / (count - 1)`. `params_equal` compares ordinary floats within tolerance, compares list values by selected index across changed cardinality, and treats NaN as equal only to NaN. Input-control comparison explicitly ignores positional index 2, the sampled gain-reduction meter rather than a setting.

## [DES-ROW-TRAP] The Row-Numbering Trap (authoritative design note)

This is the single most important safety invariant in the domain model, repeated here so it is not lost:

- **Rows are 0-based in the API, 1-4 on screen.** A `row = 0` in a `GridMove` or `DefaultParameters` message targets the on-screen row 1.
- **A wrong-row edit succeeds silently.** The device does not push back; the edit is accepted and applied to the wrong row. The result is a preset that looks correct in the editor but is wrong on the device.
- **A recalled preset carries no explicit row.** Writing the recalled blob back wholesale does nothing. To move or edit a block, the client layer (150) must set the `row` (and `column`) explicitly before sending.
- **Every helper that takes or returns a row documents this.** Not just one place - every signature.
- **The MCP safety surface (300) repeats it in tool descriptions.** An agentic patch editor must not silently edit the wrong row.

## [DES-SCALING] Parameter Scaling Design

Two constants/helpers for parameter scaling:

- `UNITY_LEVEL = 10.0 / 13.0` (= `0.76923077`). This is the parameter value that represents 0 dB on the -100..+30 dB span used by level parameters. The GUI uses it to render a "0 dB" tick.
- `input_level_db(level) = -12.0 + 72.0 * level`. Input ports span -12 to +60 dB; this converts the normalised `level` (0.0..1.0) to dB. The IO-settings surface (a future message wrapper) uses it.

These are free functions re-exported from the crate root. They remain in `client.rs` alongside the I/O settings types that consume them; preset interpretation is isolated in `helpers.rs`.

## [DES-LAYERS] Layer Map (cross-reference)

```text
Layer 4: Domain (130)      - DeviceKind, Message, Preset, Grid, Block, Scene, Catalog
       ^
       |  (cortex_rs::proto::* types, reassembled buffer)
       |
Layer 3: Proto (120)       - prost-generated types
Layer 2: Framing (110)     - reassembled buffer production
```

This zone is Layer 4. Protocol layers still use generated types directly where the wire requires them; host-facing rendering and validation use the shared domain views and builders.

## [DES-TEST] Testing Strategy

- **Unit tests for `Message::parse`** (implemented): body/trailer split, LE `u16` read, short-buffer rejection.
- **Unit tests for `DeviceKind` profiles** (implemented): both hardware VID:PIDs, report dimensions and device-specific write-STALL policies are asserted.
- **Unit tests for helpers and views** cover slot round-trips, row validation, scaling, occupancy and serialisable projections.
- **Catalog parsing tests** build fictional minimal XML/tar/gzip fixtures and exercise malformed/bounded input without committing vendor or device data.
- **Hardware smoke test** (manual runbook): recall a preset, parse it, edit a block, write it back, recall again and confirm the edit landed on the right row. CI has no hardware.

Agent-generated tests are not the sole basis for accepting the row-numbering-trap behaviour; the trap is cross-checked against `pyquadcortex` and a real device.
