---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-05T05:01:48.000Z"
tags: ["session", "connect", "keepalive", "correlation", "broadcast"]
---

# 140 Session - Spec

> The session layer: the connect handshake, keepalive, request_id correlation, and broadcast waiting. It sits between the transport (zone 100) and the client (zone 150).

## References

- **Transport (lower layer)**: [`../100-transport/spec.md`](../100-transport/spec.md) - USB HID device open, read/write, the benign write STALL
- **Framing (lower layer)**: [`../110-framing/spec.md`](../110-framing/spec.md) - flag-driven reassembly, trailer-tagged envelope
- **Protobuf schema**: [`../120-proto-schema/spec.md`](../120-proto-schema/spec.md) - `CortexMessageType` registry, `MessageAction`, `request_id` field semantics
- **Client (upper layer, consumer)**: [`../150-client/spec.md`](../150-client/spec.md) - the ergonomic `QuadCortex` API this handshake enables
- **Public protocol reference**: [`../../docs/protocol.md`](../../docs/protocol.md) - authoritative protocol facts
- **Prior art (MIT, ported)**: `pyquadcortex/pyquadcortex/transport.py` - the `Transport` class this is a port of; `pyquadcortex/pyquadcortex/client.py::_hello` - the handshake sequence

---

## Problem Statement

The Quad Cortex will not push state to a client that has merely opened the USB pipe. A host must perform a specific connect handshake before the device treats it as a connected editor and starts broadcasting. This zone owns the session state machine that sits above raw framed transport and below the ergonomic client: it knows how to bring a session up, keep it alive, correlate replies to requests, and wait for unsolicited device pushes.

This zone does NOT own the ergonomic API (zone 150) or hidapi open/enumeration (zone 100). It owns the transport-neutral `HidLink` seam, the removable link lifetime, background report loop, correlated request/broadcast primitives, and subscribed state continuity.

The protocol facts and this Rust session implementation are hardware-verified against a real Quad Cortex running CorOS 4.0.1. The verification includes the paced handshake, one-second keepalive, correlation, broadcast waiting, CPU-load subscription, clean shutdown, and a held idle session.

---

## Requirements

### Functional Requirements

| ID | Requirement | Priority |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-1 | `Session::connect(timeout, settle)` performs the paced subscribed handshake and returns a ready session: (a) correlated `ResetCommsBuffers` with a fresh UUID hex `session_id`; (b) best-effort `Version` READ and cache; (c) `Version` UPDATE announcing `cortex_control_version: "4.0.1"`; (d) `ModelRepo` READ, waiting for the catalog before continuing; (e) `Connection{connected: true}`; (f) the 22 subscribe READs from FR-3; (g) `CPULoad{CREATE}` with a request id; (h) settle until the non-heartbeat stream is quiet, bounded by a floor and ceiling. `connect_minimal` stops after step (e). | Must Have |
| FR-2 | `ResetCommsBuffers` carries a fresh 32-hex `session_id` (a UUID4 `.hex()` string). The device echoes it in the reply; correlate the reply by `request_id`. | Must Have |
| FR-3 | The 22 ordinary subscriptions, in order: `ModuleStats`, `License`, `UndoRedo`, `IOSettings`, `GeneralSettings`, `ShowGigView`, `Mode`, `GlobalEQ`, `MasterVolume`, `File`, `RecentsFavorites`, `CompilerInhibitedModules`, `RecallPreset`, `NewModels`, `PinnedModels`, `DefaultParameters`, `GlobalTempo`, `SetlistPosition`, `PresetDirty`, `Scene`, `BulkOperation`, `Updater`. Each is a fire-and-forget `<Type>Message{action: READ}`. `CPULoad` is the exception: subscribe with `CREATE` and a request id; a READ is silently ignored. | Must Have |
| FR-4 | `Session::disconnect()` sends `Connection{connected: false}` (best effort; write errors are swallowed per the benign STALL). | Must Have |
| FR-5 | A background keepalive task sends `KeepAlive{UPDATE}` every second. Send failures are swallowed; the keepalive thread never dies. A five-second interval makes device pushes stop silently after about forty seconds, so this timing is a liveness requirement rather than a tuning choice. A dead device surfaces through read silence and request failure, not as a keepalive write error. | Must Have |
| FR-6 | `request(message, timeout)` assigns a fresh monotonic `request_id`, registers a waiter BEFORE writing (so a fast reply cannot race the registration), sends, and blocks for the correlated reply up to `timeout`. Raises `Timeout` if no reply arrives. | Must Have |
| FR-7 | Correlation is BY MESSAGE TYPE first, `request_id` as a consistency check. READ replies (e.g. `Version`) carry NO `request_id` echo; a state-changing request triggers a cascade of OTHER-type messages that all echo the request's id (recalling a preset emits `UndoRedo`/`Grid`/`Scene`/... all carrying the same `request_id` before the `SetlistPosition` echo). The reply is the first inbound message whose TYPE matches the request's, and whose `request_id` (if present on both sides) matches too. | Must Have |
| FR-8 | When a READ reply carries no `request_id`, the first same-type pending waiter is satisfied (lowest `request_id` first). This covers the common case where the device answers a READ with a bare same-type message. | Must Have |
| FR-9 | `next_request_id()` draws a fresh id from the same monotonic counter `request` uses, so a caller (e.g. `read_preset`) can tag a fire-and-forget message and later correlate a broadcast against it without colliding with `request` ids. | Must Have |
| FR-10 | `await_broadcast(expected_type, trigger, timeout, match)` registers a type waiter FIRST, then runs `trigger` (e.g. a recall `send`), then blocks for the next matching `expected_type` broadcast. A `match` predicate `(message) -> bool` filters candidates: a right-type message whose predicate returns false is left undelivered, so the waiter keeps waiting for the right one. Raises `Timeout` on no matching broadcast. | Must Have |
| FR-11 | `collect(expected_type, trigger, seconds, match)` fires `trigger` and gathers EVERY matching message for `seconds`, returning them in arrival order. A collector does NOT consume messages - they still reach any waiter or other collector. Used for the folder-enumeration flood a single `File` READ produces. | Should Have |
| FR-12 | The RX/dispatch loop must never die. Per-message decode/parse errors are logged and skipped; the reassembly buffer is reset on a malformed frame or a lost LAST flag so one bad frame cannot wedge the stream. Unknown message types and non-protobuf "raw payload" pushes (e.g. `License`, `CloudLogin`) are logged at debug and dropped. | Must Have |
| FR-13 | Frame-level gzip decompression: if a reassembled payload starts with `1f 8b`, decompress it before protobuf decode. Field-level gzip inside protobuf `bytes` fields is handled at the domain layer (zone 130), not here. Decompression is bounded at `MAX_DECOMPRESSED_MESSAGE_LEN` (8 MiB) independently of the FR-15 reassembly cap, because a compressed body under that cap can still inflate to an unbounded size. | Must Have |
| FR-14 | A FIRST-flagged report begins a new logical message. If it abandons a partial body, reassembly continues but cache continuity is invalidated because a state update may have been lost. | Must Have |
| FR-15 | Reassembly in the long-lived subscribed session is capped at 1 MiB of reassembled body (`MAX_MESSAGE_BODY = 1 << 20`). A legitimate observed message never reaches the cap: the largest is a 150,008-byte `LocalBackup` chunk, while the routine `ModelRepo` reply is ~47 KB gzipped across ~371 reports. The cap is enforced against actual buffered bytes for both an in-progress partial and a just-completed body. A report-count approximation can overestimate a partial because reports need not fill their capacity; the completing LAST/COMPLETE branch must be checked separately so its final bytes cannot bypass the cap. An oversized body is rejected before envelope decoding and reassembly resets for the next FIRST. | Must Have |
| FR-16 | Writes are serialized so one logical message's reports are written as an atomic group (a keepalive cannot interleave between a multi-report message's header and its continuation reports, which carry no header). The write lock is SEPARATE from the state lock; the state lock is never held across blocking device I/O. | Must Have |
| FR-17 | After a `request` wait() times out, a reply can still land in the race window between the timeout and removing the pending entry. Whoever pops the entry "wins": if the RX thread already popped it, it is committed to delivering, so wait a short grace (0.5 s) for that to complete rather than dropping a reply that actually arrived. | Should Have |
| FR-18 | `Session::stop()` signals the background RX and keepalive threads to exit and joins them. `Session::close()` announces disconnect, joins both workers, then explicitly takes and drops the owned link so returning proves the HID handle is gone even if other `Arc<Session>` references remain. Both are idempotent. Joining before dropping matters: closing the handle while the RX thread is inside `read()` can crash. | Must Have |
| FR-19 | The session holds one link for its lifetime. Effective process ownership is claimed by `cortex-host` before opening because a second hidapi open can succeed and wedge the first owner. | Must Have |
| FR-20 | No protocol-version field exists on the wire: a CorOS update can silently break the handshake. Read and cache the device `Version` before announcing the client version, while no other same-type request can race its id-less reply, and surface that cached identity to callers. | Should Have |
| FR-21 | `DeviceStateCache` observes each state-bearing inbound message before collectors or waiters. Observation is non-consuming: the same message both updates the cache and satisfies normal correlation. | Must Have |
| FR-22 | A complete four-row `RecallPreset` replaces the live-preset baseline. Sparse keyed `Grid` messages merge only for established routing, split, parameter, bypass, scene-mode promotion, and removal shapes; an ambiguous or unsupported delta invalidates the live preset rather than guessing. | Must Have |
| FR-23 | Cache values carry a physical-session generation and cache revision. `wait_for_change(after, timeout)` returns only a latest revision token, so a knob burst coalesces instead of accumulating an unbounded raw-message queue. | Must Have |
| FR-24 | A malformed report, abandoned partial message, reassembly error, cap breach, or host-detected disconnect invalidates cached state. A replacement session starts a new generation and old-generation delivery is ignored. | Must Have |
| FR-25 | `Session::open_with_state` / `over_with_state` attach a replacement physical session to one stable cache handle. `Session::is_responsive` uses the same measured ten-second silence limit as request fail-fast so a cache hit cannot conceal an unplug. | Must Have |
| FR-26 | A stream gap remains `Invalidated` even if handshake settlement later finishes. The daemon treats `Invalidated` as a reconnect condition despite continuing heartbeats, excludes in-flight operations, releases the old link, and requires a new subscribed generation to return `Live`. | Must Have |
| FR-27 | A non-empty `NewModels.models` invalidates the current generation's raw and parsed catalog; an empty list does not. A later non-empty `ModelRepo` in that generation replaces it. Stream gaps and generation changes clear catalog state, and old-generation delivery cannot repopulate it. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| NFR-1 | The session layer adds no async runtime dependency; it uses std threads + channels so the leaf crate stays embeddable in the CLI, MCP server, and Tauri backend without dragging in tokio. | Architectural invariant |
| NFR-2 | A healthy subscribed handshake completes in 3.8-3.9 s and remains below 10 s. Each awaited reply is bounded by `timeout`; the adaptive settle has a 2 s floor and 30 s ceiling. | Hardware-observed |
| NFR-3 | The keepalive interval is 1 s, matching Cortex Control's measured 1.04 s cadence. At 1 s a held session remained continuously live for 90 s; at 5 s pushes stopped silently after about 40 s. | Hardware-observed |
| NFR-4 | Pending request waiters, broadcast waiters and collectors use their own synchronisation; no waiter lock is held across blocking device I/O or waits. | Code invariant |
| NFR-5 | Unit tests for correlation logic (type-first matching, the id-less READ-reply fallback, the race-window grace, the stale-seed-push skip) run in CI without hardware. | CI-enforced |
| NFR-6 | `Session` is shared behind `Arc`, owns its removable boxed `HidLink` and worker handles, and is not itself cloned. Explicit `close()` proves link release before replacement. | Code invariant |
| NFR-7 | The RX thread is the sole cache writer. It never invokes caller callbacks and never performs a refresh read while reducing; snapshots use a mutex and revision waiters use a condition variable. | Code invariant |

---

## Acceptance Criteria

- [x] `Session::connect()` performs the paced subscribed handshake and the device begins pushing state.
- [x] `Session::disconnect()` sends `Connection{connected: false}` best effort.
- [x] Keepalive sends `KeepAlive{UPDATE}` about once a second and survives a send failure.
- [x] `request()` correlates id-less READ replies by type and state-changing replies by type plus consistent `request_id`.
- [x] `await_broadcast()` skips a stale seed push and accepts the push matching the trigger.
- [x] `collect()` gathers multiple matching pushes without consuming waiter delivery.
- [x] The RX thread survives malformed input while invalidating continuity.
- [x] Direct fake-link frame-injection coverage proves FIRST-over-partial recovery and exact byte-level 1 MiB cap enforcement, including partial and LAST branches that exceed the cap in their final chunk, the accepted exact boundary, and a body assembled from many short reports well under the cap.
- [x] `Session::stop()` and `close()` join workers, and `close()` explicitly releases the link.
- [x] Correlation, race-window, stale-seed, writer-priority, keepalive and fake-link tests pass without hardware.
- [x] Cache reduction is non-consuming, sparse updates apply transactionally, ambiguous updates invalidate, old generations cannot repopulate new state, and a 135-update burst retains only the latest value.
- [x] The fake-link RX test proves a malformed report invalidates continuity without killing later message delivery.
- [x] Full handshake and a 90-second held idle session verified against a real Quad Cortex (CorOS 4.0.1).
- [x] Host process tests prove request-idle teardown never overlaps an in-flight operation and retains explicit `Session::close()` as the release boundary.
- [x] Daemon admission and status observe `Invalidated` cache phase immediately, refusing new device work as reconnecting before the periodic health poll catches up and without performing device I/O.
- [x] The ignored hardware lifecycle test passed on CorOS 4.0.1: an auto-managed idle exit released HID, removed its endpoint, and permitted a replacement direct version read.

---

## Non-Goals

- The ergonomic `QuadCortex` client API (zone 150) - this zone provides the primitives, the client builds on them.
- USB HID enumeration/open and the minimal synchronous diagnostic (zone 100). This zone consumes `HidLink` and owns session-level link lifetime.
- Framing, flag-driven reassembly, and the trailer-tagged envelope decode (zone 110). This zone receives reassembled payloads.
- The protobuf schema and `prost` build (zone 120). This zone imports the generated types.
- The typed domain model - `BinaryPreset`, `Block`, `Split` (zone 130). This zone passes raw protobuf messages.
- MCP safety surface (zone 300) - exact-target preparation and save confirmation live there; this zone just provides `send`.

---

## Dependencies

- **Crate-internal**: zone 100 (hidapi transport implementation), zone 110 (framing), zone 120 (generated proto types), and `link.rs` (this zone's transport-neutral seam).
- **External (leaf)**: `std::sync` (threads, `Mutex`, `mpsc`), `flate2` (frame-level gzip), the generated `prost` types. No async runtime.
- **Prior art**: `pyquadcortex/pyquadcortex/transport.py` (`Transport` class) and `pyquadcortex/pyquadcortex/client.py::_hello` - ported under MIT with attribution; see `THIRD-PARTY-NOTICES.md`.

---

## Appendix

### Protocol Provenance & Attribution

The connect handshake sequence, the correlation rules, the keepalive, and the broadcast-wait semantics are all ported from `pyquadcortex` (MIT, (c) 2026 Stokes). The recovered `.proto` files that define the 22 subscribe types and the `request_id` field semantics are vendored into `crates/cortex-rs/proto/` under their own MIT SPDX header. Record any derivation in `NOTICE` / `THIRD-PARTY-NOTICES.md`.

The "device gates push behaviour on a valid `cortex_control_version`" finding and the "a minimal `ResetCommsBuffers`+`Connection` is NOT enough" finding are both from `pyquadcortex` live probes, confirmed by capture. The `CC_VERSION = "4.0.1"` constant is the version captured on the wire against CorOS 4.0.1.

### Connect handshake detail (from pyquadcortex, confirmed by capture)

1. `ResetCommsBuffers` with a fresh `session_id` (UUID hex). Device echoes it; correlate by `request_id`.
2. `Version` READ, best effort, before announcing. Cache the non-empty reply while no later `version()` caller can race the device's own id-less `Version` READ.
3. `Version` UPDATE announcing `cortex_control_version: "4.0.1"`. The device gates state PUSH behaviour on receiving a valid CC version.
4. `ModelRepo` READ - fetch and wait for the catalog (~47 KB gzipped, spanning ~371 reports) before sending the rest. The READ is load-bearing and pacing prevents the client from queueing its own transfer behind the subscription burst.
5. `Connection{connected: true}`.
6. 22 subscribe READs (FR-3). This is the subscription that makes the device start pushing each ordinary state type.
7. `CPULoad{CREATE}` with a request id. A `CPULoad` READ is ignored without an error.
8. Settle until non-heartbeat inbound traffic is quiet, with the caller's `settle` as a floor and a hard ceiling.

### Correlation rules (from pyquadcortex, confirmed on hardware)

- Correlation is BY MESSAGE TYPE first, `request_id` as a consistency check.
- READ replies (e.g. `Version`) carry NO `request_id` echo.
- A state-changing request triggers a cascade of OTHER-type messages that all echo the request's `request_id` (recalling a preset emits `UndoRedo`/`Grid`/`Scene`/... all carrying the same id before the `SetlistPosition` echo).
- A `RecallPreset` push triggered by a host recall echoes that recall's `request_id`; the unsolicited seed push carries none. Without matching on the id, the waiter returns whichever `RecallPreset` arrives first, which can lag by one recall when a prior push remains in flight.

### Verification status

The Rust implementation has passed the full hardware smoke: handshake, paced catalog transfer, subscriptions, one-second keepalive, correlation, broadcast waits, CPU-load pushes, and clean shutdown against CorOS 4.0.1. Re-run that smoke after a firmware update because the wire carries no protocol-version field.

### Hardware findings (CorOS 4.0.1)

First contact between this crate's session layer and a real Quad Cortex. Captured with `CORTEX_TRACE=1 cortex device version --session`.

**The device sends the host an unsolicited `Version` READ.** A `Version` READ from the host produces TWO inbound `Version` messages, in this order:

```text
rx Version (290 bytes) request_id=None   <- the reply to our READ
rx Version (2 bytes)   request_id=None   <- the device asking US for our version
```

The 2-byte message is `VersionMessage{action: READ}` and decodes as a structurally valid `VersionMessage` with every informational field absent. This confirms on the wire what `pyquadcortex` documents in `client.py::_hello`, and it has a consequence for [FR-8]:

- READ replies carry no `request_id`, so the id-less fallback matches by TYPE alone.
- The device's own `Version` READ is indistinguishable by type from a reply.
- Therefore **two concurrent `Version` requests risk one of them being satisfied by the device's request rather than by a reply**, yielding a near-empty `VersionMessage` rather than an error.

Mitigations, in preference order:

1. Issue the handshake's best-effort `Version` READ before the UPDATE and cache its non-empty reply, while no other same-type waiter exists.
2. Serve later version queries from that cache rather than issuing concurrent id-less READs.
3. Keep the broadcast predicate that rejects the device's empty `Version{READ}` request.

**Lock-contention diagnosis corrected.** Reducing the RX poll from 2 seconds to 200 ms limited one read but did not prevent the unfair mutex from immediately handing the device back to the reader. The definitive fix is the `writers_waiting` gate: once a writer declares intent, the RX loop stands aside until that write completes. This removed the 47-second idle gaps visible on the wire.

### Handshake performance: client bugs corrected (2026-08-02)

The `ModelRepo` READ is load-bearing. Removing it shortened the handshake and then made every later read time out, so the device gates push behaviour on that request. What was wrong was the explanation for its variable cost: a wire trace disproved the earlier claim that the unit lazily built and cached the catalog.

Two client-side faults caused the 2-102 second spread:

- The RX loop repeatedly reacquired an unfair device mutex and starved the writer. On the wire, the device answered each write in microseconds while the client left the bus idle for up to 47 seconds. A waiting-writer gate removed the starvation; handshake variance collapsed to a stable few seconds and preset-list variance collapsed with it.
- The client fired `ModelRepo`, `Connection`, and the subscribe burst together. That queued roughly 24 requests against the 46 KB catalog transfer and inserted a 4.4-second gap. Waiting for the catalog first reduced request-to-last-report from 5.06 seconds to 0.67 seconds, matching Cortex Control's 0.65 seconds.

`connect_with_progress` reports each step, waits for the catalog before continuing, and gives a waiting writer priority over the reader. The adaptive settle remains bounded by the caller's floor and `SETTLE_MAX`; heartbeat messages do not keep it artificially open.

### Why commands were slow, and what fixed it (2026-08-02)

The writer-starvation and handshake-pacing fixes removed the wild variance, but a one-shot command still paid a correct multi-second handshake for milliseconds of work. The structural fix is `cortex session start`: one subscribed owner pays that handshake once and serves later commands over local IPC.

Measured through the held session, `scene` takes about 0.07 seconds, `grid` 0.14 seconds, status 0.005 seconds, and the already-fetched catalog 0.02 seconds. This is also the correct architecture for the MCP server and GUI because the HID interface permits only one effective owner.

`ConnectMode::Minimal` remains available for a one-shot path that needs no subscriptions; `ConnectMode::Subscribed` is correct for the held session because the pushes keep its cache current. The `ModelRepo` READ remains mandatory in both modes.

The catalog is held only in memory. A transparent disk cache would not remove the mandatory live READ/drain, the held path already serves it in about 0.02 seconds, explicit CLI dump/from-file operations cover offline snapshots, and CorOS alone is not established as a complete content key. The remaining same-CorOS entitlement question is optional protocol research, not unfinished session performance work.

### Heartbeat and liveness (2026-08-02)

`GlobalTempo` arrives continuously in pairs: roughly 0.03 seconds within a pair and 0.5-0.8 seconds between pairs. It is a tempo/metronome heartbeat rather than state, so `HEARTBEAT_TYPES` excludes it and `IoMeter` from the adaptive-settle stamp while dispatching both normally.

The apparently contradictory minute-long silences were caused by this client's old five-second keepalive. At that cadence the device stops pushing after about forty seconds without an error. Cortex Control sends every 1.04 seconds; with this project at one second, a held 90-second session remained continuously live. Silence is therefore a useful failure signal only after the device has first spoken and with the correct keepalive running; request waits use a conservative ten-second threshold because the post-handshake lull reaches 4-5 seconds.

On-unit scene, bypass, and knob edits all push to a subscribed client, as detailed below. A held session can therefore keep a live cache current; reconnect must still invalidate it because edits made while disconnected are unknowable.

### Exclusive ownership, not cumulative degradation

Repeated connect/disconnect cycles do not degrade the unit. The actual collision is concurrent ownership: a second process can open the HID interface without error, after which the held session fails on its next request. Claim the socket or lock before beginning the handshake, and route every command through the held owner.

The subscription is not wasteful in itself - it is what makes the device report on-unit edits, and therefore what makes a cache trustworthy. Paying it once in a held session avoids repeated handshakes and preserves the single-owner invariant.

### On-unit edits push, including knob turns (2026-08-02)

The question that gates a cached persistent connection: does the device tell a subscribed client when the PLAYER changes something on the hardware? If not, a cache of device state silently goes stale and is worse than no cache.

**It does.** Captured while the unit was operated by hand:

| Action on the unit | Pushed |
| --- | --- |
| Turning knobs on several blocks | **135 `Grid`** messages, 23 bytes each - sparse parameter updates in the same shape we send |
| Bypassing and un-bypassing a block | `Grid` at 15 and 17 bytes |
| Changing scene by footswitch | `Scene` and `RecallPreset` |
| (accompanying) | 15 `UndoRedo`, 2 `PresetDirty` |

All `Grid` traffic arrived well after the initial subscription dump had finished, so it is attributable to the hand edits rather than to the handshake.

**Consequence for `cortex session start`.** A held, subscribed session can cache live device state - including parameter values - and keep it correct, because the device reports edits made both by us and by the player. That is what makes the cache trustworthy rather than merely fast.

It also settles the design tension recorded above. The 22-type subscription is expensive per command and is exactly right per session: it is the mechanism by which the cache stays true.

The implementation carries those caveats directly. `PresetDirty` is cached as its own value, including `false` (a proto3 scalar with no presence). A knob sweep is reduced synchronously into one latest snapshot and consumers wait on a revision rather than a message queue. Reconnect invalidates wholesale before backoff and starts a new generation, because edits made while disconnected are unknowable.
