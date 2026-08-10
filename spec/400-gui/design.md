---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "0.1"
created_at: "2026-08-08T00:00:00.000Z"
updated_at: "2026-08-08T00:00:00.000Z"
tags: ["gui", "tauri", "daemon", "state", "fixtures"]
---

# 400 GUI - Design

## [DES-BOUNDARY] One Engine, One Device Owner

The production flow is `React -> named Tauri command -> managed Rust AppState -> cortex-host::DaemonClient -> held cortex session -> cortex-rs -> USB HID`. The GUI never opens HID, constructs `QuadCortex`, sends arbitrary daemon requests from TypeScript, or reimplements protocol/domain behavior in the webview.

`AppState` owns one reusable backend configuration containing `DaemonClient`. Connections remain short-lived per dashboard request because the daemon currently serves one accepted connection until EOF; a permanently open GUI socket would monopolise the accept loop and starve CLI/MCP clients. The daemon remains the sole long-lived USB owner.

The backend command runs synchronous local IPC on Tauri's blocking pool and returns typed serialisable data. Command failures are structured as `{code,message}`. A daemon failure is never replaced with fixture data.

## [DES-SNAPSHOT] Generation-Checked Read Snapshot

The `dashboard` command returns one `DashboardSnapshot` containing daemon status, an optional live snapshot, and a preset directory. It reads status before and after the constituent cache-backed requests. Live data is returned only when both statuses report a connected device, a `live` cache phase, and the same physical-session generation. A reconnect or generation change therefore returns current status with `live: null` and no directory instead of presenting values from the previous device handle as current.

The snapshot carries generation, ordinary revision, and storage revision. The frontend replaces device state rather than merging it optimistically. A generation change clears selected-cell view state. The preset directory is cached in Rust by `(generation, storage_revision)`, so CPU and knob revisions do not rebuild it.

The current daemon contract exposes status, live preset, active scene, CPU and complete setlist listings as separate requests. The generation guard makes this milestone honest across reconnects; a future aggregate daemon request can strengthen same-revision atomicity without changing the Tauri/frontend contract. `ReconnectNow` is the one reconnect control: it interrupts the daemon monitor's current health-poll or exponential-backoff wait, but the GUI remains non-live until the ordinary subscribed handshake produces a fresh complete generation.

Active scene labels and active-scene bypass state are resolved in Rust. TypeScript receives `screen_row`, `active_scene_label`, and one current `bypassed` value; it does not perform wire/screen conversion or select a scene value from protocol-shaped arrays.

## [DES-MODES] Explicit Fixture And Tauri Modes

Browser development uses Vite mode `fixture`; Tauri uses Vite mode `tauri`. Both implement the same `CortexApi` and `DashboardSnapshot` contract. Unknown modes fail at startup. Fixture mode is visibly labelled and is never an error fallback.

Canonical commands:

```sh
npm run dev:fixture
s/gui-dev
npm run check
```

`npm run check` type-checks and builds both modes. Tauri configuration explicitly selects the Tauri mode for development and production builds.

## [DES-FRONTEND] Interaction State Only

React owns polling, loading/error presentation, selected cell identity, layout and accessibility. It starts the next one-second refresh only after the previous request settles, so requests cannot overlap. Selection is stored as `{row,column}` and resolved against the newest snapshot; stale block objects are never retained. Reconnecting state shows the daemon's attempt count and last error plus a **Reconnect now** action; it does not invent a determinate progress bar because the daemon uses bounded exponential backoff rather than a fixed-duration operation.

The first daemon-backed milestone is read-only. The GUI shows health, cache generation/revision, live grid, active scene, CPU total and per-core columns, and populated setlist slots. Recall, edits and persistent actions remain outside this milestone.

## [DES-TESTING] Boundary Evidence

Rust tests exercise scene-label conversion, connected/cache readiness, retry signalling, and failure propagation without fixture fallback. Frontend checks compile both adapters against one strict TypeScript contract. The repository Rust and frontend gates run together locally and in CI. Hardware verification on 2026-08-09 unplugged the USB data cable while the native GUI was live: within one second the grid and directory disappeared, generation 1 remained visibly invalid, automatic recovery returned the same eight-block preset after about 10 seconds, and the rendered cache advanced to generation 2. A second run displayed the real failed-attempt progression, accepted **Reconnect now** at attempt 4 without a UI error, completed the replacement handshake in about three seconds and again restored the preset under generation 2; the focused offline timing test separately proves the signal interrupts a 10-second wait in under one second.

## Known Limits

- The directory currently contains complete setlist listings already known to the daemon; empty and unavailable folders are not represented as empty.
- CPU is optional because the first subscribed push can arrive several seconds after connection.
- The dashboard polls rather than long-polling on reducer revision; an aggregate daemon snapshot/wait request is a later optimisation.
- Typed daemon error codes remain broader MCP/host work. The Tauri boundary supplies stable GUI-level error codes while retaining the daemon message for diagnostics.
