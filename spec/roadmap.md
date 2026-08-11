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
> Completed parent items move to `completed.md`; completed sub-items remain beside unfinished siblings while an active milestone is mixed. `s/progress` counts both files. Once a CHANGELOG exists, this roadmap becomes a pure backlog.
>
> **`[x]` means the code exists and passes the local gate. It does NOT mean hardware-verified.** Anything touching the wire stays provisional until its hardware smoke item passes against a real Quad Cortex.
>
> Hardware-verified so far (through 2026-08-09, CorOS 4.0.1): transport through typed client; the held daemon and live cache; core read, navigation and grid-edit paths; prepared save, same-setlist move/restore, recall, read-back and delete; physical reconnect; and the native GUI's fail-closed reconnect rendering. The latest core/CLI scripted smoke passed 42/42 checks. Individual items below identify what remains provisional or unimplemented.
>
> **Overnight routing:** `Night: ready` marks a bounded task that can be completed without USB hardware. `Night: slice` permits only the explicitly named offline subset and requires a PR `Hardware follow-up` section where applicable. Unmarked items are not available to the overnight routine. The copy-paste routine prompt and its safety rules live in [overnight-routine.md](overnight-routine.md).

## Next Milestone: Daemon-backed GUI Reads

Connect the existing Tauri first draft to the same held-session contract already proven by CLI and MCP, without widening the write surface:

1. [Done] Write the as-built GUI design and make `cortex-host` the explicit Tauri backend boundary.
2. [Done] Replace fixture-only status, preset directory, live grid, active scene and CPU data with typed daemon snapshots and revisions.
3. [Done] Surface connected/reconnecting/failed state and never render a stale generation as live.
4. [Done] Keep fixture mode as a deliberate development/test adapter rather than hidden fallback behavior.
5. [In progress] Boundary-focused Rust tests, both frontend builds, browser fixture smoke, the real-device dashboard boundary and physical unplug/reconnect pass. Automated native DOM/IPC checks through Tauri MCP remain.

Saving remains outside this milestone. GUI save needs exact-target preparation and confirmation UX, restoration semantics, typed failures and its own hardware smoke. In parallel, CLI-004.4 remains the next distribution milestone: a Linux x86_64 preview containing both `cortex` and `cortex-mcp`.

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

!!! tip "Check [prior-art.md](prior-art.md) before implementing a protocol item"

    Many unticked PROT items already have an implementation, captured wire shape, or documented investigation in `pyquadcortex`, and several carry silent-failure traps that cost that project real time. [prior-art.md](prior-art.md) records what the prior-art projects know that we do not, the verification level of those findings, and which negative results are worth re-testing. Items below cite it where it applies.

!!! note

    Progress is tracked HERE and nowhere else. Zone folders hold `spec.md` (what it must do) and `design.md` (how it is built and why), but no `tasks.md` - see [001-overview/spec.md](001-overview/spec.md#progress-tracking) for why that convention was dropped.

## Protocol and Crate (PROT)

The bottom-up port of the Cortex Control USB HID protocol from `pyquadcortex` (MIT) into a Rust leaf crate.

### PROT-003: Proto schema (zone 120)

- [ ] **PROT-003.1**: Curate the message-type registry - map each `CortexMessageType` to its generated struct, so the session layer can dispatch by type tag. **Night: ready.** Pure schema/registry work with exhaustive offline tests; do not add unsupported semantic claims

### PROT-004: Domain model - core (zone 130)

- [x] **PROT-004.2**: Read-side grid. `grid::Row` provides checked wire/screen conversion and `view::Preset::{rows,blocks}` provides the shared serialisable grid view. A denser `Grid` abstraction is not required unless it adds behavior beyond those types
- [x] **PROT-004.4**: `view::Scene` exposes index, optional label and ARGB colour in every shared preset view; per-block scene bypass remains available through `view::Block::bypass`. A duplicated scene-centric aggregate is deliberately absent unless a concrete consumer demonstrates value
- [x] **PROT-004.6**: Core checked helpers: `slot_to_position` (+ checked variant), `position_to_slot`, `input_level_db`, `db_to_input_level`, `preset_has_block`. Richer navigation helpers (`blocks`, `splits`, `free_rows`, `row_status`) are independently tracked under PROT-006.15 rather than duplicated here

### PROT-006: Client API (zone 150)

The ergonomic `QuadCortex` struct - the Rust equivalent of pyquadcortex's 60+ methods.

- [~] **PROT-006.4**: Read operations. HARDWARE-VERIFIED 2026-08-02: `read_current_preset` (live grid, no side effects), `read_preset` (recall + capture the echoing push), `active_scene`, `list_presets`, `find_preset`, `list_folders` (via `collect`), plus the `PresetEntry`/`Folder` value objects. `list_presets` now accepts only index-complete 256-slot setlist updates; establish a completion rule before restoring targeted variable-length plugin/capture folder listings. Planned: `captures`, `list_irs`, `recents`, `favorites`, `pinned_models`, `master_volume`, `looper`, `tuner`, `io_settings`, `settings`, `global_eq`, `mode` - all twelve have upstream implementation or coverage notes, so start from [prior-art.md](prior-art.md#pyquadcortex---the-one-to-check-first) rather than rediscovering them. Two reported limits matter: master-volume writes were ignored, and the tuner's needle did not stream to a host, so `tuner` reads settings and not pitch
- [x] **PROT-006.5**: Navigation. HARDWARE-VERIFIED on CorOS 4.0.1: `recall_preset`, `switch_scene`, `copy_scene`/swap, `set_scene_label`/unlabel and `set_scene_color`. The 2026-08-09 copy/swap test used discriminating parameter values and confirmed labels and ARGB colours travel with scene state. It also found that the device acts on `is_swap` but omits the flag from its acknowledgement; the reducer now invalidates that lossy echo and the daemon forces a fresh `RecallPreset{READ}` before reporting copy/swap success
- [x] **PROT-006.6**: Grid write. HARDWARE-VERIFIED on CorOS 4.0.1: `set_block` (echo plus read-back fallback), `set_param`, `remove_block` (DELETE), `set_bypass`, `set_chain_input`, `set_chain_output`, `set_split`, `set_param_in_scene` and `move_block`. The 2026-08-10 move run validated both cells before sending, moved one block within a row and back with every parameter and all bypass state unchanged, moved another across rows, cleared the source after every operation, retained an existing split 2 / mix 5 path, then restored the stored preset by recall. `GridMove` omits the advisory snapshot and forces complete live-grid read-back. A low-level whole-preset `write_preset` is deliberately not part of the supported API because recalled presets have no row keys and writing one back applies nothing. The grid is ground truth and an echo only a fast path
  - Upstream supplies the verified `move_block` shape. Its low-level `write_preset` also establishes that writing a recalled preset wholesale does nothing: only row/column-keyed elements apply. Preserve two silent-state traps when placing blocks: empty cells retain old bypass-table state, and adding a block can renormalise unrelated selectors whose option list enumerates the preset's blocks ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))

!!! warning "Start PROT-006.7 through PROT-006.15 from upstream evidence"

    The methods below have upstream implementations, captured wire shapes, or documented negative investigations against the firmware we target. Work from the operation-coverage and manual-coverage tables, preserve their evidence level, then verify this implementation on hardware. See [prior-art.md](prior-art.md#pyquadcortex---the-one-to-check-first).

    Two traps apply throughout. **A nested submessage write replaces the whole submessage** - send one flag of a group and its siblings go false, which in one upstream case quietly stopped the Master Volume knob governing most outputs. Read, merge, write. And **the action field is load-bearing**: some operations need `CREATE`, some `READ`, some no action at all, and the wrong one is ignored in silence rather than refused.

- [ ] **PROT-006.7**: Splitter/mixer/lane/gate: `set_splitter_param`, `set_mixer_param`, `set_lane_output`, `set_input_gate`, `set_split_mute`. `set_split` is already implemented and hardware-verified under PROT-006.6
- [ ] **PROT-006.8**: Tempo: `set_tempo_param`, `set_tempo_option`, `set_tempo_subdivision`, `set_metronome_sound`, `set_metronome_routing`, `set_time_signature`, `set_tempo_led`, `set_metronome_volume`. All eight exist upstream. The Tempo menu's MODE control is reported to be absent from the wire entirely, and the internal MIDI clock is reported unwritable - both deserve one re-test against the action field before accepting the negative result ([prior-art.md](prior-art.md#where-it-is-wrong-and-why-that-matters))
- [ ] **PROT-006.9**: Stomp/expression/MIDI: `set_stomp_assignment`, `clear_stomp_assignment`, `set_stomp_momentary`, `set_stomp_label`, `set_expression`, `set_expression_bypass`, `set_midi_out`, `set_preset_load_midi_out`; use the upstream operation table and docstrings rather than inferring the multi-message stomp sequence ([prior-art.md](prior-art.md#what-it-has-that-we-do-not))
- [~] **PROT-006.10**: File ops. **`save_current_preset`, `delete_preset`, and same-setlist `move_preset` are hardware-verified.** Save names a destination slot (`FileMessage{action: CREATE}`); it does NOT upload a preset - the unit commits the working grid, proven by enabling a bypassed block, saving, recalling a different preset, and recalling back to find the change intact. Delete addresses its target by device FILE PATH, not slot index. Move uses the captured same-setlist shape (`action: MOVE`, source listed path in `folder`, destination index in `to_folder`), accepts exact source and destination slots, refuses factory content, empty sources, no-op moves, and observed occupancy, then polls fresh listings for convergence because a mutation may land without an acknowledgement. The 42-check CorOS 4.0.1 smoke created a prepared fixture in `7A`, moved it `7A -> 7B -> 7A`, observed storage-revision advancement and convergence in both directions, deleted it, and confirmed both slots empty. CLI save and move execute by default and expose `-n`/`--dry-run`; MCP exposes no persistent move tool. Outstanding: `copy_preset`, `create_setlist`, `delete_setlist`, `duplicate_setlist`, and an `instrument` argument on save (the tag is currently hardcoded to Guitar).
  - **The device may rename what you save.** On a name collision within the setlist it de-duplicates - truncating and appending `_N`, to 20 characters. The stored name is therefore not necessarily the one requested, and delete is name-addressed, so a caller that saves and later deletes must read the listing back rather than assume. Documented by `pyquadcortex` (MIT) and not visible in our own capture.
  - **File mutations are eventually consistent.** Upstream measured all eleven deleted entries still present in a listing five seconds later. Poll until the expected state appears; a fixed sleep produces false failures. There is no host-driven bulk copy, so copy/duplicate helpers must recall and save one preset at a time ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))
- [ ] **PROT-006.11**: Captures/IRs: `set_capture`, `set_ir`, `show_capture_dialog`. **Order matters:** upstream reports `set_capture` resets that block's other parameters to the capture defaults without warning, so select the capture before writing the block's remaining parameters ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))
- [ ] **PROT-006.12**: Global settings: `update_settings`, `set_hold_timing`, `set_scene_bypass_behavior`, `set_master_volume_assignment`, `set_global_bypass`, `set_global_eq`, `set_mode_cycle`, `set_mode`, `set_gig_view`, `show_tuner`, `set_tuner_input`, `set_tuner_mute`, `set_tuner_reference`. Read-merge-write nested submessages rather than treating them as sparse. Refuse the upstream-observed mode-cycle value that stores and reads back but leaves the footswitches dead ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))
- [ ] **PROT-006.13**: I/O ports: `set_input_port`, `set_output_port`, `set_usb_port`, `set_midi_thru`, `set_output_pairing`. Output mute, input impedance mode, and USB dry/wet are reported to vanish silently when packed with a sibling; send those fields alone ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))
- [ ] **PROT-006.14**: Pinning/favorites: `pin_model`, `unpin_model`, `add_favorite`, `remove_favorite`. Do not infer the action: upstream reports pinning with no action field, unpinning with `DELETE`, and favourites with `CREATE` ([prior-art.md](prior-art.md#where-it-is-wrong-and-why-that-matters)). **Night: slice.** Implement the four typed wire builders/client methods and offline shape tests from licensed upstream/schema evidence; leave the item provisional and request next-day read-back for all four operations
- [ ] **PROT-006.15**: Module-level helpers: `blocks()`, `splits()`, `input_chain_rows()`, `stomp_assignments()`, `midi_out()`, `tempo_params()`, `param_options()`, `free_rows()`, `row_status()`, `params_equal()` - `slot_to_position`, `position_to_slot`, `input_level_db`, `db_to_input_level` done. Also port the upstream ergonomic helper that selects a list parameter by option name and performs `index / (count - 1)` centrally ([prior-art.md](prior-art.md#what-it-has-that-we-do-not)). **Night: slice.** Choose one coherent pure-helper group, add exhaustive value tests, and leave the parent `[~]` until the list is complete
- [~] **PROT-006.16**: Hardware smoke test. The implemented core/CLI surface passed 42/42 on 2026-08-07 against CorOS 4.0.1, including prepared save, same-setlist move/restore, storage-revision advancement, recall, delete and empty-slot cleanup; physical reconnect and strict save/file-correlation hardening also passed. A separate 2026-08-10 run verified same-row and cross-row block moves with exact read-back and recall restoration. Repeat the smoke as each remaining PROT-006 operation lands

### PROT-008: Session performance

Commands were taking tens of seconds for milliseconds of work. Most of that is fixed (see [140-session/spec.md](140-session/spec.md)); what is left is listed here.

- [~] **PROT-008.6**: `cortex session start` - a persistent subscribed session. The session holds and serves, routes every ordinary command, reduces subscribed state, reports health, and reconnects with backoff. Only request-based idle shutdown remains. Measured held commands are typically tens of milliseconds rather than paying a multi-second handshake.
  - [~] **008.6.3**: Lifecycle. Done: status, bounded clean stop, stale-endpoint handling, atomic ownership claim, and background readiness after a real request. Outstanding: request-based idle shutdown shared with CLI-004.8; the GUI must also release its host use under a bounded window-close path
- [ ] **PROT-008.7**: Cache the catalog on disk, keyed by `CorOS` version. It changes only on a firmware update or a new capture. Less urgent than it was: a held session now caches it in memory and serves `cortex catalog` in 0.02 s, so this is for the one-shot path and for surviving a restart

### PROT-007: Capture and IR export / import

Neural Captures and user IRs live only on the unit and in Neural's cloud. No existing tool - official or community - can export a capture to a local file, so a player's own captures cannot be backed up, version-controlled, or moved between units. They are the player's OWN data, which makes this the least legally fraught significant feature on this page and arguably the most valuable.

Status: **investigation, not yet designed.** Export remains unconfirmed, but the file category is solved: `FileMessage.type` is 0 for presets, 1 for IRs, and 2 for captures. `FileMessage` carries `ir_payload` and `preset_payload` `bytes` fields, and `LocalBackup`, `CloudBackup`, and `BackupsForward` message types exist in the recovered schema, so a route probably exists - but which one carries capture data, and in what format, is unknown.

!!! warning "Known constraints before tracing"

    Upstream hardware work mapped a candidate IR-import request, established that `total_bulk_create_count` is mandatory, and round-tripped an outbound write spanning 26 HID reports. Its imports still produced no IR, so payload content, action choice, and other request semantics remain unknown; outbound framing itself is proven. Repeated multi-kilobyte attempts coincided with the USB link dying until a power cycle; space destructive probes out ([prior-art.md](prior-art.md#on-captures-and-irs-prot-007)).

    Separately, reference-only pre-CorOS-3 prior art without a repository-wide licence reports captures as encrypted protobufs and local captures as serial-number-keyed. That is a provisional lead to test, not source material or a current-format fact; if true, cross-unit import may be impossible without a portable representation ([prior-art.md](prior-art.md#opencortex---one-high-value-fact-and-a-minefield)).

- [ ] **PROT-007.1**: Establish whether a capture's payload can be read over the wire at all, and by which message type. **Use `s/usb-trace` and `s/usb-decode`** to record Cortex Control performing a backup and read the message types it uses. The original wording said to use `CORTEX_TRACE`, which cannot work: that traces our own client, not the official one - the tooling to watch another client did not exist when this was written (ENG-005)
- [ ] **PROT-007.2**: Identify the payload container and whether it is compressed, encrypted, or both. Outbound chunking itself is no longer an open question: the existing encoder round-tripped 26 reports upstream (the `ModelRepo` precedent remains gzip inside a `bytes` field, ~47 KB over ~371 reports)
- [ ] **PROT-007.3**: `export_capture(key, path)` - write one capture to a local file
- [ ] **PROT-007.4**: `import_capture(path)` - write a capture back to the unit. **Destructive; gate behind confirmation and the MCP safety surface.** Do not assume a capture exported from one serial can be imported by another until the encryption lead is resolved
- [ ] **PROT-007.5**: Same for user IRs. First establish a safe `list_irs` completion rule, then start from the upstream candidate import request and its mandatory bulk-count field; do not repeat failed payload encodings without new evidence
- [ ] **PROT-007.6**: `cortex capture export` / `import` CLI surface
- [ ] **PROT-007.7**: Decide and document a container format. Prefer something self-describing that records the source unit, CorOS version, and capture metadata, so a file is still meaningful years later

Do not ship import before export has been round-tripped on hardware: writing a malformed capture to the unit is the most plausible way this project could damage a user's data.

### PROT-009: Correctness gaps found in review, 2026-08-05

Raised by a review of the state-cache/prepared-save/daemon-routing change (`a603bce`). The tree was green - fmt, clippy, `reuse`, and 149 tests all passed - so every item here is a behavioural gap that the existing tests did not reach. All claims have now been checked; completed items state their verification level.

Ordered by what would hurt a user most.

- [x] **PROT-009.2**: **The shipping save path bypassed the prepared-save API entirely.** FIXED 2026-08-05 in `743692a`, then corrected for pre-edit ordering under PROT-009.3. The daemon exposes `PrepareSave`/`CommitSave` with server-held preparations and opaque tokens; the CLI uses separate `preset prepare-save` and `preset save --token` commands, with `-n`/`--dry-run` as the non-mutating path. The raw `SavePreset` request is gone. CONFIRMED by call-site search: `save_current_preset` has zero production callers outside `safety.rs`. Referred to elsewhere as PROT-006.10.1
- [x] **PROT-009.1**: **Reconnect could open a replacement while the old HID handle was still held. FIXED AND HARDWARE-VERIFIED 2026-08-07 on CorOS 4.0.1.** `Session` now owns a removable boxed link; `close()` announces disconnect, joins workers, then explicitly takes and drops it. The daemon uses an operation/recovery `RwLock` so replacement waits for in-flight calls and no new call races the health transition. An exclusivity-aware fake retains an old `Arc<Session>` and proves its lease drops before either replacement attempt. A physical unplug/replug advanced generation 1 to 2, returned the cache to `live`, and the first grid read succeeded - the important next-request check for overlapping HID ownership.
- [x] **PROT-009.3**: **A save prepared after editing recalled its target and saved the wrong grid. FIXED AND HARDWARE-VERIFIED 2026-08-06.** The CLI now requires `preset prepare-save` on a held daemon before editing and `preset save --token` afterwards; there is no post-edit preparation or direct one-shot save path. `recall_preset` waits for the correlated `RecallPreset` push before acknowledging. The 37-check hardware smoke prepared empty `7A`, placed a block, changed GAIN, committed, recalled, and confirmed both the block and stored edit before deleting the test preset and restoring `1A`. The run also found and fixed two daemon-path blockers: the version check deadlocked by retaining one socket while opening another, and a measured post-recall `input_control.sidechain_source_flag=false` delta invalidated the live-grid cache.
- [x] **PROT-009.4**: **An eventually consistent listing is treated as proof a slot is empty.** FIXED 2026-08-05 in `743692a`. The backup read is now always attempted regardless of the listing; the listing determines `previous_name` for the UI but never decides whether to skip the backup. The consent gate widens to all targets. A fake-session test (`a_stale_listing_saying_empty_still_backs_up_the_real_preset`) pins the stale-listing case.
- [x] **PROT-009.5**: **Preparation used an untrustworthy epoch. FIXED AND HARDWARE-VERIFIED 2026-08-07 on CorOS 4.0.1.** Preparation and commit now require `CachePhase::Live`; one unchanged generation and storage revision must span the listing and backup read. Every other phase fails closed, and finishing a stream-gapped subscription cannot relabel `Invalidated` as `Incomplete`. Tests inject a mutation between listing and backup. An unchanged scratch-slot prepare/save passed, and a token prepared in generation 1 was refused after physical reconnect advanced the daemon to generation 2.
- [x] **PROT-009.6**: **File replies were correlated by shape, not operation or target. FIXED AND HARDWARE-VERIFIED 2026-08-07 on CorOS 4.0.1.** Listings require `UPDATE`, the exact folder and complete unique indices 0-255. Save requires `CREATE` plus exact folder/slot; delete requires `DELETE` plus exact folder/full path. Folder collection ignores mutations and retains the fullest repeated listing. Strict save and delete acknowledgements both passed against disposable `7A`; pure predicate tests reject wrong operations, targets, partial listings and duplicate indices.
- [x] **PROT-009.7**: **A cached listing can freeze a stale result indefinitely.** FIXED 2026-08-05. Only a complete, index-complete 256-slot `File UPDATE` enters the preset cache. A partial update invalidates that folder and advances `storage_revision`, so a daemon listing re-reads instead of retaining a stale partial result. The reducer already invalidates both explicit mutation-folder keys; `SWAP` remains unverified because it is not in the recovered action enum.
- [x] **PROT-009.8**: **A malformed envelope is skipped without breaking cache continuity.** FIXED 2026-08-05. A failed `Message::decode` now calls `stream_gap`, invalidating the cache because the lost envelope's message type cannot be recovered. A fake-link test proves the next valid message still reaches its waiter.
- [x] **PROT-009.9**: **Cache continuity never recovered once lost. FIXED 2026-08-07, OFFLINE-VERIFIED.** `Invalidated` is now a reconnect trigger even while heartbeats keep the old session responsive. Recovery changes health first, drains operations, explicitly releases the old link, performs a full replacement handshake in a new generation and accepts it only when the cache is `Live`. A responsive-invalidated fake covers the path; deliberate malformed-report hardware recovery remains in the runbook.
- [x] **PROT-009.10**: **The daemon socket protocol changed without a version gate.** FIXED 2026-08-05 in `743692a`. `DAEMON_PROTOCOL_VERSION` is checked by the client before sending and bumped whenever a request shape changes (currently 7); a mismatch names the fix ("run `cortex session stop`") rather than producing a parse error.
- [x] **PROT-009.11**: **`SavePolicy::is_user_setlist` accepts dot path components.** FIXED 2026-08-05 in `743692a`. `.` and `..` are now rejected; the override-escape test covers them.
- [x] **PROT-009.12**: **Named parameter resolution does not enforce the catalog's value type.** FIXED 2026-08-05. Named parameters now accept text only for catalog `Str` controls and normalised or real-unit values only for numeric (`Float`, `Int`, `Switch`, `Fader`) controls. Meters, empty placeholders, and unknown types refuse before the wire. Raw index addressing remains intentionally untyped because it has no catalog metadata. A pure resolver test covers every allowed and refused category.
- [x] **PROT-009.13**: **An empty prepared target can be saved with no name.** FIXED 2026-08-05. `save_prepared` now refuses `name: None` when the prepared target has no existing name, before it lists or writes to the device. A fake-link test pins the refusal and asserts zero device writes.
- [~] **PROT-009.14**: **Regression coverage.** Done: explicit link-drop with retained session references; exclusivity-aware responsive-gap reconnect; prepared-save phase matrix and generation/storage epoch; strict File operation/target/completeness predicates; recall/edit/save ordering; stale-empty target backup; partial File invalidation; malformed envelopes; daemon version gate; parameter kinds and real-unit conversion. Outstanding: concurrent interleaving of live list/save/delete waiters, delayed acknowledgements from an earlier identical file operation (the wire has no usable request ID), the unavoidable final interval between a fresh listing/epoch check and the destructive write, `SWAP`, malformed 256-entry listings beyond duplicate/missing coverage, version skew both ways as spawned processes, and hardware fault recovery. **Night: slice.** Add exactly one offline regression: concurrent waiter interleaving, malformed complete-listing rejection, or spawned-process version skew; do not attempt hardware fault recovery or claim the unavoidable race is solved

## Docs (DOCS)

### DOCS-002: Agent manual - factory preset reference

A per-device, per-CorOS reference of the factory presets: what each is modelled on, and how to get a usable sound out of it. Aimed at agents driving the MCP server, who otherwise have no idea that "Brit 2203" is a Marshall-style voicing.

- [ ] **DOCS-002.1**: Generate the raw preset inventory from the device rather than transcribing it - `cortex preset list --setlist "/opt/neuraldsp/Factory Library"` already emits all 256 names. Keep it a build step so a CorOS update regenerates it
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

### CLI-002: Format and output

- [ ] **CLI-002.3**: `--schema` / `--print-schema` - JSON Schema of a command's inputs
- [x] **CLI-002.5**: Uniform `-n`/`--dry-run`. Every device-state mutation, recall, daemon lifecycle action and local file write is exhaustively classified before dispatch; dry-run performs pure argument validation, emits a structured plan and returns before IPC, HID, process or filesystem access. Read-only commands accept and ignore the global flag. A command-policy test covers every side-effect class, and adding an enum variant requires an explicit classification before the CLI compiles

### CLI-003: Preset and scene commands

The implemented preset/scene command surface is hardware-verified against CorOS 4.0.1. `scene switch|label|unlabel|color|copy|swap` use noun-then-verb naming; the shipped `scene --index` switch shorthand remains accepted.

- [ ] **CLI-003.9**: `cortex capture` / `cortex ir` - export and import (blocked on PROT-007)

### CLI-004: Distribution

**Decision (2026-08-07):** the primary user channel is a Linux x86_64 preview containing both `cortex` and `cortex-mcp`, installed without Rust or `protoc`, followed by one guided udev and agent-harness setup. Local stdio remains the MCP transport and the existing daemon remains the sole USB owner. Do not add a remote MCP service or systemd dependency. Homebrew/crates.io are secondary developer channels; add a `.deb` only after the release archive has user evidence, because package-level udev installation is its main additional value.

- [ ] **CLI-004.3**: crates.io publish workflow (PROT layers must be complete first)
- [ ] **CLI-004.4**: Linux x86_64 cargo-dist release pipeline. Publish `cortex` and `cortex-mcp` together as the supported product surface, attach `SHA256SUMS`, licences/notices and the udev rule, and expose a checksum-verifying `install.sh` from the docs-site root. The release workflow must be `workflow_call`-able from `auto-tag.yml`; generated actions must be replaced with verified SHA pins. Add Linux aarch64 only after build and HID behaviour are verified; defer macOS/Windows until their local IPC, process lifecycle and hardware paths are implemented. **Night: slice.** Configure and test the non-publishing pipeline in a PR after reading house-style distribution/CI standards; do not tag, release, publish, or merge
- [x] **CLI-004.7**: First agent-onboarding slice. The source installer now installs both binaries by default, stale MCP-stub claims are removed, package repository/homepage metadata names the real project, and `docs/agent-setup.md` gives current Claude Code and generic stdio registration with an absolute binary path. `s/version++` synchronizes the existing npm and Tauri manifest versions
- [ ] **CLI-004.8**: Self-starting MCP lifecycle. If no daemon endpoint exists, `cortex-mcp` locates its sibling `cortex` binary and starts `cortex session`; concurrent MCP launches converge on the local IPC claim. A protocol mismatch remains an explicit stop/restart refusal. Add request-based daemon idle shutdown so abandoned agent sessions eventually release the device without coupling ownership to one MCP process. **Night: slice.** Implement one process-tested lifecycle boundary (sibling discovery/concurrent start or request-based idle shutdown), using fake local endpoints and no HID access
- [ ] **CLI-004.9**: Guided `cortex setup` for non-developers. Diagnose architecture, USB presence, udev installation/effective hidraw access, Cortex Control contention, daemon health and MCP registration. Support Claude Code first with an absolute stdio command; print harness-specific configuration for others. Privilege escalation must be explicit and narrowly scoped
- [ ] **CLI-004.10**: Test the published install path in a clean Linux environment: checksum verification, both binaries on `PATH`, completions, MCP process discovery, upgrade replacement of both binaries, and actionable no-device/udev diagnostics
- [ ] **CLI-004.11**: Add a `.deb` only after user demand is established. Its purpose is to install both binaries and the udev rule in conventional system locations; RPM remains demand-led rather than default scope
- [~] **CLI-004.12**: Keep the Windows door open. Done: `cortex-host` now exposes transport-neutral `LocalEndpoint`/`LocalListener`/`LocalConnection`; Unix socket paths, stale cleanup and owner permissions live only in its Unix adapter; daemon and MCP tests use the facade; `cortex-host` and `cortex-mcp` cross-check for `x86_64-pc-windows-gnu`. Outstanding: choose a maintained safe named-pipe dependency, implement current-user pipe ACLs and duplex byte streams, abstract detached process creation, then hardware-test the HID/session path on Windows
- [ ] **CLI-004.13**: Add changelog generation before the first public release. Resolve whether `git-cliff` or cargo-dist owns release notes, generate `CHANGELOG.md` in the version-bump flow, and make release automation consume that one source

### CLI-007: Shared machine contract

- [ ] **CLI-007.1**: Add a typed command/input registry and `cortex --schema` output shared by CLI and MCP. MCP currently owns explicit bounded JSON Schemas because the CLI contract promised by MCP NFR-3 does not yet exist; migrate both surfaces to one registry rather than allowing those schemas to drift. **Night: ready.** Keep one registry shared by both surfaces without pulling host/runtime concerns into `cortex-rs`, preserve bounded schemas, and make this satisfy CLI-002.3 rather than adding a second schema implementation

## MCP (MCP)

The `cortex-mcp` MCP server for agentic patch editing. Its first non-persistent slice is implemented and hardware-verified through the held-session daemon; no save or delete tool is exposed.

### MCP-001: Safety surface

- [ ] **MCP-001.1**: Read and recall are free; saving is always explicitly confirmed
- [ ] **MCP-001.2**: Never write to the factory setlist; require one explicitly named USER target (1A-32H) and authorise only that target for the prepared operation
- [ ] **MCP-001.3**: Prepare every target and retain any backup **before working-copy edits begin**. A listing cannot prove emptiness because storage is eventually consistent; calling `read_preset` after edits recalls the target and destroys the grid being saved
- [ ] **MCP-001.6**: Implement and hardware-verify a restoration path before describing retained backup bytes as rollback. Candidate routes are device-side copy/import or keyed replay; an unkeyed whole-preset write is known to do nothing
- [x] **MCP-001.4**: Surface the row-numbering trap (0-based API, 1-4 on screen) in every row-taking tool description and schema
- [x] **MCP-001.5**: Single owning process for the USB interface. `cortex-host` has no HID feature; MCP requires and reuses `cortex session`, opening zero transports. Hardware-verified 2026-08-06

### MCP-003: Show what the MCP server is for

The reason this project has an MCP server at all is that an agent can do things no editor UI can: take a plain-English brief, research what it means, and build the preset. That has to be demonstrated, not asserted.

- [ ] **MCP-003.1**: A worked demo - a brief like "a basic 1987 GnR Slash tone" taken through research, model selection and grid construction to a working tone. **Decided:** build into the LIVE grid and stop short of saving, starting from an empty preset. That needs no save confirmation, is reversible by recalling, and still shows the capability. **The agent researches the web live rather than following a fixed recipe** - the research is the part worth demonstrating, and a canned recipe would show nothing an editor cannot already do. The demo therefore will not reproduce byte-for-byte, which is accepted.
- [ ] **MCP-003.2**: A second demo for the reverse direction - "why does this preset not fit" - using the per-core CPU breakdown, which is a question the official editor answers poorly. The useful answer is not just a number but a strategy: which blocks could move to the other core, row or column.

### MCP-002: Tool surface

- [x] **MCP-002.1**: Read tools: status, version, active scene, CPU, current/stored presets, blocks, folders, preset listings and catalog search. Hardware-verified 2026-08-06 through the official `rmcp` client
- [x] **MCP-002.2**: Transient write tools: `recall_preset`, `switch_scene` (changes what is heard, nothing persistent lost). Hardware-verified 2026-08-06
- [x] **MCP-002.9**: Working-copy scene tools: `set_scene_label`, `unlabel_scene`, `set_scene_color`, `copy_scene`, `swap_scenes`. Introduced with daemon protocol v5 and hardware-verified on 2026-08-09; copy/swap perform a mandatory live-grid refresh because the device acknowledgement cannot distinguish them
- [x] **MCP-002.3**: Working-copy write tools: block placement/removal, named parameter writes, bypass, input/output routing and split. Hardware-verified 2026-08-06 with live-grid read-back and restoration by recall
- [ ] **MCP-002.4**: Destructive tool: `save_preset`. Core PROT-009 correctness blockers are closed; remaining work is an MCP-held exact-target preparation-token registry, explicit confirmation, restoration semantics, typed failures and an MCP save hardware smoke
- [x] **MCP-002.5**: Official `rmcp` stdio server with modern discovery, structured tool results, a custom 16 MiB bounded reader, and an eight-request aggregate cap held through response transmission. Required because stable `rmcp` 3.1.0 still contains the unbounded read tracked by upstream issue #1030 / PR #1049. Process-tested with the official client
- [ ] **MCP-002.6**: Add typed daemon error codes so model-correctable failures such as invalid rows, DSP refusal and reconnecting survive the socket boundary without parsing human-readable strings. **Night: ready.** Define and process-test the shared typed error contract without changing device behavior
- [ ] **MCP-002.7**: Replace raw routing port integers with typed input/output enums and add post-write read-back for bypass, remove, routing, split and parameter writes. The device accepts meaningless output IDs silently, while those daemon acknowledgements currently prove only that a write was sent. Measured 2026-08-10: immediately after `set_bypass`, a cache-backed grid read returned the old value; the explicit full read before a subsequent block move observed the new value, proving dispatch succeeded but cache convergence had not
- [ ] **MCP-002.8**: Start a missing compatible held daemon through the installed sibling `cortex` binary before serving stdio, without ever opening HID in `cortex-mcp`. Preserve stdout exclusively for MCP frames, surface startup diagnostics on stderr, and process-test missing-daemon, concurrent-start and protocol-skew cases. Implement with CLI-004.8 rather than selecting this as a second overnight task

## GUI (GUI)

The typed read/edit, live-cache, health, reconnect, and shared prepared-save foundations are in place. The interactive read-only Tauri first draft now has explicit fixture and daemon-backed modes with generation-checked status, grid, scene, CPU and populated preset directory reads. Its production Rust boundary and physical unplug/reconnect behavior have passed against a real held session in the native Linux window; automated Tauri DOM/IPC checks remain outstanding. Save controls remain disabled until the GUI implements exact-target preparation and confirmation UX, restoration semantics, typed failures and its own write-path hardware smoke. See [400-gui/spec.md](400-gui/spec.md).

**The prepared-save API is now enforced by the daemon and CLI.** Fixed in `743692a` (PROT-009.2): the daemon holds exact-target preparations under opaque tokens, and the CLI is two-phase with confirmation. A GUI that wants target backup and revalidation should call the same API.

The visual design goal is a **hardware-faithful rendering of the Quad Cortex front panel** - 10 footswitch/encoder positions, the colour OLED grid, scene LEDs, and the context strip - with wrapper panels (patch browser, block palette, parameter inspector, scene manager, IR/capture loader) alongside. Use Tauri MCP to tighten the feedback loop during GUI development.

### GUI-001: Scaffold and Tauri MCP

- [x] **GUI-001.1**: `gui/` with Tauri 2 + React + Mantine + Vite, `s/gui-dev` script. Implemented as an interactive read-only first draft with explicit fixture and daemon-backed modes
- [x] **GUI-001.2**: Tauri owns one managed daemon backend using `cortex-host::DaemonClient`; its typed dashboard command returns status, generation/revision-tagged live grid, active scene, CPU and a storage-revision-cached populated preset directory. Rust resolves scene labels, screen rows and active-scene bypass. TypeScript owns only interaction/presentation and never opens HID or sends arbitrary daemon requests
- [ ] **GUI-001.3**: Wire Tauri MCP for the dev feedback loop - drive the GUI from the MCP server to test Tauri commands without manual clicking
- [ ] **GUI-001.4**: Remove the temporary `RUSTSEC-2024-0429` audit exception when stable Tauri moves its Linux runtime from the unmaintained GTK3/glib 0.18 stack to GTK4/glib 0.20 or later. Tauri 2.11.5 is the latest stable release and still pins GTK3; upstream migration is tracked in [tauri-apps/tauri#12562](https://github.com/tauri-apps/tauri/issues/12562) and [PR #14684](https://github.com/tauri-apps/tauri/pull/14684). Do not force two incompatible glib generations or ship the GUI from an unreleased Tauri branch
- [x] **GUI-001.5**: `spec/400-gui/design.md` records the as-built host boundary, short-lived daemon connections, generation guard, directory epoch, explicit fixture/Tauri modes, frontend state rules and limits
- [~] **GUI-001.6**: Rust boundary tests cover readiness, Rust-owned scene conversion and no fixture fallback; both strict TypeScript modes build; browser fixture interaction was inspected at 800x600 and 1280x800. The production dashboard test passes against a real CorOS 4.0.1 held session. HARDWARE-VERIFIED 2026-08-09: unplugging hid the live grid and directory within one second while generation 1 became invalid; automatic recovery restored the same eight-block preset after about 10 seconds under generation 2. A second run exposed attempt/error details, accepted **Reconnect now** at attempt 4 and restored generation 2 in about three seconds; an offline timing test proves the signal interrupts a 10-second wait in under one second. Outstanding: use Tauri MCP from a client that exposes it for automated DOM/IPC checks

### GUI-002: Hardware-faithful control surface

- [ ] **GUI-002.1**: Render the Quad Cortex front panel: 10 footswitch/encoder positions, OLED grid, scene LEDs, context strip
- [ ] **GUI-002.2**: Footswitch interaction: click-to-press (toggle bypass / recall / navigate), drag-to-turn / scroll (adjust parameter), keyboard equivalents
- [ ] **GUI-002.3**: Mode-aware footswitch labels - reflect the current device mode (Preset / Stomp / Scene / Looper / Tuner)
- [ ] **GUI-002.4**: The OLED grid mirrors the device's live state (signal chain, block icons, bypass, active scene) from the crate's read paths
- [ ] **GUI-002.5**: Honest state - render what the device reports, not what the GUI thinks it sent

### GUI-005: Always-visible preset directory and CPU load

Stack confirmed as Tauri 2 + React + Mantine, as AGENTS.md says, unless a concrete reason to change appears.

Improve on the Cortex Control appearance while staying familiar enough to navigate without relearning.

- [x] **GUI-005.1**: A permanently visible left sidebar renders the daemon-supplied tree of populated setlists and slots. Missing/unavailable listings are not presented as empty, and Rust caches the directory by generation/storage revision
- [x] **GUI-005.2**: CPU load is always visible when reported, including total and per-row/per-column second-core markers. Initial absence is rendered as awaiting the first subscribed push rather than failure

### GUI-006: Screen-reader accessibility

[OpenCortex issue #10](https://github.com/VanIseghemThomas/OpenCortex/issues/10) records the concrete need for complete, independent Quad Cortex editing by a blind musician because the official visual editor does not expose a usable screen-reader surface. Treat accessibility as an architectural constraint from the first GUI scaffold, not a later audit. The hardware-faithful panel is one visual presentation of the domain model, never the only route to a control; keyboard equivalents alone are not acceptance.

- [ ] **GUI-006.1**: Build the interaction model from semantic controls. Every control exposes an accessible name, role, current state, value, units, and available action; the signal chain has an ordered nonvisual representation with explicit row, column, routing, block type, and bypass state. Nothing depends only on colour, position, pointer drag, hover, or an icon
- [ ] **GUI-006.2**: Provide screen-reader feature parity across the whole editing surface: browse and recall presets; inspect, place, move, replace, bypass, and remove blocks; edit every parameter in real units; manage scenes, routing, I/O and global settings; and review and confirm saves. Do not ship a reduced "accessible mode"
- [ ] **GUI-006.3**: Make asynchronous device behaviour understandable nonvisually. Announce device-originated state changes, command completion, refusals and silent-failure safeguards without flooding the screen reader; keep focus deterministic across dialogs, live updates and destructive confirmations; expose undo/dirty state wherever the protocol supplies it
- [ ] **GUI-006.4**: Make accessibility part of the release gate: automated semantic/accessibility checks plus manual end-to-end runs with actual screen readers and blind users. Establish Orca on the supported Linux webview as the Linux-first baseline during GUI scaffolding, then add NVDA and VoiceOver when Windows and macOS builds become supported; record the tested app, webview and screen-reader versions

### GUI-003: Wrapper panels

- [ ] **GUI-003.1**: Patch browser - setlist/slot grid for quick preset switching, with search and favourites
- [ ] **GUI-003.2**: Block palette - searchable list of available models from the `Catalog`, drag onto a grid cell
- [ ] **GUI-003.3**: Parameter inspector - form-based editor for the selected block's parameters, showing real units (dB, ms, Hz) via catalog range conversion
- [ ] **GUI-003.4**: Scene manager - copy/swap/relabel/recolor scenes without the footswitch mode dance. **Night: slice.** Build one typed non-persistent scene-manager interaction against the fixture/Tauri API boundary, preserve zero-based API vs A-H display, and request next-day hardware read-back
- [ ] **GUI-003.5**: IR / Capture loader - file-browser-style access to the device's captures and IRs

### GUI-004: Safety surface and governance

- [ ] **GUI-004.1**: Reuse the MCP safety surface (factory refusal, exact target, pre-edit target preparation/backup, explicit confirmation, trap-surfacing) for save actions. If an occupied target was not prepared before the grid became dirty, require the user to choose and prepare another target rather than recalling it and losing the edits
- [ ] **GUI-004.2**: Label hardware-verified vs provisional surfaces in the UI. Model this as a typed capability matrix (`confirmed-readable`, `confirmed-writable`, `inferred`, `unsupported`, `unverified`) whose default is tested to make no unsupported claims, rather than as ad-hoc copy ([prior-art.md](prior-art.md#the-idea-most-worth-stealing))
Version synchronization is completed and tracked canonically under ENG-001.2.
- [ ] **GUI-004.4**: `docs/gui/` explains how to use and run the GUI

## Engineering (ENG)

### ENG-001: DX and testing

- [x] **ENG-001.1**: `s/gui-dev`
- [x] **ENG-001.2**: `s/version++` synchronizes the canonical workspace version into `gui/package.json`, `gui/package-lock.json` and `gui/src-tauri/tauri.conf.json` before creating the release commit
- [ ] **ENG-001.3**: `s/install-hooks` and `.githooks/pre-commit`. **Night: ready.** Follow the house-style hook installer and keep installation explicit
- [ ] **ENG-001.4**: Markdown lint. **Night: ready.** Use the house-style maintained tool/config and avoid reflowing existing prose as drive-by cleanup
- [ ] **ENG-001.5**: Close the documented local/CI parity gap where practical: no-default workspace clippy/tests locally, while keeping platform cross-checks CI-only when native toolchains are unavailable. **Night: ready.** Prefer extending canonical `s/test`/`s/lint` over adding another script

### ENG-002: CI

Release workflows live under CLI-004; this section tracks non-release CI gaps only.

- [ ] **ENG-002.2**: Add frontend `npm run check` and a Tauri build boundary to CI. Add native Windows/macOS jobs only as those hosts become supported. **Night: ready.** Audit the existing partial frontend check first, add only the missing boundary, and use verified SHA-pinned Actions

### ENG-003: Governance

- [x] **ENG-003.2**: CLA decision: no contributor licence agreement is currently wanted; the AGPL inbound-outbound grant is sufficient. Revisit only for a concrete governance need
- [ ] **ENG-003.3**: If a closed derivative ever needs to exist, add `DUAL-LICENSE.md` and the boilerplate. Requires approval
- [ ] **ENG-003.4**: SECURITY.md and CONTRIBUTING.md before the repo is public-facing
- [ ] **ENG-003.5**: If we ever target on-device builds, adapt `qc-stomp-tools` (MIT) with attribution and a NOTICE entry

### ENG-004: Traceability

- [ ] **ENG-004.1**: Add `@see` traceability headers to all owned source files linking to zone specs. **Night: slice.** Cover one complete zone per PR and leave the parent planned until every owned source file is covered
- [ ] **ENG-004.2**: CI gate for `@see` link resolution (optional, low priority). `deskop-nano-cortex/scripts/check-traceability.mjs` is a working reference: resolve document and node ID, fail broken links, warn missing back-references, and include an explicit config/workflow file list ([prior-art.md](prior-art.md#for-eng-004-and-eng-001))

### ENG-005: `s/usb-trace` - observe Cortex Control on the wire

A script that sets up passive USB observation of the official Cortex Control app driving the device, so its traffic can be decoded against our schema. This is the tool for questions of the form "how does the official client do X" - the answer to which is evidence about the wire, not inference about intent.

**It paid for itself before Cortex Control was ever traced.** The first capture was of our OWN client, run only to check whether the device's apparent silences were real or an artefact of our RX path. It showed the bus idle for 46.8 s in the middle of a handshake, with the device answering every write in 217 us - which identified writer starvation in `session.rs` (PROT-008.5, PROT-008.6.10) after a full day of attributing that variance to the device. Two features had already been built and withdrawn on the strength of the wrong explanation. Trace our own side first; it is cheap, needs no VM, and the bug is as likely to be ours.

**Named `usb-trace`, not `usb-record` or `usb-capture`, deliberately.** "Capture" already means a Neural Capture in this domain and "record" implies audio; either would suggest this script records sound, which it emphatically does not. `trace` is already the project's word for protocol observation (`CORTEX_TRACE`), so the two read as the same idea at different levels.

The method is documented in the [protocol reference](../docs/protocol.md#observing-the-wire): with the QC passed through to a Windows VM under QEMU, the **host** kernel still sees the traffic. So `modprobe usbmon` plus a capture of the relevant `usbmonN` interface on the Linux host records everything, without needing USBPcap inside Windows and without the macOS exclusive-access problem.

- [ ] **ENG-005.4**: Optionally a Wireshark Lua dissector, since Wireshark's built-in protobuf support can be pointed at our vendored `.proto` files - worth it only if the GUI earns its place alongside `s/usb-decode --live`
- [ ] **ENG-005.3**: Runbook: what to do in Cortex Control while tracing to answer a specific question, starting with capture export (PROT-007.1)

**Known obstacle.** The QEMU/Windows/Cortex Control setup on the development machine works but drops its connection regularly, so it is adequate for short targeted observations and not for sustained work. Plan traces as single short scripted actions - "open the app, export one capture, stop" - rather than long exploratory sessions, and expect to repeat them.

**Do not commit raw captures.** They contain readable preset, path, device, and build strings. Commit decoded findings in our own words, as the prior art does. See the legal hygiene section of AGENTS.md.

### ENG-006: `s/hardware-smoke` - scripted CLI smoke test with read-back assertions

- [x] **ENG-006.1**: `s/hardware-smoke` - exercises the CLI binary end to end against a real unit: device identity (direct and session paths, cross-checked for agreement), the connect handshake, preset/setlist enumeration, catalog search, a grid-edit cycle (place a block, set a parameter, refuse an unknown one, remove it), and a prepared-save/move/restore/recall/stored-read/delete round trip - each write verified by reading it back through a DIFFERENT command than the one that made it, per the pattern this project's own bug history argues for (spec/roadmap.md PROT-009, spec/completed.md). All writes are confined to one operator-designated scratch bank; the script refuses to run without `--scratch-bank`, `--restore-slot`, and explicit `--discard-working-copy` consent, starts its edits by recalling and preparing the scratch slot, creates its move fixture through that prepared save, and always restores the named slot on exit. This mirrors `safety.rs`'s refusal to supply a default. Asynchronous and file-mutation assertions poll reported state rather than assuming immediate consistency. A failed check is logged and the run continues, so one bad step does not hide the rest.
  - Automates `docs/runbook-hardware-smoke.md` sections 1 and 3-10, the start/status/routed-command/stop portions of section 11, and the move/restore path in section 12. Section 2 is folded into section 1 as a direct/session cross-check. Physical knob input, an on-device save, and physical unplug/replug stay manual: this project has already recorded a USB link dying under repeated large writes (OpenCortex, [prior-art.md](prior-art.md#opencortex---one-high-value-fact-and-a-minefield)), and scripting a physical disconnect is not worth that risk.
  - Writes a result to `smoke-fixtures/<coros_version>.json` (gitignored: it records local firmware details) and, on a successful run, updates `smoke-fixtures/latest.json`. The next run against a DIFFERENT `CorOS` version diffs against it and prints what changed - the concrete answer to "how much of a breaking change did this firmware update cause," in place of an inferred one.
- [~] **ENG-006.2**: Daemon start/status/stop and routed read/edit/save paths are scripted and hardware-verified. Physical unplug/replug was manually verified on 2026-08-07; only a safe automated reconnect substitute remains optional
- [x] **ENG-006.3**: Final core/CLI hardware run, 2026-08-06, CorOS 4.0.1. **37/37 checks passed.** Device identity, direct/session equality, 3.8 s subscribed handshake, enumeration, catalog lookup, daemon lifecycle and live cache, scratch recall and pre-edit save preparation, block placement, parameter write, unknown-parameter refusal, save/list/live-grid/recall read-back, scene switch/reset, block removal, delete, output JSON, final `1A` restore, and daemon shutdown all passed. Earlier failed runs directly found PROT-009.3, the two-connection daemon deadlock, compact-view deserialisation drift, `device probe` bypassing daemon routing, the recall `input_control` cache delta, and asynchronous read-back assumptions; each was fixed before the passing run
- [x] **ENG-006.4**: Preset-move hardware run, repeated 2026-08-08 against CorOS 4.0.1 after the CLI dry-run inversion. **42/42 checks passed.** The script prepared and created a fictional fixture in disposable `7A`, saved and moved it through daemon protocol v4 with the default-executing explicit-slot interface to `7B`, verified source/destination convergence and `storage_revision` advancement, moved it back to `7A`, verified restoration, deleted it, confirmed both slots empty, restored `1A`, and stopped the daemon. Separate `-n`/`--dry-run` checks completed with no daemon running and no device IPC

## Future

- **FUTURE-001**: Nano Cortex hardware verification - third-party macOS observation records provisional VID:PID `152A:88E7` and 65-byte HID reports, not the Quad Cortex's 129. Its HID interface opened but emitted no passive reports; nobody has shown it speaking this protobuf/trailer protocol. Plug in a Nano, verify the transport and handshake rather than assuming a shared shape, then replace the `0xFFFF` sentinel and promote `DeviceKind::NanoCortex` only if the evidence supports it ([prior-art.md](prior-art.md#what-it-says-about-the-nano-cortex-and-one-contradiction))
- **FUTURE-002**: Nano Cortex BLE protocol - the Nano uses BLE for control telemetry; `deskop-nano-cortex` has a provisional decode (Apache-2.0) whose field map credits `choldy/nano-cortex-web-editor` (MIT). Any adaptation carries both attributions ([prior-art.md](prior-art.md#an-additional-project-not-vendored))
- **FUTURE-003 (promoted)**: Cross-platform GUI completion is now active under GUI-001 through GUI-006 and CLI-004.12; preserve this ID as the original umbrella rather than counting it as a separate future item
- **FUTURE-004**: On-device builds (qc-stomp-tools ioctl route) - only if there is a compelling reason; the USB route is preferred
- **FUTURE-005**: Protocol-version probe - surface a CorOS version check rather than hard-coding assumptions, since the protocol has no version field on the wire
- **FUTURE-006**: Conformance suite - port pyquadcortex's offline test suite as a Rust integration test reference
- **FUTURE-007**: Audio feedback loop - let the MCP server "hear" the unit. The Quad Cortex presents class-compliant USB **audio** interfaces that are separate from the HID interface we use, so a host could play a standardised stimulus (DI guitar phrase, sine sweep, impulse) through the chain and capture the processed result **without contending for the exclusive HID connection**.

  Confirmed on hardware 2026-08-02: of the unit's six USB interfaces, **0 through 4 are Audio class (class 1) and only interface 5 is HID (class 3)**. ALSA already enumerates the device as a working card (`USB-Audio - Quad Cortex`) with no driver work required, so the capture side of this needs no reverse engineering at all - it is an ordinary audio device that happens to also speak our HID protocol on a different interface. Comparing captured output against a dry reference would characterise what a chain is doing to the signal.

  Worth being precise about what this buys, because it is not what it first appears. An agent editing a patch already has **ground truth** available: `read_current_preset` returns the actual grid, so "did my edit land on the right block" is answerable today by read-back, and audio analysis is a strictly worse way to answer it. What audio adds is **aesthetic and perceptual judgement** - "is this too dark", "is the gain staging sensible", "does this sound like the reference tone" - which read-back cannot answer at all.

  So this is not a correctness or safety mechanism and should not be treated as one; it is what would let an agent iterate on *tone* rather than on *structure*. That is genuinely novel and nobody has built it, but it is a substantial subsystem (audio I/O, latency alignment, feature extraction, a perceptual similarity metric) and it should not start until the grid-edit surface it would be judging actually exists.

  Open questions: does the QC expose a usable dry/wet split over USB (there is a `dry_wet` field in `USBPortSettings`) so a dry reference can be captured simultaneously rather than in a separate pass; and what stimulus set is both compact and discriminating enough to be worth standardising.
