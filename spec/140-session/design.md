---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["session", "connect", "keepalive", "correlation", "broadcast", "provisional"]
spec: spec.md
---

# 140 Session - Design

## [DES-SES-OVR] Overview

The session layer is the orchestration core between raw framed transport and the ergonomic client. It owns three concurrent concerns: (1) bringing a session up via the connect handshake, (2) keeping it alive with a periodic keepalive, and (3) correlating inbound messages to outbound requests and broadcast waiters. It uses std threads + channels, not an async runtime, so the leaf crate stays embeddable in every host surface (CLI, MCP, Tauri) without a tokio dependency.

The design is a direct port of `pyquadcortex/pyquadcortex/transport.py::Transport` (MIT), adapted to Rust idioms: the `itertools.count` id source becomes an `AtomicU64`; the `threading.Event`/`threading.Lock` become `std::sync::Condvar`/`std::sync::Mutex`; the `_pending` dict and `_type_waiters` list become typed Rust collections. The handshake (`_hello`) lives in `client.py` upstream but is owned here, not in zone 150, because the handshake is session-layer state-machine work, not ergonomic API surface.

---

## [DES-SES-ARCH] Architecture

### File Map (planned)

```text
crates/cortex-rs/src/
├── session/
│   ├── mod.rs             — pub mod declarations; re-exports
│   ├── session.rs         — Session struct: connect/disconnect/stop, owns threads [FR-1,4,18]
│   ├── handshake.rs       — connect handshake sequence (the 6 steps)        [FR-1,2,3]
│   ├── keepalive.rs       — KeepAlive{UPDATE} loop, swallows failures        [FR-5]
│   ├── correlation.rs     — request/await_broadcast/collect + dispatch      [FR-6,7,8,9,10,11]
│   └── dispatch.rs        — RX loop body: decode, gzip, route to waiters     [FR-12,13,14,15]
└── transport.rs           — (zone 100) the transport trait + HID impl
```

### Flow Map: `[Flow.Session]`

```text
[Client (zone 150) — QuadCortex]
        │ calls session.connect() / session.request() / session.await_broadcast()
        ▼
[Session (this zone)]
  ├── handshake.rs:  ResetCommsBuffers → Version UPDATE → ModelRepo READ
  │                  → Connection{true} → 22 subscribe READs → settle
  ├── keepalive.rs: every 5s, KeepAlive{UPDATE} (swallow failures)
  ├── correlation.rs: request_id counter, pending waiters, type waiters, collectors
  └── dispatch.rs:   decode → gzip check → route to waiter (type-first, id-check)
        │
        ▼
[Transport trait (zone 100)]
        │  read_report() / write_report() (swallowing the benign STALL)
        ↕  USB HID
[Quad Cortex hardware]
```

---

## [DES-SES-DATA] Data Model

### `Session` struct

Holds the session state. `Send` but not `Clone`; owns the transport and the background thread handles.

| Field                | Type                              | Description                                                              |
| -------------------- | --------------------------------- | ------------------------------------------------------------------------ |
| `transport`          | `Transport` (trait object or impl) | The framed transport; provides `read_report`, `write_report`, `stop`    |
| `keepalive_interval` | `Duration`                        | Default 5 s                                                              |
| `ids`                | `AtomicU64`                       | Monotonic request_id counter, starts at 1                                |
| `pending`            | `Mutex<HashMap<u64, Waiter>>`     | `request_id -> Waiter`; guarded by `state_lock`                          |
| `type_waiters`       | `Mutex<Vec<TypeWaiter>>`          | Unsolicited-broadcast waiters, matched by message type                   |
| `collectors`         | `Mutex<Vec<Collector>>`           | `collect()` entries; do not consume messages                              |
| `state_lock`         | `Mutex<()>`                       | Guards `pending` / `type_waiters` / `collectors` (state only, never I/O)  |
| `write_lock`         | `Mutex<()>`                       | Serializes device writes per logical message                             |
| `running`            | `AtomicBool`                      | RX/keepalive loop control                                                 |
| `stop_event`         | `Condvar` / `AtomicBool`          | Interruptible sleep for keepalive                                         |
| `rx_handle`          | `JoinHandle`                      | The RX/dispatch thread                                                    |
| `ka_handle`          | `JoinHandle`                      | The keepalive thread                                                      |

### `Waiter` (for `request()`)

| Field            | Type                          | Description                                  |
| ---------------- | ----------------------------- | -------------------------------------------- |
| `event`          | `Event` (Condvar-based)       | Signalled when the reply lands               |
| `slot`           | `Mutex<Option<Message>>`      | The reply, set before signalling             |
| `expected_type`  | `CortexMessageType`           | The request's message type (for type match)  |

### `TypeWaiter` (for `await_broadcast()`)

| Field            | Type                          | Description                                  |
| ---------------- | ----------------------------- | -------------------------------------------- |
| `expected_type`  | `CortexMessageType`           | The broadcast type to wait for               |
| `match_fn`       | `Box<dyn Fn(&Message) -> bool>` | Optional predicate; false = keep waiting  |
| `event`          | `Event`                       | Signalled when a matching broadcast lands     |
| `slot`           | `Mutex<Option<Message>>`      | The broadcast, set before signalling          |

### `Collector` (for `collect()`)

| Field            | Type                          | Description                                  |
| ---------------- | ----------------------------- | -------------------------------------------- |
| `expected_type`  | `CortexMessageType`           | The type to gather                           |
| `match_fn`       | `Option<Box<dyn Fn(&Message) -> bool>>` | Optional filter                  |
| `bucket`          | `Mutex<Vec<Message>>`         | Appended in arrival order; not consuming     |

### Constants

| Constant                | Value      | Description                                                              |
| ----------------------- | ---------- | ------------------------------------------------------------------------ |
| `CC_VERSION`            | `"4.0.1"`  | Cortex Control version announced in the handshake (CorOS 4.0.1 capture) |
| `DEFAULT_KEEPALIVE`     | 5 s        | Keepalive interval                                                        |
| `DEFAULT_REQUEST_TIMEOUT` | 5 s      | Default `request()` timeout                                               |
| `DEFAULT_BROADCAST_TIMEOUT` | 40 s   | Default `await_broadcast()` timeout (device services pushes lazily)      |
| `DEFAULT_SETTLE`        | 2 s        | Post-handshake settle delay                                               |
| `DELIVERY_GRACE`        | 0.5 s      | Race-window grace after a `request` timeout                               |
| `MAX_MESSAGE_BODY`      | 1 MiB      | Reassembly cap; a legitimate message never reaches this                   |
| `READ_TIMEOUT_MS`       | 200 ms     | RX read poll timeout (how quickly the loop notices `stop()`)             |

---

## [DES-SES-HANDSHAKE] Connect Handshake

`Session::connect(timeout, settle)` performs the 6-step sequence from the spec ([FR-1]). The handshake lives in `handshake.rs`, not in the client zone, because it is session-layer state-machine work: it establishes the subscription state the device needs before it will push, and it is not part of the ergonomic call surface.

Key points ported from `pyquadcortex`:

- The `session_id` is a fresh UUID4 `.hex()` string (32 hex chars, no dashes). The device echoes it; the reply is correlated by `request_id` via the normal `request()` path.
- Do NOT issue a `Version` READ alongside the `Version` UPDATE: the device sends its own `Version` READ to the host, and a redundant host READ would race with a later `version()` call (READ replies carry no `request_id`).
- The 22 subscribe READs are fire-and-forget `send` calls (not `request`), because their replies are unsolicited pushes the device emits over time, not prompt same-type echoes.
- The settle delay (default 2 s) is a real sleep, not a spin. A command sent before the device has finished processing the burst gets no push (observed as flaky `read_preset` timeouts).

---

## [DES-SES-CORR] Correlation

`correlation.rs` owns the request/broadcast primitives. The central design decision, confirmed on hardware, is:

**Correlation is by MESSAGE TYPE first, `request_id` as a consistency check.**

This is because:
- READ replies (e.g. `Version`) carry NO `request_id` echo. The device answers a READ with a same-type message that has no id, so the only match signal is type. When multiple READs of the same type are in flight, the lowest-id pending waiter of that type is satisfied (first-in, first-out).
- State-changing requests (e.g. `recall_preset`) trigger a CASCADE of other-type messages (`UndoRedo`, `Grid`, `Scene`, ...) that all echo the request's `request_id` before the same-type echo (`SetlistPosition`) arrives. The dispatch must not deliver those cascade messages to the waiter - only the same-type echo matches.

`await_broadcast` ([FR-10]) is the companion for unsolicited device pushes that answer an action rather than a request. The `RecallPreset` push emitted after a preset is recalled is delivered lazily (10-25 s observed) and carries the recall's `request_id`; the seed push from the handshake carries none. The `match` predicate lets `read_preset` skip the seed and accept only the push echoing its own recall's id.

`collect` ([FR-11]) is the fan-out variant: a single `File` READ makes the device enumerate every folder (~399 on the observed unit), arriving over 10-20 s. A collector gathers them all without consuming them (waiters and other collectors still receive copies).

---

## [DES-SES-DISPATCH] RX/Dispatch Loop

`dispatch.rs` is the background thread that reads reports, reassembles, decodes, and routes. Design invariants:

- **The loop never dies.** Every per-message decode/parse is wrapped; a malformed frame, unknown type, or non-protobuf raw payload is logged at debug and skipped. The reassembly buffer is reset on failure.
- **A FIRST-flagged report begins a new message.** If a partial buffer exists, it is dropped (routine: the device interleaves bursts). Recovery is automatic.
- **Reassembly is capped** at `MAX_MESSAGE_BODY` (1 MiB). A lost LAST flag leaves the buffer unable to complete; the cap lets the loop reset rather than accumulate forever. No legitimate message reaches the cap.
- **Frame-level gzip** (`1f 8b` prefix) is decompressed before protobuf decode. Field-level gzip inside `bytes` fields is a domain-layer concern (zone 130).
- **Write serialization.** A `write_lock` serializes the reports of a single logical message so a keepalive cannot interleave. Encoding happens outside the lock to keep the critical section to device I/O only. The `state_lock` is never held across blocking I/O.

---

## [DES-SES-KEEPALIVE] Keepalive

`keepalive.rs` runs a background thread that sends `KeepAlive{UPDATE}` every `keepalive_interval` (default 5 s). The sleep is interruptible (wakes immediately on `stop()` via the stop event/condvar). Send failures are swallowed; the thread never dies. A genuinely dead device surfaces as `request` timeouts on the next caller, not as a keepalive error - the same principle as the benign write STALL.

---

## [DES-SES-DEC] Key Decisions

| Decision                                         | Choice                                                       | Rationale                                                                                                                                                            |
| ------------------------------------------------ | ------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| std threads over async runtime                  | `std::thread` + `Mutex`/`Condvar`/`mpsc`                      | Leaf-crate discipline: no tokio dependency so the crate embeds in any host. The pyquadcortex precedent is the same (threading, not asyncio).                        |
| Handshake owned by session, not client           | `handshake.rs` in this zone                                  | The handshake is session-layer state machine work (subscription + settle), not ergonomic API surface. The client (zone 150) calls `session.connect()`.            |
| Type-first correlation                           | Match on `CortexMessageType` before `request_id`             | READ replies carry no id; cascade messages echo a foreign type's id. Type-first is the only rule that covers both. Confirmed on hardware.                            |
| `request_id` monotonic from 1                    | `AtomicU64`                                                  | Lets `next_request_id()` share the counter with `request()` so a fire-and-forget tag never collides with a request id.                                              |
| Race-window grace on timeout                     | 0.5 s wait after a timeout if the RX thread already popped    | Avoids dropping a reply that landed in the microsecond window between the wait() timeout and the pending-entry removal.                                              |
| Write lock separate from state lock               | Two `Mutex<()>`                                              | The state lock guards waiter maps; the write lock serializes I/O. Never hold state across blocking I/O.                                                            |
| Reassembly cap at 1 MiB                           | `MAX_MESSAGE_BODY = 1 << 20`                                 | The envelope has no total-length field; a lost LAST flag would wedge the buffer. The cap resets it. Largest observed message (ModelRepo, ~47 KB) is far under.     |
| Keepalive failures swallowed                      | Thread continues after a send error                           | A dead device shows up as read timeouts on the next request. The keepalive thread must not be the thing that reports device death (it would just stop reporting).    |

---

## [DES-SES-TEST] Testing Notes

**In-crate unit tests** (`correlation.rs`, `#[cfg(test)]`), no hardware:

- `test_type_first_no_id`: a READ reply with no `request_id` satisfies the pending same-type waiter.
- `test_id_consistency_check`: a same-type reply with a matching `request_id` satisfies the waiter; a non-matching id does not.
- `test_cascade_ignored`: an other-type message echoing the request's id is NOT delivered to the waiter (cascade is skipped).
- `test_broadcast_match_predicate`: a `RecallPreset` push with no `request_id` (the seed) is rejected by the match predicate; the push echoing the recall's id is accepted.
- `test_race_window_grace`: simulate a reply landing after the wait() timeout but before pending removal; the grace window delivers it.
- `test_reassembly_cap`: a buffer exceeding `MAX_MESSAGE_BODY` resets.

**Hardware verification** (manual, documented in the release smoke matrix):

- `connect()` against a real Quad Cortex (CorOS 4.0.1): the device begins pushing state after settle (a `RecallPreset` seed push arrives).
- Keepalive: a session left idle for 60 s stays alive (a subsequent `version()` succeeds).
- `read_preset` end-to-end: recall + `await_broadcast` returns the preset's `BinaryPreset` with the recall's `request_id` echoed.
- Disconnect: `Connection{connected: false}` is accepted (the device stops pushing).

**Provisional labelling**: the session is provisional until the full handshake + correlation has been exercised from this crate's own code against real hardware. Cross-check against `pyquadcortex` offline tests as a conformance reference, but that is not a substitute for a hardware smoke run.

---

## [DES-SES-PROVENANCE] Provenance & Attribution

The session layer is a port of `pyquadcortex/pyquadcortex/transport.py::Transport` (MIT, (c) 2026 Stokes). The handshake sequence (`_hello`) lives in `pyquadcortex/pyquadcortex/client.py` upstream but is relocated to this zone because it is session-layer work. The correlation rules (type-first, id-check, the seed-push skip) and the benign-write-STALL swallowing are all confirmed by `pyquadcortex` capture and live probe. See `THIRD-PARTY-NOTICES.md` for the MIT attribution.

No code is copied from the unlicensed reference repos. The protocol facts are re-expressed in this project's own words and Rust idioms.