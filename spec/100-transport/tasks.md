---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["transport", "tasks", "roadmap"]
spec: spec.md
design: design.md
---

# cortex-rs - Transport Tasks

> Implementation and verification tasks for the USB HID transport layer. Phase 1-2 are done; phase 3 is the immediate next slice; phase 4 is the future session-layer handoff.

## Phase 1 - Core transport (done)

### 1.1 `Transport` struct and `open`

- [x] Define `Transport { device: hidapi::HidDevice }`.
- [x] Implement `Transport::open(DeviceKind) -> Result<Transport>`: enumerate bus, match VID:PID, `open_path`, return `Error::DeviceNotFound` on miss.
- [x] Gate the module behind the `hid` feature; gate the `Hid` and `DeviceNotFound` error variants likewise.

### 1.2 `write` with STALL swallow

- [x] Implement `Transport::write(&[u8]) -> Result<()>`: split into 128-byte frames, set FIRST/LAST/COMPLETE/MIDDLE flags, `let _ = self.device.write(&report)`, return `Ok(())`.
- [x] Document the benign write STALL in the doc-comment and in `lib.rs` protocol invariants.

### 1.3 `read` with timeout-as-liveness

- [x] Implement `Transport::read(Duration) -> Result<Vec<u8>>`: `read_timeout` into a 129-byte buffer, return `Error::ReadTimeout` on `n == 0`.
- [x] Clamp the `Duration` to `i32` milliseconds for the hidapi call.

### 1.4 Constants

- [x] Export `HID_BODY_LEN = 128`, `HID_REPORT_LEN = 129`, `DEFAULT_READ_TIMEOUT = 2s`.

## Phase 2 - Synchronous request/response (done)

### 2.1 `Transport::request`

- [x] Implement `Transport::request(message_type, payload, timeout) -> Result<Message>`: `encode_message`, write each report (swallow STALL), deadline-shrinking read loop, `FrameReassembler`, `Message::parse`.
- [x] Reset the reassembler when a new `FIRST` frame arrives mid-partial (the device interleaves pushes).
- [x] Return the first reassembled message (no `request_id` correlation).

### 2.2 Frame-level gzip decompression

- [x] Detect `msg.body.starts_with(&[0x1f, 0x8b])` after trailer strip and `flate2`-decompress into `msg.body`.

## Phase 3 - Verification and docs (planned, next)

### 3.1 Hardware smoke runbook

- [ ] Write a manual hardware smoke runbook for this layer (owned by `500-dx-tooling`): `cortex version` round-trip, write-STALL observation, read-timeout-on-unplug, gzip-decompressed RecallPreset push.
- [ ] Record the verified device node (`/dev/hidraw7`), CorOS version (`4.0.1`), and firmware (`d14e`) in the runbook and the spec verification table.

### 3.2 Error messages and README

- [ ] Confirm `Error::DeviceNotFound` message points at the udev rule and the README setup section.
- [ ] Confirm the README udev rule and setup walkthrough are current (VID:PID `152a:880a`, `70-quadcortex.rules`, `uaccess` tag).

### 3.3 Conformance cross-check

- [ ] Cross-check `Transport::write` frame layout against `pyquadcortex` framing (report ID, len byte, flags byte, data).
- [ ] Cross-check the gzip magic detection and decompression against a RecallPreset capture from `pyquadcortex`.

## Phase 4 - Future (session layer handoff)

### 4.1 Background RX thread

- [ ] Expose a `read`-driven hook for the session layer (`140-session`) to own the read loop in a background thread.
- [ ] Keep the synchronous `Transport::request` path for the CLI's fire-and-forget commands; the background thread is additive, not a replacement.

### 4.2 Correlation and broadcast

- [ ] Hand off `request_id` correlation and unsolicited-push broadcasting to the session layer. The transport returns the first reassembled message; the session layer correlates.

### 4.3 Protocol-version probe

- [ ] Surface a protocol-version probe in the session layer (no version field on the wire; a CorOS update can silently break things). The transport itself does not assume a version.

### 4.4 Nano Cortex hardware verification

- [ ] Verify `DeviceKind::NanoCortex` product ID against a real Nano Cortex; replace the `0xFFFF` placeholder in `device.rs`.
- [ ] Move Nano Cortex rows in the spec verification table from "Provisional" to "Hardware-verified" once confirmed.

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1-1.4, 2.1-2.2 | Implemented `Transport` (open/write/read/request) | `crates/cortex-rs/src/transport.rs` | [x] | [x] |
| 2026-08-01 | 1.2 | Wrote spec/design/tasks for this zone | `spec/100-transport/{spec,design,tasks}.md` | [x] | [x] |