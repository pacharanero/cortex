# Cross-Platform GUI

The desktop GUI is one of the toolkit's three user-facing interfaces alongside the CLI and MCP server. It is intended to provide a complete Quad Cortex editor on Linux, Windows and macOS over the same typed Rust core, not to become a separate implementation of device behaviour.

The current first draft is a Quad Cortex-specific, read-only interactive shell with two explicit modes. Browser fixture mode uses fictional development data. Tauri mode calls a managed Rust backend, which reads status, grid, active scene, CPU and populated preset slots through `cortex-host` and the held `cortex session` daemon. It never opens a second HID connection.

Linux is the first and only hardware-verified baseline today because Neural DSP has not provided Cortex Control for Linux and this community project began by filling that gap for ourselves. Windows and macOS transport, local IPC, packaging and hardware testing remain outstanding; the project will not describe them as supported until that evidence exists.

It adapts the mockable IPC-boundary architecture of `rixrix/deskop-nano-cortex` (Apache-2.0), while independently implementing the Quad-specific model and Mantine presentation. See `NOTICE` and `THIRD-PARTY-NOTICES.md`.

Install the platform's [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), then install frontend dependencies in `gui/`.

- Start the device owner with `cortex session start`, then run `s/gui-dev` from anywhere in the repository for daemon-backed Tauri mode.
- Run `npm run dev` or `npm run dev:fixture` inside `gui/` for browser-only fixture mode.
- Run `npm run check` inside `gui/` to type-check and build both adapters.

The mode is never inferred from success or failure. A daemon error remains an error; Tauri mode does not fall back to plausible-looking fixture state. The header identifies fixture mode explicitly.

Daemon snapshots carry physical-session generation and cache revisions. Reconnecting, failed, incomplete and invalidated state hides the old live grid rather than presenting it as current. During reconnect the GUI shows the real daemon attempt count and last error; **Reconnect now** interrupts the current automatic backoff without bypassing the full subscribed handshake. A generation change also clears the selected block. CPU may initially say that it is awaiting a device push because the first subscribed CPU report is delayed.

This milestone is read-only. Preset recall, grid editing and saving remain CLI/MCP operations until their GUI interaction and safety contracts are separately implemented and hardware-tested.
