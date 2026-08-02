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
> **`[x]` means the code exists and passes the local gate. It does NOT mean hardware-verified.** Anything touching the wire stays provisional until its hardware smoke item (PROT-005.11, PROT-006.16) passes against a real Quad Cortex. Only `cortex version` has been round-tripped against hardware so far.

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
- [ ] **PROT-004.2**: `Grid` model - rows, columns, blocks, the row-numbering trap (0-based API, 1-4 on screen)
- [ ] **PROT-004.3**: `Block` struct (row, column, model_id, params)
- [ ] **PROT-004.4**: `Scene` struct (index, label, color, bypass state)
- [ ] **PROT-004.5**: `Catalog` - parse the device's ModelRepo payload (gzip(tar(ModelRepo.xml)), ~47 KB) into a model-id-to-metadata map
- [ ] **PROT-004.6**: Helper functions: `blocks()`, `splits()`, `slot_to_position()`, `position_to_slot()`, `input_level_db()`, `db_to_input_level()`, `free_rows()`, `row_status()`
- [ ] **PROT-004.7**: Constants: `UNITY_LEVEL`, `USER_SETLIST_ROOT`, `SCENE_UNLABELLED`, `BANKS`, `SLOTS_PER_BANK`, `SETLIST_SLOTS`

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
- [ ] **PROT-005.11**: Hardware smoke test - connect, verify state pushes flow, disconnect cleanly (PROVISIONAL until tested against real device)

### PROT-006: Client API (zone 150)

The ergonomic `QuadCortex` struct - the Rust equivalent of pyquadcortex's 60+ methods.

- [x] **PROT-006.1**: `QuadCortex` struct wrapping `Arc<Session>`, lifecycle (connect/disconnect/close, Drop)
- [x] **PROT-006.2**: `version()` - wired through `Session::request` (correlated by type, no request_id echo)
- [ ] **PROT-006.3**: Catalog property - lazily fetch and parse ModelRepo
- [~] **PROT-006.4**: Read operations. Done: `read_current_preset` (live grid, no side effects), `read_preset` (recall + capture the echoing push), `active_scene`, `list_presets`, `find_preset`, `list_folders` (via `collect`), plus the `PresetEntry`/`Folder` value objects. Planned: `captures`, `list_irs`, `recents`, `favorites`, `pinned_models`, `master_volume`, `looper`, `tuner`, `io_settings`, `settings`, `global_eq`, `mode`
- [x] **PROT-006.5**: Navigation: `recall_preset`, `switch_scene` (implemented); `copy_scene`, `set_scene_label`, `set_scene_color` (planned)
- [ ] **PROT-006.6**: Grid write: `set_param` (with scene/promote/text/real), `set_bypass`, `set_block` (with verify), `remove_block`, `move_block`, `set_chain_input`, `set_chain_output`, `write_preset` (low-level)
- [ ] **PROT-006.7**: Splitter/mixer/lane/gate: `set_splitter_param`, `set_mixer_param`, `set_lane_output`, `set_input_gate`, `set_split`, `set_split_mute`
- [ ] **PROT-006.8**: Tempo: `set_tempo_param`, `set_tempo_option`, `set_tempo_subdivision`, `set_metronome_sound`, `set_metronome_routing`, `set_time_signature`, `set_tempo_led`, `set_metronome_volume`
- [ ] **PROT-006.9**: Stomp/expression/MIDI: `set_stomp_assignment`, `clear_stomp_assignment`, `set_stomp_momentary`, `set_stomp_label`, `set_expression`, `set_expression_bypass`, `set_midi_out`, `set_preset_load_midi_out`
- [ ] **PROT-006.10**: File ops: `save_current_preset`, `delete_preset`, `move_preset`, `create_setlist`, `delete_setlist`, `copy_preset`, `duplicate_setlist`, `wait_for_listing`
- [ ] **PROT-006.11**: Captures/IRs: `set_capture`, `set_ir`, `show_capture_dialog`
- [ ] **PROT-006.12**: Global settings: `update_settings`, `set_hold_timing`, `set_scene_bypass_behavior`, `set_master_volume_assignment`, `set_global_bypass`, `set_global_eq`, `set_mode_cycle`, `set_mode`, `set_gig_view`, `show_tuner`, `set_tuner_input`, `set_tuner_mute`, `set_tuner_reference`
- [ ] **PROT-006.13**: I/O ports: `set_input_port`, `set_output_port`, `set_usb_port`, `set_midi_thru`, `set_output_pairing`
- [ ] **PROT-006.14**: Pinning/favorites: `pin_model`, `unpin_model`, `add_favorite`, `remove_favorite`
- [ ] **PROT-006.15**: Module-level helpers: `blocks()`, `splits()`, `input_chain_rows()`, `stomp_assignments()`, `midi_out()`, `tempo_params()`, `param_options()`, `free_rows()`, `row_status()`, `params_equal()` - `slot_to_position`, `position_to_slot`, `input_level_db`, `db_to_input_level` done
- [ ] **PROT-006.16**: Hardware smoke test - exercise recall, read_preset, set_param, save against a real Quad Cortex

## CLI (CLI)

The `cortex` command-line surface over the crate.

### CLI-001: Scaffold and version

- [x] `cortex version` - reads device firmware, prints all fields
- [x] `cortex completions <shell>` - bash, zsh, fish, powershell
- [x] `cortex --version` / `-V` - standard version flag
- [x] SIGPIPE reset, `arg_required_else_help`
- [x] Clap derive, thin main.rs, all behaviour in crate

### CLI-002: Format and output

- [ ] **CLI-002.1**: `--format text|json` global flag, honoured by every command
- [ ] **CLI-002.2**: `cortex version --format json` - structured JSON output
- [ ] **CLI-002.3**: `--schema` / `--print-schema` - JSON Schema of a command's inputs
- [ ] **CLI-002.4**: Data on stdout, hints on stderr (house-style invariant)

### CLI-003: Preset and scene commands

- [ ] **CLI-003.1**: `cortex recall --setlist <path> --slot <slot>` - recall a preset
- [ ] **CLI-003.2**: `cortex scene --index <n>` - switch active scene
- [ ] **CLI-003.3**: `cortex dump-preset --setlist <path> --slot <slot>` - recall and print full BinaryPreset as text or JSON
- [ ] **CLI-003.4**: `cortex list-presets --setlist <path>` - list presets in a setlist
- [ ] **CLI-003.5**: `cortex list-folders` - list all folders the device knows

### CLI-004: Distribution

- [ ] **CLI-004.1**: `s/version++` script - bump version across Cargo.toml in one release commit
- [ ] **CLI-004.2**: auto-tag workflow - version bump on main creates `v<x.y.z>` tag
- [ ] **CLI-004.3**: crates.io publish workflow (PROT layers must be complete first)
- [ ] **CLI-004.4**: cargo-dist release pipeline (archives + installers)
- [ ] **CLI-004.5**: Shell completions install command (`cortex completions install`)

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

### ENG-004: Traceability

- [ ] **ENG-004.1**: Add `@see` traceability headers to all owned source files linking to zone specs
- [ ] **ENG-004.2**: CI gate for `@see` link resolution (optional, low priority)

## Future

- **FUTURE-001**: Nano Cortex hardware verification - plug in the Nano, verify the protocol shape holds, record the product ID, promote `DeviceKind::NanoCortex` from provisional to verified
- **FUTURE-002**: Nano Cortex BLE protocol - the Nano uses BLE for control telemetry; the deskop-nano-cortex project has a provisional decode (Apache-2.0)
- **FUTURE-003**: Tauri desktop GUI (zone 400) - React + Mantine + Vite, a consumer of the crate; use Tauri MCP to tighten the feedback loop
- **FUTURE-004**: On-device builds (qc-stomp-tools ioctl route) - only if there is a compelling reason; the USB route is preferred
- **FUTURE-005**: Protocol-version probe - surface a CorOS version check rather than hard-coding assumptions, since the protocol has no version field on the wire
- **FUTURE-006**: Conformance suite - port pyquadcortex's offline test suite as a Rust integration test reference