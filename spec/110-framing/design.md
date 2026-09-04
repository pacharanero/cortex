---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["framing", "design", "frame", "reassembler", "encode", "report-id", "flags", "chunk-size"]
spec: spec.md
---

# cortex-rs - Framing Design

> Design for the pure-logic frame codec: `ReportId`, `Flags`, `Frame`, `FrameReassembler`, and `encode_message`. The interesting parts are not the parsing itself but the two behaviours that make Cortex Control unlike a length-prefixed protocol: flag-driven reassembly with no total-length field, and the trailer-tagged envelope where the message type lives at the *end* of the reassembled body, not the start.

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this layer sits in (`[Flow.Framing]`).
- [100-transport design](../100-transport/design.md) [DES-REQUEST] - the `request` path that drives the reassembler and applies the reset-on-FIRST rule.
- [pyquadcortex `framing.py`](https://github.com/stokes-audio/pyquadcortex/blob/main/pyquadcortex/framing.py) - the MIT-licensed Python reference this is ported from.
- Owned source: `crates/cortex-rs/src/framing.rs`.

## [DES-FRAME] Frame Layout and Parsing

### Wire layout

A 129-byte hidapi report is laid out as:

```text
[report_id u8][len u8][flags u8][data: up to 126 bytes, zero-padded]
 \___ byte 0 ___/ \_ 1 _/ \_ 2 _/ \____ bytes 3..3+len ____/
```

- `report_id` is `0x01` for input (device-to-host) or `0x02` for output (host-to-device). Validated by the caller (transport), not by `Frame::parse` - the frame codec is report-ID-agnostic so it can be used in tests with arbitrary report IDs.
- `len` is the number of *valid* data bytes in this report, excluding the report-ID / len / flags bytes themselves. Non-final fragments always carry a full 126 bytes; the final fragment's `len` says how many of its data bytes are meaningful (the rest is padding or stale buffer).
- `flags` encodes where this frame sits in a multi-frame message: `0x40` FIRST, `0x80` LAST, `0xC0` COMPLETE, `0x00` MIDDLE.
- `data` is the valid payload; bytes after `3 + len` are padding and are discarded by `Frame::parse`.

### Design choice: `Frame::parse` validates `len` against the report, not against the protocol

`Frame::parse` takes a `&[u8]` and checks two things: the report is at least 3 bytes long, and `3 + len` does not exceed the report length. It does not check the report ID (the caller's job) or the flag value (the reassembler's job). This keeps `Frame::parse` a thin wire-to-struct conversion with no protocol knowledge:

```rust
pub fn parse(report: &[u8]) -> crate::Result<Self> {
    if report.len() < 3 {
        return Err(crate::Error::Framing(format!(
            "report too short: {} bytes (need at least 3)",
            report.len()
        )));
    }
    let len = usize::from(report[1]);
    let flags = Flags(report[2]);
    let data_end = 3 + len;
    if data_end > report.len() {
        return Err(crate::Error::Framing(format!(
            "declared len {len} exceeds available bytes ({})",
            report.len() - 3
        )));
    }
    Ok(Self { flags, data: report[3..data_end].to_vec() })
}
```

### Design choice: `Flags` is a `u8` newtype, not an enum

The wire uses four exact byte values, with `COMPLETE = FIRST | LAST = 0xC0`. A `u8` newtype preserves an unknown byte for a useful framing error while the predicates accept only the four observed values; reserved or stray bits never turn an unknown value into FIRST or LAST:

```rust
pub struct Flags(pub u8);
impl Flags {
    pub const FIRST: u8 = 0x40;
    pub const LAST: u8 = 0x80;
    pub const COMPLETE: u8 = 0xC0;
    pub const MIDDLE: u8 = 0x00;
    pub const fn is_first(self) -> bool {
        self.0 == Self::FIRST || self.0 == Self::COMPLETE
    }
    pub const fn is_last(self) -> bool {
        self.0 == Self::LAST || self.0 == Self::COMPLETE
    }
    pub const fn is_complete(self) -> bool { self.0 == Self::COMPLETE }
    pub const fn is_middle(self) -> bool { self.0 == Self::MIDDLE }
}
```

`Flags(0xC0).is_first()` and `Flags(0xC0).is_last()` are both true because a `COMPLETE` frame is conceptually both first and last, but all predicates use exact-value comparisons. For example, `0x41`, `0x81`, and `0xC1` are unknown rather than FIRST, LAST, or COMPLETE. The reassembler checks `is_complete` first so a single-frame message takes the direct path before the broader exact `is_first` predicate.

### Design choice: `ReportId::from_raw` returns `Option`, not `Result`

`from_raw` is a total function over `u8` that returns `Some(Input)` for `0x01`, `Some(Output)` for `0x02`, and `None` for anything else. `Option` is the honest return type: there is no error message to attach, just "recognised or not". The transport layer calls this when classifying a report; a `None` there is a protocol violation, not a framing error.

### Alternatives considered

- **Parse the report ID in `Frame::parse` and store it on `Frame`.** Rejected: the frame codec is report-ID-agnostic so it can be used in tests with arbitrary report IDs, and the transport already knows which direction it read from. Storing the report ID on `Frame` would couple the frame to the transport direction.
- **Make `Flags` an enum with `First`, `Last`, `Complete`, `Middle` variants.** Rejected: the newtype preserves the offending raw byte for diagnostics and serialisation while exact predicates still enforce the closed set of observed values.

## [DES-REASSEMBLY] Flag-Driven Reassembly

### Behaviour

There are no sequence numbers and no total-length field on the wire. Reassembly is driven purely by the flag byte: a `FIRST` frame starts a buffer, `MIDDLE` frames append, a `LAST` frame appends and yields the whole body, a `COMPLETE` frame yields its data directly. This is the `pyquadcortex` design, ported faithfully.

### State machine

`FrameReassembler` holds two fields:

```rust
pub struct FrameReassembler {
    buffer: Vec<u8>,
    in_progress: bool,
}
```

`feed` dispatches on the flags in a fixed order:

1. **`COMPLETE`** (`0xC0`): reset, return `Ok(Some(frame.data.clone()))`. No buffer involvement - a single-frame message has no partial state.
2. **`FIRST`** (`0x40`): reset, copy `frame.data` into the buffer, set `in_progress = true`, return `Ok(None)`. The session records whether this abandoned a partial and invalidates state continuity.
3. **`MIDDLE`** (`0x00`): if `!in_progress`, return `Error::Framing("middle frame without a preceding first frame")`; otherwise append, return `Ok(None)`.
4. **`LAST`** (`0x80`): if `!in_progress`, return `Error::Framing("last frame without a preceding first frame")`; otherwise append, take the buffer as the body, set `in_progress = false`, return `Ok(Some(body))`.
5. **Any other flag value**: return `Error::Framing("unknown flags byte: {:#04x}")`.

The `is_complete` check comes *before* `is_first` because `Flags(0xC0)` satisfies both exact predicates. Ordering the dispatch this way makes a single-frame message a single-step path.

### Design choice: `FIRST` mid-partial drops the stale buffer

The wire has no message identifier with which to resume an interrupted body. `feed` therefore resynchronises on every `FIRST`, whether or not `in_progress` is true:

```rust
if frame.flags.is_first() {
    self.reset();
    self.buffer.extend_from_slice(&frame.data);
    self.in_progress = true;
    return Ok(None);
}
```

This decoder recovery rule follows `pyquadcortex`. It does not prove the dropped body was harmless: the session marks a stream gap and invalidates cached state before continuing.

### Design choice: `MIDDLE`/`LAST` without `FIRST` is an error

If the reassembler is idle (`in_progress == false`) and receives a `MIDDLE` or `LAST` frame, that means a `FIRST` was lost or the caller started feeding from the middle of a stream. `feed` returns `Error::Framing` rather than silently starting a buffer, because a `MIDDLE` frame does not carry the start of a message and a `LAST` frame alone is a truncated message. The caller's response is to reset and wait for the next `FIRST` - which the transport's read loop does by treating the error as non-fatal and continuing.

### Design choice: `reset()` is public

`reset()` is called in three places: internally on `COMPLETE` and `FIRST`, by the transport on reconnect, and by the transport's `request` loop as belt-and-braces on a new `FIRST`. Making it public lets the transport own its reconnect policy without the reassembler having to expose a "reconnect" method that is just `reset` with a different name.

### Design choice: `feed` takes `&Frame`, not `Frame`

`feed` borrows the frame and clones its data into the buffer. Taking `Frame` by value would consume it and force the caller to clone if it wants to inspect the frame after feeding (the transport does not, but tests do). Borrowing is the cheaper default; the clone happens inside `feed` either way.

### Alternatives considered

- **Track a total-length or frame-count field.** Rejected: the wire has none. Adding one to the state machine would be dead state - it could never be populated from the wire and would invite bugs where the caller sets it wrong.
- **Terminate the RX loop on `FIRST` mid-partial.** Rejected: the decoder can safely resynchronise. State continuity is invalidated at session level instead.
- **Return the partial buffer on `reset()` so the caller can salvage it.** Rejected: a partial message with no `LAST` is not useful to any caller, and salvaging it would invite the caller to treat truncated data as complete. Drop it silently.
- **Use a `nom`-style parser combinator.** Rejected: the state machine is four branches and a buffer, not a grammar. A combinator would add a dependency and obscure the logic.

## [DES-ENCODE] Message Encoding

### Envelope: trailer-tagged, not header-tagged

The logical message is `protobuf body ++ 8-byte trailer`. The trailer is:

```text
[message_type u16 LE][6 bytes: zeros from host, device-filled on input, ignored]
```

The message-type tag lives in the *trailer*, not a header. This is the `pyquadcortex` design and is confirmed on hardware. The host sends six zero bytes after the `u16` tag; the device fills them on input replies with values whose meaning is not documented in the recovered schema - this client ignores them. `Message::parse` (zone `130`) strips the trailer and reads the tag; `encode_message` (this zone) appends it.

### `encode_reports` and `encode_message`

`encode_reports(geometry, body)` owns only HID framing: it chunks an already-formed application body at `geometry.data_capacity()`, assigns flags, and pads reports to `geometry.report_len()`. `encode_message` remains the Quad compatibility wrapper that forms `protobuf ++ 8-byte trailer` and delegates to `encode_reports(HidReportGeometry::QUAD_CORTEX, body)`. The separate Nano codec forms its four-byte-footer bodies and calls the same raw encoder.

### `encode_message` algorithm

```rust
pub fn encode_message(message_type: u16, payload: &[u8]) -> Vec<Vec<u8>> {
    // 1. Build the full body: protobuf payload ++ 8-byte trailer.
    let mut body = Vec::with_capacity(payload.len() + crate::message::TRAILER_LEN);
    body.extend_from_slice(payload);
    body.extend_from_slice(&message_type.to_le_bytes());
    body.extend_from_slice(&[0u8; crate::message::TRAILER_LEN - 2]);

    // 2. Split into 126-byte chunks.
    let chunk_count = body.len().div_ceil(CHUNK_SIZE);
    let mut reports = Vec::with_capacity(chunk_count);

    for (i, chunk) in body.chunks(CHUNK_SIZE).enumerate() {
        let is_first = i == 0;
        let is_last = i == chunk_count - 1;
        let flags = if is_first && is_last { Flags::COMPLETE }
                    else if is_first          { Flags::FIRST }
                    else if is_last           { Flags::LAST }
                    else                      { Flags::MIDDLE };

        // 3. Wrap each chunk as a 129-byte report.
        let mut report = vec![0u8; HID_REPORT_LEN];
        report[0] = ReportId::Output as u8;
        report[1] = chunk.len() as u8;
        report[2] = flags;
        report[3..3 + chunk.len()].copy_from_slice(chunk);
        reports.push(report);
    }
    reports
}
```

The reports are zero-padded to `HID_REPORT_LEN` (129 bytes). The `len` byte reflects the valid chunk length, not the padded length - the final chunk's `len` is whatever is left over, and the device reads exactly `len` bytes of it.

### Design choice: chunk the trailer-appended body, not the payload

The 8-byte trailer is part of the reassembled body, so it is part of what gets chunked. A payload of exactly `CHUNK_SIZE` bytes produces a 134-byte body (126 + 8), which is two reports: a `FIRST` of 126 bytes and a `LAST` of 8 bytes. This matches the `pyquadcortex` `encode_message` and is the only layout that round-trips through the reassembler to the original `message_type` + `payload`.

### Design choice: `ReportId::Output` is the only report ID `encode_message` uses

`encode_message` produces host-to-device reports, which are always output reports (`0x02`). Input reports (`0x01`) are only produced by the device. Hard-coding `ReportId::Output` in the encoder is correct and avoids a caller passing the wrong direction.

### Design choice: pre-allocate from computed capacities

`body` is allocated with `payload.len() + TRAILER_LEN`; `reports` is allocated with `body.len().div_ceil(CHUNK_SIZE)`. No re-allocation happens inside the loop. This is a micro-optimisation that costs nothing and keeps the encoder deterministic in allocation count.

### Design choice: `len` is `chunk.len() as u8`

`chunk.len()` is at most `CHUNK_SIZE = 126 < 256`, so the `as u8` cast is safe. The `#[allow(clippy::cast_possible_truncation)]` attribute documents why the cast is correct rather than silencing the lint blindly.

### Alternatives considered

- **Put the message type in a header, not the trailer.** Rejected: the wire puts it in the trailer. `Message::parse` (zone `130`) reads it from the trailer; `encode_message` appends it. Any other layout would not round-trip.
- **Chunk the payload, then append the trailer to the last chunk.** Rejected: the trailer is part of the reassembled body, so it must be inside the chunking, not appended after it. Otherwise the reassembled body would be `payload ++ trailer` only if the payload length happens to align with a chunk boundary.
- **Make `encode_message` return an iterator, not a `Vec`.** Rejected: the transport writes each report synchronously and needs the full set up front. An iterator would add complexity for no gain on a synchronous path. The session layer (`140`) may revisit this if it needs streaming encode.
- **Let the caller choose the report ID.** Rejected: host-to-device reports are always `0x02`. Exposing the report ID as a parameter would be a footgun - the only correct value is `ReportId::Output`.

## [DES-CONSTS] Constants

| Constant | Value | Purpose |
| --- | --- | --- |
| `ReportId::Input` | `0x01` | Device-to-host report ID. Used by the transport's read path. |
| `ReportId::Output` | `0x02` | Host-to-device report ID (via `SET_REPORT`). Used by `encode_message`. |
| `Flags::FIRST` | `0x40` | First frame of a multi-frame message. |
| `Flags::LAST` | `0x80` | Last frame of a multi-frame message. |
| `Flags::COMPLETE` | `0xC0` | Single-frame message (`FIRST \| LAST`). |
| `Flags::MIDDLE` | `0x00` | Middle frame of a multi-frame message. |
| `HidReportGeometry::QUAD_CORTEX` | `128` / `129` / `126` | Quad body, report and data capacity. |
| `HidReportGeometry::NANO_CORTEX` | `64` / `65` / `62` | Nano body, report and data capacity. |
| `HID_BODY_LEN` / `HID_REPORT_LEN` / `CHUNK_SIZE` | `128` / `129` / `126` | Existing Quad compatibility constants. |

The geometry values are the single source of truth for device report dimensions. `message::TRAILER_LEN` (8) remains a Quad application-envelope constant owned by zone 130.

## [DES-TEST] Testing Strategy

The framing unit tests cover the behaviour, not the implementation:

| Test | What it asserts |
| --- | --- |
| `complete_frame_round_trips` | A `COMPLETE` frame yields its data immediately via the reassembler. |
| `multi_frame_assembles_in_order` | `FIRST` + `MIDDLE` + `LAST` concatenates in order and yields the full body. |
| `middle_without_first_errors` | A `MIDDLE` frame with no preceding `FIRST` returns `Error::Framing`. |
| `last_without_first_errors` | A `LAST` frame with no preceding `FIRST` returns `Error::Framing`. |
| `flags_decode_correctly` | All four flag predicates return true for their respective constants. |
| `encode_then_decode_round_trips` | `encode_message` output, fed through `Frame::parse` + `FrameReassembler` + `Message::parse`, recovers the original `message_type` and `payload`. The end-to-end symmetry test. |
| `encode_single_frame_is_complete` | A short payload encodes to one report whose parsed `Frame` is `COMPLETE`. |
| `encode_multi_frame_sets_flags` | A payload larger than `CHUNK_SIZE` encodes to multiple reports; the first is `FIRST`-but-not-`LAST`, the last is `LAST`-but-not-`FIRST`. |
| `closed_geometries_have_measured_dimensions` | Quad and Nano dimensions match their report descriptors. |
| `raw_report_boundaries_follow_selected_geometry` | 126/127 and 62/63 bytes split at the correct device boundary. |
| `nano_state_sized_body_reassembles_from_nine_reports` | A fictional 546-byte body reproduces the measured Nano nine-report geometry and reassembles exactly. |

These run in `cargo test -p cortex-rs` with no hardware. They are the CI gate for this zone. Hardware verification is a manual runbook owned by `500-dx-tooling` and is not a substitute for the unit tests - the unit tests guard the pure logic, the runbook guards the wire format.

## [DES-LIMITS] Known Limitations

- **No detection of a lost `MIDDLE` or `LAST` frame.** Without sequence numbers, a dropped `MIDDLE` frame produces a shorter-but-plausible reassembled body that `Message::parse` may accept. The only signal is a protobuf decode failure downstream (zone `130`), not a framing error here. This is a wire-protocol limitation, not a code defect.
- **The pure reassembler is uncapped.** The live session enforces a 1 MiB body cap and invalidates continuity on breach; offline callers must provide their own input bound.
- **No streaming encode.** `encode_message` returns a `Vec<Vec<u8>>` holding all reports. For the CLI's short commands this is fine; a streaming encoder for very large writes is a future concern for the session layer (`140`).
- **`Frame::parse` does not validate the report ID.** The caller (transport) is responsible for classifying the report direction. A test feeding a report with an arbitrary report ID byte to `Frame::parse` will succeed; this is deliberate.
- **Nano Cortex application coverage remains partial.** Geometry and raw framing are shared, while the implemented Nano codec parses its four-byte footer separately from `Message::parse`'s Quad-specific eight-byte trailer. Typed state plus hardware-verified amp, Gate reduction, bypass and raw FX parameter operations are implemented; wider application operations remain provisional. A timeout or malformed response causes the held path to discard and reopen the transport before another request, invalidate the old generation, and require a fresh state read before serving live data. Quad request/session entry points reject Nano before USB I/O so the envelopes cannot be conflated.
