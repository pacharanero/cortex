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

Verification is per method. The implemented non-UI read, navigation, grid-edit, tempo, STOMP/expression/MIDI, file, capture/IR selection, global-setting, I/O and pin/Favorite paths are hardware-verified against CorOS 4.0.1. A CorOS 4.0.1 false-only capture-dialog response froze and rebooted the unit, so no response is exposed; visual-only Gig View/Tuner visibility methods remain unverified; positive host capture acceptance is structurally unavailable and planned separately.

---

## Requirements

### Functional Requirements

#### Lifecycle

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-1 | `QuadCortex::new(session)` constructs the client around a `Session`. | Must Have |
| FR-2 | `connect(timeout, settle)` is a convenience that opens the transport, starts the session, runs the handshake, and returns a `QuadCortex`. The Rust equivalent of `pyquadcortex.connect()`. | Must Have |
| FR-3 | `disconnect()` sends `Connection{connected: false}` (delegates to `Session::disconnect`). | Must Have |
| FR-4 | `close()` delegates to `Session::close()`: announce disconnect, join workers, explicitly release the owned link. Safe to call more than once. | Must Have |
| FR-5 | `version(timeout)` reads the device's version info (`VersionMessage`: `app_fw_version`, `device_type`, `device_serial_number`, `comms_version`). Works without the full handshake. | Must Have |
| FR-6 | `Session::Drop` calls `close()` as a final fallback. `QuadCortex` itself has no closing `Drop`: short-lived wrappers share the daemon session and dropping one must not disconnect every caller. | Should Have |

#### Catalog

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-7 | `fetch_model_repo(timeout)` returns the catalog payload captured by the handshake, or performs `ModelRepo` READ + `await_broadcast` when no captured copy exists. `Catalog::parse` turns that device-specific payload into model and parameter metadata. | Must Have |

#### Read operations

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-8 | `find_preset(name, setlist, timeout)` looks up a preset by display name (exact, case-insensitive). Returns the listing entry whose `index` is the slot position. | Must Have |
| FR-9 | `read_preset(setlist_path, position, is_factory, timeout)` recalls a preset and returns its full `BinaryPreset`. NOTE: this RECALLS the slot (side effect - it loads the preset onto the grid). Tags the recall with a fresh `request_id` and accepts only the `RecallPreset` push echoing it (skips the seed push). | Must Have |
| FR-10 | `read_current_preset(timeout)` returns the LIVE grid (`RecallPreset{READ}` with a `request_id`, matched on the echo). No side effects - unsaved edits survive; the active scene is untouched. | Must Have |
| FR-11 | `list_presets(setlist, timeout, include_empty)` lists presets in slot order. Sends a `File` READ and accepts only a preset-category, non-move `File{UPDATE}` for the normalized target key with exactly one unique entry for every index 0-255. | Must Have |
| FR-12 | `list_folders(seconds)` enumerates every folder the device knows (uses `collect` for the folder flood). | Should Have |
| FR-13 | `active_scene(timeout)` reads the current scene (`Scene{READ}`, matched on `request_id`). | Must Have |
| FR-14 | `captures(timeout)` reads the Neural Capture library (`File` READ with `type: 2`, matched on `request_id` + folder key). | Should Have |
| FR-15 | `list_irs(folder, timeout)` lists IRs (`File` READ with `type: 1`, matched on `request_id` + folder key). | Should Have |
| FR-16 | `recents(timeout)` reads the Recents list (`RecentsFavorites{READ}`, non-empty match). An empty uncorrelated push is not treated as a complete empty Recents list because transient empty pushes are known. | Should Have |
| FR-17 | `favorites(timeout, attempts)` reads the Favorites list (`RecentsFavorites{READ, is_favorites: true}`, matched on `request_id`; retries on timeout). | Should Have |
| FR-18 | `pinned_models(timeout)` reads pinned model ids. | Should Have |
| FR-19 | `master_volume(timeout)` reads the Master Volume state (read-only; no setter - the knob is the only way to move it). | Should Have |
| FR-20 | `looper(timeout)` reads the Looper X state (read-only). | Should Have |
| FR-21 | `tuner(timeout)` reads the tuner state (`input_port_id`, `frequency` as Hz offset from 440, `mute`). | Should Have |
| FR-22 | `io_settings(timeout)` reads input/output/headphone/USB/MIDI/expression port settings. | Must Have |
| FR-23 | `settings(timeout)` reads global device settings (`GeneralSettings`). | Must Have |
| FR-24 | `global_eq(timeout)` reads the Global EQ state (bypassed + 5 bands). | Should Have |
| FR-25 | `mode(timeout)` reads the footswitch mode state; `mode_cycle(timeout)` reads the configured slots. | Should Have |

FR-14 through FR-25 are hardware-verified on CorOS 4.0.1. Wide firmware-defined state is returned as generated protobuf values; capture and IR listings use a minimal `LibraryEntry { key, name }`. State reads reject partial pushes that omit the field promised by the method. `mode` requires `mode` presence while `mode_cycle` independently requires a present, non-empty `available_modes.modes`. Favorites, PinnedModels and variable-length File listings use request-id correlation so empty replies remain distinguishable from no reply.

Variable-length File replies have no total count, terminal marker, or observed multi-part completion field. The client therefore accepts one `File{UPDATE}` only when both its echoed request id and normalized folder key match. This proves response identity and permits a genuine empty result, but it does not prove firmware could not omit entries or send another matching response later. Repeated capture reads were stable on CorOS 4.0.1, and one correlated response arrived for each of the loadable IR library and a user-IR folder; each response remains the honest observable boundary.

#### Navigation

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-26 | `recall_preset(setlist_path, position, is_factory, timeout)` sends `SetlistPosition{UPDATE}` and waits for the correlated `RecallPreset` push before returning. Position is linear index or slot name (e.g. `"28C"`). | Must Have |
| FR-27 | `switch_scene(scene)` sends `Scene{UPDATE, selected_scene}`. Scenes are 0-based. | Must Have |
| FR-28 | `copy_scene(from_index, to_index, swap)` sends `SceneCopy{UPDATE}` (label and color travel with the copy). | Should Have |
| FR-29 | `set_scene_label(scene_index, label)` sends `SceneLabel{UPDATE}`; `None` sends `SCENE_UNLABELLED` (a single space, not empty string). | Should Have |
| FR-30 | `set_scene_color(scene_index, color)` sends `SceneColor{UPDATE}` with an ARGB uint32. | Should Have |

All five navigation operations are implemented and hardware-verified on CorOS 4.0.1. Scene indices are validated before wire I/O. A `SceneCopy` acknowledgement cannot safely update the subscribed baseline because the device omits `is_swap`; hosts refresh the full live preset after copy/swap.

#### Grid write

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-31 | A low-level whole-preset write, if retained for investigation, must document that a recalled preset has no row keys and therefore applies nothing useful. Host surfaces use keyed wrappers instead. | Should Have |
| FR-32 | `set_chain_input(row, GridInputPort)` emits the row-keyed numeric wire value, then reads the complete live grid and confirms that route. The ONLY shape that actually moves an input. | Must Have |
| FR-33 | `set_chain_output(row, GridOutputPort)` does the same for output routing. Arbitrary wire integers are unavailable through the public method because the device silently stores meaningless ids. | Must Have |
| FR-34 | `set_param(row, column, param_index, value)` is the low-level wire-index write. `set_parameter(row, column, target, input, scene, promote, timeout)` is the shared host-facing API: it validates rows/columns/scenes and normalised values, reads the cell's model when addressed by name, resolves through the device catalog, refuses meters, converts real units, performs the 3-message scene sequence when requested, and confirms the target scene/value through a complete live-grid read. `set_param_option` resolves an index or catalog name against dynamic options in a supplied current preset and centrally normalizes the named option. | Must Have |
| FR-35 | `set_param_scene_mode(row, column, param_index, enabled)` sets the scene-following flag (must travel ALONE). | Must Have |
| FR-36 | `set_bypass(row, column, bypassed)` writes the bypass state across the block's stored scene slots and confirms the active-scene or global result through a complete live-grid read. Scene-targeted bypass is not part of the implemented API. | Must Have |
| FR-37 | `set_block(row, column, model, verify, timeout)` treats a matching echo as a fast confirmation, then falls back to live-grid read-back. It returns `BlockRefused` only when read-back proves the model is absent; echo timeout alone is never proof of refusal. | Must Have |
| FR-38 | `remove_block(row, column)` sends `Grid{action: DELETE, ...}` (NOT UPDATE with hash:0, which is ignored) and succeeds only when a complete live-grid read contains the target cell as empty. A missing row/cell is unconfirmed, not proof of removal. | Must Have |
| FR-39 | `move_block(from_row, from_col, to_row, to_col, drop, timeout)` reads and validates an occupied source and empty destination, sends `GridMove` without its advisory grid snapshot, then reads the live grid back to prove the source cleared and the complete model payload plus bypass state reached the destination. A cross-row move lets the device compute a parallel path. | Should Have |

#### Splitter/mixer/lane/gate

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-40 | `set_splitter_param(row, param_index, value, scene, promote)` writes `chain.combined_splitter` (NOT `chain.splitter`, which is read-only), with no invented model hash. Row must be 0 or 2. | Should Have |
| FR-41 | `set_mixer_param(row, param_index, value, scene, promote)` writes `chain.mixer[]` with model 11000. Row must be 0 or 2. | Should Have |
| FR-42 | `set_lane_output(row, param_index, value, scene, promote)` writes `chain.output_control[]` with model 23000. | Should Have |
| FR-43 | `set_input_gate(row, param_index, value, scene, promote)` writes `chain.input_control[]` with model 28000. The raw-index core does not claim meter safety from an index; any future catalog name resolver must reject `Meter` parameters. | Should Have |
| FR-44 | `set_split(row, split_column, mix_column)` sets `split_control_points` (activates a branch) and confirms both points through a complete live-grid read. `mix_column=-1` creates a non-rejoining branch. Row must be 0 or 2. | Should Have |
| FR-45 | `set_split_mute(row, muted)` writes `chain.splitBypass` (sets all 8 scenes at once). | Should Have |

FR-40 through FR-43 intentionally expose the composable raw-index/normalised-value core rather than four near-identical catalog APIs. Values are rejected unless finite and within 0..1. Optional scene writes use the existing separate promote, switch, value sequence; the scene-mode flag is never packed with a value. Pure message-shape and fake-link sequencing/refusal tests establish these behaviours offline. Every row-control method and split mute passed fresh live read-back and recall restoration on CorOS 4.0.1; the splitter result is read from `combined_splitter`, not the legacy view.

#### Tempo

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-46 | `set_tempo_param(parameter, input, timeout)` writes a finite normalised or catalog-converted real-unit value to one supported per-preset Tempo-menu parameter in `tempoProgramData`, model 25000. `TempoParameter` preserves the positional indices and the screen names that disagree with the catalog. | Should Have |
| FR-47 | `set_tempo_option(parameter, option)` sets one of the four established list-valued parameters by zero-based option number, range-checking and converting exactly as `index / (count - 1)`. | Should Have |
| FR-48 | `set_tempo_subdivision`, `set_metronome_sound`, `set_metronome_routing`, `set_time_signature`, `set_tempo_led`, and `set_metronome_volume` expose checked typed values for established controls. | Should Have |

FR-46 through FR-48 use a pure builder for `Grid{UPDATE, preset{tempoProgramData{hash:25000, params{index, value}}}}`. Tempo program data is the deliberate exception to row-keyed grid edits: it sits outside `chains` and has no row or column key, while the parameter index remains explicit. The supported indices are TEMPO 0, LED LIGHT 2, VOLUME 3, MUTE 4 (misnamed START by the catalog), PAN 5, TIME SIGNATURE 6, SUBDIVISIONS 7 (NOTELENGTH in the catalog), SOUND 8, and ROUTING 9. Index 1 is not exposed as MODE: changing the Tempo menu's MODE produced no wire traffic, and positive evidence for MODE or internal MIDI clock writes is absent. The checked lists have 4 subdivisions, 6 sounds, 5 routes, and 21 time signatures; time-signature changes may rewrite positional STEPSTATE accent parameters 10-22. Pure shape, boundary, invalid-input and fake-link tests establish the offline contract. All eight exposed write methods passed fresh read-back on CorOS 4.0.1 after muting first, and recall restored the complete tempo baseline.

#### Stomp/expression/MIDI

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-49 | `set_stomp_assignment(row, column, footswitch)` assigns a block to a STOMP footswitch (DELETE existing then UPDATE - two messages). | Should Have |
| FR-50 | `set_expression(row, column, param, pedal, minimum, maximum, model)` assigns an expression pedal to a parameter. | Should Have |
| FR-51 | `set_midi_out(source, messages)` sets MIDI messages via `MIDISettings` (NOT `Grid` - a Grid update carrying `midi_messages_general_v2` is ignored). | Should Have |

FR-49 through FR-51 include `clear_stomp_assignment`, preset-local momentary and both STOMP label maps, expression bypass, source-specific MIDI output, and preset-load MIDI output. Rows are checked as wire rows 0-3 and columns as 0-7; footswitches are A-H/0-7; expression pedals are 1-2; normalized expression endpoints are finite and within 0..1, with minimum above maximum deliberately allowed to reverse the sweep. Expression bypass modes preserve the hardware-established numbering STOP 0, SWITCH 1, HEEL_TOE 2, and delay is limited to 0-5000 ms.

MIDI messages use checked constructors for CC, expression CC, CC Toggle, and PC. Channels are 1-16, every message data value is 7-bit, and each source or preset-load group replaces at most 12 messages. The wire message is always `MIDISettings{UPDATE}` with one nested `GeneralMIDIMessage{source, msg[]}` in `general_midi_messages` or `preset_load_messages`; it is never `Grid`. Pure shape and fake-link tests cover zero-valued row, column, source, and footswitch keys plus the required STOMP DELETE-before-UPDATE order. CorOS 4.0.1 hardware verification covered every PROT-006.9 method: reversible working-copy STOMP/expression read-back and persistent generated-storage MIDI save/recall verification across all message families, typed helpers and raw 10x12 layout. Generated storage was removed and the original preset restored. MIDI recall may emit preset-load messages, so outputs remain disconnected.

#### File ops

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-52 | `save_current_preset(setlist_path, position, name, instrument, timeout)` sends `File{CREATE, type: 0}` and accepts only a `CREATE` acknowledgement naming the exact setlist and slot. `Instrument` is the closed wire enumeration None/Guitar/Bass/Synth/Vocal/Other = 0..5. The device saves the grid it already has and may de-duplicate the name. Host surfaces enforce preparation and confirmation separately. | Must Have |
| FR-53 | `delete_preset(setlist_path, name)` sends `File{DELETE}` addressed by file path (`<setlist>/<name>.pb`), NOT slot index, and accepts only a `DELETE` acknowledgement naming that exact setlist and path. | Must Have |
| FR-54 | `move_preset(policy, setlist_path, from_position, to_position)` authorises the exact source and destination through the caller's policy, then uses a fresh complete listing to refuse the factory library, empty sources, no-op moves, and observed occupancy. It resolves the source's listed file path, sends `File{MOVE}` with the same-setlist destination by linear index, then polls fresh listings for source-absent/destination-present convergence without requiring an acknowledgement. | Should Have |
| FR-55 | `create_setlist(name)` accepts one safe component, sends `File{CREATE, type: 0, folder{key: USER_SETLIST_ROOT/<name>}}`, refuses a fresh collision, and returns the one newly appearing direct USER folder from fresh listings so the operation owns its destination. | Should Have |
| FR-56 | `delete_setlist(name)` refuses factory/root/`My Presets` and unsafe components before I/O, sends `File{DELETE}`, and polls fresh directory listings to absence. | Should Have |
| FR-57 | `copy_preset(policy, from_setlist, position, to_setlist, to_position, name, instrument)` prepares and backs up the destination before recalling the source, saves the recalled grid, and returns actual destination metadata from a fresh complete listing. It changes what is loaded on the unit. | Should Have |
| FR-58 | `duplicate_setlist(source_name, dest_name, limit)` composes verified create plus one occupied-slot recall/save via `copy_preset`; it never sends `BulkOperation`, preserves slot positions and instrument tags, and reports a created partial destination honestly after failure. | Should Have |
| FR-59 | `wait_for_listing(setlist, until, timeout, interval)` polls `list_presets` until a condition holds (file ops are eventually consistent). Rides out missed pushes. | Should Have |

Preset File replies have no usable request-id correlation. Exact action and target predicates safely separate concurrent different-target flows, but two acknowledgements for the same operation and target are indistinguishable. Likewise, a fresh listing cannot make a later File write atomic because the request carries no storage revision or compare-and-swap precondition. Timeouts and non-convergent final listings are unconfirmed outcomes that require inspection rather than blind retry; these protocol limits are tested and documented, not claimed as solved. `File SWAP` is outside the implemented surface because no supporting wire evidence exists.

#### Captures/IRs

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-60 | `set_capture(row, column, capture, model, params, timeout)` accepts a validated device-returned entry, optionally verifies placement of default capture model 14000 or requires an existing compatible 14000/14001 block, writes `file_name` param 5 as the exact `<64-char hash><display name>` concatenation, then writes caller parameters in order. Index 5 is reserved and refused in `params`. Loading a capture RESETS the block's other parameters, so no follow-up may precede selection. | Should Have |
| FR-61 | `set_ir(row, column, ir, slot, model, timeout)` accepts a validated device-returned entry and slot 0/1, optionally verifies placement of loader model 29001-29008 or requires an existing compatible loader, then writes IR PATH (param 2/10) as the exact non-filesystem library key followed by IR NAME (param 22/23) as the display name. Every IR Loader has two slots. Fresh read-back cannot prove loadability because invalid strings are stored unchanged; the on-unit warning icon remains the decisive check. | Should Have |
| FR-80 | The public client exposes no Neural Capture dialog response. On CorOS 4.0.1, the exact false-only `NeuralCapture{UPDATE, show_dialog: false}` response following `try_to_show_dialog: true` froze and rebooted the unit. Causation is not established, but no retry is permitted absent new evidence. Capture creation, dialog handling, transfer, backup, and cloud workflows are native-device operations; selecting an existing device capture remains supported. Positive acceptance remains structurally unavailable from CLI/MCP/GUI. No `show_dialog_fail_reason` semantics are inferred. | Must Have |

#### Global settings

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-62 | `update_settings(GeneralSettingsPatch)` changes only known writable scalar settings. The type cannot represent power/reset/updater/reboot/shutdown fields or unsupported `internal_midi_clock_enabled`. `set_hold_timing` accepts exactly 500-1000 ms in 100 ms steps; `set_scene_bypass_behavior` takes the closed three-value enum. | Must Have |
| FR-63 | `set_master_volume_assignment` and `set_global_bypass` perform explicit settings READ, merge partial intent, and write every sibling in each nested replacement group. Their restore APIs require complete typed nested state. | Must Have |
| FR-64 | Global EQ exposes whole-EQ bypass; five bands at stride 5 with Gain/Frequency/Q/Type/Enabled offsets 0-4; and OUT level/1-2/3-4 at indices 25-27. Every parameter is a sparse one-index write, normalized 0-1; no dB mapping is claimed for OUT level. | Should Have |
| FR-65 | `set_mode_cycle(slots)` replaces a non-empty cycle of typed values 0-8, with at most one HYBRID and never a HYBRID alone. Value 9 cannot be represented. `set_mode` accepts the same closed slot type. | Should Have |
| FR-81 | `set_gig_view(shown)` and `show_tuner(shown)` open/close their device views. Tuner input is limited to the established accepted set; mute is Boolean; reference is a finite -15..=15 Hz offset from 440. | Should Have |

#### Pinning and Favorites

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-78 | `pin_model(model_id)` sends exactly one id with the default/omitted `CREATE` action; `unpin_model(model_id)` sends exactly one id with `DELETE`. Pinning appends and may duplicate; unpinning removes every copy. | Should Have |
| FR-79 | `add_favorite(item, timeout)` and `remove_favorite(item, timeout)` send exactly one complete device-supplied `RecentsFavoritesItem`, with `is_favorites: true` and `CREATE`/`DELETE` respectively, and accept only an exact same-operation echo. | Should Have |

These four methods are hardware-verified on CorOS 4.0.1. Duplicate pinning, unpin-all semantics, Favorite add/remove and exact final baseline restoration passed; PinnedModels reads established request-id correlation. Favorites callers must preserve exact device metadata from Recents or Favorites; a matching name alone is not a safe target identity.

#### I/O ports

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-66 | `set_input_port(input_port_id, level, impedance, input_type, ground_lift)` - each field sent in its OWN message (the device drops fields that share a port entry). `input_port_id` takes the `Input` enum, NOT 1/2/3/4 (Return 1 is 4). | Must Have |
| FR-67 | `set_output_port(output_port_id, level, ground_lift, mute)` - one field per message (mute is dropped when paired). | Must Have |
| FR-68 | `set_usb_port(level, hp_select, dry_wet)` - one field per message. | Should Have |
| FR-69 | `set_midi_thru(enabled)` toggles MIDI Thru. | Should Have |
| FR-70 | `set_output_pairing(xlr1_2, out3_4)` pairs/unpairs output couples. | Should Have |

FR-66 through FR-70 use closed `InputPort` and `OutputPort` identifiers plus typed sparse patches. Input ids preserve the interleaved combined entries, so Return 1 is 4; physical output ids preserve paired and individual XLR/Out/Send entries 1-9. Every selected input, output, USB, MIDI or pairing control is validated before the first write, then sent as its own `IOSettings{UPDATE}` in documented patch order. Input and output messages repeat exactly one typed port key; no sibling writable field shares that message. Empty patches and non-finite or out-of-range normalized values fail before I/O. `set_input_level`, `set_output_level` and `set_output_mute` retain the same one-field invariant.

Successful write dispatch is not device confirmation. `io_settings_complete` issues an explicit READ and enforces the hardware-measured CorOS 4.0.1 capability matrix: inputs 1/2 have level/impedance/type/ground and 4/5 level/ground; outputs 1/4/5 have level/ground/mute, 2/6/7 level/mute, and 8/9 level. USB/MIDI and both pairing flags are required while read-only plugged/headphone/expression state is excluded. `poll_io_settings` repeats fresh complete reads for eventual-consistency confirmation. Pure builders and fake-link tests pin exact one-field shapes, key repetition, order, typed ids, the exact complete structural matrix, truly incomplete rejection and invalid-input no-I/O behaviour. With outputs disconnected, hardware verification changed every applicable field using valid discrete selectors, polled eventual state, restored each field and pairing member, and independently matched the complete final baseline.

#### Helper functions (module-level)

| ID | Requirement | Priority |
| ----- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| FR-71 | `blocks(p) -> Vec<Block>` returns the OCCUPIED grid cells (every row reports 8 column slots, empty ones have hash absent/zero; `len(chain.models)` is always 8). | Must Have |
| FR-72 | `splits(p) -> Vec<Split>` returns where each row branches (`split_control_points`; `split >= 0` means a branch; `mix == -1` means non-rejoining). | Should Have |
| FR-73 | `slot_to_position(slot) -> u32` converts a slot name (e.g. `"28C"`) to a linear index (`(28-1)*8 + 2 == 218`). | Must Have |
| FR-74 | `position_to_slot(index) -> String` is the inverse. | Should Have |
| FR-75 | `input_level_db(level) -> f64` converts a wire `level` (0..1) to dB (`-12 + 72 * level`; input ports span -12..+60 dB). | Should Have |
| FR-76 | `db_to_input_level(db) -> f64` is the inverse; refuses values outside -12..+60 dB. | Should Have |
| FR-77 | `field_present(message, field) -> bool` checks proto3 field presence without raising on fields without presence (e.g. `SceneBypass.bypass`). | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| NFR-1 | The client adds no async runtime dependency; it uses the session's blocking primitives directly. The leaf-crate discipline is preserved. | Architectural invariant |
| NFR-2 | Host-facing types in `view`, plus `PresetEntry`, `Folder`, parameter inputs and placement results, are pure serialisable value objects with no I/O. | Code invariant |
| NFR-3 | The handshake fully receives the load-bearing catalog payload and held hosts cache parsed/static state only for its exact generation/revision. Non-empty `NewModels`, stream gaps, and new generations prevent name or parameter resolution through stale metadata; client methods do not repeat a still-current transfer unnecessarily. | Code invariant |
| NFR-4 | Unit tests for the helper functions (`blocks`, `splits`, `slot_to_position`, `input_level_db`, `field_present`) and the message-building logic run in CI without hardware, using fixture presets. | CI-enforced |
| NFR-5 | The ~60-method API surface mirrors pyquadcortex's method names (snake_case) so a porting caller recognises the surface; deviations are documented. | Architectural invariant |
| NFR-6 | Every domain trap (see Domain Traps below) is documented in the method's rustdoc and surfaced in the MCP tool descriptions where relevant. | Code invariant |

---

## Acceptance Criteria

- [x] `QuadCortex::connect()` returns a ready client; `version()` works on both minimal and held-session paths.
- [x] `read_preset()` recalls a slot and returns the correlated `BinaryPreset`.
- [x] `read_current_preset()` returns the live grid with no recall side effect.
- [x] `list_presets()` accepts only complete unique 256-slot setlist updates and normalises keys.
- [x] `switch_scene()` / `set_param()` / `set_bypass()` / `set_block()` passed live read-back and save/recall verification where persistence applies.
- [x] `set_param(scene=D)` issues the promote/switch/write sequence and changes only the target scene.
- [x] `set_block(verify=true)` uses echo or read-back confirmation and reports a hardware-verified genuine DSP refusal only after absence is read back.
- [x] `remove_block()` uses action DELETE and passed read-back.
- [x] Host-facing parameter, bypass, removal, routing, and split methods fail with `GridWriteUnconfirmed` when complete live-grid read-back does not match the requested state. Typed routing and mandatory per-call verification passed the official-client MCP hardware smoke on CorOS 4.0.1 on 2026-08-11.
- [x] `move_block()` refuses invalid cells before writing and confirms both source and destination by complete live-grid read-back. Same-row move/reverse preserved every parameter and all bypass state, and a cross-row move passed on CorOS 4.0.1; recall restored the original grid.
- [x] PROT-006.7 splitter, mixer, lane-output, gate and split-mute methods passed fresh hardware read-back and recall restoration; splitter verification uses hashless `combined_splitter`.
- [x] `save_current_preset()` writes the working grid to the exact slot and exact `CREATE` acknowledgement is correlated; host surfaces gate it through prepared save.
- [x] `delete_preset()` addresses by file path, correlates the exact `DELETE`, and passed disposable-slot hardware smoke.
- [x] `move_preset()` validates scratch containment and a fresh complete listing, sends the captured same-setlist `MOVE` shape, and polls for listing convergence. Fake-link tested and hardware-verified through the held daemon on CorOS 4.0.1 with `7A -> 7B -> 7A`, storage-revision advancement, deletion and empty-slot cleanup.
- [x] PROT-006.4 wider reads passed hardware correlation and field-presence checks, including stable repeated capture responses, IR folders, empty Favorites and request-correlated PinnedModels.
- [x] `set_capture()` preserved the exact selected reference and a post-selection parameter through fresh hardware read-back.
- [x] `set_ir()` preserved the exact device-returned key/name, and timed on-unit inspection confirmed no warning for the selected user IR.
- [x] PROT-006.13 complete I/O read, mutation, pairing and exact restoration coverage passed on CorOS 4.0.1 with external outputs disconnected.
- [x] Pure helpers and typed views pass fictional-fixture tests; PROT-006.15 is complete.
- [~] Verification remains per operation: the false-only capture-dialog response was removed after it froze and rebooted a CorOS 4.0.1 unit; positive host capture belongs to the separate planned host-workflow scope, and Gig View/Tuner visibility remain visually unverified.

---

## Non-Goals

- The session layer (zone 140): connect handshake, keepalive, correlation. This zone consumes `Session`.
- USB HID transport (zone 100), framing (zone 110), protobuf schema (zone 120).
- The typed domain model (zone 130): `BinaryPreset`, `Block`, `Split` definitions. This zone imports them.
- MCP/GUI host integration (zones 300/400): token registries, exact-target confirmation UI, and safety-event logging. The shared policy and prepared-save implementation live in `cortex-rs::safety`; hosts must use it rather than wrapping `save_current_preset` independently.
- CLI surface (zone 200): `clap` commands, output formatting. This zone is the engine the CLI calls.
- Tauri backend (zone 400): Tauri commands. This zone is the engine the backend calls.

---

## Dependencies

- **Crate-internal**: zone 140 (`Session`), zone 130 (`view::*`, grid builders, `Catalog`, safety values), and zone 120 (generated proto types).
- **External (leaf)**: `serde` (for the value objects), `uuid` (for the session_id in the handshake, if owned here), the generated `prost` types. No async runtime, no `clap`, no `tauri`.
- **Prior art**: `pyquadcortex/pyquadcortex/client.py` (`QuadCortex` class, ~60 methods) and `pyquadcortex/pyquadcortex/catalog.py` (`ModelCatalog` parser) - ported under MIT with attribution; see `THIRD-PARTY-NOTICES.md`.

---

## Domain Traps (must be documented in rustdoc and MCP descriptions)

These are confirmed on hardware and are the primary source of silent wrong-row / silent-no-op behaviour. Every one must appear in the relevant method's rustdoc and, where the MCP server exposes the method, in the tool description.

1. **Rows are 0-based in the API, 1-4 on screen.** `row=0` is the top row on screen; `row=2` is labelled 3. Getting this wrong is QUIET: an edit lands on a real row, just not the one intended, and reads back perfectly. `GridOutputPort::NextRow3`, `NextRow4`, and `NextRow34` encode internal wire values 16-18; `Multiple` encodes the real destination 19.

2. **A recalled preset carries no explicit `row`.** Writing it back wholesale via `write_preset()` does nothing - a full-preset write that re-pointed `in_portid` read back UNCHANGED. Use the keyed wrappers (`set_chain_input`, `set_param`, `set_bypass`) instead.

3. **`read_preset` RECALLS the slot (side effect).** It loads the preset onto the grid, discarding unsaved edits and resetting the active scene. `read_current_preset` does NOT - use it for inspection during editing.

4. **`set_param(scene=)` is 3 messages.** The flag (`scene_mode`) and a value CANNOT travel in the same message - sent together, the flag is silently dropped. So: promote scene_mode, switch scene, then write. Ordering over the pipe is enough; no settle delay needed. Naming a scene leaves the unit sitting on it (visible side effect).

5. **`set_block` can be refused for DSP capacity.** The preset has a processing budget; a block that does not fit is accepted on the wire and simply absent afterwards. No per-block error message. A matching `Grid` echo is the fast path; after an echo timeout, live-grid read-back is ground truth and only confirmed absence is `BlockRefused`.

6. **`remove_block` uses action DELETE.** An UPDATE carrying `hash: 0` is transmitted and ignored. The action is what marks the removal.

7. **`set_splitter_param` writes `combined_splitter`, not `splitter[]`.** `splitter[]` is read-only; a write there is silently ignored. Always go through the method.

8. **Splitters/mixers exist only on rows 0 and 2.** An odd row raises `ValueError` (in Rust: returns `Error`) rather than sending a write into a collection the device does not have there.

9. **`set_midi_out` goes through `MIDISettings`, not `Grid`.** A `Grid` update carrying `midi_messages_general_v2` is accepted and ignored.

10. **`save_current_preset` may rename.** The device de-duplicates on collision (truncates + `_N` suffix). Read back to confirm the stored name if it matters.

11. **`delete_preset` addresses by file path, not slot index.** `File{DELETE, files{key: "<setlist>/<name>.pb"}}`.

12. **`set_input_port` takes the `Input` enum, not 1/2/3/4.** Return 1 is 4, Return 2 is 5 (combined ids are interleaved). Passing 3 for "Return 1" writes the combined Input 1/2 entry - an easy, expensive mistake.

13. **Each I/O port field is sent in its own message.** The device drops fields that share a port entry (mute + ground_lift on an output both fail when paired, both work alone). One field per message is the guarantee.

14. **`read_preset` interleaved with scene-targeted writes silently retargets them.** A `read_preset` resets the active scene to the preset's default. A later `set_param_in_scene` switches scenes as part of its sequence and leaves that scene active. Inspect with `read_current_preset` and check `active_scene` rather than assuming the prior selection survived a recall.

15. **Loading a capture RESETS the block's other parameters.** `set_capture` writes the reserved index-5 selector first, then applies its ordered parameter slice. It refuses index 5 in that slice and stops before all selector/follow-up writes if verified placement fails.

16. **IR reference read-back does not prove loadability.** The device stores arbitrary IR PATH/NAME strings unchanged and reports a broken reference only with a warning icon on the unit. Use a device-returned library key, never a filesystem path, and retain the manual screen check.

---

## Appendix

### Protocol Provenance & Attribution

The implemented `QuadCortex` operations are derived from the corresponding parts of `pyquadcortex/pyquadcortex/client.py` (MIT, (c) 2026 Stokes), adapted to Rust and verified per operation rather than inheriting upstream's evidence wholesale. The implemented slot and level helpers have the same provenance; helpers and methods still listed as planned are not claimed as present. The `ModelCatalog` parser is ported from `pyquadcortex/pyquadcortex/catalog.py`. Record any derivation in `NOTICE` / `THIRD-PARTY-NOTICES.md`.

Implemented constants such as `UNITY_LEVEL`, `USER_SETLIST_ROOT`, and `SCENE_UNLABELLED` encode measured wire or device values. Any future operation-specific maps, ids, and indices named in the requirements must likewise come from evidence when that operation is implemented rather than being invented in advance.

### Provisional labelling

The implemented non-UI Quad Cortex surface is hardware-verified per operation from this crate on CorOS 4.0.1. The host-owned capture-dialog response and visual-only Gig View/Tuner visibility methods remain unverified, as do all future operations and platforms. Do not apply one blanket label to the entire client.

### Hardware findings (CorOS 4.0.1)

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

#### Grid editing verified on hardware

The first destructive surface, exercised end to end. Safe by construction at this stage: no save is implemented, so every grid edit is transient and a recall discards it. The unit was restored to `1A` unchanged afterwards.

| Step | Action | Verified by |
| --- | --- | --- |
| 1 | `set_block(screen row 2, col 0, model 1)` | the device's `Grid` echo, AND an independent `read_current_preset` showing `Myth Drive` in that cell |
| 2 | `set_param(--param GAIN --value 0.9)` | read-back showing `GAIN 0.9` |
| 3 | `remove_block(screen row 2, col 0)` | read-back showing the row empty |

**Evidence at this point in the chronology.** `set_block` reported "echo confirmed" and the read-back agreed, so `grid_echoes_cell` matched the positive real-traffic case. A DSP refusal had not yet been provoked in this first run; the later investigation below supersedes that limitation and verifies both read-back acceptance and genuine refusal.

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
| `set_chain_input(GridInputPort::Return1)` | row input became `return1` (wire 4) |
| `set_chain_output(GridOutputPort::Multiple)` | row output became `multiple` (wire 19) |
| `set_split` | `split 2 rejoin 5` on an even row; **refused with an explanation on an odd row**, as designed |
| `set_param_in_scene` | `per-scene A-H: [0.2, 0.2, 0.2, 0.8, 0.2, 0.2, 0.2, 0.2]` - scene D alone changed |

**The three-message per-scene sequence works.** Promote `scene_mode`, switch scene, write value. Scene D holds 0.8 while every other scene holds 0.2, which is exactly the requested edit and could not happen if any of the three messages were dropped.

**A stored preset carries eight values for EVERY parameter**, not only scene-following ones - `TREBLE` and `LEVEL` read back as eight identical entries. So the presence of eight values does not indicate `scene_mode`; only a difference between them does.

**A verification tool that hides what it verifies is worse than none.** The per-scene edit initially appeared to have failed: `GAIN` read back as 0.2 on both scene A and scene D. The write was correct all along - the read-back displayed only `param_values[0]`, which is always scene A regardless of the active scene. Two wrong conclusions were available (the write failed; the device ignores `scene_mode`) and both would have been recorded as protocol findings. `cortex grid show --params` now shows the whole per-scene array.
