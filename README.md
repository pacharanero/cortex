# cortex

An unofficial toolkit for the Neural DSP **Quad Cortex**, built around a cross-platform Tauri desktop GUI target in active development, a hardware-backed `cortex` CLI, a `cortex-mcp` server for agentic patch editing, and the reusable `cortex-rs` Rust crate beneath them. The CLI, MCP server and read-only GUI share the crate's typed preset/grid model, block views, scene metadata and active-scene state over the Cortex Control USB HID protocol.

**Nano Cortex** support is planned, but its transport compatibility is not yet established.

> **Unofficial.** This project is not affiliated with, endorsed by, or
> sponsored by Neural DSP Technologies. "Neural DSP", "Quad Cortex", "Nano
> Cortex", and "Cortex Control" are trademarks of Neural DSP Technologies.
> All trademarks are the property of their respective owners. The project
> reverse-engineers the Cortex Control USB HID protocol for interoperability,
> which is the established case under UK CDPA s50B / s296A and EU Software
> Directive Article 6.

## Status

**Pre-alpha and actively changing.** The Quad Cortex core and CLI are usable on Linux and passed a 42-check hardware smoke against CorOS 4.0.1, including live state, grid editing, prepared save, same-setlist preset move/restore, recall and delete. This is not a finished editor: much of the wider device API remains unimplemented, some reconnect and file-operation edge-case coverage remains tracked, and releases are not yet distributed.

The MCP server exposes hardware-verified read, recall, scene switching/metadata/copy and unsaved live-grid editing tools through the held-session daemon; it deliberately exposes no save or delete tool. Source installation installs both binaries and the [agent setup guide](https://pacharanero.github.io/cortex/agent-setup/) covers Claude Code and generic stdio harnesses. The Tauri GUI has an interactive read-only first draft with explicit fixture and daemon-backed modes; its production boundary reads status, grid, scene, CPU and populated preset slots through the same held daemon. Physical unplug/reconnect hides stale state and restores a fresh generation in the native Linux window. The desktop target is Linux, Windows and macOS; Linux is the only implemented and hardware-verified host today. The hardware-faithful panel is next, while prebuilt Linux binaries are the parallel distribution milestone. Run `s/progress` for counted progress and read `spec/roadmap.md` for the outstanding backlog.

## What it is

- `gui/` - the Tauri 2 + React + Mantine desktop editor, targeting Linux, Windows and macOS. Its first draft is an interactive read-only shell with explicit fixture and daemon-backed modes; write interactions, automated native DOM/IPC checks and cross-platform packaging remain outstanding.
- `cortex-cli` - a thin CLI over the crate, including a persistent daemon that
  owns the one device connection and reconnects without serving stale state.
- `cortex-mcp` - an MCP server for agentic patch editing through `cortex session`. Its read, recall, scene and working-copy tools are hardware-verified; persistent writes remain deliberately unavailable.
- `cortex-rs` - a leaf Rust crate providing the USB HID transport, Cortex Control framing and protobuf envelope, session/correlation, typed domain and client APIs, subscribed state reduction, and shared prepared-save safety.
- `cortex-host` - the shared synchronous daemon contract and local IPC facade used by host surfaces; it has no HID feature and cannot open the device. Unix sockets are the current adapter, with Windows named pipes planned behind the same API.

## Why the community is building it

Neural DSP provides Cortex Control for macOS and Windows but has not provided a Linux editor. This project began because Linux players still need full access to hardware they own, so the community is building that support for itself. Today that community effort is maintained by one person.

Linux is therefore the project's origin, its first implementation and its only hardware-verified host so far, not the intended boundary of the product. The shared Rust core, local IPC seam and Tauri frontend are being built toward a GUI that runs on Linux, Windows and macOS, alongside the CLI and MCP interfaces.

## What it is not

- Not a Neural DSP product, and not affiliated with Neural DSP.
- Not a re-distribution of Neural DSP binaries, firmware, or artwork.
- Not a device-rooting tool. It uses the USB HID route exclusively; the
  device-rooting route (OpenCortex) carries warranty risk that the USB route
  does not.

## Current Developer Setup

The working hardware path is currently Linux. Windows and macOS setup will be documented once those paths are implemented and tested.

### 1. udev rule

The Quad Cortex presents as USB `152a:880a` on HID interface 5. By default
`/dev/hidraw*` is root-only, so install a udev rule granting the locally
logged-in user access:

```sh
echo 'KERNEL=="hidraw*", ATTRS{idVendor}=="152a", ATTRS{idProduct}=="880a", MODE="0660", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/70-quadcortex.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=hidraw
```

Re-plug the Quad Cortex, then `ls -l /dev/hidraw*` should show `crw-rw----+`
on the interface-5 node.

### 2. Build

Install the native prerequisites in the [installation guide](https://pacharanero.github.io/cortex/install/), then build the surface you need:

```sh
cargo build -p cortex-cli
cargo build -p cortex-rs --no-default-features   # protocol/domain crate without hidapi
```

### 3. Run

```sh
cargo run -p cortex-cli -- device version
```

## Project layout

```text
crates/
  cortex-rs/    The leaf crate (transport, framing, session, client, live state,
                save safety, typed domain model, vendored protobuf schema).
  cortex-host/  Shared held-session daemon contract and local IPC facade.
  cortex-cli/   The `cortex` CLI - a thin surface over the crate.
  cortex-mcp/   Non-persistent MCP read, recall, scene and live-grid tools.
gui/           Cross-platform Tauri 2 + React + Mantine desktop editor
               (read-only fixture and daemon-backed modes).
docs/          Protocol notes, runbooks, GUI docs.
spec/          Living spec/design per zone; roadmap and completed work ledgers.
s/             Repo scripts: s/test, s/lint, s/gui-dev, s/version++ ...
```

## Prior art and attribution

This project builds on the protocol work established by the MIT-licensed
[`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex)
project, which recovered the Cortex Control protobuf schema and documented the
USB HID framing, the trailer-tagged message envelope, and the benign write-STALL
gotcha. The recovered `.proto` files are vendored into
`crates/cortex-rs/proto/` under the MIT license's distribution terms.

The Tauri app architecture follows the precedent set by the Apache-2.0-licensed
[`rixrix/deskop-nano-cortex`](https://github.com/rixrix/deskop-nano-cortex)
project (Rust device I/O backend, honest verified-vs-provisional labelling, AFX
spec layout).

Full attribution and license texts are in `NOTICE` and
`THIRD-PARTY-NOTICES.md`.

## Licensing

- **Code:** GNU Affero General Public License v3.0 or later
  ([AGPL-3.0-or-later](LICENSE)). The work is not available for subsumption
  into proprietary products; dual licensing is available on request.
- **Written content:** Creative Commons Attribution-ShareAlike 4.0
  International (CC-BY-SA-4.0).
- **Vendored schema:** The recovered Cortex Control `.proto` files remain
  under the MIT license (copyright (c) 2026 Stokes); attribution is recorded
  in `NOTICE` and `THIRD-PARTY-NOTICES.md`.

As a non-binding ethical request, the project's own work should not be used in weaponry, immigration enforcement, or other activities that infringe human rights. This request is not an additional licence condition; the AGPL and CC-BY-SA grants above are the enforceable terms.
