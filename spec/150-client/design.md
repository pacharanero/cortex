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

The implemented client-owned values are `PresetEntry`, `Folder`, `ParameterInput`, and `ParameterWrite`. Renderable grid values such as `view::Preset`, `view::Block`, and `view::Row` belong to zone 130 so every host sees one representation. `Split` helpers and MIDI-output values remain planned under PROT-006.15 and PROT-006.9 rather than forming part of the current API.

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

**Pattern D: fire-and-forget `send`** - used by `switch_scene()`, scene label/colour/copy, `set_param()`, `set_block(verify=false)`, `set_bypass()`. The write STALL is swallowed by the transport; persistence is confirmed by a later read or a save. The daemon follows every scene copy/swap with `read_current_preset()` because the device's `SceneCopy` acknowledgement omits `is_swap` even when it performed a swap, so reducing that echo would turn a correct device swap into an incorrect cached copy.

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

---

## [DES-CLIENT-SPLIT] Splitter/Mixer/Lane/Gate

Only split-point placement is implemented: `set_split` writes `split_control_points` and rejects odd rows before sending. Splitter, mixer, lane-output, input-gate and split-mute parameter methods remain planned under PROT-006.7.

---

## [DES-CLIENT-FILE] File Ops

File operations are eventually consistent. Save and delete require exact action/target acknowledgements. Move instead polls fresh complete listings for source-absent/destination-present convergence because prior-art hardware evidence shows a file mutation may land without replying; a materially changed complete listing advances `storage_revision` and stales prepared-save epochs even without an acknowledgement. A missed deadline is an explicitly unconfirmed outcome, never permission to retry blindly.

Implemented addressing rules:
- `save_current_preset`: destination by linear slot `index` in `folder.files[]`.
- `delete_preset`: source by file PATH (`<setlist>/<name>.pb`) in `folder.files[].key`.
- `move_preset`: same-setlist source by listed file PATH in `folder.files[].key`, destination by linear slot `index` in `to_folder.files[]`. The host authorises the exact named source and destination slots, then a fresh complete listing resolves the source path and refuses observed occupancy, empty sources, no-op moves, and factory content.

Move message construction, scratch checks, convergence predicates, the public list/send/re-list sequence over a fake link, daemon confirmation, protocol-skew shutdown, and MCP absence are tested offline. The complete held-daemon path is hardware-verified on CorOS 4.0.1 with a prepared fixture moved `7A -> 7B -> 7A`, storage revision advancement, listing convergence in both directions, deletion, and empty-slot cleanup. Copy, create/delete setlist, and duplicate-setlist operations remain planned under PROT-006.10.

---

## [DES-CLIENT-CAPTURE] Captures and IRs

Capture and IR selection are not implemented. Their prior-art wire shapes and silent-reset constraints are tracked under PROT-006.11; export/import remains a separate investigation under PROT-007.

---

## [DES-CLIENT-IO] I/O Ports

Per-port settings are not implemented. The one-field-per-message rule, typed input identifiers, and other prior-art constraints are tracked under PROT-006.13. Implemented chain routing currently accepts raw port ids through `set_chain_input` and `set_chain_output`; replacing host-facing raw integers with typed routing values is tracked under MCP-002.7.

---

## [DES-CLIENT-CATALOG] Catalog

`fetch_model_repo()` returns the payload captured by the paced handshake, avoiding a second 46 KB transfer; without that captured copy it performs `ModelRepo` READ + `await_broadcast`. `Catalog::parse` turns integer model ids into names, categories, and parameter lists in wire-index order. It covers installed plugins and the player's own Neural Captures, which no hard-coded table could know. The persistent daemon parses its captured copy once.

Name resolution belongs to `QuadCortex::set_parameter`, not a host surface. It reads the live cell to discover the model, resolves the named parameter through the catalog, and converts a real-unit value through that parameter's declared range. The CLI, daemon, MCP server and GUI therefore share one implementation and never ask callers to repeat a model id already present on the grid.

---

## [DES-CLIENT-DEC] Key Decisions

| Decision                                         | Choice                                                       | Rationale                                                                                                                                                            |
| ------------------------------------------------ | ------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Arc<Session>` not `&mut Session`               | Shared ownership                                             | The MCP server holds one session across many tool calls; the CLI holds one for the process lifetime. `Arc` lets both without a borrowdance.                        |
| Keep orchestration together; extract pure boundaries | `client.rs` plus `grid.rs`, `view.rs`, `catalog.rs`, `safety.rs` | Splitting stateful orchestration would expose private session concerns; pure reusable logic earns separate modules. |
| Value objects are pure                          | Serialisable values with no I/O                 | Client request/result values and zone-130 host views cross CLI, MCP, Tauri and daemon boundaries without carrying session state.                                          |
| `Session::Drop` calls `close()`                 | Final implicit teardown                                     | The physical owner releases the device; dropping one shared `QuadCortex` wrapper does not disconnect sibling callers.                                                |
| Domain traps in rustdoc AND MCP descriptions     | Double documentation                                         | The MCP server exposes these methods to agents; the traps must be in the tool description so an agent does not silently edit the wrong row.                         |
| Name resolution via catalog, not hardcoded indices | `param="THRESHOLD"` -> catalog -> wire index              | Indices are positional and not every one is a visible knob; naming is the safer route. The catalog is device-specific (covers installed plugins + captures).        |

---

## [DES-CLIENT-TEST] Testing Notes

Offline tests cover strict listing and file-acknowledgement predicates, slot conversion, dB conversion, catalog-typed parameter resolution, folder/listing conversion, recall encoding, block-echo matching, factory-save refusal, and malformed inputs. Pure keyed grid builders have their own tests in `grid.rs`, including mutation checks for DELETE-vs-UPDATE, scene-mode isolation, and keyed rows.

The manual hardware runbook covers the implemented read, recall, scene, grid-write, save and delete paths. Unimplemented operations are not presented as verified; each newly implemented wire operation remains provisional until added to that smoke.

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
