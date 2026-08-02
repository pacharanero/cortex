---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["session", "connect", "keepalive", "correlation", "broadcast", "provisional"]
spec: spec.md
design: design.md
---

# 140 Session - Tasks

> Implementation checklist for the session layer. Phases 1-6 are implemented; Phase 7 (hardware verification) is outstanding, so the whole zone remains **provisional**. Protocol facts are hardware-verified via pyquadcortex; the Rust implementation is unverified against a device from this crate's own code.

## Divergences from the plan (recorded, not silently absorbed)

Two things differ from what Phases 1-3 below describe. Both were deliberate; neither is a migration gap to close without a reason.

1. **One `session.rs`, not a `session/` module tree.** The plan split the layer across `session/{mod,session,correlation,dispatch,keepalive,handshake}.rs`. It shipped as a single ~750-line `crates/cortex-rs/src/session.rs`. At this size the split would be five files of under 150 lines each with the shared `Shared` struct threaded between them; the single file is easier to read and the `@see` traceability is unaffected. Revisit if the file passes ~1000 lines.

2. **The named unit tests in Phases 2-3 were not written.** `test_type_first_no_id`, `test_id_consistency_check`, `test_cascade_ignored`, `test_broadcast_match_predicate`, `test_race_window_grace`, `test_reassembly_cap`, and `test_first_flag_resets_partial` all require a fake transport to inject inbound frames, which does not exist yet. What IS tested is the `request_id` varint extraction and the read-message encoding. **This is the largest gap in the zone**: the correlation rules are the part most likely to be silently wrong, and they are currently only covered by the hardware smoke run that has not happened yet. See Phase 8.

---

## Phase 0: Spike and Conformance Reference

<!-- files: (no source changes — study and conformance cross-check) -->
<!-- @see ../../quad-cortex-linux-editor-and-protocol.md -->
<!-- @see ../../../pyquadcortex/pyquadcortex/transport.py -->
<!-- @see ../../../pyquadcortex/pyquadcortex/client.py -->

- [ ] Read `pyquadcortex/pyquadcortex/transport.py` end to end; note the correlation rules, the race-window grace, and the reassembly cap.
- [ ] Read `pyquadcortex/pyquadcortex/client.py::_hello` and the `_SUBSCRIBE_TYPES` list; confirm the 22-type order against a capture.
- [ ] Run the `pyquadcortex` offline test suite as a conformance reference for the correlation and broadcast-wait semantics.
- [ ] Capture a live connect handshake with a real Quad Cortex (CorOS 4.0.1) and confirm the 6-step sequence matches the spec.

---

## Phase 1: Session Struct and State

<!-- files: crates/cortex-rs/src/session/mod.rs, crates/cortex-rs/src/session/session.rs -->
<!-- @see spec.md [FR-1] [FR-4] [FR-18] [FR-19] -->
<!-- @see design.md [DES-SES-DATA] [DES-SES-ARCH] -->

- [ ] Create the `session/` module under `crates/cortex-rs/src/`.
- [ ] Define the `Session` struct with the fields in [DES-SES-DATA] (transport, keepalive_interval, ids, pending, type_waiters, collectors, state_lock, write_lock, running, stop_event, rx_handle, ka_handle).
- [ ] Define `Waiter`, `TypeWaiter`, `Collector` structs.
- [ ] Define the constants: `CC_VERSION`, `DEFAULT_KEEPALIVE`, `DEFAULT_REQUEST_TIMEOUT`, `DEFAULT_BROADCAST_TIMEOUT`, `DEFAULT_SETTLE`, `DELIVERY_GRACE`, `MAX_MESSAGE_BODY`, `READ_TIMEOUT_MS`.
- [ ] Implement `Session::new(transport, keepalive_interval)`.
- [ ] Implement `Session::stop()` (signal threads, join with timeout, idempotent).
- [ ] Re-export `Session` and the constants from `session/mod.rs` and `lib.rs`.

---

## Phase 2: Correlation Primitives

<!-- files: crates/cortex-rs/src/session/correlation.rs -->
<!-- @see spec.md [FR-6] [FR-7] [FR-8] [FR-9] [FR-10] [FR-11] -->
<!-- @see design.md [DES-SES-CORR] -->

- [ ] Implement `next_request_id() -> u64` (AtomicU64, starts at 1).
- [ ] Implement `request(message, timeout) -> Result<Message>` (register waiter before send, type-first correlation, id consistency check, race-window grace).
- [ ] Implement `await_broadcast(expected_type, trigger, timeout, match_fn) -> Result<Message>` (register type waiter, run trigger, wait).
- [ ] Implement `collect(expected_type, trigger, seconds, match_fn) -> Vec<Message>` (non-consuming collector, gather for duration).
- [ ] Write unit tests: `test_type_first_no_id`, `test_id_consistency_check`, `test_cascade_ignored`, `test_broadcast_match_predicate`, `test_race_window_grace` (no hardware, use a fake transport).

---

## Phase 3: RX/Dispatch Loop

<!-- files: crates/cortex-rs/src/session/dispatch.rs -->
<!-- @see spec.md [FR-12] [FR-13] [FR-14] [FR-15] [FR-16] -->
<!-- @see design.md [DES-SES-DISPATCH] -->

- [ ] Implement the background RX thread: read reports, reassemble, handle FIRST-flag reset, enforce `MAX_MESSAGE_BODY` cap.
- [ ] Implement frame-level gzip decompression (`1f 8b` prefix check) before protobuf decode.
- [ ] Implement per-message decode error handling (log and skip, never kill the thread).
- [ ] Implement the dispatch routing: type-first + id-check for `pending`, type + match predicate for `type_waiters`, append for `collectors`.
- [ ] Implement write serialization via `write_lock` (encoding outside the lock).
- [ ] Write unit test: `test_reassembly_cap` (buffer exceeding cap resets).
- [ ] Write unit test: `test_first_flag_resets_partial` (a FIRST-flagged report mid-reassembly drops the stale buffer).

---

## Phase 4: Keepalive

<!-- files: crates/cortex-rs/src/session/keepalive.rs -->
<!-- @see spec.md [FR-5] -->
<!-- @see design.md [DES-SES-KEEPALIVE] -->

- [ ] Implement the keepalive thread: interruptible sleep via the stop event, send `KeepAlive{UPDATE}` every `keepalive_interval`.
- [ ] Swallow send failures (the thread never dies); log at debug.
- [ ] Confirm the keepalive thread exits promptly on `stop()` (the interruptible sleep wakes immediately).

---

## Phase 5: Connect Handshake

<!-- files: crates/cortex-rs/src/session/handshake.rs -->
<!-- @see spec.md [FR-1] [FR-2] [FR-3] -->
<!-- @see design.md [DES-SES-HANDSHAKE] -->

- [ ] Implement `Session::connect(timeout, settle)`:
  - [ ] `ResetCommsBuffers` with a fresh UUID4 hex `session_id`, sent as a `request()` (correlate by id).
  - [ ] `Version` UPDATE with `cortex_control_version: CC_VERSION` (fire-and-forget `send`).
  - [ ] `ModelRepo` READ (fire-and-forget `send`).
  - [ ] `Connection{connected: true}` (fire-and-forget `send`).
  - [ ] 22 subscribe READs in the FR-3 order (fire-and-forget `send` each).
  - [ ] `settle` delay (a real sleep, default 2 s).
- [ ] Define the 22-subscribe-type list as a constant array in `handshake.rs`, matching FR-3 order exactly.
- [ ] Start the RX and keepalive threads as part of `connect()` (or a separate `start()` called by `connect`).

---

## Phase 6: Disconnect

<!-- files: crates/cortex-rs/src/session/session.rs -->
<!-- @see spec.md [FR-4] -->
<!-- @see design.md [DES-SES-OVR] -->

- [ ] Implement `Session::disconnect()`: send `Connection{connected: false}`, swallow the write STALL.
- [ ] Implement `Session::close()`: disconnect, then stop, then drop the transport (the owned-resource teardown order, matching pyquadcortex's `connect()` owned list).

---

## Phase 7: Hardware Verification

<!-- files: (no source changes — manual verification against hardware) -->
<!-- @see spec.md Acceptance Criteria -->
<!-- @see design.md [DES-SES-TEST] -->

- [ ] `connect()` against a real Quad Cortex (CorOS 4.0.1): device begins pushing state after settle (RecallPreset seed push arrives).
- [ ] Keepalive: session idle 60 s stays alive; subsequent `version()` succeeds.
- [ ] `request()` correlates a `Version` READ reply (no request_id) by type alone.
- [ ] `request()` correlates a `SetlistPosition` UPDATE echo by type AND request_id.
- [ ] `await_broadcast()` skips the seed RecallPreset push and accepts the push echoing the recall's request_id (read_preset end-to-end).
- [ ] `collect()` gathers multiple File folder-listing pushes from a single READ.
- [ ] RX thread survives a malformed frame (buffer resets, thread continues).
- [ ] `Session::stop()` joins RX and keepalive threads within the join timeout.
- [ ] `Session::disconnect()` accepted by the device (pushes stop).
- [ ] Remove "provisional" labelling from session docs and release notes once the above pass.

---

## Work Sessions

| Date       | Task                 | Action | Files Modified                                                                                              | Agent | Human |
| ---------- | -------------------- | ------ | ----------------------------------------------------------------------------------------------------------- | ----- | ----- |
| 2026-08-01 | Spec authoring       | Wrote  | spec/140-session/spec.md, spec/140-session/design.md, spec/140-session/tasks.md                              | [x]   | [ ]   |