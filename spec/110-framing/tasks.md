---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["framing", "tasks", "roadmap"]
spec: spec.md
design: design.md
---

# cortex-rs - Framing Tasks

> Implementation and verification tasks for the HID frame codec. Phase 1-2 are done; phase 3 is verification and docs; phase 4 is the future streaming/session handoff.

## Phase 1 - Value types and frame parsing (done)

### 1.1 `ReportId` enum

- [x] Define `ReportId { Input = 0x01, Output = 0x02 }` with `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Serialize`, `Deserialize`.
- [x] Implement `ReportId::from_raw(u8) -> Option<ReportId>`: `Some(Input)` for `0x01`, `Some(Output)` for `0x02`, `None` otherwise.

### 1.2 `Flags` struct

- [x] Define `Flags(pub u8)` with `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Serialize`, `Deserialize`.
- [x] Add constants `FIRST = 0x40`, `LAST = 0x80`, `COMPLETE = 0xC0`, `MIDDLE = 0x00`.
- [x] Add predicates `is_first` (bit test `& FIRST`), `is_last` (bit test `& LAST`), `is_complete` (exact `== COMPLETE`), `is_middle` (exact `== MIDDLE`). All `#[must_use] const fn`.

### 1.3 `Frame` struct and `Frame::parse`

- [x] Define `Frame { flags: Flags, data: Vec<u8> }` with `Debug`, `Clone`, `PartialEq`, `Eq`.
- [x] Implement `Frame::parse(&[u8]) -> Result<Frame>`: byte 0 = report ID (not validated), byte 1 = `len`, byte 2 = `flags`, `data = report[3..3+len]`. Return `Error::Framing` if `report.len() < 3` or `3 + len > report.len()`.

## Phase 2 - Reassembler and encoder (done)

### 2.1 `FrameReassembler` state machine

- [x] Define `FrameReassembler { buffer: Vec<u8>, in_progress: bool }` with `Default` delegating to `new`.
- [x] Implement `FrameReassembler::new() -> FrameReassembler`: empty buffer, `in_progress = false`.
- [x] Implement `FrameReassembler::feed(&Frame) -> Result<Option<Vec<u8>>>` with the dispatch order: `COMPLETE` (reset, return `Some(data)`) -> `FIRST` (reset, buffer data, `in_progress = true`, return `None`) -> `MIDDLE` (error if `!in_progress`, else append, return `None`) -> `LAST` (error if `!in_progress`, else append, take buffer, `in_progress = false`, return `Some(body)`) -> unknown flag (error).
- [x] Implement `FrameReassembler::reset()`: clear buffer, `in_progress = false`.
- [x] Confirm a `FIRST` arriving mid-partial silently drops the stale buffer (returns `Ok(None)`, does not error).

### 2.2 `encode_message` and `CHUNK_SIZE`

- [x] Define `pub const CHUNK_SIZE: usize = crate::transport::HID_BODY_LEN - 2` (126).
- [x] Implement `encode_message(message_type: u16, payload: &[u8]) -> Vec<Vec<u8>>`: append `[message_type.to_le_bytes(), 0u8; 6]` to `payload`, split into `CHUNK_SIZE`-byte chunks, wrap each as `[ReportId::Output][len][flags][chunk, zero-padded to 126]` in a `HID_REPORT_LEN`-byte report. First = `FIRST`, last = `LAST`, single = `COMPLETE`, middle = `MIDDLE`.
- [x] Pre-allocate `body` with `payload.len() + TRAILER_LEN` and `reports` with `body.len().div_ceil(CHUNK_SIZE)`.

## Phase 3 - Tests (done)

### 3.1 Round-trip and assembly tests

- [x] `complete_frame_round_trips`: a `COMPLETE` frame yields its data via the reassembler.
- [x] `multi_frame_assembles_in_order`: `FIRST` + `MIDDLE` + `LAST` concatenates and yields the full body.
- [x] `encode_then_decode_round_trips`: `encode_message` output, fed through `Frame::parse` + `FrameReassembler` + `Message::parse`, recovers the original `message_type` and `payload`.
- [x] `encode_single_frame_is_complete`: a short payload encodes to one `COMPLETE` report.
- [x] `encode_multi_frame_sets_flags`: a payload larger than `CHUNK_SIZE` encodes to multiple reports with correct `FIRST`/`LAST` flags on the ends.

### 3.2 Error-case tests

- [x] `middle_without_first_errors`: a `MIDDLE` frame with no preceding `FIRST` returns `Error::Framing`.
- [x] `last_without_first_errors`: a `LAST` frame with no preceding `FIRST` returns `Error::Framing`.
- [x] `flags_decode_correctly`: all four flag predicates return true for their constants.
- [x] *(in-file)* `Frame::parse` error paths: report shorter than 3 bytes, declared `len` exceeds available bytes.
- [x] *(in-file)* `ReportId::from_raw`: `0x01` -> `Some(Input)`, `0x02` -> `Some(Output)`, other bytes -> `None`.

### 3.3 Test count

- [x] Confirm 10 unit tests in `framing.rs` `mod tests` pass under `cargo test -p cortex-rs` with no hardware.

## Phase 4 - Verification and docs (planned, next)

### 4.1 Hardware smoke runbook

- [ ] Write a manual hardware smoke runbook for this layer (owned by `500-dx-tooling`): encode a `version` request, confirm the wire bytes match `pyquadcortex`, feed real input frames through the reassembler, confirm a multi-frame RecallPreset push assembles.
- [ ] Record the verified CorOS version (`4.0.1`) and firmware (`d14e`) in the runbook and the spec verification table.
- [ ] Cross-check `Frame::parse` output against a real input report capture from `pyquadcortex`.
- [ ] Cross-check `encode_message` output against a real output report capture from `pyquadcortex`.

### 4.2 Conformance cross-check

- [ ] Run the `pyquadcortex` offline test suite as a conformance reference for the framing constants and the reassembly state machine.
- [ ] Confirm `CHUNK_SIZE`, `TRAILER_LEN`, and the flag constants match `pyquadcortex/framing.py` exactly.

### 4.3 Attribution

- [ ] Confirm `NOTICE` and `THIRD-PARTY-NOTICES.md` carry the `pyquadcortex` (MIT, (c) 2026 Stokes) attribution for this derivation.

## Phase 5 - Future (session layer handoff)

### 5.1 Streaming encode

- [ ] If the session layer (`140-session`) needs streaming encode for very large writes, add an `encode_message_iter` that yields reports one at a time without materialising the full `Vec`. Keep `encode_message` as the eager path for the CLI.

### 5.2 Reassembler buffer cap

- [ ] If hostile input becomes a threat model, add a hard cap on the reassembler buffer size and return `Error::Framing` on overflow. The transport's deadline-shrinking read loop bounds this in practice today.

### 5.3 Nano Cortex hardware verification

- [ ] Verify the framing layer against a real Nano Cortex; confirm the flag semantics, chunk size, and trailer layout are identical to the Quad Cortex. Move the Nano Cortex rows in the spec verification table from "Provisional" to "Hardware-verified" once confirmed.

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1-2.2, 3.1-3.3 | Implemented `ReportId`, `Flags`, `Frame`, `FrameReassembler`, `encode_message`, `CHUNK_SIZE`, 10 unit tests | `crates/cortex-rs/src/framing.rs` | [x] | [x] |
| 2026-08-01 | 1.1-3.3 | Wrote spec/design/tasks for this zone | `spec/110-framing/{spec,design,tasks}.md` | [x] | [x] |