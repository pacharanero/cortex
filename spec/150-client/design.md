---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["client", "api", "quad-cortex", "presets", "grid", "scenes", "provisional"]
spec: spec.md
---

# 150 Client - Design

## [DES-CLIENT-OVR] Overview

The `QuadCortex` client is the ergonomic API surface. It holds an `Arc<Session>` and builds protobuf messages, handing them to the session's `send`/`request`/`await_broadcast`/`collect` primitives. It knows nothing about hidapi, framing, or the session state machine. Implemented operations are ported from the corresponding parts of `pyquadcortex/pyquadcortex/client.py::QuadCortex` (MIT) and adapted to Rust idioms; the remaining upstream surface stays explicit roadmap work rather than a claimed API.

The client is a thin layer: most methods are 5-20 lines of protobuf construction plus a `session.send` or `session.await_broadcast`. The complexity lives in the domain traps (see spec -> Domain Traps), which must be encoded as runtime checks and rustdoc, not hidden.

---

## [DES-CLIENT-ARCH] Architecture

### File Map

```text
crates/cortex-rs/src/
├── client.rs              — QuadCortex lifecycle and implemented operations
├── grid.rs                — pure keyed grid-message builders and checked rows
├── catalog.rs             — model catalog parser
├── view.rs                — shared host-facing serialisable views
└── safety.rs              — prepared-save policy and execution
```

### Flow Map: `[Flow.Client]`

```text
[CLI (zone 200) / MCP (zone 300) / Tauri (zone 400)]
        │ calls QuadCortex::read_preset / set_param / save_current_preset / ...
        ▼
[QuadCortex (this zone)]
  ├── builds a protobuf message (Grid, File, Scene, SetlistPosition, ...)
  ├── resolves names -> wire indices via ModelCatalog
  ├── applies domain-trap guards (row 0/2 for splitters, Input enum for ports, ...)
  └── calls session.send / session.request / session.await_broadcast / session.collect
        │
        ▼
[Session (zone 140)]
        │  request_id correlation, broadcast waiting, keepalive
        ▼
[Transport (zone 100)]
        ↕  USB HID
[Quad Cortex hardware]
```

---

## [DES-CLIENT-DATA] Data Model

### `QuadCortex` struct

| Field            | Type                      | Description                                                              |
| ---------------- | ------------------------- | ------------------------------------------------------------------------ |
| `session` | `Arc<Session>` | The session this client drives. Short-lived wrappers share one held session. |

The implemented client-owned values include `PresetEntry`, `Folder`, `ParameterInput`, `ParameterWrite`, checked footswitch/expression/MIDI selectors, and `MidiOut` constructors. Renderable grid values such as `view::Preset`, `view::Block`, and `view::Row` belong to zone 130 so every host sees one representation. Pure protobuf navigation and comparison values belong to zone 130's public `helpers` module; the client reuses its keyed occupancy and dynamic-option interpretation.

The implemented client constants are `USER_SETLIST_ROOT`, `USER_SETLIST`, `SCENE_UNLABELLED`, `UNITY_LEVEL`, `BANKS`, `SLOTS_PER_BANK`, and `SETLIST_SLOTS`. Firmware versions, model ids, capture/IR keys, and parameter indices are not asserted as client constants unless an implemented operation needs them.

---

## [DES-CLIENT-CONNECT] Lifecycle

`QuadCortex::connect(kind, timeout, settle)` is the convenience entry point (the Rust equivalent of `pyquadcortex.connect()`):

1. `Session::open(kind)` opens the HID link and starts the session workers.
2. `session.connect(timeout, settle)` runs the paced handshake.
3. The ready session is wrapped in `Arc` and passed to `QuadCortex::new`.

`close()` delegates to `Session::close()`. The session is held in `Arc` so the daemon can share it across tool invocations; therefore `QuadCortex` does not close on drop, because dropping one short-lived wrapper must not disconnect its siblings. `Session::Drop` remains the final fallback, while explicit close proves the link is gone before reconnect even if other session references remain.

---

## [DES-CLIENT-READ] Read Operations

The read methods fall into two correlation patterns:

**Pattern A: `request()` (prompt same-type reply)** - used by `version()` and `active_scene()`. Correlation is type-first; an echoed request id is only a consistency check because some READ replies are id-less.

**Pattern B: `await_broadcast()` (asynchronous push)** - used by `read_current_preset()`, `read_preset()`, and `list_presets()`. These send a trigger and wait for a matching push. Both preset reads match a `RecallPreset` push carrying the request id; `read_preset` additionally skips the id-less seed push generated around recall.

**Pattern C: `collect()` (fan-out)** - used by `list_folders()`. A single `File` READ produces a flood of folder listings over 10-20 s; `collect` gathers them all.

**Pattern D: fire-and-forget `send`** - retained for low-level primitives such as `switch_scene()`, scene label/colour, `set_param()`, and `set_block(verify=false)`. The write STALL is swallowed by the transport. The daemon follows every scene copy/swap with `read_current_preset()` because the device's `SceneCopy` acknowledgement omits `is_swap` even when it performed a swap, so reducing that echo would turn a correct device swap into an incorrect cached copy.

**Pattern E: predicate-bearing state READ** - used by Master Volume, Looper, Tuner, I/O settings, general settings, Global EQ and mode. A bare typed READ waits for a push containing the specific presence-bearing field promised by that method, rather than accepting an unrelated partial update. Mode and mode-cycle reads intentionally use different predicates. Recents requires a non-empty list because its uncorrelated transient empty push is ambiguous; Pinned Models has no repeated-field presence and therefore accepts an empty same-type reply.

**Pattern F: request-id and folder-correlated variable File listing** - used by captures and IRs. The request carries the category type and a fresh request id; a reply must echo that id and name the normalized folder. One matched `File{UPDATE}` is returned, including an empty folder, but no total-count or end marker exists to prove that firmware supplied every possible entry.

---

## [DES-CLIENT-GRID] Grid Write

The grid-write methods share a single design principle: **sparse, row/column-keyed updates.** A `Grid{UPDATE}` carrying only the changed chain/model/param is the ONLY shape that persists. A full-preset write whose chains lack `row` is dropped (trap #2).

`set_param_in_scene` is the most complex: the flag (`scene_mode`) and a value cannot travel together (trap #4), so it issues 3 messages in order:
1. `set_param_scene_mode(row, column, param_index, true)` - the flag, alone.
2. `switch_scene(scene)` - switch to the target scene.
3. The value write - `Grid{UPDATE, preset{chains{row, models{column, params{index, param_values[0]{float_value}}}}}}`.

No settle delay is needed between them; ordering over the pipe is sufficient.

`set_block(verify=true)` treats a matching `Grid` echo as a fast path. If it times out, the live grid is read back; only confirmed absence becomes `BlockRefused`. This avoids false refusals when echo latency varies.

`remove_block` uses action `DELETE` (trap #6). An `UPDATE` with `hash: 0` is transmitted and ignored.

Host-facing parameter, bypass, removal, routing, and split methods send their sparse write and then issue a complete `RecallPreset{READ}` before returning. `send_grid_and_verify` centralises the send/read/error sequence; operation-specific predicates resolve explicit row, column, and parameter keys with positional fallback where the complete preset omits keys. A missing row or cell is not proof of removal or a cleared split. A mismatch returns `GridWriteUnconfirmed`, which the daemon maps to `outcome_unconfirmed`.

---

## [DES-CLIENT-SPLIT] Splitter/Mixer/Lane/Gate

PROT-006.7 uses pure grid builders beneath five thin client methods. A typed `SubControl` selects `mixer`/11000, `output_control`/23000, or `input_control`/28000 as one inseparable pair; splitter has dedicated builders because its writable `combined_splitter` shape deliberately carries no hash, and split mute has a dedicated builder because only `split_bypass` is writable. Parameter and scene-mode builders are separate types of message. The public methods take raw indices and finite normalised values, validate before I/O, and compose promotion, scene switch, and value writes without duplicating catalog resolution across four controls. Name-based gate resolution is deferred rather than exposing a route that could mistake a meter for a control. All five operations passed fresh hardware read-back and recall restoration on CorOS 4.0.1; splitter verification reads `combined_splitter` because the legacy `splitter` view does not expose the applied write.

---

## [DES-CLIENT-TEMPO] Tempo

PROT-006.8 uses one pure grid builder for the non-row-keyed `tempo_program_data` exception. It always emits one model with hash 25000 and one explicitly indexed finite normalised parameter. `TempoParameter` encodes only controls with positive wire evidence and preserves the positional gap at index 1; no MODE or internal MIDI clock write is inferred. Normalised inputs are checked before I/O, while real-unit inputs resolve the matching positional catalog parameter and therefore fail honestly for SOUND or ROUTING when the catalog does not describe those indices. `set_tempo_option` owns the four established cardinalities rather than depending on incomplete catalog list metadata. The four typed enums implement checked integer conversion and exact `option / (count - 1)` normalization. Time-signature verification checks its target only because the device may legitimately rewrite STEPSTATE accent parameters. All eight exposed methods passed fresh hardware read-back on CorOS 4.0.1 after muting first, and recall restored the complete tempo-program baseline.

---

## [DES-CLIENT-STOMP-MIDI] Stomp, Expression, and MIDI

PROT-006.9 keeps STOMP and expression construction in the pure grid builder layer. Assignment deliberately exposes separate DELETE and UPDATE builders, while `QuadCortex::set_stomp_assignment` is the only convenience sequence and always sends DELETE first. Momentary state and the general/single label maps each travel alone in a sparse preset. Expression parameter assignment is row/column keyed and stores pedal plus normalized endpoints on one indexed `Param`; expression bypass stores both `bypass_expression` and `expression_bypass_info` on one keyed model.

MIDI output deliberately does not use the grid builder layer. A pure `MIDISettingsMessage` constructor creates exactly one nested source group for either `general_midi_messages` or `preset_load_messages`, and the client sends it with message type `MidiSettings`. Checked `MidiOut` constructors encode the type-dependent three-parameter layouts and reject invalid channels, 7-bit values, and groups above 12 messages before I/O. Fake-link tests distinguish type 8 from `Grid` and inspect the nested source, while pure grid tests preserve proto3-default zero keys. Hardware verification on CorOS 4.0.1 passed reversible STOMP/expression working-copy read-back and an isolated-output MIDI persistence flow in generated temporary storage. The latter verified every message family through the helpers and raw 10x12 layout after save/recall, then removed generated storage and restored the original preset. Recall can emit preset-load MIDI, so outputs remain disconnected during this test.

---

## [DES-CLIENT-FILE] File Ops

File operations are eventually consistent. Save and preset delete require exact action/target acknowledgements. Concurrent different-target list/save/delete waiters are safe because action and exact target remain part of every predicate, but an earlier delayed acknowledgement for an identical operation is wire-identical to a current one: preset File broadcasts carry no usable request id. Move polls fresh complete listings for source-absent/destination-present convergence because prior-art hardware evidence shows a file mutation may land without replying. Setlist create/delete similarly use fresh directory listings: create starts from a collision-free baseline and claims only one newly appearing direct USER child; delete requires repeated absence. A materially changed complete listing advances `storage_revision` and stales prepared-save epochs even without an acknowledgement. The final listing-to-write interval remains irreducible because File writes carry no storage revision or compare-and-swap precondition. A missed deadline is an explicitly unconfirmed outcome, never permission to retry blindly.

Implemented addressing rules:
- `save_current_preset`: destination by linear slot `index` in `folder.files[]`.
- `delete_preset`: source by file PATH (`<setlist>/<name>.pb`) in `folder.files[].key`.
- `move_preset`: same-setlist source by listed file PATH in `folder.files[].key`, destination by linear slot `index` in `to_folder.files[]`. The host authorises the exact named source and destination slots, then a fresh complete listing resolves the source path and refuses observed occupancy, empty sources, no-op moves, and factory content.
- `create_setlist`/`delete_setlist`: one validated direct child of `USER_SETLIST_ROOT`; root, factory paths, dot/path components and deletion of `My Presets` never reach I/O.
- `copy_preset`: prepare/backup destination first, recall source second, save with typed `Instrument` third, then read actual stored metadata from a fresh complete listing.
- `duplicate_setlist`: create plus one copy composition per occupied source slot. `BulkOperation` is never faked; a failure returns the created destination and verified partial progress.

Message shapes, safe-component/path policy, copy ordering over a fake link, typed tags, concurrent public-flow waiter interleaving, malformed complete-listing rejection, daemon protocol round trips and spawned older/newer skew with shutdown, CLI dry-run coverage, partial duplicate reporting, and MCP absence are tested offline. The indistinguishability of repeated identical acknowledgements and the absence of a write precondition are pinned as protocol limits rather than claimed as solved. No `File SWAP` operation is implemented or inferred. Save/delete/move and generated-setlist create/copy/duplicate/delete composition are hardware-verified on CorOS 4.0.1. A healthy `Incomplete` cache is repaired by a side-effect-free live read before copy preparation; `Invalidated` remains fail-closed.

---

## [DES-CLIENT-CAPTURE] Captures and IRs

Read-only capture and IR listings are implemented as category-specific, request-id and folder-correlated File reads returning minimal `LibraryEntry` values. Their variable-length replies have no completeness marker, so one correlated reply is an honest response boundary rather than proof of library-wide completeness.

PROT-006.11 selection validates entries and the keyed grid cell before I/O. `set_capture` either verifies placement of default model 14000 or reads and requires an existing compatible 14000/14001 block. It then sends the reserved index-5 selector as exact key plus name with no separator, followed by each caller `ParameterWrite` in slice order; malformed values and caller-owned index 5 are refused before placement, and placement failure short-circuits the sequence. `set_ir` applies the same placement/existing-block discipline to models 29001-29008, validates slot 0/1, and emits path-index then name-index as separate writes using `(2,22)` or `(10,23)`. It rejects filesystem-looking keys rather than pretending IR PATH takes a path.

The public client sends no Neural Capture dialog response. A 2026-08-12 CorOS 4.0.1 probe sent the exact false-only `NeuralCapture{UPDATE, show_dialog: false}` after a decoded `try_to_show_dialog: true`; the unit froze, stopped responding on HID, and rebooted. Causation is not established, but the response path and its tests were removed rather than treating false as graceful. Positive acceptance remains unavailable to CLI, MCP and GUI until a complete v1/v2 host capture UI exists.

Offline fake-link tests pin exact shapes, order, validation-before-I/O, existing-block reuse, the placement-refusal short circuit, no unsolicited capture-dialog response, unrelated-message rejection, false-only response shape and a healthy follow-up read. Hardware tests selected one existing library capture and one existing user IR without publishing identities, fresh-read the exact strings, verified a post-capture VOLUME write, never saved, and unconditionally recalled before disconnect. A timed on-unit inspection confirmed no warning for the selected user IR. The ignored decline test waits up to 90 seconds for the operator's on-unit action, observes NeuralCapture traffic for at least 10 seconds after false, rejects capture state/progress/A-B preparation, performs an active-scene positive control, and disconnects. Export/import remains separate under PROT-007; positive host capture is separate planned host-workflow scope. PROT-006.11 remains partial until the decline test passes on hardware.

---

## [DES-CLIENT-GLOBAL] Global Settings

PROT-006.12 deliberately does not expose a generated `GeneralSettingsMessage` write. `GeneralSettingsPatch` names only scalar settings with positive writable evidence and structurally omits power, reset, updater, reboot/shutdown, factory-reset and unsupported internal-MIDI-clock controls. HOLD timing and scene bypass use checked domain values rather than raw wire integers. Empty or invalid patches fail before session I/O.

Master Volume assignment and Cab/IR global bypass are nested replacement groups, not sparse objects. Their patch methods trigger an explicit complete settings READ, merge the caller's partial intent, then send every sibling. Separate restore methods accept complete typed state and never reconstruct missing siblings from a possibly stale subscribed push.

Global EQ is sparse by index: five controls per band at offsets Gain 0, Frequency 1, Q 2, Type 3 and Enabled 4, followed by OUT level 25 and output assignments 26-27. Each selected control travels in its own one-parameter UPDATE. Numeric controls remain normalized 0-1; OUT level has no invented dB conversion. Mode slots are a closed 0-8 type so broken value 9 cannot reach the wire; cycle validation permits at most one hybrid and refuses a hybrid-only cycle. Tuner input similarly exposes only the seven values upstream hardware accepted, and reference is checked against the unit's 425-455 Hz range as an offset from 440.

Complete state methods issue explicit READs and require restoration-grade content. Mode is the exception that proves the rule: because its pushes are partial, `mode_complete` performs separate active-slot and cycle reads and merges them. The CorOS 4.0.1 hardware smoke snapshot complete General Settings, Mode, Tuner and Global EQ state, changed one field at a time, polled explicit reads, restored immediately, then independently restored and compared all four snapshots before disconnect. It clears only action/request correlation and storage-capacity fields documented as volatile for its final General Settings comparison. Gig View and Tuner visibility still require visual confirmation because no complete readable baseline for those screen-only states is established; neither UI method is claimed verified.

---

## [DES-CLIENT-IO] I/O Settings

PROT-006.13 treats the upstream one-field rule as an invariant across the entire writable I/O surface rather than a special case for the three fields already observed failing. `InputPortPatch`, `OutputPortPatch`, `UsbPortPatch` and `OutputPairingPatch` are validated completely before dispatch; builders then return one `IOSettings{UPDATE}` per selected field in stable order. Every input/output message repeats its typed key. MIDI Thru and each top-level pairing flag also travel alone. This costs extra writes but prevents a later invalid patch field from leaving an earlier sibling applied and avoids relying on unmeasured safe packing combinations.

`InputPort` preserves ids 1-6 including combined entries, with Return 1 at 4. `OutputPort` preserves the known physical paired/individual XLR, Out and Send ids 1-9. Normalized controls remain finite 0-1 values; the implementation does not invent names or cardinalities for impedance, type, USB headphone-source or dry/wet list values that the evidence does not establish.

Setters are dispatch-only because the benign write STALL and eventual consistency provide no write acknowledgement. `io_settings_complete` explicitly reads restoration-grade state using the CorOS 4.0.1 hardware-measured capability matrix: inputs 1/2 carry all four writable fields while 4/5 omit impedance and type; outputs 1/4/5 carry level/ground/mute, 2/6/7 level/mute, and 8/9 level only. It requires the exact identity set, USB, MIDI and both pairings while excluding `plugged`, headphones and expression-pedal telemetry. `poll_io_settings` composes repeated fresh complete reads with a caller predicate. Hardware verification retained optional per-port fields, sent and compared only baseline-present controls, used valid discrete-selector values, exercised pairings last, restored each member using its actual capabilities, and independently matched the final complete baseline. The test passed with external outputs disconnected.

---

## [DES-CLIENT-CATALOG] Catalog

`fetch_model_repo()` returns the payload captured by the paced handshake, avoiding a second 46 KB transfer; without a current captured copy it performs `ModelRepo` READ + `await_broadcast`. `Catalog::parse` turns integer model ids into names, categories, and parameter lists in wire-index order. It covers installed block types, including purchased plugins. Neural Capture models are capture block types; individual capture inventory comes from separate `File` listings. The persistent daemon parses its captured copy once per exact generation/revision and evicts the parsed form when the reducer invalidates the payload.

Name resolution belongs to `QuadCortex::set_parameter`, not a host surface. It reads the live cell to discover the model, resolves the named parameter through the catalog, and converts a real-unit value through that parameter's declared range. The CLI, daemon, MCP server and GUI therefore share one implementation and never ask callers to repeat a model id already present on the grid.

List option resolution belongs to `QuadCortex::set_param_option`. Its caller supplies the current preset because dynamic option names can enumerate blocks and therefore cannot come from static catalog metadata. The target may be an index or catalog name; the client obtains the source block's model when needed, then delegates dynamic-list lookup and `index / (count - 1)` normalization to zone 130 helpers before issuing the ordinary parameter write.

---

## [DES-CLIENT-DEC] Key Decisions

| Decision                                         | Choice                                                       | Rationale                                                                                                                                                            |
| ------------------------------------------------ | ------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Arc<Session>` not `&mut Session`               | Shared ownership                                             | The MCP server holds one session across many tool calls; the CLI holds one for the process lifetime. `Arc` lets both without a borrowdance.                        |
| Keep orchestration together; extract pure boundaries | `client.rs` plus `grid.rs`, `view.rs`, `catalog.rs`, `safety.rs` | Splitting stateful orchestration would expose private session concerns; pure reusable logic earns separate modules. |
| Value objects are pure                          | Serialisable values with no I/O                 | Client request/result values and zone-130 host views cross CLI, MCP, Tauri and daemon boundaries without carrying session state.                                          |
| `Session::Drop` calls `close()`                 | Final implicit teardown                                     | The physical owner releases the device; dropping one shared `QuadCortex` wrapper does not disconnect sibling callers.                                                |
| Domain traps in rustdoc AND MCP descriptions     | Double documentation                                         | The MCP server exposes these methods to agents; the traps must be in the tool description so an agent does not silently edit the wrong row.                         |
| Name resolution via catalog, not hardcoded indices | `param="THRESHOLD"` -> catalog -> wire index              | Indices are positional and not every one is a visible knob; naming is the safer route. Resolution uses only the current generation/revision's installed block catalog. |

---

## [DES-CLIENT-TEST] Testing Notes

Offline tests cover strict listing and file-acknowledgement predicates, malformed 256-entry listing shapes, concurrent public list/save/delete interleaving with wrong targets and controlled completion order, the indistinguishability of identical delayed acknowledgements, the absence of a File compare-and-swap guard, request-id and folder-correlated variable listings including empty replies, Favorites retry and empty-list correlation, partial state-push rejection, distinct mode/mode-cycle field presence, slot conversion, dB conversion, catalog-typed parameter resolution, folder/listing conversion, recall encoding, block-echo matching, factory-save refusal, and malformed inputs. Pure keyed grid builders have their own tests in `grid.rs`, including mutation checks for DELETE-vs-UPDATE, scene-mode isolation, and keyed rows.

The manual hardware runbook covers the implemented read, recall, scene, grid-write, tempo, STOMP/expression/MIDI, file, capture/IR selection, global-setting, I/O, pin/Favorite and daemon-lifecycle paths. Unimplemented operations and the explicitly residual UI methods are not presented as verified; each newly implemented wire operation remains provisional until added to that smoke.

---

## [DES-CLIENT-PROVENANCE] Provenance & Attribution

The implemented `QuadCortex` operations are derived from the corresponding parts of `pyquadcortex/pyquadcortex/client.py` (MIT, (c) 2026 Stokes), then adapted and verified per operation. The `ModelCatalog` parser is ported from `pyquadcortex/pyquadcortex/catalog.py`. See `THIRD-PARTY-NOTICES.md` for the MIT attribution.

No code is copied from the reference-only repositories without a clear repository-wide licence. Their findings are re-expressed in this project's own words.
## [DES-CLIENT-DIVERGENCE] Divergences from the original plan

### One `client.rs`, not a `client/` module tree

Same rationale as [140-session/design.md](../140-session/design.md): at this size the split would fragment more than it clarifies. Grid-edit message construction IS a separate module (`grid.rs`), because those builders are pure and benefit from being testable in isolation from the client that sends them.

### What is covered without hardware

Slot-name round-tripping and rejection of malformed names, dB conversion and its range guard, empty-slot detection in listings, trailing-slash key normalisation, folder occupancy counting, recall-payload encoding, the `Grid` echo matcher including its positional-fallback case, and `preset_has_block`.

The grid builders in `grid.rs` are covered separately and more heavily, since that is where the silent-no-op traps live: 18 tests, three of which were verified by mutation - reintroducing UPDATE-instead-of-DELETE, a value beside the `scene_mode` flag, and an unkeyed chain each made exactly the test that claims to guard it fail.
