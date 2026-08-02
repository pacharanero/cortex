---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["client", "api", "quad-cortex", "presets", "grid", "scenes", "provisional"]
spec: spec.md
design: design.md
---

# 150 Client - Tasks

> Implementation checklist for the ergonomic `QuadCortex` client API. Protocol facts are hardware-verified via pyquadcortex; **no method here has been exercised against a device from this crate's own code**, so the whole zone is provisional.

## Current state (2026-08-02)

Implemented in `crates/cortex-rs/src/client.rs` (a single file, not the planned `client/` module tree - same rationale as [140-session/tasks.md](../140-session/tasks.md#divergences-from-the-plan-recorded-not-silently-absorbed): at ~880 lines including tests the split would fragment more than it clarifies).

| Surface | Status |
| --- | --- |
| Lifecycle: `new`, `connect`, `disconnect`, `close` | Implemented |
| `version()` | Implemented, hardware-verified via the CLI |
| Reads: `read_current_preset`, `read_preset`, `active_scene`, `list_presets`, `find_preset`, `list_folders` | Implemented, **unverified against hardware** |
| Navigation: `recall_preset`, `switch_scene` | Implemented, unverified |
| Value objects: `PresetEntry`, `Folder` | Implemented + unit tested |
| Helpers: `slot_to_position(_checked)`, `position_to_slot`, `input_level_db`, `db_to_input_level`, `build_recall`, `folder_key` | Implemented + unit tested |
| Constants: `USER_SETLIST_ROOT`, `USER_SETLIST`, `SCENE_UNLABELLED`, `UNITY_LEVEL`, `BANKS`, `SLOTS_PER_BANK`, `SETLIST_SLOTS` | Implemented |
| Everything else (grid write, splitter/mixer, tempo, stomp, file ops, captures/IRs, global settings, I/O ports, catalog) | Not started |

What is tested without hardware: slot round-tripping and rejection of malformed names, dB conversion and its range guard, empty-slot detection in listings, trailing-slash key normalisation, folder occupancy counting, and recall-payload encoding. 15 client tests.

What is **not** covered: every method that talks to the device. The read paths in particular depend on correlation behaviour (which push echoes which `request_id`) that has no test double yet - see the same gap noted in [140-session/tasks.md](../140-session/tasks.md).

---

## Phase 0: Spike and Conformance Reference

<!-- files: (no source changes — study and conformance cross-check) -->
<!-- @see ../../../pyquadcortex/pyquadcortex/client.py -->
<!-- @see ../../../pyquadcortex/pyquadcortex/catalog.py -->

- [ ] Read `pyquadcortex/pyquadcortex/client.py` end to end; catalogue the ~60 methods and their correlation patterns (request / await_broadcast / collect / send).
- [ ] Read `pyquadcortex/pyquadcortex/catalog.py`; understand the `ModelCatalog` parse and the `Parameter` range conversion (`to_normalized`, `option_to_value`).
- [ ] Extract the full list of wire constants (model ids, param indices, TEMPO_PARAMS, GLOBAL_EQ layout, CAPTURE/IR param indices) into a reference table.
- [ ] Confirm the domain traps against `pyquadcortex` docstrings and a capture; cross-reference with `quad-cortex-linux-editor-and-protocol.md`.

---

## Phase 1: Module Skeleton and Constants

<!-- files: crates/cortex-rs/src/client/mod.rs, crates/cortex-rs/src/client/constants.rs -->
<!-- @see spec.md [FR-71..77] -->
<!-- @see design.md [DES-CLI-ARCH] [DES-CLI-DATA] -->

- [ ] Create the `client/` module under `crates/cortex-rs/src/`.
- [ ] Define `constants.rs` with all wire constants: `UNITY_LEVEL`, `USER_SETLIST_ROOT`, `SCENE_UNLABELLED`, `CC_VERSION`, model ids (`SPLITTER`, `MIXER`, `LANE_OUTPUT_CONTROL`, `TEMPO_CONTROL`, `INPUT_GATE_CONTROL`), `CAPTURE_FILE_NAME_PARAM`, `CAPTURES_LIBRARY`, `IR_LIBRARY`, `IR_PATH_PARAMS`, `IR_NAME_PARAMS`, `IR_LOADER_MODELS`, `GLOBAL_EQ_BAND_STRIDE`, `GLOBAL_EQ_BANDS`, `GLOBAL_EQ_OUT_*`, `TEMPO_PARAMS` map, `HOLD_TIMING_MS`.
- [ ] Re-export the constants from `client/mod.rs` and `lib.rs`.

---

## Phase 2: Value Objects and Helpers

<!-- files: crates/cortex-rs/src/client/helpers.rs -->
<!-- @see spec.md [FR-71..77] -->
<!-- @see design.md [DES-CLI-DATA] [DES-CLI-TEST] -->

- [ ] Define `Block` struct (`row`, `column`, `model_id`; `#[derive(Clone, Debug, Serialize)]`).
- [ ] Define `Split` struct (`row`, `split_column`, `mix_column`; derived `rejoins()`, `lane_row()`).
- [ ] Define `Folder` struct (`key`, `name`, `slots`, `occupied`, `is_factory`).
- [ ] Define `MidiOut` struct (`type`, `channel`, `param1`, `param2`, `param3`); constructors `cc()`, `pc()`.
- [ ] Implement `blocks(p) -> Vec<Block>` (skip empty cells: hash absent/zero; column from presence or position).
- [ ] Implement `splits(p) -> Vec<Split>` (from `split_control_points`; `split < 0` = no branch; `mix == -1` = non-rejoining).
- [ ] Implement `slot_to_position(slot: &str) -> u32` (`(bank-1)*8 + letter`, 0-based).
- [ ] Implement `position_to_slot(index: u32) -> String` (inverse).
- [ ] Implement `input_level_db(level: f64) -> f64` (`-12.0 + 72.0 * level`).
- [ ] Implement `db_to_input_level(db: f64) -> f64` (inverse; refuse outside -12..+60).
- [ ] Implement `field_present(message, field) -> bool` (proto3 presence; false on fields without presence, no raise).
- [ ] Write unit tests: `test_blocks`, `test_splits`, `test_slot_to_position`, `test_input_level_db`, `test_field_present` (fixture presets, no hardware).

---

## Phase 3: Catalog

<!-- files: crates/cortex-rs/src/catalog.rs -->
<!-- @see spec.md [FR-7] -->
<!-- @see design.md [DES-CLI-CATALOG] -->

- [ ] Port the `ModelCatalog` parser from `pyquadcortex/pyquadcortex/catalog.py` (MIT, attribute).
- [ ] Define `ModelCatalog`, `Model`, `Parameter` structs with `to_normalized(real)`, `option_to_value(option)`, `parameter(name)`, `parameters[index]`.
- [ ] Implement `QuadCortex::catalog()` (lazy fetch via `ModelRepo` READ + `await_broadcast`; cache in `Mutex<Option<ModelCatalog>>`).
- [ ] Implement `_resolve_param_index(param, model)` (name -> wire index via catalog).

---

## Phase 4: Lifecycle

<!-- files: crates/cortex-rs/src/client/client.rs -->
<!-- @see spec.md [FR-1..6] -->
<!-- @see design.md [DES-CLI-CONNECT] -->

- [ ] Define the `QuadCortex` struct (`session: Arc<Session>`, `catalog_cache: Mutex<Option<ModelCatalog>>`).
- [ ] Implement `QuadCortex::new(session)`.
- [ ] Implement `QuadCortex::connect(timeout, settle)` (open transport, start session, handshake, construct client; register teardown order).
- [ ] Implement `disconnect()` (delegate to `Session::disconnect`).
- [ ] Implement `close()` (reverse-order teardown: disconnect, stop, close transport; idempotent).
- [ ] Implement `Drop` (calls `close()`).
- [ ] Implement `version(timeout)` (`Version{READ}` via `request`; works without handshake).

---

## Phase 5: Read Operations

<!-- files: crates/cortex-rs/src/client/client.rs -->
<!-- @see spec.md [FR-8..25] -->
<!-- @see design.md [DES-CLI-READ] -->

- [ ] Implement `find_preset(name, setlist, timeout)` (case-insensitive exact match on `list_presets`).
- [ ] Implement `read_preset(setlist_path, position, is_factory, timeout)` (recall + `await_broadcast` with `request_id` match; skips seed push). Document the side-effect trap.
- [ ] Implement `read_current_preset(timeout)` (`RecallPreset{READ}` with `request_id`; no side effects).
- [ ] Implement `list_presets(setlist, timeout, include_empty)` (`File` READ + `await_broadcast`; trailing-slash normalization).
- [ ] Implement `list_folders(seconds)` (`File` READ + `collect`).
- [ ] Implement `active_scene(timeout)` (`Scene{READ}` + `request_id` match).
- [ ] Implement `_read_state(cls, match_fn, timeout)` helper (READ + await push with field-presence match).
- [ ] Implement `captures`, `list_irs`, `recents`, `favorites`, `pinned_models`, `master_volume`, `looper`, `tuner`, `io_settings`, `settings`, `global_eq`, `mode`, `mode_cycle` via `_read_state`.

---

## Phase 6: Navigation

<!-- files: crates/cortex-rs/src/client/navigation.rs -->
<!-- @see spec.md [FR-26..30] -->
<!-- @see design.md [DES-CLI-OVR] -->

- [ ] Implement `recall_preset(setlist_path, position, is_factory, request_id)` (`SetlistPosition{UPDATE}`; position via `slot_to_position`).
- [ ] Implement `switch_scene(scene)` (`Scene{UPDATE, selected_scene}`).
- [ ] Implement `copy_scene(from_index, to_index, swap)` (`SceneCopy{UPDATE}`).
- [ ] Implement `set_scene_label(scene_index, label)` (`SceneLabel{UPDATE}`; `None` -> `SCENE_UNLABELLED`).
- [ ] Implement `set_scene_color(scene_index, color)` (`SceneColor{UPDATE}`; ARGB uint32).

---

## Phase 7: Grid Write

<!-- files: crates/cortex-rs/src/client/grid_write.rs -->
<!-- @see spec.md [FR-31..39] -->
<!-- @see design.md [DES-CLI-GRID] -->

- [ ] Implement `write_preset(p)` (`Grid{UPDATE, preset: p}`). Document the no-row trap.
- [ ] Implement `set_chain_input(row, in_portid)` (row-keyed update).
- [ ] Implement `set_chain_output(row, out_portid)` (row-keyed update).
- [ ] Implement `set_param(row, column, param_index, value, scene, param, model, real, promote, text)`:
  - [ ] `real=` conversion via catalog (needs `param` + `model`).
  - [ ] `text=` for string-valued params.
  - [ ] `scene=` path: 3 messages (promote, switch, write). Document the flag-travels-alone trap.
- [ ] Implement `set_param_scene_mode(row, column, param_index, enabled)` (flag alone).
- [ ] Implement `set_bypass(row, column, bypassed, scene)` (with `scene=`: switch first).
- [ ] Implement `set_block(row, column, model, verify, timeout)`:
  - [ ] `verify=true`: `await_broadcast` for the `Grid` echo naming the cell; `BlockRefused` on timeout. Document the DSP-capacity trap.
  - [ ] `verify=false`: fire-and-forget `send`.
- [ ] Implement `remove_block(row, column)` (`Grid{action: DELETE}`). Document the action-DELETE trap.
- [ ] Implement `move_block(from_row, from_col, to_row, to_col, drop)` (`GridMove`).
- [ ] Define `BlockRefused` error type.
- [ ] Write message-building tests: `test_set_param_scene_3_messages`, `test_remove_block_action_delete` (fake session).

---

## Phase 8: Splitter/Mixer/Lane/Gate

<!-- files: crates/cortex-rs/src/client/splitter_mixer.rs -->
<!-- @see spec.md [FR-40..45] -->
<!-- @see design.md [DES-CLI-SPLIT] -->

- [ ] Implement `_require_even_row(row, what)` guard (raise on odd rows; document trap).
- [ ] Implement `set_splitter_param(row, param, value, real, scene, promote)` (writes `combined_splitter`; document the not-`splitter[]` trap).
- [ ] Implement `set_mixer_param(row, param, value, real, scene, promote)` (writes `mixer[]`).
- [ ] Implement `set_lane_output(row, param, value, real, scene, promote)` (writes `output_control[]`).
- [ ] Implement `set_input_gate(row, param, value, real, scene, promote)` (writes `input_control[]`).
- [ ] Implement `_set_sub_param` shared helper (flag-travels-alone rule).
- [ ] Implement `set_split(row, split_column, mix_column)` (activates branch via `split_control_points`; `mix_column=-1` for non-rejoining).
- [ ] Implement `clear_split(row)` (both columns to -1).
- [ ] Implement `set_split_mute(row, muted)` (writes `splitBypass`; sets all 8 scenes).
- [ ] Write test: `test_set_splitter_param_combined` (writes `combined_splitter`, not `splitter`).

---

## Phase 9: Tempo

<!-- files: crates/cortex-rs/src/client/tempo.rs -->
<!-- @see spec.md [FR-46..48] -->
<!-- @see design.md [DES-CLI-OVR] -->

- [ ] Implement `set_tempo_param(param, value, real)` (name resolution via `TEMPO_PARAMS` first, then catalog; writes `tempoProgramData`).
- [ ] Implement `set_tempo_option(param, option)` (range-checked; `option_to_value` via catalog).
- [ ] Implement `set_tempo_subdivision`, `set_metronome_sound`, `set_metronome_routing`, `set_time_signature`, `set_tempo_led`, `set_metronome_volume` (typed wrappers).

---

## Phase 10: Stomp/Expression/MIDI

<!-- files: crates/cortex-rs/src/client/stomp_midi.rs -->
<!-- @see spec.md [FR-49..51] -->
<!-- @see design.md [DES-CLI-OVR] -->

- [ ] Implement `set_stomp_assignment(row, column, footswitch)` (DELETE existing then UPDATE - 2 messages).
- [ ] Implement `clear_stomp_assignment(row, column)`.
- [ ] Implement `set_stomp_momentary(footswitch, momentary)`, `set_stomp_label(footswitch, label, single)`.
- [ ] Implement `set_expression(row, column, param, pedal, minimum, maximum, model)`.
- [ ] Implement `set_expression_bypass(row, column, pedal, mode, invert, delay_ms, latch_emulation)`.
- [ ] Implement `set_midi_out(source, messages)` (via `MIDISettings`, NOT `Grid`; document trap).
- [ ] Implement `set_preset_load_midi_out(messages)`.

---

## Phase 11: File Ops

<!-- files: crates/cortex-rs/src/client/file_ops.rs -->
<!-- @see spec.md [FR-52..59] -->
<!-- @see design.md [DES-CLI-FILE] -->

- [ ] Implement `_file_operation(msg, timeout)` (request with short timeout; `None` on timeout).
- [ ] Implement `save_current_preset(setlist_path, position, name, instrument, default_scene, confirm)`:
  - [ ] `default_scene` switch first (active scene at save time is recorded).
  - [ ] `File{CREATE, type: 0}` with `folder.files{index, name, instrument}`.
  - [ ] `confirm=true`: `wait_for_listing` to get the device-stored name.
- [ ] Implement `delete_preset(setlist_path, name)` (`File{DELETE}` by file path; document trap).
- [ ] Implement `move_preset(setlist_path, name, to_position)` (`File{MOVE}`; source by path, dest by index).
- [ ] Implement `create_setlist(name)` / `delete_setlist(name)`.
- [ ] Implement `copy_preset(from_setlist, position, to_setlist, to_position, name)` (recall + save composition).
- [ ] Implement `duplicate_setlist(source_name, dest_name, limit)` (create + copy each).
- [ ] Implement `wait_for_listing(setlist, until, timeout, interval)` (poll; ride out missed pushes; two diagnosis types).

---

## Phase 12: Captures and IRs

<!-- files: crates/cortex-rs/src/client/captures_irs.rs -->
<!-- @see spec.md [FR-60..61] -->
<!-- @see design.md [DES-CLI-CAPTURE] -->

- [ ] Implement `set_capture(row, column, capture, model, params)`:
  - [ ] Optionally `set_block` if `model` is set.
  - [ ] Write `file_name` param = `<key><name>` (concatenated).
  - [ ] Apply `params` after (floats as values, strings as text). Document the resets-params trap.
- [ ] Implement `set_ir(row, column, ir, slot, model)`:
  - [ ] Optionally `set_block`.
  - [ ] Write IR PATH (param 2/10) = key; IR NAME (param 22/23) = name. Two strings, not one. Document.

---

## Phase 13: Global Settings

<!-- files: crates/cortex-rs/src/client/global_settings.rs -->
<!-- @see spec.md [FR-62..65] -->
<!-- @see design.md [DES-CLI-OVR] -->

- [ ] Implement `update_settings(**fields)` (sparse; refuse `power_option`, `reset_wifi_networks`; validate field names).
- [ ] Implement `set_hold_timing(milliseconds)` / `hold_timing_ms(timeout)` (ms <-> index).
- [ ] Implement `set_scene_bypass_behavior(behavior)`.
- [ ] Implement `set_master_volume_assignment(...)` (read-merge-write for submessage).
- [ ] Implement `set_global_bypass(cab, ir)` (read-merge-write).
- [ ] Implement `set_global_eq(band, gain, frequency, q, filter_type, enabled)` (sparse by index; 5 params/band).
- [ ] Implement `set_global_eq_output(level, out12, out34)`.
- [ ] Implement `set_global_eq_bypassed(bypassed)`.
- [ ] Implement `set_mode_cycle(slots)` (validate: at most one hybrid; refuse broken value 9).
- [ ] Implement `set_gig_view(shown)` / `show_tuner(shown)`.

---

## Phase 14: I/O Ports

<!-- files: crates/cortex-rs/src/client/io_ports.rs -->
<!-- @see spec.md [FR-66..70] -->
<!-- @see design.md [DES-CLI-IO] -->

- [ ] Implement `set_input_port(input_port_id, level, impedance, input_type, ground_lift)` (one field per message; `Input` enum; document the enum-id trap).
- [ ] Implement `set_output_port(output_port_id, level, ground_lift, mute)` (one field per message).
- [ ] Implement `set_usb_port(level, hp_select, dry_wet)` (one field per message).
- [ ] Implement `set_midi_thru(enabled)`.
- [ ] Implement `set_output_pairing(xlr1_2, out3_4)`.
- [ ] Implement `set_input_level` / `set_output_level` / `set_output_mute` convenience wrappers.
- [ ] Write test: `test_set_input_port_one_field_per_message`, `test_set_input_port_enum` (fake session).

---

## Phase 15: Re-exports and lib.rs

<!-- files: crates/cortex-rs/src/client/mod.rs, crates/cortex-rs/src/lib.rs -->
<!-- @see design.md [DES-CLI-ARCH] -->

- [ ] Re-export `QuadCortex`, `Block`, `Split`, `Folder`, `MidiOut`, `BlockRefused`, and all public constants from `client/mod.rs`.
- [ ] Re-export from `lib.rs` so consumers use `cortex_rs::QuadCortex`.
- [ ] Implement `connect(timeout, settle)` as a module-level function (the `pyquadcortex.connect()` equivalent).

---

## Phase 16: Hardware Verification

<!-- files: (no source changes — manual verification against hardware) -->
<!-- @see spec.md Acceptance Criteria -->
<!-- @see design.md [DES-CLI-TEST] -->

- [ ] `connect()` returns a ready client; `version()` succeeds without the full handshake.
- [ ] `read_preset()` recalls a slot; recall's `request_id` echoed on the push.
- [ ] `read_current_preset()` returns the live grid; no side effects.
- [ ] `list_presets()` returns occupied slots in order; trailing-slash normalized.
- [ ] `switch_scene()` / `set_param()` / `set_bypass()` / `set_block()` persist (save + read-back).
- [ ] `set_param(scene=D)` issues 3 messages; lands on scene D.
- [ ] `set_block(verify=true)` raises `BlockRefused` when DSP exhausted (no echo).
- [ ] `remove_block()` uses action DELETE.
- [ ] `set_splitter_param()` writes `combined_splitter` (not `splitter[]`).
- [ ] `save_current_preset()` writes; `confirm=true` returns device-stored name.
- [ ] `delete_preset()` addresses by file path.
- [ ] `set_capture()` writes `file_name` = `<hash><name>`; params after survive.
- [ ] `set_ir()` writes key to IR PATH, name to IR NAME (two strings).
- [ ] `set_input_port()` one field per message; `Input` enum ids (Return 1 = 4).
- [ ] Helper functions pass on fixture presets (already CI-tested).
- [ ] Each domain trap confirmed by reproducing the silent-no-op condition.
- [ ] Remove "provisional" labelling from client docs and release notes once the above pass.

---

## Work Sessions

| Date       | Task                 | Action | Files Modified                                                                                              | Agent | Human |
| ---------- | -------------------- | ------ | ----------------------------------------------------------------------------------------------------------- | ----- | ----- |
| 2026-08-01 | Spec authoring       | Wrote  | spec/150-client/spec.md, spec/150-client/design.md, spec/150-client/tasks.md                                | [x]   | [ ]   |