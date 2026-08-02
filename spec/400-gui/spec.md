---
afx: true
type: SPEC
status: Deferred
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["gui", "tauri", "react", "mantine", "vite", "deferred"]
---

# 400 GUI - Spec (deferred stub)

> The Tauri 2 desktop app: a consumer of the `cortex-rs` crate, not a second implementation of the product. **Deferred** until the crate (zones 100-150), the CLI (200), and the MCP server (300) are complete. This is a stub spec; no `design.md` yet.

## References

- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this surface sits at (`[Flow.GUI]`).
- [AGENTS.md](../../AGENTS.md) - the architecture and the "Rust owns behaviour; the webview owns interaction" rule.
- [house-style tauri-gui.md](https://github.com/marcus-pacharanero/house-style/blob/main/tauri-gui.md) - the GUI stack, project shape, and rules this surface will follow.
- [house-style ui.md](https://github.com/marcus-pacharanero/house-style/blob/main/ui.md) - the presentation and interaction standards the frontend will follow.
- [300-mcp spec](../300-mcp/spec.md) - the safety surface design the GUI will reuse (factory-setlist refusal, scratch range, slot backup, trap-surfacing).
- [150-client spec](../150-client/spec.md) - the `QuadCortex` client API the Tauri backend will call.
- Planned source: `gui/` (does not exist yet).

## Problem Statement

The GUI is the interactive surface for a player who wants a desktop editor for the Quad Cortex on Linux (where Neural DSP ships no official editor). It is a consumer of the `cortex-rs` crate: the Rust backend calls the crate's `QuadCortex` client and returns typed serialisable data to the frontend; the React webview renders it and owns view state, forms, layout, and keyboard interaction.

The GUI is deferred until the crate and the thinner surfaces (CLI, MCP) are complete. Starting it now would mean the GUI reimplementation of protocol/domain logic drifting from the crate, or blocking on crate APIs that do not yet exist. The deferred status is deliberate.

## Stack (planned, per house-style tauri-gui.md)

- **Tauri 2** - the desktop shell; Rust backend, webview frontend.
- **React + TypeScript** - the frontend.
- **Mantine** - UI primitives.
- **Vite** - development and builds.
- **`cortex-rs`** - the crate the Tauri backend calls (the single implementation of protocol and domain logic).

## Project shape (planned, per house-style tauri-gui.md)

```text
gui/
|-- package.json
|-- vite.config.ts
|-- src/          # React + TypeScript frontend
`-- src-tauri/
    |-- Cargo.toml
    |-- tauri.conf.json
    `-- src/       # Rust backend (Tauri commands calling cortex-rs)
docs/gui/         # how to use and run the GUI
spec/400-gui/     # this zone
s/gui-dev         # run the Tauri dev server from any working directory
```

## Visual Design Goal: Hardware-Faithful Control Surface

The Quad Cortex front panel has **10 footswitches that double as rotary encoders** - the player presses them to toggle bypass / recall scenes / navigate, and turns them to adjust the parameter of the block in that grid column. This is the primary tactile interface, and the GUI should emulate it graphically so a player who knows the hardware immediately knows where things are.

The planned visual model:

- **A faithful rendering of the Quad Cortex front panel** - the 10 footswitch/encoder positions, the colour OLED grid display, the scene LEDs, and the context strip along the top. The player sees a virtual Quad Cortex on screen, not a generic "editor window".
- **Click-to-press and drag-to-turn** on each footswitch/encoder. A click toggles bypass (or recalls a scene, or navigates a menu, depending on mode). A vertical drag or scroll adjusts the encoder value. Keyboard equivalents for accessibility.
- **The grid display** mirrors the device's OLED - the current signal chain, block icons, bypass state, and the active scene. This is the same `Grid` / `Block` / `Scene` domain model the crate owns (zone 130), rendered.
- **Wrapper layers for common workflows** on top of the hardware-faithful view:
  - **Patch browser** - a setlist/slot grid for quick preset switching (the `list_presets` + `recall_preset` path), with search and favourites.
  - **Block palette** - a searchable list of available models (from the `Catalog`) to drag onto a grid cell.
  - **Parameter inspector** - a form-based editor for the selected block's parameters, showing real units (dB, ms, Hz) via the catalog's range conversion. This is for fine edits where an encoder emulation is too coarse.
  - **Scene manager** - copy/swap/relabel/recolor scenes without the footswitch mode dance.
  - **IR / Capture loader** - file-browser-style access to the device's captures and IRs.
- **Mode-aware footswitch labels** - the footswitches change meaning with the device mode (Preset / Stomp / Scene / Looper / Tuner); the GUI reflects the current mode and labels the switches accordingly, same as the hardware's context strip.
- **Honest state on the virtual panel** - the GUI shows what the device reports, not what the GUI thinks it sent. Bypass state, active scene, and parameter values come from the crate's read paths (or the device's live pushes via the session layer); the GUI does not optimistically render a write and assume it took.

The hardware-faithful view is the default; the wrapper panels are tabs or sidebars. A player who only wants to recall presets and tweak a knob never leaves the panel view; a player doing a complex edit drops into the parameter inspector or block palette.

## Requirements (planned, not yet numbered)

When this zone is unblocked, the spec will own:

- **Rust owns behaviour.** Tauri commands call `cortex-rs` and return typed serialisable data; the frontend renders it. No protocol/domain logic in TypeScript.
- **The webview owns interaction.** View state, forms, layout, keyboard interaction, copy/paste affordances, and presentation live in the React frontend.
- **Hardware-faithful control surface.** The default view is a graphical emulation of the Quad Cortex front panel: 10 footswitch/encoder positions, the OLED grid, scene LEDs, and the context strip. Click-to-press, drag-to-turn, with keyboard equivalents.
- **Wrapper panels for common workflows.** Patch browser, block palette, parameter inspector, scene manager, and IR/capture loader sit alongside the hardware view as tabs or sidebars.
- **Mode-aware footswitch labels.** The virtual footswitches reflect the current device mode and label themselves accordingly.
- **Honest verified-vs-provisional labelling.** The GUI labels hardware-verified behaviour vs provisional surfaces (Nano Cortex specifics, unknown message types) in the UI, following the `deskop-nano-cortex` discipline.
- **Safety surface reuse.** The GUI reuses the same safety rules as the MCP server (factory-setlist refusal, scratch range, slot backup, trap-surfacing). A save action in the GUI is gated the same way a `save_preset` tool call is gated.
- **`s/gui-dev`** runs the Tauri dev server from any working directory (house-style tauri-gui.md).
- **Versioning with the repo.** `gui/package.json` and `tauri.conf.json` versions move with the canonical version via `s/version++`.
- **Same terminology as `CONTEXT.md` and docs.** The UI uses the same terms as the CLI, MCP server, and docs (house-style tauri-gui.md).

## Acceptance Criteria (deferred)

- [ ] `gui/` exists with the Tauri 2 + React + Mantine + Vite stack.
- [ ] `s/gui-dev` runs the Tauri dev server from any working directory.
- [ ] Tauri commands call `cortex-rs` and return typed serialisable data; no protocol/domain logic in TypeScript.
- [ ] The default view is a hardware-faithful rendering of the Quad Cortex front panel (10 footswitch/encoders, OLED grid, scene LEDs, context strip).
- [ ] Footswitch/encoders are interactive: click-to-press (toggle/recall/navigate), drag-to-turn (adjust parameter), with keyboard equivalents.
- [ ] The virtual panel reflects the current device mode and labels footswitches accordingly.
- [ ] Wrapper panels (patch browser, block palette, parameter inspector, scene manager, IR/capture loader) are accessible as tabs or sidebars.
- [ ] The GUI labels hardware-verified vs provisional surfaces in the UI.
- [ ] A save action in the GUI reuses the safety surface (factory refusal, scratch range, slot backup).
- [ ] `gui/package.json` and `tauri.conf.json` versions move with `s/version++`.
- [ ] `docs/gui/` explains how to use and run the GUI.

## Non-Goals

- **Protocol or domain logic.** Owned by the crate (zones 100-150). The GUI is a consumer.
- **Starting the GUI before the crate and CLI are complete.** The deferred status is deliberate; starting now would block on APIs that do not yet exist.
- **A second implementation of the safety surface.** The safety rules belong in a shared module the CLI, MCP server, and GUI all reuse.
- **Mobile breakpoints.** The GUI is a desktop app; test the supported minimum, default, and large window sizes (house-style tauri-gui.md).

## Dependencies

- **`cortex-rs`** - the crate the Tauri backend calls.
- **Tauri 2** - the desktop shell.
- **React + TypeScript** - the frontend.
- **Mantine** - UI primitives.
- **Vite** - development and builds.
- **house-style tauri-gui.md** - the stack, project shape, and rules.
- **house-style ui.md** - the presentation and interaction standards.
- **Zone 150 (client)** - the `QuadCortex` API the backend calls.
- **Zone 300 (MCP)** - the safety surface design the GUI reuses.

## Future

- **`design.md`.** Written when this zone is unblocked. Progress is tracked in [roadmap.md](../roadmap.md) under GUI-00x.
- **`docs/gui/`.** How to use and run the GUI.
- **E2E tests.** GUI smoke tests focused on the user workflows that prove the Rust/frontend boundary is wired (house-style tauri-gui.md). Avoid brittle visual snapshots.
- **Nano Cortex specifics.** Provisional until verified against real hardware; the GUI labels them.

## Glossary

| Term | Definition |
| --- | --- |
| Deferred | This zone is not started; the spec is a stub. The crate, CLI, and MCP server come first. |
| Tauri command | A Rust function exposed to the webview; calls `cortex-rs` and returns typed serialisable data |
| `s/gui-dev` | Repo script that runs the Tauri dev server from any working directory |
| Safety surface reuse | The GUI gates saves the same way the MCP server does (factory refusal, scratch range, slot backup) |
## Related roadmap items

- **[DOCS-002](../roadmap.md)** - the factory preset reference (what each factory preset evokes, and how to set it up) is aimed at agents driving the MCP server, but the GUI wants the same data to annotate the patch browser. Build it as a shared, generated artefact rather than duplicating it per surface.
- **[PROT-007](../roadmap.md)** - capture and IR export/import. The GUI is the natural home for a "back up my captures" workflow, since it is the surface a player already has open when they care about their captures.
- **[FUTURE-007](../roadmap.md)** - audio feedback. If the analysis subsystem is built, the GUI is where its output belongs (a spectrum or gain-staging readout beside the grid), not the CLI.
