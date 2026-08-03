# cortex-rs

An unofficial, Linux-first toolkit for the Neural DSP **Quad Cortex** (and, in
time, the **Nano Cortex**) over the Cortex Control USB HID protocol. The core
deliverable is a low-level Rust crate (`cortex-rs`) that speaks the protocol
and exposes a typed domain model - presets, scenes, grid, blocks. On top of
the crate sit a CLI (`cortex`), an MCP server (`cortex-mcp`) for agentic
patch editing, and a planned Tauri desktop GUI.

> **Unofficial.** This project is not affiliated with, endorsed by, or
> sponsored by Neural DSP Technologies. "Neural DSP", "Quad Cortex", "Nano
> Cortex", and "Cortex Control" are trademarks of Neural DSP Technologies.
> All trademarks are the property of their respective owners. The project
> reverse-engineers the Cortex Control USB HID protocol for interoperability,
> which is the established case under UK CDPA s50B / s296A and EU Software
> Directive Article 6.

## Status

Pre-alpha scaffold. The transport, framing, and message-envelope modules are
in place; the protobuf schema is vendored and built via `prost`; the
`cortex device version` command and the GUI/MCP tool surfaces are stubbed. See
`AGENTS.md` for the roadmap and protocol invariants.

## What it is

- `cortex-rs` - a leaf Rust crate: USB HID transport, the Cortex Control
  framing/reassembly, the trailer-tagged protobuf envelope, and a typed
  domain model. Designed to drive the CLI, the MCP server, the Tauri
  backend, and any third-party consumer via crates.io.
- `cortex-cli` - a thin CLI surface over the crate.
- `cortex-mcp` - an MCP server for agentic patch editing. The safety surface
  (gated saves, factory-setlist protection, slot backups, the row-numbering
  trap) is designed in from the start; see `AGENTS.md`.
- `gui/` (planned) - a Tauri 2 desktop app, a consumer of the crate.

## What it is not

- Not a Neural DSP product, and not affiliated with Neural DSP.
- Not a re-distribution of Neural DSP binaries, firmware, or artwork.
- Not a device-rooting tool. It uses the USB HID route exclusively; the
  device-rooting route (OpenCortex) carries warranty risk that the USB route
  does not.

## Setup

### 1. udev rule (Linux)

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

```sh
cargo build              # default: includes the hidapi transport
cargo build --no-default-features   # leaf protocol/domain surface only
```

### 3. Run

```sh
cargo run -p cortex-cli -- version
```

## Project layout

```text
crates/
  cortex-rs/    The leaf crate (USB HID transport, framing, message envelope,
                typed domain model, vendored protobuf schema).
  cortex-cli/   The `cortex` CLI - a thin surface over the crate.
  cortex-mcp/   The `cortex-mcp` MCP server (scaffold; safety surface designed
                in, see AGENTS.md).
gui/           Planned: Tauri 2 desktop app (React + Mantine + Vite).
docs/          Protocol notes, runbooks, GUI docs.
spec/          AFX-style spec/design/tasks per zone.
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

The project's own work is not to be used in weaponry, immigration enforcement,
or other activities which infringe human rights.