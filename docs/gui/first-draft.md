# Cross-Platform GUI

The desktop GUI is one of the toolkit's three user-facing interfaces alongside the CLI and MCP server. It is intended to provide a complete Quad Cortex editor on Linux, Windows and macOS over the same typed Rust core, not to become a separate implementation of device behaviour.

The current first draft is a Quad Cortex-specific, read-only interactive shell. It remains fixture-backed and browser-runnable until the Tauri backend can safely route through the held `cortex session start` daemon without opening a second HID connection.

Linux is the first and only hardware-verified baseline today because Neural DSP has not provided Cortex Control for Linux and this community project began by filling that gap for ourselves. Windows and macOS transport, local IPC, packaging and hardware testing remain outstanding; the project will not describe them as supported until that evidence exists.

It adapts the mockable IPC-boundary architecture of `rixrix/deskop-nano-cortex` (Apache-2.0), while independently implementing the Quad-specific model and Mantine presentation. See `NOTICE` and `THIRD-PARTY-NOTICES.md`.

Run `s/gui-dev` after `npm install` in `gui/`.
