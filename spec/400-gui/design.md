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

## [DES-BOUNDARY] One Engine, One Owner Per Device

The production flow is `React -> named Tauri command -> managed Rust AppState -> cortex-host::DaemonClient -> held cortex session -> cortex-rs -> USB HID`. The GUI never opens HID, constructs `QuadCortex`, sends arbitrary daemon requests from TypeScript, or reimplements protocol/domain behavior in the webview.

`AppState` owns one reusable `DaemonClient` per product endpoint. Connections remain short-lived per dashboard request; each product daemon serves accepted connections concurrently, and request completion is the lifecycle activity boundary shared by GUI, CLI, and MCP. A daemon remains the sole long-lived USB owner of its physical device. Quad and Nano may each have one owner concurrently because they are distinct USB devices; no physical HID interface gains a second owner.

Quad retains the legacy `cortex.sock` endpoint and Nano uses `cortex-nano.sock`; lock and log names follow those sockets. The GUI never shuts one daemon down to view the other. It starts or reuses the selected product session and sends a status request with a bounded 500 ms timeout to the inactive endpoint during the ordinary dashboard poll, keeping both auto-managed sessions warm while preventing an in-progress inactive handshake from blocking the selected dashboard. Explicit daemons are reused without requiring the sibling CLI to exist and are never replaced. Startup gates are product-scoped, auto-detect retains Quad-first behavior, and an explicit selection routes only to that product endpoint.

Window close needs no host-use lease or Tauri-specific shutdown protocol. Each GUI operation owns a short-lived socket; process/window close drops it within the operating system's bounded process teardown. If the daemon has already parsed that request, daemon-side in-flight accounting protects it through completion and then starts the ordinary idle period. The GUI must not send daemon `Shutdown` on close because an explicit daemon or another concurrent CLI/MCP user may own the same session.

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

React owns polling, loading/error presentation, selected cell identity, layout and accessibility. It starts the next one-second refresh only after the previous request settles, so requests cannot overlap. Selection is stored as `{row,column}` and resolved against the newest snapshot; stale block objects are never retained. Device choices carry an epoch so a slower earlier request cannot overwrite the final selection. Reconnecting state shows the daemon's attempt count and last error plus a **Reconnect now** action; it does not invent a determinate progress bar because the daemon uses bounded exponential backoff rather than a fixed-duration operation.

The daemon-backed GUI shows health, cache generation/revision, the Quad live grid, active scene, CPU total and per-core columns, populated setlist slots, and the Nano fixed signal chain. Quad and Nano retain separate domain models but share semantic editor-canvas block cards, operational-state text, selection treatment and inspector framing. Nano FX display names arrive in the typed snapshot after Rust resolves the Nano-specific model id; the webview only chooses the device-supplied Capture/IR name, resolved FX name, or explicit unknown-id fallback. Fixed topologies scroll rather than reflowing into false routes at narrow windows or high zoom. It exposes typed non-persistent recall and working-state edits for the implemented Quad and Nano operations. Persistent save/delete actions remain outside this milestone until the GUI implements the shared safety contract and confirmation UX.

## [DES-TESTING] Boundary Evidence

Rust tests exercise scene-label conversion, connected/cache readiness, retry signalling, and failure propagation without fixture fallback. Frontend checks compile both adapters against one strict TypeScript contract; component tests cover topology, semantic card state, keyboard-native selection and all 32 Quad positions. The repository Rust and frontend gates run together locally and in CI. Hardware verification on 2026-08-09 unplugged the USB data cable while the native GUI was live: within one second the grid and directory disappeared, generation 1 remained visibly invalid, automatic recovery returned the same eight-block preset after about 10 seconds, and the rendered cache advanced to generation 2. A second run displayed the real failed-attempt progression, accepted **Reconnect now** at attempt 4 without a UI error, completed the replacement handshake in about three seconds and again restored the preset under generation 2; the focused offline timing test separately proves the signal interrupts a 10-second wait in under one second. Dual-device verification on 2026-08-25 started both auto-managed product daemons, confirmed both remained connected concurrently, and measured Quad to Nano and the warm return to Quad at under one second each; the previous teardown/reconnect design took 5-8 seconds to return to Quad.

## [DES-CAPABILITY] Capability Matrix Defaults To Unverified

`gui/src-tauri/src/capability.rs` carries the honest verified-vs-provisional labelling AGENTS.md calls for, following the `deskop-nano-cortex` precedent recorded in [prior-art.md](../prior-art.md#the-idea-most-worth-stealing). `CapabilityStatus` is `confirmed-readable`, `confirmed-writable`, `inferred`, `unsupported`, or `unverified`, and `unverified` is the `Default` - both for the enum and for `CapabilityMatrix::status` on any operation key the matrix does not contain. An unlabelled surface is exactly the one likely to be wrong, so the type makes silence read as unverified rather than as confirmed by omission.

`default_matrix()` is seeded only from operations with a recorded hardware pass elsewhere in this repository (`spec/roadmap.md`), never from "it is implemented" or "it passed offline/fixture verification". `set_bypass`, `set_scene_label` and `set_scene_color` are the worked example: all three exist, are exercised by Rust unit tests, and are offline/fixture-verified - and all three stay `unverified` here, because GUI-003.3 and GUI-003.4 record their hardware follow-up as still needed. Promoting an operation requires editing this seed alongside the roadmap entry that supplies the hardware evidence, not on the strength of the code appearing to work.

This first slice is not yet consulted by any Tauri command or rendered in the webview. GUI-004.2 therefore remains partial: its governing acceptance criterion requires the labels in the UI, not only a backend type. The natural consumers are the hardware-faithful panel (GUI-002) and the screen-reader surface (GUI-006), which is why the matrix lives beside the other Tauri backend types rather than as a one-off; completing the item requires exposing it through the Tauri boundary and rendering it there.

## [DES-FAULT] Per-Panel Fault Isolation

`gui/src/shared/ErrorBoundary.tsx` wraps the scene selector, grid and inspector panels independently in `App.tsx`. Before this, a thrown render error unmounted `SceneSelector` while the daemon and its writes kept working, which presented as "the keyboard stopped working" rather than an obvious crash - a live incident recorded in GUI-003.4, and the same class of fault a Rules-of-Hooks violation in the parameter editor would also cause. React error boundaries can only be class components with `getDerivedStateFromError`; the fallback names the failed panel, shows the caught error's message, and offers a reload rather than leaving a blank space, and it does not call `console.error` itself so React's own logging of the caught error is not duplicated or suppressed. Each panel gets its own boundary rather than one boundary around the whole dashboard, so a fault in the grid does not also blank the scene selector or inspector.

`ErrorBoundary.test.tsx` (`gui/vitest.config.ts`, jsdom environment) proves the isolation directly against the component: a thrown test child produces the named fallback and a Reload control, `console.error` is still invoked, and a second boundary rendered alongside a failed one keeps showing its own children. This is the first automated frontend test in the repository; `npm run test` (`vitest run`) is wired into `npm run check`, so `s/lint` and CI exercise it the same way they exercise the TypeScript build.

## Known Limits

- The directory currently contains complete setlist listings already known to the daemon; empty and unavailable folders are not represented as empty.
- CPU is optional because the first subscribed push can arrive several seconds after connection.
- The dashboard polls rather than long-polling on reducer revision; an aggregate daemon snapshot/wait request is a later optimisation.
- Typed daemon error codes remain broader MCP/host work. The Tauri boundary supplies stable GUI-level error codes while retaining the daemon message for diagnostics.
