# cortex-rs

An unofficial, Linux-first toolkit for the **Neural DSP Quad Cortex**: a Rust crate that speaks the Cortex Control USB HID protocol, and a `cortex` command-line tool built on it.

!!! warning "Unofficial and unaffiliated"

    This project is not affiliated with, endorsed by, or supported by Neural DSP Technologies. "Neural DSP", "Quad Cortex", and "Nano Cortex" are their trademarks. It is an interoperability client for hardware whose vendor ships no Linux editor. See [Legal and attribution](legal.md).

## What it is

Neural DSP's own editor, Cortex Control, runs on macOS and Windows. There is no Linux build. If you use Linux, your Quad Cortex is a device you can play through but not edit from your computer.

`cortex-rs` closes that gap:

- **A leaf crate** implementing the USB HID transport, framing, protobuf envelope, session handshake, and a typed domain model. It depends on no host application and no async runtime, so one implementation can drive every surface.
- **A CLI** for reading device state, browsing presets, searching the model catalog, and editing the grid.
- **Planned**: an MCP server for agentic patch editing, and a Tauri desktop GUI.

## What it is not

- **Not a replacement for Cortex Control.** It does not do cloud sync, firmware updates, or Neural Capture creation, and it is not trying to.
- **Not finished.** Saving a preset is not implemented, which means every edit is currently transient.
- **Not a jailbreak.** It talks to the device over USB exactly as the official editor does. It does not modify firmware, root the unit, or touch the SD card, and it carries none of the warranty risk that route does.

## Status

The project's central discipline is being honest about what has actually run against real hardware. Throughout these docs:

<span class="status verified">verified</span> means exercised against a real Quad Cortex (CorOS 4.0.1, firmware `d14e`) from this crate's own code.

<span class="status provisional">provisional</span> means implemented and unit-tested, but never run against a device.

<span class="status planned">planned</span> means specified but not built.

| Area | Status |
| --- | --- |
| USB HID transport, framing, message envelope | <span class="status verified">verified</span> |
| Connect handshake, keepalive, correlation | <span class="status verified">verified</span> |
| Reading device version, scene, presets, folders | <span class="status verified">verified</span> |
| Model catalog | <span class="status verified">verified</span> |
| Recall preset, switch scene | <span class="status verified">verified</span> |
| Grid: place block, set parameter, remove block | <span class="status verified">verified</span> |
| Grid: bypass, routing, per-scene values, splits | <span class="status provisional">provisional</span> |
| Saving a preset | <span class="status planned">planned</span> |
| Capture and IR export/import | <span class="status planned">planned</span> |
| MCP server | <span class="status planned">planned</span> |
| Desktop GUI | <span class="status planned">planned</span> |
| Nano Cortex | <span class="status planned">planned</span> |

## Start here

- **[Install](install.md)** - the udev rule, building, and your first command.
- **[Walkthrough](walkthrough.md)** - a tour of the CLI with real captured output.
- **[CLI reference](cli-reference.md)** - every command.
- **[The protocol](protocol.md)** - what we know about the wire, and how we know it.

## A note on how this was built

Almost everything here rests on [`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex), an MIT-licensed Python library that established the protocol against real hardware and recovered the protobuf schema. This project is a Rust port of that work with attribution, not an independent rediscovery. See [Legal and attribution](legal.md).
