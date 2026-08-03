---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["roadmap", "planning"]
---

# cortex-rs Roadmap

> Stable IDs, never renumbered - reference them in commits, PRs, and release notes.
>
> Status legend: `[x]` done, `[~]` in-progress, `[ ]` planned. Future items live under `## Future` and promote to Planned when scheduled.
>
> Completed items are RETAINED here until the first release, because until then this file is the only place that records what is built and what is merely specified. Once a CHANGELOG exists to carry that record, done items move out and this becomes a pure backlog.
>
> **`[x]` means the code exists and passes the local gate. It does NOT mean hardware-verified.** Anything touching the wire stays provisional until its hardware smoke item passes against a real Quad Cortex.
>
> Hardware-verified so far (2026-08-02, CorOS 4.0.1 / firmware `d14e` / serial `QA00AB123`): the transport, framing, and envelope; `version` by both the one-shot transport path and the session path; the full connect handshake, keepalive, and clean disconnect; the `request`, `await_broadcast`, and `collect` correlation primitives; the `active_scene`, `read_current_preset`, `list_presets`, `list_folders`, and `read_preset` read paths; and the `recall_preset` and `switch_scene` navigation writes. **No grid-edit or file-write path has ever run against hardware, because none is implemented yet.**

## Where each ID lives

This file is the single place to see where the project is up to. Each ID names work; the zone spec beside it says what that work must do and how it is designed.

| ID prefix | Zone | What it covers |
| --- | --- | --- |
| PROT-001 | [100-transport](100-transport/spec.md) | USB HID open, read/write, the benign write STALL |
| PROT-002 | [110-framing](110-framing/spec.md) | Report framing, flag-driven reassembly, the trailer envelope |
| PROT-003 | [120-proto-schema](120-proto-schema/spec.md) | Vendored `.proto`, `prost` build, message types |
| PROT-004 | [130-domain-model](130-domain-model/spec.md) | `DeviceKind`, `Row`, `Catalog`, and the typed preset/grid views |
| PROT-005 | [140-session](140-session/spec.md) | Handshake, keepalive, correlation, broadcast waiting |
| PROT-006 | [150-client](150-client/spec.md) | The ergonomic `QuadCortex` API |
| PROT-007 | [150-client](150-client/spec.md) | Capture and IR export/import (investigation) |
| CLI-00x | [200-cli](200-cli/spec.md) | The `cortex` command surface |
| MCP-00x | [300-mcp](300-mcp/spec.md) | The MCP server and its safety boundary |
| GUI-00x | [400-gui](400-gui/spec.md) | The Tauri desktop app |
| DOCS-00x | - | The documentation site and the agent-facing model reference |
| ENG-001 | [500-dx-tooling](500-dx-tooling/spec.md) | Scripts, lint, test |
| ENG-002 | [600-ci-release](600-ci-release/spec.md) | CI and release |
| ENG-003 | [900-project-governance](900-project-governance/spec.md) | Licensing, attribution, legal hygiene |
| ENG-004 | [001-overview](001-overview/spec.md) | Traceability |
| ENG-005 | - | `s/usb-trace`, for observing the official client on the wire |

!!! note

    Progress is tracked HERE and nowhere else. Zone folders hold `spec.md` (what it must do) and `design.md` (how it is built and why), but no `tasks.md` - see [001-overview/spec.md](001-overview/spec.md#progress-tracking) for why that convention was dropped.

## Protocol and Crate (PROT)

The bottom-up port of the Cortex Control USB HID protocol from `pyquadcortex` (MIT) into a Rust leaf crate.

### PROT-001: Transport layer (zone 100)

- [x] `Transport::open(DeviceKind)` - find and open the Quad Cortex on the USB bus
- [x] `Transport::write(&[u8])` - send a message, split into HID frames, swallow the STALL
- [x] `Transport::read(Duration)` - read one 129-byte input report
- [x] `Transport::request(message_type, payload, timeout)` - synchronous request/response with reassembly + gzip
- [x] Hardware-verified: `cortex version` reads CorOS 4.0.1 / firmware d14e from a real Quad Cortex

### PROT-002: Framing layer (zone 110)

- [x] `ReportId`, `Flags`, `Frame`, `FrameReassembler` value types
- [x] `Frame::parse` - strip report ID / len / flags, validate
- [x] `FrameReassembler::feed` - flag-driven reassembly state machine
- [x] `encode_message(message_type, payload)` - trailer + chunk + wrap
- [x] 10 unit tests (round-trip, multi-frame, error cases, encode/decode symmetry)

### PROT-003: Proto schema (zone 120)

- [x] Vendor `Preset.proto` and `ProductionAutomation.proto` (MIT, with SPDX headers)
- [x] `build.rs` compiling via `prost-build`
- [x] Add `package cortex_protobuf_v2` to `Preset.proto` so cross-file `Model` references resolve
- [x] Generated `proto` module with all 71 `CortexMessageType` variants and all message structs
- [ ] **PROT-003.1**: Curate the message-type registry - map each `CortexMessageType` to its generated struct, so the session layer can dispatch by type tag

### PROT-004: Domain model - core (zone 130)

- [x] `DeviceKind` enum (QuadCortex, NanoCortex) with `vid_pid()`
- [x] `Message` struct with `parse()` (trailer strip, message-type extraction)
- [x] `TRAILER_LEN = 8` constant
- [ ] **PROT-004.1**: `Preset` newtype wrapping `proto::BinaryPreset` with ergonomic field access
- [~] **PROT-004.2**: `Grid`. The `Row` newtype exists (`from_wire` / `from_screen`, refusing screen row 0) and encodes the numbering trap. A read-side `Grid` view over a preset is still planned. NOTE: `grid.rs` is taken by the message builders, so the domain view needs another home
- [ ] **PROT-004.3**: `Block` struct (row, column, model_id, params)
- [ ] **PROT-004.4**: `Scene` struct (index, label, color, bypass state)
- [x] **PROT-004.5**: `Catalog` - HARDWARE-VERIFIED. Container confirmed as `gzip(tar(ModelRepo.xml))`; parses 533 models, 31 categories, 3809 parameters, with the vendor's `tm` attribution carried verbatim
- [~] **PROT-004.6**: Helpers. Done: `slot_to_position` (+ checked variant), `position_to_slot`, `input_level_db`, `db_to_input_level`, `preset_has_block`. Planned: `blocks()`, `splits()`, `free_rows()`, `row_status()`
- [x] **PROT-004.7**: Constants: `UNITY_LEVEL`, `USER_SETLIST_ROOT`, `USER_SETLIST`, `SCENE_UNLABELLED`, `BANKS`, `SLOTS_PER_BANK`, `SETLIST_SLOTS`

### PROT-005: Session layer (zone 140)

The background RX thread, connect handshake, keepalive, and request/response correlation.

- [x] **PROT-005.1**: `Session` struct holding the shared `HidDevice`, a background RX thread, and the pending-request/broadcast-waiter maps
- [x] **PROT-005.2**: RX thread - read frames, reassemble, decode (gzip if needed), dispatch by type tag to waiters; never dies on a malformed message
- [x] **PROT-005.3**: `request(message, timeout)` - assign request_id, register waiter, send, block for correlated reply (type-first, request_id consistency check)
- [x] **PROT-005.4**: `await_broadcast(cls, trigger, timeout, match)` - register type waiter, fire trigger, block for matching broadcast
- [x] **PROT-005.5**: `collect(cls, trigger, seconds, match)` - gather every matching message for a duration. Collectors observe rather than consume, so a message still reaches any waiter
- [x] **PROT-005.6**: Keepalive thread - every 5s send `KeepAlive{UPDATE}`, swallow failures
- [x] **PROT-005.7**: Connect handshake - `ResetCommsBuffers` + `Version UPDATE` (announce `cortex_control_version: "4.0.1"`) + `ModelRepo READ` + `Connection{connected: true}` + 22 subscribe READs + 2s settle
- [x] **PROT-005.8**: `disconnect()` - send `Connection{connected: false}` (best effort)
- [x] **PROT-005.9**: Write serialization - a `Mutex` around device writes so a keepalive cannot interleave between a multi-report message's frames
- [x] **PROT-005.10**: 1 MiB reassembly cap - if the buffer exceeds this without completing, reset (defense against a lost LAST frame)
- [x] **PROT-005.11**: Hardware smoke test - VERIFIED 2026-08-02 against CorOS 4.0.1 / firmware d14e / QA00AB123. Handshake completed in 2.2 s; state pushes flowed; `active_scene`, `read_current_preset`, and `list_presets` all answered; disconnect and thread join clean. Run with `cortex probe`

### PROT-006: Client API (zone 150)

The ergonomic `QuadCortex` struct - the Rust equivalent of pyquadcortex's 60+ methods.

- [x] **PROT-006.1**: `QuadCortex` struct wrapping `Arc<Session>`, lifecycle (connect/disconnect/close, Drop)
- [x] **PROT-006.2**: `version()` - wired through `Session::request` (correlated by type, no request_id echo)
- [x] **PROT-006.3**: Catalog - `fetch_model_repo` + `Catalog::parse`. HARDWARE-VERIFIED 2026-08-02: parsed the real payload from CorOS 4.0.1 (533 models, 31 categories, 318 with vendor attribution). Container confirmed as `gzip(tar(ModelRepo.xml))` - 46,704 bytes gzipped, 558,592 tar, 556,732 XML. Wired into `cortex preset` so blocks show names, and into `cortex catalog` for search by model name OR by the gear it evokes
- [~] **PROT-006.4**: Read operations. HARDWARE-VERIFIED 2026-08-02: `read_current_preset` (live grid, no side effects), `read_preset` (recall + capture the echoing push), `active_scene`, `list_presets`, `find_preset`, `list_folders` (via `collect`), plus the `PresetEntry`/`Folder` value objects. Planned: `captures`, `list_irs`, `recents`, `favorites`, `pinned_models`, `master_volume`, `looper`, `tuner`, `io_settings`, `settings`, `global_eq`, `mode`
- [~] **PROT-006.5**: Navigation. HARDWARE-VERIFIED 2026-08-02: `recall_preset` (grid swap confirmed by read-back), `switch_scene` (confirmed by `active_scene`). Planned: `copy_scene`, `set_scene_label`, `set_scene_color`
- [~] **PROT-006.6**: Grid write. HARDWARE-VERIFIED 2026-08-02: `set_block` (echo-verified, cross-checked by read-back), `set_param` (catalog name resolution, read-back confirmed), `remove_block` (DELETE action, read-back confirmed). ALSO hardware-verified 2026-08-02: `set_bypass` (all eight scene slots at once), `set_chain_input`, `set_chain_output`, `set_split` (including the odd-row refusal), and `set_param_in_scene` (the three-message promote/switch/write sequence, confirmed by a per-scene read-back). Not implemented: `move_block`, `write_preset`. The BlockRefused path is now fully hardware-verified in BOTH directions, and provoking a real refusal found a false-positive bug: a missing echo was treated as proof of refusal when echo latency merely varies. The grid read-back is now ground truth and the echo a fast path
- [ ] **PROT-006.7**: Splitter/mixer/lane/gate: `set_splitter_param`, `set_mixer_param`, `set_lane_output`, `set_input_gate`, `set_split`, `set_split_mute`
- [ ] **PROT-006.8**: Tempo: `set_tempo_param`, `set_tempo_option`, `set_tempo_subdivision`, `set_metronome_sound`, `set_metronome_routing`, `set_time_signature`, `set_tempo_led`, `set_metronome_volume`
- [ ] **PROT-006.9**: Stomp/expression/MIDI: `set_stomp_assignment`, `clear_stomp_assignment`, `set_stomp_momentary`, `set_stomp_label`, `set_expression`, `set_expression_bypass`, `set_midi_out`, `set_preset_load_midi_out`
- [ ] **PROT-006.10**: File ops: `save_current_preset`, `delete_preset`, `move_preset`, `create_setlist`, `delete_setlist`, `copy_preset`, `duplicate_setlist`, `wait_for_listing`
- [ ] **PROT-006.11**: Captures/IRs: `set_capture`, `set_ir`, `show_capture_dialog`
- [ ] **PROT-006.12**: Global settings: `update_settings`, `set_hold_timing`, `set_scene_bypass_behavior`, `set_master_volume_assignment`, `set_global_bypass`, `set_global_eq`, `set_mode_cycle`, `set_mode`, `set_gig_view`, `show_tuner`, `set_tuner_input`, `set_tuner_mute`, `set_tuner_reference`
- [ ] **PROT-006.13**: I/O ports: `set_input_port`, `set_output_port`, `set_usb_port`, `set_midi_thru`, `set_output_pairing`
- [ ] **PROT-006.14**: Pinning/favorites: `pin_model`, `unpin_model`, `add_favorite`, `remove_favorite`
- [ ] **PROT-006.15**: Module-level helpers: `blocks()`, `splits()`, `input_chain_rows()`, `stomp_assignments()`, `midi_out()`, `tempo_params()`, `param_options()`, `free_rows()`, `row_status()`, `params_equal()` - `slot_to_position`, `position_to_slot`, `input_level_db`, `db_to_input_level` done
- [~] **PROT-006.16**: Hardware smoke test. Done 2026-08-02 for the implemented surface: `version`, `active_scene`, `read_current_preset`, `list_presets`, `list_folders`, `recall_preset`, `switch_scene`, `read_preset` - all against CorOS 4.0.1 / d14e / QA00AB123, with the unit restored to its starting state. Trap 14 (a recall resets the active scene) was confirmed live. Outstanding: `set_param` and `save`, which are not implemented yet (PROT-006.6, PROT-006.10)

### PROT-008: Session performance

Commands were taking tens of seconds for milliseconds of work. Most of that is fixed (see [140-session/spec.md](140-session/spec.md)); what is left is listed here.

- [x] **PROT-008.1**: `ConnectMode::Minimal` - skip the 22-type subscription, which is what makes the device dump 600 KB and is not needed for a targeted read
- [x] **PROT-008.2**: Capture the handshake's `ModelRepo` payload instead of requesting it a second time
- [x] **PROT-008.3**: Name the folder in a `File` READ rather than enumerating all 399
- [x] **PROT-008.4**: Interruptible keepalive sleep, so `stop()` does not wait up to 5 s
- [x] **PROT-008.5**: Reduce the spread on a `File` READ listing. **Resolved, and the diagnosis was wrong from the start.** The spread was not the device delivering lazily; it was our own RX loop starving the writer, so the READ sat unsent. Fixed in `session.rs` by a writer-priority gate. Measured `cortex presets` before 5.4/8.1/10.1/11.4/18.2 s, after 5.34/5.33/5.38/5.34/5.37 s - the floor unchanged, the tail gone entirely. See the ENG-005 note below for how the wire capture identified it.
  - **Retrying was tried and made it worse**, which in hindsight was the clue. Re-firing the READ added writes, and writes were the thing being starved - so the "fix" fed the actual fault. At the time this was read as the device rebuilding the listing repeatedly, which was plausible and wrong. Recorded here because the measurement was sound even though the explanation was not. Implemented as a re-fire every 3 s with a single waiter held across attempts, measured A/B over 5 runs each of `cortex presets`:

    | | min | median | max | mean |
    | --- | --- | --- | --- | --- |
    | without retry | 5.4 s | 10.1 s | 18.2 s | 10.6 s |
    | with retry (3 s) | 9.8 s | 36.9 s | 65.1 s | 36.3 s |

    About 3.5x worse. Three of the five retry runs exceeded the baseline's worst run. The ordering confound runs the wrong way to explain it away: the baseline arm ran *second*, so drift would have penalised it, and it still won. The mechanism is plain in hindsight - each re-send makes the device build the whole 256-slot listing again, adding exactly the load that made it slow. A `File` READ is not a cheap poll.
  - Untested variants that are not ruled out: a much longer retry interval (10 s+), or retrying only after evidence the request was dropped rather than on a timer. Neither is worth trying until there is a way to tell "dropped" from "still working", which the protocol does not currently give us.
  - The `pyquadcortex` `wait_for_listing` approach was the original motivation here. Whatever it does, a naive periodic re-READ is not it.
- [ ] **PROT-008.6**: `cortex connect` - a persistent, subscribed session. **In progress: the session holds and serves, the cache and health work do not exist yet.** Cortex Control is fast because it opens ONE session and keeps it; we pay a handshake per command. It is also the right shape for the MCP server, which must hold a single connection anyway, and for the GUI. Measured against a held session: `scene` 0.07 s, `grid` 0.14 s, `--status` 0.005 s, against a 3-5 s direct baseline.
  - [x] **008.6.1**: `cortex connect` holds a `ConnectMode::Subscribed` session and owns the HID interface. Subscribing is expensive per command and correct per session: it is how the device reports edits made by the player
  - [x] **008.6.2**: A unix socket at `$XDG_RUNTIME_DIR`, line-delimited JSON, reusing the existing `--format json` output types so client and daemon share one contract
  - [ ] **008.6.3**: Lifecycle: `--status` (done), clean `--stop` that announces the disconnect (done, and bounded by a watchdog - see below), stale-socket detection (done), **idle timeout (outstanding)**
  - [ ] **008.6.4**: **Health reporting.** Detect a dropped or unresponsive device and say so, rather than hanging. Reconnect with backoff, reporting each attempt.
    - **The silence was ours. Root cause found and fixed: the keepalive interval was 5 s.** A capture of Cortex Control shows it sending a keepalive every 1.04 s (681 over 708 s), and its session never quiet for more than **0.11 s** across a 60 s idle. Our 5 s interval was set from a code comment asserting that was what Cortex Control did; it is not. At 1 s, a 90 s idle now reads `last_message_seconds: 0` throughout, where it previously climbed past 51 s.
    - **The withdrawn fail-fast is worth rebuilding now, and its original premise was right.** The design was: while a wait is blocked, poll every 0.5 s and abandon the request once nothing has arrived for 5 s - purely observational, sending nothing. It was withdrawn because it refused a healthy `version` request, but that was the keepalive bug producing genuine silence on a session we had starved. Silence is conclusive again. Rebuild with the `has_heard_from_device` guard intact, since nothing has arrived before a session's first message and that must not read as a fault.
    - This also resolves the apparent contradiction with `GlobalTempo` arriving every 0.8 s indefinitely - the observation that broke the adaptive settle. Both were real: the heartbeat is continuous while the client keeps the session alive, and stops when it does not. Nothing about the device switches; we did. See `spec/140-session/spec.md`.
    - `status` therefore reports `last_message_seconds` raw, with no verdict attached, and the doc comments that used to present it as a liveness signal have been corrected.
  - [ ] **008.6.5**: Cache device state, kept current by the subscription. Verified pushable: parameter values (`Grid`), bypass (`Grid`), scene (`Scene`/`RecallPreset`), dirty state (`PresetDirty`). Plus static data: the catalog and folder listings
  - [ ] **008.6.6**: **Invalidate the cache wholesale on reconnect.** Edits made while disconnected are invisible, so resuming a stale cache would silently lie
  - [x] **008.6.11**: **`Version` READ before announcing our own**, mirroring Cortex Control. Costs ~0.8 s (handshake 2.2 s -> 3.0 s, three consecutive runs) and buys two things: `connect --status` can report the unit's serial and `CorOS` version, which it previously left `None`, and `cortex version` through the daemon is served from that cache in **0.002 s** rather than 2.86 s. It also removes a documented race - `Version` READ replies carry no `request_id`, so a later caller's reply is indistinguishable from the handshake's own announce.
  - [x] **008.6.12**: **`CpuLoad` for a live DSP-load display.** Added to the subscribe set and exposed as `Session::cpu_load()` and `cortex cpu`, with a typed view carrying total load plus a per-column breakdown flagged by DSP core (`is_on_core2` - the QC splits the grid across two cores).
    - **Asked for with CREATE, not READ.** Every other subscribe is a plain READ and gets answered; a READ for `CPULoad` is silently ignored, and that cost an afternoon of looking for the difference elsewhere (keepalive rate, missing `CloudProduct`, a UI toggle). Cortex Control sends action `CREATE` with a `request_id`, which on the wire is a single field 2 and no action field at all - proto3 omits defaults, and `CREATE` is 0. Reading it as "create a subscription" rather than "read a value" also makes more sense of a message whose reply is a continuous stream.
    - Found by `cortex decode-trace --verbose` (ENG-005.3) on its first use, by putting our request and CC's side by side: `field 1: varint 3` against `field 2: varint 2`. Nothing about the message sizes or types differed, so no amount of staring at the message log would have shown it.
    - The first push lands about 8 s after the request, not immediately - long enough that an early check reads as failure. Verified working: total 54.8 % with a per-column breakdown across four rows, second-core columns flagged.
  - [ ] **008.6.7**: Commands address the daemon when it is running. `version`, `scene` and `grid` do; `presets`, `folders`, `recall`, `probe` and the `set-*` edits still open the device for themselves and are refused while a daemon holds it.
    - **Correction, hardware-verified:** the original plan had `version` keep addressing the device directly, on the reasoning that it needs no handshake. It cannot. A direct open alongside a held session wedges the device (see 008.6.9), and `version` was the command that proved it. It now routes when a daemon is running and goes direct only when none is - verified byte-identical output on both paths, 2.9 s routed. The daemon answers in the same `DeviceVersion` shape rather than a `{:?}` dump, so the two paths are one format, not two
  - [x] **008.6.8**: Fall back to a direct `Minimal` session when no daemon is running, so single commands still work standalone
  - [x] **008.6.9**: **Refuse to open the device while a daemon holds it.** Hardware-verified the hard way: a one-shot command that opened the device alongside the held session left every later read on that held session timing out. Nothing errored at the point of the collision - the damage only showed on the next request, which is what makes refusing loudly worth the code. `ensure_device_free()` is the CLI-side expression of the exclusive-access invariant. The socket is bound *before* the handshake, because the handshake window (33 s on an unsettled device) is when the daemon owns the interface but a late `bind()` would leave `is_running()` answering false.
    - The device also went quiet at that moment. That was discounted for a while, because our own sessions were falling silent for an unrelated reason (too slow a keepalive, since fixed - 008.6.4). The read failures carry the finding on their own either way
  - [x] **008.6.10**: **Handshake time varied hugely between identical runs. Resolved: it was our RX loop starving the writer, not the device.** Five consecutive runs after the fix: **2.2 s every time**, against a prior range of 2.2-102.7 s. The step breakdown now shows every handshake message leaving at t=0, with the only remaining cost the 2 s settle window we choose ourselves.
    - The wire evidence: on one 102.7 s handshake the bus was silent for 46.8 s between our first and second messages, while the device answered every write in 217 us. Handshake steps 2-4 are fire-and-forget `send()` calls, so the delay surfaced in the progress labels as though the device were slow to reply - when in fact nothing had been sent yet. That misreading is why this looked like device behaviour for so long.
    - Everything below is the reasoning from before the cause was known. Kept because the measurements were sound and only the explanations were wrong.
    - **This is almost certainly the same phenomenon as PROT-008.5**, which records the same 5-50 s spread on a listing read because delivery is lazy. The observed handshake range (5.6-52.1 s) matches closely, and the handshake performs exactly those reads. Treat them as one problem.
    - It does **not** follow that they have one fix. Retrying was the obvious candidate and it was measured to be about 3.5x *worse* (see 008.5). Whatever the mechanism is, asking the device again is not the lever.
    - Two hypotheses were entertained and neither survives the data. **Collision residue:** an earlier run of 7.4 s, then 32.9 s, then 52.1 s across sessions separated by the 008.6.9 collisions looked like monotonic degradation; against a 4x idle spread it is three samples of a noisy distribution. `pyquadcortex` had already ruled out plain reconnection (12 abandoned sessions, no degradation). **DSP load** (the unit was being played during those runs) remains plausible and untested, but distinguishing it needs many samples per arm given the variance, so it is not worth chasing before 008.5 lands.
    - Why it matters beyond curiosity: every timeout in the crate (10 s, 15 s, 30 s) was chosen against a handful of runs on an idle unit. The 52.1 s handshake still succeeded only because those budgets are per-step rather than total. Budgets set against a distribution this wide need justifying from its tail, not its median
- [x] **PROT-008.13**: **Pace the handshake: wait for the catalog before sending the rest.** Firing the catalog READ, `Connection` and the 22 subscribes together (~24 requests inside a millisecond) makes the device serialise them against a 46 KB transfer, and the catalog stalls behind the queue the client created - measured as a single 4.4 s gap mid-transfer, with the reports either side 0.6 ms apart. Waiting first: **0.67 s**, against Cortex Control's 0.65 s, from 5.06 s. Handshake goes 3.0 s -> 3.8 s (three consecutive runs) and the catalog is cached by the time it completes, where it previously arrived four seconds later.
- [ ] **PROT-008.7**: Cache the catalog on disk, keyed by CorOS version. It changes only on a firmware update or a new capture

### PROT-007: Capture and IR export / import

Neural Captures and user IRs live only on the unit and in Neural's cloud. No existing tool - official or community - can export a capture to a local file, so a player's own captures cannot be backed up, version-controlled, or moved between units. They are the player's OWN data, which makes this the least legally fraught significant feature on this page and arguably the most valuable.

Status: **investigation, not yet designed.** The wire path is unconfirmed. `FileMessage` carries `ir_payload` and `preset_payload` `bytes` fields, and `LocalBackup`, `CloudBackup`, and `BackupsForward` message types exist in the recovered schema, so a route probably exists - but which one carries capture audio data, and in what format, is unknown.

- [ ] **PROT-007.1**: Establish whether a capture's payload can be read over the wire at all, and by which message type. Capture with `CORTEX_TRACE` while Cortex Control performs a backup
- [ ] **PROT-007.2**: Identify the on-wire container and whether it is gzipped, chunked, or both (the `ModelRepo` precedent is gzip inside a `bytes` field, ~47 KB over ~371 reports)
- [ ] **PROT-007.3**: `export_capture(key, path)` - write one capture to a local file
- [ ] **PROT-007.4**: `import_capture(path)` - write a capture back to the unit. **Destructive; gate behind confirmation and the MCP safety surface**
- [ ] **PROT-007.5**: Same for user IRs (`list_irs` already enumerates them)
- [ ] **PROT-007.6**: `cortex capture export` / `import` CLI surface
- [ ] **PROT-007.7**: Decide and document a container format. Prefer something self-describing that records the source unit, CorOS version, and capture metadata, so a file is still meaningful years later

Do not ship import before export has been round-tripped on hardware: writing a malformed capture to the unit is the most plausible way this project could damage a user's data.

## Docs (DOCS)

### DOCS-001: Documentation site

A Zensical site per house-style [docs.md](https://github.com/marcus-pacharanero/house-style/blob/main/docs.md), served by `s/docs`.

- [x] **DOCS-001.1**: Zensical 0.0.52 scaffold, `s/docs`, artifact-based Pages deploy with path filters. Builds clean
- [x] **DOCS-001.2**: `docs/install.md` - udev rule with the reasoning, the exclusive-HID gotcha, `s/install`, completions, and a first check
- [x] **DOCS-001.3**: `docs/walkthrough.md` - every output captured from real hardware, nothing invented. Plus `docs/cli-reference.md`, GENERATED from `--help` by `s/docs-cli-reference` so it cannot drift
- [x] **DOCS-001.4**: `docs/protocol.md` - the wire, the handshake, correlation, the catalog, and the grid traps, with this project's own measurements marked as such
- [x] **DOCS-001.5**: `docs/runbook-hardware-smoke.md` - ten checkpointed steps ending in a restore, with the known gaps listed

### DOCS-002: Agent manual - factory preset reference

A per-device, per-CorOS reference of the factory presets: what each is modelled on, and how to get a usable sound out of it. Aimed at agents driving the MCP server, who otherwise have no idea that "Brit 2203" is a Marshall-style voicing.

- [ ] **DOCS-002.1**: Generate the raw preset inventory from the device rather than transcribing it - `cortex presets --setlist "/opt/neuraldsp/Factory Library"` already emits all 256 names. Keep it a build step so a CorOS update regenerates it
- [ ] **DOCS-002.2**: Key the reference by device AND CorOS version; factory content changes between releases and a stale mapping is worse than none
- [ ] **DOCS-002.3**: Setup tips per preset - the genuinely additive part, since the gear mapping is no longer ours to write (see below)
- [ ] **DOCS-002.4**: Expose it to the MCP server so an agent can resolve intent ("something like a cranked Plexi") to a slot

**This item shrank substantially on 2026-08-02.** The device's own catalog carries a `tm` attribute holding **Neural DSP's own attribution** for each model - `Based on Marshall(R) JCM800(R)`, `Based on ProCo(R) Rat(R)`, `Based on Universal Audio(R) 1176(R)` - on 318 of 533 models. So the "what is this modelled on" mapping does not need writing at all: it ships with the unit, in the vendor's own carefully-worded form, and `cortex catalog --search marshall` already surfaces it.

That changes the plan in three ways:

1. **Never paraphrase the `tm` string.** Reproduce it verbatim. It is Neural DSP's statement about other companies' marks; rewording it, or presenting our own mapping as authoritative, is both less accurate and less defensible. The crate surfaces it as `Model::based_on` and the CLI prints it unchanged.
2. **The remaining work is genuinely additive only** - setup tips, and mapping *presets* (which the `tm` data does not cover) to the *models* they contain, which the catalog does let us resolve.
3. **This is a runtime lookup, not a document to write.** An agent can already resolve "something like a cranked Plexi" by searching the live catalog, which stays correct across CorOS updates for free.

Remaining constraints:

- **Trademark caveating is still mandatory for anything WE write.** Fender, Marshall, Mesa/Boogie, Vox and the rest are other companies' marks. Follow the industry norm every modelling vendor and community site uses: describe what a preset is *evocative of*, never imply endorsement, licensing, or that the model IS the amp. Neural DSP's own naming is deliberately oblique ("Brit 2203", not "Marshall JCM800") and our docs should not undo that by publishing a decode table presented as authoritative.
- **Preset NAMES are factual interoperability information; preset DATA is Neural DSP's.** Listing names and our own commentary is fine. Committing extracted factory preset payloads into this repo is not - see the legal hygiene section of AGENTS.md.

## CLI (CLI)

The `cortex` command-line surface over the crate.

### CLI-001: Scaffold and version

- [x] `cortex version` - reads device firmware, prints all fields
- [x] `cortex completions <shell>` - bash, zsh, fish, powershell
- [x] `cortex --version` / `-V` - standard version flag
- [x] SIGPIPE reset, `arg_required_else_help`
- [x] Clap derive, thin main.rs, all behaviour in crate

### CLI-002: Format and output

- [x] **CLI-002.1**: `--format text|json` global flag, honoured by every command
- [x] **CLI-002.2**: `cortex version --format json` - structured JSON. The output types are defined in the CLI rather than serialising the prost types, so the JSON is an interface with stable field names rather than a wire representation. Two fields are renamed to what they actually hold (`coros_version`, `wireless_firmware_checksum`), with the vendor's misleading names recorded in the type's docs
- [x] **CLI-003.10**: `cortex grid [--params]` - the LIVE grid, read without side effects. Distinct from `cortex preset --slot X`, which reads a STORED slot and can only do so by recalling it, discarding unsaved edits
- [x] **CLI-003.11**: `cortex set-param` / `set-bypass` / `set-block` / `remove-block`, taking rows as the unit LABELS them (1-4)
- [ ] **CLI-002.3**: `--schema` / `--print-schema` - JSON Schema of a command's inputs
- [x] **CLI-002.4**: Data on stdout, hints on stderr - every command follows this; progress, warnings, and handshake steps all go to stderr so output stays pipeable

### CLI-003: Preset and scene commands

All HARDWARE-VERIFIED 2026-08-02 against CorOS 4.0.1 / d14e / QA00AB123. Named without the `list-` prefix, since the noun already reads as a listing and `cortex presets` is what a user reaches for.

- [x] **CLI-003.1**: `cortex recall --slot <slot> [--setlist <path>] [--factory]`
- [x] **CLI-003.2**: `cortex scene --index <0-7>` - zero-based, where the unit labels scenes A-H
- [x] **CLI-003.3**: `cortex preset --slot <slot>` - recalls and prints the preset, with each block NAMED via the catalog and the vendor's attribution shown
- [x] **CLI-003.4**: `cortex presets [--setlist <path>] [--include-empty]`
- [x] **CLI-003.5**: `cortex folders` - all 399 folders, via the session's `collect`
- [x] **CLI-003.6**: `cortex probe` - handshake plus every read path, the hardware smoke test
- [x] **CLI-003.7**: `cortex catalog [--search <text>] [--model <id>] [--dump <file>] [--from-file <file>]`
- [x] **CLI-003.8**: `CORTEX_TRACE=1` - stderr tracing of inbound traffic and handshake steps
- [ ] **CLI-003.9**: `cortex capture` / `cortex ir` - export and import (blocked on PROT-007)

### CLI-004: Distribution

- [ ] **CLI-004.1**: `s/version++` script - bump version across Cargo.toml in one release commit
- [ ] **CLI-004.2**: auto-tag workflow - version bump on main creates `v<x.y.z>` tag
- [ ] **CLI-004.3**: crates.io publish workflow (PROT layers must be complete first)
- [ ] **CLI-004.4**: cargo-dist release pipeline (archives + installers)
- [x] **CLI-004.5**: `cortex completions install` - detects the shell, writes to `~/.zfunc` (zsh) or the conventional directory, prints the one-time setup, never edits startup files
- [x] **CLI-004.6**: `s/install` - installs from `crates/cortex-cli` (the workspace root has no `[package]`, so `cargo install --path .` fails), with a udev-rule preflight

## MCP (MCP)

The `cortex-mcp` MCP server for agentic patch editing. Greenfield - no MCP server for any Neural DSP hardware exists.

### MCP-001: Safety surface

- [ ] **MCP-001.1**: Read and recall are free; saving is always explicitly confirmed
- [ ] **MCP-001.2**: Never write to the factory setlist; restrict saves to a designated scratch range of USER slots unless overridden
- [ ] **MCP-001.3**: Back up the target slot (`read_preset`) before overwriting, and keep the blob
- [ ] **MCP-001.4**: Surface the row-numbering trap (0-based API, 1-4 on screen) in tool descriptions
- [ ] **MCP-001.5**: Single owning process for the USB interface

### MCP-002: Tool surface

- [ ] **MCP-002.1**: Read tools: `list_presets`, `read_preset`, `list_blocks`, `get_device_version` (unrestricted)
- [ ] **MCP-002.2**: Transient write tools: `recall_preset`, `switch_scene` (changes what is heard, nothing persistent lost)
- [ ] **MCP-002.3**: Working-copy write tools: `set_block`, `set_param`, `set_routing` (edits the recalled preset in device RAM)
- [ ] **MCP-002.4**: Destructive tool: `save_preset` (gated: explicit slot, refuse FACTORY, require confirmation)

## GUI (GUI)

Deferred until the crate and CLI are complete. See [spec/400-gui/spec.md](spec/400-gui/spec.md).

The visual design goal is a **hardware-faithful rendering of the Quad Cortex front panel** - 10 footswitch/encoder positions, the colour OLED grid, scene LEDs, and the context strip - with wrapper panels (patch browser, block palette, parameter inspector, scene manager, IR/capture loader) alongside. Use Tauri MCP to tighten the feedback loop during GUI development.

### GUI-001: Scaffold and Tauri MCP

- [ ] **GUI-001.1**: `gui/` with Tauri 2 + React + Mantine + Vite, `s/gui-dev` script
- [ ] **GUI-001.2**: Tauri commands calling `cortex-rs` and returning typed serialisable data; no protocol/domain logic in TypeScript
- [ ] **GUI-001.3**: Wire Tauri MCP for the dev feedback loop - drive the GUI from the MCP server to test Tauri commands without manual clicking

### GUI-002: Hardware-faithful control surface

- [ ] **GUI-002.1**: Render the Quad Cortex front panel: 10 footswitch/encoder positions, OLED grid, scene LEDs, context strip
- [ ] **GUI-002.2**: Footswitch interaction: click-to-press (toggle bypass / recall / navigate), drag-to-turn / scroll (adjust parameter), keyboard equivalents
- [ ] **GUI-002.3**: Mode-aware footswitch labels - reflect the current device mode (Preset / Stomp / Scene / Looper / Tuner)
- [ ] **GUI-002.4**: The OLED grid mirrors the device's live state (signal chain, block icons, bypass, active scene) from the crate's read paths
- [ ] **GUI-002.5**: Honest state - render what the device reports, not what the GUI thinks it sent

### GUI-003: Wrapper panels

- [ ] **GUI-003.1**: Patch browser - setlist/slot grid for quick preset switching, with search and favourites
- [ ] **GUI-003.2**: Block palette - searchable list of available models from the `Catalog`, drag onto a grid cell
- [ ] **GUI-003.3**: Parameter inspector - form-based editor for the selected block's parameters, showing real units (dB, ms, Hz) via catalog range conversion
- [ ] **GUI-003.4**: Scene manager - copy/swap/relabel/recolor scenes without the footswitch mode dance
- [ ] **GUI-003.5**: IR / Capture loader - file-browser-style access to the device's captures and IRs

### GUI-004: Safety surface and governance

- [ ] **GUI-004.1**: Reuse the MCP safety surface (factory refusal, scratch range, slot backup, trap-surfacing) for save actions
- [ ] **GUI-004.2**: Label hardware-verified vs provisional surfaces in the UI
- [ ] **GUI-004.3**: `s/version++` bumps `gui/package.json` and `tauri.conf.json` with the canonical version
- [ ] **GUI-004.4**: `docs/gui/` explains how to use and run the GUI

## Engineering (ENG)

### ENG-001: DX and testing
- [x] **ENG-001.x**: Correlation unit tests - 12 tests over `dispatch` covering type-first matching, the id-less oldest-first fallback, cascade rejection, the stale-seed-push skip, collector semantics, and the liveness stamp. No fake transport needed; `dispatch` is a free function. The HashMap-ordering guard was verified to fail 12/12 with the bug reintroduced and 0/12 with it fixed
- [ ] **ENG-001.y**: Fake transport for the RX loop itself - the 1 MiB reassembly cap and the FIRST-resets-stale-partial rule need frame injection

- [x] `s/test` - cargo fmt + clippy + test
- [x] `s/lint` - cargo fmt + clippy + reuse lint
- [x] `.editorconfig`
- [ ] **ENG-001.1**: `s/gui-dev` (once `gui/` exists)
- [ ] **ENG-001.2**: `s/version++` (once GUI manifests exist)
- [ ] **ENG-001.3**: `s/install-hooks` and `.githooks/pre-commit`
- [ ] **ENG-001.4**: Markdown lint

### ENG-002: CI

- [x] `.github/workflows/ci.yml` - fmt, clippy (all + no-default-features), tests (both), REUSE lint, protoc install
- [x] `.github/dependabot.yml` - cargo + github-actions, weekly, cooldown, grouping
- [ ] **ENG-002.1**: auto-tag workflow
- [ ] **ENG-002.2**: crates.io publish workflow
- [ ] **ENG-002.3**: cargo-dist release pipeline

### ENG-003: Governance

- [x] AGPL-3.0-or-later LICENSE + LICENSES/ (AGPL, MIT for vendored .proto)
- [x] SPDX headers on every source file
- [x] REUSE.toml + `reuse lint` passing
- [x] NOTICE + THIRD-PARTY-NOTICES.md (pyquadcortex MIT, deskop-nano-cortex Apache-2.0, qc-stomp-tools MIT)
- [x] Trademark and unaffiliation notice in README, AGENTS.md, NOTICE
- [x] AGENTS.md (repo-local, pointing at parent workspace)
- [ ] **ENG-003.1**: **Confirm the copyright holder.** Currently `2026 Dr Marcus Baw` with no company. AGENTS.md flags this as mixed-domain work with no default company, so it needs a decision. If it changes, every SPDX header and `REUSE.toml` move in ONE commit
- [ ] **ENG-003.2**: Decide whether a contributor licence agreement is wanted. Current stance: not in scope - the AGPL header is the inbound-outbound grant
- [ ] **ENG-003.3**: If a closed derivative ever needs to exist, add `DUAL-LICENSE.md` and the boilerplate. Requires approval
- [ ] **ENG-003.4**: SECURITY.md and CONTRIBUTING.md before the repo is public-facing
- [ ] **ENG-003.5**: If we ever target on-device builds, adapt `qc-stomp-tools` (MIT) with attribution and a NOTICE entry

### ENG-004: Traceability

- [ ] **ENG-004.1**: Add `@see` traceability headers to all owned source files linking to zone specs
- [ ] **ENG-004.2**: CI gate for `@see` link resolution (optional, low priority)

### ENG-005: `s/usb-trace` - observe Cortex Control on the wire

A script that sets up passive USB observation of the official Cortex Control app driving the device, so its traffic can be decoded against our schema. This is the tool for questions of the form "how does the official client do X" - the answer to which is evidence about the wire, not inference about intent.

**It paid for itself before Cortex Control was ever traced.** The first capture was of our OWN client, run only to check whether the device's apparent silences were real or an artefact of our RX path. It showed the bus idle for 46.8 s in the middle of a handshake, with the device answering every write in 217 us - which identified writer starvation in `session.rs` (PROT-008.5, PROT-008.6.10) after a full day of attributing that variance to the device. Two features had already been built and withdrawn on the strength of the wrong explanation. Trace our own side first; it is cheap, needs no VM, and the bug is as likely to be ours.

**Named `usb-trace`, not `usb-record` or `usb-capture`, deliberately.** "Capture" already means a Neural Capture in this domain and "record" implies audio; either would suggest this script records sound, which it emphatically does not. `trace` is already the project's word for protocol observation (`CORTEX_TRACE`), so the two read as the same idea at different levels.

The method, from [the research note](../quad-cortex-linux-editor-and-protocol.md): with the QC passed through to a Windows VM under QEMU, the **host** kernel still sees the traffic. So `modprobe usbmon` plus a capture of the relevant `usbmonN` interface on the Linux host records everything, without needing USBPcap inside Windows and without the macOS exclusive-access problem.

- [x] **ENG-005.1**: `s/usb-trace` - preflight `usbmon` (module loaded, `/dev/usbmonN` present and readable) and `dumpcap`, identify the QC's bus and device address from `lsusb`, and start a capture to a gitignored `traces/`. Each preflight failure names its own fix rather than just refusing, because a setup error discovered halfway through a session with the official client wastes the whole session. Writes a sidecar `.txt` recording bus and device address: both are assigned at plug time, so a capture without them cannot be filtered afterwards with any confidence.
  - Uses the **binary** usbmon interface via `dumpcap`, not the text interface at `/sys/kernel/debug/usb/usbmon/<bus>u`. The text interface truncates payload data, which would drop bytes from the middle of a 128-byte body while still looking like a successful capture.
- [x] **ENG-005.2**: `s/usb-decode` plus `cortex decode-trace` - reads a capture and prints it in the same shape as `CORTEX_TRACE`. Verified against the first real capture: 666 messages, **0 reports skipped**, independently reproducing two known facts (the `GlobalTempo` pair structure - 35 ms within a pair, 786 ms between - and 399 inbound `File` messages for the 399-folder enumeration).
  - **Reuses the crate's framing rather than reimplementing it.** `Message::decode` was extracted so the live RX path and the offline decoder share one implementation; a decoder that parsed the wire its own way would be worse than useless, because it would be trusted while wrong. The order is the trap: the 8-byte trailer is stripped BEFORE gunzip, since the type tag sits outside the compression.
  - Both directions in one pass. Inbound reports carry their bytes in `usb.capdata`, but our writes are `SET_REPORT`s on the control endpoint and carry theirs in `usb.data_fragment`; asking for both fields and taking whichever is populated is what makes a single pass cover both.
  - `s/usb-decode --live` is the development monitor - every message to and from the unit as it happens, whoever sent it. Wireshark's GUI can watch the same bus but knows nothing of our framing, so it shows 129-byte blobs rather than messages.
  - Unknown type tags print as `<unknown N>` rather than collapsing onto `Undefined`. Reading a capture of a client we do not control is exactly when an unrecognised tag is the interesting part.
- [x] **ENG-005.3**: `cortex decode-trace --verbose` describes each message's protobuf fields - field number, wire type and value, with length-delimited values shown as text where they are text.
  - **Generic rather than per-type, deliberately.** A match over the 70-odd message types would decode more prettily and would go blank on exactly the messages worth looking at: the ones the official client sends that we do not model. The wire format carries field numbers and types regardless, which is enough to compare two clients' requests byte for byte.
  - It earned that on its first use, identifying why `CPULoad` never pushed to us (008.6.12) from a one-line difference invisible at the message level.
- [ ] **ENG-005.4**: Optionally a Wireshark Lua dissector, since Wireshark's built-in protobuf support can be pointed at our vendored `.proto` files - worth it only if the GUI earns its place alongside `s/usb-decode --live`
- [ ] **ENG-005.3**: Runbook: what to do in Cortex Control while tracing to answer a specific question, starting with capture export (PROT-007.1)

**Known obstacle.** The QEMU/Windows/Cortex Control setup on the development machine works but drops its connection regularly, so it is adequate for short targeted observations and not for sustained work. Plan traces as single short scripted actions - "open the app, export one capture, stop" - rather than long exploratory sessions, and expect to repeat them.

**Do not commit raw captures.** They contain readable preset, path, device, and build strings. Commit decoded findings in our own words, as the prior art does. See the legal hygiene section of AGENTS.md.

## Future

- **FUTURE-001**: Nano Cortex hardware verification - plug in the Nano, verify the protocol shape holds, record the product ID, promote `DeviceKind::NanoCortex` from provisional to verified
- **FUTURE-002**: Nano Cortex BLE protocol - the Nano uses BLE for control telemetry; the deskop-nano-cortex project has a provisional decode (Apache-2.0)
- **FUTURE-003**: Tauri desktop GUI (zone 400) - React + Mantine + Vite, a consumer of the crate; use Tauri MCP to tighten the feedback loop
- **FUTURE-004**: On-device builds (qc-stomp-tools ioctl route) - only if there is a compelling reason; the USB route is preferred
- **FUTURE-005**: Protocol-version probe - surface a CorOS version check rather than hard-coding assumptions, since the protocol has no version field on the wire
- **FUTURE-006**: Conformance suite - port pyquadcortex's offline test suite as a Rust integration test reference
- **FUTURE-007**: Audio feedback loop - let the MCP server "hear" the unit. The Quad Cortex presents class-compliant USB **audio** interfaces that are separate from the HID interface we use, so a host could play a standardised stimulus (DI guitar phrase, sine sweep, impulse) through the chain and capture the processed result **without contending for the exclusive HID connection**.

  Confirmed on hardware 2026-08-02: of the unit's six USB interfaces, **0 through 4 are Audio class (class 1) and only interface 5 is HID (class 3)**. ALSA already enumerates the device as a working card (`USB-Audio - Quad Cortex`) with no driver work required, so the capture side of this needs no reverse engineering at all - it is an ordinary audio device that happens to also speak our HID protocol on a different interface. Comparing captured output against a dry reference would characterise what a chain is doing to the signal.

  Worth being precise about what this buys, because it is not what it first appears. An agent editing a patch already has **ground truth** available: `read_current_preset` returns the actual grid, so "did my edit land on the right block" is answerable today by read-back, and audio analysis is a strictly worse way to answer it. What audio adds is **aesthetic and perceptual judgement** - "is this too dark", "is the gain staging sensible", "does this sound like the reference tone" - which read-back cannot answer at all.

  So this is not a correctness or safety mechanism and should not be treated as one; it is what would let an agent iterate on *tone* rather than on *structure*. That is genuinely novel and nobody has built it, but it is a substantial subsystem (audio I/O, latency alignment, feature extraction, a perceptual similarity metric) and it should not start until the grid-edit surface it would be judging actually exists.

  Open questions: does the QC expose a usable dry/wet split over USB (there is a `dry_wet` field in `USBPortSettings`) so a dry reference can be captured simultaneously rather than in a separate pass; and what stimulus set is both compact and discriminating enough to be worth standardising.