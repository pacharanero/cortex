# cortex

An unofficial toolkit for the **Neural DSP Quad Cortex**, developing a cross-platform desktop GUI alongside a hardware-backed CLI, an MCP server for agentic patch editing, and the reusable Rust core beneath them.

!!! warning "Unofficial and unaffiliated"

    This project is not affiliated with, endorsed by, or supported by Neural DSP Technologies. "Neural DSP", "Quad Cortex", and "Nano Cortex" are their trademarks. It is an interoperability client built by the community for hardware it owns. See [Legal and attribution](legal.md).

## What it is

The project is one device model with several interfaces:

- **A desktop GUI targeting Linux, Windows and macOS**, built with Tauri, React and Mantine. Its read-only first draft has explicit browser-fixture and daemon-backed Tauri modes over the shared Rust domain model.
- **A hardware-backed CLI** for reading device state, browsing presets, searching the model catalog and editing the grid.
- **An MCP server** for agentic patch editing through the held-session daemon. Read, recall, scene and unsaved live-grid editing tools are hardware-verified; save and delete are not exposed.
- **A reusable Rust core** implementing USB HID transport, framing, the protobuf envelope, session handling and a typed domain model. It depends on no host application or async runtime. The CLI and MCP server use it today; the GUI will consume the same behaviour rather than reimplementing it.

## Why this exists

Neural DSP provides Cortex Control for macOS and Windows but has not provided a Linux editor. This community project began by building that missing support for ourselves; today it is maintained by one person.

Linux is the first implementation and the only hardware-verified host today, but it is not the product boundary. The desktop GUI is intended for Linux, Windows and macOS, with the CLI and MCP server available for players and workflows that prefer those interfaces. Each platform will be called supported only after its transport, local IPC, packaging and real-hardware behaviour have been tested.

## What it is not

- **Not a replacement for Cortex Control.** It does not do cloud sync, firmware updates, or Neural Capture creation, and it is not trying to.
- **Not finished.** The CLI can save through a hardware-verified prepare/edit/commit flow and MCP can edit the unsaved live grid, but major device operations, persistent MCP writes, GUI device integration, cross-platform support and distribution remain unfinished.
- **Not a jailbreak.** It talks to the device over USB exactly as the official editor does. It does not modify firmware, root the unit, or touch the SD card, and it carries none of the warranty risk that route does.

## Status

The project's central discipline is being honest about what has actually run against real hardware. Throughout these docs:

<span class="status verified">verified</span> means exercised against a real Quad Cortex running CorOS 4.0.1 from this project's own code.

<span class="status provisional">provisional</span> means implemented but not yet verified against the applicable hardware or production integration boundary.

<span class="status planned">planned</span> means specified but not built.

| Area | Status |
| --- | --- |
| USB HID transport, framing, message envelope | <span class="status verified">verified</span> |
| Connect handshake, keepalive, correlation | <span class="status verified">verified</span> |
| Reading device version, scene, presets, folders | <span class="status verified">verified</span> |
| Model catalog | <span class="status verified">verified</span> |
| Recall preset, switch scene | <span class="status verified">verified</span> |
| Grid: place/move/remove block, set parameter | <span class="status verified">verified</span> |
| Grid: bypass, routing, per-scene values, splits | <span class="status verified">verified</span> |
| Prepared save, stored read-back and delete | <span class="status verified">verified</span> |
| Capture and IR export/import | <span class="status provisional">investigation; no working export/import</span> |
| MCP server read, recall, scene management and live-grid tools | <span class="status verified">verified; no save/delete tools</span> |
| Desktop GUI shell | <span class="status provisional">daemon read boundary verified on Linux; native reconnect UI smoke pending</span> |
| Windows and macOS hardware paths and packaging | <span class="status planned">planned; Linux is the only verified host</span> |
| Nano Cortex | <span class="status provisional">unverified target; transport compatibility unestablished</span> |

## Start here

- **[Install](install.md)** - the udev rule, building, and your first command.
- **[GUI first draft](gui/first-draft.md)** - the current desktop shell and its verification boundary.
- **[Walkthrough](walkthrough.md)** - a tour of the CLI with hardware-captured output shapes and fictionalised identifiers/preset names.
- **[CLI reference](cli-reference.md)** - every command.
- **[The protocol](protocol.md)** - what we know about the wire, and how we know it.

## A note on how this was built

Almost everything here rests on [`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex), an MIT-licensed Python library that established the protocol against real hardware and recovered the protobuf schema. This project is a Rust port of that work with attribution, not an independent rediscovery. See [Legal and attribution](legal.md).
