---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["session", "connect", "keepalive", "correlation", "broadcast", "provisional"]
---

# 140 Session - Spec

> The session layer: the connect handshake, keepalive, request_id correlation, and broadcast waiting. It sits between the transport (zone 100) and the client (zone 150).

## References

- **Transport (lower layer)**: [`../100-transport/spec.md`](../100-transport/spec.md) - USB HID device open, read/write, the benign write STALL
- **Framing (lower layer)**: [`../110-framing/spec.md`](../110-framing/spec.md) - flag-driven reassembly, trailer-tagged envelope
- **Protobuf schema**: [`../120-proto-schema/spec.md`](../120-proto-schema/spec.md) - `CortexMessageType` registry, `MessageAction`, `request_id` field semantics
- **Client (upper layer, consumer)**: [`../150-client/spec.md`](../150-client/spec.md) - the ergonomic `QuadCortex` API this handshake enables
- **Research note**: [`../../quad-cortex-linux-editor-and-protocol.md`](../../quad-cortex-linux-editor-and-protocol.md) - authoritative protocol facts
- **Prior art (MIT, ported)**: `pyquadcortex/pyquadcortex/transport.py` - the `Transport` class this is a port of; `pyquadcortex/pyquadcortex/client.py::_hello` - the handshake sequence

---

## Problem Statement

The Quad Cortex will not push state to a client that has merely opened the USB pipe. A host must perform a specific connect handshake before the device treats it as a connected editor and starts broadcasting. This zone owns the session state machine that sits above raw framed transport and below the ergonomic client: it knows how to bring a session up, keep it alive, correlate replies to requests, and wait for unsolicited device pushes.

This zone does NOT own the ergonomic API (zone 150), the HID device handle (zone 100), or the framing/decode layer (zones 110/120). It consumes a transport-like trait that exposes `send`, `request`, and the reassembled-message stream, and it provides the correlated request/response and broadcast-wait primitives the client is built on.

The protocol facts this zone encodes are hardware-verified via `pyquadcortex` against CorOS 4.0.1 and re-verified on this project's machine. The Rust implementation is provisional until the full session has been exercised against a real Quad Cortex from this crate.

---

## Requirements

### Functional Requirements

| ID    | Requirement                                                                                                                                                                                                                     | Priority    |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-1  | `Session::connect(timeout, settle)` performs the full connect handshake and returns a ready session. Steps: (a) `ResetCommsBuffers` with a fresh `session_id` (UUID hex), sent as a correlated request; (b) `Version` UPDATE announcing `cortex_control_version: "4.0.1"` (device gates push behaviour on a valid CC version); (c) `ModelRepo` READ (fetches the catalog); (d) `Connection{connected: true}`; (e) 22 subscribe READs (see FR-3); (f) settle for `settle` seconds (default 2.0). | Must Have   |
| FR-2  | `ResetCommsBuffers` carries a fresh 32-hex `session_id` (a UUID4 `.hex()` string). The device echoes it in the reply; correlate the reply by `request_id`. | Must Have   |
| FR-3  | The 22 subscribe READs, in order: `ModuleStats`, `License`, `UndoRedo`, `IOSettings`, `GeneralSettings`, `ShowGigView`, `Mode`, `GlobalEQ`, `MasterVolume`, `File`, `RecentsFavorites`, `CompilerInhibitedModules`, `RecallPreset`, `NewModels`, `PinnedModels`, `DefaultParameters`, `GlobalTempo`, `SetlistPosition`, `PresetDirty`, `Scene`, `BulkOperation`, `Updater`. Each is a fire-and-forget `send` of `<Type>Message{action: READ}`. | Must Have   |
| FR-4  | `Session::disconnect()` sends `Connection{connected: false}` (best effort; write errors are swallowed per the benign STALL). | Must Have   |
| FR-5  | A background keepalive task sends `KeepAlive{UPDATE}` every 5 seconds (default). Send failures are swallowed; the keepalive thread never dies. A dead device surfaces as read timeouts on the next request, not as a keepalive failure. | Must Have   |
| FR-6  | `request(message, timeout)` assigns a fresh monotonic `request_id`, registers a waiter BEFORE writing (so a fast reply cannot race the registration), sends, and blocks for the correlated reply up to `timeout`. Raises `Timeout` if no reply arrives. | Must Have   |
| FR-7  | Correlation is BY MESSAGE TYPE first, `request_id` as a consistency check. READ replies (e.g. `Version`) carry NO `request_id` echo; a state-changing request triggers a cascade of OTHER-type messages that all echo the request's id (recalling a preset emits `UndoRedo`/`Grid`/`Scene`/... all carrying the same `request_id` before the `SetlistPosition` echo). The reply is the first inbound message whose TYPE matches the request's, and whose `request_id` (if present on both sides) matches too. | Must Have   |
| FR-8  | When a READ reply carries no `request_id`, the first same-type pending waiter is satisfied (lowest `request_id` first). This covers the common case where the device answers a READ with a bare same-type message. | Must Have   |
| FR-9  | `next_request_id()` draws a fresh id from the same monotonic counter `request` uses, so a caller (e.g. `read_preset`) can tag a fire-and-forget message and later correlate a broadcast against it without colliding with `request` ids. | Must Have   |
| FR-10 | `await_broadcast(expected_type, trigger, timeout, match)` registers a type waiter FIRST, then runs `trigger` (e.g. a recall `send`), then blocks for the next matching `expected_type` broadcast. A `match` predicate `(message) -> bool` filters candidates: a right-type message whose predicate returns false is left undelivered, so the waiter keeps waiting for the right one. Raises `Timeout` on no matching broadcast. | Must Have   |
| FR-11 | `collect(expected_type, trigger, seconds, match)` fires `trigger` and gathers EVERY matching message for `seconds`, returning them in arrival order. A collector does NOT consume messages - they still reach any waiter or other collector. Used for the folder-enumeration flood a single `File` READ produces. | Should Have |
| FR-12 | The RX/dispatch loop must never die. Per-message decode/parse errors are logged and skipped; the reassembly buffer is reset on a malformed frame or a lost LAST flag so one bad frame cannot wedge the stream. Unknown message types and non-protobuf "raw payload" pushes (e.g. `License`, `CloudLogin`) are logged at debug and dropped. | Must Have   |
| FR-13 | Frame-level gzip decompression: if a reassembled payload starts with `1f 8b`, decompress it before protobuf decode. Field-level gzip inside protobuf `bytes` fields is handled at the domain layer (zone 130), not here. | Must Have   |
| FR-14 | A FIRST-flagged report always begins a new logical message: drop any stale partial reassembly buffer so the new message reassembles cleanly. This is routine (the device interleaves bursts of pushes); recovery is automatic. | Must Have   |
| FR-15 | Reassembly is capped at 1 MiB of reassembled body (`_MAX_MESSAGE_BODY = 1 << 20`). A legitimate message never reaches the cap (the `ModelRepo` reply, ~47 KB gzipped across ~371 reports, is the largest observed). A wedged buffer exceeding the cap is reset so the stream resyncs instead of accumulating forever. | Must Have   |
| FR-16 | Writes are serialized so one logical message's reports are written as an atomic group (a keepalive cannot interleave between a multi-report message's header and its continuation reports, which carry no header). The write lock is SEPARATE from the state lock; the state lock is never held across blocking device I/O. | Must Have   |
| FR-17 | After a `request` wait() times out, a reply can still land in the race window between the timeout and removing the pending entry. Whoever pops the entry "wins": if the RX thread already popped it, it is committed to delivering, so wait a short grace (0.5 s) for that to complete rather than dropping a reply that actually arrived. | Should Have |
| FR-18 | `Session::stop()` signals the background RX and keepalive threads to exit and joins them (bounded) so the caller can safely close the HID handle. Idempotent. Joining matters: closing the hidapi handle while the RX thread is still inside `read()` can crash. | Must Have   |
| FR-19 | The session holds the exclusive HID ownership for its lifetime (one owning process per device). The MCP server especially must hold a single session. | Must Have   |
| FR-20 | No version field on the wire: a CorOS update can silently break the handshake. Surface a `version()` probe (a `Version` READ, which works without the full handshake) as the pre-handshake sanity check, not a hard-coded assumption. | Should Have |

### Non-Functional Requirements

| ID    | Requirement                                                                                                                                                                                                                              | Target                  |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| NFR-1 | The session layer adds no async runtime dependency; it uses std threads + channels so the leaf crate stays embeddable in the CLI, MCP server, and Tauri backend without dragging in tokio. | Architectural invariant |
| NFR-2 | The connect handshake completes within `timeout` (default 5 s) for the `ResetCommsBuffers` reply, plus the `settle` delay (default 2 s). Total wall-clock < 10 s on a healthy device. | Hardware-observed       |
| NFR-3 | The keepalive interval (5 s) is chosen to be comfortably inside the device's session timeout (observed to tolerate far longer gaps, but Cortex Control pings every ~5 s). | Hardware-observed       |
| NFR-4 | Pending waiters (`request_id -> (Event, slot, expected_type)`) and the type-waiter/collector lists are guarded by a single `Mutex`; no lock is held across blocking device I/O or channel waits. | Code invariant          |
| NFR-5 | Unit tests for correlation logic (type-first matching, the id-less READ-reply fallback, the race-window grace, the stale-seed-push skip) run in CI without hardware. | CI-enforced             |
| NFR-6 | The session struct is `Send` but not `Clone`; it owns the transport and the background threads. Callers hold it by reference or behind an `Arc<Mutex<Session>>` at the host layer. | Code invariant          |

---

## Acceptance Criteria

- [ ] `Session::connect()` performs the 6-step handshake and the device begins pushing state (a `RecallPreset` seed push arrives after settle).
- [ ] `Session::disconnect()` sends `Connection{connected: false}` and swallows the expected write STALL.
- [ ] Keepalive sends `KeepAlive{UPDATE}` every 5 s and survives a send failure (thread stays alive).
- [ ] `request()` correlates a `Version` READ reply (no `request_id` echo) by type alone.
- [ ] `request()` correlates a `SetlistPosition` UPDATE echo by type AND `request_id`.
- [ ] `await_broadcast()` skips a stale/seed `RecallPreset` push (no `request_id`) and accepts only the push echoing the recall's `request_id`.
- [ ] `collect()` gathers multiple `File` folder-listing pushes from a single READ.
- [ ] The RX thread survives a malformed frame (buffer resets, thread continues).
- [ ] A FIRST-flagged report arriving mid-reassembly drops the stale partial and starts clean.
- [ ] `Session::stop()` joins the RX and keepalive threads within the join timeout.
- [ ] Unit tests for correlation, the race-window grace, and the stale-seed skip pass under `cargo test` (no hardware required).
- [ ] Full handshake verified against a real Quad Cortex (CorOS 4.0.1) - hardware-only.

---

## Non-Goals

- The ergonomic `QuadCortex` client API (zone 150) - this zone provides the primitives, the client builds on them.
- USB HID device open/close and the raw read/write loop (zone 100). This zone consumes a transport trait.
- Framing, flag-driven reassembly, and the trailer-tagged envelope decode (zone 110). This zone receives reassembled payloads.
- The protobuf schema and `prost` build (zone 120). This zone imports the generated types.
- The typed domain model - `BinaryPreset`, `Block`, `Split` (zone 130). This zone passes raw protobuf messages.
- MCP safety surface (zone 300) - the save-confirmation and scratch-slot policy lives there; this zone just provides `send`.

---

## Dependencies

- **Crate-internal**: zone 100 (transport trait), zone 110 (framing/decode), zone 120 (generated proto types + `CortexMessageType` registry).
- **External (leaf)**: `std::sync` (threads, `Mutex`, `mpsc`), `flate2` (frame-level gzip), the generated `prost` types. No async runtime.
- **Prior art**: `pyquadcortex/pyquadcortex/transport.py` (`Transport` class) and `pyquadcortex/pyquadcortex/client.py::_hello` - ported under MIT with attribution; see `THIRD-PARTY-NOTICES.md`.

---

## Appendix

### Protocol Provenance & Attribution

The connect handshake sequence, the correlation rules, the keepalive, and the broadcast-wait semantics are all ported from `pyquadcortex` (MIT, (c) 2026 Stokes). The recovered `.proto` files that define the 22 subscribe types and the `request_id` field semantics are vendored into `crates/cortex-rs/proto/` under their own MIT SPDX header. Record any derivation in `NOTICE` / `THIRD-PARTY-NOTICES.md`.

The "device gates push behaviour on a valid `cortex_control_version`" finding and the "a minimal `ResetCommsBuffers`+`Connection` is NOT enough" finding are both from `pyquadcortex` live probes, confirmed by capture. The `CC_VERSION = "4.0.1"` constant is the version captured on the wire against CorOS 4.0.1.

### Connect handshake detail (from pyquadcortex, confirmed by capture)

1. `ResetCommsBuffers` with a fresh `session_id` (UUID hex). Device echoes it; correlate by `request_id`.
2. `Version` UPDATE announcing `cortex_control_version: "4.0.1"`. The device gates state PUSH behaviour on receiving a valid CC version. Do NOT also issue a `Version` READ here: the device sends its own `Version` READ to the host, and a redundant host READ would race with a caller's later `version()` request (READ replies carry no `request_id` to disambiguate).
3. `ModelRepo` READ - fetches the catalog (~47 KB gzipped, spanning ~371 reports).
4. `Connection{connected: true}`.
5. 22 subscribe READs (FR-3). This is the subscription that makes the device start pushing each state type.
6. ~2 s settle. The device needs a moment after the burst before it treats the client as connected; a command sent too soon gets no push (observed as flaky `read_preset` timeouts).

### Correlation rules (from pyquadcortex, confirmed on hardware)

- Correlation is BY MESSAGE TYPE first, `request_id` as a consistency check.
- READ replies (e.g. `Version`) carry NO `request_id` echo.
- A state-changing request triggers a cascade of OTHER-type messages that all echo the request's `request_id` (recalling a preset emits `UndoRedo`/`Grid`/`Scene`/... all carrying the same id before the `SetlistPosition` echo).
- `RecallPreset` pushes a host recall triggers ECHO that recall's `request_id`; the unsolicited seed push (hello's subscription grid state) carries NONE. Without matching on the id, the waiter returns whatever `RecallPreset` arrives first - which lags by one recall when a prior push is still in flight (the seed seeds the lag).

### Provisional labelling

The protocol facts above are hardware-verified via `pyquadcortex`. The Rust implementation in this crate is **provisional** until the full session (handshake + keepalive + correlation + broadcast wait) has been exercised against a real Quad Cortex from this crate's own code. Label the session as "provisional" in docs and release notes until that hardware smoke run passes.