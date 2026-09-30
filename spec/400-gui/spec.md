---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["gui", "tauri", "react", "mantine", "vite", "in-progress", "accessible"]
---

# 400 GUI - Spec (in progress)

> The cross-platform Tauri 2 desktop app: a consumer of the shared Rust engine and host boundary, not a second implementation. Explicit fixture and daemon-backed Tauri modes expose non-persistent Quad and Nano working-state operations; the GUI has no save or delete action.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at (`[Flow.GUI]`).
- [AGENTS.md](../../AGENTS.md) - the architecture and the "Rust owns behaviour; the webview owns interaction" rule.
- [house-style tauri-gui.md](https://github.com/marcus-pacharanero/house-style/blob/main/tauri-gui.md) - the GUI stack, project shape, and rules this surface will follow.
- [house-style ui.md](https://github.com/marcus-pacharanero/house-style/blob/main/ui.md) - the presentation and interaction standards the frontend will follow.
- [300-mcp spec](../300-mcp/spec.md) - the safety surface design the GUI will reuse (factory-setlist refusal, exact target, slot backup, explicit confirmation, trap-surfacing).
- [150-client spec](../150-client/spec.md) - the `QuadCortex` client API the Tauri backend will call.
- Owned source: `gui/`, `docs/gui/`, `s/gui-dev`.

## Problem Statement

The GUI is the interactive surface for a player who wants a desktop editor for the Quad Cortex. The project is Linux-first because Linux has no official editor and is the only host verified here today, but the intended Tauri product supports Linux, Windows and macOS once each platform is implemented, packaged and tested. The Tauri backend will call `cortex-host` for daemon-owned operations and use shared `cortex-rs` types and behavior; the React webview renders typed results and owns view state, forms, layout, and keyboard interaction.

The live read/edit and shared save-safety foundations are present: typed serialisable views and parameter inputs live in `cortex-rs`, every ordinary CLI operation routes through one held session, subscribed state is reduced into generation/revision snapshots, health/reconnect invalidates stale state, and `safety.rs` supplies exact-target authorisation plus opaque prepared-save tokens. Core save/reconnect correctness blockers are closed. GUI save remains absent until the GUI implements exact-target preparation and confirmation UX, restoration semantics, typed failures and its own hardware smoke.

**`safety.rs` is enforced by the daemon.** The CLI routes through `PrepareSave`/`CommitSave` with server-held preparations and opaque tokens. The GUI must call that enforced API and present its policy clearly; it must never call the unsafe primitive directly.

## Stack (per house-style tauri-gui.md)

- **Tauri 2** - the desktop shell; Rust backend, webview frontend.
- **React + TypeScript** - the frontend.
- **Mantine** - UI primitives.
- **Vite** - development and builds.
- **`cortex-host`** - the implemented daemon-facing command and local IPC boundary used by Tauri.
- **`cortex-rs`** - shared typed views and domain behavior used by the Rust backend; no protocol logic is implemented in TypeScript.

## Project shape (current scaffold, per house-style tauri-gui.md)

```text
gui/
|-- package.json
|-- vite.config.ts
|-- src/          # React + TypeScript frontend
`-- src-tauri/
    |-- Cargo.toml
    |-- tauri.conf.json
    `-- src/       # managed daemon-backed Rust command boundary
docs/gui/         # how to use and run the GUI
spec/400-gui/     # this zone
s/gui-dev         # run the Tauri dev server from any working directory
```

## Visual Design Goal: Device Editor Console

The primary desktop workflow is an editor console, not a virtual rendering of either pedal's physical front panel. [NanoDesk](https://github.com/grliszas14/NanoDesk-releases) is useful inspiration for this direction: it puts topology, preset navigation, and the selected block's controls in one focused working surface. Its release repository does not publish source, so this project implements an independent layout and never reuses its code, artwork, screenshots, or UI assets.

The planned visual model:

- **A quiet session bar** shows the selected device, connection state, current preset when the device reports one, and only the global actions that the active device supports.
- **A topology overview** directly below the session bar makes the signal path immediately scannable and selects a block. The Quad overview remains its routed 4x8 grid; the Nano overview remains its ordered fixed eight-role chain. Neither is coerced into the other device's model or faked as a physical front panel.
- **A contextual navigator** keeps the most frequent device-specific browsing action available without leaving the editor. Quad uses the typed preset directory when it is available; Nano gains preset navigation only after its own inventory and recall contracts exist.
- **A selected-block workspace** is the visual centre: model/role, bypass state, direct controls, and the parameter inspector occupy the main area. Fine numeric edits use the shared control language and exact Rust-owned units/ranges.
- **Focused utilities** such as block replacement, scene management, IR/capture selection, tuner, and settings are disclosed only where the typed device contract supports them. They use clear standard controls rather than simulated hardware modes.
- **Honest device state** drives every view. Bypass state, active scene, parameter values, dirty state, and pending work come from the crate's read/reducer paths; the GUI does not optimistically render a write as confirmed.

The console must work equally well for both devices while preserving their different capabilities. A hardware-style Quad view may later be offered as an optional companion for players who prefer it, but it is not the default workflow and cannot be the only accessible route to an operation.

### Soft Window Geometry

The native desktop window is resizable, not aspect-ratio-locked. Its reference canvas is 16:10 at 1440x900 logical pixels, with a 1080x675 logical-pixel minimum. The header offers 75%, 100%, 125%, 150%, and 200% 16:10 native-size presets for a predictable editor layout. Window managers and users may still choose arbitrary dimensions, so the console must retain its responsive compact fallback rather than assuming the reference ratio. The presets resize the Tauri window in logical pixels and do not change browser zoom, text size, control targets, or device state.

## Requirements

The first draft establishes the stack, mockable frontend API boundary, typed Tauri commands, an accessible 4x8 grid, a parameter inspector and a device-specific Nano fixed-chain surface. The remaining requirements are:

- **Rust owns behaviour.** Tauri commands call `cortex-host` and shared `cortex-rs` APIs, returning typed serialisable data. No protocol/domain logic lives in TypeScript.
- **The webview owns interaction.** View state, forms, layout, keyboard interaction, copy/paste affordances, and presentation live in the React frontend.
- **Device editor console.** The default view combines a quiet session bar, the device's honest topology overview, a contextual navigator where supported, and a selected-block workspace. It must make the frequent inspect, recall, select, bypass, and adjust path clear without copying any third-party UI.
- **Focused utilities for common workflows.** Patch browsing, block replacement, parameter editing, scene management, and IR/capture selection are contextual panels or navigators, not simulated hardware modes.
- **Optional hardware companion.** A future Quad front-panel view may expose familiar footswitch/encoder context, but it is supplementary and any drag interaction has keyboard and standard-control alternatives.
- **Soft 16:10 geometry.** Use a 1440x900 logical-pixel reference, an 1080x675 minimum, and native 16:10 size presets from 75% through 200%. Support arbitrary resize and high-zoom compact layouts rather than locking the window ratio.
- **Honest capability presentation.** Hardware evidence remains Rust-owned, host-aware engineering metadata and is documented in the roadmap, diagnostics and release notes. Ordinary player controls do not repeat development-maturity badges; unavailable or refused operations remain explicit at the point of action, and the GUI never presents an unimplemented control as usable.
- **Live state comes from the reducer.** The Rust backend owns one subscribed session and exposes typed cache snapshots plus generation/revision changes. The frontend does not pollute its interaction state with optimistic device state and never renders a pre-reconnect generation as current.
- **Reconnect is truthful and actionable.** Reconnecting state shows the daemon's real attempt count and last error. A manual retry interrupts automatic backoff but does not mark the device live before a complete replacement handshake.
- **Bounded host-use release.** The GUI holds only short-lived request sockets, so window/process close releases its host use without stopping a shared daemon. A request already accepted remains daemon-side in flight through completion; the GUI never sends global `Shutdown` merely because its window closed.
- **Fast dual-device selection.** Quad and Nano use product-scoped local endpoints and may remain warm concurrently when both physical devices are connected. Switching views never kills an explicit daemon or gives either physical HID interface a second owner.
- **Shared editor language, honest topology.** Quad's routed 4x8 grid and Nano's fixed eight-role chain use the same semantic block cards, selection states and inspector framing without coercing either product into the other's domain model.
- **Product-specific shell.** Quad-only navigation is absent in Nano mode. At the supported minimum window, the Nano's complete fixed eight-role chain remains visible as one ordered row without horizontal scrolling; high zoom may overflow rather than folding the hardware topology.
- **Consistent continuous controls.** Numeric editing uses a labelled slider plus editable numeric stepper with increment/decrement controls where a typed range exists. Both controls share one draft value and commit through the same device-confirmed write path; the frontend does not invent ranges, units or resolution.
- **Rust-owned Nano model names.** Nano FX cards render product-facing names resolved by `cortex-rs`; React does not own a second model table, and unknown firmware ids remain explicit rather than receiving a guessed Quad name.
- **Rust-owned Nano parameter names.** Nano FX controls render the semantic label carried by the Rust boundary while retaining the explicit zero-based wire index. Unknown models and surplus parameters fall back to `Param <index>`; React does not infer units, ranges, enums or normalized-value conversions.
- **Safety surface reuse.** The GUI reuses the same rules as the MCP server: absolute factory refusal, one exact target, pre-edit preparation/backup for that target, explicit confirmation, and trap-surfacing. If a target was not prepared before the grid became dirty, the GUI requires another target rather than recalling the original target and destroying the edits.
- **`s/gui-dev`** runs the Tauri dev server from any working directory (house-style tauri-gui.md).
- **Versioning with the repo.** `gui/package.json` and `tauri.conf.json` versions move with the canonical version via `s/version++`.
- **Same terminology as the domain specs and docs.** The UI uses the same terms as the CLI, MCP server, and public documentation. A future glossary may centralise them.
- **Per-panel fault isolation.** A thrown frontend error in one editing panel is visible - naming the panel and offering a reload - rather than silently unmounting that panel while the daemon and its writes keep working.

## Acceptance Criteria

- [x] `gui/` exists with the Tauri 2 + React + Mantine + Vite stack.
- [x] `s/gui-dev` runs the Tauri dev server from any working directory.
- [x] Tauri commands call the shared host/core boundary and return typed serialisable data; no protocol/domain logic lives in TypeScript. Fixture and Tauri modes implement the same contract.
- [x] One managed Rust backend exposes generation/revision-tagged daemon snapshots; reconnecting/failed status is visible and old generations are never rendered as live.
- [x] The production dashboard boundary returns one live generation with populated blocks and preset directory against a real CorOS 4.0.1 held session on Linux.
- [x] Physical unplug/reconnect hides the old grid and directory within one refresh, then restores the same live preset only under a newer generation.
- [x] With both products connected, the native selector preserves one live owner per device and switches Quad to Nano and back in under one second after initial startup.
- [x] Quad and Nano use one semantic editor-canvas component set while preserving their distinct fixed topologies and keyboard-selectable blocks.
- [ ] The default view is a device editor console with a session bar, the product's honest topology overview, contextual navigation where available, and a selected-block workspace.
- [ ] The topology overview supports keyboard-operable selection and keeps selected, active, bypassed, and unavailable state explicit without relying on colour or position alone.
- [ ] Quad and Nano share the console hierarchy while retaining their distinct topology and only showing operations their typed contracts support.
- [ ] Focused utilities (patch browser, block palette, parameter inspector, scene manager, IR/capture loader) are accessible without mode simulation or a pointer-only path.
- [x] Hardware evidence is retained in the Rust capability matrix and engineering documentation without repeated development-maturity badges in the ordinary player workflow.
- [x] At 800x600, Nano mode reserves no Quad-only preset sidebar and shows all eight fixed roles in one row without horizontal scrolling.
- [ ] Continuous numeric controls use the shared slider-plus-stepper pattern wherever an honest typed range is available.
- [ ] A save action reuses the shared prepared-save surface (factory refusal, exact target, pre-edit backup, explicit confirmation).
- [x] `gui/package.json`, `package-lock.json`, and `tauri.conf.json` versions move with `s/version++`.
- [x] `docs/gui/` explains explicit fixture and daemon run modes, state freshness, non-persistent working-state edits, and the absent save boundary.
- [x] A thrown error in the scene selector, grid, or inspector renders a visible per-panel failure (naming the panel, offering a reload) instead of silently unmounting, proven by an automated test that throws.

## Non-Goals

- **Protocol or domain logic.** Owned by the crate (zones 100-150). The GUI is a consumer.
- **Bypassing the prepared-save contract.** The daemon retains `SavePreparation` and exposes only opaque token/views. The Tauri backend and frontend must never serialise a raw backup or call `save_current_preset` directly.
- **A second implementation of the safety surface.** The safety rules belong in a shared module the CLI, MCP server, and GUI all reuse.
- **Mobile breakpoints.** The GUI is a desktop app; test the supported minimum, default, and large window sizes (house-style tauri-gui.md).

## Dependencies

- **`cortex-host`** - the typed daemon client and local IPC abstraction.
- **`cortex-rs`** - shared types and behavior used beneath the daemon.
- **Tauri 2** - the desktop shell.
- **React + TypeScript** - the frontend.
- **Mantine** - UI primitives.
- **Vite** - development and builds.
- **house-style tauri-gui.md** - the stack, project shape, and rules.
- **house-style ui.md** - the presentation and interaction standards.
- **Zone 150 (client)** - the `QuadCortex` API the backend calls.
- **Zone 300 (MCP)** - the safety surface design the GUI reuses.

## Next

- **Editor console.** Establish the shared session bar, topology overview, contextual navigator, and selected-block workspace without adding persistent writes.
- **E2E tests.** Add browser/Tauri workflow automation as the interaction surface grows; avoid brittle visual snapshots.
- **Nano Cortex specifics.** Provisional until verified against real hardware; the GUI labels them.

## Glossary

| Term | Definition |
| --- | --- |
| First draft | The stack and interactive shell exist with explicit fixture and daemon-backed modes; implemented working-state edits are non-persistent and save/delete workflows do not exist. |
| Tauri command | A Rust function exposed to the webview; calls `cortex-host`, uses shared `cortex-rs` views, and returns typed serialisable data |
| `s/gui-dev` | Repo script that runs the Tauri dev server from any working directory |
| Safety surface reuse | The GUI gates saves through the same prepared-target contract as the MCP server (factory refusal, exact target, pre-edit backup, explicit confirmation) |

## Related roadmap items

- **[DOCS-002](../roadmap.md)** - the factory preset reference (what each factory preset evokes, and how to set it up) is aimed at agents driving the MCP server, but the GUI wants the same data to annotate the patch browser. Build it as a shared, generated artefact rather than duplicating it per surface.
- **[PROT-007](../roadmap.md)** - capture and IR export/import are deliberate non-features. Capture and IR creation, transfer, backup, and cloud processing remain native-device workflows; selecting existing device captures and IRs in compatible blocks remains in scope. The GUI must not present an unsupported backup or import path.
- **[FUTURE-007](../roadmap.md)** - audio feedback. If the analysis subsystem is built, the GUI is where its output belongs (a spectrum or gain-staging readout beside the grid), not the CLI.
