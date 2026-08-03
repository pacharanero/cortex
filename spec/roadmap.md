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

Complete. See [completed.md](completed.md).

### PROT-002: Framing layer (zone 110)

Complete. See [completed.md](completed.md).

### PROT-003: Proto schema (zone 120)

- [ ] **PROT-003.1**: Curate the message-type registry - map each `CortexMessageType` to its generated struct, so the session layer can dispatch by type tag

### PROT-004: Domain model - core (zone 130)

- [ ] **PROT-004.1**: `Preset` newtype wrapping `proto::BinaryPreset` with ergonomic field access
- [~] **PROT-004.2**: `Grid`. The `Row` newtype exists (`from_wire` / `from_screen`, refusing screen row 0) and encodes the numbering trap. A read-side `Grid` view over a preset is still planned. NOTE: `grid.rs` is taken by the message builders, so the domain view needs another home
- [ ] **PROT-004.3**: `Block` struct (row, column, model_id, params)
- [ ] **PROT-004.4**: `Scene` struct (index, label, color, bypass state)
- [~] **PROT-004.6**: Helpers. Done: `slot_to_position` (+ checked variant), `position_to_slot`, `input_level_db`, `db_to_input_level`, `preset_has_block`. Planned: `blocks()`, `splits()`, `free_rows()`, `row_status()`

### PROT-005: Session layer (zone 140)

Complete. See [completed.md](completed.md).

The background RX thread, connect handshake, keepalive, and request/response correlation.

### PROT-006: Client API (zone 150)

The ergonomic `QuadCortex` struct - the Rust equivalent of pyquadcortex's 60+ methods.

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

- [ ] **PROT-008.6**: `cortex session start` - a persistent, subscribed session. **In progress: the session holds and serves, the cache and health work do not exist yet.** Cortex Control is fast because it opens ONE session and keeps it; we pay a handshake per command. It is also the right shape for the MCP server, which must hold a single connection anyway, and for the GUI. Measured against a held session: `scene` 0.07 s, `grid` 0.14 s, `--status` 0.005 s, against a 3-5 s direct baseline.
  - [ ] **008.6.3**: Lifecycle. Done: `status`, a clean `stop` that announces the disconnect and is bounded by a watchdog, stale-socket detection, and **backgrounding** - `session start` detaches with `setsid` and returns only once the session answers a request, not merely once the socket binds (those differ by the whole handshake, because the socket is claimed first). **Outstanding: an idle timeout.**
  - [ ] **008.6.4**: **Health reporting.** The fail-fast is done: a request against a silent device returns `Error::DeviceSilent` in 10 s rather than burning the full 30 s timeout, guarded by `has_heard_from_device` and threshold-set from the measured 4-5 s post-handshake lull. **Outstanding: reconnect with backoff**, reporting each attempt, and surfacing a verdict in `session status` rather than the raw seconds
    - **The silence was ours. Root cause found and fixed: the keepalive interval was 5 s.** A capture of Cortex Control shows it sending a keepalive every 1.04 s (681 over 708 s), and its session never quiet for more than **0.11 s** across a 60 s idle. Our 5 s interval was set from a code comment asserting that was what Cortex Control did; it is not. At 1 s, a 90 s idle now reads `last_message_seconds: 0` throughout, where it previously climbed past 51 s.
    - **The withdrawn fail-fast is worth rebuilding now, and its original premise was right.** The design was: while a wait is blocked, poll every 0.5 s and abandon the request once nothing has arrived for 5 s - purely observational, sending nothing. It was withdrawn because it refused a healthy `version` request, but that was the keepalive bug producing genuine silence on a session we had starved. Silence is conclusive again. Rebuild with the `has_heard_from_device` guard intact, since nothing has arrived before a session's first message and that must not read as a fault.
    - This also resolves the apparent contradiction with `GlobalTempo` arriving every 0.8 s indefinitely - the observation that broke the adaptive settle. Both were real: the heartbeat is continuous while the client keeps the session alive, and stops when it does not. Nothing about the device switches; we did. See `spec/140-session/spec.md`.
    - `status` therefore reports `last_message_seconds` raw, with no verdict attached, and the doc comments that used to present it as a liveness signal have been corrected.
  - [ ] **008.6.5**: Cache device state, kept current by the subscription. Verified pushable: parameter values (`Grid`), bypass (`Grid`), scene (`Scene`/`RecallPreset`), dirty state (`PresetDirty`). Plus static data: the catalog and folder listings
  - [ ] **008.6.6**: **Invalidate the cache wholesale on reconnect.** Edits made while disconnected are invisible, so resuming a stale cache would silently lie
  - [ ] **008.6.7**: Commands address the daemon when it is running. Routed: `version`, `scene`, `grid`, `presets`, `recall`, `catalog`, `cpu`. Measured through a held session: `catalog` 1.97 s -> **0.02 s** (served from the copy the handshake already waited for, rather than making the device build and send 46 KB again), `presets` 6.44 s -> 2.35 s, `recall` instant. Grid edits route too: `set-bypass`, `remove-block`, `set-split`, `set-input`/`-output` at **0.08-0.20 s** against 1.88 s direct. Rows cross the socket as plain zero-based wire indices and are rebuilt with `Row::from_wire` on the far side - deriving `Deserialize` on `Row` would let any integer off a socket become one, defeating the guard on a mistake that succeeds silently and edits the wrong row.
    - Still direct, and so refused while a daemon holds the device: `folders`, `probe`, `set-block` and `set-param`. `set-param` needs restructuring first: it opens the session before resolving its value, because name-to-index and real-to-normalised conversion need the catalog. With the catalog now available from the daemon in 0.02 s it can resolve first and then send a simple request. `set-block` returns a `Placement`, which needs a typed representation on the wire rather than a debug string.
    - **Correction, hardware-verified:** the original plan had `version` keep addressing the device directly, on the reasoning that it needs no handshake. It cannot. A direct open alongside a held session wedges the device (see 008.6.9), and `version` was the command that proved it. It now routes when a daemon is running and goes direct only when none is - verified byte-identical output on both paths, 2.9 s routed. The daemon answers in the same `DeviceVersion` shape rather than a `{:?}` dump, so the two paths are one format, not two
- [ ] **PROT-008.7**: Cache the catalog on disk, keyed by `CorOS` version. It changes only on a firmware update or a new capture. Less urgent than it was: a held session now caches it in memory and serves `cortex catalog` in 0.02 s, so this is for the one-shot path and for surviving a restart

### PROT-007: Capture and IR export / import

Neural Captures and user IRs live only on the unit and in Neural's cloud. No existing tool - official or community - can export a capture to a local file, so a player's own captures cannot be backed up, version-controlled, or moved between units. They are the player's OWN data, which makes this the least legally fraught significant feature on this page and arguably the most valuable.

Status: **investigation, not yet designed.** The wire path is unconfirmed. `FileMessage` carries `ir_payload` and `preset_payload` `bytes` fields, and `LocalBackup`, `CloudBackup`, and `BackupsForward` message types exist in the recovered schema, so a route probably exists - but which one carries capture audio data, and in what format, is unknown.

- [ ] **PROT-007.1**: Establish whether a capture's payload can be read over the wire at all, and by which message type. **Use `s/usb-trace` and `s/usb-decode`** to record Cortex Control performing a backup and read the message types it uses. The original wording said to use `CORTEX_TRACE`, which cannot work: that traces our own client, not the official one - the tooling to watch another client did not exist when this was written (ENG-005)
- [ ] **PROT-007.2**: Identify the on-wire container and whether it is gzipped, chunked, or both (the `ModelRepo` precedent is gzip inside a `bytes` field, ~47 KB over ~371 reports)
- [ ] **PROT-007.3**: `export_capture(key, path)` - write one capture to a local file
- [ ] **PROT-007.4**: `import_capture(path)` - write a capture back to the unit. **Destructive; gate behind confirmation and the MCP safety surface**
- [ ] **PROT-007.5**: Same for user IRs (`list_irs` already enumerates them)
- [ ] **PROT-007.6**: `cortex capture export` / `import` CLI surface
- [ ] **PROT-007.7**: Decide and document a container format. Prefer something self-describing that records the source unit, CorOS version, and capture metadata, so a file is still meaningful years later

Do not ship import before export has been round-tripped on hardware: writing a malformed capture to the unit is the most plausible way this project could damage a user's data.

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
- [ ] **MCP-001.2**: Never write to the factory setlist; restrict saves to a designated scratch range of USER slots unless overridden
- [ ] **MCP-001.3**: Back up the target slot (`read_preset`) before overwriting, and keep the blob
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

### GUI-005: Always-visible preset directory and CPU load

Stack confirmed as Tauri 2 + React + Mantine, as AGENTS.md says, unless a concrete reason to change appears.

Improve on the Cortex Control appearance while staying familiar enough to navigate without relearning.

- [ ] **GUI-005.1**: Preset directory in a left sidebar, visible at all times rather than behind a picker. **Decided:** a tree (setlist then slots), showing only populated folders by default. The unit reports 399 folders and two hold anything, so an unfiltered tree would misrepresent how full the unit is.
- [ ] **GUI-005.2**: CPU load visible at all times, **including the per-column, per-core breakdown**, not just a total. `Session::cpu_load()` supplies both (PROT-008.6.12). The per-core detail is the actionable part: when a preset will not fit, it shows which work could move to the other core, row or column.

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
- [ ] **ENG-004.2**: CI gate for `@see` link resolution (optional, low priority)

### ENG-005: `s/usb-trace` - observe Cortex Control on the wire

A script that sets up passive USB observation of the official Cortex Control app driving the device, so its traffic can be decoded against our schema. This is the tool for questions of the form "how does the official client do X" - the answer to which is evidence about the wire, not inference about intent.

**It paid for itself before Cortex Control was ever traced.** The first capture was of our OWN client, run only to check whether the device's apparent silences were real or an artefact of our RX path. It showed the bus idle for 46.8 s in the middle of a handshake, with the device answering every write in 217 us - which identified writer starvation in `session.rs` (PROT-008.5, PROT-008.6.10) after a full day of attributing that variance to the device. Two features had already been built and withdrawn on the strength of the wrong explanation. Trace our own side first; it is cheap, needs no VM, and the bug is as likely to be ours.

**Named `usb-trace`, not `usb-record` or `usb-capture`, deliberately.** "Capture" already means a Neural Capture in this domain and "record" implies audio; either would suggest this script records sound, which it emphatically does not. `trace` is already the project's word for protocol observation (`CORTEX_TRACE`), so the two read as the same idea at different levels.

The method, from [the research note](../quad-cortex-linux-editor-and-protocol.md): with the QC passed through to a Windows VM under QEMU, the **host** kernel still sees the traffic. So `modprobe usbmon` plus a capture of the relevant `usbmonN` interface on the Linux host records everything, without needing USBPcap inside Windows and without the macOS exclusive-access problem.

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