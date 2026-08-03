---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["framing", "hid", "report", "frame", "reassembler", "encode", "quad-cortex", "protocol"]
---

# cortex-rs - HID Frame Codec

> Owns the pure-logic conversion between logical messages (`message_type` + protobuf bytes) and the 129-byte HID reports the device exchanges. No I/O: this zone is bytes-in, bytes-out, and is the layer the transport (`100`), the message envelope (`120`/`130`), and every higher surface build on. Ported from `pyquadcortex/framing.py` (MIT, attribution in `NOTICE`); hardware-verified against a real Quad Cortex (CorOS 4.0.1, firmware `d14e`).

## References

- [Protocol research note](../../quad-cortex-linux-editor-and-protocol.md) (at the parent workspace root) - the authoritative protocol facts.
- [pyquadcortex `framing.py`](https://github.com/stokes-audio/pyquadcortex/blob/main/pyquadcortex/framing.py) - the MIT-licensed Python reference this module is ported from (c) 2026 Stokes.
- [pyquadcortex protocol docs](https://github.com/stokes-audio/pyquadcortex/blob/main/docs/protocol.md) - the wire format reference.
- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the system context flow map this layer sits in (`[Flow.Framing]`).
- [100-transport spec](../100-transport/spec.md) - the I/O layer that feeds `Frame::parse` and consumes `encode_message`. Cross-references this zone in its Non-Goals and Dependencies.
- [100-transport design](../100-transport/design.md) [DES-REQUEST] - the `request` path that calls into this layer's reassembler reset-on-FIRST rule.
- [120-proto-schema spec](../120-proto-schema/spec.md) - the protobuf schema and `CortexMessageType` enum whose tag the 8-byte trailer carries.
- [130-domain-model spec](../130-domain-model/spec.md) - owns `Message::parse`, which strips the trailer this layer's reassembler yields.
- [NOTICE](../../NOTICE) and [THIRD-PARTY-NOTICES.md](../../THIRD-PARTY-NOTICES.md) - attribution for the MIT-licensed `pyquadcortex` derivation.
- Owned source: `crates/cortex-rs/src/framing.rs`.

## Problem Statement

The Cortex Control protocol does not send a length-prefixed protobuf body over a socket. It sends 128-byte USB HID reports, each carrying a 2-byte `[len][flags]` prefix and up to 126 bytes of payload, with a flag byte (`FIRST` / `MIDDLE` / `LAST` / `COMPLETE`) telling the receiver where this report sits in a multi-report message. There are no sequence numbers and no total-length field: reassembly is purely flag-driven. On top of that, the logical message is not just the protobuf body - it is `protobuf ++ 8-byte trailer`, and the message-type tag lives in the trailer as a little-endian `u16`, not in a header.

This zone owns the four pieces of pure logic that convert between those two representations:

1. `Frame::parse` - strip the report-ID / len / flags prefix off a 129-byte hidapi report and return the flag byte + the valid data bytes.
2. `FrameReassembler` - the flag-driven state machine that appends frame data until a `LAST` or `COMPLETE` frame yields the full reassembled body.
3. `encode_message` - the inverse: append the 8-byte trailer to a protobuf payload, split into 126-byte chunks, wrap each as a 129-byte output report with the correct flags.
4. The `ReportId` and `Flags` value types that name the wire constants once, so no other module hard-codes `0x01`, `0x02`, `0x40`, `0x80`, `0xC0`, or `126`.

The framing layer is deliberately I/O-free. It does not know about `hidapi`, gzip, or the `CortexMessageType` enum. The transport layer (`100`) owns the hidapi read/write and the gzip; the message layer (`130`) owns the trailer strip and the tag decode. This layer is the shared pure-logic seam between them.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| Report layout `[report_id][len][flags][data...]`, 128-byte body + 1-byte report ID | Hardware-verified | Matches `pyquadcortex/docs/protocol.md` and re-confirmed by `cortex device version` round-trip on this machine |
| Input report ID `0x01`, output report ID `0x02` | Hardware-verified | Observed in both directions on a real Quad Cortex |
| `flags`: `0x40` FIRST, `0x80` LAST, `0xC0` COMPLETE, `0x00` MIDDLE | Hardware-verified | Matches `pyquadcortex/framing.py` and observed in captures |
| No sequence numbers, no total-length field; reassembly is flag-driven | Hardware-verified | Confirmed by `pyquadcortex` and by multi-frame RecallPreset pushes on this machine |
| `CHUNK_SIZE = 126` (128-byte body minus 2-byte `[len][flags]` prefix) | Hardware-verified | Matches `pyquadcortex` `CHUNK_SIZE = REPORT_SIZE - 2` |
| 8-byte trailer = `[message_type u16 LE][6 bytes: zeros from host, device-filled, ignored]` | Hardware-verified | Matches `pyquadcortex` `TRAILER_SIZE = 8`; the 6 trailing bytes are observed device-filled on input but have no documented meaning |
| A `FIRST` frame arriving mid-partial drops the stale buffer | Hardware-verified | Observed: the device interleaves unsolicited pushes (RecallPreset, parameter changes) with replies; a new `FIRST` mid-message is routine, not an error |
| `encode_message` output round-trips through `Frame::parse` + `FrameReassembler` + `Message::parse` | Hardware-verified | The `encode_then_decode_round_trips` unit test passes; `cortex device version` exercises the full path on hardware |

The `pyquadcortex` offline test suite is a conformance reference but not a substitute for a hardware smoke run. Agent-generated tests must not be the sole basis for accepting framing behaviour.

## User Stories

### Primary Users

The transport layer (`100`), the message/domain layers (`120`/`130`), the CLI (`200`), the MCP server (`300`), and the future Tauri backend - all of which call this layer's public API and none of which reimplement it.

### Stories

**As a** transport layer author
**I want** `Frame::parse` to validate the `len` byte against the available bytes and return a `Frame`
**So that** I can feed it to the reassembler without re-validating the wire prefix myself.

**As a** transport layer author
**I want** `FrameReassembler::feed` to return `Some(body)` only when a message is complete
**So that** I can run a read loop that yields one message per `Some` and keeps reading on `None`.

**As a** CLI author
**I want** `encode_message(message_type, payload)` to return ready-to-write 129-byte reports
**So that** I do not have to know about chunk sizes, flag bits, or the 8-byte trailer.

**As a** downstream crate consumer
**I want** the framing layer to compile with `default-features = false`
**So that** I can decode captures and run unit tests on a machine with no HID hardware.

**As a** maintainer
**I want** `ReportId` and `Flags` to be the single named source of the wire constants
**So that** no other module hard-codes `0x01`, `0x02`, `0x40`, `0x80`, `0xC0`, or `126`.

## Requirements

### Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | `ReportId` enum has two variants: `Input = 0x01` (device-to-host) and `Output = 0x02` (host-to-device, via `SET_REPORT`). `ReportId::from_raw(u8) -> Option<ReportId>` returns `None` for any other byte. | Must Have |
| FR-2 | `Flags(u8)` struct exposes constants `FIRST = 0x40`, `LAST = 0x80`, `COMPLETE = 0xC0`, `MIDDLE = 0x00` and predicates `is_first`, `is_last`, `is_complete`, `is_middle`. `is_complete` is exact equality to `0xC0`; `is_middle` is exact equality to `0x00`; `is_first`/`is_last` are bit tests. | Must Have |
| FR-3 | `Frame` struct has `flags: Flags` and `data: Vec<u8>` (the valid payload bytes, after the report-ID and `[len][flags]` prefix are stripped). | Must Have |
| FR-4 | `Frame::parse(&[u8]) -> Result<Frame>` reads byte 0 as report ID (not validated here), byte 1 as `len`, byte 2 as `flags`, and takes `report[3..3+len]` as `data`. Returns `Error::Framing` if the report is shorter than 3 bytes or if `3 + len` exceeds the report length. | Must Have |
| FR-5 | `FrameReassembler::new() -> FrameReassembler` creates a fresh state machine with an empty buffer and `in_progress = false`. `FrameReassembler` implements `Default` delegating to `new`. | Must Have |
| FR-6 | `FrameReassembler::feed(&Frame) -> Result<Option<Vec<u8>>>` returns `Ok(Some(body))` when a frame completes a message, `Ok(None)` when more frames are expected, and `Error::Framing` on a `MIDDLE` or `LAST` frame without a preceding `FIRST`. | Must Have |
| FR-7 | On a `COMPLETE` frame, `feed` resets the state machine and returns `Ok(Some(frame.data.clone()))` without touching the buffer. | Must Have |
| FR-8 | On a `FIRST` frame, `feed` resets the state machine, copies `frame.data` into the buffer, sets `in_progress = true`, and returns `Ok(None)`. A `FIRST` arriving mid-partial silently drops the stale buffer (the device interleaves pushes). | Must Have |
| FR-9 | On a `MIDDLE` frame, `feed` appends `frame.data` to the buffer and returns `Ok(None)`. Returns `Error::Framing` if `in_progress` is false. | Must Have |
| FR-10 | On a `LAST` frame, `feed` appends `frame.data`, takes the buffer as the returned body, sets `in_progress = false`, and returns `Ok(Some(body))`. Returns `Error::Framing` if `in_progress` is false. | Must Have |
| FR-11 | On any flag value other than `FIRST`, `LAST`, `COMPLETE`, or `MIDDLE`, `feed` returns `Error::Framing` with the offending byte. | Must Have |
| FR-12 | `FrameReassembler::reset()` clears the buffer and sets `in_progress = false`. Used on transport reconnect and by the transport's reset-on-FIRST rule. | Must Have |
| FR-13 | `encode_message(message_type: u16, payload: &[u8]) -> Vec<Vec<u8>>` appends the 8-byte trailer (`message_type.to_le_bytes()` ++ six zero bytes) to `payload`, splits the result into `CHUNK_SIZE`-byte chunks, and wraps each chunk as a 129-byte report: `[ReportId::Output][len][flags][chunk, zero-padded to 126]`. The first chunk carries `FIRST`, the last carries `LAST`, a single chunk carries `COMPLETE`, middle chunks carry `MIDDLE`. | Must Have |
| FR-14 | `CHUNK_SIZE = 126` is exported as a public constant, defined as `transport::HID_BODY_LEN - 2`. It is the single source of truth for the per-report data capacity; no other module hard-codes `126`. | Must Have |
| FR-15 | `encode_message` zero-pads each report to `transport::HID_REPORT_LEN` (129 bytes); the `len` byte reflects the valid chunk length, not the padded length. | Must Have |
| FR-16 | The framing module has no dependency on `hidapi`, `flate2`, or any async runtime. It depends only on `serde` (for `ReportId`/`Flags` derive) and the crate's own `transport` constants and `message::TRAILER_LEN`. | Must Have |
| FR-17 | The framing module compiles under `default-features = false` (i.e. with the `hid` feature off). It is not behind the `hid` feature gate. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | The framing layer is pure logic: no I/O, no globals, no blocking. All functions are total over their input domain and return `Result` or a value. | Code invariant |
| NFR-2 | `Frame::parse` and `FrameReassembler::feed` are `O(n)` in the frame data length and allocate only for the returned `Vec<u8>`. | Review-enforced |
| NFR-3 | `encode_message` pre-allocates the body `Vec` and the reports `Vec` from computed capacities; no re-allocation mid-loop. | Review-enforced |
| NFR-4 | `ReportId` and `Flags` are `Copy`, `Debug`, `Clone`, `PartialEq`, `Eq`, `Serialize`, `Deserialize`. `Frame` is `Debug`, `Clone`, `PartialEq`, `Eq`. | Code invariant |
| NFR-5 | Unit tests cover: single-frame round-trip, multi-frame in-order assembly, middle-without-first error, last-without-first error, flag decode of all four flag values, encode-then-decode symmetry, single-frame encode is `COMPLETE`, multi-frame encode sets `FIRST` on the first and `LAST` on the last. Tests run in `cargo test` with no hardware. | CI-enforced |
| NFR-6 | No other module hard-codes the report IDs (`0x01`/`0x02`), the flag bits (`0x40`/`0x80`/`0xC0`/`0x00`), the chunk size (`126`), or the trailer length (`8`). All go through this zone's or the transport/message zone's exported constants. | Review-enforced |
| NFR-7 | Leaf-crate discipline: the framing module depends only on `serde` and the crate's own `transport` and `message` modules. It pulls in no host app, no async runtime, and (behind the feature gate) no `hidapi`. | Architectural invariant |

## Acceptance Criteria

- [x] `ReportId::from_raw(0x01) == Some(ReportId::Input)` and `ReportId::from_raw(0x02) == Some(ReportId::Output)`; `ReportId::from_raw(0x00)` and `ReportId::from_raw(0x03)` return `None`.
- [x] `Flags(Flags::FIRST).is_first()` is true; `Flags(Flags::LAST).is_last()` is true; `Flags(Flags::COMPLETE).is_complete()` is true; `Flags(Flags::MIDDLE).is_middle()` is true.
- [x] `Frame::parse` on a 129-byte report with `len = 5` returns a `Frame` whose `data` is exactly 5 bytes.
- [x] `Frame::parse` on a report shorter than 3 bytes returns `Error::Framing`.
- [x] `Frame::parse` on a report whose declared `len` exceeds the available bytes returns `Error::Framing`.
- [x] `FrameReassembler::feed` on a `COMPLETE` frame returns `Ok(Some(data))` and leaves the state machine idle.
- [x] `FrameReassembler::feed` on a `FIRST` + `MIDDLE` + `LAST` sequence returns `Ok(None)`, `Ok(None)`, `Ok(Some(concatenated))`.
- [x] `FrameReassembler::feed` on a `MIDDLE` or `LAST` frame with no preceding `FIRST` returns `Error::Framing`.
- [x] `FrameReassembler::feed` on a `FIRST` frame arriving mid-partial silently drops the stale buffer and starts a new message (returns `Ok(None)`).
- [x] `encode_message(10, b"short")` returns one report whose parsed `Frame` has `is_complete() == true`.
- [x] `encode_message(10, &[0xAB; CHUNK_SIZE + 10])` returns more than one report; the first parsed `Frame` has `is_first()` and not `is_last()`; the last parsed `Frame` has `is_last()` and not `is_first()`.
- [x] `encode_message` output round-trips through `Frame::parse` + `FrameReassembler` + `Message::parse` to the original `message_type` and `payload` (the `encode_then_decode_round_trips` test).
- [x] `cargo build --no-default-features -p cortex-rs` compiles `framing.rs` with no `hidapi` in the dependency graph.
- [x] The 10 unit tests pass under `cargo test -p cortex-rs` with no hardware.
- [ ] A manual hardware smoke runbook exists for this layer (owned by `500-dx-tooling`).

## Non-Goals

- **USB HID I/O, the write STALL, and exclusive access.** Owned by zone `100-transport` (`transport.rs`). This layer never touches `hidapi`.
- **Protobuf decode and the `CortexMessageType` tag enum.** Owned by zones `120-proto-schema` and `130-domain-model` (`message.rs`). This layer appends and yields the raw 8-byte trailer; it does not interpret the tag.
- **Frame-level gzip decompression.** Owned by `100-transport` (`Transport::request`); the framing layer operates on raw frames, not decompressed bodies.
- **Field-level gzip (inside protobuf `bytes` fields).** Owned by the domain layer (`130`).
- **`request_id` correlation and unsolicited-push dispatch.** Owned by the planned session layer (`140-session`).
- **Background RX thread.** A future concern for the session layer; the reassembler is a synchronous state machine a caller drives.
- **Nano Cortex-specific framing differences (if any).** The protocol shape is shared; Nano-specific behaviour is provisional until verified against real hardware.

## Dependencies

- **`serde`** - `ReportId` and `Flags` derive `Serialize`/`Deserialize` for the IPC/CLI surface. No other external crate.
- **Zone `100-transport`** - `transport::HID_BODY_LEN` (128) and `transport::HID_REPORT_LEN` (129), the constants `CHUNK_SIZE` and the report buffer size derive from. The dependency arrow is framing -> transport constants, not framing -> transport I/O.
- **Zone `130-domain-model`** - `message::TRAILER_LEN` (8), the constant `encode_message` uses to size the trailer. The dependency arrow is framing -> message constant, not framing -> message decode.
- **Consuming zones**: `100-transport` (`Frame::parse`, `FrameReassembler`, `encode_message`), `130-domain-model` (indirectly, via the reassembled body), `200-cli` and `300-mcp` (via `encode_message`).

## Protocol Provenance & Attribution

The framing logic in `crates/cortex-rs/src/framing.rs` is a Rust port of
`pyquadcortex/pyquadcortex/framing.py` (MIT, (c) 2026 Stokes), used under the
terms of the MIT license. The wire format constants, the flag semantics, the
flag-driven reassembly state machine, and the 8-byte trailer layout all
originate in that module and are re-verified against a real Quad Cortex on
this machine (CorOS 4.0.1, firmware `d14e`). The derivation is recorded in
`NOTICE` and `THIRD-PARTY-NOTICES.md`.

The Python reference is the source of truth for the wire format; this Rust
port is the source of truth for the typed API. The two are kept in lock-step
by the `pyquadcortex` offline test suite as a conformance reference.

## Glossary

| Term | Definition |
| --- | --- |
| HID report | The 129-byte unit at the hidapi boundary: 1-byte report ID + 128-byte body |
| HID body | The 128-byte payload portion of a report, excluding the 1-byte report ID |
| Frame | A parsed HID report with the report-ID / len / flags prefix stripped: `Flags` + valid `data` bytes |
| Report ID | `0x01` for input (device-to-host), `0x02` for output (host-to-device) |
| Flags | The byte encoding where this frame sits in a multi-frame message: `0x40` FIRST, `0x80` LAST, `0xC0` COMPLETE, `0x00` MIDDLE |
| Chunk | A `CHUNK_SIZE` (126)-byte slice of the trailer-appended body; one per report |
| Trailer | The 8-byte suffix on a reassembled message: `[message_type u16 LE][6 bytes: zeros from host, device-filled, ignored]` |
| Reassembler | The flag-driven state machine that appends frame data until a `LAST`/`COMPLETE` frame yields the full body |
| Logical message | `message_type: u16` + `payload: &[u8]` - the input to `encode_message` and the output of `Message::parse` |
| Flag-driven reassembly | No sequence numbers, no total-length field; a `FIRST` starts a buffer, `MIDDLE` appends, `LAST`/`COMPLETE` yields |
| Interleaved push | An unsolicited device-to-host message (RecallPreset, parameter change) arriving mid-reply; a new `FIRST` frame drops the stale buffer |
| Hardware-verified | Confirmed against a real Quad Cortex on this machine (CorOS 4.0.1, firmware `d14e`) |
| Provisional | Not yet verified against real hardware by this project; may work but is not confirmed |

## Agent Entry Map

| Owned file | Local anchors | Key functions / types | Tests | Dependencies | Out of scope |
| --- | --- | --- | --- | --- | --- |
| `crates/cortex-rs/src/framing.rs` | [FR-1]-[FR-17], [NFR-1]-[NFR-7] | `ReportId`, `Flags`, `Frame`, `FrameReassembler`, `encode_message`, `CHUNK_SIZE` | 10 in-file unit tests (round-trip, multi-frame, errors, encode/decode symmetry) | `serde`, `crate::transport::HID_BODY_LEN`/`HID_REPORT_LEN`, `crate::message::TRAILER_LEN` | hidapi I/O, gzip, protobuf decode, `CortexMessageType` enum, session correlation |