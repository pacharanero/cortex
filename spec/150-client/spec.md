---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["client", "api", "quad-cortex", "presets", "grid", "scenes", "provisional"]
---

# 150 Client - Spec

> The ergonomic `QuadCortex` client API: the Rust equivalent of pyquadcortex's `QuadCortex` class. This is the layer the CLI, MCP server, and Tauri backend all call.

## References

- **Session (lower layer)**: [`../140-session/spec.md`](../140-session/spec.md) - `request`, `await_broadcast`, `collect`, `next_request_id` primitives this client builds on
- **Transport (lower layer)**: [`../100-transport/spec.md`](../100-transport/spec.md) - the send path (fire-and-forget)
- **Domain model**: [`../130-domain-model/spec.md`](../130-domain-model/spec.md) - `BinaryPreset`, `Block`, `Split`, `Folder` types
- **Protobuf schema**: [`../120-proto-schema/spec.md`](../120-proto-schema/spec.md) - generated `ProductionAutomation` types
- **Public protocol reference**: [`../../docs/protocol.md`](../../docs/protocol.md) - authoritative protocol facts
- **Prior art (MIT, ported)**: `pyquadcortex/pyquadcortex/client.py` - the `QuadCortex` class this is a port of; `pyquadcortex/pyquadcortex/catalog.py` - the model catalog parser

---

## Problem Statement

The session layer (zone 140) provides correlated request/response and broadcast-wait primitives. The CLI, MCP server, and Tauri backend all need a higher surface: methods named after the things a player does - `recall_preset`, `switch_scene`, `set_param`, `set_block`, `save_current_preset` - not raw protobuf construction. This zone owns that ergonomic API, ported from pyquadcortex's `QuadCortex` class (~60 methods), adapted to Rust idioms.

This zone knows NOTHING about hidapi, HID reports, framing, or the session state machine. It holds a `Session` reference and builds protobuf messages, handing them to the session's `send`/`request`/`await_broadcast`/`collect` primitives. That keeps this layer testable with a fake session and keeps all wire concerns below it.

The protocol facts this zone encodes are hardware-verified via `pyquadcortex` against CorOS 4.0.1. The Rust implementation is provisional until exercised against a real Quad Cortex from this crate.

---

## Requirements

### Functional Requirements

#### Lifecycle

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-1  | `QuadCortex::new(session)` constructs the client around a `Session`. | Must Have   |
| FR-2  | `connect(timeout, settle)` is a convenience that opens the transport, starts the session, runs the handshake, and returns a `QuadCortex`. The Rust equivalent of `pyquadcortex.connect()`. | Must Have   |
| FR-3  | `disconnect()` sends `Connection{connected: false}` (delegates to `Session::disconnect`). | Must Have   |
| FR-4  | `close()` tears down in reverse order: disconnect, stop session, close transport. Safe to call more than once. | Must Have   |
| FR-5  | `version(timeout)` reads the device's version info (`VersionMessage`: `app_fw_version`, `device_type`, `device_serial_number`, `comms_version`). Works without the full handshake. | Must Have   |
| FR-6  | `Drop` implementation calls `close()` so a client dropped without explicit close still releases the device. | Should Have |

#### Catalog

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-7  | `fetch_model_repo(timeout)` returns the catalog payload captured by the handshake, or performs `ModelRepo` READ + `await_broadcast` when no captured copy exists. `Catalog::parse` turns that device-specific payload into model and parameter metadata. | Must Have   |

#### Read operations

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-8  | `find_preset(name, setlist, timeout)` looks up a preset by display name (exact, case-insensitive). Returns the listing entry whose `index` is the slot position. | Must Have   |
| FR-9  | `read_preset(setlist_path, position, is_factory, timeout)` recalls a preset and returns its full `BinaryPreset`. NOTE: this RECALLS the slot (side effect - it loads the preset onto the grid). Tags the recall with a fresh `request_id` and accepts only the `RecallPreset` push echoing it (skips the seed push). | Must Have   |
| FR-10 | `read_current_preset(timeout)` returns the LIVE grid (`RecallPreset{READ}` with a `request_id`, matched on the echo). No side effects - unsaved edits survive; the active scene is untouched. | Must Have   |
| FR-11 | `list_presets(setlist, timeout, include_empty)` lists presets in slot order. Sends a `File` READ and waits for the matching folder listing. Trailing-slash normalization on the key. | Must Have   |
| FR-12 | `list_folders(seconds)` enumerates every folder the device knows (uses `collect` for the folder flood). | Should Have |
| FR-13 | `active_scene(timeout)` reads the current scene (`Scene{READ}`, matched on `request_id`). | Must Have   |
| FR-14 | `captures(timeout)` lists every Neural Capture in the library (delegates to `list_presets(CAPTURES_LIBRARY)`). | Should Have |
| FR-15 | `list_irs(folder, timeout)` lists IRs (`File` READ with `type: 1`, matched on `request_id` + key). | Should Have |
| FR-16 | `recents(timeout)` reads the Recents list (`RecentsFavorites{READ}`, non-empty match). | Should Have |
| FR-17 | `favorites(timeout, attempts)` reads the Favorites list (`RecentsFavorites{READ, is_favorites: true}`, matched on `request_id`; retries on timeout). | Should Have |
| FR-18 | `pinned_models(timeout)` reads pinned model ids. | Should Have |
| FR-19 | `master_volume(timeout)` reads the Master Volume state (read-only; no setter - the knob is the only way to move it). | Should Have |
| FR-20 | `looper(timeout)` reads the Looper X state (read-only). | Should Have |
| FR-21 | `tuner(timeout)` reads the tuner state (`input_port_id`, `frequency` as Hz offset from 440, `mute`). | Should Have |
| FR-22 | `io_settings(timeout)` reads input/output/headphone/USB/MIDI/expression port settings. | Must Have   |
| FR-23 | `settings(timeout)` reads global device settings (`GeneralSettings`). | Must Have   |
| FR-24 | `global_eq(timeout)` reads the Global EQ state (bypassed + 5 bands). | Should Have |
| FR-25 | `mode(timeout)` reads the footswitch mode state; `mode_cycle(timeout)` reads the configured slots. | Should Have |

#### Navigation

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-26 | `recall_preset(setlist_path, position, is_factory, request_id)` sends `SetlistPosition{UPDATE}`. Position is linear index or slot name (e.g. `"28C"`). | Must Have   |
| FR-27 | `switch_scene(scene)` sends `Scene{UPDATE, selected_scene}`. Scenes are 0-based. | Must Have   |
| FR-28 | `copy_scene(from_index, to_index, swap)` sends `SceneCopy{UPDATE}` (label and color travel with the copy). | Should Have |
| FR-29 | `set_scene_label(scene_index, label)` sends `SceneLabel{UPDATE}`; `None` sends `SCENE_UNLABELLED` (a single space, not empty string). | Should Have |
| FR-30 | `set_scene_color(scene_index, color)` sends `SceneColor{UPDATE}` with an ARGB uint32. | Should Have |

#### Grid write

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-31 | `write_preset(p)` sends `Grid{UPDATE, preset: p}`. A recalled preset carries NO explicit `row`, so writing it back wholesale does nothing - use the keyed wrappers. | Must Have   |
| FR-32 | `set_chain_input(row, in_portid)` re-points a row's input (row-keyed update). The ONLY shape that actually moves an input. | Must Have   |
| FR-33 | `set_chain_output(row, out_portid)` re-points a row's output (row-keyed update). | Must Have   |
| FR-34 | `set_param(row, column, param_index, value)` is the low-level wire-index write. `set_parameter(row, column, target, input, scene, promote, timeout)` is the shared host-facing API: it validates rows/columns/scenes and normalised values, reads the cell's model when addressed by name, resolves through the device catalog, refuses meters, converts real units, and performs the 3-message scene sequence when requested. | Must Have   |
| FR-35 | `set_param_scene_mode(row, column, param_index, enabled)` sets the scene-following flag (must travel ALONE). | Must Have   |
| FR-36 | `set_bypass(row, column, bypassed, scene)` bypasses/enables a block. With `scene=`: switches scene first (visible side effect). | Must Have   |
| FR-37 | `set_block(row, column, model, verify, timeout)` places a model in a cell. With `verify=true` (default): waits for the `Grid` echo naming the cell; raises `BlockRefused` if no echo (DSP capacity). | Must Have   |
| FR-38 | `remove_block(row, column)` sends `Grid{action: DELETE, ...}` (NOT UPDATE with hash:0, which is ignored). | Must Have   |
| FR-39 | `move_block(from_row, from_col, to_row, to_col, drop)` sends `GridMove`. A cross-row move creates a parallel path. | Should Have |

#### Splitter/mixer/lane/gate

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-40 | `set_splitter_param(row, param, value, real, scene, promote)` writes `chain.combined_splitter` (NOT `chain.splitter`, which is read-only). Row must be 0 or 2. | Should Have |
| FR-41 | `set_mixer_param(row, param, value, real, scene, promote)` writes `chain.mixer[]`. Row must be 0 or 2. | Should Have |
| FR-42 | `set_lane_output(row, param, value, real, scene, promote)` writes `chain.output_control[]` (model 23000). | Should Have |
| FR-43 | `set_input_gate(row, param, value, real, scene, promote)` writes `chain.input_control[]` (model 28000). | Should Have |
| FR-44 | `set_split(row, split_column, mix_column)` sets `split_control_points` (activates a branch). `mix_column=-1` for a non-rejoining branch. Row must be 0 or 2. | Should Have |
| FR-45 | `set_split_mute(row, muted)` writes `chain.splitBypass` (sets all 8 scenes at once). | Should Have |

#### Tempo

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-46 | `set_tempo_param(param, value, real)` writes a per-preset tempo parameter (`tempoProgramData`, model 25000). Name resolution via `TEMPO_PARAMS` map first (two names disagree with the catalog). | Should Have |
| FR-47 | `set_tempo_option(param, option)` sets a list-valued tempo parameter by option number (range-checked). | Should Have |
| FR-48 | `set_tempo_subdivision`, `set_metronome_sound`, `set_metronome_routing`, `set_time_signature`, `set_tempo_led`, `set_metronome_volume` - typed convenience wrappers. | Should Have |

#### Stomp/expression/MIDI

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-49 | `set_stomp_assignment(row, column, footswitch)` assigns a block to a STOMP footswitch (DELETE existing then UPDATE - two messages). | Should Have |
| FR-50 | `set_expression(row, column, param, pedal, minimum, maximum, model)` assigns an expression pedal to a parameter. | Should Have |
| FR-51 | `set_midi_out(source, messages)` sets MIDI messages via `MIDISettings` (NOT `Grid` - a Grid update carrying `midi_messages_general_v2` is ignored). | Should Have |

#### File ops

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-52 | `save_current_preset(setlist_path, position, name, instrument, default_scene, confirm)` sends `File{CREATE, type: 0}`. The device saves the grid it already has; it may de-duplicate the name. With `confirm`: re-lists to get the stored name. | Must Have   |
| FR-53 | `delete_preset(setlist_path, name)` sends `File{DELETE}` addressed by file path (`<setlist>/<name>.pb`), NOT slot index. | Must Have   |
| FR-54 | `move_preset(setlist_path, name, to_position)` sends `File{MOVE}` (source by file path, destination by linear index). | Should Have |
| FR-55 | `create_setlist(name)` sends `File{CREATE, type: 0, folder{key: USER_SETLIST_ROOT/<name>}}`. | Should Have |
| FR-56 | `delete_setlist(name)` sends `File{DELETE}`. | Should Have |
| FR-57 | `copy_preset(from_setlist, position, to_setlist, to_position, name)` - a composition: recalls the source, saves the grid into the destination. Changes what is loaded on the unit. | Should Have |
| FR-58 | `duplicate_setlist(source_name, dest_name, limit)` - a composition: creates the destination, copies each preset via `copy_preset`. Slow (recall + save per preset). | Should Have |
| FR-59 | `wait_for_listing(setlist, until, timeout, interval)` polls `list_presets` until a condition holds (file ops are eventually consistent). Rides out missed pushes. | Should Have |

#### Captures/IRs

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-60 | `set_capture(row, column, capture, model, params)` points a Neural Capture block at a library entry. `file_name` param = `<64-char hash><display name>` (concatenated). Loading a capture RESETS the block's other parameters - write params AFTER. | Should Have |
| FR-61 | `set_ir(row, column, ir, slot, model)` points an IR Loader block at an IR. Two strings: IR PATH (param 2/10) = the key; IR NAME (param 22/23) = the name. Every IR Loader has TWO slots. | Should Have |

#### Global settings

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-62 | `update_settings(**fields)` changes global settings sparsely. Refuses `power_option` and `reset_wifi_networks` (device commands). Brightness is quantized; `hold_timing` is an index (use `set_hold_timing` for ms). | Must Have   |
| FR-63 | `set_global_eq(band, gain, frequency, q, filter_type, enabled)` sets one Global EQ band's controls (5 params per band, sparse by index). | Should Have |
| FR-64 | `set_mode_cycle(slots)` sets the footswitch mode cycle. At most one HYBRID slot; refuses the broken value 9. | Should Have |
| FR-65 | `set_gig_view(shown)` opens/closes Gig View. `show_tuner(shown)` opens/closes the Tuner. | Should Have |

#### I/O ports

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-66 | `set_input_port(input_port_id, level, impedance, input_type, ground_lift)` - each field sent in its OWN message (the device drops fields that share a port entry). `input_port_id` takes the `Input` enum, NOT 1/2/3/4 (Return 1 is 4). | Must Have   |
| FR-67 | `set_output_port(output_port_id, level, ground_lift, mute)` - one field per message (mute is dropped when paired). | Must Have   |
| FR-68 | `set_usb_port(level, hp_select, dry_wet)` - one field per message. | Should Have |
| FR-69 | `set_midi_thru(enabled)` toggles MIDI Thru. | Should Have |
| FR-70 | `set_output_pairing(xlr1_2, out3_4)` pairs/unpairs output couples. | Should Have |

#### Helper functions (module-level)

| ID    | Requirement                                                                                                       | Priority    |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-71 | `blocks(p) -> Vec<Block>` returns the OCCUPIED grid cells (every row reports 8 column slots, empty ones have hash absent/zero; `len(chain.models)` is always 8). | Must Have   |
| FR-72 | `splits(p) -> Vec<Split>` returns where each row branches (`split_control_points`; `split >= 0` means a branch; `mix == -1` means non-rejoining). | Should Have |
| FR-73 | `slot_to_position(slot) -> u32` converts a slot name (e.g. `"28C"`) to a linear index (`(28-1)*8 + 2 == 218`). | Must Have   |
| FR-74 | `position_to_slot(index) -> String` is the inverse. | Should Have |
| FR-75 | `input_level_db(level) -> f64` converts a wire `level` (0..1) to dB (`-12 + 72 * level`; input ports span -12..+60 dB). | Should Have |
| FR-76 | `db_to_input_level(db) -> f64` is the inverse; refuses values outside -12..+60 dB. | Should Have |
| FR-77 | `field_present(message, field) -> bool` checks proto3 field presence without raising on fields without presence (e.g. `SceneBypass.bypass`). | Must Have   |

### Non-Functional Requirements

| ID    | Requirement                                                                                                                                                                                                                              | Target                  |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| NFR-1 | The client adds no async runtime dependency; it uses the session's blocking primitives directly. The leaf-crate discipline is preserved. | Architectural invariant |
| NFR-2 | Domain types (`Block`, `Split`, `Folder`, `MidiOut`) are pure value objects: `Clone`, `Serialize`, `Debug`, no I/O. | Code invariant          |
| NFR-3 | The catalog is fetched once and cached for the session; no method re-fetches it implicitly. | Code invariant          |
| NFR-4 | Unit tests for the helper functions (`blocks`, `splits`, `slot_to_position`, `input_level_db`, `field_present`) and the message-building logic run in CI without hardware, using fixture presets. | CI-enforced             |
| NFR-5 | The ~60-method API surface mirrors pyquadcortex's method names (snake_case) so a porting caller recognises the surface; deviations are documented. | Architectural invariant |
| NFR-6 | Every domain trap (see Domain Traps below) is documented in the method's rustdoc and surfaced in the MCP tool descriptions where relevant. | Code invariant          |

---

## Acceptance Criteria

- [ ] `QuadCortex::connect()` returns a ready client; `version()` succeeds without the full handshake.
- [ ] `read_preset()` recalls a slot and returns the `BinaryPreset`; the recall's `request_id` is echoed on the push.
- [ ] `read_current_preset()` returns the live grid with no side effects.
- [ ] `list_presets()` returns occupied slots in order; trailing-slash keys normalized.
- [ ] `switch_scene()` / `set_param()` / `set_bypass()` / `set_block()` persist (survive a save and read-back).
- [ ] `set_param(scene=D)` issues 3 messages (promote, switch, write) and lands on scene D.
- [ ] `set_block(verify=true)` raises `BlockRefused` when DSP capacity is exhausted (no echo).
- [ ] `remove_block()` uses action DELETE (not UPDATE with hash:0).
- [ ] `set_splitter_param()` writes `combined_splitter` (not `splitter[]`).
- [ ] `save_current_preset()` writes to the slot; `confirm=true` returns the device-stored (possibly de-duplicated) name.
- [ ] `delete_preset()` addresses by file path, not slot index.
- [ ] `set_capture()` writes `file_name` = `<hash><name>`; params written after survive.
- [ ] `set_ir()` writes the key to IR PATH and the name to IR NAME (two strings, not a path).
- [ ] `set_input_port()` sends one field per message; uses `Input` enum ids (Return 1 is 4).
- [ ] Helper functions: `blocks()`, `splits()`, `slot_to_position()`, `input_level_db()`, `field_present()` pass unit tests on fixture presets.
- [ ] Full method surface verified against a real Quad Cortex (CorOS 4.0.1) - hardware-only.

---

## Non-Goals

- The session layer (zone 140): connect handshake, keepalive, correlation. This zone consumes `Session`.
- USB HID transport (zone 100), framing (zone 110), protobuf schema (zone 120).
- The typed domain model (zone 130): `BinaryPreset`, `Block`, `Split` definitions. This zone imports them.
- MCP/GUI host integration (zones 300/400): token registries, scratch-range configuration, confirmation UI, and safety-event logging. The shared policy and prepared-save implementation live in `cortex-rs::safety`; hosts must use it rather than wrapping `save_current_preset` independently.
- CLI surface (zone 200): `clap` commands, output formatting. This zone is the engine the CLI calls.
- Tauri backend (zone 400): Tauri commands. This zone is the engine the backend calls.

---

## Dependencies

- **Crate-internal**: zone 140 (`Session`), zone 130 (`BinaryPreset`, `Block`, `Split`, `Folder`, `MidiOut`), zone 120 (generated proto types), zone 110 (framing constants).
- **External (leaf)**: `serde` (for the value objects), `uuid` (for the session_id in the handshake, if owned here), the generated `prost` types. No async runtime, no `clap`, no `tauri`.
- **Prior art**: `pyquadcortex/pyquadcortex/client.py` (`QuadCortex` class, ~60 methods) and `pyquadcortex/pyquadcortex/catalog.py` (`ModelCatalog` parser) - ported under MIT with attribution; see `THIRD-PARTY-NOTICES.md`.

---

## Domain Traps (must be documented in rustdoc and MCP descriptions)

These are confirmed on hardware and are the primary source of silent wrong-row / silent-no-op behaviour. Every one must appear in the relevant method's rustdoc and, where the MCP server exposes the method, in the tool description.

1. **Rows are 0-based in the API, 1-4 on screen.** `row=0` is the top row on screen; `row=2` is labelled 3. Getting this wrong is QUIET: an edit lands on a real row, just not the one intended, and reads back perfectly. Check `chain.out_portid`: values 16-18 are internal row-to-row routing, not jacks (19/MULTIPLE is a real destination).

2. **A recalled preset carries no explicit `row`.** Writing it back wholesale via `write_preset()` does nothing - a full-preset write that re-pointed `in_portid` read back UNCHANGED. Use the keyed wrappers (`set_chain_input`, `set_param`, `set_bypass`) instead.

3. **`read_preset` RECALLS the slot (side effect).** It loads the preset onto the grid, discarding unsaved edits and resetting the active scene. `read_current_preset` does NOT - use it for inspection during editing.

4. **`set_param(scene=)` is 3 messages.** The flag (`scene_mode`) and a value CANNOT travel in the same message - sent together, the flag is silently dropped. So: promote scene_mode, switch scene, then write. Ordering over the pipe is enough; no settle delay needed. Naming a scene leaves the unit sitting on it (visible side effect).

5. **`set_block` can be refused for DSP capacity.** The preset has a processing budget; a block that does not fit is accepted on the wire and simply absent afterwards. No per-block error message. `verify=true` (default) catches this by waiting for the `Grid` echo; no echo within timeout = `BlockRefused`.

6. **`remove_block` uses action DELETE.** An UPDATE carrying `hash: 0` is transmitted and ignored. The action is what marks the removal.

7. **`set_splitter_param` writes `combined_splitter`, not `splitter[]`.** `splitter[]` is read-only; a write there is silently ignored. Always go through the method.

8. **Splitters/mixers exist only on rows 0 and 2.** An odd row raises `ValueError` (in Rust: returns `Error`) rather than sending a write into a collection the device does not have there.

9. **`set_midi_out` goes through `MIDISettings`, not `Grid`.** A `Grid` update carrying `midi_messages_general_v2` is accepted and ignored.

10. **`save_current_preset` may rename.** The device de-duplicates on collision (truncates + `_N` suffix). Read back to confirm the stored name if it matters.

11. **`delete_preset` addresses by file path, not slot index.** `File{DELETE, files{key: "<setlist>/<name>.pb"}}`.

12. **`set_input_port` takes the `Input` enum, not 1/2/3/4.** Return 1 is 4, Return 2 is 5 (combined ids are interleaved). Passing 3 for "Return 1" writes the combined Input 1/2 entry - an easy, expensive mistake.

13. **Each I/O port field is sent in its own message.** The device drops fields that share a port entry (mute + ground_lift on an output both fail when paired, both work alone). One field per message is the guarantee.

14. **`read_preset` interleaved with scene-targeted writes silently retargets them.** A `read_preset` resets the active scene to the preset's default. A `set_bypass(scene=)` issued after a `read_preset` lands on the default scene, not the one you switched to. Inspect with `read_current_preset`, check `active_scene`.

15. **Loading a capture RESETS the block's other parameters.** Write parameters AFTER `set_capture`, or pass them via the `params` argument to be applied once the capture is in.

---

## Appendix

### Protocol Provenance & Attribution

The `QuadCortex` client API is a port of `pyquadcortex/pyquadcortex/client.py` (MIT, (c) 2026 Stokes). The ~60 methods, the helper functions (`blocks`, `splits`, `slot_to_position`, `input_level_db`, `field_present`), and the domain-trap documentation all originate there, confirmed against real hardware (CorOS 4.0.1). The `ModelCatalog` parser is ported from `pyquadcortex/pyquadcortex/catalog.py`. Record any derivation in `NOTICE` / `THIRD-PARTY-NOTICES.md`.

The constants (`UNITY_LEVEL`, `USER_SETLIST_ROOT`, `SCENE_UNLABELLED`, `TEMPO_PARAMS`, `GLOBAL_EQ_BAND_STRIDE`, `CAPTURE_FILE_NAME_PARAM`, etc.) are wire values measured on the device, not invented.

### Provisional labelling

The protocol facts are hardware-verified via `pyquadcortex`. The Rust implementation is **provisional** until each method has been exercised against a real Quad Cortex from this crate's own code. Label the client as "provisional" in docs and release notes until the hardware smoke run passes.
### Hardware findings (2026-08-02, CorOS 4.0.1 / firmware d14e / QA00AB123)

First verification of the read paths against a real Quad Cortex, via `cortex device probe`, `cortex setlist list`, and `cortex preset list`.

**Verified working:** `active_scene`, `read_current_preset`, `list_presets`, `list_folders`. The connect handshake completed in 2.2 s and state pushes flowed afterwards, confirming the device does gate pushes on the handshake ([FR-2] of zone 140).

**`list_folders` returned 399 folders** - the same count `pyquadcortex` reports for its unit. `/media/p4/Presets/My Presets` reported 11 occupied of 256, agreeing exactly with `list_presets` on the same key, and `/opt/neuraldsp/Factory Library` reported 256 of 256 with `is_factory` set.

**Plugin artist folders report slots but no names, and this is device state rather than a decode bug.** Folders such as `/opt/neuraldsp/Plugins/Archetype: Cory Wong X/Artists/Jack Gardiner` announce a non-zero slot count (19) but zero occupied, and `list_presets` against that key returns a listing whose entries carry no `name`. The same code path against `/opt/neuraldsp/Factory Library` returns all 256 names correctly, which isolates the cause to the device: the plugin's folder structure ships with CorOS, but the preset files are absent on this unit.

Consequences for callers:

- `Folder::occupied` counts NAMED entries. A `0/N` folder means N declared slots with nothing loadable in them, not a parse failure.
- `list_presets` returning an empty `Vec` with `Ok` is a legitimate answer meaning "the listing arrived and held nothing named". It is distinct from `Err(ReadTimeout)`, which means "no listing arrived - ask again". Do not collapse the two.

#### Navigation and stored-preset reads (same session, 2026-08-02)

A fully reversible sequence exercised the remaining implemented paths, restoring the unit to its starting state (`1A`, scene 0):

| Step | Action | Observed |
| --- | --- | --- |
| 1 | `recall_preset("1B")` | grid became `Tweed Porchlight`, scene unchanged at 0 |
| 2 | `switch_scene(1)` | `active_scene` became 1, grid unchanged |
| 3 | `read_preset("1A")` | returned `Plexi Sunrise`, 4 chains, rows of 5/0/4/3 blocks |
| 4 | (no action) | `active_scene` had returned to 0 **by itself** |

**`read_preset` correlation works.** Step 3 is the hardest correlation case in the crate: the recall is tagged with a fresh `request_id` and only the `RecallPreset` push echoing that id is accepted, so the unsolicited seed push from the handshake's subscription is skipped. It returned the correct preset first time.

**Every grid row reports 8 column slots.** Row 1 of `Plexi Sunrise` holds no blocks yet still reports 8 `models` entries, confirming that occupancy must be derived from a present, non-zero `hash` rather than from `models.len()`.

**Trap 14 confirmed on hardware, unprompted.** Between steps 3 and 4 nothing switched the scene, yet it moved from 1 back to 0: `read_preset`'s recall reset the active scene to the preset's default. This is exactly the documented silent-retarget hazard - a scene-targeted write issued after a `read_preset` lands on the default scene rather than the one the caller selected. Observed here as a side effect of a read, which is what makes it easy to miss.

#### VersionMessage field names are the vendor's, and two are inaccurate (2026-08-02)

Reported as a suspected transposition on our side. It is not: verified by decoding the raw protobuf field numbers off the wire rather than trusting the recovered schema's names.

| Wire field | Schema name | Actual content |
| --- | --- | --- |
| 4 | `zenos_git_hash` | `4.0.1` - the CorOS **version**, not a hash |
| 5 | `zenwireless_fw_version` | `0123456789abcdef0123456789abcdef` - a 32-hex **checksum** |

Field 5's value is 32 hex characters, which is MD5 length; a git SHA-1 would be 40. So neither field holds a git hash, and the two are not swapped - each simply carries something its name does not describe. `pyquadcortex` renders them identically, which is independent agreement rather than a shared bug, since both read the same field numbers from the same recovered schema.

The names are Neural DSP's own and are presumably historical. We keep them so output maps to the schema, but annotate them in the CLI (`zenos_git_hash (= CorOS version)`) rather than silently passing on a misleading label. Do not "fix" this by swapping the fields.

#### Grid editing verified on hardware (2026-08-02, CorOS 4.0.1 / d14e / QA00AB123)

The first destructive surface, exercised end to end. Safe by construction at this stage: no save is implemented, so every grid edit is transient and a recall discards it. The unit was restored to `1A` unchanged afterwards.

| Step | Action | Verified by |
| --- | --- | --- |
| 1 | `set_block(screen row 2, col 0, model 1)` | the device's `Grid` echo, AND an independent `read_current_preset` showing `Myth Drive` in that cell |
| 2 | `set_param(--param GAIN --value 0.9)` | read-back showing `GAIN 0.9` |
| 3 | `remove_block(screen row 2, col 0)` | read-back showing the row empty |

**Echo verification works.** `set_block` reported "echo confirmed" and the read-back agreed, so `grid_echoes_cell` is neither over- nor under-matching on real traffic. A DSP refusal has not yet been provoked, so the negative path - no echo within the timeout - remains unverified against hardware; only the positive path is confirmed.

**The screen-row convention holds end to end.** `--row 2` landed on wire row 1, confirmed by a read-back that reports both numbers.

**Catalog-driven parameter naming works.** `--param GAIN` resolved to wire index 0 by reading which model occupied the cell and looking it up. An unrecognised name lists the model's real parameters (`GAIN, TREBLE, LEVEL`), which is materially better than a failed write.

**Normalisation agrees with the catalog.** The untouched `TREBLE` and `LEVEL` read back as `0.5`, matching the catalog's default of 5 on a 0-10 range.

**A stored preset can carry MORE parameters than the catalog describes.** Myth Drive's catalog entry declares three parameters; the stored block carried four, the last unnamed. This is the same phenomenon already recorded for the tempo block (23 described, 24 stored). Consequences: do not size a parameter array from the catalog, and do not assume an index beyond the catalog's range is invalid.

**`read_current_preset` sees unsaved edits**, which is what makes it the correct inspection path during editing - and what `cortex grid show` now exposes. Reading a STORED slot recalls it, which would have discarded each edit before it could be checked.

#### `set_block` verification was wrong, and the fix (2026-08-02)

Provoking a genuine DSP-capacity refusal - the one path that had never been exercised - found a real bug in the opposite direction.

**What happened.** Placing `Cory Wong Delay-y-y` (model 6025, the catalog's most expensive at `cpu=3.5`) into a freshly recalled empty preset, the first three placements reported `BlockRefused` and the next two echoed immediately. A read-back showed **all five were present**. The three "refusals" were false.

**Cause.** The echo latency varies with how busy the unit is, exactly as its handshake latency does. Straight after a recall it is still settling, so the `Grid` echo takes longer than the 5 s timeout. The original implementation treated "no echo within the timeout" as proof of refusal, which it is not.

**Why this direction matters more.** Reporting a placement as refused when it worked is worse than the converse: the caller re-adds a block that is already there, or abandons an edit that actually landed. A false success would at least be caught by the next read.

**Fix.** The echo is now a FAST PATH and the grid is ground truth. When no echo arrives, `set_block` reads the grid back and only reports `BlockRefused` if the cell genuinely does not hold the model. If the read-back itself fails, it says so rather than guessing either way.

`set_block` now returns `Placement::EchoConfirmed` or `Placement::ReadBackConfirmed` so a caller - and the CLI - can report which check actually confirmed it. Saying "echo confirmed" when the echo timed out and a read-back rescued it would misstate how much the device told us.

**Both paths verified afterwards.** With `--timeout 0`, which makes an echo impossible, a placement is still correctly confirmed by read-back. And filling the grid produced a **genuine refusal**: six blocks at `cpu=3.5` were accepted, the seventh and eighth were not - no echo AND a read-back confirming absence. So the negative path is now hardware-verified rather than merely implemented.

This also gives a rough figure for the preset DSP budget: 6 x 3.5 = 21.0 catalog CPU units fit on the unit tested, 7 did not. That is a single data point on one preset shape, not a formula.

#### Remaining grid write paths verified (2026-08-02)

Done on the empty 2B scratchpad, unit restored to 1A afterwards. All on the working grid; nothing saved.

| Path | Observed |
| --- | --- |
| `set_bypass` | `xxxxxxxx` - all eight scene slots set at once, confirming that bypass is one global state for a block that does not follow scenes |
| `set_chain_input` | row input became port 4 (Return 1) |
| `set_chain_output` | row output became port 19 (MULTIPLE) |
| `set_split` | `split 2 rejoin 5` on an even row; **refused with an explanation on an odd row**, as designed |
| `set_param_in_scene` | `per-scene A-H: [0.2, 0.2, 0.2, 0.8, 0.2, 0.2, 0.2, 0.2]` - scene D alone changed |

**The three-message per-scene sequence works.** Promote `scene_mode`, switch scene, write value. Scene D holds 0.8 while every other scene holds 0.2, which is exactly the requested edit and could not happen if any of the three messages were dropped.

**A stored preset carries eight values for EVERY parameter**, not only scene-following ones - `TREBLE` and `LEVEL` read back as eight identical entries. So the presence of eight values does not indicate `scene_mode`; only a difference between them does.

**A verification tool that hides what it verifies is worse than none.** The per-scene edit initially appeared to have failed: `GAIN` read back as 0.2 on both scene A and scene D. The write was correct all along - the read-back displayed only `param_values[0]`, which is always scene A regardless of the active scene. Two wrong conclusions were available (the write failed; the device ignores `scene_mode`) and both would have been recorded as protocol findings. `cortex grid show --params` now shows the whole per-scene array.
