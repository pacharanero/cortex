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
> Hardware-verified so far (2026-08-02, CorOS 4.0.1 / firmware `d14e` / serial `QA00AB123`): the transport, framing, envelope, session handshake, keepalive, correlation primitives, read and navigation paths; grid writes including block placement/removal, parameters, bypass, routing, and splits; and `save_current_preset` plus `delete_preset`. Individual items below identify what remains provisional or unimplemented.

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

### PROT-001: Transport layer (zone 100)

Complete. See [completed.md](completed.md).

### PROT-002: Framing layer (zone 110)

Complete. See [completed.md](completed.md).

### PROT-003: Proto schema (zone 120)

- [ ] **PROT-003.1**: Curate the message-type registry - map each `CortexMessageType` to its generated struct, so the session layer can dispatch by type tag

### PROT-004: Domain model - core (zone 130)

- [~] **PROT-004.2**: `Grid`. The `Row` newtype exists (`from_wire` / `from_screen`, refusing screen row 0) and encodes the numbering trap. A read-side `Grid` view over a preset is still planned. NOTE: `grid.rs` is taken by the message builders, so the domain view needs another home
- [ ] **PROT-004.4**: `Scene` struct (index, label, color, bypass state)
- [~] **PROT-004.6**: Helpers. Done: `slot_to_position` (+ checked variant), `position_to_slot`, `input_level_db`, `db_to_input_level`, `preset_has_block`. Planned: `blocks()`, `splits()`, `free_rows()`, `row_status()`

### PROT-005: Session layer (zone 140)

Complete. See [completed.md](completed.md).

The background RX thread, connect handshake, keepalive, and request/response correlation.

### PROT-006: Client API (zone 150)

The ergonomic `QuadCortex` struct - the Rust equivalent of pyquadcortex's 60+ methods.

- [~] **PROT-006.4**: Read operations. HARDWARE-VERIFIED 2026-08-02: `read_current_preset` (live grid, no side effects), `read_preset` (recall + capture the echoing push), `active_scene`, `list_presets`, `find_preset`, `list_folders` (via `collect`), plus the `PresetEntry`/`Folder` value objects. Planned: `captures`, `list_irs`, `recents`, `favorites`, `pinned_models`, `master_volume`, `looper`, `tuner`, `io_settings`, `settings`, `global_eq`, `mode` - all twelve have upstream implementation or coverage notes, so start from [prior-art.md](prior-art.md#pyquadcortex---the-one-to-check-first) rather than rediscovering them. Two reported limits matter: master-volume writes were ignored, and the tuner's needle did not stream to a host, so `tuner` reads settings and not pitch
- [~] **PROT-006.5**: Navigation. HARDWARE-VERIFIED 2026-08-02: `recall_preset` (grid swap confirmed by read-back), `switch_scene` (confirmed by `active_scene`). Planned: `copy_scene`, `set_scene_label`, `set_scene_color`; all three have upstream wire shapes and hardware read-back evidence ([prior-art.md](prior-art.md#what-it-has-that-we-do-not))
- [~] **PROT-006.6**: Grid write. HARDWARE-VERIFIED 2026-08-02: `set_block` (echo-verified, cross-checked by read-back), `set_param` (catalog name resolution, read-back confirmed), `remove_block` (DELETE action, read-back confirmed). ALSO hardware-verified 2026-08-02: `set_bypass` (all eight scene slots at once), `set_chain_input`, `set_chain_output`, `set_split` (including the odd-row refusal), and `set_param_in_scene` (the three-message promote/switch/write sequence, confirmed by a per-scene read-back). Not implemented: `move_block`, `write_preset`. The BlockRefused path is now fully hardware-verified in BOTH directions, and provoking a real refusal found a false-positive bug: a missing echo was treated as proof of refusal when echo latency merely varies. The grid read-back is now ground truth and the echo a fast path
  - Upstream supplies the verified `move_block` shape. Its low-level `write_preset` also establishes that writing a recalled preset wholesale does nothing: only row/column-keyed elements apply. Preserve two silent-state traps when placing blocks: empty cells retain old bypass-table state, and adding a block can renormalise unrelated selectors whose option list enumerates the preset's blocks ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))

!!! warning "Start PROT-006.7 through PROT-006.15 from upstream evidence"

    The methods below have upstream implementations, captured wire shapes, or documented negative investigations against the firmware we target. Work from the operation-coverage and manual-coverage tables, preserve their evidence level, then verify this implementation on hardware. See [prior-art.md](prior-art.md#pyquadcortex---the-one-to-check-first).

    Two traps apply throughout. **A nested submessage write replaces the whole submessage** - send one flag of a group and its siblings go false, which in one upstream case quietly stopped the Master Volume knob governing most outputs. Read, merge, write. And **the action field is load-bearing**: some operations need `CREATE`, some `READ`, some no action at all, and the wrong one is ignored in silence rather than refused.

- [ ] **PROT-006.7**: Splitter/mixer/lane/gate: `set_splitter_param`, `set_mixer_param`, `set_lane_output`, `set_input_gate`, `set_split`, `set_split_mute`
- [ ] **PROT-006.8**: Tempo: `set_tempo_param`, `set_tempo_option`, `set_tempo_subdivision`, `set_metronome_sound`, `set_metronome_routing`, `set_time_signature`, `set_tempo_led`, `set_metronome_volume`. All eight exist upstream. The Tempo menu's MODE control is reported to be absent from the wire entirely, and the internal MIDI clock is reported unwritable - both deserve one re-test against the action field before accepting the negative result ([prior-art.md](prior-art.md#where-it-is-wrong-and-why-that-matters))
- [ ] **PROT-006.9**: Stomp/expression/MIDI: `set_stomp_assignment`, `clear_stomp_assignment`, `set_stomp_momentary`, `set_stomp_label`, `set_expression`, `set_expression_bypass`, `set_midi_out`, `set_preset_load_midi_out`; use the upstream operation table and docstrings rather than inferring the multi-message stomp sequence ([prior-art.md](prior-art.md#what-it-has-that-we-do-not))
- [ ] **PROT-006.10**: File ops. **`save_current_preset` and `delete_preset` done and hardware-verified.** Save names a destination slot (`FileMessage{action: CREATE}`); it does NOT upload a preset - the unit commits the working grid, proven by enabling a bypassed block, saving, recalling a different preset, and recalling back to find the change intact. Delete addresses its target by device FILE PATH, not slot index. Guards refuse the factory library and a malformed slot before a session opens. Outstanding: `move_preset` (captured shape documented in `pyquadcortex`: `action: MOVE`, source by path in `folder`, destination by index in `to_folder`), `copy_preset`, `create_setlist`, `delete_setlist`, `duplicate_setlist`, and an `instrument` argument on save (the tag is currently hardcoded to Guitar).
  - **The device may rename what you save.** On a name collision within the setlist it de-duplicates - truncating and appending `_N`, to 20 characters. The stored name is therefore not necessarily the one requested, and delete is name-addressed, so a caller that saves and later deletes must read the listing back rather than assume. Documented by `pyquadcortex` (MIT) and not visible in our own capture.
  - **File mutations are eventually consistent.** Upstream measured all eleven deleted entries still present in a listing five seconds later. Poll until the expected state appears; a fixed sleep produces false failures. There is no host-driven bulk copy, so copy/duplicate helpers must recall and save one preset at a time ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))
- [ ] **PROT-006.11**: Captures/IRs: `set_capture`, `set_ir`, `show_capture_dialog`. **Order matters:** upstream reports `set_capture` resets that block's other parameters to the capture defaults without warning, so select the capture before writing the block's remaining parameters ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))
- [ ] **PROT-006.12**: Global settings: `update_settings`, `set_hold_timing`, `set_scene_bypass_behavior`, `set_master_volume_assignment`, `set_global_bypass`, `set_global_eq`, `set_mode_cycle`, `set_mode`, `set_gig_view`, `show_tuner`, `set_tuner_input`, `set_tuner_mute`, `set_tuner_reference`. Read-merge-write nested submessages rather than treating them as sparse. Refuse the upstream-observed mode-cycle value that stores and reads back but leaves the footswitches dead ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))
- [ ] **PROT-006.13**: I/O ports: `set_input_port`, `set_output_port`, `set_usb_port`, `set_midi_thru`, `set_output_pairing`. Output mute, input impedance mode, and USB dry/wet are reported to vanish silently when packed with a sibling; send those fields alone ([prior-art.md](prior-art.md#device-behaviour-that-fails-silently))
- [ ] **PROT-006.14**: Pinning/favorites: `pin_model`, `unpin_model`, `add_favorite`, `remove_favorite`. Do not infer the action: upstream reports pinning with no action field, unpinning with `DELETE`, and favourites with `CREATE` ([prior-art.md](prior-art.md#where-it-is-wrong-and-why-that-matters))
- [ ] **PROT-006.15**: Module-level helpers: `blocks()`, `splits()`, `input_chain_rows()`, `stomp_assignments()`, `midi_out()`, `tempo_params()`, `param_options()`, `free_rows()`, `row_status()`, `params_equal()` - `slot_to_position`, `position_to_slot`, `input_level_db`, `db_to_input_level` done. Also port the upstream ergonomic helper that selects a list parameter by option name and performs `index / (count - 1)` centrally ([prior-art.md](prior-art.md#what-it-has-that-we-do-not))
- [~] **PROT-006.16**: Hardware smoke test. Done 2026-08-02 for the currently implemented read, navigation, grid-write, routing, parameter, bypass, save, and delete surfaces against CorOS 4.0.1 / d14e / QA00AB123, with the unit restored to its starting state. Trap 14 (a recall resets the active scene) and the real DSP-capacity refusal path were confirmed live. Repeat the smoke run as each remaining PROT-006 method lands

### PROT-008: Session performance

Commands were taking tens of seconds for milliseconds of work. Most of that is fixed (see [140-session/spec.md](140-session/spec.md)); what is left is listed here.

- [ ] **PROT-008.6**: `cortex session start` - a persistent, subscribed session. **In progress: the session holds and serves, routes every ordinary command, reduces subscribed state, reports health, and reconnects with backoff. Only the daemon idle-timeout policy remains.** Cortex Control is fast because it opens ONE session and keeps it; we pay a handshake per command. It is also the right shape for the MCP server, which must hold a single connection anyway, and for the GUI. Measured against a held session: `scene` 0.07 s, `grid` 0.14 s, `--status` 0.005 s, against a 3-5 s direct baseline.
  - [ ] **008.6.3**: Lifecycle. Done: `status`, a clean `stop` that announces the disconnect and is bounded by a watchdog, stale-socket detection, and **backgrounding** - `session start` detaches with `setsid` and returns only once the session answers a request, not merely once the socket binds (those differ by the whole handshake, because the socket is claimed first). **Outstanding: an idle timeout.** The GUI equivalent should follow `deskop-nano-cortex` and release its device handle under a bounded timeout on window close ([prior-art.md](prior-art.md#for-gui-001-to-gui-005))
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
- [ ] **PROT-007.5**: Same for user IRs (`list_irs` already enumerates them). Start from the upstream candidate request and its mandatory bulk-count field; do not repeat its failed payload encodings without new evidence
- [ ] **PROT-007.6**: `cortex capture export` / `import` CLI surface
- [ ] **PROT-007.7**: Decide and document a container format. Prefer something self-describing that records the source unit, CorOS version, and capture metadata, so a file is still meaningful years later

Do not ship import before export has been round-tripped on hardware: writing a malformed capture to the unit is the most plausible way this project could damage a user's data.

### PROT-009: Correctness gaps found in review, 2026-08-05

Raised by a review of the state-cache/prepared-save/daemon-routing change (`a603bce`). The tree was green - fmt, clippy, `reuse`, and 149 tests all passed - so every item here is a behavioural gap that the existing tests do not reach. **009.1, 009.2, 009.4, 009.10, and 009.11 have been independently confirmed and fixed; the rest are the reviewer's claims and need checking before they are acted on or dismissed.**

Ordered by what would hurt a user most.

- [x] **PROT-009.2**: **The shipping save path bypasses the prepared-save API entirely.** FIXED 2026-08-05 in `743692a`. The daemon now exposes `PrepareSave`/`CommitSave` with server-held preparations and opaque tokens; the CLI `preset save` command is two-phase with `--yes`/TTY confirmation and `--scratch-range` policy. The raw `SavePreset` request is gone. CONFIRMED by call-site search: `save_current_preset` now has zero production callers outside `safety.rs`. So the safety layer this change added does not protect the surfaces a user actually touches, and a non-`/opt/neuraldsp/` slot can still be overwritten with no backup. **Fix before anything describes prepared saves as a guarantee.** Referred to elsewhere as PROT-006.10.1
- [ ] **PROT-009.1**: **Reconnect can open a replacement while the old HID handle is still held.** CONFIRMED by reading the code. The handle lives behind `Arc<Mutex<dyn HidLink>>`, cloned into both the RX and keepalive threads, and `Transport` releases it only on drop; `Session::stop()` only joins the workers and `disconnect()` only sends a message. The reconnect path keeps the old `Arc<Session>` in its slot until a replacement has opened, and in-flight handlers may hold further clones - so the replacement can open alongside a live handle, violating the single-owner invariant that this repo documents as unenforced and fatal on the *next* request. A fix probably needs an explicit release on `Session` rather than relying on `Arc` drop order. The fake-link test cannot see this, because independent fakes model neither exclusivity nor drop order
- [ ] **PROT-009.3**: **The daemon acknowledges a recall before the grid swap lands.** The direct path sleeps because the swap is lazy; the routed path returns straight after the write. A recall followed immediately by an edit or save can therefore act on the previous grid, and `CurrentPreset` can serve the pre-recall cache value. The same window follows scene and working-copy writes. Await the correlated push, or invalidate the affected cache entry, before replying
- [x] **PROT-009.4**: **An eventually consistent listing is treated as proof a slot is empty.** FIXED 2026-08-05 in `743692a`. The backup read is now always attempted regardless of the listing; the listing determines `previous_name` for the UI but never decides whether to skip the backup. The consent gate widens to all targets. A fake-session test (`a_stale_listing_saying_empty_still_backs_up_the_real_preset`) pins the stale-listing case.
- [ ] **PROT-009.5**: **Preparation is revalidated against an epoch that may not be trustworthy.** `validate_current` rejects only `Invalidated`, admitting `Unsubscribed` and `Incomplete`; without a live File subscription an in-place edit that leaves listing metadata unchanged will not advance `storage_revision` and will pass. `finish_subscription` can also turn a stream-gapped handshake into `Incomplete`, routing around the explicit refusal. Prepared saves need a demonstrably live subscribed generation, or a content-level proof of the target
- [ ] **PROT-009.6**: **File replies are correlated by shape, not by operation or target.** Save and delete accept the first non-empty `File` message, so an unrelated folder announcement can report a destructive operation as successful; `list_presets` accepts any same-folder non-empty `File` without checking `action` or completeness, so a one-item mutation acknowledgement can pass as a full listing. `SaveReceipt` inherits the false-success risk
- [ ] **PROT-009.7**: **A cached listing can freeze a stale result indefinitely.** Every `File UPDATE` is cached as a complete folder and daemon listings then serve it without re-reading, so a post-mutation listing that was stale when cached may never converge. Mutation handling also ignores `MessageAction::Swap` and invalidates only inferred folder keys
- [x] **PROT-009.8**: **A malformed envelope is skipped without breaking cache continuity.** FIXED 2026-08-05. A failed `Message::decode` now calls `stream_gap`, invalidating the cache because the lost envelope's message type cannot be recovered. A fake-link test proves the next valid message still reaches its waiter.
- [ ] **PROT-009.9**: **Cache continuity never recovers once lost.** `stream_gap` clears state and permanently clears `stream_valid`, but the reconnect monitor only replaces a session that stops being responsive. A device that keeps answering therefore leaves the daemon in uncached fallback for good
- [x] **PROT-009.10**: **The daemon socket protocol changed without a version gate.** FIXED 2026-08-05 in `743692a`. `DAEMON_PROTOCOL_VERSION = 2` is checked by the client before sending; a mismatch names the fix ("run `cortex session stop`") rather than producing a parse error.
- [x] **PROT-009.11**: **`SavePolicy::is_user_setlist` accepts dot path components.** FIXED 2026-08-05 in `743692a`. `.` and `..` are now rejected; the override-escape test covers them.
- [x] **PROT-009.12**: **Named parameter resolution does not enforce the catalog's value type.** FIXED 2026-08-05. Named parameters now accept text only for catalog `Str` controls and normalised or real-unit values only for numeric (`Float`, `Int`, `Switch`, `Fader`) controls. Meters, empty placeholders, and unknown types refuse before the wire. Raw index addressing remains intentionally untyped because it has no catalog metadata. A pure resolver test covers every allowed and refused category.
- [x] **PROT-009.13**: **An empty prepared target can be saved with no name.** FIXED 2026-08-05. `save_prepared` now refuses `name: None` when the prepared target has no existing name, before it lists or writes to the device. A fake-link test pins the refusal and asserts zero device writes.
- [ ] **PROT-009.14**: **Tests the above need**, none of which exist: an exclusivity-aware fake that asserts every lease drops before a replacement opens; recall/edit/save ordering against the device push; a target filled immediately before preparation; unrelated `File` traffic interleaved with listing, save and delete; the `Unsubscribed`/`Seeding`/`Incomplete` phases and same-metadata mutation; stale post-mutation listings, partial `File UPDATE`, `SWAP`, and malformed complete envelopes; old-client/new-daemon skew both ways; and parameter kind mismatch, real-unit conversion and meter refusal. A fake-link pass cannot stand in for the hardware smoke on reconnect and prepared save

## Docs (DOCS)

### DOCS-001: Documentation site

Complete. See [completed.md](completed.md).

A Zensical site per house-style [docs.md](https://github.com/marcus-pacharanero/house-style/blob/main/docs.md), served by `s/docs`.

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

### CLI-001: Scaffold and version

Complete. See [completed.md](completed.md).

### CLI-002: Format and output

- [ ] **CLI-002.3**: `--schema` / `--print-schema` - JSON Schema of a command's inputs

### CLI-003: Preset and scene commands

All HARDWARE-VERIFIED 2026-08-02 against CorOS 4.0.1 / d14e / QA00AB123. Named without the `list-` prefix, since the noun already reads as a listing and `cortex preset list` is what a user reaches for.

- [ ] **CLI-003.9**: `cortex capture` / `cortex ir` - export and import (blocked on PROT-007)

### CLI-005: Noun-primitive command redesign

Complete. See [completed.md](completed.md).

The surface grew verb-first (`set-param`, `set-bypass`, `remove-block`) and should be rooted in the nouns the Neural DSP / Quad Cortex user guide uses - preset, slot, grid, row, column - named exactly as a player meets them.

### CLI-006: Command reference with syntax and examples

Complete. See [completed.md](completed.md).

`docs/cli-reference.md` is currently the raw `--help` dump for every subcommand, which is accurate and hard to read.

### CLI-004: Distribution

- [ ] **CLI-004.3**: crates.io publish workflow (PROT layers must be complete first)
- [ ] **CLI-004.4**: cargo-dist release pipeline (archives + installers)

## MCP (MCP)

The `cortex-mcp` MCP server for agentic patch editing. Greenfield - no MCP server for any Neural DSP hardware exists.

### MCP-001: Safety surface

- [ ] **MCP-001.1**: Read and recall are free; saving is always explicitly confirmed
- [ ] **MCP-001.2**: Never write to the factory setlist; restrict saves to a host-configured range of valid USER slots (1A-32H) unless explicitly overridden. Do not invent a default range: only the user knows which slots are disposable
- [ ] **MCP-001.3**: Prepare an occupied target and retain its backup **before working-copy edits begin**. Correction: calling `read_preset` immediately before save recalls the target and destroys the unsaved grid being saved. A target selected after edits must be listing-confirmed empty or the save is refused
- [ ] **MCP-001.4**: Surface the row-numbering trap (0-based API, 1-4 on screen) in tool descriptions
- [ ] **MCP-001.5**: Single owning process for the USB interface

### MCP-003: Show what the MCP server is for

The reason this project has an MCP server at all is that an agent can do things no editor UI can: take a plain-English brief, research what it means, and build the preset. That has to be demonstrated, not asserted.

- [ ] **MCP-003.1**: A worked demo - a brief like "a basic 1987 GnR Slash tone" taken through research, model selection and grid construction to a working tone. **Decided:** build into the LIVE grid and stop short of saving, starting from an empty preset. That needs no save confirmation, is reversible by recalling, and still shows the capability. **The agent researches the web live rather than following a fixed recipe** - the research is the part worth demonstrating, and a canned recipe would show nothing an editor cannot already do. The demo therefore will not reproduce byte-for-byte, which is accepted.
- [ ] **MCP-003.2**: A second demo for the reverse direction - "why does this preset not fit" - using the per-core CPU breakdown, which is a question the official editor answers poorly. The useful answer is not just a number but a strategy: which blocks could move to the other core, row or column.

### MCP-002: Tool surface

- [ ] **MCP-002.1**: Read tools: `list_presets`, `read_preset`, `list_blocks`, `get_device_version` (unrestricted)
- [ ] **MCP-002.2**: Transient write tools: `recall_preset`, `switch_scene` (changes what is heard, nothing persistent lost)
- [ ] **MCP-002.3**: Working-copy write tools: `set_block`, `set_param`, `set_routing` (edits the recalled preset in device RAM)
- [ ] **MCP-002.4**: Destructive tool: `save_preset` (gated: explicit slot, refuse FACTORY, require confirmation)

## GUI (GUI)

The typed read/edit, live-cache, health, reconnect, and shared prepared-save foundations are now in place. The GUI can begin with the Rust backend retaining opaque `SavePreparation` values and exposing only their serialisable views. Keep saves labelled provisional until the safety sequence passes its hardware smoke and require user-configured scratch ranges before enabling them. See [400-gui/spec.md](400-gui/spec.md).

**The prepared-save API is now enforced by the daemon and CLI.** Fixed in `743692a` (PROT-009.2): the daemon holds preparations under opaque tokens, the CLI is two-phase with confirmation. A GUI that wants target backup, scratch policy and revalidation should call the same API.

The visual design goal is a **hardware-faithful rendering of the Quad Cortex front panel** - 10 footswitch/encoder positions, the colour OLED grid, scene LEDs, and the context strip - with wrapper panels (patch browser, block palette, parameter inspector, scene manager, IR/capture loader) alongside. Use Tauri MCP to tighten the feedback loop during GUI development.

### GUI-001: Scaffold and Tauri MCP

- [ ] **GUI-001.1**: `gui/` with Tauri 2 + React + Mantine + Vite, `s/gui-dev` script
- [ ] **GUI-001.2**: Tauri commands calling `cortex-rs` and returning typed serialisable data; no protocol/domain logic in TypeScript. Follow the prior-art boundary: one managed Rust `AppState`, optional transport behind a feature with stubs, and frontend Tauri access only through mockable wrapper modules ([prior-art.md](prior-art.md#for-gui-001-to-gui-005))
- [ ] **GUI-001.3**: Wire Tauri MCP for the dev feedback loop - drive the GUI from the MCP server to test Tauri commands without manual clicking

### GUI-002: Hardware-faithful control surface

- [ ] **GUI-002.1**: Render the Quad Cortex front panel: 10 footswitch/encoder positions, OLED grid, scene LEDs, context strip
- [ ] **GUI-002.2**: Footswitch interaction: click-to-press (toggle bypass / recall / navigate), drag-to-turn / scroll (adjust parameter), keyboard equivalents
- [ ] **GUI-002.3**: Mode-aware footswitch labels - reflect the current device mode (Preset / Stomp / Scene / Looper / Tuner)
- [ ] **GUI-002.4**: The OLED grid mirrors the device's live state (signal chain, block icons, bypass, active scene) from the crate's read paths
- [ ] **GUI-002.5**: Honest state - render what the device reports, not what the GUI thinks it sent

### GUI-005: Always-visible preset directory and CPU load

Stack confirmed as Tauri 2 + React + Mantine, as AGENTS.md says, unless a concrete reason to change appears.

Improve on the Cortex Control appearance while staying familiar enough to navigate without relearning.

- [ ] **GUI-005.1**: Preset directory in a left sidebar, visible at all times rather than behind a picker. **Decided:** a tree (setlist then slots), showing only populated folders by default. The unit reports 399 folders and two hold anything, so an unfiltered tree would misrepresent how full the unit is.
- [ ] **GUI-005.2**: CPU load visible at all times, **including the per-column, per-core breakdown**, not just a total. `Session::cpu_load()` supplies both (PROT-008.6.12). The per-core detail is the actionable part: when a preset will not fit, it shows which work could move to the other core, row or column.

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
- [ ] **GUI-003.4**: Scene manager - copy/swap/relabel/recolor scenes without the footswitch mode dance
- [ ] **GUI-003.5**: IR / Capture loader - file-browser-style access to the device's captures and IRs

### GUI-004: Safety surface and governance

- [ ] **GUI-004.1**: Reuse the MCP safety surface (factory refusal, configured scratch range, pre-edit target preparation/backup, trap-surfacing) for save actions. If an occupied target was not prepared before the grid became dirty, offer an empty scratch slot rather than recalling it and losing the edits
- [ ] **GUI-004.2**: Label hardware-verified vs provisional surfaces in the UI. Model this as a typed capability matrix (`confirmed-readable`, `confirmed-writable`, `inferred`, `unsupported`, `unverified`) whose default is tested to make no unsupported claims, rather than as ad-hoc copy ([prior-art.md](prior-art.md#the-idea-most-worth-stealing))
- [ ] **GUI-004.3**: `s/version++` bumps `gui/package.json` and `tauri.conf.json` with the canonical version. `deskop-nano-cortex` has a working multi-manifest sync script and CI drift mode to use as an architectural reference ([prior-art.md](prior-art.md#for-eng-004-and-eng-001))
- [ ] **GUI-004.4**: `docs/gui/` explains how to use and run the GUI

## Engineering (ENG)

### ENG-001: DX and testing

- [ ] **ENG-001.1**: `s/gui-dev` (once `gui/` exists)
- [ ] **ENG-001.2**: `s/version++` exists (CLI-004.1). Outstanding: teach it the GUI manifests - `gui/package.json` and `gui/src-tauri/tauri.conf.json` must move in the same release commit once they exist
- [ ] **ENG-001.3**: `s/install-hooks` and `.githooks/pre-commit`
- [ ] **ENG-001.4**: Markdown lint

### ENG-002: CI

- [ ] **ENG-002.1**: The release workflows live in CLI-004 (auto-tag, crates.io, cargo-dist), which is where distribution is tracked. Kept here only as a pointer, because CI is where they run

### ENG-003: Governance

- [ ] **ENG-003.2**: Decide whether a contributor licence agreement is wanted. Current stance: not in scope - the AGPL header is the inbound-outbound grant
- [ ] **ENG-003.3**: If a closed derivative ever needs to exist, add `DUAL-LICENSE.md` and the boilerplate. Requires approval
- [ ] **ENG-003.4**: SECURITY.md and CONTRIBUTING.md before the repo is public-facing
- [ ] **ENG-003.5**: If we ever target on-device builds, adapt `qc-stomp-tools` (MIT) with attribution and a NOTICE entry

### ENG-004: Traceability

- [ ] **ENG-004.1**: Add `@see` traceability headers to all owned source files linking to zone specs
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

- [~] **ENG-006.1**: `s/hardware-smoke` - exercises the CLI binary end to end against a real unit: device identity (direct and session paths, cross-checked for agreement), the connect handshake, preset/setlist enumeration, catalog search, a grid-edit cycle (place a block, set a parameter, refuse an unknown one, remove it), and a save/recall/stored-read/delete round trip - each write verified by reading it back through a DIFFERENT command than the one that made it, per the pattern this project's own bug history argues for (spec/roadmap.md PROT-009, spec/completed.md). All writes are confined to one operator-designated scratch bank; the script refuses to run without `--scratch-bank`, `--restore-slot`, and explicit `--discard-working-copy` consent, starts its edits by recalling the scratch slot, and always restores the named slot on exit. This mirrors `safety.rs`'s refusal to supply a default. File-mutation assertions poll the listing rather than assuming it is instantly consistent. A failed check is logged and the run continues, so one bad step does not hide the rest.
  - Automates `docs/runbook-hardware-smoke.md` sections 1, 3-10. Section 2 is folded into section 1 as a direct/session cross-check. Section 11 (held-session/cache/reconnect) and a physical unplug/replug stay manual: this project has already recorded a USB link dying under repeated large writes (OpenCortex, [prior-art.md](prior-art.md#opencortex---one-high-value-fact-and-a-minefield)), and scripting a physical disconnect is not worth that risk.
  - Writes a result to `smoke-fixtures/<coros_version>.json` (gitignored: it records local firmware details) and, on a successful run, updates `smoke-fixtures/latest.json`. The next run against a DIFFERENT `CorOS` version diffs against it and prints what changed - the concrete answer to "how much of a breaking change did this firmware update cause," in place of an inferred one.
- [ ] **ENG-006.2**: Script `cortex session start`/`status`/`stop` and the daemon-routed command paths (runbook section 11), so the held-session, cache, and reconnect behaviour gets the same read-back discipline as the one-shot paths
- [ ] **ENG-006.3**: First hardware run against a real unit. The script asserts the documented `device probe --format json` `.active_scene` field and preflights the selected grid-test model's writable `GAIN` parameter before it writes. It also asserts that save preserves the edited working grid, which is essential because a preparation that recalls the target after editing silently saves the wrong grid.

## Future

- **FUTURE-001**: Nano Cortex hardware verification - third-party macOS observation records provisional VID:PID `152A:88E7` and 65-byte HID reports, not the Quad Cortex's 129. Its HID interface opened but emitted no passive reports; nobody has shown it speaking this protobuf/trailer protocol. Plug in a Nano, verify the transport and handshake rather than assuming a shared shape, then replace the `0xFFFF` sentinel and promote `DeviceKind::NanoCortex` only if the evidence supports it ([prior-art.md](prior-art.md#what-it-says-about-the-nano-cortex-and-one-contradiction))
- **FUTURE-002**: Nano Cortex BLE protocol - the Nano uses BLE for control telemetry; `deskop-nano-cortex` has a provisional decode (Apache-2.0) whose field map credits `choldy/nano-cortex-web-editor` (MIT). Any adaptation carries both attributions ([prior-art.md](prior-art.md#an-additional-project-not-vendored))
- **FUTURE-003**: Tauri desktop GUI (zone 400) - React + Mantine + Vite, a consumer of the crate; use Tauri MCP to tighten the feedback loop
- **FUTURE-004**: On-device builds (qc-stomp-tools ioctl route) - only if there is a compelling reason; the USB route is preferred
- **FUTURE-005**: Protocol-version probe - surface a CorOS version check rather than hard-coding assumptions, since the protocol has no version field on the wire
- **FUTURE-006**: Conformance suite - port pyquadcortex's offline test suite as a Rust integration test reference
- **FUTURE-007**: Audio feedback loop - let the MCP server "hear" the unit. The Quad Cortex presents class-compliant USB **audio** interfaces that are separate from the HID interface we use, so a host could play a standardised stimulus (DI guitar phrase, sine sweep, impulse) through the chain and capture the processed result **without contending for the exclusive HID connection**.

  Confirmed on hardware 2026-08-02: of the unit's six USB interfaces, **0 through 4 are Audio class (class 1) and only interface 5 is HID (class 3)**. ALSA already enumerates the device as a working card (`USB-Audio - Quad Cortex`) with no driver work required, so the capture side of this needs no reverse engineering at all - it is an ordinary audio device that happens to also speak our HID protocol on a different interface. Comparing captured output against a dry reference would characterise what a chain is doing to the signal.

  Worth being precise about what this buys, because it is not what it first appears. An agent editing a patch already has **ground truth** available: `read_current_preset` returns the actual grid, so "did my edit land on the right block" is answerable today by read-back, and audio analysis is a strictly worse way to answer it. What audio adds is **aesthetic and perceptual judgement** - "is this too dark", "is the gain staging sensible", "does this sound like the reference tone" - which read-back cannot answer at all.

  So this is not a correctness or safety mechanism and should not be treated as one; it is what would let an agent iterate on *tone* rather than on *structure*. That is genuinely novel and nobody has built it, but it is a substantial subsystem (audio I/O, latency alignment, feature extraction, a perceptual similarity metric) and it should not start until the grid-edit surface it would be judging actually exists.

  Open questions: does the QC expose a usable dry/wet split over USB (there is a `dry_wet` field in `USBPortSettings`) so a dry reference can be captured simultaneously rather than in a separate pass; and what stimulus set is both compact and discriminating enough to be worth standardising.
