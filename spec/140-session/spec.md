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
### Hardware findings (2026-08-02, CorOS 4.0.1 / firmware d14e / QA00AB123)

First contact between this crate's session layer and a real Quad Cortex. Captured with `CORTEX_TRACE=1 cortex version --session`.

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

1. Do not issue a redundant host `Version` READ during the handshake (already the case - [FR-1] step (b) sends an UPDATE announcing `cortex_control_version`, never a READ, precisely for this reason).
2. Do not run concurrent `version()` calls on one session.
3. A future hardening: have the id-less fallback reject a candidate whose `action` is READ, since a genuine reply is an UPDATE. Not yet implemented - see the tasks file.

**Lock-contention fix validated.** Before this run the RX thread held the device mutex across a 2 s blocking read, so every `send()` waited for the current read to return. The RX poll is now a separate 200 ms `RX_POLL_TIMEOUT` (matching pyquadcortex), bounding the wait a writer can experience. The single-request path above completed with no perceptible delay.

### Why the handshake sometimes takes 35 seconds (2026-08-02)

Reported as "the handshake appears to hang". It does not hang; it waits, and how long depends on how warm the device is.

**The `ModelRepo` READ at step (c) is load-bearing.** It looks like a gratuitous 46 KB fetch in the middle of a handshake and is the obvious thing to optimise away. Removing it was measured: the handshake dropped from ~35 s to **4.2 s**, and then **every read failed** - `active_scene`, `read_current_preset`, and `list_presets` all timed out. The device gates its push behaviour on this request. It is retained deliberately and the code says so.

**The cost is the device's, not the client's.** Report-reassembly throughput was measured during the same run:

| Message | Reports | Time | Rate |
| --- | --- | --- | --- |
| `ModelRepo` | 371 | 4551 ms | 82/sec |
| `ModuleStats` | 179 | 119 ms | 1504/sec |
| others | 2-72 | ~1 ms | 1500-3000/sec |

The RX loop sustains 1500+ reports/sec. `ModelRepo` alone trickles at 82/sec because the unit builds the catalog on demand. Everything after it arrives at full speed.

**Cold versus warm.** Two runs of identical code: 37.2 s on a cold device, 2.2 s minutes later on a warm one. The unit evidently caches the built catalog. Both runs produced working reads.

**Therefore the settle is adaptive rather than fixed** ([FR-1] step (f)). A fixed sleep cannot serve both cases: short enough for the warm path leaves the cold path issuing reads into a 46 KB backlog, where replies queue behind the pushes and time out - which is exactly what "the handshake is broken" looked like. `connect` now waits for the inbound stream to fall silent for `SETTLE_QUIET_PERIOD` (1.5 s), with the caller's `settle` as a floor and `SETTLE_MAX` (30 s) as a ceiling so a permanently chatty device cannot stall the handshake.

**And it reports progress.** `connect_with_progress` emits a label per step. Several seconds of silence reads as a hang regardless of whether it is one, so the CLI surfaces these on stderr without needing `CORTEX_TRACE`.

**Open: an interrupted session is not announced.** Ctrl-C during a handshake never sends `Connection{connected: false}`, so the device keeps pushing to a client that has gone. The next session then contends with that backlog, which is why the first reproduction of this report stalled where an instrumented rerun did not. `ResetCommsBuffers` exists to recover from precisely this and appears to, but a signal handler that announces the disconnect would be better. Not yet implemented.

### Why commands were slow, and what fixed it (2026-08-02)

Reported as "several minutes"; Cortex Control is near-instant by comparison. Measured rather than guessed, and the cause was almost entirely self-inflicted.

**The baseline.** `cortex version`, which needs no handshake, took 0.77 s. `cortex scene`, which does one 9 ms write, took **32.4 s**. So essentially all of it was handshake and teardown, not work.

| Command | Before | After |
| --- | --- | --- |
| `version` (no handshake) | 0.77 s | 0.80 s |
| `scene` (one write) | 32.4 s | ~3 s |
| `grid` | 13.9 s | ~2.7 s |
| `catalog` | 26.4 s | ~1.4 s |
| `presets` | 50.6 s | 5-19 s |

**The subscribe burst was the main cost, and was not needed.** The handshake subscribes to 22 state types, and subscribing is what makes the device dump everything it has - over 600 KB of folder listings on the unit measured. That dump is what a long-lived editor wants, because it then receives state changes unsolicited. A one-shot command does not: it sends its own targeted READ, which the device answers regardless.

Verified by removing it: `presets` went from 50.6 s to 9.4 s returning the same listing, and every read path still worked. Hence `ConnectMode::Minimal` (the default) and `ConnectMode::Subscribed` (which only `probe` uses, since it exists to exercise the full handshake).

This refines the earlier finding rather than contradicting it. The **`ModelRepo` READ** at step (c) is still load-bearing - removing that one breaks every read. It is the 22 **subscribe** READs at step (e) that are optional for targeted reads.

**A bare `File` READ enumerates everything.** `list_presets` sent one and discarded all but the folder it wanted - 399 folders' worth. Naming the folder in the request narrows what the device sends: 14.1 s versus 5.3 s for the same listing. `list_folders` still sends the bare form, because enumerating everything is what it wants.

**The catalog was fetched twice.** The handshake requests `ModelRepo` (load-bearing), and `fetch_model_repo` then requested it again, making the device rebuild and resend 46 KB at 82 reports/sec. The session now captures the first payload it sees and serves it from there: 26.4 s to 1.4 s.

**The keepalive slept uninterruptibly.** A plain 5 s sleep, joined by `stop()`, so every command paid up to 5 s on teardown. Now sliced.

**What remains, and is the device's.** `presets` still varies from 5 s to 49 s across consecutive identical runs, always returning correct data. `pyquadcortex` documents the same: a `File` READ "does not reliably produce one promptly, delivery being lazy", and treats a timeout as "ask again" rather than as an answer. A polling retry (their `wait_for_listing`) is the documented mitigation and is not yet implemented here.

**The structural difference from Cortex Control remains.** It opens ONE session and keeps it, paying the handshake once; we open and tear down a session per command. A persistent session would remove the remaining per-command cost, and is the right shape for the MCP server, which must hold a single connection anyway.

### The device is never quiet, and what that means (2026-08-02)

**`GlobalTempo` is a continuous heartbeat.** Observed arriving roughly every 0.8 s, indefinitely, in pairs. It is the tempo and metronome clock, not state.

This broke the adaptive settle outright. Waiting for 1.5 s of inbound silence can never succeed against a 0.8 s heartbeat, so every subscribed handshake ran to `SETTLE_MAX` - a 30 s wait on a command doing 9 ms of work. `HEARTBEAT_TYPES` now excludes `GlobalTempo` and `IoMeter` from the liveness stamp; they are still dispatched normally, they just do not count as the device having more to say.

**On-unit changes DO push.** Changing scene on the hardware produced `Scene` and `RecallPreset` pushes to a subscribed client. So a cached view of device state CAN be kept current, which is what makes a persistent connection worth building rather than merely faster.

Not yet established: whether turning a knob on the unit produces a `Grid` push. No `Grid` was observed, but no knob turn was confirmed within the window either. Verify before caching parameter values.

### Congestion: cause corrected

An earlier note here claimed that repeated connect/disconnect cycles degrade the device. **That was wrong**, and `pyquadcortex` had already tested it: they opened and abandoned twelve sessions with no goodbye and measured no degradation - the seed push still arrived, subscriptions still fired, and `read_preset` was unchanged (9.04 s to 8.77 s).

The real cause is narrower: **a SUBSCRIBED handshake makes the device dump over 600 KB, and that leaves it busy for whatever comes next.** Measured in sequence: two subscribed `probe` runs at 24 s and 39 s, then a minimal `scene` at 44.6 s inheriting the backlog, then 2.8 s and 4.9 s once it cleared.

So:

- A minimal handshake costs 3-5 s and does not congest the device.
- A subscribed handshake costs tens of seconds and taxes the next command too.
- `pyquadcortex` reports 2.03-3.80 s handshakes, and ~9 s for `read_preset`, so the device being slow at some operations is normal rather than a fault of ours.

**This is the argument for a persistent connection, stated precisely.** The subscription is not wasteful in itself - it is what makes the device report on-unit edits, and therefore what makes a cache trustworthy. It is wasteful *per command*. Pay it once in a held session and both problems disappear: no repeated dumps, and a cache that stays correct.

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

**Consequence for `cortex connect`.** A held, subscribed session can cache live device state - including parameter values - and keep it correct, because the device reports edits made both by us and by the player. That is what makes the cache trustworthy rather than merely fast.

It also settles the design tension recorded above. The 22-type subscription is expensive per command and is exactly right per session: it is the mechanism by which the cache stays true.

Caveats to carry into the implementation:

- `PresetDirty` marks the grid as having unsaved changes. It is the signal that a cached preset no longer matches the stored slot.
- A knob sweep produces a BURST of `Grid` messages (135 for a handful of knobs). The cache should apply them, not queue work per message.
- Nothing here says what happens across a RECONNECT. If the connection drops, edits made while away are invisible, so a reconnect must invalidate the cache wholesale rather than resume.
