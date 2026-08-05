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

## [DES-CLI-OVR] Overview

The `QuadCortex` client is the ergonomic API surface. It holds a `Session` reference and builds protobuf messages, handing them to the session's `send`/`request`/`await_broadcast`/`collect` primitives. It knows nothing about hidapi, framing, or the session state machine. The design is a direct port of `pyquadcortex/pyquadcortex/client.py::QuadCortex` (MIT), adapted to Rust idioms: the Python `self._t` transport reference becomes a `&Session` (or `Arc<Session>`); the `NamedTuple` helpers (`Block`, `Split`, `Folder`, `MidiOut`) become Rust structs with `#[derive(Clone, Debug, Serialize)]`; the module-level helper functions stay module-level.

The client is a thin layer: most methods are 5-20 lines of protobuf construction plus a `session.send` or `session.await_broadcast`. The complexity lives in the domain traps (see spec -> Domain Traps), which must be encoded as runtime checks and rustdoc, not hidden.

---

## [DES-CLI-ARCH] Architecture

### File Map (planned)

```text
crates/cortex-rs/src/
├── client/
│   ├── mod.rs             — pub mod declarations; re-exports; module-level helpers [FR-71..77]
│   ├── client.rs          — QuadCortex struct: lifecycle, catalog, read ops        [FR-1..25]
│   ├── navigation.rs      — recall_preset, switch_scene, copy_scene, scene label/color [FR-26..30]
│   ├── grid_write.rs      — write_preset, set_chain_*, set_param, set_bypass, set_block, remove_block, move_block [FR-31..39]
│   ├── splitter_mixer.rs  — set_splitter/mixer/lane/gate/split/split_mute          [FR-40..45]
│   ├── tempo.rs           — set_tempo_param/option/subdivision/metronome            [FR-46..48]
│   ├── stomp_midi.rs      — set_stomp_assignment, set_expression, set_midi_out     [FR-49..51]
│   ├── file_ops.rs        — save/delete/move_preset, create/delete_setlist, copy/duplicate, wait_for_listing [FR-52..59]
│   ├── captures_irs.rs    — set_capture, set_ir                                     [FR-60..61]
│   ├── global_settings.rs — update_settings, set_global_eq, set_mode_cycle, set_gig_view, show_tuner [FR-62..65]
│   ├── io_ports.rs        — set_input/output/usb_port, set_midi_thru, set_output_pairing [FR-66..70]
│   ├── helpers.rs         — blocks, splits, slot_to_position, position_to_slot, input_level_db, db_to_input_level, field_present [FR-71..77]
│   └── constants.rs       — UNITY_LEVEL, USER_SETLIST_ROOT, SCENE_UNLABELLED, TEMPO_PARAMS, model ids, etc.
└── catalog.rs             — ModelCatalog parser (ported from pyquadcortex/catalog.py)
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

## [DES-CLI-DATA] Data Model

### `QuadCortex` struct

| Field            | Type                      | Description                                                              |
| ---------------- | ------------------------- | ------------------------------------------------------------------------ |
| `session`        | `Arc<Session>`            | The session this client drives. Shared so the MCP server can hold one.   |
| `catalog_cache`  | `Mutex<Option<ModelCatalog>>` | Lazily fetched on first `catalog()` call; cached for the session.   |

### Value objects (in `helpers.rs` / `client/mod.rs`)

**`Block`** - one occupied grid cell:

| Field       | Type  | Description                |
| ----------- | ----- | -------------------------- |
| `row`       | `u32` | Grid row (0-based)         |
| `column`    | `u32` | Grid column (0-based)      |
| `model_id`  | `u32` | The model hash at this cell |

**`Split`** - where a row branches:

| Field           | Type  | Description                                         |
| --------------- | ----- | --------------------------------------------------- |
| `row`           | `u32` | The branching row (0 or 2)                          |
| `split_column`  | `i32` | Column where the lane leaves (`-1` = no branch)     |
| `mix_column`    | `i32` | Column where it rejoins (`-1` = non-rejoining)      |

With derived `rejoins() -> bool` (`mix_column >= 0`) and `lane_row() -> u32` (`row + 1`).

**`Folder`** - a device folder listing:

| Field         | Type     | Description                          |
| ------------- | -------- | ------------------------------------ |
| `key`         | `String` | Device filesystem key                |
| `name`        | `String` | Display name                         |
| `slots`       | `u32`    | Total slot count                     |
| `occupied`    | `u32`    | Occupied slot count                  |
| `is_factory`  | `bool`   | Factory folder flag                  |

**`MidiOut`** - one per-preset MIDI message:

| Field      | Type  | Description                          |
| ---------- | ----- | ------------------------------------ |
| `type`     | `u32` | Message type (PC, CC, ...)           |
| `channel`  | `u32` | MIDI channel                          |
| `param1`   | `u32` | Type-dependent param 1               |
| `param2`   | `u32` | Type-dependent param 2               |
| `param3`   | `u32` | Type-dependent param 3               |

With constructor methods `cc(channel, cc, value)`, `pc(channel, preset)`.

### Constants (in `constants.rs`)

| Constant                    | Value                                | Source                                              |
| --------------------------- | ------------------------------------ | --------------------------------------------------- |
| `UNITY_LEVEL`               | `0.76923077`                         | Measured: 10/13 = 0 dB on the -100..+30 dB span     |
| `USER_SETLIST_ROOT`         | `"/media/p4/Presets"`               | Device filesystem path                              |
| `SCENE_UNLABELLED`          | `" "` (single space)                 | The unit stores unlabelled as a space, not empty    |
| `CC_VERSION`                | `"4.0.1"`                           | CorOS 4.0.1 capture                                 |
| `SPLITTER`                  | `10004`                              | Unified Splitter model id                           |
| `MIXER`                     | `11000`                              | Mixer model id                                      |
| `LANE_OUTPUT_CONTROL`       | `23000`                              | Lane Output Control model id                        |
| `TEMPO_CONTROL`             | `25000`                              | Tempo Control model id                              |
| `INPUT_GATE_CONTROL`        | `28000`                              | Input Gate Control model id                         |
| `CAPTURE_FILE_NAME_PARAM`   | `5`                                  | Wire index of `file_name` on a capture block        |
| `CAPTURES_LIBRARY`          | `"local_nc_root"`                    | Captures Library folder key                         |
| `IR_LIBRARY`                | `"local_ir_root"`                    | IR Library folder key                               |
| `GLOBAL_EQ_BAND_STRIDE`     | `5`                                  | Parameters per Global EQ band                       |
| `GLOBAL_EQ_BANDS`           | `5`                                  | Number of Global EQ bands                            |
| `TEMPO_PARAMS`              | map of name -> index                 | Two names disagree with the catalog (MUTE=4, SUBDIVISIONS=7) |

---

## [DES-CLI-CONNECT] Lifecycle

`QuadCortex::connect(timeout, settle)` is the convenience entry point (the Rust equivalent of `pyquadcortex.connect()`):

1. Open the transport (zone 100): find the device by VID:PID, open the HID interface.
2. Create the `Session` around the transport.
3. Start the session (RX + keepalive threads).
4. Run the handshake (`session.connect(timeout, settle)`).
5. Construct `QuadCortex { session: Arc::new(session), catalog_cache: Mutex::new(None) }`.
6. Register the teardown order: disconnect, stop session, close transport (so `close()` pops in reverse).

`close()` runs the teardown in reverse order. `Drop` calls `close()` so a dropped client releases the device. The session is held in `Arc` so the MCP server can share it across tool invocations.

---

## [DES-CLI-READ] Read Operations

The read methods fall into two correlation patterns:

**Pattern A: `request()` (prompt same-type echo with `request_id`)** - used by `version()`, `active_scene()`, `read_current_preset()`. These send a READ with a `request_id` and match the reply on type + id.

**Pattern B: `await_broadcast()` (asynchronous push)** - used by `read_preset()`, `list_presets()`, `list_irs()`, `favorites()`. These send a trigger (a recall or a READ) and wait for an unsolicited push, filtered by a `match` predicate. `read_preset` is the canonical case: the recall's `request_id` is echoed on the push; the seed push (no id) is skipped.

**Pattern C: `collect()` (fan-out)** - used by `list_folders()`. A single `File` READ produces a flood of folder listings over 10-20 s; `collect` gathers them all.

**Pattern D: fire-and-forget `send`** - used by `switch_scene()`, `set_param()`, `set_block(verify=false)`, `set_bypass()`. The write STALL is swallowed by the transport; persistence is confirmed by a later read or a save.

**The `_read_state` helper** (ported from pyquadcortex): sends a READ and waits for a push containing the field it needs (state pushes can be PARTIAL - a push following an UPDATE may carry only what changed). Each reader matches on a field-presence predicate, not just "any push of this type".

---

## [DES-CLI-GRID] Grid Write

The grid-write methods share a single design principle: **sparse, row/column-keyed updates.** A `Grid{UPDATE}` carrying only the changed chain/model/param is the ONLY shape that persists. A full-preset write whose chains lack `row` is dropped (trap #2).

`set_param(scene=)` is the most complex: the flag (`scene_mode`) and a value cannot travel together (trap #4), so it issues 3 messages in order:
1. `set_param_scene_mode(row, column, param_index, true)` - the flag, alone.
2. `switch_scene(scene)` - switch to the target scene.
3. The value write - `Grid{UPDATE, preset{chains{row, models{column, params{index, param_values[0]{float_value}}}}}}`.

No settle delay is needed between them; ordering over the pipe is sufficient.

`set_block(verify=true)` uses `await_broadcast` to wait for the `Grid` echo naming the cell. The echo arrives ~0.3 s on the measured firmware. No echo within `timeout` = `BlockRefused` (trap #5).

`remove_block` uses action `DELETE` (trap #6). An `UPDATE` with `hash: 0` is transmitted and ignored.

---

## [DES-CLI-SPLIT] Splitter/Mixer/Lane/Gate

These write to sub-collections of `Chain` (`combined_splitter`, `mixer`, `output_control`, `input_control`) rather than `models[]`, so `set_param` cannot reach them. They share a helper (`_set_sub_param` in pyquadcortex) that builds a row-keyed `Grid{UPDATE}` against the right collection, with the flag-travels-alone rule applied.

The `_require_even_row` guard (trap #8) raises an error for odd rows before sending, rather than writing into a collection the device does not have there.

`set_splitter_param` writes `combined_splitter` (trap #7), NOT `splitter[]` (read-only).

---

## [DES-CLI-FILE] File Ops

File operations are eventually consistent: a `File{CREATE/DELETE/MOVE}` is accepted and the device updates lazily. The `_file_operation` helper (ported from pyquadcortex) sends the message via `request` with a short timeout and returns `None` on timeout - a missing reply says nothing about whether the op worked. Confirm by re-reading (`wait_for_listing`).

Addressing rules (traps #9, #10, #11):
- `save_current_preset`: destination by linear slot `index` in `folder.files[]`.
- `delete_preset`: source by file PATH (`<setlist>/<name>.pb`) in `folder.files[].key`.
- `move_preset`: source by file path, destination by linear index.
- `create_setlist`: `File{CREATE, type: 0, folder{key: USER_SETLIST_ROOT/<name>}}`.

`copy_preset` and `duplicate_setlist` are compositions (the device has no host-drivable copy): recall source, save grid into destination. Slow (recall + save per preset).

---

## [DES-CLI-CAPTURE] Captures and IRs

`set_capture` (trap #15): the `file_name` parameter (index 5) holds `<64-char content hash><display name>` concatenated. Loading a capture RESETS the block's other parameters silently - write params AFTER, or pass them via the `params` argument.

`set_ir`: an IR reference is TWO strings (not one concatenated like a capture). IR PATH (param 2 or 10) = the library key; IR NAME (param 22 or 23) = the display name. Every IR Loader has TWO slots (0 and 1), each with its own param pair.

---

## [DES-CLI-IO] I/O Ports

`set_input_port` / `set_output_port` / `set_usb_port` send ONE field per message (trap #13). The device drops fields that share a port entry: `mute` + `ground_lift` on an output both fail when paired, both work alone. Rather than track safe combinations, every field goes separately.

`set_input_port` takes the `Input` enum, not 1/2/3/4 (trap #12): the combined ids are interleaved, so Return 1 is 4 and Return 2 is 5. Passing 3 for "Return 1" writes the combined Input 1/2 entry.

---

## [DES-CLI-CATALOG] Catalog

`fetch_model_repo()` returns the payload captured by the paced handshake, avoiding a second 46 KB transfer; without that captured copy it performs `ModelRepo` READ + `await_broadcast`. `Catalog::parse` turns integer model ids into names, categories, and parameter lists in wire-index order. It covers installed plugins and the player's own Neural Captures, which no hard-coded table could know. The persistent daemon parses its captured copy once.

Name resolution belongs to `QuadCortex::set_parameter`, not a host surface. It reads the live cell to discover the model, resolves the named parameter through the catalog, and converts a real-unit value through that parameter's declared range. The CLI, daemon, MCP server and GUI therefore share one implementation and never ask callers to repeat a model id already present on the grid.

---

## [DES-CLI-DEC] Key Decisions

| Decision                                         | Choice                                                       | Rationale                                                                                                                                                            |
| ------------------------------------------------ | ------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Arc<Session>` not `&mut Session`               | Shared ownership                                             | The MCP server holds one session across many tool calls; the CLI holds one for the process lifetime. `Arc` lets both without a borrowdance.                        |
| Module-per-concern file layout                  | `navigation.rs`, `grid_write.rs`, `file_ops.rs`, ...        | A single 3000-line `client.rs` (like pyquadcortex's `client.py`) is unreadable; splitting by concern keeps each file navigable.                                        |
| Constants in a dedicated module                 | `constants.rs`                                               | The wire constants (model ids, param indices, TEMPO_PARAMS map) are measured, not invented; centralising them makes the provenance clear.                          |
| Value objects are pure                          | `#[derive(Clone, Debug, Serialize)]`, no I/O                 | `Block`, `Split`, `Folder`, `MidiOut` cross every host boundary (CLI, MCP, Tauri); they must serialize and never carry I/O.                                          |
| `Drop` calls `close()`                          | Implicit teardown                                           | A client dropped without explicit close still releases the device (the exclusive HID ownership matters).                                                            |
| Domain traps in rustdoc AND MCP descriptions     | Double documentation                                         | The MCP server exposes these methods to agents; the traps must be in the tool description so an agent does not silently edit the wrong row.                         |
| `_read_state` matches on field presence          | Not just "any push of this type"                             | State pushes can be PARTIAL; a push following an UPDATE may carry only what changed. Matching on the needed field prevents a stale/partial read.                  |
| Name resolution via catalog, not hardcoded indices | `param="THRESHOLD"` -> catalog -> wire index              | Indices are positional and not every one is a visible knob; naming is the safer route. The catalog is device-specific (covers installed plugins + captures).        |

---

## [DES-CLI-TEST] Testing Notes

**In-crate unit tests** (`helpers.rs`, `#[cfg(test)]`), no hardware:

- `test_blocks`: a fixture preset with occupied/empty cells returns only occupied `Block`s; `len(chain.models)` is 8 for every row.
- `test_splits`: a fixture with a branch reports `Split{row:0, split:2, mix:-1}` (non-rejoining) and `Split{row:2, split:4, mix:4}` (rejoining); a serial row is omitted.
- `test_slot_to_position`: `"28C"` -> 218; `"30A"` -> 232; round-trip via `position_to_slot`.
- `test_input_level_db`: wire 0.0 -> -12 dB; 1.0 -> +60 dB; 0.16667 -> ~0 dB; `db_to_input_level(0.0)` -> ~0.16667; out-of-range raises.
- `test_field_present`: a field with presence returns true when set; a field without presence (`SceneBypass.bypass`) returns false, no raise.

**Message-building tests** (with a fake `Session`):

- `test_set_param_scene_3_messages`: `set_param(scene=D)` issues 3 messages (promote, switch, write) in order.
- `test_remove_block_action_delete`: `remove_block` sends `Grid{action: DELETE}`, not UPDATE.
- `test_set_splitter_param_combined`: writes to `combined_splitter`, not `splitter`.
- `test_set_input_port_one_field_per_message`: `set_input_port(level=0.5, impedance=1.0)` sends 2 messages.
- `test_set_input_port_enum`: passing `Input::RETURN_1` writes id 4, not 3.

**Hardware verification** (manual, documented in the release smoke matrix):

- Each method exercised against a real Quad Cortex (CorOS 4.0.1): recall, read, switch scene, set param (with/without scene), set bypass, set block (verify + refused), remove block, save, delete, move, set capture, set IR, set input/output/usb port, update settings, set global eq, set mode cycle.
- Persistence confirmed by save + read-back.
- The domain traps confirmed by reproducing the silent-no-op conditions.

**Provisional labelling**: the client is provisional until the method surface has been exercised from this crate's own code against real hardware.

---

## [DES-CLI-PROVENANCE] Provenance & Attribution

The `QuadCortex` client is a port of `pyquadcortex/pyquadcortex/client.py` (MIT, (c) 2026 Stokes). The ~60 methods, the helper functions, the value objects, the constants, and the domain-trap documentation all originate there, confirmed against real hardware. The `ModelCatalog` parser is ported from `pyquadcortex/pyquadcortex/catalog.py`. See `THIRD-PARTY-NOTICES.md` for the MIT attribution.

No code is copied from the reference-only repositories without a clear repository-wide licence. Their findings are re-expressed in this project's own words.
## [DES-CLI-DIVERGENCE] Divergences from the original plan

### One `client.rs`, not a `client/` module tree

Same rationale as [140-session/design.md](../140-session/design.md): at this size the split would fragment more than it clarifies. Grid-edit message construction IS a separate module (`grid.rs`), because those builders are pure and benefit from being testable in isolation from the client that sends them.

### What is covered without hardware

Slot-name round-tripping and rejection of malformed names, dB conversion and its range guard, empty-slot detection in listings, trailing-slash key normalisation, folder occupancy counting, recall-payload encoding, the `Grid` echo matcher including its positional-fallback case, and `preset_has_block`.

The grid builders in `grid.rs` are covered separately and more heavily, since that is where the silent-no-op traps live: 18 tests, three of which were verified by mutation - reintroducing UPDATE-instead-of-DELETE, a value beside the `scene_mode` flag, and an unkeyed chain each made exactly the test that claims to guard it fail.
