---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-05T05:01:48.000Z"
tags: ["session", "connect", "keepalive", "correlation", "broadcast"]
spec: spec.md
---

# 140 Session - Design

## [DES-SES-OVR] Overview

The session layer is the orchestration core between raw framed transport and the ergonomic client. It owns three concurrent concerns: (1) bringing a session up via the connect handshake, (2) keeping it alive with a periodic keepalive, and (3) correlating inbound messages to outbound requests and broadcast waiters. It uses std threads + channels, not an async runtime, so the leaf crate stays embeddable in every host surface (CLI, MCP, Tauri) without a tokio dependency.

The design is a direct port of `pyquadcortex/pyquadcortex/transport.py::Transport` (MIT), adapted to Rust idioms: the `itertools.count` id source becomes an `AtomicU64`; the `threading.Event`/`threading.Lock` become `std::sync::Condvar`/`std::sync::Mutex`; the `_pending` dict and `_type_waiters` list become typed Rust collections. The handshake (`_hello`) lives in `client.py` upstream but is owned here, not in zone 150, because the handshake is session-layer state-machine work, not ergonomic API surface.

---

## [DES-SES-ARCH] Architecture

### File Map

```text
crates/cortex-rs/src/
├── link.rs                — transport-neutral `HidLink` seam and fake-link support
├── session.rs             — handshake, RX/keepalive threads, correlation and dispatch
├── state.rs               — subscribed-state cache, transactional sparse reducer, revisions
└── transport.rs           — (zone 100) hidapi report I/O implementation
```

### Flow Map: `[Flow.Session]`

```text
[Client (zone 150) — QuadCortex]
        │ calls session.connect() / session.request() / session.await_broadcast()
        ▼
[Session (this zone)]
  ├── session.rs:    ResetCommsBuffers → Version READ → Version UPDATE
  │                  → ModelRepo READ+wait → Connection{true}
  │                  → 22 subscribe READs → CPULoad CREATE → settle
  ├── keepalive:     every 1s, KeepAlive{UPDATE} (swallow failures)
  ├── correlation:   request_id counter, pending waiters, type waiters, collectors
  └── dispatch:      decode → liveness → state reducer → collectors/waiters
        │                         │
        │                         └─ DeviceStateCache snapshots + revision watch
        │
        ▼
[HidLink (this zone), implemented by Transport (zone 100)]
        │  read_report() / write_report() (swallowing the benign STALL)
        ↕  USB HID
[Quad Cortex hardware]
```

---

## [DES-SES-DATA] Data Model

### `Session` and waiter state

`Session` owns a removable `Box<dyn HidLink>`, shared request/broadcast/collector state, an optional `DeviceStateCache`, and mutex-protected RX/keepalive join handles. `close()` announces disconnect, stops and joins workers, then takes the boxed link so return proves the device lease is gone even when another `Arc<Session>` remains. Waiters use `Condvar`-based slots and raw `MessageType` tags; their synchronisation is separate from the link's writer-priority gate.

### Constants

| Constant | Value | Description |
| ----------------------- | ---------- | ------------------------------------------------------------------------ |
| `CC_VERSION` | `"4.0.1"` | Cortex Control version announced in the handshake (CorOS 4.0.1 capture) |
| `DEFAULT_KEEPALIVE_INTERVAL` | 1 s | Keepalive interval, matching Cortex Control's measured 1.04 s cadence |
| `DEFAULT_REQUEST_TIMEOUT` | 5 s | Default `request()` timeout |
| `DEFAULT_BROADCAST_TIMEOUT` | 40 s | Default `await_broadcast()` timeout for asynchronous push paths |
| `DEFAULT_SETTLE` | 2 s | Minimum wait before the adaptive quiet-period check |
| `DELIVERY_GRACE` | 0.5 s | Race-window grace after a `request` timeout |
| `MAX_MESSAGE_BODY` | 1 MiB | Reassembly cap; a legitimate message never reaches this |
| `READ_TIMEOUT_MS` | 200 ms | RX read poll timeout (how quickly the loop notices `stop()`) |

---

## [DES-SES-HANDSHAKE] Connect Handshake

`Session::connect(timeout, settle)` performs the paced 8-step sequence from the spec ([FR-1]). The handshake lives in `session.rs`, not in the client zone, because it is session-layer state-machine work: it establishes the subscription state the device needs before it will push, and it is not part of the ergonomic call surface.

Key points ported from `pyquadcortex`:

- The `session_id` is a fresh UUID4 `.hex()` string (32 hex chars, no dashes). The device echoes it; the reply is correlated by `request_id` via the normal `request()` path.
- Issue the `Version` READ before the UPDATE and cache its non-empty reply. The device later sends its own id-less `Version` READ, so ordering this inside the handshake prevents a caller's `version()` request from racing it.
- Wait for the `ModelRepo` payload before announcing the connection and subscriptions. Firing the burst together queued the catalog behind requests the client itself created; pacing reduced the transfer from 5.06 s to 0.67 s, matching Cortex Control's 0.65 s.
- Keep the catalog generation-local and in memory. Non-empty `NewModels`, a stream gap, or a generation change removes the raw payload; the daemon evicts its separately parsed form whenever no exact current payload exists. A fresh payload is keyed by generation and revision before it can supply block names or parameter metadata.
- The 22 subscribe READs are fire-and-forget `send` calls (not `request`), because their replies are unsolicited pushes the device emits over time, not prompt same-type echoes.
- `CPULoad` is subscribed separately with `CREATE` and a request id. A READ is silently ignored.
- Settling is adaptive: sleep for the caller-provided floor (2 s by default), then wait until non-heartbeat traffic has been quiet for 1.5 s, with a 30 s ceiling. `GlobalTempo` is excluded because its continuous clock traffic would otherwise force every handshake to the ceiling.

---

## [DES-SES-CORR] Correlation

`session.rs` owns the request/broadcast primitives. The central design decision, confirmed on hardware, is:

**Correlation is by MESSAGE TYPE first, `request_id` as a consistency check.**

This is because:

- READ replies (e.g. `Version`) carry NO `request_id` echo. The device answers a READ with a same-type message that has no id, so the only match signal is type. When multiple READs of the same type are in flight, the lowest-id pending waiter of that type is satisfied (first-in, first-out).
- State-changing requests (e.g. `recall_preset`) trigger a CASCADE of other-type messages (`UndoRedo`, `Grid`, `Scene`, ...) that all echo the request's `request_id` before the same-type echo (`SetlistPosition`) arrives. The dispatch must not deliver those cascade messages to the waiter - only the same-type echo matches.

`await_broadcast` ([FR-10]) is the companion for unsolicited device pushes that answer an action rather than a request. The `RecallPreset` push emitted after a preset is recalled is asynchronous and carries the recall's `request_id`; the seed push from the handshake carries none. The `match` predicate lets `read_preset` skip the seed and accept only the push echoing its own recall's id.

`collect` ([FR-11]) is the fan-out variant: a single `File` READ can make the device enumerate hundreds of folders. A collector gathers them without consuming waiter or other collector delivery.

---

## [DES-SES-DISPATCH] RX/Dispatch Loop

`rx_loop` and `dispatch` in `session.rs` read reports, reassemble, decode, and route. Design invariants:

- **The loop never dies.** Every per-message decode/parse is wrapped; a malformed frame, unknown type, or non-protobuf raw payload is logged at debug and skipped. The reassembly buffer is reset on failure.
- **A FIRST-flagged report begins a new message.** If a partial buffer exists, framing resynchronises and session invalidates cache continuity.
- **Reassembly is capped** at `MAX_MESSAGE_BODY` (1 MiB). A lost LAST flag leaves the buffer unable to complete; the cap lets the loop reset rather than accumulate forever. No legitimate message reaches the cap.
- **Frame-level gzip** (`1f 8b` prefix) is decompressed before protobuf decode. Field-level gzip inside `bytes` fields is a domain-layer concern (zone 130).
- **Write serialization.** A `write_lock` serializes the reports of a single logical message so a keepalive cannot interleave. Encoding happens outside the lock to keep the critical section to device I/O only. The `state_lock` is never held across blocking I/O.
- **State observation precedes consumption.** The reducer sees a message after liveness is stamped and before collectors/pending/type waiters. An explicit read therefore repairs the same cache before its caller returns.
- **A broken stream invalidates continuity.** A malformed report, stale partial abandoned by a new FIRST, reassembly error, or cap breach means a complete state update may have been lost. The RX loop continues, but the cache is cleared rather than serving the pre-gap snapshot. Continuing heartbeats do not repair that proof: the daemon drains active operations and replaces the physical session so a complete subscribed handshake establishes a new generation.

### Subscribed state reducer

`state.rs` keeps the latest device-reported values behind one mutex. The RX thread is the sole writer; hosts clone snapshots and use `wait_for_change` to coalesce bursts. No callback runs on the RX thread and no reducer path performs device I/O.

A full four-row, positional `RecallPreset` is the live-grid baseline. Sparse `Grid` messages are keyed by row, column and parameter index, so they are applied to a clone and committed only if every operation is supported. Routing, split points, global/per-scene parameter values, scene-mode promotion, global/per-scene bypass, and DELETE removal are reducible from the established message shapes. Block placement does not carry the defaults the device instantiated, scene-mode demotion does not identify the retained value, and malformed/unknown targets cannot be reconstructed; those invalidate the baseline and the next side-effect-free `read_current_preset` repairs it.

Version, model-repository payload, CPU load, active scene, dirty state, selected slot, and complete per-folder `File{UPDATE}` listings are also retained. `File` mutation acknowledgements invalidate the named listing rather than masquerading as a one-item snapshot. The device's later empty `Version{READ}` is ignored so it cannot erase the handshake identity.

One cache handle survives host reconnect. Every attached `Session` begins a generation, and reducer calls carry that token; delivery from a stopped old RX thread is therefore ignored after replacement. Invalidation happens before reconnect backoff, not after success. `Session` owns the link in a removable slot: `close()` joins workers and takes the link before replacement open, so retained session references cannot retain the HID lease. The daemon's read/write operation gate covers health recheck, selected-session use, handle release and replacement handshake.

The host's auto-managed idle verdict does not weaken that lifetime boundary. It is request-based and cannot fire while an operation is in flight; after it fires, the daemon interrupts reconnect backoff, acquires the operation write gate, and calls `Session::close()` before removing its endpoint. Explicitly started sessions have no idle verdict.

---

## [DES-SES-KEEPALIVE] Keepalive

The keepalive loop runs a background thread that sends `KeepAlive{UPDATE}` every `keepalive_interval` (default 1 s). Cortex Control's measured cadence is 1.04 s; at 5 s the device stops pushing state after about 40 s with no error. The sleep is interruptible (wakes immediately on `stop()` via the stop event/condvar). Send failures are swallowed; a genuinely dead device surfaces through read silence and request failure rather than a write error - the same principle as the benign write STALL.

---

## [DES-SES-DEC] Key Decisions

| Decision | Choice | Rationale |
| ------------------------------------------------ | ------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| std threads over async runtime | `std::thread` + `Mutex`/`Condvar`/`mpsc` | Leaf-crate discipline: no tokio dependency so the crate embeds in any host. The pyquadcortex precedent is the same (threading, not asyncio). |
| Handshake owned by session, not client | `session.rs` in this zone | The handshake is session-layer state machine work (subscription + settle), not ergonomic API surface. The client (zone 150) calls `session.connect()`. |
| Type-first correlation | Match on `CortexMessageType` before `request_id` | READ replies carry no id; cascade messages echo a foreign type's id. Type-first is the only rule that covers both. Confirmed on hardware. |
| `request_id` monotonic from 1 | `AtomicU64` | Lets `next_request_id()` share the counter with `request()` so a fire-and-forget tag never collides with a request id. |
| Race-window grace on timeout | 0.5 s wait after a timeout if the RX thread already popped | Avoids dropping a reply that landed in the microsecond window between the wait() timeout and the pending-entry removal. |
| Write lock separate from state lock | Two `Mutex<()>` | The state lock guards waiter maps; the write lock serializes I/O. Never hold state across blocking I/O. |
| Waiting writers stop the RX loop reacquiring | `writers_waiting` + RAII `WriteIntent` | The device mutex is unfair; merely shortening or yielding after a read still starves writes. A declared writer gets priority until its logical message is sent. |
| Reassembly cap at 1 MiB | `MAX_MESSAGE_BODY = 1 << 20` | The envelope has no total-length field; a lost LAST flag would wedge the buffer. The cap resets it. Largest observed message (ModelRepo, ~47 KB) is far under. |
| Keepalive failures swallowed | Thread continues after a send error | A dead device shows up through read silence and `DeviceSilent` on a request. The benign write STALL makes a keepalive write error unusable as a health verdict. |
| State reducer before waiters | Non-consuming synchronous observation | A correlated read and the cache must see the same message; putting observation after an early return silently misses replies. |
| Invalidate instead of generic protobuf merge | Narrow keyed reducer | Repeated proto fields append under generic merge, while full presets are positional and deltas are keyed. Guessing silently corrupts the grid. |
| Generation + revision | Stable cache across replaceable sessions | Generation rejects old RX delivery; revision lets GUI consumers fetch latest state without queueing 135 knob messages. |

---

## [DES-SES-TEST] Testing Notes

**In-crate unit tests** (`session.rs`, `#[cfg(test)]`), no hardware:

- Correlation tests cover id-less type-first matching, id consistency, cascade rejection, broadcast predicates, oldest-first fallback and the timeout race grace.
- Fake-link tests cover reassembled delivery, continuity invalidation with continued RX, writer priority under a saturated reader, keepalive cadence and never-heard liveness.
- Direct frame injection for the 1 MiB cap and FIRST-over-partial branches remains the narrow coverage gap.
- State reducer tests cover full seeding, global/per-scene parameter and bypass updates, promotion, removal, malformed and structural invalidation, folder mutation invalidation, old-generation rejection, explicit invalidation, empty-Version protection, and a 135-message knob burst.
- Dispatch tests prove one `Scene` message updates the cache and satisfies its pending waiter; fake-link tests prove a malformed report invalidates the cache while RX continues.

**Hardware verification** (manual, documented in the release smoke matrix):

- `connect()` against a real Quad Cortex (CorOS 4.0.1): the device begins pushing state after settle (a `RecallPreset` seed push arrives).
- Keepalive: a session left idle for 60 s stays alive (a subsequent `version()` succeeds).
- `read_preset` end-to-end: recall + `await_broadcast` returns the preset's `BinaryPreset` with the recall's `request_id` echoed.
- Disconnect: `Connection{connected: false}` is accepted (the device stops pushing).

**Verification status**: the full Rust handshake, correlation, broadcast waiting, one-second keepalive, CPU-load subscription, and clean shutdown are hardware-verified against CorOS 4.0.1. Cross-check `pyquadcortex` offline tests as a conformance reference and repeat the hardware smoke after a firmware update.

---

## [DES-SES-PROVENANCE] Provenance & Attribution

The session layer is a port of `pyquadcortex/pyquadcortex/transport.py::Transport` (MIT, (c) 2026 Stokes). The handshake sequence (`_hello`) lives in `pyquadcortex/pyquadcortex/client.py` upstream but is relocated to this zone because it is session-layer work. The correlation rules (type-first, id-check, the seed-push skip) and the benign-write-STALL swallowing are all confirmed by `pyquadcortex` capture and live probe. See `THIRD-PARTY-NOTICES.md` for the MIT attribution.

No code is copied from the reference-only repositories without a clear repository-wide licence. Their findings are re-expressed in this project's own words.

## [DES-SES-DIVERGENCE] Divergences from the original plan

Recorded rather than silently absorbed. Both were deliberate; neither is a migration gap to close without a reason.

### One `session.rs`, not a `session/` module tree

The plan split the layer across `session/{mod,session,correlation,dispatch,keepalive,handshake}.rs`. Handshake, correlation, dispatch and thread ownership remain together in `session.rs` because separating them would thread the private `Shared` type through a module tree. The subscribed reducer became `state.rs` once it acquired an independent public contract, generation/revision watch, and substantial pure merge tests; it depends only on decoded messages and does not need session internals.

### The correlation tests needed no fake transport

The plan assumed a fake transport for injecting inbound frames. None was needed: [`dispatch`] is a free function over `(&InboundMessage, &Shared)`, so the correlation rules can be driven directly with no device abstraction at all.

The correlation tests cover type-first matching, the id-less oldest-first fallback, cascade rejection, broadcast predicates rejecting the stale seed push, collector observe-not-consume semantics, state observation before waiter consumption, and the liveness stamp the adaptive settle depends on.

**One of them is worth calling out, because the first version of it did not work.** `id_less_replies_drain_waiters_oldest_first` guards the HashMap-iteration-order bug. Written with two waiters, reintroducing the bug made it fail only 4 times in 12 runs - Rust randomises HashMap iteration per process, so a two-way choice comes out right about half the time. A guard that waves the regression through two runs in three is worse than none, because it looks like coverage. It now registers six waiters and asserts the whole drain ORDER, which fails 12/12 with the bug present and 0/12 without. Both figures were measured, not assumed.

The later `HidLink`/`FakeLink` seam covers the RX loop and writer gate without hardware: reassembled delivery, malformed-report recovery plus cache invalidation, writer priority under a saturated reader, one-second keepalives, and the never-heard liveness guard. The 1 MiB cap and FIRST-over-partial branches remain the narrow unexercised frame-injection cases.
