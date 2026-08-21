# Cross-Platform GUI

The desktop GUI is one of the toolkit's three user-facing interfaces alongside the CLI and MCP server. Its destination is one open-source editor for Quad Cortex and Nano Cortex on Linux, Windows and macOS over the same Rust transport and host foundation, not separate applications that reimplement device behaviour. Shared infrastructure will remain shared, while the Quad's grid and the Nano's fixed signal chain retain honest device-specific domain models and screens.

The current first draft supports working Quad and Nano surfaces through one managed Rust backend. Quad mode reads status, grid, active scene, CPU and populated preset slots and exposes non-persistent recall, scene, parameter and bypass controls. Nano mode renders the fixed eight-role signal chain, exposes explicit Apply controls for the five raw amp values from the typed, paced daemon snapshot, and provides an FX parameter inspector; it deliberately does not force those roles into the Quad grid model. Neither mode opens a second HID connection.

Linux is the first and only hardware-verified baseline today because Neural DSP has not provided Cortex Control for Linux and this community project began by filling that gap for ourselves. Windows and macOS transport, local IPC, packaging and hardware testing remain outstanding; the project will not describe them as supported until that evidence exists.

It adapts the mockable IPC-boundary architecture of `rixrix/deskop-nano-cortex` (Apache-2.0), while independently implementing the Quad-specific model and Mantine presentation. See `NOTICE` and `THIRD-PARTY-NOTICES.md`.

## Choosing a mode

Install the platform's [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), then install frontend dependencies in `gui/`.

- **Daemon-backed mode**, the real thing: run `s/gui-dev` from anywhere in the repository. The GUI reuses an existing compatible held session or starts an auto-managed one itself, trying Quad and then Nano when no session exists. The header device selector can explicitly choose Quad, Nano, or Auto-detect; choosing a different device replaces the existing held session.
- **Browser fixture mode**, for frontend development without hardware: run `npm run dev` or `npm run dev:fixture` inside `gui/`. This serves fictional development data in an ordinary browser tab, with no Tauri backend and no daemon. Add `?device=nano` to the browser URL for the fictional Nano fixture.
- Run `npm run check` inside `gui/` to type-check and build both adapters.

The mode is never inferred from success or failure. A daemon error remains an error; Tauri mode does not fall back to plausible-looking fixture state. The header identifies fixture mode explicitly with a yellow banner.

Daemon snapshots carry physical-session generation and cache revisions. Reconnecting, failed, incomplete and invalidated state hides the old live grid rather than presenting it as current. During reconnect the GUI shows the real daemon attempt count and last error; **Reconnect now** interrupts the current automatic backoff without bypassing the full subscribed handshake. A generation change also clears the selected block. CPU may initially say that it is awaiting a device push because the first subscribed CPU report is delayed.

## What the GUI does today

Everything below is non-persistent: it changes the working copy the unit is currently playing, exactly as pressing a footswitch or turning a knob on the device does, and saves nothing to storage. **There is no save control in the GUI.** Preset saving remains a CLI/MCP operation until its own confirmation UX, restoration semantics and hardware-tested safety contract are implemented here (tracked as GUI-004.1).

### Preset directory and recall

The sidebar lists every setlist and slot the daemon can read, grouped by setlist. Clicking a slot **recalls** it: the unit loads that preset as the new working copy, replacing whatever was there before, exactly as pressing the preset button on the unit does. Recall is offered without confirmation because, unlike save, it writes nothing to the unit's storage - only the working copy changes, and the previous working copy was itself never saved by the GUI.

The clicked slot shows "Recalling..." while in flight; the whole directory is disabled until it completes so a second click cannot race the first. Nothing is updated optimistically - the sidebar's active-preset highlight and the working-grid title reflect what the device reports back after the recall, not the slot that was clicked. A failed recall surfaces as a refresh error rather than a silent no-op.

### Scene selector

A radio group across the unit's eight scenes (A-H) lets you switch which scene is active. Switching a scene is a real, audible change to what the unit is currently playing; nothing is saved. The active selection, letter and label are always read back from the device rather than assumed, so a switch that the unit refuses or redirects elsewhere shows up as the device's actual answer, not the one requested.

Renaming and recolouring the active scene are also available, with a full RGB colour picker rather than the unit's fixed eight-colour palette - hardware confirmed accepting and rendering arbitrary RGB on 2026-08-16 (see [spec/roadmap.md](https://github.com/pacharanero/cortex/blob/main/spec/roadmap.md) for the underlying evidence). Both edits are offline-verified only; a GUI rename or recolour appearing correctly on the physical unit has not yet been separately confirmed. A blank name clears the label rather than writing an empty string.

Copy and swap between scenes are not yet implemented.

### Parameter inspector

Selecting a block on the grid opens its inspector: model name, category, position, and (if the model carries one) Neural DSP's own attribution string for what it is evocative of. A bypass switch engages or bypasses the selected block; like every edit here, bypass reaches the **active scene only**, because that is how the device stores it, and the inspector says so.

Below that, every editable parameter the block's catalog entry describes gets its own control - a slider and number input in real units (dB, ms, Hz, etc.) where the catalog gives a usable range, a dropdown for a named-step switch, or a text field for a string parameter. Read-only meters are shown but not editable. A write is followed by a device read-back, so a clamped or refused value shows what the unit actually holds rather than what was asked for.

Parameter and bypass edits are hardware-verified for the read/write/read-back cycle itself (2026-08-17); parameter search or grouping for models with many parameters is not yet implemented.

### Nano Cortex surface

When the held daemon owns a Nano Cortex, the GUI renders the fixed eight-role signal chain (Gate, Pre FX 1-2, Capture, IR/Cab, Post FX 1-3) as one responsive row of panels, each showing its loaded model name and bypass state. Below that, the five hardware-verified raw amp controls (Gain, Level, Bass, Mid, Treble) each get a 0-255 number input and an explicit **Apply** button. The Nano's roles are never forced into the Quad's grid, scenes or presets.

Each Apply sends the typed amp write to the daemon, which paces a fresh state read after the device's measured six-second settle and confirms the value read back exactly before returning. The control stays disabled during that round-trip, so a second Apply cannot race the first, and the panel re-reads device state rather than displaying an optimistic value. The whole cycle changes heard working state and saves nothing.

Gate/FX bypass is also exposed: a Switch control for each of the six addressable roles (Gate, Pre FX 1-2, Post FX 1-3) toggles bypass on or off. Like amp writes, each toggle sends the typed bypass write to the daemon, which paces a fresh state read after the device's measured six-second settle and confirms the new value before returning. The whole cycle changes heard working state and saves nothing.

Selecting an FX panel opens its parameter inspector. Editable panels are named button controls and support Enter/Space as well as pointer input; the header device selector is also a named native button rather than a pointer-only badge. The inspector first requests the model's normalized parameter values and only enables **Apply** once they are available. The underlying core and daemon/CLI paths were hardware-verified on 2026-08-21 across reads from all five slots, a reversible write/read-back/restoration cycle and a daemon-backed same-value write. The Tauri backend boundary also passed all five reads and a same-value write/read-back against hardware. The rendered native Linux control then loaded Pre FX 2, changed one slider by `0.001`, displayed the device-confirmed result, restored the original through the same control and displayed matching device/draft values with **Apply** disabled. Amp, bypass and FX parameter edits change heard working state; no Nano operation saves a preset.

One decoder caveat: the Gate's "on" state is represented by the absence of field 54 in the state protobuf, so the decoder reports `bypassed = None` (unknown) when the gate is on rather than `Some(false)`. The bypass toggle for the Gate is therefore disabled in the UI when the state reads as unknown; this is a decoder limitation, not a write failure, and the write itself still works.

The header badge identifies which product is connected (Nano Cortex or Quad Cortex) and is also the device selector: click it to switch between Quad Cortex, Nano Cortex, or Auto-detect when both products are connected. Switching stops the current daemon session and starts one for the preferred device, so the dashboard re-reads from the new device on the next refresh.

### Health, reconnect and fault isolation

The header badge shows whether the session is a daemon connection (its live state) or fixture mode. If the device is reconnecting or unavailable, the live panels hide rather than show stale data, and the reconnect panel offers attempt/error detail plus a manual **Reconnect now** control.

The scene selector, grid and inspector each render behind their own error boundary. If one panel's frontend code throws, that panel shows its own failure message and a reload control instead of silently disappearing while the other panels and the daemon connection keep working - which matters because a thrown error does not stop writes from reaching the device, only the panel that would show you they happened.
